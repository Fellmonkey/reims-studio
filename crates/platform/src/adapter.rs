//! Versioned `manifest.abi_version -> QEMU CLI args` mapper.
//!
//! # Quick start
//!
//! ```
//! use reims_platform::adapter::EngineAdapter;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let adapter = EngineAdapter::for_abi(1)?;
//! println!("ABI {}", adapter.abi_version());
//! # Ok(())
//! # }
//! ```

use std::collections::BTreeMap;

use reims_core::contracts::{
    ContractError, EngineManifest, GraphicsMode, HiDpi, HostOs, Validate, VmConfig,
};
use thiserror::Error;

/// Highest ABI version understood by this adapter.
pub const LATEST_ABI: u32 = 1;

const SLIRP_NETWORK: &str = "net=10.222.0.0/24,host=10.222.0.2";
const APPLE_SMC_OSK: &str = "ourhardworkbythesewordsguardedpleasedontsteal(c)AppleComputerInc";
const ENGINE_LOG: &str = "warn,reims_vgpu_core::device::lifecycle=info";

/// Failures of the engine adapter.
#[derive(Debug, Error)]
pub enum AdapterError {
    /// The bundle uses an ABI version this launcher does not implement.
    #[error("unsupported engine ABI {version}; supported range is 1..={LATEST_ABI}")]
    UnsupportedAbi {
        /// Declared ABI version.
        version: u32,
    },
    /// A versioned contract failed validation.
    #[error(transparent)]
    Contract(#[from] ContractError),
    /// The command specification is internally inconsistent.
    #[error("invalid command spec: {0}")]
    InvalidSpec(String),
}

/// Inputs that are not part of the stored VM configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmSpec {
    /// Host the VM will run on.
    pub os: HostOs,
    /// Engine folder name from `settings.toml` and `index.json`.
    pub engine_id: String,
    /// Contract of the selected engine bundle.
    pub manifest: EngineManifest,
    /// Stored VM configuration.
    pub vm: VmConfig,
    /// Installer image path relative to the VM folder, when attached.
    pub installer_image: Option<String>,
    /// Local QMP TCP port.
    pub qmp_port: u16,
    /// Guest bridge port on the SLIRP gateway.
    pub bridge_port: u16,
    /// Shader cache path relative to the VM folder.
    pub shader_cache_dir: String,
    /// Whether to disable hardware video decoding with the manifest property.
    pub disable_video_decode: bool,
}

/// Fully rendered QEMU invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    /// Engine binary path relative to the VM folder.
    pub program: String,
    /// Ordered QEMU arguments.
    pub args: Vec<String>,
    /// Environment variables for the engine process.
    pub env: BTreeMap<String, String>,
    /// Working directory of the engine process.
    pub working_dir: String,
}

/// Maps one engine ABI version to a QEMU command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineAdapter {
    abi_version: u32,
}

impl EngineAdapter {
    /// Creates an adapter from a validated engine contract.
    ///
    /// # Errors
    ///
    /// Returns [`AdapterError::UnsupportedAbi`] for an unknown ABI version.
    pub fn for_manifest(manifest: &EngineManifest) -> Result<Self, AdapterError> {
        Self::for_abi(manifest.abi_version)
    }

    /// Creates an adapter for an explicit ABI version.
    ///
    /// # Errors
    ///
    /// Returns [`AdapterError::UnsupportedAbi`] for an unknown ABI version.
    pub fn for_abi(abi_version: u32) -> Result<Self, AdapterError> {
        if (1..=LATEST_ABI).contains(&abi_version) {
            Ok(Self { abi_version })
        } else {
            Err(AdapterError::UnsupportedAbi {
                version: abi_version,
            })
        }
    }

    /// Returns the ABI version handled by this adapter.
    pub fn abi_version(&self) -> u32 {
        self.abi_version
    }

