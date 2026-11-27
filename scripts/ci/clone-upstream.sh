#!/usr/bin/env bash
# Clones the pinned upstream engine source and its QEMU fork.
set -euo pipefail

: "${VGPU_REPO:?VGPU_REPO is required}"
: "${VGPU_REF:?VGPU_REF is required}"
: "${QEMU_REPO:?QEMU_REPO is required}"
: "${QEMU_REF:?QEMU_REF is required}"

git clone --depth 1 --branch "$VGPU_REF" "$VGPU_REPO" upstream/reims-vgpu
mkdir -p upstream/reims-vgpu/vendor
git clone --filter=blob:none "$QEMU_REPO" upstream/reims-vgpu/vendor/qemu
git -C upstream/reims-vgpu/vendor/qemu checkout "$QEMU_REF"
