# muniment-desktop — ROADMAP

Phases mirror harness-spec §9. **This lane is CLOSED until muniment-cloud
Phase 1 (OIDC + grants + key regen) is live** — the shell's first real
milestone authenticates against it (owner-set gate, 2026-07-08).

Gate reading in practice (updated 2026-07-10, per the merged 2.8a–2.8c
waves and the shell-frame/ring/supervisor slices that followed):
client-side slices that are fully verifiable locally — toolchain, protocol
core proven against an in-process mock IdP in unit tests, UI states that
fabricate nothing — proceed. Anything that needs the live control plane —
desktop client registration on api.muniment.ai, the first real handshake,
entitlement snapshot fetch, websocket-pushed refresh — stays BLOCKED, not
mocked (SPEC law 1), until the owner declares cloud Phase 1 live.

## M0 — Scaffold (done at bootstrap, 2026-07-09)
- Tauri v2 hello-world shell (static webview page, brand-neutral).
- Proven to build through `desktop-ci` on all three platforms.
- Specs + desktop/mobile mockups + ring reference vendored.

## Phase 2 — Client core (§9 items 8-11)
8. Shell + auth handshake + entitlement snapshot consumption (depends
   cloud 4, 5). Frontend framework decision recorded here (React or Svelte,
   Tailwind fine — team's choice per harness-spec §6).
   - 8a DONE 2026-07-09 — frontend toolchain: Svelte 5 + Vite (ADR
     docs/decisions/0001), design-token layer, vendored fonts.
   - 8b DONE 2026-07-09 — OIDC auth core in Rust: PKCE S256 + loopback
     redirect + keychain token store, mock-IdP test suite (docs/auth.md).
   - 8c DONE 2026-07-10 — session freshness (refresh-on-expiry via the
     tested refresh grant) + real signed-out/signed-in shell states
     replacing the temporary trigger row.
   - 8e DONE 2026-07-10 — pre-chat shell frame, fully local and nothing
     fabricated: design-spec §2.1 layout (collapsible sidebar, thread
     surface with the §2.6 first-run state, composer shell without send,
     artifact rail) plus the §1.8 ring promoted to a tested component
     (rest + thinking states). Send/threads/provenance arrive only with
     item 9 — no mocks.
   - 8f NEXT — the §2.6 server-unreachable state: full-surface notice in
     mono ledger style (what happened, retrying countdown, "Copy
     diagnostics"). SPEC law 1 requires this honest state before any real
     handshake ships, and it is fully verifiable locally — it renders
     precisely when the control plane is absent, which is today's reality.
   - 8d BLOCKED on cloud Phase 1 — client registration + real handshake
     against api.muniment.ai, entitlement snapshot fetch + display
     (profile block "Your access" peek).
9. Pi sidecar (RPC over stdio), chat against the user's virtual key
   (depends cloud 6 + item 8). Streaming = signal underline + caret; the
   provenance line lands with this item.
10. Local model sidecar (llama.cpp + resident Gemma quant), health-managed.
    - Groundwork DONE 2026-07-10 (shared with item 9): sidecar process
      supervisor in muniment-core — spawn/stdio/health/crash-restart/
      shutdown, proven against a stub binary in unit tests (docs/sidecar.md).
    - Groundwork NEXT (shared with item 9): line-delimited JSON-RPC framing
      over the supervisor's stdio, proven against the same stub binary —
      still no Tauri, no network. Pi RPC wiring stays blocked on cloud 6;
      llama.cpp + model residency land with item 10 proper.
11. Attachments pipeline + content-addressed file store client. The store
    itself is control-plane-side (harness-spec §6.6: sha256-addressed via
    the cloud file store), so the client pipeline follows items 8d/9.

## Phase 3 — Routing metadata + voice (§9 items 12, 15)
- Classifier labels ride as request metadata; model pin chip for
  `router.override` holders only.
- Voice: Parakeet capture → Gemma polish (flash) → transforms; Kokoro
  read-aloud; global hotkeys (⌥Space; Windows binding decided in-build).

## Phase 4+ — Org surface (§9 items 16-19)
- Remote MCP consumption, local stdio allowlist, package install flow,
  artifact side panel, projects (Threads/Shared/Artifacts/Inbox/
  Connections) with the redaction rule (`output withheld · connection not
  granted`).

## Standing gates
- Code PRs: structure smoke followed by green desktop-CI builds on all
  three platforms (Linux, Windows, and macOS). Docs-only PRs (markdown-only
  diffs) gate on the structure smoke alone — markdown cannot break a
  platform build. Pushes to `main` run the smoke only.
- Mobile is never engineered. macOS/Windows/Linux only.
