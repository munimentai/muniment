# Core agent instructions

The root `AGENTS.md` covers the repository: layout, commits, releases and
steering files. This file adds the rules for `core/`, the shared Rust core.

The desktop workspace builds the core crates by path. Headless hosts run
the router gateway (`muniment-router serve`) or the runtime server
(`muniment-server serve`) from a `vX.Y.Z` release and read its `pins.toml`.
`version` under `[workspace.package]` in `Cargo.toml` is the one release version
of the repository. Every crate inherits it.
`README.md` describes each crate. `docs/` holds technical documentation about
the core, such as `docs/core-boundary.md`, which the boundary check reads.

## Boundary

No crate may depend on Tauri, on a desktop crate, or on a desktop-only module.
No file under `core/` names a desktop directory, and no core crate has a path dependency
outside `core/`. The desktop-only modules `auth`, `chat_grant` and
`browser_control` live in the desktop. No source file here may name them, and no
file has an exception. The host supplies desktop behavior through port-owned
values and traits. `scripts/check-core-boundary.sh` checks the inventory in
`docs/core-boundary.md` and every source edge.

`muniment-router` and `muniment-pins` never depend on `muniment-core`.
`muniment-core` re-exports them at the module paths callers use, such as
`muniment_core::model_router`. Keep those paths stable: the desktop imports
`muniment_core::...` throughout.

## Pins

`pins/pins.toml` is the one source for the Pi release, its platform assets with
size and SHA-256, the Pi extension packages, and the Claude Code version.
`pins/packages.bun.lock` locks the packages. The `muniment-pins` build script
compiles the file into constants, so a malformed file fails the build. Change
pins with `muniment-pins bump`, which rewrites both files, and run
`muniment-pins compat` before you commit. The daily pins update workflow does
both, runs the CI checks on Linux and macOS, then commits to `main`. The next
release carries the bump.

## Verification loop

CI runs formatting, clippy, tests on Linux and macOS, a Windows check, and the
boundary check for every change under `core/`. Run cargo from `core/` and use
focused local checks while you develop:

- `cargo test -p <crate>` for the crate you changed.
- `cargo fmt --all --check` before each commit.
- `scripts/check-core-boundary.sh` when you add an import between modules.

Core tests that spawn stub processes run with `--test-threads=1`. Keep edition,
dependency versions and `Cargo.lock` stable unless the change is a dependency
update.

## Windows code

Windows-only code cannot compile on a Linux or macOS host. The Windows CI job is
the only Windows check. Re-read every changed `#[cfg(windows)]` path by hand
before you push. A green local build does not cover that code.

## Release key

The release workflow attaches `pins/pins.toml`, `pins/packages.bun.lock` and the
signed `muniment-server` archives that `deploy/runtime/release-assets.sh` builds
to each `vX.Y.Z` release. It fails without the `MUNIMENT_RELEASE_SIGNING_KEY` secret or when
`RELEASE_PUBLIC_KEY` in `crates/server/src/release.rs` does not verify the
signature. Installed launchers verify every update against that public key, so
change it only together with the signing key, and never commit the private key.
