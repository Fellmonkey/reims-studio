//! Core contracts and runtime primitives for Reims Studio.
//!
//! The launcher treats every on-disk file as a versioned contract. The engine
//! bundle supplies `manifest.json`; the release supplies `index.json` and
//! `signatures.json`; the user supplies `settings.toml` and `vm.toml`. Rust
//! code validates these files before use and never hardcodes firmware,
//! kext or device property names.
//!
//! # Quick start
//!
//! ```no_run
//! use reims_core::contracts::{load_json, EngineManifest};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let manifest: EngineManifest = load_json(std::path::Path::new("manifest.json"))?;
//! println!("engine {} uses ABI {}", manifest.engine_id, manifest.abi_version);
//! # Ok(())
//! # }
//! ```

pub mod contracts;
pub mod crash;
pub mod lock;
pub mod log;
pub mod qmp;
