//! Portable snapshots of the mutable part of a VM.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::manager::EngineError;

const META_FILE: &str = "meta.json";
const SNAPSHOT_FILES: [&str; 3] = ["OpenCore.qcow2", "OVMF_VARS.fd", "vm.toml"];

/// Metadata stored with every snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotMeta {
    /// Snapshot tag.
    pub tag: String,
    /// VM folder name.
    pub vm: String,
    /// Engine folder name used by the VM.
    pub engine_id: String,
    /// Creation time in seconds since the Unix epoch.
    pub created_at: u64,
}

fn is_safe_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 128
        && !tag.starts_with('.')
        && !tag
            .chars()
            .any(|character| matches!(character, '/' | '\\' | '\0'))
}

fn io_error(path: &Path, source: std::io::Error) -> EngineError {
    EngineError::Io {
        path: path.display().to_string(),
        source,
    }
}

/// Creates an offline snapshot of `vm_dir`.
///
/// # Errors
///
/// Returns [`EngineError::Io`] when a required file is missing or a copy
/// fails, and [`EngineError::UnsafePath`] for an invalid tag.
pub fn create_snapshot(vm_dir: &Path, tag: &str, engine_id: &str) -> Result<PathBuf, EngineError> {
    if !is_safe_tag(tag) {
        return Err(EngineError::UnsafePath(tag.to_owned()));
    }
    let destination = vm_dir.join("snapshots").join(tag);
    std::fs::create_dir_all(&destination).map_err(|source| io_error(&destination, source))?;
    for name in SNAPSHOT_FILES {
        let source = vm_dir.join(name);
        std::fs::copy(&source, destination.join(name))
            .map_err(|source_error| io_error(&source, source_error))?;
    }
    let meta = SnapshotMeta {
        tag: tag.to_owned(),
        vm: vm_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned(),
        engine_id: engine_id.to_owned(),
        created_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };
    let meta_path = destination.join(META_FILE);
    let file = std::fs::File::create(&meta_path).map_err(|source| io_error(&meta_path, source))?;
    serde_json::to_writer(&file, &meta)
        .map_err(|source| EngineError::InvalidArchive(source.to_string()))?;
    Ok(destination)
}

/// Restores a snapshot over the live VM files.
///
/// # Errors
///
/// Returns [`EngineError`] when the snapshot is incomplete or a copy fails.
pub fn restore_snapshot(vm_dir: &Path, tag: &str) -> Result<(), EngineError> {
    if !is_safe_tag(tag) {
        return Err(EngineError::UnsafePath(tag.to_owned()));
    }
    let source = vm_dir.join("snapshots").join(tag);
    for name in SNAPSHOT_FILES {
        let snapshot_file = source.join(name);
        std::fs::copy(&snapshot_file, vm_dir.join(name))
            .map_err(|error| io_error(&snapshot_file, error))?;
    }
    Ok(())
}

/// Lists snapshot tags that have valid metadata.
///
/// # Errors
///
/// Returns [`EngineError::Io`] when the snapshot directory cannot be read.
pub fn list_snapshots(vm_dir: &Path) -> Result<Vec<String>, EngineError> {
    let root = vm_dir.join("snapshots");
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io_error(&root, error)),
    };
    let mut tags = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| io_error(&root, error))?;
        if entry.path().join(META_FILE).is_file()
            && let Some(name) = entry.file_name().to_str()
        {
            tags.push(name.to_owned());
        }
    }
    tags.sort();
    Ok(tags)
}

/// Reads the metadata of one snapshot.
///
/// # Errors
///
/// Returns [`EngineError`] when the metadata is missing or malformed.
pub fn read_snapshot_meta(vm_dir: &Path, tag: &str) -> Result<SnapshotMeta, EngineError> {
    let path = vm_dir.join("snapshots").join(tag).join(META_FILE);
    let raw = std::fs::read_to_string(&path).map_err(|error| io_error(&path, error))?;
    serde_json::from_str(&raw).map_err(|error| EngineError::InvalidArchive(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vm_dir(dir: &tempfile::TempDir) -> PathBuf {
        let vm = dir.path().join("macos-tahoe");
        std::fs::create_dir_all(&vm).expect("vm dir");
        for name in SNAPSHOT_FILES {
            std::fs::write(vm.join(name), name.as_bytes()).expect("vm file");
        }
        vm
    }

    #[test]
    fn creates_lists_and_restores_a_snapshot() {
        let dir = tempfile::tempdir().expect("temp dir");
        let vm = vm_dir(&dir);
        let path =
            create_snapshot(&vm, "before-post-install", "linux64-master-a").expect("snapshot");
        assert!(path.join(META_FILE).exists());
        let meta = read_snapshot_meta(&vm, "before-post-install").expect("meta");
        assert_eq!(meta.engine_id, "linux64-master-a");
        assert_eq!(
            list_snapshots(&vm).expect("list"),
            vec!["before-post-install"]
        );

        std::fs::write(vm.join("vm.toml"), b"changed").expect("modify vm");
        restore_snapshot(&vm, "before-post-install").expect("restore");
        assert_eq!(std::fs::read(vm.join("vm.toml")).expect("read"), b"vm.toml");
    }

    #[test]
    fn rejects_unsafe_tags() {
        let dir = tempfile::tempdir().expect("temp dir");
        let vm = vm_dir(&dir);
        assert!(matches!(
            create_snapshot(&vm, "../escape", "engine"),
            Err(EngineError::UnsafePath(_))
        ));
    }

    #[test]
    fn missing_snapshot_directory_lists_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert!(
            list_snapshots(&dir.path().join("missing"))
                .expect("list")
                .is_empty()
        );
    }
}
