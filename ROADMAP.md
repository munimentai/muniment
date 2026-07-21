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
- DONE — pinned Parakeet/Silero acquisition and publication, sherpa-onnx packaging/bindings, fixed-capacity microphone capture, bounded utterance segmentation, safe VAD boundary, chunk-invariant dictation composition, desktop command/event wiring from native capture through recognition with redacted statuses, basic composer dictation controls/transcript insertion, a reproducible target-hardware evaluator, a bounded 100-utterance endurance mode, and composer press-and-hold dictation with Escape-to-cancel/restore.
- DONE 2026-07-19 — accepted ADR 0014 pins the `ggml-org/llama.cpp` release `b10068` CPU-baseline archives for all four native targets, with a complete-entry-manifest extraction contract and pre-spawn re-verification.
- DONE 2026-07-20 — ADR 0014 implementation through activation: pinned llama-server descriptors for all four targets, manifest-verified safe extraction and reusable tree verification, bounded download with staging/lock/pointer publication, pre-spawn re-verification at the spawn boundary, supervised llama-server activation with health readiness, and third-party notice content; the required Qwen3.5-4B descriptor rides the same verified path with background required-download acquisition deliberately detached from onboarding.
- NEXT — the desktop dictation-polish command against the activated resident server (ticket filed 2026-07-20); the composer polish UI, transforms, and global OS hotkeys follow as separate slices. Physical target-hardware runs still require the approved external corpus and representative machines.

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
- DONE E0–E14 — extension-only real-profile architecture through the injected, memory-only pairing handoff and MV3 worker authorized relay provider.
- BLOCKED E15 — desktop-to-extension handoff delivery waits for the owner to upload a real package to the existing private Chrome Web Store draft and provide its public key. The development manifest must use that matching public key; this repository must not generate a replacement key/ID or claim real handoff validation against an unpinned extension.
- After the matching public key lands, compose and validate E15 as one slice. Later slices add runtime tools, entitlement and ask/allow/deny policy, journal receipts, and kill switch.
- RESERVED EXTENSION ID — `cdedcfbgomnhfpifpgdlpfkkanaofkjd`; add its public `key` only after owner package upload and retrieval from the CWS Package tab, and replace it only after an explicit owner-directed store migration.
- Chrome Web Store publication/upload are owner-gated. v1 excludes OS, filesystem, and other-application control.

## Onboarding, memory, and the Muniment Home (owner rulings 2026-07-19)
- RATIFIED 2026-07-19 (operator session) — the user-visible **Muniment Home** folder: plain, human-editable markdown under `memory/`, `agents/`, `projects/<name>/`, `sessions/`; files are the source of truth and any index (sqlite/embeddings) is a rebuildable cache; never a hidden-dotdir primary store, no proprietary memory DB, no muniment-built sync (the Home rides the user's existing sync); semantic session-transcript names and a visible retention policy.
- RATIFIED — per-surface defaults: desktop shows an unskippable first-run directory picker (default `Documents/Muniment`, changeable later); CLI and VS Code treat the opened directory as the workspace memory location by default, with the user-level Home created lazily on first cross-project need. AGENTS.md is honored for repo instructions (open standard, nearest-file monorepo resolution); instructions and memory are distinct layers.
- RATIFIED — one REQUIRED on-device model: Qwen3.5-4B instruct (Q4 GGUF, Apache 2.0), with a dual role — onboarding triage AND per-query front-door router (cloud / proxy / local) — pinned by checksummed descriptor on the ADR 0008 verified-acquisition path and served by the ADR 0014 llama-server line; swappable by descriptor bump; voice/audio models stay optional. Onboarding import is consent-gated with preview, provenance frontmatter, verbatim originals, and a user-confirmed report before scaffolding; folder setup fails open, only AI-dependent features fail closed.
- RATIFIED (same session) — the agent system prompt model: five binding rules (contracts not taste; nothing always-on that isn't always-true; static hand-written base + runtime-generated labeled-data tail; hand-written, versioned, eval-gated prompt text; no negative fixation lists), a verbatim base prompt v0 (well under 500 tokens static), and a per-session generated tail of length-capped, audited, labeled data fields — org-authored text can never masquerade as system instructions.
- DONE 2026-07-20 — both rulings are codified as harness-spec §15/§16, and the required Qwen3.5-4B pinned descriptor plus background required-download acquisition landed on the verified path.
- DONE 2026-07-21 — desktop first-run Home picker and visible scaffold, bounded read-only preview of an explicitly chosen assistant-export ZIP, and the manifest review UI.
- NEXT — bounded extraction of only explicitly selected ZIP entries, preserving verbatim text and provenance without Home writes or model calls (ticket filed 2026-07-21). Consent checklist UI, local-model triage, user-confirmed report, and import/scaffolding writes follow as separate slices. Required-download visibility/status UI also remains. CLI/VS Code inherit-opened-directory defaults wait for workspace memory semantics.
- Open items: headless/server CLI stance (owner ruling pending); the descriptor has landed but the final model choice/promotion remains gated on the MUNIQA routing eval (a descriptor bump swaps the model if the eval selects otherwise); Gemma 4 E2B is tracked only as a future replacement contingency pending llama.cpp PLE support (ggml-org/llama.cpp#22243), and adopting it would require a new owner ruling. Ripple: MUNICLOUD artifact proxy/redirect for the model download.

## Desktop QA automation
- RATIFIED 2026-07-19 — add a layered desktop QA lane: cheap frontend browser coverage plus installed-nightly real-app validation on the serialized pve01 desktop-ci VMs.
- Windows and Linux receive full WebdriverIO automation through the Tauri WebDriver seam; macOS remains install/launch smoke plus Proxmox screendumps and a short manual owner pass. Do not substitute paid device clouds or claim full macOS desktop automation.
- DONE groundwork — accepted ADR 0013 records the harness, security boundary, VM/install lifecycle, real-auth fixture ownership, artifact/log retention, and serialization contract.
- UNGATED 2026-07-19 — the canonical desktop E2E runner contract is published with `--collect-artifacts`, `--screendump`, and `--env-stdin`.
- DONE 2026-07-19 — the canonical Linux `.deb` installed launch + real-sign-in WDIO smoke runs in nightly via desktop-ci with fail-closed pinned-asset identity, secret-stdin fixture credentials, an idempotent finalizer, and redacted diagnostic bundles retained 7/30 days; the lane emits JUnit XML under the stable `linux-e2e-report` artifact and probes the installed `/usr/bin/muniment-desktop` binary correctly.
- DONE 2026-07-19 — the Windows per-user MSI installed launch + real-sign-in smoke, serialized after the Linux case on the pve01 lock and the exclusive fixture lease.
- DONE 2026-07-19 — the macOS install/launch smoke per ADR 0013: pinned `.app.zip` identity, `/Applications` install, healthy-first-window assertion, and Proxmox screendump in nightly; no WDIO, no automated sign-in.
- DONE 2026-07-20 — stable `windows-e2e-report` JUnit artifact parity for the Windows lane.
- NEXT — extend the Linux installed nightly past sign-in with one bounded real chat round-trip (ticket filed 2026-07-20); Windows chat parity, attachment flows, and automatic failure-to-ticket reporting follow as separate slices.
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
