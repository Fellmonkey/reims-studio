//! Host-facing adapters for Reims Studio.
//!
//! The launcher never links against the engine. [`adapter::EngineAdapter`]
//! maps `manifest.abi_version` to a deterministic QEMU command, so old engines
//! and old VMs keep working after the upstream CLI changes.
//!
//! # Quick start
//!
//! ```no_run
//! use reims_core::contracts::{EngineManifest, HostOs, VmConfig, load_json};
//! use reims_platform::adapter::{EngineAdapter, VmSpec};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let manifest: EngineManifest = load_json(std::path::Path::new("manifest.json"))?;
//! let adapter = EngineAdapter::for_manifest(&manifest)?;
//! let spec = VmSpec {
//!     os: HostOs::Linux,
//!     engine_id: manifest.engine_id.clone(),
//!     manifest,
//!     vm: VmConfig::new("macos-tahoe"),
//!     installer_image: Some(String::from("BaseSystem.img")),
//!     qmp_port: 4444,
//!     bridge_port: 8080,
//!     shader_cache_dir: String::from("../../cache/shaders/linux64"),
//!     disable_video_decode: false,
//! };
//! let command = adapter.build(&spec)?;
//! println!("{} {}", command.program, command.args.join(" "));
//! # Ok(())
//! # }
//! ```

pub mod adapter;
pub mod preflight;
