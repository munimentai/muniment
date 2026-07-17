# 0011 — Repository and CI lanes for the CLI and editor extension

- Status: proposed — awaiting owner ratification
- Date: 2026-07-16
- Context: harness-spec §13 E0.5; ROADMAP standing gates; ADR 0009

## Context

Harness-spec §13 adds the **muniment CLI** (E1) and the **muniment editor
extension** (E2) as companion work surfaces over the SAME local runtime the
desktop app owns. Accepted [ADR 0009](0009-companion-attach-protocol.md) makes
both governed clients of the desktop-owned runtime: they attach over a
versioned local IPC contract, never spawn a second runtime, never hold provider
keys, never open the journal/CAS, and receive only mediated, redacted
projections. §13 phases E0 (attach protocol) as DONE and names E1 and E2 as the
next surfaces, with marketplace publishing owner-gated.

Before either scaffold begins, §13's E0.5 slice must decide **where the E1 and
E2 code lives and how each is built, tested, and released**. This repository is
a Svelte/Tauri workspace: the GUI-bearing `muniment-desktop` crate in
`src-tauri/` links a pure Rust `src-tauri/core` (`muniment-core`) crate that is
kept free of Tauri on purpose so it compiles and tests on the shared CI runner.
The attach-protocol envelope/codec/negotiation/authorization/cursor/idempotency
types from ADR 0009 already live in `muniment-core`. The code PR gate in
`.github/workflows/ci.yml` is a smoke job (structure check plus frontend and
`muniment-core` unit tests) followed by native Linux, Windows, and macOS
desktop compile preflights and then three sequential platform builds, all
serialized by the one-VM-at-a-time `desktop-ci` driver. Markdown-only PRs run
structure smoke alone.

This ADR decides the E0.5 question. Following [ADR 0007](0007-mobile-companion-repo-strategy.md),
it evaluates rather than assumes: unlike mobile — which shares almost nothing
with the core and pays a heavy native iOS/Android build cost — both companion
surfaces here couple most tightly to the attach-protocol wire contract whose
canonical types and golden fixtures already live in this repository. This ADR
creates no crate, package, workspace change, CI change, repository, publisher
account, or release.

## Decision

**Both surfaces live in this repository, each with its own independent,
path-scoped PR lane.**

- **E1 CLI — a Rust workspace binary in this repository.** It links only the
  pure-core attach-protocol partition of `muniment-core`, not the runtime.
- **E2 editor extension — a TypeScript package lane in this repository.** It is
  a thin attach client that consumes the versioned protocol fixtures
  `muniment-core` emits and never re-derives run state.

The dominant risk for both surfaces is cross-repo contract drift against a wire
contract that is canonical here; co-location makes each contract change and its
consumer atomic in one reviewed PR. Neither surface pays the mobile repo's
motivating cost: the CLI compiles on the shared runner exactly as `muniment-core`
does today, and a VS Code extension is host-OS-independent, so neither needs the
serialized three-platform `desktop-ci` farm for its merge gate.

This decision is **proposed** until explicit owner ratification. It is the sole
prerequisite before any E1 or E2 scaffolding ticket.

### Per-surface comparison

Each surface is compared for in-repo versus a separate repository against
core/protocol reuse, toolchain ownership, cross-repo contract drift, review
ownership, and release cadence.

**E1 CLI**

| Dimension | In this repo (recommended) | Separate `muniment-cli` repo (rejected) |
|---|---|---|
| Core/protocol reuse | Links `muniment-core`'s protocol partition directly; the ADR 0009 envelope/codec/negotiation/cursor/idempotency types are a source dependency, not a copy. | Must vendor or re-publish the protocol crate; the CLI cannot reach `muniment-core` without a release/versioning pipeline that does not yet exist. |
| Toolchain ownership | Reuses the existing Rust toolchain and `Cargo.lock`; no new language or build system. | Owns a duplicate Rust toolchain, lockfile, and pinning discipline for the same stack. |
| Cross-repo contract drift | None: one PR changes the protocol and the CLI together and both are gated by the same contract-fixture tests. | High: the CLI can lag the desktop's protocol; drift is caught only at runtime by `protocol_incompatible`, not at PR time. |
| Review ownership | Single owner sees protocol, desktop, and CLI changes together. | Split review; a protocol change needs coordinated PRs across two repos. |
| Release cadence | CLI packaging rides the stable desktop channel (it requires the installed desktop app anyway per §13). | Independent cadence buys nothing — v1 CLI is inert without a matching desktop, so decoupled releases only add drift surface. |

