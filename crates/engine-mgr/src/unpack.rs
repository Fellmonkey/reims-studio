//! Safe extraction of CI-built `.tar.zst` engine archives.

use std::path::{Component, Path};

use tar::Archive;
use zstd::stream::read::Decoder;

use crate::manager::EngineError;

fn validate_entry_path(path: &Path) -> Result<(), EngineError> {
    let unsafe_path = path.as_os_str().is_empty()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        });
    if unsafe_path {
        return Err(EngineError::UnsafePath(path.display().to_string()));
    }
    Ok(())
}

/// Unpacks a verified engine archive into `destination`.
///
/// Every entry must be a relative path without `..`, a root or a Windows
/// prefix. The caller must call [`crate::verify_archive`] first.
///
/// # Errors
///
/// Returns [`EngineError::InvalidArchive`] for a broken stream and
/// [`EngineError::UnsafePath`] for path traversal attempts.
pub fn unpack_tar_zst(archive: &Path, destination: &Path) -> Result<(), EngineError> {
    let file = std::fs::File::open(archive).map_err(|source| EngineError::Io {
        path: archive.display().to_string(),
        source,
    })?;
    let decoder =
        Decoder::new(file).map_err(|source| EngineError::InvalidArchive(source.to_string()))?;
    std::fs::create_dir_all(destination).map_err(|source| EngineError::Io {
        path: destination.display().to_string(),
        source,
    })?;
    let mut tar = Archive::new(decoder);
    let entries = tar
        .entries()
        .map_err(|source| EngineError::InvalidArchive(source.to_string()))?;
    for entry in entries {
        let mut entry = entry.map_err(|source| EngineError::InvalidArchive(source.to_string()))?;
        let path = entry
            .path()
            .map_err(|source| EngineError::InvalidArchive(source.to_string()))?;
        validate_entry_path(&path)?;
        entry
            .unpack_in(destination)
            .map_err(|source| EngineError::Io {
                path: destination.display().to_string(),
                source,
            })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    fn write_archive(
        path: &Path,
        entries: &[(&str, &[u8])],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let file = std::fs::File::create(path)?;
        let encoder = zstd::stream::write::Encoder::new(file, 1)?;
        let mut tar = tar::Builder::new(encoder);
        for (name, content) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, name, *content)?;
        }
        let encoder = tar.into_inner()?;
        encoder.finish()?;
        Ok(())
    }

    #[test]
    fn extracts_a_valid_archive() {
        let dir = tempfile::tempdir().expect("temp dir");
        let archive = dir.path().join("engine.tar.zst");
        write_archive(&archive, &[("firmware/OVMF_CODE.fd", b"firmware data")]).expect("archive");
        let destination = dir.path().join("engines");
        unpack_tar_zst(&archive, &destination).expect("unpack");
        let content = std::fs::read(destination.join("firmware/OVMF_CODE.fd")).expect("content");
        assert_eq!(content, b"firmware data");
    }

    #[test]
    fn rejects_path_traversal() {
        let dir = tempfile::tempdir().expect("temp dir");
        let archive = dir.path().join("evil.tar.zst");
        let file = std::fs::File::create(&archive).expect("create archive");
        let encoder = zstd::stream::write::Encoder::new(file, 1).expect("encoder");
        let mut tar = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        let raw = header.as_gnu_mut().expect("gnu header");
        raw.name.fill(0);
        raw.name[..b"../escape.txt".len()].copy_from_slice(b"../escape.txt");
        header.set_size(7);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append(&header, b"escaped".as_slice())
            .expect("append entry");
        let encoder = tar.into_inner().expect("finish tar");
        encoder.finish().expect("finish zstd");

        let destination = dir.path().join("engines");
        assert!(matches!(
            unpack_tar_zst(&archive, &destination),
            Err(EngineError::UnsafePath(_))
        ));
        assert!(!dir.path().join("escape.txt").exists());
    }

    #[test]
    fn rejects_a_non_zstd_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let archive = dir.path().join("broken.tar.zst");
        let mut file = std::fs::File::create(&archive).expect("create");
        file.write_all(b"not zstd").expect("write");
        drop(file);
        assert!(matches!(
            unpack_tar_zst(&archive, dir.path()),
            Err(EngineError::InvalidArchive(_))
        ));
    }
}
