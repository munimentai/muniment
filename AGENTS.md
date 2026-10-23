# Agent operating instructions

Muniment desktop is the local harness for the user's models, tools and work.
Phase one delivers the free desktop and its public release. Phase two
adds cloud availability with paid accounts. The Tauri shell, Rust runtime,
Pi sidecar and on-device voice stack serve the desktop without a Muniment account.
`SPEC.md` defines current behavior.

## Repository layout

- `core/` is the shared Rust core: its own Cargo workspace (`core/Cargo.toml`)
  with the crates under `core/crates/`, the runtime pins in `core/pins/`, and the
  protocol fixtures in `core/protocol-fixtures/`. `core/AGENTS.md` holds its rules.
- `src-tauri/` is the desktop's Cargo workspace: the Tauri app plus the
  `integration`, `cli`, `acp` and `runtime` crates. It builds the core crates by path.
- `src/` is the desktop UI. `browser-control/`, `scripts/` and `test/` serve the desktop.

Core owns the product logic: runtime, runs, storage, routing, models, pins and
protocols. The desktop is the UI layer and the platform host. Put a behavior in
core when another host needs it, and keep the desktop code to the window, the UI
and the host services that core reaches through its ports.

## Portable boundary

No core crate may depend on Tauri, on a `src-tauri` crate, or on a desktop-only
module, and no file under `core/` names `src-tauri`. Desktop behavior reaches core
through port-owned values and traits. `core/docs/core-boundary.md` lists every core
crate and module, and `scripts/check-core-boundary.sh` enforces it. Run that check
when you add an import between modules or crates.

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

CI owns the full test suite, Rust formatting and lint checks, and UI copy checks.
Use focused local checks to develop and debug the changed code. Do not run the
full suite locally as a mandatory step before each push. Wait for the required
CI checks and each applicable native platform check before merging.

CI reuses a successful PR result on main only when the tested Git tree matches.
Missing proof or a different tree runs the checks. Platform and feature variants
keep separate coverage. The steering check remains the local file hygiene check.

The nightly workflow
runs on demand as the release candidate proof. Targeted platform runs support
repairs; a full run proves the final candidate before stable promotion. A merge
does not finish the release: installed behavior, distribution and live download
links must pass. Stable releases reuse the exact verified packages.

The format check fails CI when formatting would create any diff. For focused Rust checks, use `src-tauri/Cargo.toml` and each changed package name. For core, run the checks in `core/AGENTS.md`.

## Windows code

Windows-specific code CANNOT be compiled or tested in the agent's Linux checkout. The Desktop compile preflight on CI is the only Windows check. Re-read every changed `#[cfg(windows)]` code path by hand before pushing. Never assume a green local build covers that code.

## Releases

One version covers the desktop app, the CLI, the server and the runtime image.
`version` under `[workspace.package]` in `core/Cargo.toml` is its source.
`src-tauri/Cargo.toml`, `package.json` and `package-lock.json` keep copies, and
`python3 scripts/version.py check` fails CI when one differs. Run
`python3 scripts/version.py set X.Y.Z` to change all of them. Each release is one
`vX.Y.Z` tag, which a maintainer dispatches as described in `README.md`. It
carries the desktop files, the pins, the signed server archives and the update
feed of headless runtimes.

## Commits

Use conventional prefixes, scoped by path: `feat(core):` or `fix(core):` for a
change under `core/`, and an unscoped or `(desktop)` prefix for the rest. `feat:`
is minor, `fix:` and `perf:` are patch, and `!` or a `BREAKING CHANGE:` trailer is
major. Before 1.0 a breaking change is minor. `core/scripts/release.py` plans the
next version from these prefixes. Keep core and desktop changes in separate
commits.

## CI logs

CI runs the workflows in `.forgejo/workflows/`. A maintainer dispatches the nightly and stable promotion workflows on `main`. They publish to GitHub Releases on `munimentai/muniment` with the `GH_RELEASE_TOKEN` secret. Installed subscription acceptance (`subscriptions.yml`) runs on GitHub Actions. CI logs land in the Actions run linked from the pull request's checks. Open the failed `Desktop compile preflight (windows)` check. Expand `Check (windows) via desktop-ci`, then read the first compiler error and its command output.

## Steering files

This repo's steering files are `AGENTS.md`, `README.md`, `SPEC.md`,
`DESIGN.md`, `core/AGENTS.md` and `core/README.md`. `docs/` holds technical
documentation about the app, and `core/docs/` about the core, such as
`core/docs/core-boundary.md`. Decision records are not kept in this
repository. Steering files and `docs/` describe the app, not product or business
decisions. They are instruction, not record: present tense, stating what is. No dates, ticket ids, commit shas, or
pull request numbers. No history phrases: "decided", "superseded", "previously", "no longer".
No ledger file under any name: no open-items,
build-history, decision-log, handoff, journal, notes, or todo file. Caps:
`SPEC.md` 600 lines, `DESIGN.md` 250, `AGENTS.md` and `core/AGENTS.md` 120. Issues track work and git holds history.
`CONTRIBUTING.md`, `SECURITY.md` and `LICENSE.md` define public participation,
private vulnerability reporting and licensing.
