# Agent operating instructions

muniment desktop is the local app for muniment, the company system of record.
This repo is the Tauri shell, the Rust runtime service, the Pi sidecar and the
on-device voice stack, with the local graph embedded in the runtime. The graph
and the grooming report are the product. The harness is a component. `SPEC.md`
is the slice of the plan this repo owns and the rules a reviewer holds a diff
against. `ROADMAP.md` lists the outcomes. Local mode needs no account.

## Verification loop

Run these commands in order from the repository root before every push:

1. `cargo fmt --manifest-path src-tauri/core/Cargo.toml --check`
2. `cargo clippy --manifest-path src-tauri/core/Cargo.toml --all-targets --locked -- -D warnings`
3. `cargo test --manifest-path src-tauri/core/Cargo.toml --locked --features network-tests`
4. `scripts/check-steering.sh .`

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
roadmap phase is deleted, not marked done. Caps: `SPEC.md` 400 lines,
`DESIGN.md` 250, `ROADMAP.md` 150, `AGENTS.md` 120. State lives in Plane and
history lives in git.
