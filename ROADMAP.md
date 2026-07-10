# muniment-desktop — ROADMAP

Phases mirror harness-spec §9. **This lane is CLOSED until muniment-cloud Phase 1 (OIDC + grants + key regen) is live** — the shell's first real milestone authenticates against it (owner-set gate, 2026-07-08).

Gate reading in practice (updated 2026-07-10): client-side slices that are fully verifiable locally — toolchain, protocol core proven against an in-process mock IdP in unit tests, sidecar transport proven against a stub binary, UI states that fabricate nothing — proceed. Anything that needs the live control plane — desktop client registration on api.muniment.ai, the first real handshake, entitlement snapshot fetch, websocket-pushed refresh — stays BLOCKED, not mocked (SPEC law 1), until the owner declares cloud Phase 1 live.

## M0 — Scaffold (done at bootstrap, 2026-07-09)
- Tauri v2 hello-world shell (static webview page, brand-neutral).
- Proven to build through `desktop-ci` on all three platforms.
- Specs + desktop/mobile mockups + ring reference vendored.

## Phase 2 — Client core (§9 items 8-11)
8. Shell + auth handshake + entitlement snapshot consumption (depends cloud 4, 5). Frontend framework decision recorded here (React or Svelte, Tailwind fine — team's choice per harness-spec §6).
   - 8a DONE 2026-07-09 — frontend toolchain: Svelte 5 + Vite (ADR docs/decisions/0001), design-token layer, vendored fonts.
   - 8b DONE 2026-07-09 — OIDC auth core in Rust: PKCE S256 + loopback redirect + keychain token store, mock-IdP test suite (docs/auth.md).
   - 8c DONE 2026-07-10 — session freshness (refresh-on-expiry via the tested refresh grant) + real signed-out/signed-in shell states replacing the temporary trigger row.
   - 8e DONE 2026-07-10 — pre-chat shell frame per design-spec §2.1 (collapsible sidebar, thread surface with the §2.6 first-run state, composer shell without send, artifact rail).
   - §1.8 ring DONE 2026-07-10 — milled ring as a tested component with rest + thinking states.
   - 8f DONE 2026-07-10 — §2.6 server-unreachable state: structured error kinds from the auth commands + full-surface mono notice (what happened, retry countdown, Copy diagnostics).
   - 8d BLOCKED on cloud Phase 1 — client registration + real handshake against api.muniment.ai, entitlement snapshot fetch + display (profile block "Your access" peek).
9. Pi sidecar (RPC over stdio), chat against the user's virtual key (depends cloud 6 + item 8). Streaming = signal underline + caret; the provenance line lands with this item. Before session/chat persistence is implemented, use the local run journal contract below. Pi RPC wiring stays blocked on cloud 6.
10. Local model sidecar (llama.cpp + resident Gemma quant), health-managed. Shared 9/10 groundwork is DONE 2026-07-10 and stub-binary tested with no Tauri or network: process supervision; typed line-delimited JSON-RPC; notifications and normalized receive; generation-safe restart; status events and bounded stderr; cancellation and abandoned-response hygiene; non-racing health probes; readiness-aware slow startup; and the split jsonrpc/io/supervisor module structure.
    - NEXT — a loopback-only managed llama-server launcher and tested HTTP health/client boundary, followed by resident model selection and the two local roles (dictation polish + routing classification).
11. Attachments pipeline + content-addressed file store client. The store itself is control-plane-side (harness-spec §6.6: sha256-addressed via the cloud file store), so the client pipeline follows items 8d/9.
    - Groundwork DONE 2026-07-10 — content-addressed local object store in muniment-core: atomic publish + dedup, constant-memory streaming put/get/verify, stale temp-file sweep (docs/cas.md).

- DECIDED 2026-07-10 — **Durable local run journal.** ADR 0002 makes a per-run append-only SQLite event journal the source of truth for live and resumed state. Deterministic replay reconstructs pending permission gates and terminal/needs-attention states; recorded external effects are never silently re-executed. Large bodies live in the local content-addressed store and journal events hold hashes. Receipt/provenance projections (§11.3) and the mobile session relay (§12) consume this same journal rather than inventing parallel histories.
  - Slice 1 DONE 2026-07-10 — SQLite schema, versioned envelope, validated open, atomic append contract, and contract tests.
  - Slice 2 DONE 2026-07-10 — deterministic run-state reducer with permission, terminal, unsupported-safety-event, incremental replay, and crash-at-effect-boundary fixtures.
  - NEXT — Pi domain/effect translation and receipt projection when its cloud dependency opens; then retention/export/deletion/CAS collection/compaction; relay projection remains deferred to its existing relay work.

- PLANNED — **Capability vocabulary + receipt provenance.** Keep the in-flight 2.x slices above unchanged. Follow-up client waves make every end-user palette/library surface say “capabilities,” support the approved one-line description before deferred loading, and render receipts as `route · model · cost · time · capability@version[, ...]`. Real `capability@version` values depend cross-repo on MUNICLOUD's capability schema; the desktop client renders what its entitlement snapshot and receipt provide and does not synthesize provenance.

## Phase 3 — Routing metadata + voice (§9 items 12, 15)
- Classifier labels ride as request metadata; model pin chip for `router.override` holders only.
- Voice: Parakeet capture → Gemma polish (flash) → transforms; Kokoro read-aloud; global hotkeys (⌥Space; Windows binding decided in-build).

## Phase 4+ — Org surface (§9 items 16-19)
- Remote MCP consumption, local stdio allowlist, capability install flow (Pi packages remain the distribution format), artifact side panel, projects (Threads/Shared/Artifacts/Inbox/Connections) with the redaction rule (`output withheld · connection not granted`).

## Standing gates
- Code PRs: structure smoke followed by green desktop-CI builds on all three platforms (Linux, Windows, and macOS). Docs-only PRs (markdown-only diffs) gate on the structure smoke alone — markdown cannot break a platform build. Pushes to `main` run the smoke only.
- This repo builds macOS/Windows/Linux. The mobile companion app is in scope product-wide (harness-spec §12); do NOT scaffold mobile targets in this repo unless/until the §12.5 repo-strategy ADR chooses the shared-workspace path (owner-gated).
