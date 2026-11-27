#!/usr/bin/env python3
"""Build index.json from the engine archives published in a release."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from urllib.parse import quote


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def describe(engine_id: str) -> tuple[str, str]:
    os_name = "windows" if engine_id.startswith("win64-") else "linux"
    if "-development-" in engine_id:
        channel = "development"
    elif "-patched-" in engine_id:
        channel = "patched"
    else:
        channel = "master"
    return os_name, channel


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--assets", required=True, type=Path)
    parser.add_argument("--base-url", required=True)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--abi-version", type=int, default=1)
    parser.add_argument("--supported-macos-range", default="26.4+")
    parser.add_argument(
        "--changelog",
        type=Path,
        help="Commit summaries published with this engine build",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    changelog = (
        args.changelog.read_text(encoding="utf-8").strip()
        if args.changelog and args.changelog.is_file()
        else None
    )
    engines = []
    for archive in sorted(args.assets.glob("*.tar.zst")):
        engine_id = archive.name.removesuffix(".tar.zst")
        os_name, channel = describe(engine_id)
        engines.append(
            {
                "engine_id": engine_id,
                "os": os_name,
                "channel": channel,
                "abi_version": args.abi_version,
                "archive_sha256": sha256(archive),
                "archive_size": archive.stat().st_size,
                "url": f"{args.base_url.rstrip('/')}/{quote(archive.name)}",
                "supported_macos_range": args.supported_macos_range,
                **({"changelog": changelog} if changelog else {}),
            }
        )
    args.output.write_text(
        json.dumps({"schema_version": 1, "engines": engines}, indent=2) + "\n",
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()
