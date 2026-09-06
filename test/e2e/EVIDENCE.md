# Installed Linux E2E evidence

A qualifying run ID is pending the first post-push targeted dispatch of
`nightly.yml` with `platform=linux`. This checkout's `agent/MUNIDESK-550`
branch does not exist on the remote, and the implementation session is not
permitted to push it, so GitHub Actions cannot run this change yet.

The qualifying run must have all of these reports with zero failures before
this evidence is complete:

- `junit-onboarding-0-0.xml`
- `junit-sign-in-0-0.xml`
- `junit-cleanup-0-0.xml`

## Installed Windows E2E

The Windows runner uses the GitHub REST API with the injected `GH_TOKEN`
for both the nightly release lookup and the installer asset download.
It passes the release response body unchanged to `asset-identity.mjs` before
it downloads the installer.

The next nightly `windows-e2e` run must confirm that the lookup reaches the
installer step. Check the runner transcript and `installer.log` in the run
artifact for installer output after the lookup.
