#!/usr/bin/env bash
# Generates the engine manifest from the built bundle.
set -euo pipefail

: "${CHANNEL:?CHANNEL is required}"
: "${GITHUB_SHA:?GITHUB_SHA is required}"
: "${OS_NAME:?OS_NAME is required}"
: "${QEMU_REF:?QEMU_REF is required}"
: "${ENGINE_PREFIX:?ENGINE_PREFIX is required}"
: "${VIDEO_PROP:?VIDEO_PROP is required}"

bundle="upstream/reims-vgpu/bundle"
rom="$(basename "$(ls "$bundle"/firmware/*.rom | head -n1)")"
if [ -f "$bundle/firmware/OVMF_CODE.fd" ]; then
  ovmf="$(stat -c %s "$bundle/firmware/OVMF_CODE.fd")"
else
  ovmf=2097152
fi

python_bin=python3
if command -v python >/dev/null 2>&1; then
  python_bin=python
fi

args=(
  --bundle "$bundle"
  --output "$bundle/manifest.json"
  --engine-id "${ENGINE_PREFIX}-${CHANNEL}-${GITHUB_SHA}"
  --os "$OS_NAME"
  --channel "$CHANNEL"
  --abi-version 1
  --source-revision "$GITHUB_SHA"
  --qemu-revision "$QEMU_REF"
  --rom-filename "$rom"
  --video-prop-name "$VIDEO_PROP"
  --ovmf-size "$ovmf"
  --supported-macos-range "26.4+"
  --set-toml upstream/reims-vgpu/guest/prebuilt/set.toml
)
if [ -n "${VA_DRIVER:-}" ]; then
  args+=(--va-driver-name "$VA_DRIVER" --va-drivers-relpath vaapi)
fi

"$python_bin" scripts/make_manifest.py "${args[@]}"
