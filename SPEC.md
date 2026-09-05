# muniment-desktop — SPEC

The muniment desktop client: Tauri v2 shell + Pi sidecar + on-device voice.
The client supports cloud-backed use and local mode. Closed source.

**The canonical spec is vendored, verbatim, in [docs/spec/](docs/spec/):**

- [harness-spec.md](docs/spec/harness-spec.md) §6 (desktop client — THE spec
  for this repo), §5 (routing: the cloud classifies at ingress),
  §15.3 (the bundled on-device router classifier), §2 (architecture), and §9
  (build order this repo follows).
- [02-desktop-app.md](docs/spec/02-desktop-app.md) +
  [design-spec.md](docs/spec/design-spec.md) §2 (layout, thread grammar,
  composer, voice states, projects) and §1 (brand foundation, color law,
  tokens, the milled ring §1.8).
- [03-mobile-app.md](docs/spec/03-mobile-app.md) — design ground truth for the
  phased mobile companion app (harness-spec §12); where mobile is engineered
  (this Tauri workspace vs a separate repo) is the §12.5 repo-strategy ADR,
  owner-gated.

**Design ground truth = the owner-built mockups in
[docs/mockups/desktop/](docs/mockups/desktop/)** (adhere closely). Ring
reference implementation: [docs/design-reference/ring/](docs/design-reference/ring/).

## Operating Constraints (LAW — reviewer enforces on every PR)

1. **Local mode exists.** Per the owner ruling on 2026-09-04, local mode may
   run Pi, use Pi's credential store, and read or write the local run journal
   without a cloud session. Sign-in gates cloud features only. Cloud outages
   still show the full-surface notice for cloud-backed use (design-spec §2.6).
2. **Cloud credentials stay scoped.** Cloud-backed use passes short-lived
   session tokens and the user's LiteLLM virtual endpoint. Local mode passes no
   cloud virtual key and lets Pi read provider credentials from its own store.
   Entitlement snapshots are display hints. The server enforces cloud use.
3. **Color law (design-spec §1.2):** color means computation. `--signal`
   only on: mark thinking state, running-tool pulse, streaming underline +
   caret, provenance route segment, voice polish flash, workflow-run
   indicators. Ink-on-paper everything else. The §1.6 anti-patterns are
   hard PR fails (no gradients, violet, glassmorphism, typing dots,
   avatars, sparkles, emoji, pill radius).
4. **If it's a record, it's mono.** Provenance lines, tool cards, costs,
   model names, paths → Commit Mono; conversation → Schibsted Grotesk.
5. **Forbidden vocabulary in all UI copy:** no "AI", no "magic"/
   "supercharge"/"unlock", no "sovereignty". Errors state what happened +
   next step, never apologize (§1.7).
   user-facing text contains no em dash, in any form.
6. **The provenance line ships in v1 and is never optional** (§2.2) — under
   every response, with an expandable receipt defined as
   `route · model · cost · time · capability@version[, ...]`.
7. **Sandbox honesty (harness-spec §6.5):** permission gates by default;
   full-auto only with `sandbox.full_auto` + real isolation (bubblewrap /
   Seatbelt); NEVER promise laptop isolation on Windows.
8. **Voice is fully on-device** (§6.7) — zero voice bytes leave the machine.
   Any PR that routes audio to a network endpoint is wrong by construction.
9. **Platform chrome follows the OS** (design-spec §2 platform note); brand
   tokens identical across platforms.
10. **Monetization and launch/publicity are owner-only.**
11. **Local run state is event-sourced.** The append-only SQLite journal in
    [ADR 0002](docs/decisions/0002-event-sourced-run-journal.md) is authoritative
    for live/resumed sessions; UI, receipts, and relay are rebuildable
    projections. This contract precedes, but does not implement, item 9.
12. **Public-evidence rule (docs sync, adopted 2026-07-13).** Any PR that
    changes what an outside user can see or do — public API endpoints or wire
    behavior, install/distribution channels or flags, authentication flows, or
    other externally observable behavior — MUST update this repo's
    public-evidence documentation IN THE SAME PR: `docs/public-evidence/`.
    Evidence files are written to be lifted verbatim into the public docs site
    (muniment.ai/docs): plain factual reference prose, copy-pasteable commands
    and config, no roadmap speculation, no internal codenames, no pricing or
    monetization content (owner-only). The reviewer blocks a PR that changes a
    public surface without updating evidence. Merged evidence changes are
    picked up automatically by the site lane — do not file site tickets by hand.
