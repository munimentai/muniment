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
`ai.muniment.runtime` label while that user runs the app. The app checks the
service status at every launch. It calls `register()` once for `notRegistered`
and rechecks the status. An MDM package cannot register absent users.

An `enabled` status permits a kick-start in that user's `gui/<uid>` domain
when the attach endpoint is absent. A `requiresApproval` status or registration
denial leaves the runtime inactive. The app explains the required approval and
opens System Settings > General > Login Items on request. A `notFound` status
or another registration error also leaves the runtime inactive. No surface
kick-starts or privately spawns a runtime without an `enabled` status.

The runtime stays in the foreground. `launchd` spaces failed starts by five
seconds, and the runtime stops after five failures within five minutes. It
exits successfully for an explicit stop, normal idle, logout, or uninstall.
At logout it handles `SIGTERM` with the bounded graceful-stop contract.

An upgrade atomically replaces the verified app bundle at the same path. The
running runtime drains work and exits with status 75. `launchd` starts the new
payload. Failed readiness starts rollback under the per-user install lock. The
installer creates an owner-only rollback marker before restoration. The marker
makes the new payload exit successfully before it opens state. The installer
requests a graceful stop and waits for its bounded deadline. It sends
`SIGTERM` and waits again. It sends `SIGKILL` if the same job remains active.
The installer verifies that no runtime holds the instance lock before it
restores the old bundle. It removes the marker, kick-starts the job, and
verifies old-payload readiness. A failure leaves the marker and signed bundle
in place and reports the failure.

Managed uninstall calls `SMAppService` `unregister()` in each logged-in
registered user context. It cannot unregister an absent user. Removal stays
pending until each absent registered user logs in and the app unregisters in
that user's context. It removes the app bundle after every registration clears
and every job stops. A stop or unregister failure leaves the signed bundle and
user data in place.

Every macOS attach peer must pass `getpeereid` before either side exchanges a
protocol frame. The kernel-supplied effective UID must match the local
effective UID. Missing, changed, or contradictory identity evidence closes
the connection. No claimed protocol field grants peer authority.

The LaunchAgent sends stdout and stderr to the absolute path `/dev/null`. The
runtime resolves the effective user's home through the OS user record. It
writes redacted, bounded diagnostics to the resulting absolute
`/Users/<user>/Library/Logs/Muniment/runtime.log` path. Structured records also
use unified log subsystem `ai.muniment.desktop` and category `runtime`.

The package is unsigned until Apple enrollment Y5DUNHQA74 clears.
macOS may reject an unsigned package outside an MDM policy that permits it.
Keep `MACOS_SIGNING_ENABLED` set to `false` until the Apple credentials exist.
After enrollment clears, add the documented credentials and set that repository variable to `true`.
The nightly build then signs and notarizes both macOS artifacts.

See [macOS signing](macos-signing.md) for the credential contract and verification commands.
