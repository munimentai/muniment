# muniment-desktop — SPEC

Muniment is a company system of record. One schema of eight tables holds
entities, identities, edges, an append-only event log, and a provenance row
that binds every extracted fact to the sentence it came from. Go importers
fill it from the tools a business already runs. A grooming report over the
resolved graph is the thing that sells.

muniment desktop is the local app. It is a Tauri v2 shell, a Rust runtime
service, a Pi sidecar, and on-device voice, with the local graph embedded in
the runtime. The harness is a component of the local app and never the offer.
The graph and the report are the product. Its licence is FSL-1.1-Apache-2.0,
and each version converts to Apache 2.0 two years after its own release.

The audience is anyone who runs a business, or wants to, with intelligence and
the company data in one app. Nobody has to be a developer. They install a local
app without asking anyone and run a harness such as Claude Code.

## What this repo is

- The shell: the thread surface, the composer, the artifact rail, the
  provenance line under every reply, and the generated local UI. The shell is
  an ACP client of the runtime service, and mobile and any web client speak
  the same protocol to the same server.
- The runtime service: the single local executor. It runs with no window open,
  hosts the local graph, serves the SQL tool, runs local workflows, and holds
  the one relay leg that mobile drives. One component in the shell owns its
  lifecycle for every window.
- The Pi sidecar: the agent harness, at parity with the factory's Pi.
- The two local models: a bundled router classifier and an optional extractor
  download.
- The event-sourced run journal, the attach protocol, the CAS store, the memory
  index, the code-diff crates, ACP interop, and the ASR and read-aloud stack.

## What this repo is not

- Not the product. The graph and the grooming report are.
- Not the cloud console. Sharing, scheduled execution off the machine, routing
  enforcement, SSO, SCIM, billing, and cross-user audit live in muniment-cloud.
- Not mobile. Mobile is a remote-control window onto this runtime.
- Not a second store of the graph. The webview holds no copy of it.
- Not a sign-in wall. Local mode reaches the thread surface with no cloud
  session.
- Not a harness for sale. Pi, OpenClaw, opencode and their peers are free.

## Interfaces

**The public core.** One app, one download, the FSL app at every tier. With
no account the whole local product works. A free account adds the mobile
connection through the relay. Payment unlocks sharing, scheduled execution and
routing enforcement in the same app. The graph, the read-only SQL tool, the Go
importers, the local report and the generated UI are built in this repo beside
the runtime service, attach, the run journal, the Pi sidecar integration, the
per-thread permission policy, the CAS store, the memory index, the code-diff
crates, ACP interop and the voice stack. The runtime opens the SQLite graph
through rusqlite and serves the SQL tool to a harness. The cloud client code
stays in the app. At the FSL release this repo becomes `munimentai/muniment`
by transfer, history included, so every workflow and every commit here is
written as public.

**muniment-cloud.** Sign-in gates cloud features only. The signing-in screen
shows the sign-in link for a browser that did not open. Cloud-backed use
passes short-lived session tokens and the user's gateway virtual endpoint.
The desktop sends no classification metadata. The cloud classifies every
request at ingress. The desktop posts the protocol alone to `/v1/chat/grants`.
The receipt request carries the run id alone. The relay is one
outbound HTTPS leg from the runtime to MUNICLOUD, direct-first with relay
fallback, end-to-end encrypted, and it needs a free MUNICLOUD account.

**muniment-mobile.** Mobile drives this runtime through the relay. It sees the
live tool stream, the permission-gate card that becomes a run receipt, Stop,
and queued follow-ups. Approving a proposal from the phone is that card.

**muniment-qa.** The nightly runs the installed build on Linux, Windows and
macOS with a readable evidence envelope per platform. The local-mode chat smoke
is the first installed spec, and real sign-in gates no other spec.

**homebrew-muniment.** The nightly bumps the cask version and hashes, and
publish skips while the tap is unseeded. Seeding the tap is human.

**muniment-site.** `docs/public-evidence/` is lifted verbatim into the public
docs. A change to what an outside user sees updates it in the same pull request.

**Updates.** The package manager is the update path before the first public
release, ADR 0029. That release adds the in-app updater: the app reads the
public release feed the nightly publishes, sends nothing that names the user or
the machine, shows one control in the app row once a build is downloaded, and
installs on the click.

## The definition of working

An installed build on all three platforms launches, enters local mode, sends a
message and sees a reply begin, proven by a green nightly. Each item is
checkable in the diff:

- `muniment-runtime` carries the ASR rpath on macOS. It lives one directory
  deeper than the app binary, so its path is `../../Resources`. The Linux e2e
  runner exports no `LD_LIBRARY_PATH`, so the probe proves the real install.
