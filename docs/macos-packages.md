# macOS packages

The nightly release includes `nightly-<sha>-macos-muniment.pkg` for Jamf, Intune, and other MDM systems.
The package installs `muniment.app` in `/Applications` for every user.

Installing a newer package replaces the existing app at the same path.
The package keeps user data because the app stores it outside `/Applications`.
An MDM should install the package in the system context and use `ai.muniment.desktop` as the app identifier.
The MDM should quit the app before an upgrade to avoid a stale running process.
No installer script needs network access or user input.

The package is unsigned until Apple enrollment Y5DUNHQA74 clears.
macOS may reject an unsigned package outside an MDM policy that permits it.
Keep `MACOS_SIGNING_ENABLED` set to `false` until the Apple credentials exist.
After enrollment clears, add the documented credentials and set that repository variable to `true`.
The nightly build then signs and notarizes both macOS artifacts.

See [macOS signing](macos-signing.md) for the credential contract and verification commands.
