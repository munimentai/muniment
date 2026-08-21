# macOS packages

The nightly release includes `nightly-<sha>-macos-muniment.pkg` for Jamf, Intune, and other MDM systems.
The package installs `muniment.app` in `/Applications` for every user.

Installing a newer package replaces the existing app at the same path.
The package keeps user data because the app stores it outside `/Applications`.
An MDM should install the package in the system context and use `ai.muniment.desktop` as the app identifier.
The MDM should quit the app before an upgrade to avoid a stale running process.
No installer script needs network access or user input.

The app bundle carries the user runtime at
`Contents/Library/LaunchServices/muniment-runtime`. It carries the
`ai.muniment.runtime.plist` LaunchAgent in `Contents/Library/LaunchAgents`.
Each user registers that agent through `SMAppService` under the
`ai.muniment.runtime` label. Registration starts it and enables it at later
logins. A surface can kick-start the job in that user's `gui/<uid>` domain
when the attach endpoint is absent.

The runtime stays in the foreground. `launchd` spaces failed starts by five
seconds, and the runtime stops after five failures within five minutes. It
exits successfully for an explicit stop, normal idle, logout, or uninstall.
At logout it handles `SIGTERM` with the bounded graceful-stop contract.

An upgrade atomically replaces the verified app bundle at the same path. The
running runtime drains work and exits with status 75. `launchd` starts the new
payload. Failed readiness restores the old verified bundle under the per-user
install lock.

Managed uninstall calls `SMAppService` `unregister()` in every registered user
context. It removes the app bundle after every job stops. A stop or unregister
failure leaves the signed bundle and user data in place.

Every macOS attach peer must pass `getpeereid` before either side exchanges a
protocol frame. The kernel-supplied effective UID must match the local
effective UID. Missing, changed, or contradictory identity evidence closes
the connection. No claimed protocol field grants peer authority.

The runtime writes redacted, bounded diagnostics to
`~/Library/Logs/Muniment/runtime.log`. Structured records also use unified log
subsystem `ai.muniment.desktop` and category `runtime`.

The package is unsigned until Apple enrollment Y5DUNHQA74 clears.
macOS may reject an unsigned package outside an MDM policy that permits it.
Keep `MACOS_SIGNING_ENABLED` set to `false` until the Apple credentials exist.
After enrollment clears, add the documented credentials and set that repository variable to `true`.
The nightly build then signs and notarizes both macOS artifacts.

See [macOS signing](macos-signing.md) for the credential contract and verification commands.
