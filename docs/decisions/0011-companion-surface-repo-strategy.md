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

- **E1 CLI — a Rust workspace binary in this repository.** It links a new
  protocol-only `muniment-attach` crate extracted from `muniment-core`, not
  `muniment-core` or the runtime.
- **E2 editor extension — a TypeScript package lane in this repository.** It is
  a thin attach client that consumes the versioned protocol fixtures
  `muniment-attach` emits and never re-derives run state.

The dominant risk for both surfaces is cross-repo contract drift against a wire
contract that is canonical here; co-location makes each contract change and its
consumer atomic in one reviewed PR. Neither surface pays the mobile repo's full
native-app build cost. Both do implement native local transport (Unix-domain
sockets on Linux/macOS and named pipes on Windows), so their focused transport
merge gates use the existing serialized three-platform `desktop-ci` farm. They
never invoke desktop bundle steps.

This decision is **proposed** until explicit owner ratification. It is the sole
prerequisite before any E1 or E2 scaffolding ticket.

### Per-surface comparison

Each surface is compared for in-repo versus a separate repository against
core/protocol reuse, toolchain ownership, cross-repo contract drift, review
ownership, and release cadence.

**E1 CLI**

| Dimension | In this repo (recommended) | Separate `muniment-cli` repo (rejected) |
|---|---|---|
| Core/protocol reuse | Links the protocol-only `muniment-attach` workspace crate directly; the ADR 0009 envelope/codec/negotiation/cursor/idempotency types are a source dependency, not a copy. | Must vendor or publish that crate; the CLI otherwise cannot share canonical types without a release/versioning pipeline. |
| Toolchain ownership | Reuses the existing Rust toolchain and `Cargo.lock`; no new language or build system. | Owns a duplicate Rust toolchain, lockfile, and pinning discipline for the same stack. |
| Cross-repo contract drift | None: one PR changes the protocol and the CLI together and both are gated by the same contract-fixture tests. | High: the CLI can lag the desktop's protocol; drift is caught only at runtime by `protocol_incompatible`, not at PR time. |
| Review ownership | Single owner sees protocol, desktop, and CLI changes together. | Split review; a protocol change needs coordinated PRs across two repos. |
| Release cadence | CLI packaging rides the stable desktop channel (it requires the installed desktop app anyway per §13). | Independent cadence buys nothing — v1 CLI is inert without a matching desktop, so decoupled releases only add drift surface. |

**E2 editor extension**

| Dimension | In this repo (recommended) | Separate `muniment-editor` repo (rejected) |
|---|---|---|
| Core/protocol reuse | Consumes the versioned golden fixtures `muniment-attach` emits (ADR 0009 cross-platform byte goldens) from the same tree; the wire contract is a single source of truth. | Must sync fixtures across repos on every protocol change, reintroducing the exact drift ADR 0007 flagged for the M1 mobile reducer. |
| Toolchain ownership | Adds a second npm package alongside the existing Svelte frontend; the repo already runs Node/npm, so the marginal toolchain cost is small and isolated to one directory. | A cleaner language boundary, but the repo still runs npm today, so the separation saves little while costing a fixture-sync pipeline. |
| Cross-repo contract drift | None: a protocol change and its TypeScript consumer land atomically. | High and silent until a sideloaded build misparses a frame. |
| Review ownership | One owner sees protocol, redaction rules, and extension together. | Split review of tightly coupled redaction/projection semantics. |
| Release cadence | Marketplace publish is a separate, owner-gated action regardless of repo; in-repo does not force it onto the desktop cadence. | Independent cadence is available but not needed, since publishing is already a decoupled owner-gated act (below). |

A VS Code extension is the one surface where a separate repo is genuinely
arguable — it shares no compiled artifact with the desktop and its language
differs. It is still rejected because its tightest coupling is to the
attach-protocol wire contract and redaction rules that are canonical here;
co-location makes that contract atomic. Its package is portable, but its native
transport is not; CI below limits its build-farm cost to focused tests.

## Layout and dependency boundary

**CLI.** `src-tauri/Cargo.toml` is the workspace root and remains the desktop
package manifest. Its `[workspace]` has exactly `members = [".", "core",
"attach", "cli"]` and `resolver = "2"`. The new manifests are
`src-tauri/attach/Cargo.toml` (`muniment-attach`, library) and
`src-tauri/cli/Cargo.toml` (`muniment-cli`, binary). All members share the root
`src-tauri/Cargo.lock` and target directory.

The ADR 0009 envelope, bounded frame codec, version negotiation, authorization
state machine, cursor/idempotency, redaction-projection types, and fixture
exporter move from `muniment-core` into `muniment-attach`. `muniment-core`
depends on `muniment-attach = { path = "../attach" }`; the CLI depends on
`muniment-attach = { path = "../attach", default-features = false, features =
["client"] }` and must not depend on `muniment-core`. The `client` feature adds
only client transport adapters. `muniment-attach` has no runtime feature and
may not depend on `rusqlite`, `sherpa-onnx`, sidecar supervision, OIDC/key
storage, Tauri, or the desktop crate. A CI dependency-tree gate (`cargo tree -p
muniment-cli`) rejects those crate/package names. This one-way graph prevents
the CLI from compiling or becoming a second runtime.

