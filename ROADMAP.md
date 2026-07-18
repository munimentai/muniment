# muniment-desktop — ROADMAP

Phases mirror harness-spec §9. Muniment-cloud Phase 1 native auth and the cloud prerequisites for desktop chat are live; real contracts, not mocked production paths, govern client work.

## M0 — Scaffold
DONE — Tauri v2 shell, specs, design references, Rust/frontend harnesses, and CI gates.

## Phase 2 — Client core (§9 items 8–11)
DONE — shell/auth/entitlements and cloud chat. Local Gemma runtime and local CAS groundwork are complete. NEXT for Gemma is first-use install UI after approved bundled terms; attachments follow the established cloud flow.

## Durable local run journal
DONE — ADR 0002 journal, replay/reducer, retention/export/compaction, summary listing, and authorized redacted thread reads. The journal remains authoritative and large bodies live in CAS.

## Phase 3 — Routing metadata + voice
DONE groundwork — classifier contract, model/runtime acquisition, native capture, VAD/segmentation, composition, recognition, and desktop commands/events. NEXT — dictation UI and target-hardware validation as independent slices.

## Companion execution surfaces (§13)
DONE E0/E0.5 — protocol, authorization, flow-control primitives, Linux transport, and repository/lane decisions. ADR 0012 selects one per-user runtime owner; extraction remains a separate build line.

DONE E1 read path and write-path groundwork — CLI discovery/pairing, thread list/open, authorized run start, shared coordinator, receipt-only command, bounded journal catch-up, server cursor acknowledgements, and protocol-client subscription plus consumed-window acknowledgement/resume.

NEXT E1 — establish bounded, loss-tolerant journal commit hints as the first live-tail slice. Subsequent independent slices wire server live delivery, protocol-client live consumption, terminal rendering, and permission-answer parity. E2 may proceed concurrently in its TypeScript lane; publication remains owner-gated. macOS and Windows attach adapters remain future slices.

## Browser-control runtime capability (§6.8)
DONE E1–E8 — relay core, pairing, Linux identity/owner authorization, listener, WebSocket upgrade, and token consumption. Later slices add other OS adapters, MV3 lifecycle, runtime tools, policy/entitlement, receipts, and kill switch. Store publication remains owner-gated.

## Stable release and distribution
DONE — rolling nightly, strict-SemVer stable promotion, Windows signing, and WinGet manifest/PR groundwork. External accounts, review, publication, Apple signing, and publicity remain owner-gated.

## Phase 4+ — Org surface
Remote MCP consumption, local stdio allowlist, capability install flow, artifact rail, and projects with the redaction rule (`output withheld · connection not granted`).

## Standing gates
Code PRs run structure smoke and relevant frontend/Rust tests before platform builds. Markdown-only PRs use smoke alone. Build only accepted repository lanes. Distribution, marketplace publication, production launch, publicity, and monetization remain owner-gated.
