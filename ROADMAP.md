# muniment-desktop — ROADMAP

Phases mirror harness-spec §9. Muniment-cloud Phase 1 native auth and the cloud prerequisites for desktop chat are live as of 2026-07-11; client work that uses them must exercise the real contracts and must not introduce mocked production paths.

## M0 — Scaffold (done 2026-07-09)
- DONE — Tauri v2 shell, specs, design reference, Rust/frontend test harnesses, and CI gates.

## Phase 2 — Client core (§9 items 8–11)

### 8. Shell, auth, and entitlements
- DONE — Svelte shell, native installation-bound authorization, session/revocation flows, entitlement projection, and recovery states.
- Production auth uses `/v1/auth/native/*`; generic OIDC is test groundwork only.

### 9. Pi sidecar and cloud chat
- DONE — verified Pi supervision, streamed chat, steering/follow-up, durable effects, provenance/receipts, tool cards, extension UI, permission-gate replay, and interrupted-session resume.
- Answering a pending permission gate remains blocked on the Needs Human decision.

### 10. Local Gemma sidecar
- DONE — supervised runtime, pinned model lifecycle, verified/cancellable acquisition, rollback, native adapters, and Tauri commands.
- NEXT after approved bundled terms exist — first-use install UI and release notice delivery as separate slices.

### 11. Attachments and local CAS
- DONE groundwork — atomic content-addressed storage, streaming I/O/verification, cleanup, journal reference accounting, retention, export, and compaction. Cloud file flow follows items 8d/9.

### Durable local run journal
- DECIDED — ADR 0002 makes a per-run append-only SQLite event journal authoritative; external effects are never silently re-executed and large bodies live in CAS.
- DONE — schema, append/reducer/replay, Pi translation, lifecycle maintenance, export, compaction, run-summary listing, and authorized redacted `thread.list`/`thread.open`.

### Capability vocabulary and provenance
- DONE — user surfaces say “capabilities”; receipts render only server-supplied route/model/cost/time/capability provenance.

## Phase 3 — Routing metadata + voice (§9 items 12, 15)
- DONE groundwork — local routing classifier plus pinned Parakeet/Silero acquisition, sherpa packaging, bounded capture/segmentation, dictation composition, and desktop command/event wiring.
- NEXT — dictation UI and target-hardware validation as independent slices.

## Companion execution surfaces (§13)
- DONE E0/E0.5 — ADRs 0009 and 0011, bounded protocol/codecs, negotiation/pairing, idempotency/cursor/artifact windows, secure Linux transport, repository/dependency boundaries, and path-scoped CI.
- DECIDED — ADR 0012 promotes Pi and journal ownership to one per-user service. Cross-platform extraction/installer integration is separate; initial E1 slices may use the protocol-identical app-managed owner.
- DONE E1 read path — Linux discovery/pairing, cursor-navigable thread listing, bounded journal-backed thread opening, and interactive CLI readers.
- DONE E1 write groundwork — bounded authorized `run.start`, protocol client, shared desktop coordinator, production adapter with durable idempotency, and receipt-only interactive `muniment run start`.
- NEXT E1 — serve a bounded, redacted journal catch-up for `run.stream` through the authorized Linux attach session. Later independent slices add the protocol client, cursor acknowledgements/live tail, terminal rendering, and permission-answer parity.
- E2 may proceed concurrently in its scoped TypeScript lane; VSIX sideload testing is sufficient and marketplace publication is owner-gated.
- The runtime service is the sole runtime/session/journal owner; surfaces are clients. macOS and Windows attach adapters remain future slices.

## Browser-control runtime capability (§6.8)
- OWNER GO 2026-07-16 — v1 is a governed browser-only actuator, not a companion surface.
- DONE E0–E8 — extension-only architecture correction, attributed relay core, fail-closed token pairing, Linux executable/socket-owner authorization, numeric-loopback listener, bounded WebSocket upgrade, and single-use pairing-token consumption.
- NEXT later — macOS/Windows identity adapters, unpacked MV3 extension/connect-tab lifecycle, runtime tools, entitlement/policy gates, journal receipts, and kill switch.
- RESERVED EXTENSION ID — `cdedcfbgomnhfpifpgdlpfkkanaofkjd`; pin it when the MV3 slice lands. Add the store public key only after owner package upload. Publication/upload remain owner-gated.

## Stable release and distribution
- DONE — pinned-SHA nightly builds, owner-triggered strict-SemVer stable promotion, Windows signing, per-machine MSI hashing, WinGet manifest generation, and draft PR automation.
- Owner account setup, publication, Homebrew, and Apple signing remain gated. No new monetization or promotion is implied.

## Phase 4+ — Org surface (§9 items 16–19)
- Remote MCP, local stdio allowlist, capability installation, artifact side panel, and projects with explicit withheld-output redaction.

## Standing gates
- Code PRs: structure smoke, frontend/Rust tests, then required platform builds. Markdown-only PRs use structure smoke; pushes to `main` run smoke only.
- Build desktop targets only unless an accepted ADR defines another surface lane.
- Distribution accounts, store publication, production launch, publicity, and monetization remain owner-gated.
