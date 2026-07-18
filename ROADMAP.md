# muniment-desktop — ROADMAP

Phases mirror harness-spec §9. Muniment-cloud Phase 1 native auth and cloud prerequisites are live; client work must exercise real contracts and must not add mocked production paths.

## M0 — Scaffold
DONE 2026-07-09: Tauri v2 shell, specs, design reference, test harnesses, and CI gates.

## Phase 2 — Client core (§9 items 8–11)

### 8. Shell, auth, and entitlements
DONE: Svelte/Vite shell, design tokens, signed-in/out states, native installation-bound authorization, keychain persistence, refresh/revocation, entitlements, device states, and unreachable recovery. Production auth is `/v1/auth/native/*`; generic OIDC is test groundwork only.

### 9. Pi sidecar and cloud chat
DONE: verified Pi runtime supervision, streamed chat, steering/follow-up, durable tool effects, provenance, tool cards, extension UI protocol, permission-gate replay, and interrupted-session resume. Answering a pending permission gate remains Needs Human.

### 10. Local Gemma sidecar
DONE: supervised local runtime, pinned model lifecycle, verified/cancellable acquisition, rollback, adapters, and Tauri commands. NEXT: first-use install UI and release notice delivery as separate slices after approved bundled terms exist.

### 11. Attachments and local CAS
DONE groundwork: atomic content-addressed storage, constant-memory I/O/verification, cleanup, journal reference accounting, retention, export, and compaction. Cloud flow follows items 8d/9.

### Durable local run journal
ADR 0002 makes the per-run append-only SQLite journal authoritative. DONE: schema/envelope/append, reducer/replay, Pi translation, lifecycle operations, export/compaction, summary listing, and authorized redacted Linux thread reads. No journal-maintenance slice is selected.

### Capability vocabulary and provenance
DONE: user surfaces say “capabilities”; receipts use only server-supplied provenance.

## Phase 3 — Routing metadata + voice (§9 items 12, 15)
Routing policy follows item 9; the classifier contract is complete. Voice remains Parakeet capture → Gemma polish → transforms, with Kokoro read-aloud and hotkeys. DONE: pinned acquisition/publication, sherpa-onnx packaging, bounded capture/VAD/composition, and desktop dictation command/event wiring. NEXT: dictation UI and target-hardware validation as independent slices.

## Companion execution surfaces (§13)
DONE E0/E0.5: ADRs 0009 and 0011 establish the bounded attach protocol, pairing, secure Linux transport, repository ownership, package boundaries, CI, and distribution implications. ADR 0012 selects one per-user background owner; extraction/installer integration is separate and does not block protocol-identical app-managed E1 slices.

DONE E1 read/write groundwork: CLI discovery/pairing, thread list/open, bounded authorized `run.start`, shared coordinator integration, durable idempotency, and receipt-only `muniment run start`.

DONE E1 stream server and catch-up client: bounded redacted journal catch-up, workspace isolation, caught-up marker, flow control and cursor acknowledgments; the protocol client validates and continues paused catch-up.

DONE E1 live-tail server: bounded loss-tolerant journal commit hints wake the authorized Linux session, which re-reads the journal and delivers newly committed events without missing the subscription-snapshot race, while preserving workspace isolation and flow control.

NEXT E1: protocol-client live consumption after the caught-up marker. Subsequent independent slices add terminal rendering and permission-answer parity. E2 may proceed concurrently in its ADR 0011 TypeScript lane; VSIX sideload testing is sufficient and publication remains owner-gated. The runtime service remains sole runtime/session/journal owner; surfaces are clients. macOS/Windows attach adapters are future slices.

## Browser-control runtime capability (§6.8)
OWNER GO 2026-07-16: browser-only governed actuator work may proceed. DONE E0–E8: corrected extension-only architecture; pinned Apache-2.0 relay core; fail-closed one-use pairing; Linux executable and socket-owner verification; composed authorization; numeric-loopback listener; bounded RFC 6455 upgrade; and pairing-token consumption. NEXT slices: macOS/Windows identity adapters, unpacked MV3 extension/connect-tab lifecycle, runtime-tool integration, entitlement and ask/allow/deny policy, journal receipts, and kill switch. Reserved extension ID: `cdedcfbgomnhfpifpgdlpfkkanaofkjd`; add the store public key only after owner upload. Publication/upload are owner-gated; v1 excludes OS/filesystem/other-app control.

## Stable release and distribution
DONE: pinned-SHA nightly builds and owner-triggered strict-SemVer stable promotion; Windows signing verified 2026-07-15. DONE groundwork: MSI hashing and draft WinGet PR generation. Accounts, review/publication, Apple signing, and other channels remain owner-gated. No monetization surface is implied.

## Phase 4+ — Org surface (§9 items 16–19)
Remote MCP, local stdio allowlist, capability install, artifact rail, and projects with `output withheld · connection not granted`.

## Standing gates
Code PRs run structure smoke, frontend/Rust tests, then desktop builds. Markdown-only PRs use smoke alone; pushes to main run smoke only. Build desktop targets unless an accepted lane ADR says otherwise. Accounts, store publication, launch, and publicity remain owner-gated.
