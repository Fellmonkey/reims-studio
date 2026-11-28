//! Versioned on-disk contracts shared by the launcher and the engine manager.
//!
//! # Quick start
//!
//! ```no_run
//! use reims_core::contracts::{load_toml, Settings};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let settings: Settings = load_toml(std::path::Path::new("settings.toml"))?;
//! println!("engine channel: {}", settings.channel);
//! # Ok(())
//! # }
//! ```

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::str::FromStr;

use regex::Regex;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Schema version of `manifest.json`.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;
/// Schema version of `index.json`.
pub const INDEX_SCHEMA_VERSION: u32 = 1;
/// Schema version of `settings.toml`.
pub const SETTINGS_SCHEMA_VERSION: u32 = 1;
/// Schema version of `vm.toml`.
pub const VM_SCHEMA_VERSION: u32 = 1;
/// Schema version of `smbios.toml`.
pub const SMBIOS_SCHEMA_VERSION: u32 = 1;
/// Schema version of `host_profiles/<fingerprint>.toml`.
pub const HOST_PROFILE_SCHEMA_VERSION: u32 = 1;
/// Schema version of `signatures.json`.
pub const SIGNATURES_SCHEMA_VERSION: u32 = 1;

/// Failures while reading, parsing or validating a contract file.
#[derive(Debug, Error)]
pub enum ContractError {
    /// The file could not be read.
    #[error("cannot read {path}: {source}")]
    Io {
        /// Path that failed.
        path: String,
        /// Underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
    /// The file is not valid JSON.
    #[error("invalid JSON in {path}: {source}")]
    Json {
        /// Path that failed.
        path: String,
        /// Underlying parse failure.
        #[source]
        source: serde_json::Error,
    },
    /// The file is not valid TOML.
    #[error("invalid TOML in {path}: {source}")]
    Toml {
        /// Path that failed.
        path: String,
        /// Underlying parse failure.
        #[source]
        source: toml::de::Error,
    },
    /// A field has an invalid value.
    #[error("{field}: {reason}")]
    Invalid {
        /// Name of the offending field.
        field: &'static str,
        /// Why the value was rejected.
        reason: String,
    },
    /// A value that must be unique occurs twice.
    #[error("duplicate {field}: {value}")]
    Duplicate {
        /// Name of the field.
        field: &'static str,
        /// Offending value.
        value: String,
    },
    /// A crash signature contains an invalid regular expression.
    #[error("signature {id}: invalid pattern: {source}")]
    Pattern {
        /// Signature identifier.
        id: String,
        /// Regex failure.
        #[source]
        source: regex::Error,
    },
}

impl ContractError {
    fn invalid(field: &'static str, reason: impl Into<String>) -> Self {
        Self::Invalid {
            field,
            reason: reason.into(),
        }
    }
}

/// A contract that validates its own invariants after deserialization.
pub trait Validate {
    /// Checks the invariants that serde cannot express.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError`] when a field violates the contract.
    fn validate(&self) -> Result<(), ContractError> {
        Ok(())
    }
}

/// Reads and validates a JSON contract file.
///
/// # Errors
///
/// Returns [`ContractError`] on I/O, JSON or validation failure.
pub fn load_json<T: DeserializeOwned + Validate>(path: &Path) -> Result<T, ContractError> {
    let raw = std::fs::read_to_string(path).map_err(|source| ContractError::Io {
        path: path.display().to_string(),
        source,
    })?;
    parse_json(&raw, path)
}

/// Parses and validates a JSON document.
///
/// # Errors
///
/// Returns [`ContractError`] on JSON or validation failure.
pub fn parse_json<T: DeserializeOwned + Validate>(
    raw: &str,
    path: &Path,
) -> Result<T, ContractError> {
    let value: T = serde_json::from_str(raw).map_err(|source| ContractError::Json {
        path: path.display().to_string(),
        source,
    })?;
    value.validate()?;
    Ok(value)
}

/// Reads and validates a TOML contract file.
///
/// # Errors
///
/// Returns [`ContractError`] on I/O, TOML or validation failure.
pub fn load_toml<T: DeserializeOwned + Validate>(path: &Path) -> Result<T, ContractError> {
    let raw = std::fs::read_to_string(path).map_err(|source| ContractError::Io {
        path: path.display().to_string(),
        source,
    })?;
    parse_toml(&raw, path)
}

/// Parses and validates a TOML document.
///
/// # Errors
///
/// Returns [`ContractError`] on TOML or validation failure.
pub fn parse_toml<T: DeserializeOwned + Validate>(
    raw: &str,
    path: &Path,
) -> Result<T, ContractError> {
    let value: T = toml::from_str(raw).map_err(|source| ContractError::Toml {
        path: path.display().to_string(),
        source,
    })?;
    value.validate()?;
    Ok(value)
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_hex_id(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_safe_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.starts_with('.')
        && !value.chars().any(|c| matches!(c, '/' | '\\' | '\0'))
}

fn is_safe_relative_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.starts_with('\\')
        && !value.contains('\0')
        && !value
            .split(['/', '\\'])
            .any(|part| part.is_empty() || part == "..")
}

/// Returns true when `value` is a colon-separated six-byte MAC address.
pub fn is_mac_address(value: &str) -> bool {
    let parts: Vec<&str> = value.split(':').collect();
    parts.len() == 6 && parts.iter().all(|part| is_hex_id(part, 2))
}

fn check_schema(value: u32, expected: u32, field: &'static str) -> Result<(), ContractError> {
    if value == expected {
        Ok(())
    } else {
        Err(ContractError::invalid(
            field,
            format!("unsupported version {value}, expected {expected}"),
        ))
    }
}

/// Supported host operating systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HostOs {
    /// Windows 10/11 x64.
    Windows,
    /// Linux x86-64.
    Linux,
}

