#!/usr/bin/env python3
"""Build an engine manifest by introspecting a freshly built bundle.

The launcher never hardcodes ROM, kext or device property names. CI discovers
them here from the built tree and the upstream ``set.toml``.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import tomllib
from pathlib import Path
from typing import Any


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def kext_names(value: Any) -> list[str]:
    """Collect kext names from any shape of upstream set.toml."""
    found: list[str] = []
    if isinstance(value, str):
        if ".kext" in value:
            found.append(value.replace("\\", "/"))
    elif isinstance(value, dict):
        for item in value.values():
            found.extend(kext_names(item))
    elif isinstance(value, (list, tuple)):
        for item in value:
            found.extend(kext_names(item))
    return found


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--engine-id", required=True)
    parser.add_argument("--os", required=True, choices=("windows", "linux"))
    parser.add_argument("--channel", required=True, choices=("master", "development", "patched"))
    parser.add_argument("--abi-version", required=True, type=int)
    parser.add_argument("--source-revision", required=True)
    parser.add_argument("--qemu-revision", required=True)
    parser.add_argument("--rom-filename", required=True)
    parser.add_argument("--video-prop-name", required=True)
    parser.add_argument("--ovmf-size", required=True, type=int)
    parser.add_argument("--supported-macos-range", required=True)
    parser.add_argument("--set-toml", type=Path)
    parser.add_argument("--va-driver-name")
    parser.add_argument("--va-drivers-relpath")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    bundle = args.bundle
    files = {
        path.relative_to(bundle).as_posix(): sha256(path)
        for path in sorted(bundle.rglob("*"))
        if path.is_file() and path.name != "manifest.json"
    }
    kexts: list[str] = []
    if args.set_toml is not None:
        with args.set_toml.open("rb") as handle:
            kexts = sorted(set(kext_names(tomllib.load(handle))))

    manifest = {
        "schema_version": 1,
        "abi_version": args.abi_version,
        "engine_id": args.engine_id,
        "os": args.os,
        "channel": args.channel,
        "source_revision": args.source_revision,
        "qemu_revision": args.qemu_revision,
        "rom_filename": args.rom_filename,
        "video_prop_name": args.video_prop_name,
        "ovmf_code_size": args.ovmf_size,
        "supported_macos_range": args.supported_macos_range,
        "va_driver_name": args.va_driver_name,
        "va_drivers_relpath": args.va_drivers_relpath,
        "files": files,
        "guest_kexts": kexts,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
