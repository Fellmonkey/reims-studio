#!/usr/bin/env python3
"""Local end-to-end harness for the Reims Studio launcher.

Simulates an engine build that finishes after a delay, then verifies that the
built launcher sees the bundle through its headless --check/--e2e modes.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
import time
from pathlib import Path

ENGINE_ID = "win64-development-e2e"
ARCHIVE_BYTES = b"engine archive"


def write(path: Path, content: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content)


def write_initial_state(root: Path) -> None:
    write(
        root / "settings.toml",
        b'schema_version = 1\nchannel = "development"\nbridge_port = 8080\nqmp_port = 4444\n',
    )
    write(
        root / "vms/macos-tahoe/vm.toml",
        b'schema_version = 1\nname = "macos-tahoe"\nram_mib = 8192\nvcpus = 4\n'
        b'disk_bus = "nvme"\ninstaller_attached = false\ngraphics = "reims"\n'
        b'audio = true\nverbose = false\nresolution = { width = 1920, height = 1080 }\n'
        b'hidpi = "1x"\nmac = "52:54:00:11:22:33"\nusb_devices = []\n'
        b'scan_policy = 983299\n',
    )


def write_build_result(root: Path) -> None:
    manifest = {
        "schema_version": 1,
        "abi_version": 1,
        "engine_id": ENGINE_ID,
        "os": "windows",
        "channel": "development",
        "source_revision": "local-e2e",
        "qemu_revision": "local-e2e",
        "rom_filename": "ReimsGraphics.rom",
        "video_prop_name": "video-disable",
        "ovmf_code_size": 2097152,
        "supported_macos_range": "26.4+",
        "va_driver_name": None,
        "va_drivers_relpath": None,
        "files": {},
        "guest_kexts": [],
    }
    digest = hashlib.sha256(ARCHIVE_BYTES).hexdigest()
    index = {
        "schema_version": 1,
        "engines": [
            {
                "engine_id": ENGINE_ID,
                "os": "windows",
                "channel": "development",
                "abi_version": 1,
                "archive_sha256": digest,
                "archive_size": len(ARCHIVE_BYTES),
                "url": "https://example.invalid/engine.tar.zst",
                "supported_macos_range": "26.4+",
            }
        ],
    }
    write(
        root / f"engines/{ENGINE_ID}/manifest.json",
        (json.dumps(manifest, indent=2) + "\n").encode(),
    )
    write(root / "index.json", (json.dumps(index, indent=2) + "\n").encode())
    write(root / f"cache/{ENGINE_ID}.tar.zst", ARCHIVE_BYTES)


def run_launcher(launcher: Path, root: Path, mode: str) -> tuple[int, str]:
    environment = dict(os.environ)
    environment["REIMS_PORTABLE_DIR"] = str(root)
    result = subprocess.run(
        [str(launcher), mode],
        capture_output=True,
        text=True,
        env=environment,
        check=False,
    )
    return result.returncode, result.stdout + result.stderr


def expect(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"E2E FAIL: {message}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--launcher", required=True, type=Path)
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--wait", type=float, default=3.0)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    launcher = args.launcher.resolve()
    root = args.root.resolve()
    expect(launcher.is_file(), f"launcher not found: {launcher}")

    write_initial_state(root)
    print("[1/5] initial state written; engine build started")

    early_code, early_output = run_launcher(launcher, root, "--check")
    expect(early_code != 0, "launcher saw an engine before the build finished")
    expect("FAIL" in early_output, f"unexpected early output: {early_output}")
    print("[2/5] launcher correctly reports no engine before the build")

    time.sleep(args.wait)
    write_build_result(root)
    print(f"[3/5] build finished after {args.wait:.1f}s; bundle published")

    check_code, check_output = run_launcher(launcher, root, "--check")
    print(check_output.strip())
    expect(check_code == 0, f"--check failed: {check_output}")
    expect(ENGINE_ID in check_output, "launcher does not see the built engine")
    print("[4/5] launcher sees the built engine")

    e2e_code, e2e_output = run_launcher(launcher, root, "--e2e")
    print(e2e_output.strip())
    expect(e2e_code == 0, f"--e2e failed: {e2e_output}")
    expect("bridge_port: 8080" in e2e_output, "bridge settings not visible to the launcher")

    archive = root / f"cache/{ENGINE_ID}.tar.zst"
    archive.write_bytes(b"tampered")
    bad_code, bad_output = run_launcher(launcher, root, "--e2e")
    expect(bad_code != 0, "corrupted archive was accepted")
    expect("FAIL" in bad_output, "corrupted archive did not produce a failure marker")
    print("[5/5] corrupted archive rejected by --e2e")
    print("E2E OK")


if __name__ == "__main__":
    main()
