//! Headless checks used by the local e2e harness and release automation.
//!
//! The GUI needs a display; `--check` and `--e2e` let CI verify that the
//! launcher sees a built engine without opening a window.

use std::fmt;
use std::path::Path;

use reims_core::contracts::{
    EngineIndex, EngineManifest, HostOs, Settings as StudioSettings, Validate, VmConfig, load_json,
    load_toml,
};
use reims_engine_mgr::{check_ovmf_pair, engine_dir, select_release, verify_archive, vm_dir};
use reims_platform::adapter::{EngineAdapter, VmSpec};

/// Result of a headless launcher check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    /// Selected engine folder name.
    pub engine_id: String,
    /// Declared ABI version.
    pub abi_version: u32,
    /// Engine channel from `settings.toml`.
    pub channel: String,
    /// Rendered QEMU binary path.
    pub program: String,
    /// Number of rendered QEMU arguments.
    pub argument_count: usize,
    /// VM working directory.
    pub working_dir: String,
    /// Guest bridge port from settings.
    pub bridge_port: u16,
}

impl fmt::Display for CheckReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "engine: {}", self.engine_id)?;
        writeln!(formatter, "abi: {}", self.abi_version)?;
        writeln!(formatter, "channel: {}", self.channel)?;
        writeln!(formatter, "qemu: {}", self.program)?;
        writeln!(formatter, "arguments: {}", self.argument_count)?;
        writeln!(formatter, "working_dir: {}", self.working_dir)?;
        write!(formatter, "bridge_port: {}", self.bridge_port)
    }
}

/// Verifies that `portable_root` contains a usable engine and VM.
///
/// # Errors
///
/// Returns a human-readable error for a missing or invalid contract.
pub fn check(portable_root: &Path, require_archive: bool) -> Result<CheckReport, String> {
    let settings: StudioSettings =
        load_toml(&portable_root.join("settings.toml")).map_err(|error| error.to_string())?;
    settings.validate().map_err(|error| error.to_string())?;

    let index: EngineIndex =
        load_json(&portable_root.join("index.json")).map_err(|error| error.to_string())?;
    index.validate().map_err(|error| error.to_string())?;
    let release = select_release(&index, HostOs::current(), settings.channel).ok_or_else(|| {
        format!(
            "no {} engine for {} in index.json",
            settings.channel,
            HostOs::current()
        )
    })?;

    let bundle = engine_dir(portable_root, &release.engine_id);
    let manifest: EngineManifest =
        load_json(&bundle.join("manifest.json")).map_err(|error| error.to_string())?;
    manifest.validate().map_err(|error| error.to_string())?;
    if manifest.abi_version != release.abi_version {
        return Err(String::from("manifest and index disagree on abi_version"));
    }

    if require_archive {
        let archive = portable_root
            .join("cache")
            .join(format!("{}.tar.zst", release.engine_id));
        verify_archive(&archive, &release.archive_sha256).map_err(|error| error.to_string())?;
    }

    let code = bundle.join("firmware/OVMF_CODE.fd");
    let variables = vm_dir(portable_root, "macos-tahoe").join("OVMF_VARS.fd");
    if code.exists() && variables.exists() {
        check_ovmf_pair(&code, &variables).map_err(|error| error.to_string())?;
    }

    let vm_path = vm_dir(portable_root, "macos-tahoe").join("vm.toml");
    let vm = if vm_path.exists() {
        load_toml(&vm_path).map_err(|error| error.to_string())?
    } else {
        VmConfig::new("macos-tahoe")
    };
    vm.validate().map_err(|error| error.to_string())?;

    let adapter = EngineAdapter::for_manifest(&manifest).map_err(|error| error.to_string())?;
    let spec = VmSpec {
        os: manifest.os,
        engine_id: manifest.engine_id.clone(),
        manifest,
        vm,
        installer_image: None,
        qmp_port: settings.qmp_port,
        bridge_port: settings.bridge_port,
        shader_cache_dir: String::from("../../cache/shaders/default"),
        disable_video_decode: false,
    };
    let command = adapter.build(&spec).map_err(|error| error.to_string())?;
    Ok(CheckReport {
        engine_id: spec.engine_id,
        abi_version: adapter.abi_version(),
        channel: settings.channel.to_string(),
        program: command.program,
        argument_count: command.args.len(),
        working_dir: command.working_dir,
        bridge_port: settings.bridge_port,
    })
}
