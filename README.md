# Muniment

Muniment is a free desktop harness for your models, tools and local work.
Connect provider accounts, API keys or local models. Work with projects,
agents, artifacts, files, a terminal and an embedded browser in one app.
Memory, permissions and on-device voice support the same thread surface.
No Muniment account is required.

Phase one is the desktop app and its public release. Phase two adds cloud
availability with paid accounts while the desktop remains useful on its own.
The company record and cloud account surfaces are hidden by default through
independent build flags. The desktop uses [FSL-1.1-ALv2](LICENSE.md), a Fair Source license. Each version
converts to Apache 2.0 after two years. Read [Contributing](CONTRIBUTING.md)
for participation and [Security](SECURITY.md) for private vulnerability reports.
Third-party components retain the licenses listed in the bundled notices.

This repo contains the Tauri v2 shell, Rust runtime service, Pi sidecar and
on-device voice stack. The local macOS build runs the runtime as an app child.
It preserves the user's workspace, credentials, companies and logs.

`SPEC.md` defines desktop behavior and the rules for reviewing a change. `ROADMAP.md` lists the outcomes this repo owns. `DESIGN.md` holds
the tokens and the visual laws. `docs/decisions/` holds the ADRs.

See [THREAT_MODEL.md](THREAT_MODEL.md) for the desktop runtime trust boundary.

Configure the Linux ACP adapter in [Zed or JetBrains](docs/acp-editors.md).

## Build

Use Node.js 24, Rust 1.96, Go, and the native build tools for your platform.
macOS requires Xcode Command Line Tools. Windows requires MSVC and the Windows SDK.
Linux requires the Tauri WebKitGTK development libraries and the CEF dependencies
listed in `scripts/test-cef-linux.sh`.

```sh
npm ci
npm run dev                               # frontend preview
npm run build                             # frontend production assets
npm test                                  # unit and browser tests
```

Native packaging includes a Rust runtime, Go reader, Pi sidecar and CEF helpers.
The scripts under `scripts/` and `.github/` define each platform's packaging.
`scripts/build-macos-local.mjs` is a maintainer signing tool that requires private
signing infrastructure. It is not required for frontend work or Rust unit tests.
Use a disposable machine for the Linux and Windows packaging test scripts.
They install dependencies and configure browser sandbox permissions.

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

## Extend

Settings → Extend manages MCP servers, skills and plugins. Search the MCP
catalog of remote servers by name, publisher or category. Filter by installation
or setup requirements. Servers connect directly to provider endpoints. Directory-hosted relays stay out. Sort by popularity or name. Custom servers support remote URLs and
local commands, OAuth sign-in and stored bearer tokens.

Install skills and compatible plugins from a GitHub repository, local folder,
or ZIP/TAR archive. Review the package contents before installation. Updates
retain a previous version for rollback. Plugins can supply skills, MCP servers
and Pi code extensions. Unsupported plugin components produce an error.

The composer ellipsis opens MCPs, Plugins and Skills branches. A paperclip adds files and folders.
Type `/` or `\` to insert a skill or plugin slash command into the message.
MCP switches and commands apply to one turn. Optional automatic selection uses
the configured classifier and respects disabled extensions. Preferences includes
separate Conversation, Headers and Records font controls.

Refresh the bundled public catalog with `python3 scripts/refresh-mcp-catalog.py`.
Refresh provider logos with `python3 scripts/refresh-extension-icons.py`.
Catalog entries describe provider services, not accounts included with Muniment.

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

Native CI uses disposable platform machines through the `desktop-ci` driver.
Maintainers provide the runner and release credentials. Contributor checks use
repository-local commands and do not require access to that infrastructure.
Pull requests run smoke checks, unit tests and applicable native build checks.
Nightly builds also test the installed app on Linux, Windows and macOS.
A successful installer build alone does not prove the app works.

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

The desktop checks for updates on launch and hourly. Builds enable the updater with
`MUNIMENT_UPDATER_PUBLIC_KEY` set to the Tauri public signing key at build time.
`MUNIMENT_UPDATER_ENDPOINT` can override the default public GitHub release feed at
`https://github.com/munimentai/muniment/releases/latest/download/latest.json`.
An unconfigured build offers no update. The app verifies the download before it
shows the green Update control. Click installs and restarts. Active replies,
voice capture and unsaved file edits block that control.

Publish `latest.json` only after the matching signed updater packages exist.
Use a final signed macOS `.app.tar.gz`, a Linux `.AppImage` and a Windows NSIS
installer, each with its Tauri `.sig`. Sign after all packaging and code signing.
The manifest uses Tauri platform keys (`darwin-aarch64`, `darwin-x86_64`,
`linux-x86_64`, `windows-x86_64`), each with an HTTPS `url` and `signature`,
plus the matching SemVer `version`. Keep the private update signing key outside
source control and provide it only to the release signing job. Package-manager
installs on Linux use their package manager instead of the in-app updater.

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
