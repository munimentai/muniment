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
- DONE — first-run registration, native browser authorization, native token exchange, and coherent credential persistence are wired through the Tauri `auth_sign_in` command.
- DONE — native local status, refresh at the safety skew, authoritative session inspection, and the pre-chat fresh-token path replace their legacy generic-OIDC counterparts end to end.
- DONE — Sign out atomically clears the production native session locally while preserving the installation identity.
- DONE — Sign out best-effort revokes the current native refresh family and access sessions before the atomic local clear.
- DONE — the signed-in profile consumes the live typed entitlement snapshot without exposing signing material.
- DONE — the signed-in access panel lists server-derived native-device metadata with current, active, revoked, empty, loading, failure, and retry states. No cross-device mutation is selected without a documented live cloud contract.

### 9. Pi sidecar and cloud chat
- DONE foundation — ADR 0008 pins Pi 0.73.1, chooses verified first-use acquisition of its platform-native executable, and proves its real `get_state` RPC readiness through `SidecarSupervisor`.
- DONE — the first signed-in streamed chat runs through the supervised Pi runtime and the user's server-resolved scoped LiteLLM access, with durable event translation and server-authoritative provenance projection.
- DONE — mid-run steer and queued follow-up from the composer over the live Pi stream.
- DONE — scroll-follow during streaming with user-scroll disengage, and startup reconciliation that marks interrupted runs `needs_attention` instead of rendering them as perpetually streaming.
- DONE — typed Pi tool-execution frame parsing, Tauri run-loop journaling of `tool.effect.*`, concurrent-effect reducer semantics, and tool activity in live/history chat payloads.
- DONE — the provenance line expands into the full receipt record (design-system §6).
- DONE — projected Pi tool activity renders as inline mono tool cards, including concurrent running effects and restored history.
- DONE groundwork — typed, validated Pi extension-UI request/response protocol for blocking select, confirm, input, and editor interactions.
- DONE durability — blocking Pi UI requests are journaled before projection and replay as a typed pending permission gate.
- DONE — explicit safe resume reopens an eligible interrupted Pi session and continues the same durable run without replaying prompts, permission decisions, or unresolved effects. Answering a pending gate remains blocked on the Needs Human decision.

### 10. Local Gemma sidecar
- DONE — supervised JSON-RPC sidecar lifecycle; loopback llama-server launch/health/chat; pinned Gemma descriptor and launch verification; dictation-polish and routing-classifier contracts/evaluations with adversarial framing.
- DONE — ADR 0006, verified revision publication/recovery, bounded resumable acquisition, the shared native HTTPS/proxy acquisition transport, the pure-core cancellable-lock/free-space install coordinator, exact resumable-stage byte accounting, Gemma acquisition/publication composition through that coordinator, activation-health rollback to the retained verified revision, and standard-library native filesystem/lock/clock/cancellation adapters.
- DONE — Tauri install, status, and cancel commands with redacted public states.
- NEXT — explicit first-use install UI and release notice delivery as separate slices after the approved bundled terms surface is available.

### 11. Attachments and local CAS
- DONE groundwork — pure-Rust content-addressed local store with atomic deduplication, constant-memory I/O/verification, and stale-temp cleanup. Cloud file flow follows items 8d/9.

### Durable local run journal
- DECIDED — ADR 0002 makes a per-run append-only SQLite event journal authoritative; external effects are never silently re-executed and large bodies live in CAS.
- DONE — schema/envelope/atomic append and deterministic reducer/replay with permission, terminal, unsupported-event, incremental, and crash-boundary coverage.
- DONE first-chat slice — Pi domain/effect translation and server-authoritative receipt projection are durable and replayable.
- DONE deletion slice — atomic deletion of a run's events with CAS reference accounting (`delete_run` returns the run's hashes; `referenced_hashes` reports journal-wide references).
- DONE collection slice — unreferenced CAS objects are collected using the journal's reference accounting (pure core).
- DONE retention slice — terminal runs older than a supplied policy age out deterministically and their newly unreferenced CAS objects are collected; nonterminal and needs-attention runs are preserved.
- DONE export slice — deterministic versioned pure-core export preserves canonical envelopes, verifies referenced CAS bodies, deduplicates bodies, and reads from one SQLite snapshot.
- DONE compaction slice — crash-safe, non-destructive SQLite compaction preserves canonical export and leaves the source journal intact on interruption or failure.
- NEXT — no further journal-maintenance slice is selected; advance an ungated product lane.

