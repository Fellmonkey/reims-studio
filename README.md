# Reims Studio

Reims Studio is a portable launcher, engine manager and diagnostic center for macOS Tahoe on Windows and Linux with `reims-vgpu` and a patched QEMU fork.

The launcher contains no emulator, Metal translator or video decoder. Those live in the engine bundle.

```text
Launcher ── CLI / env / stderr / QMP ──> Engine bundle
   │                                       │
   ├─ GUI on tiny-skia                     ├─ qemu-system-x86_64
   ├─ host checks and VM setup             ├─ reims-vgpu-compile
   └─ diagnostics                          └─ firmware and guest assets
```

## Guarantees

- Zero compilation on the user machine. Engine archives are 150–300 MB `.tar.zst` files from GitHub Releases.
- The UI uses `tiny-skia`: ~15–25 MB RAM idle, 0 MB VRAM, 0% GPU, no redraw loop. It works in 16-bit RDP.
- `portable.flag` marks a folder that moves between Windows and Linux, Intel and AMD, without admin rights. Paths are UTF-8 with `longPathAware`. `FAT32` is not supported.
- The VM uses a fixed `q35` map, UTC RTC with host clock, `threads=1` and NVMe at `addr=0xa`.

Idle figures are measured on a clean Windows 11 host with the VM stopped.

## Supported hosts

| Host | Requirement |
|---|---|
| Windows 10/11 x64 | MSVC build, UTF-8 and `longPathAware` manifest, `asInvoker` |
| Linux x86-64 | glibc baseline from Ubuntu 22.04 |

## Documentation

- [Documentation index](docs/README.md)
- [Product architecture](docs/ARCHITECTURE.md)
- [Delivered result](docs/RESULT.md)
- [Troubleshooting](docs/TROUBLESHOOTING.md)