**E2 editor extension**

| Dimension | In this repo (recommended) | Separate `muniment-editor` repo (rejected) |
|---|---|---|
| Core/protocol reuse | Consumes the versioned golden fixtures `muniment-core` emits (ADR 0009 cross-platform byte goldens) from the same tree; the wire contract is a single source of truth. | Must sync fixtures across repos on every protocol change, reintroducing the exact drift ADR 0007 flagged for the M1 mobile reducer. |
| Toolchain ownership | Adds a second npm package alongside the existing Svelte frontend; the repo already runs Node/npm, so the marginal toolchain cost is small and isolated to one directory. | A cleaner language boundary, but the repo still runs npm today, so the separation saves little while costing a fixture-sync pipeline. |
| Cross-repo contract drift | None: a protocol change and its TypeScript consumer land atomically. | High and silent until a sideloaded build misparses a frame. |
| Review ownership | One owner sees protocol, redaction rules, and extension together. | Split review of tightly coupled redaction/projection semantics. |
| Release cadence | Marketplace publish is a separate, owner-gated action regardless of repo; in-repo does not force it onto the desktop cadence. | Independent cadence is available but not needed, since publishing is already a decoupled owner-gated act (below). |

A VS Code extension is the one surface where a separate repo is genuinely
arguable — it shares no compiled artifact with the desktop and its language
differs. It is still rejected because its tightest coupling is to the
attach-protocol wire contract and redaction rules that are canonical here;
co-location makes that contract atomic, and its OS-independence means it never
burdens the desktop build farm (CI below).

## Layout and dependency boundary

**CLI.** Introduce a Cargo workspace so `muniment-core`, the desktop crate, and
a new `muniment-cli` binary crate share one lockfile and target directory (see
Cargo workspaces reference in Sources). Concretely, `src-tauri/Cargo.toml`
becomes a `[workspace]` with members `.` (the desktop GUI crate), `core`, and a
new `src-tauri/cli/` (`muniment-cli`); the exact workspace rooting is finalized
at scaffold time provided the desktop crate stays isolable behind `-p`/
`--manifest-path` so it never enters the shared-runner lanes. The desktop
crate's GUI dependency surface is unchanged.

The CLI **may** link `muniment-core`, but **only its attach-protocol
partition** — the ADR 0009 envelope, bounded frame codec, version negotiation,
authorization state machine, cursor/idempotency, and redaction-projection
types. It must **not** compile the runtime: no `rusqlite` journal/CAS writer, no
Pi sidecar spawn/supervision, no `sherpa-onnx` models, and no native OIDC/key
flow. Because those runtime dependencies are currently non-optional in
`muniment-core`, E1 scaffolding must partition them behind a Cargo feature
(default-off for the CLI) or extract a `muniment-attach` sub-crate, so the CLI's
dependency graph excludes the runtime and it keeps compiling on the shared
runner without the GUI/build VMs. This partition is the mechanism that keeps the
CLI a client and not a second runtime, per ADR 0009.

**Editor extension.** A self-contained TypeScript package at top-level
`editor-extension/`, separate from the Svelte frontend `src/`, with its own
`package.json`, `tsconfig`, lockfile, and `vsce` packaging config. It is a
**thin attach client**: it renders redacted projections streamed over ADR 0009
and answers permission gates in-editor; it never opens the journal/CAS, never
holds keys, and — critically — never re-derives run state from raw events, so
it introduces no second reducer. It consumes the attach protocol as **versioned
golden fixtures** exported from `muniment-core`'s contract-test corpus: the
extension pins a protocol major (`muniment.attach/1`), validates its wire types
against those fixtures in its own tests, ignores unknown optional fields/events,
and fails closed on `protocol_incompatible`. The fixtures are the boundary that
lets the extension track the contract without embedding the runtime.

## CI lanes

Both companions get **independent, path-scoped PR checks** that run only when
their paths change.

- **CLI lane.** Portable protocol and client logic — including the ADR 0009
  contract-fixture suite — run as `cargo fmt`/`clippy`/`test` for the
  `muniment-cli` and protocol-partition crates on the shared self-hosted runner,
  exactly as `muniment-core` runs today (no GUI stack needed). The CLI's local
  transport is OS-specific (Unix-domain socket on macOS/Linux, named pipe on
  Windows, per ADR 0009), so its per-OS transport slices get compile/test
  coverage through the existing `desktop-ci` VM seam using the same
  `cargo check`/`cargo test` invocation the desktop preflight uses, added as
  CLI-scoped jobs that do **not** run the desktop bundle steps.
