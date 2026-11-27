//! Hybrid `vm.lock` based on an OS advisory lock plus an owner record.
//!
//! # Quick start
//!
//! ```no_run
//! use std::path::Path;
//!
//! use reims_core::lock::VmLock;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let lock = VmLock::acquire(Path::new("vms/macos-tahoe"), "host-fingerprint")?;
//! println!("locked by {}", lock.record().pid);
//! # Ok(())
//! # }
//! ```

use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sysinfo::{Pid, Process, ProcessesToUpdate, System};
use thiserror::Error;

/// Name of the lock file inside a VM folder.
pub const LOCK_FILE: &str = "vm.lock";
/// Name of the unlocked owner mirror used to report live lock holders.
pub const OWNER_FILE: &str = "vm.owner.json";

/// Owner record written into `vm.lock`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct LockRecord {
    /// PID of the process that owns the VM.
    pub pid: u32,
    /// Start time of that process, in seconds since the Unix epoch.
    pub start_time: u64,
    /// Fingerprint of the host that started the VM.
    pub host_id: String,
    /// Creation time of the record, in seconds since the Unix epoch.
    pub created_at: u64,
}

impl LockRecord {
    /// Builds a record for the current process.
    pub fn new(host_id: impl Into<String>) -> Self {
        Self {
            pid: std::process::id(),
            start_time: process_start_time(std::process::id()),
            host_id: host_id.into(),
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    /// Returns true when the recorded owner can no longer hold the lock.
    ///
    /// A record is stale on a different host, with a missing process, or with
    /// a process whose start time does not match (PID reuse).
    pub fn is_stale(&self, host_id: &str) -> bool {
        if self.host_id != host_id || self.start_time == 0 {
            return true;
        }
        let pid = Pid::from_u32(self.pid);
        let Ok(mut system) = System::new_all() else {
            return true;
        };
        system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
        system
            .process(pid)
            .is_none_or(|process| process.start_time() != self.start_time)
    }
}

fn read_owner(path: &Path) -> Option<LockRecord> {
    let file = std::fs::File::open(path).ok()?;
    serde_json::from_reader(file).ok()
}

fn write_owner(path: &Path, record: &LockRecord) -> Result<(), LockError> {
    let file = std::fs::File::create(path).map_err(|source| LockError::Io {
        path: path.display().to_string(),
        source,
    })?;
    serde_json::to_writer(&file, record).map_err(|source| LockError::Corrupt { source })?;
    file.sync_all().map_err(|source| LockError::Io {
        path: path.display().to_string(),
        source,
    })
}

fn process_start_time(pid: u32) -> u64 {
    let pid = Pid::from_u32(pid);
    let Ok(mut system) = System::new_all() else {
        return 0;
    };
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    system.process(pid).map_or(0, Process::start_time)
}

/// Failures while acquiring a VM lock.
#[derive(Debug, Error)]
pub enum LockError {
    /// The lock file could not be opened or updated.
    #[error("cannot access {path}: {source}")]
    Io {
        /// Path that failed.
        path: String,
        /// Underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
    /// Another live process owns the lock.
    #[error("vm.lock is held by pid {}", .0.pid)]
    Held(LockRecord),
    /// The lock file is not a valid owner record.
    #[error("vm.lock is corrupt: {source}")]
    Corrupt {
        /// JSON failure.
        #[source]
        source: serde_json::Error,
    },
}

/// Exclusive guard for a VM folder.
///
/// The guard owns the advisory lock until it is dropped. Dropping removes the
/// `vm.lock` file together with the stale owner record.
#[derive(Debug)]
pub struct VmLock {
    file: File,
    path: PathBuf,
    owner_path: PathBuf,
    record: LockRecord,
}

impl VmLock {
    /// Acquires the lock for `vm_dir` using the current host fingerprint.
    ///
    /// # Errors
    ///
    /// Returns [`LockError::Held`] when another live process owns the VM.
    pub fn acquire(vm_dir: &Path, host_id: impl Into<String>) -> Result<Self, LockError> {
        let host_id = host_id.into();
        std::fs::create_dir_all(vm_dir).map_err(|source| LockError::Io {
            path: vm_dir.display().to_string(),
            source,
        })?;
        let path = vm_dir.join(LOCK_FILE);
        let owner_path = vm_dir.join(OWNER_FILE);
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|source| LockError::Io {
                path: path.display().to_string(),
                source,
            })?;
        // Windows reports ERROR_LOCK_VIOLATION instead of WouldBlock and also
        // blocks reads of the locked region. The unlocked owner mirror gives
        // a typed holder even while another process holds the lock.
        if let Err(source) = file.try_lock_exclusive() {
            return match read_owner(&owner_path) {
                Some(record) if !record.is_stale(&host_id) => Err(LockError::Held(record)),
                _ => Err(LockError::Io {
                    path: path.display().to_string(),
                    source,
                }),
            };
        }

        file.seek(SeekFrom::Start(0))
            .map_err(|source| LockError::Io {
                path: path.display().to_string(),
                source,
            })?;
        let existing: Option<LockRecord> = match serde_json::from_reader(&file) {
            Ok(record) => Some(record),
            Err(error) if error.is_eof() => None,
            Err(source) => return Err(LockError::Corrupt { source }),
        };
        if let Some(record) = existing
            && !record.is_stale(&host_id)
        {
            return Err(LockError::Held(record));
        }

        let record = LockRecord::new(host_id);
        file.set_len(0).map_err(|source| LockError::Io {
            path: path.display().to_string(),
            source,
        })?;
        file.seek(SeekFrom::Start(0))
            .map_err(|source| LockError::Io {
                path: path.display().to_string(),
                source,
            })?;
        serde_json::to_writer(&file, &record).map_err(|source| LockError::Corrupt { source })?;
        file.sync_all().map_err(|source| LockError::Io {
            path: path.display().to_string(),
            source,
        })?;
        write_owner(&owner_path, &record)?;
        Ok(Self {
            file,
            path,
            owner_path,
            record,
        })
    }

    /// Returns the owner record written by this guard.
    pub fn record(&self) -> &LockRecord {
        &self.record
    }

    /// Returns the path of the lock file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the path of the unlocked owner mirror.
    pub fn owner_path(&self) -> &Path {
        &self.owner_path
    }
}

impl Drop for VmLock {
    fn drop(&mut self) {
        // The advisory lock is still held here, so no other owner can replace
        // this file between the removal and the unlock below.
        let _ = std::fs::remove_file(&self.path);
        // Unlock errors cannot be reported from Drop; closing the handle also
        // releases the OS advisory lock.
        let _ = self.file.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_writes_a_live_owner_and_releases_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let vm_dir = dir.path().join("macos-tahoe");
        {
            let lock = VmLock::acquire(&vm_dir, "test-host").expect("acquire");
            let record = lock.record();
            assert_eq!(record.pid, std::process::id());
            assert!(!record.is_stale("test-host"));
            assert!(vm_dir.join(LOCK_FILE).exists());
            assert!(vm_dir.join(OWNER_FILE).exists());
        }
        assert!(!vm_dir.join(LOCK_FILE).exists());
        VmLock::acquire(&vm_dir, "test-host").expect("reacquire after drop");
    }

    #[test]
    fn second_acquire_reports_the_live_owner() {
        let dir = tempfile::tempdir().expect("temp dir");
        let _first = VmLock::acquire(dir.path(), "test-host").expect("first acquire");
        match VmLock::acquire(dir.path(), "test-host") {
            Err(LockError::Held(record)) => assert_eq!(record.pid, std::process::id()),
            other => panic!("expected a held lock, got {other:?}"),
        }
    }

    #[test]
    fn records_from_other_hosts_and_dead_pids_are_stale() {
        let record = LockRecord::new("host-a");
        assert!(record.is_stale("host-b"));
        let dead = LockRecord {
            pid: 4_000_000_000,
            ..record
        };
        assert!(dead.is_stale("host-a"));
    }
}
