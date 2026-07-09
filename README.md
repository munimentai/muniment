# muniment-desktop

The muniment desktop client (Tauri v2). Private, closed source. See
SPEC.md / ROADMAP.md / DESIGN.md. **Lane closed until muniment-cloud
Phase 1 ships** — see ROADMAP.

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
one VM at a time, clone destroyed after. The GitHub workflow only runs the
structure smoke on the shared runners; desktop-ci workflow wiring lands
when the lane opens.

### macOS CI note (2026-07-09)

`.dmg` bundling is EXCLUDED from CI targets: Tauri's `bundle_dmg.sh` drives
Finder via AppleScript and needs a GUI session, which the SSH-only CI VMs
don't have (verified failing in the M0 shakeout; the `.app` bundle builds
fine). DMG creation is a release-time step — solve at first release
(hdiutil-based script or a GUI-session build), not in the CI gate.