- A saved cloud session launches signed in. Any other launch enters the thread
  surface in local mode, with sign-in at the sidebar foot. The Pi sidecar
  authenticates with Pi's own credential store, never a cloud virtual key.
- The runtime logs its startup and the desktop logs the native-auth call, so a
  silent auth path names its cause from an envelope.
- Signed-in runs give Pi 30 seconds to become ready. After readiness each prompt has
  30 seconds to its first reply event. An acknowledgment does not count. A timeout records
  a failed reply with its cause in the run journal and shell. The runtime logs the run start,
  Pi spawn, first reply, provider outcome and stderr tail by run id.
- Pi sidecar parity with the factory harness, below.

## First run

The first run is one screen, and the composer is on top, ready or one line
from ready. Nothing asks a question before the first message. Under the
composer sit three chips in mono, each opening its own panel, and none blocks
Send. After the first run the chips are the composer band: the model source,
Home, the context meter and the running cost. A global shortcut opens a
one-line launcher that starts a new thread with what the user typed.

**The model chip.** The chip shows the provider's mark and the model id in
use, the saved default when it is shown, else the first shown model. When no
provider answers it reads `Connect a model`, and the first Send opens
Settings → Models. There is no free hosted model at the no-account tier, and
the first run says so plainly.

**Settings → Models.** A key added here goes into Pi's `auth.json`, an
account sign-in runs Pi's own OAuth flow in an RPC process the desktop owns
and lands in the same file, and a local or custom endpoint goes into Pi's
`models.json`, so Pi uses each at once and nothing leaves the machine except
to that provider. The catalog is Pi's built-in provider table. The section
lists connected providers, each with its source tag, `Key`, `Account`,
`Local`, `Custom` or `Claude Code`, its models from `pi --list-models` with a
show switch, the default model and Disconnect, then the popular providers
not yet connected, Anthropic, OpenAI, xAI, Google, OpenRouter, Ollama, LM
Studio and a custom endpoint, as rows. Connect provider searches the rest. A provider opens on one view with its first
method, an account where Pi signs in, Claude Code for Anthropic through
`pi-claude-bridge`, an API key, or a server URL, with its other methods one
switch away. The OpenAI redirect lands on the desktop's own page on port
1455, in muniment's mark and voice, and the desktop hands Pi the code. A
connect adopts the provider's largest-context model that answers a probe as
the default, and an account sign-in brings the app to the front. A local server is a
first-class provider beside the hosted ones, never a fallback. The custom
endpoint form takes a name, a base URL, an optional key and a model list. The
chip's picker sets Pi's `defaultProvider` and `defaultModel` at once.

**The Home chip.** It shows `~/Documents/muniment`, lowercase, with one
control to change it. A configured Home keeps its path. The four folders are
created on the first Send. When one Obsidian vault holds most of the files
the scan found, the chip offers that vault's parent and says why.

