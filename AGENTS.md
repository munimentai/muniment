# Agent operating instructions

## Verification loop

Run these commands in order from the repository root before every push:

1. `cargo fmt --manifest-path src-tauri/core/Cargo.toml --check`
2. `cargo clippy --manifest-path src-tauri/core/Cargo.toml --all-targets --locked -- -D warnings`
3. `cargo test --manifest-path src-tauri/core/Cargo.toml --locked --features network-tests`

The format check fails CI when formatting would create any diff. For other Rust crates, use `src-tauri/Cargo.toml` and each changed package name.

## Windows code

Windows-specific code CANNOT be compiled or tested in the agent's Linux checkout. The Desktop compile preflight on CI is the only Windows check. Re-read every changed `#[cfg(windows)]` code path by hand before pushing. Never assume a green local build covers that code.

## CI logs

CI logs land in the GitHub Actions run linked from the pull request's Checks tab. Open the failed `Desktop compile preflight (windows)` check. Expand `Check (windows) via desktop-ci`, then read the first compiler error and its command output.
