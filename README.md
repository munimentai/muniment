# muniment-desktop

The muniment desktop client (Tauri v2). Private, closed source. See
SPEC.md / ROADMAP.md / DESIGN.md. The muniment-cloud Phase 1 prerequisite is
live and the desktop client lane is active; see ROADMAP.md for current
implementation status.

## Build

```sh
npm ci
npm run tauri build        # or: npm run tauri dev
```

Platform prerequisites are pre-provisioned in the CI VM templates (rust
1.96 + node 24 everywhere; webkit2gtk-4.1 on linux; MSVC + Win11 SDK on
windows; CLT on macOS).

## CI

Real builds run on ephemeral pve01 VM clones via `desktop-ci`
(`ssh pve01 sudo desktop-ci <platform> --repo <url> --cmd '<build>'`),
one VM at a time, clone destroyed after. Pull requests are gated on the
structure smoke, Rust and JS unit tests, followed by sequential Linux, Windows,
and macOS builds via `desktop-ci`; pushes to `main` run the smoke only.
Nightly and manually dispatched release builds use the same serialized VMs and
replace the assets on the private repository's `nightly` pre-release.

The accepted [installed-nightly desktop E2E architecture](docs/decisions/0013-desktop-e2e-harness.md)
defines the Windows/Linux WDIO real-auth lane, macOS smoke-only contract, and
pinned-artifact lifecycle for later implementation.

## Stable releases

`package.json` is the single source of truth for the desktop version; Tauri reads
it through `src-tauri/tauri.conf.json`. The owner updates it before the nightly
build, then manually runs **Promote stable desktop release** with that nightly's
exact 40-character SHA and matching `vMAJOR.MINOR.PATCH`. Promotion requires
green CI and the finalized six-asset nightly, and copies those bytes without
rebuilding or changing `nightly`.

The owner assigns SemVer and promotes when a tested nightly is ready. Patches are
compatible bug or security fixes, minors add backward-compatible functionality,
and majors may break compatibility. A bad release is never overwritten: stop
package-manager publication, mark it yanked in the release title/body, and
promote a new patch. Delete a tag/release only when nothing was distributed and
the owner confirms it was accidental. Homebrew, WinGet, and other package-manager
manifests are published only after stable promotion succeeds.

### macOS CI note (2026-07-09)

`.dmg` bundling is EXCLUDED from CI targets: Tauri's `bundle_dmg.sh` drives
Finder via AppleScript and needs a GUI session, which the SSH-only CI VMs
don't have (verified failing in the M0 shakeout; the `.app` bundle builds
fine). DMG creation is a release-time step — solve at first release
(hdiutil-based script or a GUI-session build), not in the CI gate.
