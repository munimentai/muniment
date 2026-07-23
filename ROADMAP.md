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
- DONE 2026-07-22 — first-use onboarding shows required local-model acquisition progress, readiness, and redacted background-retry status while keeping folder setup fail-open and AI proposal generation fail-closed.
- NEXT — release notice delivery after the approved bundled terms surface is available.

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
- DONE 2026-07-22 — the desktop dictation-polish command, composer integration, and four post-dictation transforms against the activated resident server.
- DONE 2026-07-22 — fixed system-wide hold-to-talk shortcut foundation with press/release capture, safe busy-state handling, redacted registration failure, and teardown cleanup.
- DONE 2026-07-22 — rapid double activation promotes the composer button or global shortcut to hands-free dictation until Escape or the next activation.
- DONE 2026-07-22 — users can rebind the global voice shortcut with validated modifier-plus-key capture, collision-safe rollback, local persistence, and accessible shortcut exposure.
- DONE 2026-07-22 — accepted ADR 0015 pins the CPU-only Kokoro runtime, immutable model/voice artifacts, deterministic English text-processing and segmentation boundary, packaging contract, cancellation/privacy rules, and target-hardware validation gates.
- DONE 2026-07-22 — the pinned Kokoro model and voice bundle have a bounded resumable acquisition path and conflict-safe publication/current-revision resolution with full re-verification.
- DONE 2026-07-22 — normalized Kokoro source text packs into contiguous, bounded initial synthesis ranges against an injected exact phoneme/token-count boundary, including protected-run and Unicode-safe hard-split handling, followed by ADR 0015's one-pass sub-20-token repair against the same boundary.
- DONE 2026-07-22 — final repaired source ranges carry exact phoneme strings and token IDs through the same injected accent-aware boundary.
- No further ungated Kokoro slice is selected. Native runtime/G2P packaging remains subject to ADR 0015's redistribution approval gate; synthesis, playback, response controls, and physical target-hardware runs remain separately deferred.

## Companion execution surfaces (§13)
- DONE E0/E0.5 — governed protocol, pairing, authorization, idempotency, bounded transport, ADR 0011 repository/lane decision, and ADR 0012 per-user runtime-service direction.
- DONE E1 — Rust CLI lane; Linux discovery/pairing; thread list/open; run start with ordered progress and flow control; typed permission handling; tool details; and receipts.
- DONE E2 — canonical fixtures and drift checks; TypeScript package/CI; cross-platform discovery, framing, pairing, authorization, thread documents, typed run streaming, workspace/current-file context, receipts, and fail-closed permission interaction.
- VSIX sideload testing is sufficient; marketplace publication and official fork-compatibility claims remain owner-gated. The runtime service is the sole runtime/session/journal owner.

## Browser-control runtime capability (§6.8)
- OWNER GO 2026-07-16 — v1 browser-only actuator line is governed runtime capability, not a companion surface.
- DONE E0–E14 — extension-only real-profile architecture through injected memory-only pairing and MV3 authorized relay.
- BLOCKED E15 — desktop-to-extension handoff waits for the owner to upload a real package to the private Chrome Web Store draft and provide its public key. Do not generate a replacement key/ID. Reserved extension ID: `cdedcfbgomnhfpifpgdlpfkkanaofkjd`.
- After the matching key lands, compose and validate E15 as one slice; runtime tools, policy, receipts, and kill switch follow. Store publication/upload are owner-gated; v1 excludes OS, filesystem, and other-app control.

