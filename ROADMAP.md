# muniment-desktop — ROADMAP

Phases mirror harness-spec §9. Muniment-cloud Phase 1 native auth and the cloud prerequisites for desktop chat are live as of 2026-07-11; client work that uses them must exercise the real contracts and must not introduce mocked production paths.

## M0 — Scaffold
- DONE 2026-07-09 — Tauri v2 shell, specifications, design reference, Rust/frontend harnesses, and Linux/Windows/macOS CI gates.

## Phase 2 — Client core (§9 items 8–11)
- DONE — shell/auth/entitlements, production native authorization, signed-in streamed cloud chat and tool flows, local Gemma lifecycle/runtime, and the attachment CAS foundation.
- DONE 2026-07-22 — first-use onboarding reports required local-model acquisition progress and readiness while Home setup remains fail-open and AI proposals fail closed.
- NEXT item 10 — release notice delivery after the approved bundled terms surface is available.
- Item 11 cloud file flow follows items 8d/9.

## Durable local run journal
- DECIDED — ADR 0002 makes a per-run append-only SQLite event journal authoritative; external effects are never silently re-executed and large bodies live in CAS.
- DONE — schema through redacted companion projections. No further journal-maintenance slice is selected.

## Capability vocabulary and provenance
- DONE — user surfaces say “capabilities” and receipts render only server-supplied route/model/cost/time/capability provenance.

## Phase 3 — Routing metadata + voice (§9 items 12, 15)
- Routing metadata carriage/policy integration follows item 9; the local classifier contract is complete.
- Voice direction remains Parakeet capture → Gemma polish → transforms, with Kokoro read-aloud and global hotkeys.
- DONE — Parakeet/Silero acquisition through native dictation, evaluator/endurance coverage, composer controls, hold-to-talk, hands-free double activation, rebindable global voice shortcuts, Gemma polish, and four post-dictation transforms.
- DONE 2026-07-22 — ADR 0014 resident llama runtime implementation and ADR 0015 Kokoro runtime/artifact/text-processing decision.
- DONE 2026-07-22 — verified resumable Kokoro model/voice acquisition and conflict-safe publication/current-revision resolution.
- DONE 2026-07-22 — normalized Kokoro text packs into bounded initial synthesis ranges, followed by ADR 0015’s one-pass sub-20-token repair against the injected exact token-count boundary.
- NEXT — produce final repaired source ranges with exact phoneme strings and token IDs through the same injected accent-aware boundary. Keep this pure core: native runtime/G2P packaging remains subject to ADR 0015’s redistribution approval gate; synthesis, cancellation, playback, response controls, and physical target-hardware runs follow separately.

## Companion execution surfaces (§13)
- DONE E0–E2 — governed protocol, pairing/authorization, Rust CLI, TypeScript extension package, thread/run/permission flows, receipts, fixtures, and drift checks.
- VSIX sideload testing is sufficient; marketplace publication and official fork compatibility remain owner-gated. The runtime service is the sole runtime/session/journal owner.

## Browser-control runtime capability (§6.8)
- OWNER GO 2026-07-16 — v1 browser-only actuator line is governed runtime capability.
- DONE E0–E14.
- BLOCKED E15 — desktop-to-extension handoff waits for the owner’s real private Chrome Web Store package/public key. Reserved extension ID: `cdedcfbgomnhfpifpgdlpfkkanaofkjd`; do not generate a replacement. Store upload/publication are owner-gated and v1 excludes OS/filesystem/other-app control.

## Onboarding, memory, and Muniment Home
- RATIFIED — visible human-editable Markdown Home, unskippable first-run directory picker, nearest-`AGENTS.md` behavior, consent-gated import with preview/provenance/verbatim originals, and required local Qwen triage/routing. Files are source of truth and indexes are rebuildable caches.
- DONE — rulings in harness-spec §15/§16; Home picker/scaffold; bounded ZIP preview/extraction; typed local triage; report review/confirmation; deterministic bounded Home write-plan compilation; model readiness/progress UI.
- NEXT — persist a confirmed bounded write plan conflict-safely into Home. This work is currently in Needs Human and must not be re-filed until resolved. Desktop/Tauri completion follows separately; CLI/VS Code defaults wait for workspace-memory semantics.
- Owner decisions remain open for headless/server CLI stance and final model promotion; Gemma 4 E2B is a future owner-gated contingency.

## Desktop QA automation
- RATIFIED — layered frontend browser coverage plus installed-nightly real-app validation on serialized desktop-ci VMs.
- DONE — ADR 0013, canonical runner, Linux/Windows real-sign-in and unique production-chat smoke with receipt route, macOS install/launch smoke, stable JUnit artifacts, and SHA-scoped failure triage.
- NEXT — attachment coverage after model file delivery exists. Test one pinned finalized installer SHA; no mocked production paths. Signing/notarization and web/API suites remain out of scope.

## Stable release and distribution
- DONE — rolling nightly pinned SHA and owner-triggered SemVer promotion; WinGet draft-manifest groundwork. Fork/token setup, publication, Homebrew, and Apple signing remain owner-gated. No monetization or promotional surface is implied.

## Phase 4+ — Org surface (§9 items 16–19)
- Remote MCP consumption, local stdio allowlist, capability install flow, artifact side panel, and projects with redaction (`output withheld · connection not granted`).

## Standing gates
- Code PRs: structure smoke, frontend/Rust tests, and applicable path-scoped checks or desktop builds. Markdown-only PRs gate on structure smoke; pushes to `main` run smoke only.
- Build desktop targets only unless an accepted ADR changes companion checks.
- Distribution accounts, marketplace/store publishing, production launch, and publicity remain owner-gated.
