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
