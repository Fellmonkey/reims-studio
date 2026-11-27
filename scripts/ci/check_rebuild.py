#!/usr/bin/env python3
"""Reports which engine build inputs changed since the last published build.

The engine factory spends about twenty minutes per platform compiling QEMU and
its tools. This check compares the inputs that can actually change the engine
with the state published by the previous build, so a run can say explicitly why
a rebuild is needed (or why it is not).
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path

# Upstream files and trees that feed the engine bundle. Changes outside this
# list (docs, tests, examples) do not affect a published engine.
UPSTREAM_WATCH = (
    "Makefile",
    "Cargo.toml",
    "scripts/install-ctl.sh",
    "xtask/src",
    "guest/prebuilt",
    "firmware/prebuilt",
    "docs/qemu.md",
    "docs/known-issues.md",
)

# Files in this repository that change how the engine is built or described.
LOCAL_WATCH = (
    "scripts/ci/clone-upstream.sh",
    "scripts/ci/collect-bundle.sh",
    "scripts/ci/build-layer.sh",
    "scripts/ci/generate-manifest.sh",
    "scripts/ci/pack.sh",
    "scripts/make_manifest.py",
    "scripts/make_index.py",
    ".github/workflows/build-engines.yml",
)

SCHEMA_VERSION = 1


def digest_path(path: Path) -> str:
    """Hashes a file or the sorted contents of a directory tree."""
    digest = hashlib.sha256()
    if path.is_file():
        digest.update(path.read_bytes())
    else:
        for child in sorted(p for p in path.rglob("*") if p.is_file()):
            digest.update(str(child.relative_to(path)).encode())
            digest.update(child.read_bytes())
    return digest.hexdigest()


def collect(upstream: Path, local: Path, args: argparse.Namespace) -> dict[str, str]:
    """Returns the current fingerprint of every watched input."""
    sources: dict[str, str] = {}
    for relative in UPSTREAM_WATCH:
        path = upstream / relative
        sources[f"upstream/{relative}"] = digest_path(path) if path.exists() else "absent"
    for relative in LOCAL_WATCH:
        path = local / relative
        sources[f"local/{relative}"] = digest_path(path) if path.exists() else "absent"
    sources["ref/vgpu"] = args.vgpu_ref
    sources["ref/qemu"] = args.qemu_ref
    sources["ref/qemu_windows"] = args.qemu_ref_windows
    sources["build/channel"] = args.channel
    return sources


def previous_sources(path: Path | None) -> dict[str, str]:
    """Loads the previous build state, or returns an empty dict."""
    if path is None or not path.is_file():
        return {}
    try:
        state = json.loads(path.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        return {}
    if state.get("schema_version") != SCHEMA_VERSION:
        return {}
    value = state.get("sources", {})
    return value if isinstance(value, dict) else {}


def verdict(
    current: dict[str, str], previous: dict[str, str], force: bool
) -> tuple[bool, list[str]]:
    """Returns (rebuild needed, human-readable report lines)."""
    lines: list[str] = ["| Input | Result |", "| --- | --- |"]
    changed: list[str] = []
    for name in sorted(current):
        before = previous.get(name)
        if before is None:
            status = "new build input" if previous else "no previous build"
            changed.append(name)
        elif before == current[name]:
            status = "unchanged"
        else:
            status = "changed"
            changed.append(name)
        lines.append(f"| {name} | {status} |")
    for name in sorted(previous.keys() - current.keys()):
        lines.append(f"| {name} | removed |")
        changed.append(name)
    if not previous:
        lines.append("")
        lines.append("No previous build state was published, so a build is required.")
    rebuild = force or not previous or bool(changed)
    lines.append("")
    if rebuild:
        lines.append(f"Rebuild needed: yes ({len(changed)} input(s) differ).")
    else:
        lines.append("Rebuild needed: no; all watched inputs match the last build.")
    return rebuild, lines


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--upstream", required=True, type=Path)
    parser.add_argument("--local", required=True, type=Path)
    parser.add_argument("--state", type=Path, help="previous build-state.json")
    parser.add_argument("--output", required=True, type=Path, help="new build-state.json")
    parser.add_argument("--report", type=Path, help="optional markdown report")
    parser.add_argument("--vgpu-ref", required=True)
    parser.add_argument("--qemu-ref", required=True)
    parser.add_argument("--qemu-ref-windows", required=True)
    parser.add_argument("--channel", required=True)
    parser.add_argument("--force", action="store_true", help="rebuild even if nothing changed")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    current = collect(args.upstream, args.local, args)
    rebuild, lines = verdict(current, previous_sources(args.state), args.force)
    args.output.write_text(
        json.dumps(
            {
                "schema_version": SCHEMA_VERSION,
                "revision": args.vgpu_ref,
                "sources": current,
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    report = "\n".join(lines) + "\n"
    if args.report:
        args.report.write_text(report, encoding="utf-8")
    print(report)
    outputs = os.environ.get("GITHUB_OUTPUT")
    if outputs:
        with Path(outputs).open("a", encoding="utf-8") as handle:
            handle.write(f"rebuild={'true' if rebuild else 'false'}\n")
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with Path(summary).open("a", encoding="utf-8") as handle:
            handle.write("## Engine rebuild check\n\n" + report)


if __name__ == "__main__":
    main()
