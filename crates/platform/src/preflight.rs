//! Pre-Flight Doctor: deterministic host checks and GPU selection policy.
//!
//! # Quick start
//!
//! ```
//! use reims_core::contracts::{CpuVendor, HostOs};
//! use reims_platform::preflight::{PreflightDoctor, PreflightInput, WhpxState};
//!
//! let input = PreflightInput {
//!     os: HostOs::Windows,
//!     filesystem: String::from("NTFS"),
//!     available_memory_mib: 16_384,
//!     virtualization_enabled: true,
//!     whpx: WhpxState::Active,
//!     color_depth: 32,
//!     kernel_release: None,
//!     kvm_accessible: false,
//!     udmabuf_accessible: false,
//!     display_available: true,
//!     gpu_count: 1,
//!     identical_gpu_ids: false,
//!     old_vulkan_loader: false,
//!     elevated: false,
//!     cpu_vendor: CpuVendor::Intel,
//!     audio_enabled: true,
//!     wayland_nvidia: false,
//!     amd_debug_kernel: false,
//! };
//! let report = PreflightDoctor.check(&input);
//! assert!(!report.has_blocker());
//! ```

use reims_core::contracts::{CpuVendor, HostOs};
use sysinfo::System;

/// Severity of one Pre-Flight finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Check passed.
    Ok,
    /// The VM can start with a degraded configuration or a documented workaround.
    Warning,
    /// The VM must not start.
    Blocker,
}

/// One Pre-Flight result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Stable check identifier.
    pub code: &'static str,
    /// Severity of the result.
    pub level: Level,
    /// User-facing explanation in English.
    pub message: String,
}

impl Finding {
    fn new(code: &'static str, level: Level, message: impl Into<String>) -> Self {
        Self {
            code,
            level,
            message: message.into(),
        }
    }
}

/// State of `WHvGetCapability(HypervisorPresent)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhpxState {
    /// WHPX is active.
    Active,
    /// WHPX is installed but a reboot is required.
    PendingReboot,
    /// WHPX is disabled.
    Disabled,
}

/// GPU selection decided by Pre-Flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuSelection {
    /// One GPU: do not override the host loader.
    Default,
    /// Different GPU IDs: set `VK_LOADER_DEVICE_SELECT` and the host-specific variable.
    LoaderVariable,
    /// Identical GPU IDs or an old loader: use `vk-gpu-select`.
    FallbackLayer,
    /// A layer would be needed but elevation makes the loader ignore it.
    BlockedByElevation,
}

/// All host facts needed by the Pre-Flight Doctor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightInput {
    /// Host operating system.
    pub os: HostOs,
    /// Filesystem label of the portable folder.
    pub filesystem: String,
    /// Available RAM in MiB, from [`available_memory_mib`] or an injected value.
    pub available_memory_mib: u64,
    /// Whether virtualization is enabled in BIOS or UEFI.
    pub virtualization_enabled: bool,
    /// WHPX capability state on Windows.
    pub whpx: WhpxState,
    /// Host color depth in bits per pixel.
    pub color_depth: u32,
    /// Kernel release string on Linux, for example `7.2.0-generic`.
    pub kernel_release: Option<String>,
    /// Whether `/dev/kvm` is readable and writable.
    pub kvm_accessible: bool,
    /// Whether `/dev/udmabuf` is readable and writable.
    pub udmabuf_accessible: bool,
    /// Whether `DISPLAY` or `WAYLAND_DISPLAY` is available.
    pub display_available: bool,
    /// Number of Vulkan GPUs visible to the host loader.
    pub gpu_count: usize,
    /// Whether two GPUs report the same VID:DID pair.
    pub identical_gpu_ids: bool,
    /// Whether the Vulkan loader is too old for device selection.
    pub old_vulkan_loader: bool,
    /// Whether the launcher runs elevated.
    pub elevated: bool,
    /// CPU vendor of the host.
    pub cpu_vendor: CpuVendor,
    /// Whether guest audio is enabled.
    pub audio_enabled: bool,
    /// Whether Wayland is combined with an NVIDIA GPU.
    pub wayland_nvidia: bool,
    /// Whether an AMD KDK debug kernel is running.
    pub amd_debug_kernel: bool,
}

