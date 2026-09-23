<p align="center">
  <a href="https://muniment.ai"><img src="docs/assets/muniment.svg" width="72" height="72" alt="Muniment" /></a>
</p>

<h1 align="center">Your models. Working together.</h1>

<p align="center">
  The desktop harness for your accounts, tools, and files.<br />
  Route requests between models. Balance work across accounts. Create agents and artifacts.
</p>

<p align="center">
  <a href="https://muniment.ai">Website</a> ·
  <a href="https://muniment.ai/docs/">Docs</a> ·
  <a href="https://muniment.ai/docs/install/">Get the app</a> ·
  <a href="https://muniment.ai/docs/start/">Getting started</a>
</p>

<p align="center">Free desktop · Source under FSL.</p>

[![Muniment desktop with project threads, Jev selected, and a launch readiness artifact beside the chat. Sample data.](docs/assets/desktop-workspace.jpg)](https://muniment.ai)

## One place to work

Bring your provider accounts, API keys, or local models. Muniment keeps the
conversation beside your files, terminal, browser, and interactive artifacts.
No Muniment account is required.

| | |
| --- | --- |
| **Your models** | Choose a model or let Jev route the request. Balance work across provider accounts. |
| **Your tools** | Connect MCP servers, install skills and plugins, and choose what each turn can use. |
| **Your workspace** | Organize projects and threads. Work with files, folders, a terminal, and an embedded browser. |
| **Your agents and artifacts** | Give reusable agents a purpose. Keep interactive outputs beside the conversation. |
| **Your preferences** | Use on-device voice, persistent memory, themes, and separate conversation and header fonts. |

Start with the [desktop guide](https://muniment.ai/docs/).
The [install guide](https://muniment.ai/docs/install/) lists release availability and platform requirements.

## Built for the desktop

Muniment runs locally on macOS, Windows, and Linux. Use your own provider accounts
without a Muniment account. Cloud services are optional future features.

## Issues and source

Bug reports and feature requests are welcome. The project does not accept outside
code contributions. Read [CONTRIBUTING.md](CONTRIBUTING.md) for the issue guide
and [SECURITY.md](SECURITY.md) to report a vulnerability privately.
See the [test architecture](docs/decisions/0013-desktop-e2e-harness.md) for verification details.

Muniment uses [FSL-1.1-ALv2](LICENSE.md), a Fair Source license. Each version
converts to Apache 2.0 after two years. Third-party components retain their own licenses.

## Download

- [macOS — Apple silicon and Intel](https://github.com/munimentai/muniment/releases/download/v0.0.1/nightly-84e8c1b74dc2030449dc91de890691627f659ce9-macos-muniment.pkg)
- [Windows — x64 installer](https://github.com/munimentai/muniment/releases/download/v0.0.1/nightly-84e8c1b74dc2030449dc91de890691627f659ce9-windows-muniment_0.0.1_x64_en-US.msi)
- [Ubuntu / Debian — x64](https://github.com/munimentai/muniment/releases/download/v0.0.1/nightly-84e8c1b74dc2030449dc91de890691627f659ce9-linux-muniment.deb)
- [Linux — x64 AppImage](https://github.com/munimentai/muniment/releases/download/v0.0.1/nightly-84e8c1b74dc2030449dc91de890691627f659ce9-linux-muniment_0.0.1_amd64.AppImage)

See the [install guide](https://muniment.ai/docs/install/) for platform requirements
and [all downloads](https://github.com/munimentai/muniment/releases/latest) for alternate installers.

### Homebrew

The official Homebrew cask is not available yet. Use the macOS download above.
Once Homebrew accepts the cask, install with:

```sh
brew install --cask muniment
```

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
with root ownership and mode 4755 when the host restricts user namespaces.
On Ubuntu, install the DEB first. Its installer configures the helper. The
AppImage uses that installed helper. Chromium checks its sandbox API version.
On hosts such as Omarchy that allow user namespaces, the AppImage runs directly.
The app keeps Chromium sandboxing enabled. The macOS signed bundle uses a private
Keychain bridge for its own cookie key. Key access fails without prompting
when the app cannot access that key.

The platform scripts and release installer jobs package CEF resources, helpers,
and license notices. Windows external agent IPC, Wayland, popups,
and downloads are not implemented. Native macOS page accessibility needs a fix.
The `cef-smoke` feature runs browser checks in a disposable profile. Do not use
its state override with a personal profile.
