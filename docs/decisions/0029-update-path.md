# 0029 — Sign desktop updates and preserve installer identity

## Decision

The desktop reads the public stable release feed without identifying the user or
machine. It downloads an update, verifies its signature and signed version, and
shows an install control. Installation requires idle runtime activity.

The public key is compiled into the app. The private signing key stays in release
credentials. Packaging signs the finished artifacts, after platform signing and
macOS notarization. Stable promotion copies those bytes and publishes their
signatures and stable URLs in `latest.json`. A full installed nightly is required.

macOS updates use a tar archive of the signed application bundle. Linux in-app
updates use AppImage. Debian packages use the system package manager. Windows
reads the registration for its exact install directory and selects per-user MSI,
machine MSI, or NSIS. Missing or ambiguous registration stops the update.

Each signature binds the artifact to the announced version. A signature failure
never enables the install control. An installation failure retains the verified
update for retry. The app restarts after installation.

Official Homebrew distribution uses `Homebrew/homebrew-cask`. There is no custom
tap. Package manager updates remain available independently of the in-app updater.

## References

- [Production-ready gates](../../SPEC.md#production-ready-gates-release-gate)
- [Stable promotion](../../.github/lib/release-promotion.mjs)
- [Homebrew](../homebrew.md)
