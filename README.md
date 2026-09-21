# muniment-desktop

Muniment is a free desktop harness for your models, tools and local work.
Connect provider accounts, API keys or local models. Work with projects,
agents, artifacts, files, a terminal and an embedded browser in one app.
Memory, permissions and on-device voice support the same thread surface.
No Muniment account is required.

Phase one is the desktop app and its open-source release. Phase two adds cloud
availability with paid accounts while the desktop remains useful on its own.
The company record and cloud account surfaces are hidden by default through
independent build flags. The public release requires a license aligned with
that goal, contributor guidance, a security policy and third-party notices.

This repo contains the Tauri v2 shell, Rust runtime service, Pi sidecar and
on-device voice stack. The local macOS build runs the runtime as an app child.
It preserves the user's workspace, credentials, companies and logs.

`SPEC.md` is this repo's slice of the plan and the rules a reviewer holds a
diff against. `ROADMAP.md` lists the outcomes this repo owns. `DESIGN.md` holds
the tokens and the visual laws. `docs/decisions/` holds the ADRs.

See [THREAT_MODEL.md](THREAT_MODEL.md) for the desktop runtime trust boundary.

Configure the Linux ACP adapter in [Zed or JetBrains](docs/acp-editors.md).

## Build

```sh
npm ci
node scripts/build-macos-local.mjs          # signed local macOS app
bash scripts/test-cef-linux.sh             # disposable Linux test machine
powershell -File scripts/test-cef-windows.ps1 # disposable Windows test machine
```

Platform prerequisites are pre-provisioned in the CI VM templates (rust
1.96 + node 24 everywhere; webkit2gtk-4.1 on linux; MSVC + Win11 SDK on
windows; CLT on macOS).

## Feature flags

The default build hides Muniment cloud and the company record. Set either
flag independently before `npm run dev`, `npm run build` or the local build:

```sh
VITE_MUNIMENT_CLOUD=true node scripts/build-macos-local.mjs
VITE_MUNIMENT_COMPANY_RECORD=true node scripts/build-macos-local.mjs
```

Only `true` enables a flag. Omit both for the phase-one desktop. Flags take
effect at build time and have no user Settings switch. Cloud controls sign-in
and Account settings. Company record controls Record, its shortcut, and
Companies settings. Hidden Settings sections return to Models & routing.
Provider account connections and local routing remain available in every build.
These flags control UI availability, not backend authorization or data deletion.

## Embedded browser

The Tauri v2 shell hosts native CEF child views through `cef-rs`. It does not
use `tauri-runtime-cef`. The lockfile pins the browser packages. Websites have
no Tauri bridge. Browser and artifact profiles are separate. Agent access
is available while the browser view is open. Websites have no workspace or terminal access.
The title bar menu opens a shared tab panel for Browser, Files and Terminal.
Files use a nested tree and Seti type icons. The file menu supports rename, duplicate, copy and paste, paths, system reveal, and Trash. Text files open in Monaco with syntax highlighting and Cmd/Ctrl+S.
The editor preserves drafts across tabs and rejects saves over changed files. The editor supports UTF-8 files up to 2 MB.
Composer @ references search the selected Files folder, or the current project or session workspace.
The terminal retains one shell per context, started in its workspace folder.
Fixed-width tabs show browser URLs or folder names with fades. The tab strip
and shared full-path headers scroll horizontally without visible scrollbars.
HTML files written in chat appear in Artifacts. The app has no manual artifact editor.
The Unix agent endpoint is `~/.muniment/browser/agent.sock`.

CEF uses its platform sandbox. Windows starts through the CEF bootstrap and
loads the app DLL. Linux requires X11 and the packaged `chrome-sandbox` helper
with root ownership and mode 4755. The macOS signed bundle uses a private
Keychain bridge for its own cookie key. Key access fails without prompting
when the app cannot access that key.

The platform scripts package CEF resources, helpers, and license notices.
The standard release installer jobs do not package CEF. Do not use those jobs
to distribute this browser build. Windows external agent IPC, Wayland, popups,
and downloads are not implemented. Native macOS page accessibility needs a fix.
The `cef-smoke` feature runs browser checks in a disposable profile. Do not use
its state override with a personal profile.

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

The [installed-nightly desktop E2E architecture](docs/decisions/0013-desktop-e2e-harness.md)
defines pinned artifact installation plus WDIO chat, real sign-in, onboarding,
and cleanup on Linux, Windows, and macOS.
The macOS lane runs `macos.sh` for the pinned `.app` smoke and Proxmox
screendump, then `macos-wdio.sh` for the specs.
Cloud sign-in checks require a build with the cloud flag enabled. The default
build must prove local chat with both optional features hidden.
WDIO uses the embedded provider with separate E2E builds from the pinned
source, not the released artifacts.

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