    /// Renders the QEMU command for `spec`.
    ///
    /// All engine paths are relative to the VM folder, so a portable folder
    /// can move between hosts without rewriting configuration files.
    ///
    /// # Errors
    ///
    /// Returns [`AdapterError`] when a contract or spec invariant is violated.
    pub fn build(&self, spec: &VmSpec) -> Result<CommandSpec, AdapterError> {
        spec.vm.validate()?;
        spec.manifest.validate()?;
        if spec.manifest.abi_version != self.abi_version {
            return Err(AdapterError::InvalidSpec(String::from(
                "manifest ABI does not match the adapter",
            )));
        }
        if spec.manifest.os != spec.os {
            return Err(AdapterError::InvalidSpec(String::from(
                "engine OS does not match the target host",
            )));
        }
        if spec.qmp_port == 0 || spec.bridge_port == 0 {
            return Err(AdapterError::InvalidSpec(String::from(
                "ports must not be zero",
            )));
        }
        if spec.shader_cache_dir.is_empty() {
            return Err(AdapterError::InvalidSpec(String::from(
                "shader cache path must not be empty",
            )));
        }
        if spec.installer_image.as_deref().is_some_and(str::is_empty) {
            return Err(AdapterError::InvalidSpec(String::from(
                "installer image path must not be empty",
            )));
        }

        let engine = format!("../../engines/{}", spec.engine_id);
        let binary = if spec.os == HostOs::Windows {
            "qemu-system-x86_64.exe"
        } else {
            "qemu-system-x86_64"
        };
        let vcpus = spec.vm.vcpus.to_string();
        let ram = format!("{}M", spec.vm.ram_mib);
        let safe_mode = spec.vm.graphics == GraphicsMode::SvgaSafe;

        let mut args = Vec::new();
        if safe_mode {
            args.push(String::from("-machine"));
            args.push(machine_arg(spec.os, false));
        } else {
            args.push(String::from("-machine"));
            args.push(machine_arg(spec.os, true));
            args.push(String::from("-display"));
            args.push(String::from("none"));
            args.push(String::from("-vga"));
            args.push(String::from("none"));
        }
        args.push(String::from("-smp"));
        args.push(format!("{vcpus},sockets=1,cores={vcpus},threads=1"));
        args.push(String::from("-rtc"));
        args.push(String::from("base=utc,clock=host"));
        args.push(String::from("-m"));
        args.push(ram.clone());
        args.push(String::from("-object"));
        args.push(memory_backend(spec, &vcpus, &ram));
        args.push(String::from("-global"));
        args.push(String::from("ICH9-LPC.disable_s3=1"));

        args.push(String::from("-drive"));
        args.push(format!(
            "if=pflash,format=raw,readonly=on,file={engine}/firmware/OVMF_CODE.fd"
        ));
        args.push(String::from("-drive"));
        args.push(String::from("if=pflash,format=raw,file=OVMF_VARS.fd"));

        args.push(String::from("-drive"));
        args.push(String::from(
            "if=none,id=opencore,format=qcow2,file=OpenCore.qcow2",
        ));
        args.push(String::from("-device"));
        args.push(String::from(
            "ide-hd,bus=ide.0,unit=0,drive=opencore,bootindex=1",
        ));

        if let Some(image) = &spec.installer_image {
            args.push(String::from("-drive"));
            args.push(format!(
                "if=none,id=installer,format=raw,readonly=on,file={image}"
            ));
            args.push(String::from("-device"));
            args.push(String::from("ide-hd,bus=ide.1,unit=0,drive=installer"));
        }

        args.push(String::from("-drive"));
        args.push(String::from(
            "if=none,id=disk0,format=qcow2,file=disk.qcow2,discard=unmap,detect-zeroes=unmap,aio=threads,cache=writeback",
        ));
        args.push(String::from("-device"));
        if spec.vm.disk_bus == reims_core::contracts::DiskBus::Nvme {
            args.push(String::from(
                "nvme,addr=0xa,id=nvme0,serial=REIMSSTUDIO1,drive=disk0",
            ));
        } else {
            args.push(String::from("ide-hd,bus=ide.2,unit=0,drive=disk0"));
        }

        args.push(String::from("-device"));
        args.push(graphics_device(spec, &engine));

        args.push(String::from("-device"));
        args.push(format!(
            "virtio-net-pci,addr=0x4,netdev=net0,ctrl_vq=off,mac={}",
            spec.vm.mac
        ));
        args.push(String::from("-netdev"));
        args.push(format!("user,id=net0,{SLIRP_NETWORK}"));

        args.push(String::from("-device"));
        args.push(String::from(
            "qemu-xhci,id=xhci,addr=0x5,conditional-intr-mapping=on,p2=7,p3=7",
        ));
        args.push(String::from("-device"));
        args.push(String::from("usb-kbd,bus=xhci.0"));
        args.push(String::from("-device"));
        args.push(String::from("usb-tablet,bus=xhci.0"));
        for device in &spec.vm.usb_devices {
            args.push(String::from("-device"));
            args.push(format!(
                "usb-host,bus=xhci.0,vendorid=0x{},productid=0x{}",
                device.vid, device.pid
            ));
        }

        if spec.vm.audio {
            args.push(String::from("-device"));
            args.push(String::from("virtio-sound-pci,addr=0x7,streams=1"));
            args.push(String::from("-device"));
            args.push(String::from(
                "virtio-sound-pci,addr=0x8,streams=1,input-streams=1",
            ));
        }

        args.push(String::from("-device"));
        args.push(format!("isa-applesmc,osk={APPLE_SMC_OSK}"));
        args.push(String::from("-qmp"));
        args.push(format!(
            "tcp:127.0.0.1:{},server=on,wait=off",
            spec.qmp_port
        ));

        Ok(CommandSpec {
            program: format!("{engine}/bin/{binary}"),
            args,
            env: environment(spec, &engine),
            working_dir: format!("vms/{}", spec.vm.name),
        })
    }
}

