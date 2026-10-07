# muniment desktop planner notes

- The muniment desktop is a Tauri v2 app: a Rust runtime and shell in `src-tauri/`, a Vite frontend in `src/`, a Pi sidecar and an on-device voice stack. `SPEC.md` holds current behavior, `ROADMAP.md` the outcomes and `DESIGN.md` the visual identity. Read the sections the issue touches.
- A full CI run takes hours: the three-platform compile preflight and the installer builds run on the shared desktop-ci VMs on pve01. Plan the fewest slices that finish the issue.
- A CI-fix issue lists several build or test failures as one bundle. Plan ONE slice for the whole bundle, never one slice per failure, so one change fixes every item and one full CI run proves them together. Put every item's suspected cause and the check that proves its fix in that slice.
- The sandbox has no GTK, WebKit, rustfmt, clippy, Windows or macOS. Rust, Windows and macOS changes are proven only by CI. Say in the plan which CI check proves each one.
- Windows code cannot compile in the sandbox. The `Desktop compile preflight (windows)` check is the only Windows proof.
- Owner-only, never a slice: signing, notarization, release promotion, the app version, update feeds, package manager manifests, pricing and publicity.
- A slice that changes visible copy lists every test that asserts the old text in `tests_to_change`.