impl HostOs {
    /// Returns the host this binary was built for.
    pub fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

impl fmt::Display for HostOs {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Windows => formatter.write_str("windows"),
            Self::Linux => formatter.write_str("linux"),
        }
    }
}

impl FromStr for HostOs {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "windows" => Ok(Self::Windows),
            "linux" => Ok(Self::Linux),
            other => Err(format!("unknown host OS: {other}")),
        }
    }
}

/// Engine release channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    /// Stable upstream revision, the default on Linux.
    Master,
    /// Latest WHPX and iGPU fixes, the default on Windows.
    Development,
    /// Reviewed local patches applied with `git apply --check` in CI.
    Patched,
}

impl Channel {
    /// Returns the default channel for a host.
    pub fn for_host(os: HostOs) -> Self {
        match os {
            HostOs::Windows => Self::Development,
            HostOs::Linux => Self::Master,
        }
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Master => formatter.write_str("master"),
            Self::Development => formatter.write_str("development"),
            Self::Patched => formatter.write_str("patched"),
        }
    }
}

impl FromStr for Channel {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "master" => Ok(Self::Master),
            "development" => Ok(Self::Development),
            "patched" => Ok(Self::Patched),
            other => Err(format!("unknown channel: {other}")),
        }
    }
}

/// Contract of one immutable engine bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct EngineManifest {
    /// Schema version of this file.
    pub schema_version: u32,
    /// Version of the `manifest.abi_version -> QEMU CLI args` mapper.
    pub abi_version: u32,
    /// Folder name of the engine, for example `win64-development-<sha>`.
    pub engine_id: String,
    /// Host the bundle was built for.
    pub os: HostOs,
    /// Release channel of the bundle.
    pub channel: Channel,
    /// Pinned `reims-vgpu` revision.
    pub source_revision: String,
    /// Pinned `qemu-reims-vgpu` revision.
    pub qemu_revision: String,
    /// Option ROM filename inside `firmware/`, discovered by CI.
    pub rom_filename: String,
    /// Name of the video-disable device property, discovered by CI.
    pub video_prop_name: String,
    /// Size of `OVMF_CODE.fd`, used for the OVMF pair check.
    pub ovmf_code_size: u64,
    /// macOS versions supported by this bundle.
    pub supported_macos_range: String,
    /// VA-API driver name, present when the bundle ships a video driver.
    pub va_driver_name: Option<String>,
    /// Path of the VA-API driver files relative to the bundle.
    pub va_drivers_relpath: Option<String>,
    /// SHA256 digest of every bundled file, keyed by relative path.
    pub files: BTreeMap<String, String>,
    /// Kext paths from `guest/prebuilt/set.toml`, discovered by CI.
    pub guest_kexts: Vec<String>,
}

