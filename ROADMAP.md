# muniment-desktop — ROADMAP

Phases mirror harness-spec §9. **This lane is CLOSED until muniment-cloud Phase 1 (OIDC + grants + key regen) is live** — the shell's first real milestone authenticates against it (owner-set gate, 2026-07-08).

Gate reading in practice (updated 2026-07-10): client-side slices that are fully verifiable locally — toolchain, protocol core proven against an in-process mock IdP in unit tests, sidecar transport proven against a stub binary, UI states that fabricate nothing — proceed. Anything that needs the live control plane — desktop client registration on api.muniment.ai, the first real handshake, entitlement snapshot fetch, websocket-pushed refresh — stays BLOCKED, not mocked (SPEC law 1), until the owner declares cloud Phase 1 live.

## M0 — Scaffold (done at bootstrap, 2026-07-09)
- Tauri v2 hello-world shell (static webview page, brand-neutral).
- Proven to build through `desktop-ci` on all three platforms.
- Specs + desktop/mobile mockups + ring reference vendored.

## Phase 2 — Client core (§9 items 8-11)
8. Shell + auth handshake + entitlement snapshot consumption (depends cloud 4, 5).
   - 8a DONE — Svelte 5 + Vite, ADR 0001, design tokens, vendored fonts.
   - 8b DONE — OIDC PKCE core, loopback redirect, keychain token store, mock-IdP tests.
   - 8c DONE — refresh-on-expiry and real signed-out/signed-in shell states.
   - 8e DONE — pre-chat shell frame per design-spec §2.1.
   - §1.8 ring DONE — tested rest + thinking component.
   - 8f DONE — structured server-unreachable state with retry and diagnostics.
   - 8d BLOCKED on cloud Phase 1 — registration, real handshake, entitlement snapshot fetch/display.
9. Pi sidecar, RPC over stdio, chat against the user's virtual key (depends cloud 6 + item 8). Pi RPC wiring stays blocked on cloud 6. Before persistence, use the durable journal contract below.
10. Local model sidecar (llama.cpp + resident Gemma quant), health-managed.
    - DONE 2026-07-10 — shared process supervisor, typed JSON-RPC transport, notification/cancellation hygiene, readiness-aware health, lifecycle events, bounded stderr diagnostics, restart safety, and module split; all stub-binary tested.
    - DONE 2026-07-10 — loopback-only managed llama-server launcher, bounded HTTP health/chat clients, and typed/redacted failures.
    - DONE 2026-07-10 — ADR 0003 pins Gemma 3 4B QAT Q4_0; launch streams exact-size/SHA-256 verification and fixes the API alias/context.
    - DONE 2026-07-10 — typed dictation-polish request/response contract, fixed prompt, and deterministic golden evaluations over the managed chat client.
    - DONE 2026-07-10 — typed routing-classifier request/response contract with Phase 3 item 12's closed vocabulary, strict/redacted decoding, and deterministic golden evaluations.
    - DONE 2026-07-10 — routing-classifier and dictation-polish prompts encode their complete untrusted input as JSON strings, with delimiter-breakout regression coverage.
    - Artifact acquisition/update/rollback remains a separate distribution concern.
11. Attachments pipeline + content-addressed file store client. The cloud file-store pipeline follows items 8d/9.
    - Groundwork DONE — pure-Rust content-addressed local store with atomic dedup publish, constant-memory put/get/verify, and stale-temp sweep.

- DECIDED 2026-07-10 — **Durable local run journal.** ADR 0002 makes a per-run append-only SQLite event journal the source of truth. Deterministic replay reconstructs permission and terminal/needs-attention state; recorded external effects are never silently re-executed. Large bodies live in CAS and events hold hashes.
  - Slice 1 DONE — schema, versioned envelope, validated open, atomic append, contract tests.
  - Slice 2 DONE — deterministic reducer with permission, terminal, unsupported-event, incremental replay, and crash-boundary fixtures.
  - NEXT — Pi domain/effect translation and receipt projection when cloud 6 opens; then retention/export/deletion/CAS collection/compaction.

- PLANNED — **Capability vocabulary + receipt provenance.** End-user surfaces say “capabilities,” show the approved one-line description, and render `route · model · cost · time · capability@version[, ...]`. Real values depend on MUNICLOUD's schema; the client never synthesizes provenance.

## Phase 3 — Routing metadata + voice (§9 items 12, 15)
- Classifier labels ride as request metadata; model pin chip for `router.override` holders only. Local classifier contract/evaluation and adversarial-framing groundwork are DONE; metadata carriage and policy integration remain blocked on Phase 2 item 9/cloud 6.
- Voice: Parakeet capture → Gemma polish (flash) → transforms; Kokoro read-aloud; global hotkeys (⌥Space; Windows binding decided in-build). Dictation-polish contract/evaluation and adversarial-framing groundwork are DONE.
  - DONE 2026-07-10 — ADR 0004 pins sherpa-onnx v1.13.2 and the immutable Parakeet-TDT 0.6B v3 INT8 four-file artifact, defines the in-process privacy/packaging boundary, records licensing duties, and sets a target-hardware validation matrix.
  - DONE 2026-07-10 — a pure-Rust, constant-memory verifier publishes success only after every pinned ASR artifact passes regular-file, exact-size, and SHA-256 checks.
  - DONE 2026-07-10 — ADR 0005 selects first-use installation and defines immutable revision storage, bounded native download, atomic publication, recovery, rollback, removal, ownership/privacy boundaries, and notice delivery.
  - NEXT — implement the pure-Rust lifecycle and injected filesystem/HTTP boundaries against tiny local fixtures and an in-process HTTP server; Tauri adapters, UI, native bindings/packaging, capture/VAD, and hardware validation follow separately.

## Phase 4+ — Org surface (§9 items 16-19)
- Remote MCP consumption, local stdio allowlist, capability install flow, artifact side panel, and projects with the redaction rule (`output withheld · connection not granted`).

## Standing gates
- Code PRs: structure smoke followed by green desktop-CI builds on Linux, Windows, and macOS. Markdown-only PRs gate on structure smoke alone. Pushes to `main` run smoke only.
- This repo builds macOS/Windows/Linux. Do not scaffold mobile targets unless §12.5's owner-gated repo-strategy ADR chooses the shared-workspace path.