**The scan chip.** It reads the scan's summary, `Claude Code: 12 files · Pi:
3 files`, or `No assistant memory found`. Its panel lists one row per
assistant with a checkbox and a mode. *Copy* writes the files under
`memory/imports/<assistant>/` through the onboarding write plan with its
secret rejection and size caps. *Use in place* records the root as a
read-only source in `home.json`, and the memory index reads it beside
`memory/`. Copy is the default under 128 files, in place for vaults and for
any store over the copy caps. Muniment never writes into an in-place source.
An export ZIP from ChatGPT or claude.ai is a second source in the same
panel. Imported text is untrusted data under
[ADR 0018](docs/decisions/0018-untrusted-content-boundary.md).

The scan reads names, never file bodies. It walks only the user-level roots
below, honors each root's environment override, stops at a depth of six, a
two second budget per root and 20,000 files per root, and says when it hit a
cap. It counts durable memory and instruction files only. The scan never
opens, counts or copies a file in the never column.

| Assistant | Root and override | Counted | Never |
| --- | --- | --- | --- |
| Claude Code | `~/.claude`, `CLAUDE_CONFIG_DIR` | `CLAUDE.md`, `rules/*.md`, `projects/*/memory/*.md`, `agent-memory/**/*.md` | `.credentials.json`, `settings*.json`, `*.jsonl` |
| Codex CLI | `~/.codex`, `CODEX_HOME` | `AGENTS.md`, `AGENTS.override.md`, `memories/**/*.md` | `auth.json`, `config.toml`, `sessions/`, `history.jsonl` |
| Grok Build | `~/.grok`, `GROK_HOME` | `skills/**` | `auth.json`, `config.toml`, `sessions/` |
| Hermes | `~/.hermes`, `HERMES_HOME`, Windows `%LOCALAPPDATA%\hermes` | `SOUL.md`, `memories/*.md`, `skills/*/` | `.env`, `auth.json`, `state.db` |
| Pi | `~/.pi/agent`, `PI_CODING_AGENT_DIR` | `AGENTS.md`, `SYSTEM.md`, `APPEND_SYSTEM.md`, `prompts/`, `skills/` | `auth.json`, `settings.json`, `sessions/` |
| Gemini CLI | `~/.gemini`, `GEMINI_CLI_HOME` | `GEMINI.md` | `oauth_creds.json`, `settings.json`, `tmp/` |
| Copilot CLI | `~/.copilot`, `COPILOT_HOME` | `copilot-instructions.md`, `instructions/*.md` | `config.json`, `mcp-secrets/`, `session-state/` |
| Obsidian | the vault paths in `obsidian.json` under the app config directory | `*.md` in each vault outside `.obsidian/` and `.trash/` | `.obsidian/` |
| OpenCode | `~/.config/opencode`, macOS and Linux | `AGENTS.md`, `agents/`, `commands/`, `skills/` | `opencode.db` |
| Goose | `~/.config/goose`, macOS and Linux | `.goosehints`, `memory/*` | `secrets.yaml`, `sessions.db` |
| Continue | `~/.continue` | `rules/*.md`, `prompts/*` | `config.yaml`, `sessions/` |
| Cline | `~/Documents/Cline/Rules`, `~/Documents/Cline/Workflows`, `~/.cline/skills` | `*.md` | `~/.cline/data/` |
| Windsurf | `~/.codeium/windsurf/memories` | `global_rules.md`, memory files | the rest of `~/.codeium` |
| Amp | `~/.config/AGENTS.md` | that file | `~/.config/amp/` |

The scan skips files inside repositories, such as a project `AGENTS.md`,
`CLAUDE.md`, `CONVENTIONS.md` or `.cursor/rules`. The importers read them one
repository at a time. The registry is data in the core crate, and its tests
plant a credential file in every fixture root that must stay uncounted. The
e2e onboarding spec proves the composer, the three chips and a first Send.

## Operating constraints (LAW — the reviewer holds every diff against these)

1. **Local mode exists.** Local mode runs Pi, uses Pi's credential store, and
   reads and writes the local run journal without a cloud session. A cloud
   outage in cloud-backed use shows the full-surface notice.
2. **Cloud credentials stay scoped.** Local mode passes no cloud virtual key.
   Entitlement snapshots are display hints. The server enforces cloud use.
3. **One mode.** The desktop runs a single mode, **the thread surface**. No
   mode switcher exists. This file and the ROADMAP name that mode identically
   and never as a chat. [docs/desktop-single-mode.md](docs/desktop-single-mode.md)
   records the mode name and the cloud routing-surface name it maps to.
   Enforcers: `test/smoke.sh`, and in `src-tauri/core/src/chat_grant.rs` the
   tests `the_grant_request_carries_no_client_classification` and
   `the_receipt_request_carries_only_the_run_id`.
4. **Color law.** Color means computation. `--signal` appears only on the
   mark's thinking state, the running-tool pulse, the streaming underline and
   caret, the provenance route segment, the voice polish flash, and
   workflow-run indicators. Everything else is ink on paper. No gradients,
   violet, glassmorphism, typing dots, avatars, sparkles, emoji, or pill radius.
5. **If it is a record, it is mono.** Provenance lines, tool cards, costs,
   model names and paths render in Commit Mono. Conversation renders in
   Schibsted Grotesk.
6. **Forbidden vocabulary in UI copy.** No "AI", "magic", "supercharge",
   "unlock", or "sovereignty", and no harness name in prose. Errors state what
   happened and the next step and never apologize. No em dash in user-facing
   text, in any form. Enforcer: `npm run lint:copy`.
7. **The provenance line ships under every response and is never optional.**
   The expanded receipt reads `route · model · cost · time ·
   capability@version[, ...]`. A local reply carries no cloud receipt and says
   so.
8. **Sandbox honesty.** Permission gates by default. Full-auto only with
   `sandbox.full_auto` and real isolation: bubblewrap on Linux, Seatbelt on
   macOS. Never promise laptop isolation on Windows.
9. **Voice is fully on-device.** Zero voice bytes leave the machine. A change
   that routes audio to a network endpoint is wrong by construction.
10. **Platform chrome follows the OS.** Brand tokens are identical across
    platforms.
11. **Local run state is event-sourced.** The append-only SQLite journal in
    [ADR 0002](docs/decisions/0002-event-sourced-run-journal.md) is
    authoritative for live and resumed sessions. UI, receipts and relay are
    rebuildable projections.
12. **Public evidence.** A pull request that changes a public surface updates
    `docs/public-evidence/` in the same pull request. Evidence is plain factual
    reference prose with copy-pasteable commands, no roadmap speculation, no
    internal codenames, no pricing.
13. **The write path is propose and commit.** A model never writes a row. The
    SQL tool is read-only. Kind validation applies to every write.
14. **Monetization, launch and publicity are owner-only.**

## The local graph

The single-player graph lives in SQLite through the rusqlite the desktop
already carries. It is the one engine that runs inside the runtime service
with no window open, which local scheduled workflows and the mobile relay
require. `data` uses JSON1, `body_text` uses FTS5, vectors use sqlite-vec, and
`event` is not partitioned locally. The agent-facing SQL dialect is SQLite.

The SQL tool runs on a dedicated read-only connection: `PRAGMA query_only`, an
authorizer set at compile time and never changed on a live connection, a
progress handler or interrupt timer, a hard heap limit, the `sqlite3_limit`
values for untrusted SQL, `SQLITE_DBCONFIG_DEFENSIVE` on, a row cap and a byte
cap per result, CSV output, and an audit log of every query with its error.
Curated views named in the system prompt cover the joins the audit log shows
the agent repeating. Backup runs through the online backup API or `VACUUM
INTO`, never a file copy while open. Third-party data is queried from the
graph, never from the source API.

## Pi version policy

The desktop owns its Pi release cycle. Its agent harness is Pi alone. It takes
neither the Claude Agent SDK nor the Claude Code CLI.
[ADR 0008](docs/decisions/0008-pi-runtime-distribution.md) governs executable
acquisition and rollback.

### Production pin and candidate

The **production pin** records the exact Pi version and extension versions a
shipped desktop carries. The **candidate** records the exact versions the
nightly exercises ahead of production. The nightly may run ahead of the pin.
Only passing nightly evidence on Linux, macOS and Windows qualifies a candidate
for promotion. Neither track follows npm `latest`, a semver range, or a mutable
release manifest. The pin moves to the version the factory runs, and a pin
move keeps exactly one verified predecessor resolvable for rollback.

The production executable pin is 0.73.1. The candidate is 0.85.1 with 0.73.1 as its verified predecessor.
The nightly and the local build select the candidate with the build-time switch `MUNIMENT_PI_CANDIDATE=1`.

The harness installs only packages listed on [pi.dev/packages](https://pi.dev/packages)
and executables from the official [earendil-works/pi repository](https://github.com/earendil-works/pi)
or a Muniment-controlled mirror that preserves ADR 0008's provenance checks.

### Node floor

The Node floor comes from the pinned Pi package's `engines.node` field. The
selected Node release satisfies that range and belongs to an Active LTS line.
The pin and its Node floor move together or not at all. This rule adds no
system Node dependency to ADR 0008's standalone executable distribution.

### Parity with the factory harness

The desktop's Pi carries what the factory's Pi carries, or the local harness
is weaker than the one that built it. Four items:

1. **The extension packages** the factory installs: `npm:pi-web-access`,
   `npm:pi-subagents`, `npm:pi-background-tasks`, and `npm:pi-claude-bridge`
   for Anthropic through the Claude Code sign-in. They give Pi web search and
   fetch, child agents, and `bg_run` with `bg_status`, `bg_logs` and
   `bg_result`. They render into `settings.json` as
   `{"packages": [...], "defaultTools": [...]}`. `pi-web-access` needs an
   Anthropic `claude-haiku` model and a Bright Data zone of type `serp`.
2. **`npm:pi-mcp-adapter`** (MIT). One proxy tool that discovers MCP tools on
   demand. It reads `.pi/mcp.json` as the project override and supports stdio,
   HTTP with SSE fallback, and Unix sockets. This is how the local harness
   reaches the user's MCP servers and, on a paid account, the cloud graph's six
   tools.
3. **The system prompt.** The desktop passes Pi its own prompt: the assistant's
   purpose, its tools, and the bash timeout rule, with no harness or product
   name. Pi reads the `bash` tool's `timeout` in seconds with no default, so the
   prompt says pass one on every call, 60 quick, 600 for a build, longer to `bg_run`.
4. **The version pin** moves to the version the factory runs.

### Built-in tool selection

The desktop enables every built-in tool the pinned registry defines and sets
`defaultTools` in `settings.json` to that full set. It never uses `--tools`,
because that allowlist also covers extension tools. Tool names come from the
exact pin, never from another version's list.

## Local models

**The router is an encoder, bundled, never downloaded.** Routing is three
classes, `route.cloud`, `route.local`, `route.proxy`. The shipped artifact is
granite-embedding-278m-multilingual, Apache-2.0, int8 per-channel ONNX, with
its tokenizer about 282 MB. [ADR 0028](docs/decisions/0028-bundled-router-classifier.md)
names the bytes. The user never opts in and routing always works. The
desktop keeps the classifier's result off the cloud-bound wire.

**The extractor is a generative model, an optional download, on request
only.** About 2.5 GB. It downloads only when the user enables extraction. It
gates the `commitment` and `decision` kinds. Absent, the app works and
extraction is off. The extractor emits a verbatim quote and no offsets. The
deterministic gate locates the span, strips wrapping quotation marks, and
rejects a quote it cannot find. The pin is Qwen3.5-4B, and local extraction
ships on no model measured so far.

**Rules for every bundled or downloaded model.**

- Apache-2.0 or MIT only, verified against the HuggingFace model card. Gemma
  and Llama-licensed models are excluded.
- Per-channel int8 is mandatory and is retested per model.
- The head and the taxonomy version with the encoder.
- The ADR names the bytes: path, size, sha256.
- The lifecycle machinery in [ADR 0021](docs/decisions/0021-on-device-classifier-store.md)
  and [ADR 0027](docs/decisions/0027-pinned-model-artifact-lifecycle.md)
  carries the download: staging, resumable `.part` files, the `current`,
  `previous` and `rejected` pointers, and the install lock.

## Production-ready gates (release gate)

A desktop release is production-ready when every criterion below holds. Each
criterion names its enforcing artifact. A criterion whose enforcer reads "gap"
is open work: closing it is a gate change first and a symptom ticket second.

1. The nightly three-platform e2e run is green with a readable evidence
   envelope on every platform. Enforcer: `.github/workflows/nightly.yml`, with
   `linux-e2e-report` consumed by the factory tester.
2. The PR compile and build matrix is green on linux, windows and macos.
   Enforcer: the desktop-compile and desktop-build jobs in `ci.yml`.
3. Windows installers are signed and verified. The per-user MSI and the NSIS
   setup are the default download, need no administration and register for
   the user. The `-machine.msi` is the managed install for MDM and RMM and
   registers for the machine. Enforcer: `test/windows-installers.ps1` in the
   desktop-build job.
4. macOS builds are signed, notarized and stapled once Apple clears the
   enrollment. Owner-gated. Enforcer: `docs/macos-signing.md` plus the
   fail-fast behavior in `.github/lib/macos-signing.mjs`.
5. A release promotes only the exact bytes of a green nightly SHA. Enforcer:
   `.github/lib/release-promotion.mjs`.
6. UI copy obeys the vocabulary law, records render in mono, and the provenance
   line is on every reply. Enforcers: the three law steps in the `ci.yml` smoke job.
7. The update path follows the Updates paragraph above and ADR 0029. Enforcer:
   `test/smoke.sh` asserts the ADR exists and states the trigger.
8. The steering files obey the rules in `AGENTS.md`. Enforcer:
   `scripts/check-steering.sh .` in the `ci.yml` smoke job.

## Folder hierarchy standard

- **Ceiling:** a directory holds at most 50 source files. At the ceiling, split
  along the largest naming family.
- **Floor:** a new folder needs at least 5 files or a machine reader named in
  the pull request.
- **Depth:** at most 3 directory levels below the source root.
- **Naming families first:** related files share a prefix stem. A family of 15
  or more files is the designated split when the ceiling hits.
- **Tests mirror source,** except layouts a test harness requires.
- **Ceiling exemptions:** append-only stores, generated, vendored and asset trees.
- **Frozen machine-read paths:** `docs/public-evidence/`, `protocol-fixtures/`, `docs/decisions/`. Move one only with a consumer sweep.
- **Recorded gaps:** a directory over the ceiling is a recorded gap. A gap
  closes gate-first: the split ships with a check that holds the new shape. New
  files must not push a recorded directory past its recorded count.

| Directory | Recorded file count | Designated split |
| --- | ---: | --- |
| `src/lib` | 56 | Largest naming family |

## State on disk

Every file the app, the runtime and the agent harness keep sits under one
root, `~/.muniment`, or the directory `MUNIMENT_STATE_DIR` names. Home is the
document folder, a different thing. Logs keep their platform paths.
