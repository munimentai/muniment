# macOS signing & notarization — "when Apple clears" checklist

The nightly release CI already runs macOS code-signing, notarization, and
stapling behind the same desktop-ci env-injection seam Windows signing uses
(`.github/build-macos-app.mjs`, config in `.github/lib/macos-signing.mjs`).
With no Apple secrets set it is a clean no-op: the `.app` builds and ships
UNSIGNED exactly as before, and the build logs `macOS signing SKIPPED: Apple
credentials absent (unsigned build)`. The day Apple enrollment **Y5DUNHQA74**
clears, only secrets need adding — no code change.

## 1. Fill these six vault keys (repo secrets)

Populate every key. All-or-nothing: any subset present makes the build fail
fast naming the missing ones (`incomplete macOS signing configuration`), and
zero present keeps the clean unsigned no-op.

| Secret | What it is |
| --- | --- |
| `APPLE_CERTIFICATE` | The Developer ID **Application** `.p12`, base64-encoded single-line: `base64 -i DeveloperIDApplication.p12 \| tr -d '\n'` |
| `APPLE_CERTIFICATE_PASSWORD` | The password protecting that `.p12` export |
| `APPLE_TEAM_ID` | The 10-character Apple Developer Team ID (e.g. `Y5DUNHQA74`) |
| `APPLE_API_KEY` | The App Store Connect `.p8` key for `notarytool`, base64-encoded single-line: `base64 -i AuthKey_XXXXXXXXXX.p8 \| tr -d '\n'` |
| `APPLE_API_KEY_ID` | The key's identifier (the "Key ID" column in App Store Connect) |
| `APPLE_API_ISSUER` | The App Store Connect issuer UUID (Users and Access → Integrations → Keys) |

Notes:
- Use a **Developer ID Application** certificate — distribution outside the Mac
  App Store. Export it from Keychain Access as a `.p12` (cert + private key).
- The App Store Connect API key must have at least the **Developer** role so
  `notarytool` can submit. Download the `.p8` once at creation; Apple never
  re-issues it.
- Both blobs are base64-encoded so they travel as single-line `KEY=VALUE`
  entries through the env-injection seam; the build decodes them in the VM.

## 2. What the build does once the secrets exist

`build-macos-app.mjs` (macOS build VM only):
1. Builds the universal `.app` (identical bits to the unsigned path).
2. Imports the `.p12` into a throwaway keychain and finds the Developer ID
   Application identity by SHA-1.
3. `codesign`s the nested ASR runtime dylibs, then the `.app`, with the
   hardened runtime (`--options runtime`) and a secure `--timestamp`.
4. `xcrun notarytool submit … --wait` using the API key; a rejected build fails
   the job instead of shipping.
5. `xcrun stapler staple` staples the ticket so Gatekeeper validates offline.
6. Packages `muniment.app.zip` from the signed + notarized + stapled bundle.

The nightly release notes flip automatically: `macOS app for <sha> was signed
and notarized by the nightly workflow.`

## 3. Verify a signed + notarized artifact

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
