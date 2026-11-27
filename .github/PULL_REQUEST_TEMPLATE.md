## What this changes and why

<!-- Title: Area: what changed (#issue) — e.g. "Platform: fixes QMP watchdog timeout (#12)".
     One change per pull request. Facts only, no water. -->

Fixes #

<!-- Anything larger than a small fix needs an issue we've agreed on first (see CONTRIBUTING.md). -->

## How you tested it

<!-- On a real machine: exact commands and output, before and after. For a UI change, add a screenshot. -->

```text

```

Tested on: <!-- Windows 11 / Linux x.x, Rust version -->

## How you found it

- [ ] I use Reims Studio and ran into this myself
- [ ] I found it by reading the code or comparing with upstream
- [ ] Someone else reported it (link the issue above)

AI tools used to write this change: <!-- yes / no; if yes, which parts and why -->

## Checklist

- [ ] Commits are signed and verified (GitHub shows "Verified")
- [ ] The author is a person; an AI agent is not the contributor (see [CONTRIBUTING.md](../CONTRIBUTING.md))
- [ ] `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo nextest run --workspace --profile ci --locked`, and `cargo test --workspace --doc --locked` pass
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --locked`, `cargo deny check`, and `cargo audit --deny warnings` pass
- [ ] Public docs match the delivered behavior
- [ ] If this fixes a bug, a regression check exists and is named in `tests/regressions.tsv`
- [ ] Hot-path changes include before and after benchmarks