## Onboarding, memory, and the Muniment Home (owner rulings 2026-07-19)
- RATIFIED — visible human-editable Markdown Home under `memory/`, `agents/`, `projects/<name>/`, `sessions/`; files are source of truth, indexes are rebuildable caches, no hidden primary store or Muniment sync, semantic transcript names and visible retention.
- RATIFIED — desktop has an unskippable first-run directory picker (default `Documents/Muniment`); CLI/VS Code use the opened directory by default and lazily create user Home for cross-project need. Honor nearest `AGENTS.md`; instructions and memory remain distinct.
- RATIFIED — required Qwen3.5-4B instruct Q4 GGUF serves onboarding triage and front-door routing, via the verified ADR 0008/0014 path. Import is consent-gated with preview, provenance, verbatim originals, and a user-confirmed report before writes; folder setup fails open and only AI-dependent features fail closed.
- RATIFIED — agent system prompt has five binding rules, hand-written/versioned/eval-gated base v0, and a bounded audited labeled-data tail.
- DONE 2026-07-20 — rulings codified in harness-spec §15/§16; required Qwen descriptor and background acquisition landed.
- DONE 2026-07-21 — first-run Home picker/scaffold; bounded ZIP preview and manifest review; bounded extraction of explicitly selected entries with verbatim text and stable provenance; consent checklist and Tauri extraction bridge retaining approved content only in transient pre-triage state. No Home writes or model calls occur in these slices.
- DONE 2026-07-21 — typed, bounded local-model triage request/report-validation contract over approved extracted entries, plus its typed Tauri invocation against the resident local model.
- DONE 2026-07-21 — structured local-triage report review, approved-source display, redacted retry handling, and explicit in-session confirmation; no Home writes occur in this slice.
- DONE 2026-07-22 — confirmed triage reports, starter-agent proposals, and provenance-bearing verbatim originals compile into a deterministic bounded Home write plan without filesystem access.
- DONE 2026-07-22 — onboarding reports required local-model progress/readiness without blocking Home setup and gates local proposal generation until AI features are available.
- NEXT — persist a confirmed bounded write plan conflict-safely into Home. Desktop/Tauri completion wiring follows as a separate slice. CLI/VS Code defaults wait for workspace memory semantics.
- Open items: headless/server CLI stance owner ruling; final model promotion waits on MUNIQA routing eval; Gemma 4 E2B remains a future owner-gated contingency pending llama.cpp PLE support. MUNICLOUD model artifact proxy/redirect is a ripple.

## Desktop QA automation
- RATIFIED — layered frontend browser coverage plus installed-nightly real-app validation on serialized pve01 desktop-ci VMs; Windows/Linux use WebdriverIO, macOS install/launch smoke plus screendumps/manual owner pass.
- DONE — ADR 0013; canonical runner contract; Linux `.deb` real-sign-in smoke; Windows MSI real-sign-in smoke; macOS install/launch smoke; stable Linux/Windows JUnit artifacts.
- DONE 2026-07-22 — installed Linux `.deb` and Windows MSI each submit one unique prompt through authenticated production chat and verify a non-empty assistant turn plus server receipt route under a bounded deadline; rendered production conversations are excluded from uploaded screenshots.
- DONE 2026-07-22 — a failed installed-nightly platform job opens or updates one SHA-scoped triage issue containing metadata and links to the private redacted diagnostics.
- DONE 2026-07-23 — installed Linux and Windows nightly validation each attach a generated-from-source PNG, submit it through authenticated production chat, and require the model to return the image-only token; fixtures remain outside uploaded diagnostics. Test one pinned finalized installer SHA; no mocked production paths. Signing/notarization and web/API suite remain out of scope.

## Stable release and distribution
- DONE — rolling nightly one pinned SHA across Linux, signed Windows, unsigned macOS; owner-triggered SemVer promotion copies exact green artifacts and labels unsigned macOS.
- DONE groundwork — promotion hashes MSI, generates `Muniment.Muniment` WinGet manifest, and opens a draft fork PR. Fork/token setup, publication, Homebrew, and Apple signing remain owner-gated. No monetization or promotional surface is implied.

## Phase 4+ — Org surface (§9 items 16–19)
- Remote MCP consumption, local stdio allowlist, capability install flow, artifact side panel, and projects with redaction (`output withheld · connection not granted`).

## Standing gates
- Code PRs: structure smoke, frontend/Rust tests, applicable path-scoped checks or desktop builds. Markdown-only PRs gate on structure smoke; pushes to `main` run smoke only.
- Build desktop targets only unless an accepted ADR changes companion checks.
- Distribution accounts, marketplace/store publishing, production launch, and publicity remain owner-gated.