impl Validate for EngineManifest {
    fn validate(&self) -> Result<(), ContractError> {
        check_schema(
            self.schema_version,
            MANIFEST_SCHEMA_VERSION,
            "schema_version",
        )?;
        if self.abi_version == 0 {
            return Err(ContractError::invalid("abi_version", "must be at least 1"));
        }
        if !is_safe_name(&self.engine_id) {
            return Err(ContractError::invalid("engine_id", "unsafe folder name"));
        }
        if !is_safe_name(&self.rom_filename) || !self.rom_filename.ends_with(".rom") {
            return Err(ContractError::invalid(
                "rom_filename",
                "must be a relative .rom filename",
            ));
        }
        if self.video_prop_name.is_empty() {
            return Err(ContractError::invalid(
                "video_prop_name",
                "must not be empty",
            ));
        }
        if self.ovmf_code_size == 0 {
            return Err(ContractError::invalid("ovmf_code_size", "must be positive"));
        }
        if self.supported_macos_range.is_empty() {
            return Err(ContractError::invalid(
                "supported_macos_range",
                "must not be empty",
            ));
        }
        if self.va_driver_name.is_some() != self.va_drivers_relpath.is_some() {
            return Err(ContractError::invalid(
                "va_driver_name",
                "va_driver_name and va_drivers_relpath must be both set or both absent",
            ));
        }
        for (path, digest) in &self.files {
            if !is_safe_relative_path(path) {
                return Err(ContractError::invalid(
                    "files",
                    format!("unsafe path: {path}"),
                ));
            }
            if !is_sha256(digest) {
                return Err(ContractError::invalid(
                    "files",
                    format!("{path} has no SHA256 digest"),
                ));
            }
        }
        for kext in &self.guest_kexts {
            if !is_safe_relative_path(kext) {
                return Err(ContractError::invalid(
                    "guest_kexts",
                    format!("unsafe path: {kext}"),
                ));
            }
        }
        Ok(())
    }
}

/// One downloadable engine in the release index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct EngineRelease {
    /// Folder name of the engine archive.
    pub engine_id: String,
    /// Host the bundle was built for.
    pub os: HostOs,
    /// Release channel of the bundle.
    pub channel: Channel,
    /// ABI version declared by the bundle.
    pub abi_version: u32,
    /// SHA256 digest of the `.tar.zst` archive.
    pub archive_sha256: String,
    /// Archive size in bytes.
    pub archive_size: u64,
    /// HTTPS URL of the release asset.
    pub url: String,
    /// macOS versions supported by this bundle.
    pub supported_macos_range: String,
    /// Commit summaries since the previous engine build, if published.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changelog: Option<String>,
    /// UTC timestamp of this engine build.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub built_at: Option<String>,
    /// Pinned `reims-vgpu` revision recorded in the index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_revision: Option<String>,
    /// Pinned QEMU fork revision recorded in the index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qemu_revision: Option<String>,
}

impl Validate for EngineRelease {
    fn validate(&self) -> Result<(), ContractError> {
        if !is_safe_name(&self.engine_id) {
            return Err(ContractError::invalid("engine_id", "unsafe folder name"));
        }
        if !is_sha256(&self.archive_sha256) {
            return Err(ContractError::invalid(
                "archive_sha256",
                "must be a SHA256 digest",
            ));
        }
        if self.archive_size == 0 {
            return Err(ContractError::invalid("archive_size", "must be positive"));
        }
        if !self.url.starts_with("https://") {
            return Err(ContractError::invalid("url", "must use HTTPS"));
        }
        if self.supported_macos_range.is_empty() {
            return Err(ContractError::invalid(
                "supported_macos_range",
                "must not be empty",
            ));
        }
        Ok(())
    }
}

