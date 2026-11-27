#!/usr/bin/env bash
# Builds the fallback Vulkan selection layer for the target triple.
set -euo pipefail

: "${TARGET:?TARGET is required}"

if [[ "${RUNNER_OS:-}" == "Windows" ]] && command -v cygpath >/dev/null 2>&1; then
  export PATH="$(cygpath -u "$USERPROFILE")/.cargo/bin:$PATH"
fi

cargo build --release -p vk-gpu-select --target "$TARGET" --locked
bundle="upstream/reims-vgpu/bundle"

if [[ "$TARGET" == *windows* ]]; then
  cp "target/$TARGET/release/vk_gpu_select.dll" "$bundle/bin/VkLayer_reims_gpu_select.dll"
  cp crates/vk-gpu-select/VkLayer_reims_gpu_select.json "$bundle/bin/"
else
  cp "target/$TARGET/release/libvk_gpu_select.so" "$bundle/bin/VkLayer_reims_gpu_select.so"
  cp crates/vk-gpu-select/VkLayer_reims_gpu_select_linux.json "$bundle/bin/VkLayer_reims_gpu_select.json"
fi
