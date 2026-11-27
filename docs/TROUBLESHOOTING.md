# Troubleshooting (User Guide)

## Black screen or crash right after starting the VM

Use **layer isolation**, from the outside in.

1. Enable **Safe Mode** (`vmware-svga`, 1080p maximum).
   - If macOS still does not boot, the problem is not graphics. On Windows check that virtualization is enabled and not `Pending Reboot`, try removing `,enforce` or changing the CPU model, and boot verbose to see where startup stops. On Linux check kernel 7.2+, `/dev/kvm` access and `kvm.ignore_msrs=Y`. Also verify that OVMF code and variables form a matching pair and that `ScanPolicy` is `983811` during installation.
   - If Safe Mode boots but the normal mode fails, the problem is in the GPU/Vulkan/driver layer.
2. Check that the VM uses a fixed `q35` topology with `max-ram-below-4g=1G`, a paired OVMF firmware and the engine ROM from the active bundle. On Linux with AMD GPUs, `/dev/udmabuf` must be writable by the `kvm` group.
3. Run once in **GPU isolation mode** (`lavapipe`).
   - If it works there but fails on the physical card, the fault is in the GPU driver or shader translation. Search the crash-signature list for the specific GPU family.
   - If two GPUs are present and the wrong one is selected, start the launcher without administrator rights and check the Vulkan loader selection in a diagnostic log.

## macOS installed but behaves incorrectly

| Symptom | Check |
|---|---|
| Black screen or broken wallpaper after Tahoe | Apply `METAL_ERROR_MODE -int 5` again in Recovery |
| Intermittent network / DHCP on macOS 26/27 | The network device must keep `ctrl_vq=off` |
| No sound, or Safari / YouTube reports an error | Two sound devices must be present and audio must not be muted in the guest |
| Host CPU at 1000–1600% idle | CPU topology must use `threads=1` |
| Slow disk on Windows | Add the VM folder to Defender exclusions; avoid `FAT32`, compact `exFAT` images periodically |
| Launcher UI freezes while reading logs | Keep `tiny-skia` active and use the single text block with deduplication |

## Missing DLL or GLIBC error

These errors mean the engine bundle does not match the host. Use an official release for your platform rather than a locally assembled bundle. On Linux, a `GLIBC_2.xx not found` message indicates a bundle built for a newer distribution.

## Reporting a problem

Use **Export Bug Report** in the debug panel. It packages logs, active VM configuration, engine manifest and the decoded crash signature without exposing host credentials.