/// Release index published next to the engine archives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct EngineIndex {
    /// Schema version of this file.
    pub schema_version: u32,
    /// Available engine bundles.
    pub engines: Vec<EngineRelease>,
}

impl Validate for EngineIndex {
    fn validate(&self) -> Result<(), ContractError> {
        check_schema(self.schema_version, INDEX_SCHEMA_VERSION, "schema_version")?;
        let mut seen = std::collections::BTreeSet::new();
        for engine in &self.engines {
            engine.validate()?;
            if !seen.insert(&engine.engine_id) {
                return Err(ContractError::Duplicate {
                    field: "engine_id",
                    value: engine.engine_id.clone(),
                });
            }
        }
        Ok(())
    }
}

/// User settings stored in `settings.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct Settings {
    /// Schema version of this file.
    pub schema_version: u32,
    /// Selected engine channel.
    pub channel: Channel,
    /// TCP port of the guest bridge on `10.222.0.2`.
    pub bridge_port: u16,
    /// TCP port of the local QMP socket.
    pub qmp_port: u16,
}

impl Settings {
    /// Returns the default settings for a host.
    pub fn for_host(os: HostOs) -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            channel: Channel::for_host(os),
            bridge_port: 8080,
            qmp_port: 4444,
        }
    }
}

impl Validate for Settings {
    fn validate(&self) -> Result<(), ContractError> {
        check_schema(
            self.schema_version,
            SETTINGS_SCHEMA_VERSION,
            "schema_version",
        )?;
        if self.bridge_port == 0 {
            return Err(ContractError::invalid("bridge_port", "must not be 0"));
        }
        if self.qmp_port == 0 {
            return Err(ContractError::invalid("qmp_port", "must not be 0"));
        }
        Ok(())
    }
}

/// Screen resolution stored in `vm.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct Resolution {
    /// Horizontal pixels.
    pub width: u32,
    /// Vertical pixels.
    pub height: u32,
}

impl Default for Resolution {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
        }
    }
}

/// Pixel density of the guest display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HiDpi {
    /// One physical pixel per logical pixel.
    #[serde(rename = "1x")]
    OneX,
    /// Two physical pixels per logical pixel.
    #[serde(rename = "2x")]
    TwoX,
}

/// Guest graphics adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphicsMode {
    /// `reims-graphics-pci` at PCI slot `0x3`.
    Reims,
    /// Safe Mode with `vmware-svga`, capped at 1080p.
    SvgaSafe,
    /// Driver isolation with `lavapipe`, Linux diagnostics only.
    Lavapipe,
}

/// System disk controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiskBus {
    /// NVMe at `addr=0xa`, the default.
    Nvme,
    /// Legacy SATA for macOS 11-13.
    SataLegacy,
}

/// A USB device passed through with `usb-host`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct UsbDevice {
    /// Vendor ID as four hexadecimal digits.
    pub vid: String,
    /// Product ID as four hexadecimal digits.
    pub pid: String,
}

impl Validate for UsbDevice {
    fn validate(&self) -> Result<(), ContractError> {
        if !is_hex_id(&self.vid, 4) {
            return Err(ContractError::invalid("vid", "expected four hex digits"));
        }
        if !is_hex_id(&self.pid, 4) {
            return Err(ContractError::invalid("pid", "expected four hex digits"));
        }
        Ok(())
    }
}

