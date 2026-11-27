//! Portable folder layout helpers.
//!
//! Every path is relative to a `portable.flag` folder so the same tree can
//! move between Windows and Linux without rewriting configuration.

use std::path::{Path, PathBuf};

use crate::manager::EngineError;

/// Returns the folder of one engine bundle.
pub fn engine_dir(portable_dir: &Path, engine_id: &str) -> PathBuf {
    portable_dir.join("engines").join(engine_id)
}

/// Returns the folder of one VM.
pub fn vm_dir(portable_dir: &Path, name: &str) -> PathBuf {
    portable_dir.join("vms").join(name)
}

/// Returns the installer cache folder.
pub fn cache_dir(portable_dir: &Path) -> PathBuf {
    portable_dir.join("cache").join("installers")
}

/// Returns the shader cache folder of one GPU driver.
pub fn shader_cache_dir(
    portable_dir: &Path,
    os: &str,
    vid: &str,
    pid: &str,
    driver: &str,
) -> PathBuf {
    portable_dir
        .join("cache")
        .join("shaders")
        .join(format!("{os}-{vid}-{pid}-{driver}"))
}

/// Creates the top-level portable folders.
///
/// # Errors
///
/// Returns [`EngineError::Io`] when a folder cannot be created.
pub fn ensure_layout(portable_dir: &Path) -> Result<(), EngineError> {
    for dir in [
        portable_dir.join("engines"),
        portable_dir.join("vms"),
        cache_dir(portable_dir),
        portable_dir.join("cache").join("shaders"),
    ] {
        std::fs::create_dir_all(&dir).map_err(|source| EngineError::Io {
            path: dir.display().to_string(),
            source,
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn layout_is_relative_to_the_portable_root() {
        let root = Path::new("ReimsStudio");
        assert_eq!(
            engine_dir(root, "linux64-master-a"),
            root.join("engines/linux64-master-a")
        );
        assert_eq!(vm_dir(root, "macos-tahoe"), root.join("vms/macos-tahoe"));
        assert_eq!(cache_dir(root), root.join("cache/installers"));
        assert_eq!(
            shader_cache_dir(root, "linux64", "1002", "73bf", "mesa"),
            root.join("cache/shaders/linux64-1002-73bf-mesa")
        );
    }

    #[test]
    fn ensure_layout_creates_all_folders() {
        let dir = tempfile::tempdir().expect("temp dir");
        ensure_layout(dir.path()).expect("layout");
        assert!(dir.path().join("engines").is_dir());
        assert!(dir.path().join("vms").is_dir());
        assert!(dir.path().join("cache/installers").is_dir());
        assert!(dir.path().join("cache/shaders").is_dir());
    }
}
