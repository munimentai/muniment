# Desktop frontend framework decision

**Decision:** React with Vite, adopted for Phase 2.8.

The desktop renderer needs a small, state-driven surface now and will shortly consume a
high-volume stream of Pi sidecar events. React's reducer/external-store patterns fit that
event model, its accessibility and testing ecosystem are mature, and the team can share
plain TypeScript contracts with future web surfaces. Vite keeps the Tauri integration
conventional and produces static assets; Tauri remains the security boundary for network
access, authentication, and secrets.

Tailwind is deliberately not included yet. The vendored design tokens are small and
authoritative, and plain CSS avoids adding a second abstraction before reusable component
patterns exist. Reconsider it only when repetition makes a utility layer pay for itself.

Tokens and refresh credentials must never enter renderer storage. React receives only the
authenticated user/org context and the signed snapshot's display payload. Tauri performs
control-plane calls and persists the session token using macOS Keychain, Windows Credential
Manager, or Linux Secret Service via the cross-platform `keyring` crate.