/// Virtual machine configuration stored in `vm.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct VmConfig {
    /// Schema version of this file.
    pub schema_version: u32,
    /// VM folder name.
    pub name: String,
    /// Guest RAM in MiB.
    pub ram_mib: u64,
    /// Guest vCPU count; QEMU always receives `threads=1`.
    pub vcpus: u32,
    /// System disk controller.
    pub disk_bus: DiskBus,
    /// Whether the installer image is attached at `ide.1`.
    pub installer_attached: bool,
    /// Graphics adapter mode.
    pub graphics: GraphicsMode,
    /// Whether `virtio-sound-pci` is present.
    pub audio: bool,
    /// Whether verbose boot (`-v`) is enabled.
    pub verbose: bool,
    /// Guest screen resolution.
    pub resolution: Resolution,
    /// Guest pixel density.
    pub hidpi: HiDpi,
    /// Guest MAC address.
    pub mac: String,
    /// USB devices for hotplug-capable passthrough.
    pub usb_devices: Vec<UsbDevice>,
    /// OpenCore `ScanPolicy`: `983811` during install, `983299` afterwards.
    pub scan_policy: u32,
}

impl VmConfig {
    /// Returns the default configuration of a new macOS VM.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            schema_version: VM_SCHEMA_VERSION,
            name: name.into(),
            ram_mib: 8192,
            vcpus: 4,
            disk_bus: DiskBus::Nvme,
            installer_attached: true,
            graphics: GraphicsMode::Reims,
            audio: true,
            verbose: false,
            resolution: Resolution::default(),
            hidpi: HiDpi::OneX,
            mac: String::from("52:54:00:11:22:33"),
            usb_devices: Vec::new(),
            scan_policy: 983_811,
        }
    }
}

impl Validate for VmConfig {
    fn validate(&self) -> Result<(), ContractError> {
        check_schema(self.schema_version, VM_SCHEMA_VERSION, "schema_version")?;
        if !is_safe_name(&self.name) {
            return Err(ContractError::invalid("name", "unsafe folder name"));
        }
        if !(2048..=262_144).contains(&self.ram_mib) {
            return Err(ContractError::invalid(
                "ram_mib",
                "expected 2048..=262144 MiB",
            ));
        }
        if !(1..=64).contains(&self.vcpus) {
            return Err(ContractError::invalid("vcpus", "expected 1..=64"));
        }
        if !is_mac_address(&self.mac) {
            return Err(ContractError::invalid("mac", "expected six hex byte pairs"));
        }
        if !(640..=7680).contains(&self.resolution.width)
            || !(480..=4320).contains(&self.resolution.height)
        {
            return Err(ContractError::invalid(
                "resolution",
                "resolution out of range",
            ));
        }
        if matches!(self.graphics, GraphicsMode::SvgaSafe)
            && (self.resolution.width > 1920 || self.resolution.height > 1080)
        {
            return Err(ContractError::invalid(
                "resolution",
                "Safe Mode is capped at 1920x1080",
            ));
        }
        if !matches!(self.scan_policy, 983_811 | 983_299) {
            return Err(ContractError::invalid(
                "scan_policy",
                "expected 983811 or 983299",
            ));
        }
        for device in &self.usb_devices {
            device.validate()?;
        }
        Ok(())
    }
}

/// SMBIOS identity stored in `smbios.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct SmbiosConfig {
    /// Schema version of this file.
    pub schema_version: u32,
    /// Mac model reported to the guest.
    pub model: String,
    /// Serial number reported to the guest.
    pub serial: String,
    /// Board-ID used by the Apple recovery client.
    pub board_id: String,
}

impl SmbiosConfig {
    /// Returns the default identity, an iMac20,1.
    pub fn new() -> Self {
        Self {
            schema_version: SMBIOS_SCHEMA_VERSION,
            model: String::from("iMac20,1"),
            serial: String::new(),
            board_id: String::new(),
        }
    }
}

impl Default for SmbiosConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl Validate for SmbiosConfig {
    fn validate(&self) -> Result<(), ContractError> {
        check_schema(self.schema_version, SMBIOS_SCHEMA_VERSION, "schema_version")?;
        if self.model.is_empty() || self.serial.is_empty() || self.board_id.is_empty() {
            return Err(ContractError::invalid(
                "smbios",
                "model, serial and board_id must not be empty",
            ));
        }
        Ok(())
    }
}

