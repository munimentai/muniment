# muniment-desktop — ROADMAP

Phases mirror harness-spec §9. Muniment-cloud Phase 1 native auth and the cloud prerequisites for desktop chat are live as of 2026-07-11; client work that uses them must exercise the real contracts and must not introduce mocked production paths.

## M0 — Scaffold (done 2026-07-09)
- DONE — Tauri v2 desktop shell, specs, mockups, design reference, Rust/frontend test harnesses, and Linux/Windows/macOS CI gates.

## Phase 2 — Client core (§9 items 8–11)

### 8. Shell, auth, and entitlements
- DONE — Svelte 5/Vite shell, design tokens, signed-out/signed-in states, native installation-bound authorization, keychain persistence, refresh/session/revocation flows, live entitlement projection, access-device states, and server-unreachable recovery.
- The production contract is `/v1/auth/native/*`; generic OIDC remains test groundwork and is not the production handshake.

### 9. Pi sidecar and cloud chat
- DONE — verified Pi runtime acquisition/supervision, signed-in streamed chat, steering/follow-up, durable tool effects, provenance/receipts, inline tool cards, extension UI, permission-gate replay/answers, and safe interrupted-session resume.

### 10. Local Gemma sidecar
- DONE — supervised local runtime, pinned model lifecycle, verified/cancellable acquisition, rollback, native adapters, and Tauri install/status/cancel commands.
- NEXT — explicit first-use install UI and release notice delivery as separate slices after the approved bundled terms surface is available.

### 11. Attachments and local CAS
- DONE groundwork — pure-Rust content-addressed local store with atomic deduplication, constant-memory I/O/verification, stale-temp cleanup, journal reference accounting, retention, export, and compaction. Cloud file flow follows items 8d/9.

### Durable local run journal
- DECIDED — ADR 0002 makes a per-run append-only SQLite event journal authoritative; external effects are never silently re-executed and large bodies live in CAS.
- DONE — schema/envelope/atomic append, reducer/replay, Pi translation, deletion/collection/retention, deterministic export, crash-safe compaction, cursor-paginated run summaries, and authorized Linux companion `thread.list`/`thread.open` backed by redacted projections.
- No further journal-maintenance slice is selected.

### Capability vocabulary and provenance
- DONE — user surfaces say “capabilities” and receipts render only server-supplied route/model/cost/time/capability provenance.

## Phase 3 — Routing metadata + voice (§9 items 12, 15)
- Routing metadata carriage/policy integration follows item 9; the local classifier contract is complete.
- Voice direction remains Parakeet capture → Gemma polish → transforms, with Kokoro read-aloud and global hotkeys.
- DONE — pinned Parakeet/Silero acquisition and publication, sherpa-onnx packaging/bindings, fixed-capacity microphone capture, bounded utterance segmentation, safe VAD boundary, chunk-invariant dictation composition, and desktop command/event wiring from native capture through recognition with redacted statuses.
- NEXT — dictation UI and target-hardware validation as independent slices.

## Companion execution surfaces (§13)
- DONE E0 — ADR 0009, bounded protocol/codecs, negotiation and explicit pairing authorization, idempotency ledger, cursor/artifact windows, secure Linux filesystem/socket transport, and authorized redacted journal-backed thread reads.
- OWNER GO 2026-07-16 — CLI E1 and editor-extension E2 may proceed concurrently after E0.5.
- DONE E0.5 — owner-ratified ADR 0011 selects this repository for both surfaces, with dependency boundaries, path-scoped build/test CI, and distribution implications.
- DECIDED — ADR 0012 promotes Pi and journal ownership to one per-user background service installed idempotently by any surface. Cross-platform extraction and installer integration are a separate build line; E1 may initially use the protocol-identical app-managed owner.
- DONE E1 — Rust workspace and path-scoped CLI lane; Linux discovery/pairing; thread list/open; run start with ordered catch-up/live progress and flow control; typed in-terminal permission handling; inline tool details; and authoritative receipts.
- DONE E2 — Rust-owned canonical `muniment.attach/1` fixtures and deterministic drift checking; fixture-pinned TypeScript package/test/package CI; bounded Linux/macOS Unix-socket plus Windows named-pipe discovery, framing, pairing, and authorization; native thread list/open and read-only redacted documents; typed run start/stream with prompt, workspace selection or current-file context, ordered live progress and authoritative receipts; and native fail-closed allow/deny interaction for pending permission gates.
- VSIX sideload testing is sufficient; marketplace publication and official fork-compatibility claims remain owner-gated.
- The runtime service is the sole runtime/session/journal owner; desktop, CLI, and editor extension remain client windows, not modes.