/// Result of all Pre-Flight checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightReport {
    /// Findings in check order.
    pub findings: Vec<Finding>,
    /// Whether the video-decode property from `manifest.json` must be set.
    pub disable_video_decode: bool,
    /// Whether the UI may switch from `tiny-skia` to `wgpu`.
    pub allow_wgpu: bool,
    /// Selected GPU override strategy.
    pub gpu_selection: GpuSelection,
}

impl PreflightReport {
    /// Returns true when at least one check blocks startup.
    pub fn has_blocker(&self) -> bool {
        self.findings
            .iter()
            .any(|finding| finding.level == Level::Blocker)
    }
}

/// Runs the host checks against injected facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PreflightDoctor;

impl PreflightDoctor {
    /// Evaluates `input` without touching the host.
    pub fn check(&self, input: &PreflightInput) -> PreflightReport {
        let mut findings = Vec::new();
        let mut disable_video_decode = false;

        findings.push(filesystem_check(&input.filesystem));
        findings.push(memory_check(input.available_memory_mib));
        if input.virtualization_enabled {
            findings.push(Finding::new(
                "virtualization",
                Level::Ok,
                "Virtualization is enabled.",
            ));
        } else {
            findings.push(Finding::new(
                "virtualization",
                Level::Blocker,
                "Enable VT-x or SVM in BIOS or UEFI.",
            ));
        }

        if input.os == HostOs::Windows {
            findings.push(whpx_check(input.whpx));
        }
        let (depth_finding, wgpu_finding) = color_depth_findings(input.color_depth);
        findings.push(depth_finding);
        if let Some(finding) = wgpu_finding {
            findings.push(finding);
        }

        if input.os == HostOs::Linux {
            match &input.kernel_release {
                Some(release) => findings.push(kernel_check(release, input.cpu_vendor)),
                None => findings.push(Finding::new(
                    "kernel",
                    Level::Blocker,
                    "Kernel release is unknown.",
                )),
            }
            if input.kvm_accessible {
                findings.push(Finding::new("kvm", Level::Ok, "/dev/kvm is accessible."));
            } else {
                findings.push(Finding::new(
                    "kvm",
                    Level::Blocker,
                    "Add the user to the kvm group and check /dev/kvm.",
                ));
            }
            if input.udmabuf_accessible {
                findings.push(Finding::new(
                    "udmabuf",
                    Level::Ok,
                    "/dev/udmabuf is accessible.",
                ));
            } else {
                findings.push(Finding::new(
                    "udmabuf",
                    Level::Blocker,
                    "Grant read and write access to /dev/udmabuf for AMD guests.",
                ));
            }
            if input.display_available {
                findings.push(Finding::new(
                    "display",
                    Level::Ok,
                    "A display server is available.",
                ));
            } else {
                findings.push(Finding::new(
                    "display",
                    Level::Blocker,
                    "Set DISPLAY or WAYLAND_DISPLAY before starting the VM.",
                ));
            }
        }

        let gpu_selection = select_gpu(input);
        findings.push(gpu_check(gpu_selection));

        if !input.audio_enabled {
            findings.push(Finding::new(
                "audio",
                Level::Warning,
                "Audio is disabled; Safari and YouTube may report an error.",
            ));
        }
        if input.cpu_vendor == CpuVendor::Intel {
            disable_video_decode = true;
            findings.push(Finding::new(
                "intel_uhd",
                Level::Warning,
                "Hardware video decoding is disabled with the manifest property (issue #142).",
            ));
        }
        if input.wayland_nvidia {
            findings.push(Finding::new(
                "wayland_nvidia",
                Level::Warning,
                "Use SDL_VIDEODRIVER=x11 or disable borderless mode (issue #132).",
            ));
        }
        if input.amd_debug_kernel {
            findings.push(Finding::new(
                "amd_debug_kernel",
                Level::Warning,
                "AMD KDK debug kernels are incompatible with the guest memory protection path.",
            ));
        }

        let allow_wgpu = input.color_depth >= 24;
        PreflightReport {
            findings,
            disable_video_decode,
            allow_wgpu,
            gpu_selection,
        }
    }
}

