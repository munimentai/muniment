# muniment-desktop — ROADMAP

Phases mirror harness-spec §9. **This lane is CLOSED until muniment-cloud
Phase 1 (OIDC + grants + key regen) is live** — the shell's first real
milestone authenticates against it (owner-set gate, 2026-07-08).

Gate reading in practice (2026-07-10, per the merged 2.8a/2.8b waves):
client-side slices that are fully verifiable locally — toolchain, protocol
core proven against an in-process mock IdP in unit tests — proceed.
Anything that needs the live control plane — desktop client registration
on api.muniment.ai, the first real handshake, entitlement snapshot fetch,
websocket-pushed refresh — stays BLOCKED, not mocked (SPEC law 1), until
the owner declares cloud Phase 1 live.

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
   - 8c NEXT — session freshness (refresh-on-expiry via the tested refresh
     grant) + real signed-out/signed-in shell states replacing the
     temporary trigger row.
   - 8d BLOCKED on cloud Phase 1 — client registration + real handshake
     against api.muniment.ai, entitlement snapshot fetch + display
     (profile block "Your access" peek).
9. Pi sidecar (RPC over stdio), chat against the user's virtual key
   (depends cloud 6 + item 8). Streaming = signal underline + caret; the
   provenance line lands with this item.
10. Local model sidecar (llama.cpp + resident Gemma quant), health-managed.
11. Attachments pipeline + content-addressed file store client.

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
- Every PR: structure smoke followed by green desktop-CI builds on all three
  platforms (Linux, Windows, and macOS). Pushes to `main` run the smoke only.
- Mobile is never engineered. macOS/Windows/Linux only.