## Browser-control runtime capability (§6.8)
- OWNER GO 2026-07-16 — open the v1 browser-only actuator line in this repository; it is a governed runtime capability, not another companion surface.
- DONE E0–E13 — extension-only real-profile architecture; attributed upstream-mergeable relay core; fail-closed single-use pairing; Linux executable identity and loopback socket-owner verification; composed authorization; authorized loopback listener; bounded RFC 6455 upgrade; pairing-token consumption; fail-closed macOS loopback browser-process identity; macOS authorized-listener composition; fail-closed Windows loopback browser-process identity; Windows authorized-listener composition; and the unpacked MV3 extension with its exactly-one visible connect-tab anchor lifecycle.
- NEXT E14 — bridge one injected, memory-only pairing handoff to the MV3 worker's authorized relay provider through a fail-closed numeric-loopback WebSocket adapter.
- Later slices compose desktop-to-extension handoff delivery, then add runtime tools, entitlement and ask/allow/deny policy, journal receipts, and kill switch.
- RESERVED EXTENSION ID — `cdedcfbgomnhfpifpgdlpfkkanaofkjd`; pinning the unpacked manifest through a repository development key remains pending and must land before E14 relies on this ID. Replace that development key only after owner store upload and retrieval of the public store key.
- Chrome Web Store publication/upload are owner-gated. v1 excludes OS, filesystem, and other-application control.

## Desktop QA automation
- RATIFIED 2026-07-19 — add a layered desktop QA lane: cheap frontend browser coverage plus installed-nightly real-app validation on the serialized pve01 desktop-ci VMs.
- Windows and Linux receive full WebdriverIO automation through the Tauri WebDriver seam; macOS remains install/launch smoke plus Proxmox screendumps and a short manual owner pass. Do not substitute paid device clouds or claim full macOS desktop automation.
- DONE groundwork — accepted ADR 0013 records the harness, security boundary, VM/install lifecycle, real-auth fixture ownership, artifact/log retention, and serialization contract.
- BLOCKED — Windows/Linux installed launch + real-sign-in smoke waits for the canonical cross-repository `desktop E2E runner contract` named by ADR 0013; this repository must not invent pve01 operations, VM paths, or fixture secrets. After that contract lands, proceed with launch + sign-in smoke before chat/attachment flows, macOS smoke, and automatic failure-to-ticket reporting as separate slices.
- Nightly QA must test the finalized installer artifact for one pinned SHA and must not introduce mocked production paths. Signing/notarization and the web/API suite remain out of scope.

## Stable release and distribution
- DONE — rolling nightly builds one pinned SHA across Linux, signed Windows, and unsigned macOS; owner-triggered strict-SemVer promotion copies one green nightly SHA's exact artifacts to a stable GitHub Release and labels unsigned macOS honestly.
- DONE groundwork — promotion hashes the signed per-machine MSI, generates a `Muniment.Muniment` WinGet manifest, and opens a draft PR from the configured fork. Fork/token setup, review, publication, Homebrew, and Apple accounts/signing remain owner-gated.
- No new monetization or promotional surface is implied.

## Phase 4+ — Org surface (§9 items 16–19)
- Remote MCP consumption, local stdio allowlist, capability install flow, artifact side panel, and projects with the redaction rule (`output withheld · connection not granted`).

## Standing gates
- Code PRs: structure smoke, frontend/Rust tests, then applicable path-scoped companion checks or Linux/Windows/macOS desktop builds. Markdown-only PRs gate on structure smoke alone; pushes to `main` run smoke only.
- Build desktop targets only unless an accepted repository/lane ADR changes a companion surface's checks.
- Distribution accounts, marketplace/store publishing, production launch, and publicity remain owner-gated.
