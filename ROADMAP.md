# muniment-desktop — ROADMAP

Phases mirror harness-spec §9. Muniment-cloud Phase 1 native auth and the cloud prerequisites for desktop chat are live as of 2026-07-11; client work that uses them must exercise the real contracts and must not introduce mocked production paths.

## M0 — Scaffold
- DONE 2026-07-09 — Tauri v2 shell, specs, design reference, Rust/frontend harnesses, and Linux/Windows/macOS CI gates.

## Phase 2 — Client core (§9 items 8–11)
- DONE — shell/auth/entitlements; verified Pi runtime and cloud chat; local model lifecycle and native adapters; local CAS and durable append-only run journal; capability vocabulary and server-supplied provenance.
- NEXT for item 10 — explicit first-use install UI and release-notice delivery remain separate slices after the approved bundled terms surface is available.

## Phase 3 — Routing metadata + voice (§9 items 12, 15)
- DONE — routing-classifier contract; Parakeet/Silero acquisition, capture, VAD, recognition, endurance evaluator, composer dictation, and resident-model dictation polish.
- DONE 2026-07-20 — ADR 0014 implementation through verified llama-server/Qwen3.5-4B acquisition and activation.
- Transforms and global OS hotkeys follow separately. Physical target-hardware runs still require the approved corpus and representative machines.

## Companion execution surfaces (§13)
- DONE E0–E2 — governed attach protocol, pairing/authorization, Rust CLI, TypeScript/VS Code surface, thread/run streaming, permissions, tool details, and receipts.
- Marketplace publication and official fork-compatibility claims remain owner-gated; the runtime service remains sole runtime/session/journal owner.

## Browser-control runtime capability (§6.8)
- DONE E0–E14 — extension-only real-profile architecture through injected memory-only pairing and MV3 authorized relay.
- BLOCKED E15 — desktop-to-extension handoff waits for the owner-provided private Chrome Web Store package/public key for reserved ID `cdedcfbgomnhfpifpgdlpfkkanaofkjd`. Do not generate a replacement. Store publication/upload remain owner-gated.

## Onboarding, memory, and Muniment Home
- RATIFIED — visible human-editable Markdown Home under `memory/`, `agents/`, `projects/<name>/`, and `sessions/`; files are source of truth and indexes are rebuildable caches. Desktop first run uses an unskippable picker; CLI/VS Code use the opened directory by default. Honor nearest `AGENTS.md`; instructions and memory remain distinct.
- RATIFIED — required Qwen3.5-4B Instruct Q4 GGUF performs onboarding triage and front-door routing through the verified acquisition path. Import is consent-gated with preview, provenance, verbatim originals, and a user-confirmed report before writes; folder setup fails open and AI-dependent features fail closed.
- DONE 2026-07-21 — Home picker/scaffold; bounded ZIP preview and manifest review; bounded extraction of explicitly selected entries; consent checklist and Tauri extraction bridge retaining approved content only in transient pre-triage state. No Home writes or model calls occur in these slices.
- NEXT — define the typed, bounded local-model triage request/report-validation contract over approved extracted entries. Tauri invocation, user-confirmed report UI, and import/scaffolding writes follow as separate slices. Required-download visibility/status UI remains. CLI/VS Code defaults wait for workspace-memory semantics.
- OPEN — headless/server CLI stance owner ruling; final model promotion waits on MUNIQA routing eval; Gemma 4 E2B remains owner-gated pending llama.cpp PLE support; MUNICLOUD model artifact proxy/redirect is a ripple.

## Desktop QA automation
- DONE — ADR 0013, canonical runner, Linux/Windows real-sign-in smoke, macOS install/launch smoke, and stable Linux/Windows JUnit artifacts.
- NEXT — bounded Linux installed-nightly real chat round-trip remains selected; Windows parity, attachments, and failure-to-ticket reporting follow separately. One pinned finalized installer SHA, no mocked production paths.

## Stable release and distribution
- DONE — rolling nightly pinned SHA, owner-triggered SemVer promotion of exact green artifacts, MSI hashing, WinGet manifest generation, and draft fork PR groundwork. Fork/token setup, publication, Homebrew, and Apple signing remain owner-gated.

## Phase 4+ — Org surface (§9 items 16–19)
- Remote MCP consumption, local stdio allowlist, capability install flow, artifact side panel, and projects with redaction.

## Standing gates
- Code PRs: structure smoke, frontend/Rust tests, applicable path-scoped checks or desktop builds. Markdown-only PRs gate on structure smoke; pushes to `main` run smoke only.
- Build desktop targets only unless an accepted ADR changes companion checks.
- Distribution accounts, marketplace/store publishing, production launch, publicity, monetization, and promotional surfaces remain owner-gated.
