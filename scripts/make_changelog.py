#!/usr/bin/env python3
"""Build a changelog from commit subjects in the engine source repository."""

from __future__ import annotations

import argparse
import os
import subprocess
import sys


def gh(*arguments: str) -> str:
    """Runs gh with the caller's GH_TOKEN and returns stdout."""
    result = subprocess.run(
        ("gh", *arguments),
        capture_output=True,
        text=True,
        check=True,
        env=os.environ.copy(),
    )
    return result.stdout


def normalize_repo(value: str) -> str:
    """Converts a clone URL into the owner/name form used by the GitHub API."""
    repo = value.strip()
    for prefix in ("https://github.com/", "http://github.com/", "git@github.com:"):
        if repo.startswith(prefix):
            repo = repo[len(prefix) :]
            break
    return repo.removesuffix(".git")


def subjects(repo: str, previous: str, current: str, limit: int) -> list[str]:
    """Returns non-merge commit subjects in the requested range."""
    if previous:
        output = gh(
            "api",
            f"repos/{repo}/compare/{previous}...{current}",
            "--paginate",
            "--jq",
            '.commits[]? | .commit.message | split("\\n")[0]',
        )
    else:
        output = gh(
            "api",
            f"repos/{repo}/commits?sha={current}&per_page={limit}",
            "--jq",
            '.[] | select(.parents | length == 1) | .commit.message | split("\\n")[0]',
        )
    result: list[str] = []
    seen: set[str] = set()
    for message in output.splitlines():
        subject = message.strip().splitlines()[0].strip() if message.strip() else ""
        if not subject or subject.startswith("Merge ") or subject in seen:
            continue
        seen.add(subject)
        result.append(subject)
    return result[:limit]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", required=True, help="clone URL or owner/name of the engine repo")
    parser.add_argument("--from", dest="previous", default="", help="previous engine revision")
    parser.add_argument("--to", dest="current", required=True, help="current engine revision")
    parser.add_argument("--output", required=True, type=argparse.FileType("w", encoding="utf-8"))
    parser.add_argument("--limit", type=int, default=30)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    repo = normalize_repo(args.repo)
    current = args.current[:12]
    if args.previous:
        title = f"Engine commits since {args.previous[:12]}"
    else:
        title = f"Latest engine commits up to {current}"
    lines = [f"# {title}", ""]
    try:
        commits = subjects(repo, args.previous, args.current, args.limit)
    except (subprocess.CalledProcessError, OSError) as error:
        lines.append(f"_Changelog unavailable: {error}_")
    else:
        lines.extend(f"- {subject}" for subject in commits)
        if not commits:
            lines.append("- No engine commits in this range.")
    args.output.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    sys.exit(main())
