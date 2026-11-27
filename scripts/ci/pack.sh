#!/usr/bin/env bash
# Packs the bundle into a .tar.zst archive. Publishing happens in publish-index.
set -euo pipefail

: "${CHANNEL:?CHANNEL is required}"
: "${GITHUB_SHA:?GITHUB_SHA is required}"
: "${ENGINE_PREFIX:?ENGINE_PREFIX is required}"

cd upstream/reims-vgpu
id="${ENGINE_PREFIX}-${CHANNEL}-${GITHUB_SHA}"
(cd bundle && tar -cf - .) | zstd -T0 -19 > "$id.tar.zst"
sha256sum "$id.tar.zst"
printf 'archive=%s.tar.zst\n' "$id" >> "${GITHUB_OUTPUT:-/dev/null}"