### Capability vocabulary and provenance
- DONE — user surfaces say “capabilities” and receipts render `route · model · cost · time · capability@version[, ...]` in both the provenance line and the expandable receipt record. The client never synthesizes values absent from the cloud schema.

## Phase 3 — Routing metadata + voice (§9 items 12, 15)
- Routing metadata carriage/policy integration follows item 9; the local classifier contract is complete.
- Voice direction remains Parakeet capture → Gemma polish → transforms, with Kokoro read-aloud and global hotkeys.
- DONE — ADRs 0004/0005, pinned Parakeet verification, revision publication/recovery, bounded resumable acquisition, consumption of the shared native HTTPS/proxy transport, the shared pure-core install coordinator, exact resumable-stage byte accounting, Parakeet acquisition/publication composition through that coordinator, and standard-library native filesystem/lock/clock/cancellation adapters.
- DONE — Tauri install, status, and cancel commands with redacted public states.
- DONE foundation — sherpa-onnx 1.13.2 native libraries are packaged for macOS universal2, Windows x86_64, and Linux x86_64, with a safe offline Parakeet recognizer binding that verifies the current model, validates finite normalized mono 16 kHz input, owns and cleans up native handles, and has packaging and contract tests.
- DONE capture foundation — the native microphone stream is owned outside the webview and feeds normalized mono 16 kHz PCM through a fixed-capacity, nonblocking queue with tested downmixing, anti-aliased resampling, overflow accounting, and deterministic cleanup.
- DONE segmentation foundation — fixed-memory utterance boundaries consume caller-supplied voice-activity decisions with pre-roll, minimum speech, trailing silence, maximum duration, discontinuity, and flush semantics covered by chunk-boundary-invariant tests.
- DONE VAD foundation — the ADR 0004-pinned Silero VAD artifact is acquired, verified, and published atomically with the Parakeet model set, and a frame-at-a-time detector boundary verifies the installed artifact before constructing the native sherpa-onnx handle, validates 16 kHz 512-sample normalized input, and exposes reset/discontinuity semantics behind a typed error surface.
- NEXT — pure-core dictation pipeline composition (arbitrary-length capture PCM → fixed VAD frames → segmenter decisions → emitted utterances), then desktop recognition command wiring, UI, and target-hardware validation as independent slices.

## Phase 4+ — Org surface (§9 items 16–19)
- Remote MCP consumption, local stdio allowlist, capability install flow, artifact side panel, and projects with the redaction rule (`output withheld · connection not granted`).

## Standing gates
- Code PRs: structure smoke, frontend/Rust tests, then Linux/Windows/macOS desktop builds. Markdown-only PRs gate on structure smoke alone; pushes to `main` run smoke only.
- Build desktop targets only. Mobile scaffolding remains owner-gated by §12.5's repo-strategy ADR. CLI/editor E0 is decided by [ADR 0009](docs/decisions/0009-companion-attach-protocol.md); pure-core protocol envelope types, bounded frame codec, version negotiation, pairing, capability authorization state, and a durable idempotency ledger for effectful requests are complete with contract tests. DONE — bounded pure-core run-stream cursor/acknowledgement window state and bounded artifact transfer window/acknowledgement state. DONE Linux filesystem foundation — the effective-UID-owned `XDG_RUNTIME_DIR` and private `muniment` directory are validated and pinned without following symlinks. DONE Linux transport foundation — an owned Unix socket is safely published and withdrawn with stale-socket recovery, endpoint identity/mode checks, and same-effective-UID peer authentication through `SO_PEERCRED`. DONE Linux negotiation — the bounded hello/welcome exchange runs on an accepted authenticated stream with per-connection nonces, typed protocol-error replies, and a hard deadline. DONE Linux pairing authorization — the negotiated stream issues an approval challenge from the pure-core `AuthorizationState`, gates the typed `authorized` grant on an explicit desktop approval decision through a deterministic seam, and closes on denial or expiry per ADR 0009. NEXT — the authorized stream serves its first read-only operation (`thread.list`) through a deterministic data seam. macOS and Windows adapters follow separately. No companion UI is complete.
