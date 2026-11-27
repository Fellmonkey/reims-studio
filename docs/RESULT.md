# Delivered Result

Reims Studio v1.0 publishes a launcher, engine bundles, assets and two JSON indexes. The user receives a portable folder that runs without compilation.

## Release artifacts

| Artifact | Description |
|---|---|
| `reims-studio.exe` | Windows x64 launcher, MSVC, static CRT, UTF-8 and `longPathAware`, `asInvoker` |
| `reims-studio` | Linux x64 launcher, Ubuntu 22.04 glibc baseline |
| Engine archives | Per OS, channel and pinned revision; SHA256 in `index.json` |
| `index.json` | Available engines, checksums, supported macOS range |
| `signatures.json` | Updatable crash-signature dictionary |
| Guest assets | OVMF, option ROM, OpenCore template, guest kexts |
| `vk-gpu-select` | Static Vulkan layer for identical GPU IDs |

## First-run flow

1. Start the launcher. The UI runs on `tiny-skia`; the crash sentinel clears after three seconds.
2. Select the engine channel in `settings.toml`. Windows uses `development`, Linux uses `master`.
3. Run Pre-Flight checks: virtualization, filesystem, memory, GPU and display depth.
4. Create a VM. A thin 128 GiB disk goes directly on NVMe. RAM, vCPU count, resolution, HiDPI, MAC and USB IDs live in `vm.toml`.
5. Attach the macOS installer, boot with `ScanPolicy=983811`, then follow the Post-Install checklist.

## Post-Install checklist

- In Recovery Terminal run `defaults write "/Volumes/<Disk>/Library/Preferences/.GlobalPreferences" METAL_ERROR_MODE -int 5`.
- Use **Eject Installer** to detach the installer, switch `ScanPolicy` from `983811` to `983299`, and change OpenCore `Timeout` from `5` to `2` with `ShowPicker=true`.

## Diagnostics

The debug panel works over TCP QMP and unbuffered `stderr`. It provides shutdown, reset, kill, screendump, USB hotplug, Safe Mode, GPU isolation and verbose boot.

Telemetry shows host and VM RAM, GPU VRAM, shader count and present mode on a canvas.

The log reader shows 60–80 lines in one text block with deduplication, pause, filters, a page slider and copy buttons. No scrolling widget is used.

The crash decoder exports a `.zip` report with logs, VM configuration, engine manifest and decoded signature.

## Known limitations

- `FAT32` is rejected. `exFAT` needs periodic compaction.
- Windows needs the VA-API module for video acceleration. Video encoding is unavailable on Windows; NVDEC is Linux-only.
- `wgpu` is disabled below 24-bit color depth. 16-bit RDP keeps `tiny-skia`.
- Linux kernels 6.13–7.1 require a manual override and are unsupported with AMD GPUs.
- The GPU selection layer is ignored when the launcher runs elevated.
- GPU driver bugs are tracked in the signature list, not fixed in the launcher.

## Acceptance criteria

- A VM created on Windows boots on Linux from the same folder, with paths over 260 characters and Cyrillic names.
- macOS survives snapshot rollback with correct RTC and HTTPS.
- `Compact QCOW2` runs only with at least 1.1× free space and warns about internal snapshot loss.
- No orphaned `reims-vgpu-compile` processes remain after shutdown.
- Logs rotate after a GPU crash and the UI stays responsive.
