//! Thin disk creation and guarded `Compact QCOW2`.

use std::path::{Path, PathBuf};

use crate::manager::EngineError;

/// A rendered external command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalCommand {
    /// Program to execute.
    pub program: PathBuf,
    /// Ordered arguments.
    pub args: Vec<String>,
}

/// Returns the command that creates a thin qcow2 disk.
pub fn create_disk_command(qemu_img: &Path, disk: &Path, size_gib: u64) -> ExternalCommand {
    ExternalCommand {
        program: qemu_img.to_path_buf(),
        args: vec![
            String::from("create"),
            String::from("-f"),
            String::from("qcow2"),
            disk.display().to_string(),
            format!("{size_gib}G"),
        ],
    }
}

/// Returns the command that compacts a qcow2 disk in place.
pub fn compact_command(qemu_img: &Path, disk: &Path) -> ExternalCommand {
    let mut temporary = disk.to_path_buf();
    temporary.set_extension("compact.qcow2");
    ExternalCommand {
        program: qemu_img.to_path_buf(),
        args: vec![
            String::from("compact"),
            String::from("-o"),
            temporary.display().to_string(),
            disk.display().to_string(),
        ],
    }
}

/// Returns true when compaction has at least 1.1x the disk size free.
///
/// Integer math avoids rounding a boundary case in favour of compaction.
pub fn can_compact(disk_bytes: u64, available_bytes: u64) -> bool {
    u128::from(available_bytes) >= u128::from(disk_bytes) * 11 / 10
}

/// Executes a rendered command, checking its exit status.
///
/// # Errors
///
/// Returns [`EngineError::Io`] when the program cannot run or exits non-zero.
pub fn run(command: ExternalCommand) -> Result<(), EngineError> {
    let status = std::process::Command::new(&command.program)
        .args(&command.args)
        .status()
        .map_err(|source| EngineError::Io {
            path: command.program.display().to_string(),
            source,
        })?;
    if !status.success() {
        return Err(EngineError::InvalidArchive(format!(
            "{} exited with {status}",
            command.program.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn builds_the_thin_disk_command() {
        let command = create_disk_command(
            Path::new("../../engines/test/bin/qemu-img"),
            Path::new("disk.qcow2"),
            128,
        );
        assert_eq!(
            command.args,
            ["create", "-f", "qcow2", "disk.qcow2", "128G"]
        );
    }

    #[test]
    fn builds_the_compact_command() {
        let command = compact_command(
            Path::new("qemu-img"),
            Path::new("vms/macos-tahoe/disk.qcow2"),
        );
        assert_eq!(command.args[0], "compact");
        assert_eq!(command.args[2], "vms/macos-tahoe/disk.compact.qcow2");
    }

    #[test]
    fn compaction_requires_headroom() {
        assert!(can_compact(100, 110));
        assert!(!can_compact(100, 109));
        assert!(!can_compact(u64::MAX, u64::MAX));
    }
}