/// CPU vendor reported by the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CpuVendor {
    /// Intel.
    Intel,
    /// AMD.
    Amd,
}

/// Hardware backend used by the VM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VmBackend {
    /// Windows Hypervisor Platform.
    Whpx,
    /// Kernel-based Virtual Machine.
    KVM,
}

/// Host-specific probe results cached in `host_profiles/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct HostProfile {
    /// Schema version of this file.
    pub schema_version: u32,
    /// Fingerprint of the host hardware and OS.
    pub fingerprint: String,
    /// Host operating system.
    pub os: HostOs,
    /// CPU vendor.
    pub cpu_vendor: CpuVendor,
    /// Backend available on this host.
    pub backend: VmBackend,
    /// Selected GPU vendor ID.
    pub gpu_vid: String,
    /// Selected GPU device ID.
    pub gpu_did: String,
    /// Video decoder reported by the host.
    pub decoder: String,
    /// Whether the probe finished successfully.
    pub probe_done: bool,
}

impl Validate for HostProfile {
    fn validate(&self) -> Result<(), ContractError> {
        check_schema(
            self.schema_version,
            HOST_PROFILE_SCHEMA_VERSION,
            "schema_version",
        )?;
        if !is_safe_name(&self.fingerprint) {
            return Err(ContractError::invalid("fingerprint", "unsafe name"));
        }
        if !is_hex_id(&self.gpu_vid, 4) || !is_hex_id(&self.gpu_did, 4) {
            return Err(ContractError::invalid(
                "gpu_vid",
                "GPU identifiers must be four hex digits",
            ));
        }
        if !self.probe_done {
            return Err(ContractError::invalid(
                "probe_done",
                "an incomplete profile must not be cached",
            ));
        }
        Ok(())
    }
}

/// How strongly a crash signature should be presented to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Informational note.
    Info,
    /// Recoverable problem with a workaround.
    Warning,
    /// Fatal failure that stops the VM.
    Critical,
}

impl fmt::Display for Severity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Info => formatter.write_str("info"),
            Self::Warning => formatter.write_str("warning"),
            Self::Critical => formatter.write_str("critical"),
        }
    }
}

/// One entry of the external crash-signature dictionary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct CrashSignature {
    /// Stable identifier, for example `vk_device_lost`.
    pub id: String,
    /// Regular expression matched against an engine log line.
    pub pattern: String,
    /// Advice shown to the user.
    pub advice: String,
    /// Presentation severity.
    pub severity: Severity,
}

impl Validate for CrashSignature {
    fn validate(&self) -> Result<(), ContractError> {
        if !is_safe_name(&self.id) {
            return Err(ContractError::invalid("id", "unsafe identifier"));
        }
        if self.advice.is_empty() {
            return Err(ContractError::invalid("advice", "must not be empty"));
        }
        Regex::new(&self.pattern).map_err(|source| ContractError::Pattern {
            id: self.id.clone(),
            source,
        })?;
        Ok(())
    }
}

/// Crash-signature dictionary published next to `index.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct SignatureCatalog {
    /// Schema version of this file.
    pub schema_version: u32,
    /// Signatures ordered by specificity.
    pub signatures: Vec<CrashSignature>,
}

