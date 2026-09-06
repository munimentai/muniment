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
