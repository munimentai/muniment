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
