# macOS signing & notarization — "when Apple clears" checklist

The nightly release CI already runs macOS code-signing, notarization, and
stapling behind the same desktop-ci env-injection seam Windows signing uses
(`.github/build-macos-app.mjs`, config in `.github/lib/macos-signing.mjs`).
With `MACOS_SIGNING_ENABLED=false`, the `.app` and `.pkg` build and ship unsigned.
The day Apple enrollment **Y5DUNHQA74** clears, add the secrets and flip the flag.

## 1. Fill these six vault keys

Populate every key before enabling signing.
When the flag is true, any partial set fails and names the missing keys.

| Secret | What it is |
| --- | --- |
| `APPLE_CERTIFICATE` | A `.p12` containing the Developer ID Application and Installer identities, base64-encoded as one line |
| `APPLE_CERTIFICATE_PASSWORD` | The password protecting that `.p12` export |
| `APPLE_TEAM_ID` | The 10-character Apple Developer Team ID (e.g. `Y5DUNHQA74`) |
| `APPLE_API_KEY` | The App Store Connect `.p8` key for `notarytool`, base64-encoded single-line: `base64 -i AuthKey_XXXXXXXXXX.p8 \| tr -d '\n'` |
| `APPLE_API_KEY_ID` | The key's identifier (the "Key ID" column in App Store Connect) |
| `APPLE_API_ISSUER` | The App Store Connect issuer UUID (Users and Access → Integrations → Keys) |

Notes:
- Export both Developer ID leaf identities and their private keys in one `.p12`.
  The `.p12` needs no intermediate certificate chain.
- The App Store Connect API key must have at least the **Developer** role so
  `notarytool` can submit. Download the `.p8` once at creation; Apple never
  re-issues it.
- Both blobs are base64-encoded so they travel as single-line `KEY=VALUE`
  entries through the env-injection seam; the build decodes them in the VM.

Set the `MACOS_SIGNING_ENABLED` repository variable to `true` after all six secrets exist.

## 2. What the build does once the flag is true

`build-macos-app.mjs` (macOS build VM only):
1. The build creates the universal `.app` with Tauri's `--no-sign` flag.
2. The build imports the `.p12` into a throwaway keychain.
3. The build places the vendored Apple Developer ID G2 intermediate certificate in the login keychain.
   trustd builds the signer's chain from the login keychain alone, so a keychain the run adds to the search list cannot supply it.
   The build skips the placement when the login keychain already holds the intermediate, and it removes a certificate it placed on exit.
4. The build prepends the throwaway keychain to the user search list and includes the System Roots keychain.
5. The build checks for a valid Developer ID Application identity before any `codesign` call.
6. The build finds the Developer ID Installer identity by SHA-1.
7. The build signs the ASR runtime dylibs, runtime, and `.app` with the hardened runtime and a secure timestamp.
8. The build submits the app archive through `xcrun notarytool submit … --wait` with the API key.
   A rejected build fails the job.
9. The build staples the ticket with `xcrun stapler staple` so Gatekeeper validates offline.
10. The build packages `muniment.app.zip` from the signed and stapled bundle.
11. The build creates the signed `.pkg`, notarizes it, and staples its ticket.

The throwaway keychain holds both Developer ID identities and their private keys.
The login keychain holds the public Developer ID G2 intermediate for the run.
The search list preserves existing keychains and includes `/System/Library/Keychains/SystemRootCertificates.keychain` exactly once.
That keychain supplies the trusted Apple root for the Developer ID chain without changing root trust settings.
The build does not import a root certificate, and it never places an identity or a private key in the login keychain.
The macOS VM must supply the Apple Root CA in System Roots.

Tauri skips its own signing so it cannot sign the bundle before the script checks trust.
Before signing any dylib, the build runs `security find-identity -v -p codesigning <keychain>`.
If that command fails or lists no valid Developer ID Application identity, the build stops with its stdout and stderr.
The error names the certificate chain and keychain access checks.
This preflight does not replace the signed nightly check.

The nightly release notes state whether the macOS artifacts have signatures.

## 3. Verify the signed nightly

Open the nightly `build (macos)` job log.
Confirm that the log shows the Developer ID Application identity before the first ASR dylib signature.
Check that `replacing existing signature` has no following `errSecInternalComponent` or chain warning.
Confirm that the app signature passes and the job reaches `notarytool submit`.

## 4. Verify a signed + notarized artifact

Download `nightly-<sha>-macos-muniment.app.zip`, then on a Mac:

```bash
ditto -x -k nightly-<sha>-macos-muniment.app.zip .

# Signature is a valid Developer ID with the hardened runtime:
codesign --verify --deep --strict --verbose=2 muniment.app
codesign --display --verbose=4 muniment.app 2>&1 | grep -E 'Authority|TeamIdentifier|flags.*runtime'

# Ticket is stapled (validates with no network):
xcrun stapler validate muniment.app

# Gatekeeper accepts it for distribution:
spctl --assess --type execute --verbose=4 muniment.app   # => "accepted", source "Notarized Developer ID"
```

All four must pass. `stapler validate` failing while `spctl` still says
accepted means notarization succeeded but the staple did not — re-staple before
distributing so offline launches are not blocked.

## Still gated on the account clearing (out of scope here)

Enabling public downloads, install docs, TestFlight, and MUNIDESK-113/115
promotion remain owner-gated on Apple clearing — this ticket only pre-stages the
signing steps so none of them wait on serial signing work afterward.
