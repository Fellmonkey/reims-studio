# Product Architecture

Reims Studio is a host launcher plus CI-built engine bundles. The launcher never links against the engine.

```text
Reims Studio (host)
├── Launcher / GUI          — setup, VM wizard, engine channels, debug panel
├── Platform adapter        — host checks, process control, VM command generation
├── Engine manager          — downloads, verifies, unpacks and updates engines
├── Guest bridge            — file portal and clipboard sync on 10.222.0.2
└── Optional GPU layer      — only for identical GPU IDs or old loaders
        ↕ CLI / env / stderr / QMP
Engine bundle (CI-built, pinned)
├── qemu-system-x86_64, qemu-img
├── reims-vgpu-compile
├── Vulkan and VA-API runtime files
└── firmware and guest assets
```

The process boundary isolates the launcher from upstream changes.

## Engines and channels

Engines are immutable bundles published on GitHub Releases. Each bundle has a `manifest.json` with checksums, source revisions, ROM filename, video-disable property, VA-API driver, supported macOS range and an ABI version. The launcher reads this manifest instead of hardcoding names.

| Channel | Default on | Purpose |
|---|---|---|
| `master` | Linux | Stable upstream revision |
| `development` | Windows | Latest WHPX and iGPU fixes |
| `patched` | opt-in | Reviewable local patches |

The channel is set in `settings.toml`. Updates download to `.part` files, verify SHA256, then replace the active engine folder.

## Portable layout

```text
ReimsStudio/
├── reims-studio.exe | reims-studio
├── portable.flag, ui_booting.flag, settings.toml
├── engines/<os>-<channel>-<sha>/
│   ├── manifest.json
│   ├── bin/
│   ├── vaapi/
│   └── firmware/
├── cache/
│   ├── installers/
│   └── shaders/<os>-<vid>-<pid>-<drv>/
└── vms/<name>/
    ├── vm.lock, vm.toml, smbios.toml, host_profiles/
    ├── disk.qcow2, OpenCore.qcow2, OVMF_VARS.fd
    ├── snapshots/<tag>/
    └── logs/
```

## Runtime model

The VM uses `q35`, `max-ram-below-4g=1G`, UTC RTC with host clock, `threads=1`, paired OVMF firmware, `ide.0` for OpenCore, `ide.1` for the installer and NVMe at `addr=0xa` for the system disk.

Safe Mode (`vmware-svga`, 1080p maximum) isolates boot-chain faults. `lavapipe` isolates GPU-driver faults. Input capture releases with `Ctrl+Alt+Win` on Windows or `Ctrl+Alt+Super` on Linux.

Process lifetime uses a Windows Job Object with `KILL_ON_JOB_CLOSE` or a Linux process group with `killpg`. QMP shutdown runs before forced termination.

The guest bridge is an authenticated HTTP portal on the SLIRP gateway `10.222.0.2` for file transfer and clipboard sync.

## Host requirements

| Windows | Linux |
|---|---|
| Virtualization enabled | Kernel 7.2+ for AMD guest memory protection |
| `WHvGetCapability` = `Active` | `/dev/kvm` and `/dev/udmabuf` accessible |
| UsbDk or WinUSB | `DISPLAY` or `WAYLAND_DISPLAY` available |
| Defender exclusions for the VM folder | `kvm.ignore_msrs=Y` |
| 24-bit depth for `wgpu` | `llvmpipe` excluded except for diagnostics |

Vulkan 1.3 with `VK_EXT_external_memory_host` is required. With one GPU nothing is overridden. With several GPUs the host loader variables select the target. A standalone selection layer is used only for identical VID:DID pairs or old loaders, and it is ignored under UAC elevation.

## Product guarantees

- Users never compile. Engine bundles are verified before use.
- The UI consumes no VRAM and no GPU time while idle.
- A portable folder survives moves between hosts and filesystems except `FAT32`.
- Snapshots are offline copies of OpenCore, OVMF variables and VM configuration. Compaction warns before dropping internal snapshots.
- Crash decoding reads an updatable signature list. New failures are recognized without rebuilding the launcher.
