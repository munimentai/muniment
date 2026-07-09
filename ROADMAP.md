# muniment-desktop — ROADMAP

Phases mirror harness-spec §9. **This lane is CLOSED until muniment-cloud
Phase 1 (OIDC + grants + key regen) is live** — the shell's first real
milestone authenticates against it (owner-set gate, 2026-07-08).

## M0 — Scaffold (done at bootstrap, 2026-07-09)
- Tauri v2 hello-world shell (static webview page, brand-neutral).
- Proven to build through `desktop-ci` on all three platforms.
- Specs + desktop/mobile mockups + ring reference vendored.

## Phase 2 — Client core (§9 items 8-11; opens when cloud Phase 1 is live)
8. Shell + auth handshake + entitlement snapshot consumption (depends
   cloud 4, 5). Frontend framework decision recorded here (React or Svelte,
   Tailwind fine — team's choice per harness-spec §6).
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
- Every PR: green desktop-CI builds (all three platforms when touched code
  is cross-platform; the workflow wiring lands when the lane opens).
- Mobile is never engineered. macOS/Windows/Linux only.