fn machine_arg(os: HostOs, gpu: bool) -> String {
    let mut value = String::from("q35,max-ram-below-4g=1G,memory-backend=pc.ram");
    if os == HostOs::Linux && gpu {
        value.push_str(",dump-guest-core=off");
    }
    value
}

fn memory_backend(spec: &VmSpec, vcpus: &str, ram: &str) -> String {
    if spec.os == HostOs::Windows {
        format!("memory-backend-ram,id=pc.ram,size={ram},share=on")
    } else {
        format!("memory-backend-memfd,id=pc.ram,size={ram},prealloc=on,prealloc-threads={vcpus}")
    }
}

fn graphics_device(spec: &VmSpec, engine: &str) -> String {
    if spec.vm.graphics == GraphicsMode::SvgaSafe {
        return String::from("vmware-svga,addr=0x3");
    }
    let mut device = format!(
        "reims-graphics-pci,addr=0x3,romfile={engine}/firmware/{},resolution={}x{}",
        spec.manifest.rom_filename, spec.vm.resolution.width, spec.vm.resolution.height
    );
    if spec.vm.hidpi == HiDpi::TwoX {
        device.push_str(",hidpi=2x");
    }
    if spec.disable_video_decode {
        device.push_str(&format!(",{}", spec.manifest.video_prop_name));
    }
    device
}

