#!/usr/bin/env bash
# Builds the portable bundle from a compiled engine tree and checks QMP-capable
# QEMU devices. Works in both GitHub bash and MSYS2 bash.
set -euo pipefail
set -x

: "${GITHUB_OUTPUT:?GITHUB_OUTPUT is required}"

cd upstream/reims-vgpu
root="$(pwd)"
suffix="${BINARY_SUFFIX:-}"
qemu_name="qemu-system-x86_64${suffix}"
img_name="qemu-img${suffix}"
helper_name="reims-vgpu-compile${suffix}"

mkdir -p "$root/bundle/bin" "$root/bundle/firmware/guest" "$root/bundle/vaapi"
cp "build/qemu/$qemu_name" "$root/bundle/bin/"
cp "build/qemu/$img_name" "$root/bundle/bin/"
cp "target/dist-vulkan/bin/$helper_name" "$root/bundle/bin/"
cp firmware/prebuilt/*.rom "$root/bundle/firmware/"
cp -r guest/prebuilt/. "$root/bundle/firmware/guest/"

# CI only compiles tools. The launcher performs install-time work with them,
# just like upstream install-ctl.sh does on a user's machine.
util_ref="7867384c0ab703b4c6aae36cbbb1a35fcffc8321"
util_name="reims-util"
[[ "${RUNNER_OS:-}" == "Windows" ]] && util_name="reims-util.exe"
if command -v git >/dev/null 2>&1 && command -v cargo >/dev/null 2>&1; then
  git clone --depth 1 https://github.com/steelbrain/reims-util.git upstream/reims-util
  git -C upstream/reims-util checkout "$util_ref"
  (cd upstream/reims-util && cargo build --release --locked)
  cp "upstream/reims-util/target/release/$util_name" "$root/bundle/bin/"
  mkdir -p "$root/bundle/licenses"
  cp upstream/reims-util/LICENSE "$root/bundle/licenses/reims-util.LICENSE"
fi

for binary in "$qemu_name" "$img_name" "$helper_name"; do
  strip "$root/bundle/bin/$binary" || true
done

qemu="$root/bundle/bin/$qemu_name"
"$qemu" -device help > device-help.txt 2>&1 || true
grep -i -E 'reims|usb-host' device-help.txt | head -20 || true
grep -q reims-graphics-pci device-help.txt
grep -q usb-host device-help.txt

# QEMU prints indented `name=type` lines, not `name=<on|off>`.
"$qemu" -device reims-graphics-pci,help | head -40
props="$("$qemu" -device reims-graphics-pci,help | sed -n 's/^[[:space:]]*\([A-Za-z0-9_-][A-Za-z0-9_-]*\)=.*/\1/p')"
prop="$(printf '%s\n' "$props" | grep -Ei 'video|disable' | head -n1 || true)"
if [ -z "$prop" ]; then
  prop="$(printf '%s\n' "$props" | grep -v -E '^(addr|id|bus|romfile|multifunction|bootindex)$' | head -n1 || true)"
fi
test -n "$prop"

outputs="$GITHUB_OUTPUT"
if command -v cygpath >/dev/null 2>&1; then
  outputs="$(cygpath -u "$outputs")"
fi
printf 'video_prop=%s\n' "$prop" >> "$outputs"

if [[ "${RUNNER_OS:-}" == "Windows" ]]; then
  # QEMU links against clang64 runtime DLLs (pixman, zlib, glib, GTK, ...).
  # ldd gives the exact transitive set; fall back to the lib*.dll prefix when
  # ldd is unavailable so a fresh bundle never misses a loader dependency.
  copy_runtime_dlls() {
    local binary="$1"
    if command -v ldd >/dev/null 2>&1; then
      ldd "$binary" 2>/dev/null \
        | tr ' ' '\n' \
        | grep -E 'clang64[/\\]bin[/\\].*\.dll' \
        | sed 's#.*[/\\]##' \
        | sort -u \
        | while read -r name; do
            if [ -f "/clang64/bin/$name" ]; then
              cp -f "/clang64/bin/$name" "$root/bundle/bin/"
            fi
          done || true
    else
      cp /clang64/bin/lib*.dll "$root/bundle/bin/"
    fi
  }
  for binary in "$qemu_name" "$img_name" "$helper_name" "$util_name"; do
    [ -f "$root/bundle/bin/$binary" ] && copy_runtime_dlls "$root/bundle/bin/$binary"
  done
  # ldd on MSYS2 misses some transitive GTK dependencies (libxkbcommon, ...).
  # Copying every lib*.dll is cheap and makes the bundle genuinely self-contained.
  cp -f /clang64/bin/lib*.dll "$root/bundle/bin/"

  # VA-API is mandatory on Windows. Never ship the loader DLL.
  rm -f "$root/bundle/bin/vulkan-1.dll"
  cp /clang64/bin/libva*.dll "$root/bundle/vaapi/" 2>/dev/null || true
  cp /clang64/bin/*_drv_video.dll "$root/bundle/vaapi/"
  driver="$(basename "$(ls "$root/bundle/vaapi/"*_drv_video.dll | head -n1)")"
  printf 'va_driver=%s\n' "$driver" >> "$outputs"
else
  # glibc, libvulkan and Mesa/NVIDIA drivers stay on the host.
  find "$root/bundle/bin" \( -name 'libc.so*' -o -name 'libvulkan.so*' \) -delete
fi
