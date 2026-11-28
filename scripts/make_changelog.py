#!/usr/bin/env python3
"""Build a detailed changelog from engine commits.

The Markdown output uses `## <sha> · <date>` sections so the launcher can
render each commit as an expandable accordion item. Override the whole file
with a prepared changelog by passing --source-file in CI.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from datetime import datetime, timezone


def gh(*arguments: str) -> str:
    """Runs gh with the caller's GH_TOKEN and returns stdout."""
    result = subprocess.run(
        ("gh", *arguments),
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
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


def commits(repo: str, previous: str, current: str, limit: int) -> list[dict[str, str]]:
    """Returns non-merge commits with author date and full message body."""
    query = (
        '.commits[]? | select(.parents | length == 1) | '
        '{sha: .sha, date: .commit.author.date, message: .commit.message}'
        if previous
        else '.[] | select(.parents | length == 1) | '
        '{sha: .sha, date: .commit.author.date, message: .commit.message}'
    )
    endpoint = (
        f"repos/{repo}/compare/{previous}...{current}" if previous
        else f"repos/{repo}/commits?sha={current}&per_page={limit}"
    )
    output = gh("api", endpoint, "--paginate", "--jq", query)
    result: list[dict[str, str]] = []
    seen: set[str] = set()
    for line in output.splitlines():
        if not line.strip():
            continue
        try:
            item = json.loads(line)
        except json.JSONDecodeError:
            continue
        message = item.get("message", "")
        subject = message.strip().splitlines()[0].strip() if message.strip() else ""
        if not subject or subject.startswith("Merge ") or subject in seen:
            continue
        seen.add(subject)
        body = "\n".join(message.strip().splitlines()[1:]).strip()
        result.append({
            "sha": item.get("sha", "")[:12],
            "date": item.get("date", ""),
            "subject": subject,
            "body": body,
        })
    return result[:limit]


def render(repo: str, previous: str, current: str, entries: list[dict[str, str]]) -> str:
    """Formats commits into launcher-friendly Markdown."""
    generated = datetime.now(timezone.utc).replace(microsecond=0).isoformat()
    title = f"Engine commits since {previous[:12]}" if previous else f"Engine commits up to {current[:12]}"
    lines = [
        "# Engine changelog",
        f"Generated: {generated}",
        f"Range: {previous or 'initial'}..{current}",
        f"Repository: {repo}",
        "",
        f"## {title}",
        "",
    ]
    if not entries:
        lines.append("No engine commits in this range.")
    for entry in entries:
        lines.append(f"## {entry['sha']} · {entry['date']}")
        lines.append(f"### {entry['subject']}")
        lines.append("")
        lines.append(entry["body"] or "_No commit body._")
        lines.append("")
    return "\n".join(lines) + "\n"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", help="clone URL or owner/name of the engine repo")
    parser.add_argument("--from", dest="previous", default="", help="previous engine revision")
    parser.add_argument("--to", dest="current", default="", help="current engine revision")
    parser.add_argument("--output", required=True, type=argparse.FileType("w", encoding="utf-8"))
    parser.add_argument("--source-file", help="use a prepared changelog instead of querying GitHub")
    parser.add_argument("--limit", type=int, default=30)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    if args.source_file:
        with open(args.source_file, encoding="utf-8") as handle:
            args.output.write(handle.read())
        return
    if not args.repo or not args.current:
        raise SystemExit("--repo and --to are required without --source-file")
    repo = normalize_repo(args.repo)
    try:
        entries = commits(repo, args.previous, args.current, args.limit)
    except (subprocess.CalledProcessError, OSError) as error:
        args.output.write(f"# Engine changelog\n\n_Changelog unavailable: {error}_\n")
        return
    args.output.write(render(repo, args.previous, args.current, entries))


if __name__ == "__main__":
    sys.exit(main())