- **Extension lane.** `npm ci`, typecheck, lint, unit and fixture-contract
  tests, and `vsce package` on a plain Node runner across the Node LTS the
  extension targets. A VS Code extension runs on the editor's Node host and is
  host-OS-independent, so **no native per-OS matrix is required** for the merge
  gate; multi-OS host validation is the marketplace's concern at publish time,
  which is owner-gated below.

**Effect on the serialized desktop CI matrix.** Today the smoke job classifies
PRs only as `docs_only` versus everything-else, and everything-else enqueues the
three sequential Linux/Windows/macOS `desktop-compile` and `desktop-build` jobs
behind the one-VM-at-a-time driver. Adding companion code in-repo would, without
change, make every companion-only PR pay that serialized desktop cost. E1/E2
scaffolding must therefore **extend the existing change classifier** (the
`docs_only` seam) into a path partition so that: companion-only PRs run only
their own lane and never enqueue the desktop matrix; desktop-only PRs do not run
the companion lanes; and shared changes to the protocol partition in
`muniment-core` run both, since they can break either consumer. This keeps the
scarce, serialized `desktop-ci` VMs uncontended by companion work while
preserving the current desktop gate for desktop and shared-core changes.

## Distribution boundaries

- **CLI packaging follows the stable desktop channel later.** v1 CLI requires
  the installed, signed-in desktop app (§13), so its binary rides the same
  stable promotion path the desktop uses (see [ADR 0008](0008-pi-runtime-distribution.md)
  and the stable-channel promotion, MUNIDESK-240; brew/winget once signed). No
  CLI artifact is built or published by this ADR.
- **The extension is testable by VSIX sideload before any marketplace step.**
  Pre-marketplace testing uses local `.vsix` install (see VS Code
  install-from-VSIX in Sources); this needs no publisher account and is not an
  owner-gated act.
- **Marketplace/store publication and account creation remain owner-gated.**
  Publishing to the VS Code Marketplace or Open VSX, creating the publisher
  account, and any store submission are public acts, owner-only like all
  launch/publicity in §13 and ADR 0007. This ADR creates no account,
  credential, package, or release.

## Consequences and ratification

The recommendation gives both companion surfaces atomic contract evolution
against the canonical attach protocol, independent path-scoped PR lanes, and a
dependency boundary that keeps each a client rather than a second runtime. It
accepts a second toolchain package (the TypeScript extension) and a Cargo
workspace restructure inside this repository, and it requires the CI change
classifier to grow from a docs/non-docs split into a path partition so companion
work does not serialize behind the desktop build farm. Unlike mobile (ADR 0007),
separate repositories are rejected because the surfaces' tightest coupling is to
a wire contract that is canonical here, and neither surface needs the
three-platform desktop farm its merge gate would otherwise inherit.

This decision is **proposed** and takes effect only on explicit owner
ratification. Until then no scaffold begins. Once ratified, this ADR (§13's
**E0.5**) is the **sole prerequisite** before the E1 and E2 scaffolding tickets,
which then execute in this repository:

- introduce the Cargo workspace and the `muniment-cli` member;
- partition `muniment-core`'s runtime dependencies behind a default-off feature
  (or extract a `muniment-attach` crate) so the CLI links protocol-only;
- add the `editor-extension/` TypeScript package and its fixture-pinned client;
- export the versioned attach-protocol golden fixtures for the extension to
  consume;
- extend the CI change classifier into a path partition and add the CLI and
  extension lanes without perturbing the desktop matrix;
- wire CLI packaging into the stable desktop channel when it ships;
- (owner-only, later) acquire the marketplace publisher account and gate any
  publication.

## Sources

- [Harness specification §13](../spec/harness-spec.md#13-companion-work-surfaces-cli-and-editor-extension-owner-decision-2026-07-12)
- [ADR 0009 — governed local attach protocol](0009-companion-attach-protocol.md)
- [ADR 0007 — mobile companion repository strategy](0007-mobile-companion-repo-strategy.md)
- [ADR 0008 — Pi runtime distribution](0008-pi-runtime-distribution.md)
- [ROADMAP standing gates](../../ROADMAP.md)
- [Cargo workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html)
- [VS Code — install from a VSIX](https://code.visualstudio.com/docs/configure/extensions/extension-marketplace#_install-from-a-vsix)
