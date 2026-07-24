# muniment-desktop — ROADMAP

Phases mirror harness-spec §9. Muniment-cloud Phase 1 native auth and the cloud prerequisites for desktop chat are live as of 2026-07-11; client work that uses them must exercise the real contracts and must not introduce mocked production paths.

## M0 — Scaffold
- DONE 2026-07-09 — Tauri v2 shell, specifications, design references, Rust/frontend test harnesses, and Linux/Windows/macOS CI gates.

## Phase 2 — Client core (§9 items 8–11)

### 8. Shell, auth, and entitlements
- DONE — Svelte/Vite shell; signed-out and signed-in states; installation-bound native authorization; keychain persistence; session, refresh, revocation, entitlement, device, and server-unreachable flows.
- Production auth uses `/v1/auth/native/*`; generic OIDC remains test groundwork only.

### 9. Pi sidecar and cloud chat
- DONE — verified Pi runtime acquisition and supervision; signed-in streaming chat; steering/follow-up; durable tool effects; receipts/provenance; inline tool cards; extension and permission-gate flows; safe interrupted-session resume.

### 10. Local model sidecars
- DONE — supervised Gemma/Qwen and llama runtime acquisition, verification, activation, cancellation, rollback, native adapters, and onboarding readiness/progress surfaces.
- NEXT after its owner gate — release-notice delivery after the approved bundled terms surface exists.

### 11. Attachments and local CAS
- DONE groundwork — pure-Rust content-addressed storage, streaming I/O, atomic deduplication, verification, cleanup, reference accounting, retention, export, compaction, and CAS-backed image delivery to production chat.

### Durable local run journal
- DONE — ADR 0002 append-only SQLite journal, reducer/replay, Pi translation, retention/deletion, export, compaction, summaries, and authorized redacted companion projections.
- No journal-maintenance slice is selected.

### Capability vocabulary and provenance
- DONE — user surfaces say “capabilities”; receipts render only server-supplied route/model/cost/time/capability provenance.

## Phase 3 — Routing metadata + voice (§9 items 12, 15)
- Routing metadata policy integration follows item 9; the local classifier contract is complete.
- DONE — Parakeet/Silero acquisition and packaging; bounded capture/VAD/recognition; desktop events and composer dictation; evaluation/endurance tooling; hold-to-talk, hands-free activation, rebinding, cancellation, polish, and transforms.
- DONE — ADR 0014 pinned llama.cpp/Qwen acquisition and supervised activation across four native targets.
- DONE groundwork — ADR 0015 Kokoro acquisition plus deterministic text segmentation, phoneme, and token boundaries.
- Native Kokoro runtime/G2P packaging remains redistribution-gated; synthesis, playback, response controls, and physical target-hardware validation remain deferred.

## Companion execution surfaces (§13)
- DONE E0–E2 — governed protocol, pairing/authorization/idempotency/transport, Rust CLI, TypeScript package, cross-platform discovery, thread/run/permission/tool/receipt flows, canonical fixtures, and drift checks.
- VSIX sideload testing is sufficient. Marketplace publication and official fork-compatibility claims remain owner-gated. The runtime service is the sole runtime/session/journal owner.

## Browser-control runtime capability (§6.8)
- OWNER GO 2026-07-16 — browser-only actuator line is a governed runtime capability.
- DONE E0–E14 — extension-only real-profile architecture through injected memory-only pairing and authorized MV3 relay.
- BLOCKED E15 — desktop-to-extension handoff waits for the owner-supplied private Chrome Web Store package and public key. Do not replace the reserved ID `cdedcfbgomnhfpifpgdlpfkkanaofkjd`.
- After the matching key lands, compose and validate E15 as one slice. Store upload/publication remain owner-gated; v1 excludes OS, filesystem, and other-app control.

## Onboarding, memory, and Muniment Home
- RATIFIED — visible human-editable Markdown Home; files are authoritative and indexes rebuildable; no hidden primary store or Muniment sync.
- RATIFIED — unskippable desktop directory picker; consent-gated bounded import with preview, provenance, verbatim originals, and confirmation before writes; folder setup fails open while AI proposals fail closed.
- DONE — Home picker/scaffold; ZIP preview and bounded selected extraction; local-model triage contract and UI; proposal/write-plan compilation; model readiness gating; conflict-safe persistence; typed Tauri completion; frontend completion and conflict recovery.
- DONE 2026-07-24 — the artifact-rail control is a real signed-in frontend shell with button/platform shortcut toggle, Escape close, lifecycle reset, and an honest empty state.
- NEXT — make the open artifact rail adjustable from 380–560px with an accessible pointer/keyboard splitter. Keep this frontend-only: artifact data, rendering, persistence, sharing, and cloud contracts remain deferred to Phase 4 item 19.
- Open owner gates: headless/server CLI stance; final model promotion pending MUNIQA routing evaluation; Gemma 4 E2B contingency pending llama.cpp PLE support. CLI/VS Code defaults wait for workspace-memory semantics.

## Desktop QA automation
- RATIFIED — layered frontend coverage plus installed-nightly real-app validation on serialized pve01 desktop-ci VMs.
- DONE — ADR 0013; runner contract; Linux/Windows real sign-in and production chat; macOS install/launch smoke; stable JUnit artifacts; SHA-scoped failure triage; installed image-attachment production validation.
- Test one pinned finalized installer SHA with no mocked production paths. Signing/notarization and web/API suites remain out of scope.

## Stable release and distribution
- DONE — rolling nightly for one pinned SHA and owner-triggered SemVer promotion of exact green artifacts; unsigned macOS is labeled.
- DONE groundwork — MSI hashing and WinGet manifest/draft-fork-PR generation.
- Fork/token setup, publication, Homebrew, Apple signing, release accounts, launch, and publicity remain owner-gated.

## Phase 4+ — Org surface (§9 items 16–19)
- Remote MCP consumption, local stdio allowlisting, capability install flow, artifact library/side-panel rendering, and projects with honest redaction remain future slices.
- Item 19 shell work may proceed in small frontend-only slices; artifact data contracts still depend on the item-5 cloud surface and item-11 storage contract.

## Standing gates
- Code PRs: structure smoke, frontend/Rust tests, and applicable path-scoped checks or desktop builds. Markdown-only PRs gate on structure smoke; pushes to `main` run smoke only.
- Build desktop targets only unless an accepted ADR changes companion checks.
- Database migrations/schema restructuring, monetization, distribution accounts, marketplace/store publishing, production launch, and publicity remain owner-gated.