fn environment(spec: &VmSpec, engine: &str) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    env.insert(String::from("REIMS_VGPU_LOG"), String::from(ENGINE_LOG));
    env.insert(String::from("REIMS_VGPU_STRICT"), String::from("0"));
    env.insert(String::from("__GL_SHADER_DISK_CACHE"), String::from("1"));
    env.insert(
        String::from("__GL_SHADER_DISK_CACHE_PATH"),
        spec.shader_cache_dir.clone(),
    );
    env.insert(
        String::from("__GL_SHADER_DISK_CACHE_SKIP_CLEANUP"),
        String::from("1"),
    );
    env.insert(
        String::from("MESA_SHADER_CACHE_DIR"),
        spec.shader_cache_dir.clone(),
    );
    env.insert(
        String::from("MESA_SHADER_CACHE_MAX_SIZE"),
        String::from("2G"),
    );
    if spec.os == HostOs::Linux {
        env.insert(String::from("XDG_CACHE_HOME"), String::from("../../cache"));
        if spec.vm.graphics == GraphicsMode::Lavapipe {
            env.insert(
                String::from("VK_LOADER_DRIVERS_SELECT"),
                String::from("lavapipe"),
            );
        }
    }
    if let (Some(name), Some(path)) = (
        &spec.manifest.va_driver_name,
        &spec.manifest.va_drivers_relpath,
    ) {
        env.insert(String::from("LIBVA_DRIVER_NAME"), name.clone());
        env.insert(
            String::from("LIBVA_DRIVERS_PATH"),
            format!("{engine}/{path}"),
        );
    }
    env
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use reims_core::contracts::{
        EngineManifest, GraphicsMode, HiDpi, HostOs, MANIFEST_SCHEMA_VERSION, UsbDevice, VmConfig,
    };

    use super::*;

    fn manifest(os: HostOs) -> EngineManifest {
        EngineManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            abi_version: 1,
            engine_id: String::from("linux64-master-abc"),
            os,
            channel: reims_core::contracts::Channel::Master,
            source_revision: String::from("abc"),
            qemu_revision: String::from("def"),
            rom_filename: String::from("Custom.rom"),
            video_prop_name: String::from("custom-video-disable"),
            ovmf_code_size: 2_097_152,
            supported_macos_range: String::from("26.4+"),
            va_driver_name: Some(String::from("nvidia")),
            va_drivers_relpath: Some(String::from("vaapi")),
            files: BTreeMap::new(),
            guest_kexts: Vec::new(),
        }
    }

    fn spec(os: HostOs, graphics: GraphicsMode) -> VmSpec {
        let mut vm = VmConfig::new("macos-tahoe");
        vm.graphics = graphics;
        vm.usb_devices = vec![UsbDevice {
            vid: String::from("1234"),
            pid: String::from("5678"),
        }];
        VmSpec {
            os,
            engine_id: String::from("linux64-master-abc"),
            manifest: manifest(os),
            vm,
            installer_image: Some(String::from("BaseSystem.img")),
            qmp_port: 4444,
            bridge_port: 8080,
            shader_cache_dir: String::from("../../cache/shaders/linux64"),
            disable_video_decode: false,
        }
    }

    fn has_arg(command: &CommandSpec, needle: &str) -> bool {
        command.args.iter().any(|arg| arg.contains(needle))
    }

    #[test]
    fn linux_v1_uses_the_fixed_pci_map() {
        let adapter = EngineAdapter::for_abi(1).expect("adapter");
        let command = adapter
            .build(&spec(HostOs::Linux, GraphicsMode::Reims))
            .expect("command");

        assert_eq!(
            command.program,
            "../../engines/linux64-master-abc/bin/qemu-system-x86_64"
        );
        assert_eq!(command.working_dir, "vms/macos-tahoe");
        assert!(has_arg(&command, "max-ram-below-4g=1G"));
        assert!(has_arg(&command, "dump-guest-core=off"));
        assert!(has_arg(&command, "threads=1"));
        assert!(has_arg(&command, "base=utc,clock=host"));
        assert!(has_arg(&command, "memory-backend-memfd"));
        assert!(has_arg(&command, "prealloc-threads=4"));
        assert!(has_arg(&command, "ICH9-LPC.disable_s3=1"));
        assert!(has_arg(
            &command,
            "readonly=on,file=../../engines/linux64-master-abc/firmware/OVMF_CODE.fd"
        ));
        assert!(has_arg(&command, "if=pflash,format=raw,file=OVMF_VARS.fd"));
        assert!(has_arg(&command, "bus=ide.0,unit=0"));
        assert!(has_arg(&command, "bus=ide.1,unit=0"));
        assert!(has_arg(&command, "nvme,addr=0xa"));
        assert!(has_arg(&command, "Custom.rom"));
        assert!(has_arg(&command, "addr=0x4,netdev=net0,ctrl_vq=off"));
        assert!(has_arg(&command, "host=10.222.0.2"));
        assert!(has_arg(
            &command,
            "addr=0x5,conditional-intr-mapping=on,p2=7,p3=7"
        ));
        assert!(has_arg(
            &command,
            "usb-host,bus=xhci.0,vendorid=0x1234,productid=0x5678"
        ));
        assert!(has_arg(&command, "addr=0x7,streams=1"));
        assert!(has_arg(&command, "addr=0x8,streams=1,input-streams=1"));
        assert!(has_arg(&command, "isa-applesmc,osk="));
        assert!(has_arg(&command, "tcp:127.0.0.1:4444,server=on,wait=off"));
        assert_eq!(
            command.env.get("LIBVA_DRIVER_NAME").map(String::as_str),
            Some("nvidia")
        );
    }

    #[test]
    fn windows_v1_shares_memory_and_uses_windows_binary() {
        let adapter = EngineAdapter::for_abi(1).expect("adapter");
        let command = adapter
            .build(&spec(HostOs::Windows, GraphicsMode::Reims))
            .expect("command");
        assert!(command.program.ends_with("qemu-system-x86_64.exe"));
        assert!(has_arg(
            &command,
            "memory-backend-ram,id=pc.ram,size=8192M,share=on"
        ));
        assert!(!has_arg(&command, "dump-guest-core=off"));
        assert!(has_arg(&command, "-display"));
        assert!(command.env.contains_key("LIBVA_DRIVERS_PATH"));
        assert!(!command.env.contains_key("XDG_CACHE_HOME"));
    }

    #[test]
    fn safe_mode_caps_the_display_and_drops_reims_device() {
        let mut input = spec(HostOs::Linux, GraphicsMode::SvgaSafe);
        input.vm.resolution.width = 1280;
        input.vm.resolution.height = 720;
        let adapter = EngineAdapter::for_abi(1).expect("adapter");
        let command = adapter.build(&input).expect("command");
        assert!(has_arg(&command, "vmware-svga,addr=0x3"));
        assert!(!has_arg(&command, "reims-graphics-pci"));
        assert!(!has_arg(&command, "-display"));
        assert!(!has_arg(&command, "-vga"));
    }

    #[test]
    fn video_property_comes_from_the_manifest() {
        let mut input = spec(HostOs::Linux, GraphicsMode::Reims);
        input.disable_video_decode = true;
        input.manifest.rom_filename = String::from("Other.rom");
        let adapter = EngineAdapter::for_abi(1).expect("adapter");
        let command = adapter.build(&input).expect("command");
        assert!(has_arg(&command, "Other.rom"));
        assert!(has_arg(&command, ",custom-video-disable"));
    }

    #[test]
    fn lavapipe_sets_the_loader_override() {
        let adapter = EngineAdapter::for_abi(1).expect("adapter");
        let command = adapter
            .build(&spec(HostOs::Linux, GraphicsMode::Lavapipe))
            .expect("command");
        assert_eq!(
            command
                .env
                .get("VK_LOADER_DRIVERS_SELECT")
                .map(String::as_str),
            Some("lavapipe")
        );
    }

    #[test]
    fn unsupported_abi_is_rejected() {
        assert!(matches!(
            EngineAdapter::for_abi(2),
            Err(AdapterError::UnsupportedAbi { version: 2 })
        ));
    }

    #[test]
    fn mismatched_os_is_rejected() {
        let adapter = EngineAdapter::for_abi(1).expect("adapter");
        let mut input = spec(HostOs::Windows, GraphicsMode::Reims);
        input.os = HostOs::Linux;
        assert!(matches!(
            adapter.build(&input),
            Err(AdapterError::InvalidSpec(_))
        ));
    }

    #[test]
    fn hidpi_reaches_the_graphics_device() {
        let mut input = spec(HostOs::Linux, GraphicsMode::Reims);
        input.vm.hidpi = HiDpi::TwoX;
        let adapter = EngineAdapter::for_abi(1).expect("adapter");
        let command = adapter.build(&input).expect("command");
        assert!(has_arg(&command, ",hidpi=2x"));
    }
}
