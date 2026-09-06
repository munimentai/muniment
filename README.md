# muniment-desktop

The local app for muniment, the company system of record.

Muniment writes down the graph a company already is: people, organizations,
deals, threads, tickets, commitments, and every relationship between them, in
one schema of eight tables, with every extracted fact bound to the sentence it
came from. Importers fill the graph from the tools a business already runs. A
grooming report over the resolved graph shows the duplicates, the dead fields,
the seat waste and the contradictions that nothing else reports.

This repo is the Tauri v2 shell, the Rust runtime service, the Pi sidecar and
the on-device voice stack. The graph lives in SQLite inside the runtime and is
reached through one read-only SQL tool from the harness the user already runs.
Local mode needs no account. Sign-in gates cloud features only. The published
local product carries FSL-1.1-Apache-2.0.

`SPEC.md` is this repo's slice of the plan and the rules a reviewer holds a
diff against. `ROADMAP.md` lists the outcomes this repo owns. `DESIGN.md` holds
the tokens and the visual laws. `docs/decisions/` holds the ADRs.

See [THREAT_MODEL.md](THREAT_MODEL.md) for the desktop runtime trust boundary.

Configure the Linux ACP adapter in [Zed or JetBrains](docs/acp-editors.md).

## Build

```sh
npm ci
npm run tauri build        # or: npm run tauri dev
```

Platform prerequisites are pre-provisioned in the CI VM templates (rust
1.96 + node 24 everywhere; webkit2gtk-4.1 on linux; MSVC + Win11 SDK on
windows; CLT on macOS).

## Test

```sh
npm test                                   # frontend unit tests
cargo test --manifest-path src-tauri/core/Cargo.toml --locked
test/smoke.sh                              # structure smoke
scripts/check-steering.sh .                # steering files
```

## CI

Real builds run on ephemeral pve01 VM clones via `desktop-ci`
(`ssh pve01 sudo desktop-ci <platform> --repo <url> --cmd '<build>'`),
one VM at a time, clone destroyed after. Pull requests are gated on the
structure smoke, the steering check, Rust and JS unit tests, followed by
sequential Linux, Windows, and macOS builds via `desktop-ci`; pushes to `main`
run the smoke only. Nightly and manually dispatched release builds use the same
serialized VMs and replace the assets on the private repository's `nightly`
pre-release.

The accepted [installed-nightly desktop E2E architecture](docs/decisions/0013-desktop-e2e-harness.md)
defines the Windows/Linux WDIO real-auth lane, macOS smoke-only contract, and
pinned-artifact lifecycle.

## Stable releases

`package.json` is the single source of truth for the desktop version; Tauri reads
it through `src-tauri/tauri.conf.json`. The owner updates it before the nightly
build, then manually runs **Promote stable desktop release** with that nightly's
exact 40-character SHA and matching `vMAJOR.MINOR.PATCH`. Promotion requires
green CI and the finalized seven-asset nightly, and copies those bytes without
rebuilding or changing `nightly`.

The owner assigns SemVer and promotes when a tested nightly is ready. Patches are
compatible bug or security fixes, minors add backward-compatible functionality,
and majors may break compatibility. A bad release is never overwritten: stop
package-manager publication, mark it yanked in the release title/body, and
promote a new patch. Delete a tag/release only when nothing was distributed and
the owner confirms it was accidental. WinGet and other stable package manifests
are published only after stable promotion succeeds. The Homebrew tap tracks the
nightly channel separately, and the nightly publish step skips cleanly while
the tap is unseeded.

See [macOS packages](docs/macos-packages.md) for `.pkg` deployment and current signing status.
See [Homebrew](docs/homebrew.md) to install the current macOS nightly from the Muniment tap.

### macOS CI note

`.dmg` bundling is excluded from CI targets: Tauri's `bundle_dmg.sh` drives
Finder via AppleScript and needs a GUI session, which the SSH-only CI VMs do
not have. The `.app` bundle builds fine. DMG creation is a release-time step
(an hdiutil-based script or a GUI-session build), not part of the CI gate.
