<p align="center">
  <a href="https://muniment.ai">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="docs/assets/pocket-fold-reverse.svg" />
      <img src="docs/assets/pocket-fold.svg" width="128" alt="Muniment" />
    </picture>
  </a>
</p>

<h1 align="center">Your models. Working together.</h1>

<p align="center">
  A free desktop app for your models, tools, and files. No Muniment account required.
</p>

<p align="center">
  <a href="https://muniment.ai">Website</a> ·
  <a href="https://muniment.ai/docs/">Docs</a> ·
  <a href="https://muniment.ai/docs/install/">Get the app</a> ·
  <a href="https://muniment.ai/docs/start/">Getting started</a>
</p>

<p align="center">Free desktop · Source under FSL.</p>

[![Muniment desktop with project threads, Jev selected, and a launch readiness artifact beside the chat. Sample data.](docs/assets/desktop-workspace.jpg)](https://muniment.ai)

## Features

- **Your models, together.** Connect provider accounts, API keys, or local models. Choose a model or let Jev route each request. Usage shows what each subscription window has left.
- **One workspace.** Keep projects, threads, files, a terminal, and a browser together.
- **Tools that fit your work.** Add MCP servers, skills, and plugins. Pick extra ones for a single turn.
- **Permission modes.** Each thread and agent runs in one of six modes, from Ask before changes to Bypass permissions. An approval is Allow once or Deny.
- **Agents and artifacts.** Create agents that run on a schedule, a webhook, or a tool check, and interactive outputs beside the conversation.
- **Your machines.** Pair other desktops, servers, and containers, and run a thread or an agent on any of them.
- **Memory and voice.** Keep context across threads, see where each memory came from, and dictate on-device with live words.

## Start

Download Muniment for your platform:

- [macOS — Apple silicon and Intel](https://github.com/munimentai/muniment/releases/download/v0.0.1/muniment-0.0.1-macos.pkg)
- [Windows — x64 installer](https://github.com/munimentai/muniment/releases/download/v0.0.1/muniment-0.0.1-windows_x64_en-US.msi)
- [Ubuntu / Debian — x64](https://github.com/munimentai/muniment/releases/download/v0.0.1/muniment-0.0.1-linux.deb)
- [Linux — x64 AppImage](https://github.com/munimentai/muniment/releases/download/v0.0.1/muniment-0.0.1-linux_amd64.AppImage)

See the [install guide](https://muniment.ai/docs/install/) for platform requirements
and [all downloads](https://github.com/munimentai/muniment/releases/latest) for alternate installers.

Open the app, [connect a provider](https://muniment.ai/docs/connections/), and start a thread.
Follow the [getting started guide](https://muniment.ai/docs/start/) for your first project.

### AppImage sandbox setup

Ubuntu 24.04 restricts unprivileged user namespaces. The AppImage needs a host sandbox helper on these systems, even without the DEB.
The AppImage includes a setup script and the Chromium sandbox helper.

1. Download the AppImage from the official release.
2. In an empty directory, extract its sandbox setup.

```sh
chmod +x /path/to/muniment.AppImage
/path/to/muniment.AppImage --appimage-extract usr/lib/muniment/cef/chrome-sandbox
/path/to/muniment.AppImage --appimage-extract setup-sandbox.sh
```

3. Install the helper with administrator access.

```sh
sudo /bin/sh squashfs-root/setup-sandbox.sh
```

4. Remove the extracted files.

```sh
rm -r squashfs-root
```

5. Run the original AppImage as your normal user.

The setup requires a setuid-enabled executable filesystem at `/usr/lib/muniment/cef`.
It keeps Chromium's sandbox enabled and leaves the host's user namespace policy unchanged.
Run setup for each AppImage update. Matching helpers need no changes.
If setup reports a different helper, keep the DEB helper when the DEB is installed.
For an AppImage-only installation, close Muniment and remove `/usr/lib/muniment/cef/chrome-sandbox` with `sudo rm` before setup.
To uninstall an AppImage-only installation, remove that helper and the AppImage.

### Homebrew

The official Homebrew cask is not available yet. Use the macOS download above.
Once Homebrew accepts the cask, install with:

```sh
brew install --cask muniment
```

## Repository layout

- `core/` is the shared Rust core: runtime, model router, pins, protocols and the
  headless servers. [core/README.md](core/README.md) describes its crates.
- `src-tauri/` is the desktop's Rust workspace. It builds the core crates by path.
- `src/` is the desktop UI. `scripts/` and `test/` hold the desktop's tooling and tests.

Core holds the logic that every host shares. The desktop is the UI layer on top of it.

## Releases

GitHub Releases on `munimentai/muniment` host every download. One `vX.Y.Z` tag
releases the desktop app, the CLI, the server and the runtime image together.
`core/Cargo.toml` names the version, and `scripts/version.py` keeps the desktop
copies in step. The CI workflows in `.forgejo/workflows/` build and publish them.
Only a maintainer dispatch of `main` runs a release job. Each job checks this in its first step and fails before any credential is read.

1. Set the version with `python3 scripts/version.py set X.Y.Z` and land it on `main`. `core/scripts/release.py` prints the next version.
2. Dispatch `nightly.yml` on `main`. It signs the three platform builds, uploads them to the `nightly` pre-release, and runs the installed checks.
3. Dispatch `subscriptions.yml` on GitHub with the nightly source SHA. Its native runners are on GitHub.
4. Dispatch `release.yml` on `main` with the source SHA, the version, and `dry_run` set. The dry run runs every check and creates nothing.
5. Dispatch `release.yml` again without `dry_run`. It creates the version tag in the source repository, and the mirror carries it to GitHub.
   The job then publishes the verified nightly bytes, `latest.json`, and `SHA256SUMS` as the stable release.
   Later jobs add the pins and the signed server archives, sign `SHA256SUMS` over every file, and push the runtime image.

The `GH_RELEASE_TOKEN` secret holds a fine-grained GitHub token with Contents write on `munimentai/muniment`.
Every GitHub Releases call uses it. The CI job token reaches only the source repository.

## Issues and license

Report a bug or request a feature by email to [support@muniment.ai](mailto:support@muniment.ai).
Outside code contributions are not accepted. See [CONTRIBUTING.md](CONTRIBUTING.md)
and [SECURITY.md](SECURITY.md) for issue and private vulnerability reports.

Muniment uses [FSL-1.1-ALv2](LICENSE.md), a Fair Source license.
Each version converts to Apache 2.0 after two years.
Third-party components retain their own licenses.

`npm run check:agent-dependencies` checks Claude Code, Pi, and extension pins against current releases and Pi peer ranges.
Updates require the focused regression tests and the native CI and installed nightly checks.
`npm run test:agent-runtime` checks extension loading, selected models, streamed completion, and cache usage against a local fixture.
Set `PI_TEST_BINARY` to the verified Pi executable and `PI_TEST_PACKAGES` to an isolated frozen install of `core/pins/packages.bun.lock`.