13. **One mode, and no classification on the cloud-bound wire.** The desktop
    runs a single mode, **the thread surface** (harness-spec §6.1). No mode
    switcher exists. This SPEC and the ROADMAP name that mode identically, and
    never as a chat. An on-device router classifier is allowed, but no desktop
    request carries its class or any other classification. A desktop request
    reaches the cloud with server-supplied grant values alone. The cloud
    classifies every request at ingress with the pinned embedding classifier
    (harness-spec §5.1, ratified by MUNICLOUD-968).
    [docs/desktop-single-mode.md](docs/desktop-single-mode.md) records the mode
    name and the cloud routing-surface name it maps to. Enforcers:
    `test/smoke.sh`, plus `the_grant_request_carries_no_client_classification`
    and `the_receipt_request_carries_only_the_run_id` in
    `src-tauri/core/src/chat_grant.rs`.

## Pi version policy

Muniment Desktop owns its Pi release cycle. Its agent harness is Pi alone.
It takes neither the Claude Agent SDK nor the Claude Code CLI.
[ADR 0008](docs/decisions/0008-pi-runtime-distribution.md) governs executable
acquisition and rollback.

### Production pin and candidate

The **PRODUCTION PIN** records the exact Pi version and extension versions
that a shipped desktop carries. The **CANDIDATE** records the exact versions
that the nightly exercises ahead of production. These are separate values.
The nightly may run ahead of the production pin.

Only Muniment Desktop's nightly evidence on Linux, macOS, and Windows can
qualify a candidate for promotion. All three platforms must pass before a
signed desktop release promotes those exact versions. Nothing else promotes
a version. The desktop never adopts a release because it is newest.
Neither track follows npm `latest`, a semver range, or a mutable release
manifest.

A pin move keeps exactly one verified predecessor resolvable.
Pointer resolution accepts only the production descriptor or that predecessor,
and failed activation rolls back to the predecessor.

