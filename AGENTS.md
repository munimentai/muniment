# Agent operating instructions

Muniment desktop is the local harness for the user's models, tools and work.
Phase one delivers the free desktop and its public release. Phase two
adds cloud availability with paid accounts. The Tauri shell, Rust runtime,
Pi sidecar and on-device voice stack serve the desktop without a Muniment account.
`SPEC.md` defines current behavior and `ROADMAP.md` lists remaining outcomes.

## Local work and optional features

Local desktop work uses local commits. Do not push without an explicit request.
Preserve unrelated work and user data. Use simple commit messages and preserve the contributor identity.
Run `scripts/check-steering.sh .` before each commit.
`CLAUDE.md` imports this file and carries no separate product rules.

`src/feature-flags.js` defines the independent cloud and company-record flags.
Both default off. Gate controls, Settings sections and keyboard entry points.
Keep provider sign-in, routing, memory, files, terminal and browser available.
A hidden feature must not erase its stored data. UI flags are not authorization.
Test all four flag combinations and the default local startup path.

## Verification loop

Run these commands in order from the repository root before every push:

1. `cargo fmt --manifest-path src-tauri/core/Cargo.toml --check`
2. `cargo clippy --manifest-path src-tauri/core/Cargo.toml --all-targets --locked -- -D warnings`
3. `cargo test --manifest-path src-tauri/core/Cargo.toml --locked --features network-tests`
4. `npm run lint:copy`, which reads every string in `src` and `src-tauri` as UI copy
5. `scripts/check-steering.sh .`

The format check fails CI when formatting would create any diff. For other Rust crates, use `src-tauri/Cargo.toml` and each changed package name.

## Windows code

Windows-specific code CANNOT be compiled or tested in the agent's Linux checkout. The Desktop compile preflight on CI is the only Windows check. Re-read every changed `#[cfg(windows)]` code path by hand before pushing. Never assume a green local build covers that code.

## CI logs

CI logs land in the GitHub Actions run linked from the pull request's Checks tab. Open the failed `Desktop compile preflight (windows)` check. Expand `Check (windows) via desktop-ci`, then read the first compiler error and its command output.
## Steering files

This repo's steering files are `AGENTS.md`, `README.md`, `SPEC.md`,
`ROADMAP.md`, `DESIGN.md`, and `docs/decisions/`. They are instruction, not
record: present tense, stating what is. No dates, ticket ids, commit shas, or
pull request numbers. No history phrases: "decided", "superseded", "previously", "no longer".
No ledger file under any name: no open-items,
build-history, decision-log, handoff, journal, notes, or todo file. A finished
roadmap phase is deleted, not marked done. Caps: `SPEC.md` 600 lines,
`DESIGN.md` 250, `ROADMAP.md` 150, `AGENTS.md` 120. Issues track work and git holds history.
`CONTRIBUTING.md`, `SECURITY.md` and `LICENSE.md` define public participation,
private vulnerability reporting and licensing.