impl Validate for SignatureCatalog {
    fn validate(&self) -> Result<(), ContractError> {
        check_schema(
            self.schema_version,
            SIGNATURES_SCHEMA_VERSION,
            "schema_version",
        )?;
        let mut seen = std::collections::BTreeSet::new();
        for signature in &self.signatures {
            signature.validate()?;
            if !seen.insert(&signature.id) {
                return Err(ContractError::Duplicate {
                    field: "signature.id",
                    value: signature.id.clone(),
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use proptest::prelude::*;

    use super::*;

    fn sha(letter: char) -> String {
        letter.to_string().repeat(64)
    }

    #[test]
    fn settings_defaults_follow_the_host() {
        assert_eq!(
            Settings::for_host(HostOs::Windows).channel,
            Channel::Development
        );
        assert_eq!(Settings::for_host(HostOs::Linux).channel, Channel::Master);
    }

    #[test]
    fn vm_config_round_trips_through_toml() {
        let config = VmConfig::new("macos-tahoe");
        let raw = toml::to_string(&config).expect("serialize vm config");
        let parsed: VmConfig = parse_toml(&raw, Path::new("vm.toml")).expect("parse vm config");
        assert_eq!(parsed, config);
    }

    #[test]
    fn catalog_round_trips_through_json() {
        let catalog = SignatureCatalog {
            schema_version: SIGNATURES_SCHEMA_VERSION,
            signatures: vec![CrashSignature {
                id: String::from("device_lost"),
                pattern: String::from("VK_DEVICE_LOST"),
                advice: String::from("Check the GPU driver."),
                severity: Severity::Critical,
            }],
        };
        let raw = serde_json::to_string(&catalog).expect("serialize catalog");
        let parsed: SignatureCatalog =
            parse_json(&raw, Path::new("signatures.json")).expect("parse catalog");
        assert_eq!(parsed, catalog);
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let raw = r#"{"schema_version":1,"channel":"master","bridge_port":8080,"qmp_port":4444,"extra":true}"#;
        let result: Result<Settings, _> = serde_json::from_str(raw);
        assert!(result.is_err());
    }

    #[test]
    fn manifest_rejects_unsafe_rom_name() {
        let mut manifest = fixture_manifest();
        manifest.rom_filename = String::from("../escape.rom");
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn manifest_requires_paired_va_api_fields() {
        let mut manifest = fixture_manifest();
        manifest.va_driver_name = Some(String::from("nvidia"));
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn index_rejects_duplicate_engine_ids() {
        let release = EngineRelease {
            engine_id: String::from("linux64-master-abc"),
            os: HostOs::Linux,
            channel: Channel::Master,
            abi_version: 1,
            archive_sha256: sha('a'),
            archive_size: 1,
            url: String::from("https://example.invalid/engine.tar.zst"),
            supported_macos_range: String::from("26.4+"),
            changelog: None,
            built_at: None,
            source_revision: None,
            qemu_revision: None,
        };
        let index = EngineIndex {
            schema_version: INDEX_SCHEMA_VERSION,
            engines: vec![release.clone(), release],
        };
        assert!(index.validate().is_err());
    }

    #[test]
    fn safe_mode_rejects_large_resolution() {
        let mut config = VmConfig::new("macos-tahoe");
        config.graphics = GraphicsMode::SvgaSafe;
        config.resolution = Resolution {
            width: 2560,
            height: 1440,
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn catalog_rejects_invalid_regex() {
        let catalog = SignatureCatalog {
            schema_version: SIGNATURES_SCHEMA_VERSION,
            signatures: vec![CrashSignature {
                id: String::from("broken"),
                pattern: String::from("("),
                advice: String::from("unused"),
                severity: Severity::Info,
            }],
        };
        assert!(catalog.validate().is_err());
    }

    fn fixture_manifest() -> EngineManifest {
        EngineManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            abi_version: 1,
            engine_id: String::from("linux64-master-abc"),
            os: HostOs::Linux,
            channel: Channel::Master,
            source_revision: String::from("abc"),
            qemu_revision: String::from("def"),
            rom_filename: String::from("ReimsGraphics.rom"),
            video_prop_name: String::from("disable-video"),
            ovmf_code_size: 2_097_152,
            supported_macos_range: String::from("26.4+"),
            va_driver_name: None,
            va_drivers_relpath: None,
            files: BTreeMap::new(),
            guest_kexts: Vec::new(),
        }
    }

    proptest! {
        #[test]
        fn sha256_validator_accepts_hex_strings(value in "[0-9a-f]{64}") {
            prop_assert!(is_sha256(&value));
        }

        #[test]
        fn mac_addresses_round_trip(bytes in prop::array::uniform6(any::<u8>())) {
            let text = format!(
                "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5]
            );
            prop_assert!(is_mac_address(&text));
        }
    }
}
