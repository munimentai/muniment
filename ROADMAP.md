# muniment-desktop — ROADMAP

Phases mirror harness-spec §9. Muniment-cloud Phase 1 native auth and the cloud prerequisites for desktop chat are live as of 2026-07-11; client work that uses them must exercise the real contracts and must not introduce mocked production paths.

## M0 — Scaffold (done 2026-07-09)
- Tauri v2 desktop shell builds on macOS, Windows, and Linux.
- Specs, mockups, design reference, Rust/Frontend test harnesses, and CI gates are present.

## Phase 2 — Client core (§9 items 8–11)

### 8. Shell, auth, and entitlements
- DONE — Svelte 5/Vite toolchain, design tokens, generic OIDC PKCE core, keychain storage, refresh-on-expiry, signed-out/signed-in states, pre-chat shell frame, reusable milled ring, and structured server-unreachable recovery.
- DONE cloud prerequisite — the `muniment-desktop` installation-bound native authorization, token rotation, session, revocation, and entitlement-snapshot contracts are live at `api.muniment.ai` (2026-07-11).
- CORRECTION — the live desktop contract is `/v1/auth/native/*`, not generic issuer discovery; `/.well-known/openid-configuration` returns 404. The existing generic OIDC core remains useful test groundwork but is not the production handshake.
- DONE — registration and coherent keychain persistence of the desktop installation identity and one-use registration material through `POST /v1/auth/native/devices`.
- DONE — construction of the signed installation proof, `POST /v1/auth/native/authorize`, validation of its opaque continuation, persistence of the rotated device challenge, system-browser launch, and receipt of a state-validated loopback callback while retaining the PKCE verifier.
- DONE — exchange of the callback code through `POST /v1/auth/native/token`, strict validation of the native token/session/entitlement envelope, and coherent persistence of the token set and rotated device challenge in pure core.
- DONE — refresh of an unexpired native session through `POST /v1/auth/native/token` with a fresh installation proof and coherent rotation of the access token, refresh token, expiries, and device challenge in pure core.
- DONE — authoritative session inspection through `GET /v1/auth/native/session` in pure core, including strict identity, desktop-role, device-binding, and signed-entitlement-envelope validation.
- DONE — desktop keychain adapter for one coherent versioned native installation/credential record, including safe migration of the legacy installation-only entry while leaving unrelated generic-OIDC credentials untouched.
- NEXT before first chat — wire first-run registration, native browser authorization, and native token exchange through the Tauri sign-in command. Follow with native session refresh/status, entitlement snapshot consumption, and revocation as separately reviewable slices.

### 9. Pi sidecar and cloud chat
- DONE foundation — ADR 0008 pins Pi 0.73.1, chooses verified first-use acquisition of its platform-native executable, and proves its real `get_state` RPC readiness through `SidecarSupervisor`.
- NEXT, after the Phase 2.8 native handshake/session wiring — deliver the first signed-in streamed chat through the supervised Pi runtime and the user's server-resolved scoped LiteLLM access, with durable event translation and a provenance line.
- FOLLOW — steering/follow-up controls, tool/permission activity, session resume, and richer receipt expansion in separately reviewable slices.

### 10. Local Gemma sidecar
- DONE — supervised JSON-RPC sidecar lifecycle; loopback llama-server launch/health/chat; pinned Gemma descriptor and launch verification; dictation-polish and routing-classifier contracts/evaluations with adversarial framing.
- DONE — ADR 0006, verified revision publication/recovery, bounded resumable acquisition, the shared native HTTPS/proxy acquisition transport, the pure-core cancellable-lock/free-space install coordinator, exact resumable-stage byte accounting, and Gemma acquisition/publication composition through that coordinator.
- NEXT — activation-health rollback in pure core. Native adapters/Tauri commands, UI, and release notice delivery follow as separate slices.

### 11. Attachments and local CAS
- DONE groundwork — pure-Rust content-addressed local store with atomic deduplication, constant-memory I/O/verification, and stale-temp cleanup. Cloud file flow follows items 8d/9.

### Durable local run journal
- DECIDED — ADR 0002 makes a per-run append-only SQLite event journal authoritative; external effects are never silently re-executed and large bodies live in CAS.
- DONE — schema/envelope/atomic append and deterministic reducer/replay with permission, terminal, unsupported-event, incremental, and crash-boundary coverage.
- NEXT with Phase 2.9 — Pi domain/effect translation and receipt projection; retention/export/deletion/CAS collection/compaction follow.

### Capability vocabulary and provenance
- PLANNED — user surfaces say “capabilities” and receipts render `route · model · cost · time · capability@version[, ...]`. The client never synthesizes values absent from the cloud schema.

## Phase 3 — Routing metadata + voice (§9 items 12, 15)
- Routing metadata carriage/policy integration follows item 9; the local classifier contract is complete.
- Voice direction remains Parakeet capture → Gemma polish → transforms, with Kokoro read-aloud and global hotkeys.
- DONE — ADRs 0004/0005, pinned Parakeet verification, revision publication/recovery, bounded resumable acquisition, consumption of the shared native HTTPS/proxy transport, the shared pure-core install coordinator, exact resumable-stage byte accounting, and Parakeet acquisition/publication composition through that coordinator.
- NEXT after the current Gemma activation slice — native adapters/Tauri commands. UI, native bindings/packaging, capture/VAD, and hardware validation follow separately.

## Phase 4+ — Org surface (§9 items 16–19)
- Remote MCP consumption, local stdio allowlist, capability install flow, artifact side panel, and projects with the redaction rule (`output withheld · connection not granted`).

## Standing gates
- Code PRs: structure smoke, frontend/Rust tests, then Linux/Windows/macOS desktop builds. Markdown-only PRs gate on structure smoke alone; pushes to `main` run smoke only.
- Build desktop targets only. Mobile scaffolding remains owner-gated by §12.5's repo-strategy ADR. CLI/editor E0 is decided by [ADR 0009](docs/decisions/0009-companion-attach-protocol.md); the first unblocked implementation slice is pure-core protocol types/codecs, negotiation, authorization state, cursor/idempotency semantics, and contract tests. No listener or companion UI is complete.
