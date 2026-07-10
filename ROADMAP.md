# muniment-desktop — ROADMAP

Phases mirror harness-spec §9. **This lane is CLOSED until muniment-cloud
Phase 1 (OIDC + grants + key regen) is live** — the shell's first real
milestone authenticates against it (owner-set gate, 2026-07-08).

Gate reading in practice (updated 2026-07-10): client-side slices that are
fully verifiable locally — toolchain, protocol core proven against an
in-process mock IdP in unit tests, sidecar transport proven against a stub
binary, UI states that fabricate nothing — proceed. Anything that needs the
live control plane — desktop client registration on api.muniment.ai, the
first real handshake, entitlement snapshot fetch, websocket-pushed refresh —
stays BLOCKED, not mocked (SPEC law 1), until the owner declares cloud
Phase 1 live.

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
   - 8e IMPLEMENTED 2026-07-10, in review — pre-chat shell frame per
     design-spec §2.1 (collapsible sidebar, thread surface with the §2.6
     first-run state, composer shell without send, artifact rail). Ticket
     complete; PR not yet on main at this update.
   - §1.8 ring IMPLEMENTED 2026-07-10, in review — milled ring as a tested
     component with rest + thinking states. PR not yet on main.
   - 8f IMPLEMENTED 2026-07-10, in review — §2.6 server-unreachable state:
     structured error kinds from the auth commands + full-surface mono
     notice (what happened, retry countdown, "Copy diagnostics"). PR not
     yet on main.
   - 8d BLOCKED on cloud Phase 1 — client registration + real handshake
     against api.muniment.ai, entitlement snapshot fetch + display
     (profile block "Your access" peek).
9. Pi sidecar (RPC over stdio), chat against the user's virtual key
   (depends cloud 6 + item 8). Streaming = signal underline + caret; the
   provenance line lands with this item.
10. Local model sidecar (llama.cpp + resident Gemma quant), health-managed.
    Shared 9/10 groundwork — all stub-binary tested, no Tauri, no network:
    - DONE 2026-07-10 — sidecar process supervisor in muniment-core:
      spawn/stdio/health/crash-restart/shutdown (docs/sidecar.md).
    - DONE 2026-07-10 — typed, line-delimited JSON-RPC 2.0 framing over
      the supervisor's stdio.
    - DONE 2026-07-10 — JSON-RPC notifications (send + interleaved receive
      during calls) and reader-line normalization (CRLF, blank keepalives).
    - DONE 2026-07-10 — JSON-RPC ping health probe through the shared
      transport, sharing the call lock so it cannot race in-flight calls.
    - DONE 2026-07-10 — restart-safe transport: generation-tagged I/O so
      stale output from a dead process generation cannot corrupt
      post-restart calls.
    - DONE 2026-07-10 — supervisor status-change events: ordered Starting/
      Healthy/Restarting/Failed/Stopped transitions with cause, generation,
      attempt, and backoff metadata pushed to subscribers.
    - DONE 2026-07-10 — bounded stderr diagnostics: producer-side retained
      line ring, generation reset, and recent stderr tails on restart/failure
      causes.
    - DONE 2026-07-10 — call cancellation + abandoned-request hygiene:
      cancel an in-flight JSON-RPC call from another thread (cancel
      notification, method name configurable) and silently discard late
      responses to cancelled or timed-out IDs via a bounded,
      generation-aware queue (closes the timeout→MismatchedId hole).
    - DONE 2026-07-10 — health probing coexists with long-running calls:
      probes never wait on the application call lock or steal its frames,
      idle hangs still fail, and shutdown is not stalled by a long call.
    - DONE 2026-07-10 — split the former 1,300-line sidecar module into
      jsonrpc / io / supervisor submodules following the auth pattern;
      public API unchanged.
    - NEXT — readiness-aware supervision for slow-loading processes: keep a
      generation Starting while its probe reports model loading, bound that
      startup interval, and preserve restart/diagnostic behavior after it is
      ready. This is the first prerequisite for managing llama-server, whose
      health endpoint distinguishes loading from ready.
    - THEN — a loopback-only managed llama-server launcher and tested HTTP
      health/client boundary, followed by resident model selection and the
      two local roles (dictation polish + routing classification).
    Pi RPC wiring stays blocked on cloud 6.
11. Attachments pipeline + content-addressed file store client. The store
    itself is control-plane-side (harness-spec §6.6: sha256-addressed via
    the cloud file store), so the client pipeline follows items 8d/9.
    - Groundwork DONE 2026-07-10 — content-addressed local object store in
      muniment-core: atomic publish + dedup, constant-memory streaming
      put/get/verify, stale temp-file sweep (docs/cas.md).

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
