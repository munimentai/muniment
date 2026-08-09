# Homebrew

The Muniment tap ships the current macOS nightly as a universal app archive.
The cask pins each archive by its source commit and SHA-256 digest.

Install it with these commands:

```sh
brew tap mikeydiamonds/muniment https://github.com/mikeydiamonds/muniment-desktop
brew install --cask mikeydiamonds/muniment/muniment-nightly
```

## Gatekeeper caveat

The app is unsigned until Apple completes the developer enrollment.
macOS Gatekeeper will block the app after Homebrew installs it.

If you knowingly trust the downloaded app, remove its quarantine attribute:

```sh
xattr -dr com.apple.quarantine /Applications/muniment.app
```

This command bypasses Gatekeeper for that app. Do not run it unless you trust the download.

Signing and notarization will change only the nightly artifact.
The cask URL, checksum update, and install commands will stay the same.

## Later migration

The Muniment tap is the shipping source now.
Move the cask to `homebrew/cask` only after the app uses its default stable channel.
The app must also meet Homebrew's current package acceptance and notability rules.
The signed artifact must pass Gatekeeper without a bypass before migration.

At migration time, review Homebrew's [cask requirements](https://docs.brew.sh/Acceptable-Casks) and [shared package policy](https://docs.brew.sh/Acceptable-Formulae).
Submit the stable cask under Homebrew's current token rules, then remove the tap copy after users can migrate.
