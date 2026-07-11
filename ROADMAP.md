# muniment-desktop — ROADMAP

Phases mirror harness-spec §9. **This lane is CLOSED until muniment-cloud Phase 1 (OIDC + grants + key regen) is live.** Locally verifiable client slices may proceed; registration, the first real handshake, entitlement fetch, and websocket refresh remain blocked and must not be mocked.

## M0 — Scaffold (done 2026-07-09)
- Tauri v2 desktop shell builds on macOS, Windows, and Linux.
- Specs, mockups, design reference, Rust/Frontend test harnesses, and CI gates are present.

## Phase 2 — Client core (§9 items 8–11)

### 8. Shell, auth, and entitlements
- DONE — Svelte 5/Vite toolchain, design tokens, OIDC PKCE core, keychain storage, refresh-on-expiry, signed-out/signed-in states, pre-chat shell frame, reusable milled ring, and structured server-unreachable recovery.
- BLOCKED on cloud Phase 1 — desktop registration, real handshake, and entitlement snapshot fetch/display.

### 9. Pi sidecar and cloud chat
- RPC wiring remains blocked on cloud 6. Before persistence, use the durable journal contract below.

### 10. Local Gemma sidecar
- DONE — supervised JSON-RPC sidecar lifecycle; loopback llama-server launch/health/chat; pinned Gemma descriptor and launch verification; dictation-polish and routing-classifier contracts/evaluations with adversarial framing.
- DONE — ADR 0006, verified revision publication/recovery, bounded resumable acquisition, the shared native HTTPS/proxy acquisition transport, the pure-core cancellable-lock/free-space install coordinator, exact resumable-stage byte accounting, and Gemma acquisition/publication composition through that coordinator.
- NEXT — activation-health rollback in pure core. Native adapters/Tauri commands, UI, and release notice delivery follow as separate slices.

### 11. Attachments and local CAS
- DONE groundwork — pure-Rust content-addressed local store with atomic deduplication, constant-memory I/O/verification, and stale-temp cleanup. Cloud file flow follows items 8d/9.

### Durable local run journal
- DECIDED — ADR 0002 makes a per-run append-only SQLite event journal authoritative; external effects are never silently re-executed and large bodies live in CAS.
- DONE — schema/envelope/atomic append and deterministic reducer/replay with permission, terminal, unsupported-event, incremental, and crash-boundary coverage.
- NEXT when cloud 6 opens — Pi domain/effect translation and receipt projection; retention/export/deletion/CAS collection/compaction follow.

### Capability vocabulary and provenance
- PLANNED — user surfaces say “capabilities” and receipts render `route · model · cost · time · capability@version[, ...]`. The client never synthesizes values absent from the cloud schema.

## Phase 3 — Routing metadata + voice (§9 items 12, 15)
- Routing metadata carriage/policy integration remains blocked on item 9/cloud 6; the local classifier contract is complete.
- Voice direction remains Parakeet capture → Gemma polish → transforms, with Kokoro read-aloud and global hotkeys.
- DONE — ADRs 0004/0005, pinned Parakeet verification, revision publication/recovery, bounded resumable acquisition, consumption of the shared native HTTPS/proxy transport, the shared pure-core install coordinator, and exact resumable-stage byte accounting.
- NEXT — compose Parakeet acquisition/publication through the coordinator using the established Gemma pattern. Native adapters/Tauri commands, UI, native bindings/packaging, capture/VAD, and hardware validation follow separately.

## Phase 4+ — Org surface (§9 items 16–19)
- Remote MCP consumption, local stdio allowlist, capability install flow, artifact side panel, and projects with the redaction rule (`output withheld · connection not granted`).

## Standing gates
- Code PRs: structure smoke, frontend/Rust tests, then Linux/Windows/macOS desktop builds. Markdown-only PRs gate on structure smoke alone; pushes to `main` run smoke only.
- Build desktop targets only. Mobile scaffolding remains owner-gated by §12.5's repo-strategy ADR.
