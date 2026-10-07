# muniment desktop implementer notes

- Run commands from the repo root. `npm ci` already ran.
- The sandbox has no GTK, WebKit, rustfmt, clippy, Windows or macOS. Do not run `npm test`, `npm run tauri`, a full `cargo build` of the desktop shell, or the e2e suites. CI runs them.
- Run focused checks only: a single vitest file with `npx vitest run --root . <file>`, or a focused `cargo test --manifest-path src-tauri/Cargo.toml --package <name>` when the crate builds without GTK.
- Re-read every changed `#[cfg(windows)]` and `#[cfg(target_os = "macos")]` path by hand. No local build covers that code.
- For a CI-fix bundle, fix every listed failure in this one change and say in your summary which CI check proves each fix.
- Copy law: `node test/ui-copy-lint.mjs src src-tauri browser-control` checks user-facing text.
- Never change signing, notarization, the app version or release promotion.
- Never edit `.factory/`, `.github/`, `.forgejo/`, `AGENTS.md`, `CLAUDE.md` or a lockfile, even when a gate fails. The protected-path gate rejects the whole change. Report a gate you cannot pass in your summary instead.
