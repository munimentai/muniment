# Installed Linux E2E evidence

The production sign-in spec handles three start states:

1. If onboarding appears, the spec creates Home and continues without importing.
2. If the signed-out screen appears, the spec clicks **Sign in**.
3. If Local mode appears, the spec clicks **Sign in for cloud features**.

Each state reaches the same production sign-in steps.
The Linux runner clears `ready/config/ai.muniment.desktop/local-mode` between the local-mode chat and production sign-in launches.
The onboarding spec uses a separate state directory.

The next nightly Linux run must report `failures="0"` for each spec.
The separate launches produce these reports:

- `junit-local-mode-chat-0-0.xml`
- `junit-real-sign-in-0-0.xml`
- `junit-onboarding-0-0.xml`

The cleanup launch also produces `junit-cleanup-0-0.xml`.
This checkout has no qualifying nightly run artifact.

## Failures before the specs

The `<platform>-e2e-<sha>-failure` artifact holds `runner-failure.txt` with the runner's terminating cause.
The runner redacts this file with its other logs before publication.
An asset lookup failure names the platform, expected asset name or Windows pattern, match count, and release asset names.
`junit-infrastructure.xml` uses this cause as its failure message, capped at 1000 characters before XML escaping.
If redaction blocks the logs, `envelope-reason.txt` names that failure instead.
The JUnit helper uses a generic message only when neither file holds a cause.
`envelope-diagnostics.txt` holds structural counts when artifact extraction fails. It does not copy the runner transcript.

## Installed Windows E2E

The Windows runner uses the GitHub REST API with the injected `GH_TOKEN`
for both the nightly release lookup and the installer asset download.
It passes the release response body unchanged to `asset-identity.mjs` before
it downloads the installer.

The next nightly `windows-e2e` run must confirm that the lookup reaches the
installer step. Check the runner transcript and `installer.log` in the run
artifact for installer output after the lookup.

## Current-source mode

The Linux, Windows and macOS candidate runners read `MUNIMENT_E2E_SOURCE_MODE`.
An unset or `nightly` value keeps the release lookup, package identity validation and release WebDriver-absence guards.
The value `pr` builds an unsigned package from the checkout and installs it, then runs the same specs, fixtures, probes and cleanup.
Any other value stops the runner.
The `pr` mode requires `MUNIMENT_E2E_SOURCE_SHA` to equal the checkout head, refuses signing credentials, and never reads the nightly release.
It builds with `--features e2e-webdriver` and the E2E configuration, so the installed payload carries the WebDriver marker and the production absence guards do not apply to it.
The macOS `pr` mode seals the installed bundle with an ad-hoc local signature, because the unsigned build has no nested signatures.
`pr-package.mjs` selects exactly one local package and fails when none or several match.
