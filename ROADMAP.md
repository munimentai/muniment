# muniment-desktop — ROADMAP

Phases mirror harness-spec §9. Muniment-cloud Phase 1 native auth and the cloud prerequisites for desktop chat are live as of 2026-07-11; client work that uses them must exercise the real contracts and must not introduce mocked production paths.

## M0 — Scaffold (done 2026-07-09)
- Tauri v2 desktop shell builds on macOS, Windows, and Linux.
- Specs, mockups, design reference, Rust/frontend test harnesses, and CI gates are present.

## Phase 2 — Client core (§9 items 8–11)

### 8. Shell, auth, and entitlements
- DONE — Svelte 5/Vite shell, design tokens, signed-out/signed-in states, native installation-bound authorization, coherent keychain persistence, refresh/session/revocation flows, live entitlement projection, access-device states, and server-unreachable recovery.
- The production contract is `/v1/auth/native/*`; generic OIDC remains test groundwork and is not the production handshake.

### 9. Pi sidecar and cloud chat
- DONE — verified Pi runtime acquisition/supervision, signed-in streamed chat, steering/follow-up, durable tool effects, provenance/receipts, inline tool cards, blocking extension-UI protocol, pending permission-gate replay, and safe interrupted-session resume.
- Answering a pending permission gate remains blocked on the Needs Human decision.

### 10. Local Gemma sidecar
- DONE — supervised local runtime, pinned model lifecycle, verified/cancellable acquisition, rollback, native adapters, and Tauri install/status/cancel commands.
- NEXT — explicit first-use install UI and release notice delivery as separate slices after the approved bundled terms surface is available.

### 11. Attachments and local CAS
- DONE groundwork — pure-Rust content-addressed local store with atomic deduplication, constant-memory I/O/verification, stale-temp cleanup, journal reference accounting, retention, export, and compaction. Cloud file flow follows items 8d/9.

### Durable local run journal
- DECIDED — ADR 0002 makes a per-run append-only SQLite event journal authoritative; external effects are never silently re-executed and large bodies live in CAS.
- DONE — schema/envelope/atomic append, reducer/replay, Pi event translation, deletion/collection/retention, deterministic export, crash-safe compaction, deterministic cursor-paginated run-summary listing, and authorized Linux companion `thread.list` backed by those redacted journal summaries.
- No further journal-maintenance slice is selected.

### Capability vocabulary and provenance
- DONE — user surfaces say “capabilities” and receipts render only server-supplied route/model/cost/time/capability provenance.

## Phase 3 — Routing metadata + voice (§9 items 12, 15)
- Routing metadata carriage/policy integration follows item 9; the local classifier contract is complete.
- Voice direction remains Parakeet capture → Gemma polish → transforms, with Kokoro read-aloud and global hotkeys.
- DONE — pinned Parakeet/Silero acquisition and publication, sherpa-onnx packaging/bindings, fixed-capacity microphone capture, bounded utterance segmentation, safe VAD boundary, chunk-invariant pure-core dictation composition, and desktop command/event wiring from owned native capture through installed Silero/Parakeet recognition with redacted statuses.
- NEXT — dictation UI and target-hardware validation as independent slices.

## Companion execution surfaces (§13)
- DONE E0 — ADR 0009, bounded protocol/codecs, negotiation and explicit pairing authorization, idempotency ledger, cursor/artifact windows, secure Linux filesystem/socket transport, and authorized `thread.list` backed by redacted journal run summaries.
- OWNER GO 2026-07-16 — CLI E1 and editor-extension E2 may proceed concurrently after E0.5; the earlier one-front-at-a-time note is superseded.
- NEXT E0.5 — owner-ratify proposed ADR 0011, which selects repository locations, dependency boundaries, path-scoped build/test CI, and distribution implications for both surfaces. No E1/E2 scaffold precedes ratification.
- AFTER ADR — decompose E1 into attach/pair, thread list/read, send, run stream, and permission-answer parity slices. E2 scaffolding may begin in its selected lane once the same ADR is accepted; VSIX sideload testing is sufficient and marketplace publication remains owner-gated.
- Desktop remains the sole runtime/session/journal owner; companions are windows, not modes. macOS and Windows attach adapters remain separate future slices.

## Browser-control runtime capability (§6.8)
- OWNER GO 2026-07-16 — open the v1 browser-only actuator line in this repository. It is a governed runtime capability, not another companion surface.
- DONE E0 correction — the architecture/spec requires extension-only access to real profiles, token plus executable-path relay pairing, and connect-tab anchor lifecycle ownership.
- DONE E1 — pinned, attributed, upstream-mergeable Apache-2.0 Playwright relay request/response/event core behind an injected transport, with bounded adversarial tests and independent CI coverage.
- DONE E2 — transport-independent fail-closed pairing session with a single-use 256-bit expiring token, injected OS-owned identity verification, rotation/revocation, concurrency safety, and redacted reason codes.
- DONE E3 — Linux pure-core executable identity verification canonicalizes desktop-selected and proc-owned paths, revalidates process start identity against PID reuse, compares exact path bytes, and exposes only redacted failures.
- DONE E4 — Linux numeric-loopback socket-owner resolution maps one accepted TCP connection through the kernel socket diagnostic interface to exactly one same-user live process/inode and returns its race-checked process start identity.
- NEXT E5 — compose the Linux connection-owner resolver and executable verifier into one fail-closed pure-core authorization seam that returns only an opaque authorized-browser proof. Later slices open the desktop loopback transport and bind it to token pairing, add macOS/Windows identity adapters, implement the unpacked MV3 extension/connect-tab lifecycle, integrate runtime tools, enforce entitlement and ask/allow/deny policy (domain/read-vs-act/sensitive approval), write journal receipts, and wire the kill switch.
- Chrome Web Store publication is owner-gated. v1 excludes OS, filesystem, and other-application control.

## Stable release and distribution
- DONE — rolling nightly release builds one pinned SHA across Linux, signed Windows, and unsigned macOS artifacts; Windows signing was verified 2026-07-15. An owner-triggered strict-SemVer promotion copies one green nightly SHA's exact artifacts to a `vX.Y.Z` stable GitHub Release, documents cadence/version rules, leaves nightly unchanged, and labels unsigned macOS honestly.
- DONE groundwork — stable promotion hashes the signed per-machine MSI, generates a current `Muniment.Muniment` WinGet manifest, and opens a draft PR from the owner's configured `winget-pkgs` fork. Fork/token setup, review, publication, and other distribution accounts remain owner-gated.
- Homebrew and other Apple distribution remain Apple-account/signing gated. No new monetization or promotional surface is implied.

## Phase 4+ — Org surface (§9 items 16–19)
- Remote MCP consumption, local stdio allowlist, capability install flow, artifact side panel, and projects with the redaction rule (`output withheld · connection not granted`).

## Standing gates
- Code PRs: structure smoke, frontend/Rust tests, then Linux/Windows/macOS desktop builds. Markdown-only PRs gate on structure smoke alone; pushes to `main` run smoke only.
- Build desktop targets only unless an accepted repository/lane ADR explicitly changes a companion surface's checks.
- Distribution accounts, marketplace/store publishing, production launch, and publicity remain owner-gated.