fn filesystem_check(label: &str) -> Finding {
    match label.to_ascii_uppercase().as_str() {
        "FAT32" | "FAT16" | "FAT" | "VFAT" => Finding::new(
            "filesystem",
            Level::Blocker,
            "FAT32 cannot hold a VM disk; use NTFS, exFAT, ext4 or APFS.",
        ),
        "EXFAT" => Finding::new(
            "filesystem",
            Level::Warning,
            "exFAT needs periodic Compact QCOW2 to reclaim space.",
        ),
        "NTFS" | "EXT4" | "APFS" => {
            Finding::new("filesystem", Level::Ok, "Filesystem is supported.")
        }
        other => Finding::new(
            "filesystem",
            Level::Warning,
            format!("Unrecognized filesystem {other}; portability is unverified."),
        ),
    }
}

fn memory_check(available_mib: u64) -> Finding {
    if available_mib >= 4096 {
        Finding::new("memory", Level::Ok, "Enough memory is available.")
    } else {
        Finding::new(
            "memory",
            Level::Blocker,
            "At least 4096 MiB of available memory is required.",
        )
    }
}

fn whpx_check(state: WhpxState) -> Finding {
    match state {
        WhpxState::Active => Finding::new("whpx", Level::Ok, "WHPX is active."),
        WhpxState::PendingReboot => Finding::new(
            "whpx",
            Level::Blocker,
            "Reboot to activate WHPX before starting the VM.",
        ),
        WhpxState::Disabled => Finding::new(
            "whpx",
            Level::Blocker,
            "Enable Windows Hypervisor Platform and virtualization in BIOS.",
        ),
    }
}

fn color_depth_findings(depth: u32) -> (Finding, Option<Finding>) {
    if depth >= 24 {
        (
            Finding::new("color_depth", Level::Ok, "Color depth allows wgpu."),
            None,
        )
    } else {
        (
            Finding::new(
                "color_depth",
                Level::Warning,
                "wgpu is disabled below 24-bit color depth.",
            ),
            Some(Finding::new(
                "wgpu",
                Level::Warning,
                "The UI stays on tiny-skia at 16-bit color depth.",
            )),
        )
    }
}

fn parse_kernel_release(release: &str) -> Option<(u32, u32)> {
    let (major, rest) = release.split_once('.')?;
    let minor: String = rest.chars().take_while(char::is_ascii_digit).collect();
    Some((major.parse().ok()?, minor.parse().ok()?))
}

fn kernel_check(release: &str, vendor: CpuVendor) -> Finding {
    match parse_kernel_release(release) {
        Some((major, minor)) if major > 7 || (major == 7 && minor >= 2) => Finding::new(
            "kernel",
            Level::Ok,
            "Kernel meets the guest memory protection requirement.",
        ),
        Some((6, minor)) if minor >= 13 && vendor != CpuVendor::Amd => Finding::new(
            "kernel",
            Level::Warning,
            "Kernel 6.13-7.1 needs a manual override and is unsupported with AMD GPUs.",
        ),
        Some(_) => Finding::new("kernel", Level::Blocker, "Kernel 7.2 or newer is required."),
        None => Finding::new("kernel", Level::Blocker, "Cannot parse the kernel release."),
    }
}

fn select_gpu(input: &PreflightInput) -> GpuSelection {
    if input.gpu_count <= 1 {
        return GpuSelection::Default;
    }
    if input.identical_gpu_ids || input.old_vulkan_loader {
        if input.elevated {
            return GpuSelection::BlockedByElevation;
        }
        return GpuSelection::FallbackLayer;
    }
    GpuSelection::LoaderVariable
}

fn gpu_check(selection: GpuSelection) -> Finding {
    match selection {
        GpuSelection::Default => Finding::new("gpu", Level::Ok, "One GPU: no override is needed."),
        GpuSelection::LoaderVariable => Finding::new(
            "gpu",
            Level::Ok,
            "Select the GPU with VK_LOADER_DEVICE_SELECT and the host loader variable.",
        ),
        GpuSelection::FallbackLayer => Finding::new(
            "gpu",
            Level::Warning,
            "Identical GPU IDs or an old loader need vk-gpu-select via VK_ADD_LAYER_PATH.",
        ),
        GpuSelection::BlockedByElevation => Finding::new(
            "gpu",
            Level::Blocker,
            "Run the launcher without elevation: the loader ignores layers under UAC.",
        ),
    }
}

