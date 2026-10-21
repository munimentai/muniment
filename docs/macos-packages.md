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
or another registration error also leaves the service inactive.

`launchd` applies a launch constraint to the bundle's `BundleProgram`. A
runtime without a trusted signature, such as an ad hoc signed local build,
fails it: the kernel ends each spawn with `OS_REASON_CODESIGNING | Launch
Constraint Violation`, and `launchctl print` reports `last exit code = 78:
EX_CONFIG` before the runtime runs at all. The unified log names the cause
under `process == "launchd"`. The Developer ID build signs the runtime with the
app's identity and the hardened runtime. When registration fails, or the job does not
stay running, the app unregisters the job, runs the bundled runtime as its
own child and says that scheduled agents run only while it is open. That
child stops when the app quits. The next launch registers the service again.

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

## Signing credentials

`.github/build-macos-app.mjs` signs, notarizes and staples the macOS builds, and
`.github/lib/macos-signing.mjs` holds its configuration. With
`MACOS_SIGNING_ENABLED` set to `true`, the build needs all six secrets below,
and a partial set fails with the missing key names.

| Secret | Value |
| --- | --- |
| `APPLE_CERTIFICATE` | One `.p12` with the Developer ID Application and Installer identities and their private keys, base64 on one line. |
| `APPLE_CERTIFICATE_PASSWORD` | The `.p12` export password. |
| `APPLE_TEAM_ID` | The 10-character Apple Developer Team ID. |
| `APPLE_API_KEY` | The App Store Connect `.p8` key for `notarytool`, base64 on one line. The key needs the Developer role or higher. |
| `APPLE_API_KEY_ID` | The key's Key ID. |
| `APPLE_API_ISSUER` | The App Store Connect issuer UUID. |

The build imports the `.p12` into a throwaway keychain and places the vendored
Developer ID G2 intermediate in the login keychain for the run. It checks for a
valid Developer ID Application identity before the first `codesign` call. It
signs the app with the hardened runtime and a secure timestamp, notarizes and
staples the app archives, then signs, notarizes and staples every PKG and DMG.

Verify a signed app on a Mac:

```bash
codesign --verify --deep --strict --verbose=2 muniment.app
xcrun stapler validate muniment.app
spctl --assess --type execute --verbose=4 muniment.app
```

All three must pass before distribution.

## Local signed build

`node scripts/build-macos-local.mjs` builds the Apple silicon app, signs it
with the nightly's `codesign` arguments, and installs it in `~/Applications`.
A Developer ID Application signature passes the launch constraint above, so
the background service runs from a local build. An ad hoc signature does not.
The certificate comes from the environment and never sits in the repository:

| Variable | Value |
| --- | --- |
| `MUNIMENT_SIGNING_CERTIFICATE_FILE` | The path of the Developer ID `.p12`. |
| `MUNIMENT_SIGNING_CERTIFICATE` | The `.p12` in base64, read when no file is named. |
| `MUNIMENT_SIGNING_CERTIFICATE_PASSWORD` | The `.p12` password. |

The script imports the identity into a throwaway keychain and deletes it on
exit. It does not notarize. `--build-only` leaves the signed app in
`src-tauri/target/release/bundle/macos/` and installs nothing.
