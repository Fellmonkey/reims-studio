//! Engine bundle selection, verification and portable layout helpers.
//!
//! The manager never compiles an engine. It selects a release from
//! `index.json`, verifies its SHA256 digest and unpacks a CI-built
//! `.tar.zst` archive into the portable folder.
//!
//! # Quick start
//!
//! ```no_run
//! use std::path::Path;
//!
//! use reims_core::contracts::{Channel, HostOs, load_json};
//! use reims_engine_mgr::{select_release, unpack_tar_zst, verify_archive};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let index = load_json(std::path::Path::new("index.json"))?;
//! let release = select_release(&index, HostOs::Windows, Channel::Development)
//!     .ok_or("no matching engine")?;
//! verify_archive(Path::new("engine.tar.zst"), &release.archive_sha256)?;
//! unpack_tar_zst(Path::new("engine.tar.zst"), Path::new("engines"))?;
//! # Ok(())
//! # }
//! ```

mod compact;
mod dmg;
mod manager;
mod opencore;
mod paths;
mod recovery;
mod snapshots;
mod unpack;

pub use compact::{
    ExternalCommand, can_compact, compact_command, create_disk_command, run as run_external,
};
pub use dmg::{AuditReport, audit_dmg, to_readonly_image};
pub use manager::{EngineError, select_release, sha256_file, supports_macos, verify_archive};
pub use opencore::{OcPatch, check_ovmf_pair, copy_kexts, patch_config, patched_paths};
pub use paths::{cache_dir, engine_dir, ensure_layout, shader_cache_dir, vm_dir};
pub use recovery::{
    Chunk, ChunkList, RECOVERY_URL, RecoveryRequest, SWSCAN_URL, parse_chunklist, verify_chunk,
};
pub use snapshots::{
    SnapshotMeta, create_snapshot, list_snapshots, read_snapshot_meta, restore_snapshot,
};
pub use unpack::unpack_tar_zst;