The harness installs only packages listed on [pi.dev/packages](https://pi.dev/packages)
and executables from the official [earendil-works/pi repository](https://github.com/earendil-works/pi).
It rejects unlisted npm packages, forks, and mirrors outside Muniment's control.
A Muniment-controlled mirror must preserve ADR 0008's reviewed provenance and
descriptor checks.

### Node floor

The Node floor comes from the pinned Pi package's `engines.node` field.
The selected Node release must satisfy that range and belong to an Active LTS
line. A lower bound does not authorize an unsupported Node line.
The Pi pin and its Node floor move together or not at all.
This rule does not add a system Node dependency to ADR 0008's standalone
executable distribution.

The 2026-09-05 metadata check confirms the upstream split.
[`@earendil-works/pi-coding-agent` 0.85.0](https://registry.npmjs.org/@earendil-works/pi-coding-agent/0.85.0)
requires `node >=22.19.0`. The `legacy-node20` dist-tag holds
[0.74.2](https://registry.npmjs.org/@earendil-works/pi-coding-agent/0.74.2)
at `node >=20.6.0`. These are reference values, not a candidate selection or
a production pin change. The production executable remains at 0.73.1.

### Approved extension set

The owner ruling of 2026-09-05 approves these four packages, all listed on
pi.dev. Adoption must record exact versions under the candidate policy.

| Package | Why the desktop takes it |
| --- | --- |
| [`npm:pi-mcp-adapter`](https://pi.dev/packages/pi-mcp-adapter) | It provides MCP reach through one proxy tool. It discovers servers on demand instead of loading every tool at startup. It reads `.pi/mcp.json` as the project override. It uses the MIT license. |
| [`npm:pi-web-access`](https://pi.dev/packages/pi-web-access) | It provides web reach because Pi ships no web tool of its own. |
| [`npm:pi-subagents`](https://pi.dev/packages/pi-subagents) | It dispatches subagents. |
| [`npm:pi-background-tasks`](https://pi.dev/packages/pi-background-tasks) | It executes background tasks. |

The `pi-web-access` adoption ticket must carry its runtime requirements:
an Anthropic `claude-haiku` model and a Bright Data zone of type `serp`.
Its pi.dev page documents the summary model and the Bright Data SERP zone.
This policy records those requirements without configuring credentials,
selecting providers, or resolving access.

### Built-in tool selection

The pinned 0.73.1 [built-in registry](https://github.com/earendil-works/pi/blob/v0.73.1/packages/coding-agent/src/core/tools/index.ts)
defines exactly `read`, `bash`, `edit`, `write`, `grep`, `find`, and `ls`.
Its default selection is `read`, `bash`, `edit`, and `write`.
The desktop policy enables all seven, adding `grep`, `find`, and `ls`.
It deliberately leaves no defined built-in off.
The pinned registry defines `find`, not `glob`. Tool names must come from
the exact pin, not a tool list from another version.

The adoption must set `defaultTools` in `settings.json` to the full enabled
built-in set. It must not use `--tools`. That CLI allowlist also covers
extension tools, so a built-in-only list would disable the four packages'
extension tools. The pinned 0.73.1 [settings implementation](https://github.com/earendil-works/pi/blob/v0.73.1/packages/coding-agent/src/core/settings-manager.ts)
has no `defaultTools` setting. Adoption therefore requires a candidate that
supports it and a fresh registry check before promotion.

MUNIDESK-1711 records policy only. Separate tickets cover extension adoption
and built-in tool settings. Other tickets cover the bash-timeout unit rule in
the Pi system prompt and a production pin move with its Node floor.
This ticket installs nothing.

## Production-ready gates (release gate)

A desktop release is production-ready when every criterion below holds.
Each criterion names its enforcing artifact. A criterion whose enforcer
reads "gap" is open work: closing it is a gate change first and a symptom
ticket second. Declaring readiness is a gate reading, never a judgment
call.

1. The nightly three-platform e2e run is green with a readable evidence
   envelope on every platform. Enforcer: .github/workflows/nightly.yml,
   with linux-e2e-report consumed by the factory tester.
2. The PR compile and build matrix is green on linux, windows, and macos.
   Enforcer: the desktop-compile and desktop-build jobs in ci.yml.
3. Windows installers are signed and verified. Enforcer:
   test/windows-installers.ps1 in the desktop-build job.
4. macOS builds are signed, notarized, and stapled once Apple clears the
   enrollment. Owner-gated. Enforcer: docs/macos-signing.md checklist plus
   the pre-staged .github/lib/macos-signing.mjs fail-fast behavior.
5. A release promotes only the exact bytes of a green nightly SHA.
   Enforcer: .github/lib/release-promotion.mjs.
6. UI copy obeys the forbidden-vocabulary law, records render in mono, and
   the provenance line is present on every reply. Enforcers: the UI copy law,
   record font law, and provenance line law steps in the ci.yml smoke job.
7. An update path exists or is explicitly deferred by ADR. Enforcer: gap -
   no updater and no ADR records the deferral.

## Folder hierarchy standard

- **Ceiling:** A directory holds at most 50 source files. At the ceiling,
  split along the largest naming family.
- **Floor:** A new folder needs at least 5 files or a machine reader named in
  the pull request. No folders exist for human browsing alone.
- **Depth:** The source layout has at most 3 directory levels below the source
  root.
- **Naming families first:** Related files share a prefix stem. A family of 15
  or more files is the designated split when the ceiling hits.
- **Tests mirror source:** Tests mirror the source layout, except layouts that
  a test harness requires.
- **Ceiling exemptions:** The ceiling does not apply to append-only stores,
  generated trees, vendored trees, or asset directories.
- **Frozen machine-read paths:** Never move or rename a machine-read path
  without a consumer sweep first. The frozen paths in this repo are
  `docs/public-evidence/` (docs-sync reads it by path), `protocol-fixtures/`
  (the versioned fixture contract), and `docs/decisions/`.
- **Recorded gaps:** Directories already over the ceiling are recorded gaps. A
  gap closes gate-first: the split ships with a check that holds the new shape.
  The production-ready model applies: every criterion names one enforcer. A
  criterion without an enforcer is a recorded gap (harness-spec section 14,
  vendored by MUNICLOUD-833). New files must not push a recorded directory
  past its recorded count.

| Directory | Recorded file count | Designated split |
| --- | ---: | --- |
| `src/lib` | 56 | Largest naming family |

## CI

Desktop builds run on ephemeral pve01 VM clones via the `desktop-ci` driver
(one VM at a time; linux 10012 / windows 10011 / macos 10013 templates —
homelab docs/infra-notes.md "Desktop-CI GLUE built"). CI-only gate. The
lint/structure smoke runs on the shared self-hosted runners (no Docker, no
GUI there — real builds happen in the VMs).

## Journal-idea notes (cross-lane content pipeline)

`docs/journal-ideas/` holds standalone notes on shipped features,
engineering lessons, and agent-struggle patterns — raw material for the
public journal on muniment.ai (rules in docs/journal-ideas/README.md).
Merged notes flow to the site lane automatically. Notes must be safe to
publish nearly verbatim: no secrets, no security-sensitive internals.
Writing a note when a PR ships something notable is encouraged, never
blocking.