**Editor extension.** A self-contained TypeScript package at top-level
`editor-extension/`, separate from the Svelte frontend `src/`, with its own
`package.json`, `tsconfig`, lockfile, and `vsce` packaging config. It is a
**thin attach client**: it renders redacted projections streamed over ADR 0009
and answers permission gates in-editor; it never opens the journal/CAS, never
holds keys, and — critically — never re-derives run state from raw events, so
it introduces no second reducer. It consumes canonical, checked-in UTF-8 JSON
fixtures at `protocol-fixtures/muniment.attach/1/*.json`. Each protocol major
has its own directory; filenames identify the envelope/operation case, and JSON
uses canonical key order plus a trailing newline. Rust owns serialization in
`muniment-attach`. From `src-tauri/`, `cargo run -p muniment-attach --bin
export-attach-fixtures -- ../protocol-fixtures` regenerates them; appending
`--check` exits nonzero for a missing, extra, or byte-stale fixture.

Extension tests read `../protocol-fixtures/muniment.attach/1/` directly; they do
not copy or vendor a snapshot. They pin `muniment.attach/1` and decode every
fixture, plus rejection and unknown-optional-field cases. The extension fails
closed on `protocol_incompatible` and never imports Rust runtime code. The
checked-in serialized bytes, not duplicate TypeScript types, are the language
boundary.

## CI lanes

Both companions get **independent, path-scoped PR checks**. CLI paths are
`src-tauri/cli/**`; extension paths are `editor-extension/**`. Changes under
`src-tauri/attach/**`, `protocol-fixtures/**`, `src-tauri/Cargo.toml`, or
`src-tauri/Cargo.lock` are shared-contract changes and select **both** companion
lanes (and the desktop lane where its graph is affected), so a shared change
cannot skip either consumer's required check.

- **CLI lane.** Portable protocol and client logic — including the ADR 0009
  contract-fixture suite and fixture exporter `--check` — run as `cargo
  fmt`/`clippy`/`test` for `muniment-cli` and `muniment-attach` on the shared runner,
  exactly as `muniment-core` runs today (no GUI stack needed). The CLI's local
  transport is OS-specific (Unix-domain socket on macOS/Linux, named pipe on
  Windows, per ADR 0009), so focused Linux, macOS, and Windows jobs on the
  existing serialized `desktop-ci` VM seam run `cargo test -p muniment-cli -p
  muniment-attach`, including real local connect and peer-rejection tests. These
  jobs consume that farm but do **not** compile or bundle the desktop.
- **Extension lane.** `npm ci`, typecheck, lint, unit and fixture-contract
  tests, canonical-fixture decoding tests, and `vsce package` on one plain Node
  runner across the targeted Node LTS. Packaging is single-OS because VSIX
  output is portable. Transport is not: focused Linux, macOS, and Windows
  `desktop-ci` jobs compile the extension and run integration tests against
  each platform's Unix-socket/named-pipe adapter, including connection and
  rejection behavior. Marketplace validation does not replace this merge gate.

The required checks are named `attach-fixtures-current` and
`extension-contract`. The former runs the Rust contract tests followed by the
exporter `--check`; a Rust protocol type or serializer change without regenerated
checked-in bytes fails it. The latter runs the TypeScript decoder against every
checked-in fixture; regenerated bytes without compatible TypeScript decoding
fail it. Shared path selection requires both checks, closing both stale-fixture
directions.

**Effect on the serialized desktop CI matrix.** Today the smoke job classifies
PRs only as `docs_only` versus everything-else, and everything-else enqueues the
three sequential Linux/Windows/macOS `desktop-compile` and `desktop-build` jobs
behind the one-VM-at-a-time driver. Adding companion code in-repo would, without
change, make every companion-only PR pay that serialized desktop cost. E1/E2
scaffolding must therefore **extend the existing change classifier** (the
`docs_only` seam) into a path partition so that: companion-only PRs run only
their own lane and never enqueue the desktop matrix; desktop-only PRs do not run
the companion lanes; and shared changes to the protocol partition in
`muniment-attach` or canonical fixtures run both, since they can break either
consumer. Companion-only PRs skip desktop compile/bundle jobs, but their three
focused transport jobs still consume the serialized farm. Desktop-only PRs keep
the current desktop gate; shared-core changes select the lanes whose dependency
graphs they affect.

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
work avoids desktop bundle jobs; native transport tests still serialize on that
farm. Unlike mobile (ADR 0007),
separate repositories are rejected because the surfaces' tightest coupling is to
a wire contract that is canonical here.

This decision is **proposed** and takes effect only on explicit owner
ratification. Until then no scaffold begins. Once ratified, this ADR (§13's
**E0.5**) is the **sole prerequisite** before the E1 and E2 scaffolding tickets,
which then execute in this repository:

- introduce the Cargo workspace and the `muniment-cli` member;
- extract the decided `muniment-attach` crate and make the CLI link only its
  `client` feature;
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