/// Returns available memory in MiB from the host.
pub fn available_memory_mib() -> u64 {
    match System::new_all() {
        Ok(system) => system.available_memory() / (1024 * 1024),
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use reims_core::contracts::{CpuVendor, HostOs};

    use super::*;

    fn input(os: HostOs) -> PreflightInput {
        PreflightInput {
            os,
            filesystem: String::from("NTFS"),
            available_memory_mib: 16_384,
            virtualization_enabled: true,
            whpx: WhpxState::Active,
            color_depth: 32,
            kernel_release: Some(String::from("7.2.0-generic")),
            kvm_accessible: true,
            udmabuf_accessible: true,
            display_available: true,
            gpu_count: 1,
            identical_gpu_ids: false,
            old_vulkan_loader: false,
            elevated: false,
            cpu_vendor: CpuVendor::Amd,
            audio_enabled: true,
            wayland_nvidia: false,
            amd_debug_kernel: false,
        }
    }

    #[test]
    fn clean_windows_host_has_no_blocker() {
        let report = PreflightDoctor.check(&input(HostOs::Windows));
        assert!(!report.has_blocker());
        assert!(report.allow_wgpu);
        assert_eq!(report.gpu_selection, GpuSelection::Default);
    }

    #[test]
    fn fat32_and_exfat_are_handled_differently() {
        let mut input = input(HostOs::Windows);
        input.filesystem = String::from("FAT32");
        assert!(PreflightDoctor.check(&input).has_blocker());
        input.filesystem = String::from("exFAT");
        let report = PreflightDoctor.check(&input);
        assert!(!report.has_blocker());
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.code == "filesystem" && f.level == Level::Warning)
        );
    }

    #[test]
    fn pending_reboot_blocks_windows() {
        let mut input = input(HostOs::Windows);
        input.whpx = WhpxState::PendingReboot;
        assert!(PreflightDoctor.check(&input).has_blocker());
    }

    #[test]
    fn old_kernel_blocks_amd_and_warns_intel() {
        let mut input = input(HostOs::Linux);
        input.kernel_release = Some(String::from("6.15.4-generic"));
        assert!(PreflightDoctor.check(&input).has_blocker());
        input.cpu_vendor = CpuVendor::Intel;
        let report = PreflightDoctor.check(&input);
        assert!(!report.has_blocker());
        assert_eq!(
            report
                .findings
                .iter()
                .find(|f| f.code == "kernel")
                .map(|f| f.level),
            Some(Level::Warning)
        );
    }

    #[test]
    fn sixteen_bit_depth_keeps_tiny_skia() {
        let mut input = input(HostOs::Windows);
        input.color_depth = 16;
        let report = PreflightDoctor.check(&input);
        assert!(!report.allow_wgpu);
        assert!(report.findings.iter().any(|f| f.code == "wgpu"));
    }

    #[test]
    fn identical_gpus_need_a_layer_except_when_elevated() {
        let mut input = input(HostOs::Windows);
        input.gpu_count = 2;
        input.identical_gpu_ids = true;
        let report = PreflightDoctor.check(&input);
        assert_eq!(report.gpu_selection, GpuSelection::FallbackLayer);
        input.elevated = true;
        let report = PreflightDoctor.check(&input);
        assert_eq!(report.gpu_selection, GpuSelection::BlockedByElevation);
        assert!(report.has_blocker());
    }

    #[test]
    fn intel_requires_the_manifest_video_property() {
        let mut input = input(HostOs::Windows);
        input.cpu_vendor = CpuVendor::Intel;
        assert!(PreflightDoctor.check(&input).disable_video_decode);
    }

    #[test]
    fn missing_linux_devices_block_startup() {
        let mut input = input(HostOs::Linux);
        input.kvm_accessible = false;
        input.udmabuf_accessible = false;
        input.display_available = false;
        let report = PreflightDoctor.check(&input);
        assert_eq!(
            report
                .findings
                .iter()
                .filter(|f| f.level == Level::Blocker)
                .count(),
            3
        );
    }
}
