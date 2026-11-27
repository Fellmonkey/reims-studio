# Contributing to Reims Studio

Reims Studio is a portable launcher and diagnostic center for `reims-vgpu`. The launcher uses stable Rust. It talks to the engine as an external process; it never links against it.

## Prerequisites

- Rust stable via `rustup`, with `rustfmt` and `clippy`.
- Windows: MSVC Build Tools and Windows SDK. Linux: `build-essential` and `pkg-config`.
- `cargo-nextest`, `cargo-deny`, `cargo-audit`.

## Quick start

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --profile ci --locked
cargo test --workspace --doc --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --locked
cargo deny check
cargo audit --deny warnings
```

PowerShell uses `$env:RUSTDOCFLAGS = "-D warnings"; cargo doc ...`. `--locked` is mandatory.

## Core rules

- Do not add `reims-vgpu`, `metal2vulkan` or `reims-util` as Cargo dependencies.
- Do not edit upstream engines. New flags go into `EngineAdapter` and `manifest.abi_version`; new crashes go into `signatures.json`.
- Update public docs with user-facing changes. Development rules stay internal.
- Unit tests use fixtures. No network CDN in unit tests.

## Pull requests

- Title: `Area: what changed (#issue)`. The first word is the area: `CI`, `Core`, `Platform`, `Engine`, `Studio`, `Docs`, `Test`, `Release`. Use a third-person verb. Do not open an issue for your own work; do so only when the task comes from other people.
- One change per pull request. A pull request with several changes or without an agreed task is closed; reopen it after splitting.
- Anything larger than a small fix needs an agreed issue first.
- Commits are signed and verified. GitHub shows `Verified`. Unsigned commits are not accepted.
- An AI agent is not a contributor. A person is the author and owns the change. AI tools may be used, but the description must state whether they were used, which parts they wrote, and why.
- Every bug fix adds a named regression check that fails without the fix and passes with it. Name it in `tests/regressions.tsv` with the issue or pull request and one line on what broke. Removing a name from that file is an error.
- Structural changes in hot paths are measured before and after with `cargo bench` and criterion baselines. Determinism alone does not justify a rewrite.
- A bug report without a reproduction on a real machine may be closed. Include exact commands, a minimal `vm.toml`, and captured output.

## Review checklist

- [ ] One change, title in the required form.
- [ ] Commits are signed and verified.
- [ ] All seven commands in Quick start pass.
- [ ] Public docs match the delivered behavior.
- [ ] AI tool use is disclosed in the description.
- [ ] Bug fixes include a named regression check.
- [ ] Hot-path changes include before and after benchmarks.

By submitting a contribution you agree to the [Contributor License Agreement](CLA.md). Participation is governed by the [Code of Conduct](CODE_OF_CONDUCT.md).
