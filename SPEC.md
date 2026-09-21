# muniment-desktop — SPEC

Muniment desktop is a local harness for the user's models, tools and work.
Phase one delivers a free desktop app with a public release under FSL-1.1-Apache-2.0.
The app integrates provider accounts, API keys, local models, MCP tools,
projects, agents, artifacts, files, a terminal, a browser, memory and voice.
No Muniment account is required to use the desktop.

Phase two adds cloud availability with paid accounts. Cloud development does
not block the desktop release. The company record and graph remain a separate,
hidden feature while the desktop harness earns adoption.

This repo contains the Tauri v2 shell, Rust runtime, Pi sidecar and on-device
voice stack. Existing company data and cloud credentials remain on disk when
those features are hidden. The release license is FSL-1.1-Apache-2.0. Each version converts to
Apache 2.0 two years after its release. Third-party notices ship with the app.

## Feature availability

`src/feature-flags.js` is the single source of frontend feature availability.
Both flags default off. Only the literal build-time value `true` enables one.
These are developer build options, not controls in user Settings.

- `VITE_MUNIMENT_CLOUD=true` enables Muniment cloud sign-in, the account footer,
  Account settings, saved-session startup and cloud entitlement notices.
- `VITE_MUNIMENT_COMPANY_RECORD=true` enables Record, its keyboard shortcut,
  the company record panel and Companies settings.

The flags are independent. Enabling either does not enable the other.
Disabled sections have no visible controls, empty placeholders or working
shortcuts. A request for a hidden Settings section opens Models & routing.
With cloud disabled, startup enters local mode without reading a cloud session.
A local startup failure offers a local retry, never a cloud sign-in prompt.
Provider sign-in, model routing, allowances, MCP pairing and local tools remain
available. They are not Muniment cloud features. These UI flags do not replace
backend authorization and do not remove stored sessions, companies or graph APIs.

## Project folders

The default Home is `Documents/muniment`. Existing Home choices persist.
Projects are folders under Home's `projects/`, with stable thread membership. Before each reply, the app automatically retrieves relevant context from other ordinary chats in that project. Recall excludes other projects, other owners, and agent or artifact chats.
Create makes the folder. Rename moves the folder and preserves its files and threads.
A project thread runs in its folder. Generated files stay there unless the user names another path.
Threads without a project use `sessions/<thread-name>-<short-id>/`. Project folders use `<project-name>-<short-id>`. Search and previews use the same folder.
Saved artifacts have an editable HTML export in `artifacts/<artifact-name>-<short-id>/index.html`. Full identifiers remain the lookup keys; readable folder names use underscores, a bounded portable name, and eight identifier characters, with case-insensitive collision checks. Files follows the active catalog, project, agent, artifact, or thread and remembers browsing state per context. Internal folders are hidden by default. Workspace tools stay open across navigation. Browser keeps its page; Terminal uses a retained shell per context, started in that context’s folder. Switching contexts never changes or stops a retained shell. Closing the Terminal tab ends its shells.
Agent definitions live in `agents/<agent-name>-<short-id>/agent.md`, with name, job title, description, avatar, project and schedule.
The description supplies persistent instructions. Original SVG avatars use a stable, versioned seed. Faces are mostly neutral or happy; frown and kiss mouths each have a 1-in-500 chance.
New avatars combine shapes, colors, eyes, mouths, glasses, cheeks and face positions. Saved versions keep their appearance.
Avatars render locally in the sidebar, catalog and profile. Rename preserves the seed; Change avatar replaces it.
The Agents control follows New thread and exposes a plus button on hover or keyboard focus.
It opens a card catalog. New agent opens a dedicated creation chat with suggested goals and outputs.
An empty catalog opens the creation chat. Each agent or artifact has a goal, a specified output, and one dedicated chat. Artifacts opens saved artifact chats or a creation chat with suggestions. Creation chats stay outside ordinary thread lists.
Each agent has one persisted primary conversation. Interactive messages and routine runs reuse it.
Routine history stays in the agent profile with time, status, errors, and conversation links.
Agent facts live under agents/<id>/memory/facts. Agent recall reads its own memory.
General recall excludes agent facts. Manual edits and chat tools use the same files, with recoverable deletion.
Changing the agent project directs new work to that project and preserves existing files.
The agent is the sidebar and title-bar identity. Its stored conversations do not appear in regular, pinned, archived, or project thread lists.
An agent profile occupies a 220 px right sidebar. Profile fields reveal inline edit controls on hover or focus.
Agent chats and regular threads use one shared composer with the same features.
Agent panels and maximized record panels fill the chat area while the expanded sidebar stays visible.
Agent threads retain their agent across replies.
Templates import from JSON, Markdown or public Grok share links into an editable draft.
Public link imports identify their profile-only coverage. Supplied skills, memories, plugins and routines persist as template context.
Plugins and routines are setup requirements, not installed capabilities or active schedules. Import never starts a run.
Portable export preserves template context and the avatar seed, with schedules paused.
Export for Grok Bot writes a Markdown setup file containing the profile, template context and original SVG.
Exports omit local project assignments, conversation history, account credentials and private learned memory.
Chat can list, read, save and run agents through harness tools backed by the same files as manual editing.
Daily, weekday and weekly schedules use the host time zone and the local background service.
Due runs wait for the current reply. A missed schedule runs once when the service returns.
An interrupted dispatch requires review instead of an automatic retry. Run results link to their thread.
Profile & Memory has separate name, preferred name, work and instructions fields backed by `memory/profile.md`.
Unrecognized profile sections remain editable. Each new reply receives the saved profile.
Durable facts live in `memory/facts/<id>.md` with their source. Users can edit and delete them.
The memory-save tool records the originating thread and run. It rejects credentials.
Chat maintains facts automatically, including corrections and deletion of verified obsolete facts.
Deleted facts leave recall and stay in private recovery storage. Settings can restore them.
Memory search rebuilds its index from files. Profile imports require Save before taking effect.
Memory and runtime state keep their own locations. A missing project folder stops the run.

## What this repo is

The desktop harness, the thread surface, workspace tabs and local runtime are
phase one. The runtime owns execution, the run journal, provider integration,
permissions, memory and voice. The app provides local project and session
folders, agent and artifact catalogs, routing and editable files.

## What this repo is not

The desktop is not an account requirement or a cloud subscription client first.
It does not require a company graph, an importer or a grooming report to release.
Hosted multi-user services, billing and cloud execution belong to phase two.
The company record remains implemented behind its own flag.

## Interfaces

**The public desktop.** One app and one download provide the free local harness.
Public release uses the desktop repo, with contributor instructions, a security
policy, a license and third-party notices. Optional cloud client code and the
company graph remain in this repo behind separate UI flags. Cloud servers stay
in `muniment-cloud`.

**muniment-cloud, phase two.** When the cloud flag is enabled, sign-in gates cloud features only. The signing-in screen
shows the sign-in link for a browser that did not open. Cloud-backed use passes
short-lived session tokens and the user's gateway virtual endpoint.
The desktop sends no classification metadata. The cloud classifies every
request at ingress. The desktop posts the protocol alone to `/v1/chat/grants`.
The receipt request carries the run id alone. The relay is one outbound HTTPS
leg from the runtime to MUNICLOUD, direct-first with relay fallback, end-to-end
encrypted, and its account tiers belong to phase two.

**muniment-mobile, phase two.** Mobile drives this runtime through the relay and sees the
live tool stream, Stop, queued follow-ups and the permission-gate card that
becomes a run receipt, and approving a proposal from the phone is that card.

**muniment-qa.** The local-mode chat smoke is the first installed nightly spec
on Linux, Windows and macOS, and real sign-in gates no other spec.

**homebrew-muniment.** The nightly bumps the cask version and hashes, publish
skips while the tap is unseeded, and seeding the tap is human.

**muniment-site.** `docs/public-evidence/` is lifted verbatim into the public
docs, and a change to what an outside user sees updates it, law 12.

**Updates.** The package manager is the update path before the first public
release, ADR 0029. That release adds the in-app updater, which reads the public
release feed the nightly publishes, sends nothing that names the user or the
machine, shows one control in the app row once a build is downloaded, and
installs on the click.

## The definition of working

An installed build on all three platforms launches, enters local mode, sends a
message and sees a reply begin, proven by a green nightly. Each item is
checkable in the diff:

- `muniment-runtime` carries the ASR rpath on macOS, `../../Resources`, one
  directory deeper than the app binary, and the Linux e2e runner exports no
  `LD_LIBRARY_PATH`, so the probe proves the real install.
- With cloud enabled, a saved cloud session can launch signed in. With cloud
  disabled, every launch enters the thread surface in local mode. The Pi sidecar
  authenticates with Pi's own credential store, never a cloud virtual key.
- The runtime logs its startup and the desktop logs the native-auth call, so a
  silent auth path names its cause from an envelope.
- Signed-in runs give Pi 30 seconds to become ready, then each prompt has 30
  seconds to its first reply event, and an acknowledgment does not count. A
  timeout records a failed reply with its cause in the run journal and shell.
  The runtime logs the run start, Pi spawn, first reply, provider outcome and
  stderr tail by run id.
- Pi sidecar parity with the factory harness, below.

## First run

The first run is one screen, and the composer is on top, ready or one line from
ready. Nothing asks a question before the first message. Under the composer sit
three chips in mono, each opening its own panel, and none blocks Send. After
the first run the chips are the composer band: the model source, Home, the
context meter and the running cost. A global shortcut opens a one-line launcher
that starts a new thread with the line typed into it.

**The model chip.** The chip shows the provider's mark and the model id in use,
the saved default when it is shown, else the first shown model. When no
provider answers it reads `Connect a model`, and the first Send opens Settings
→ Models. The user supplies model access through a provider or local server.

**Settings → Models & routing is one screen.** Model selection and classifier
settings lead. Full-width account rows show allowance, with usage and weight
in account details. One searchable model list holds visibility and routing
statements. A sample routing test shows the choice and fallback cause without
generating a reply. A key added here goes into Pi's `auth.json`, an
account sign-in runs Pi's own OAuth flow in an RPC process the desktop owns
and lands in the same file, or in the router's pool when the router pools
its family, and a local or custom endpoint goes into Pi's `models.json`, so Pi uses each at once and nothing leaves the machine except
to that provider. The catalog is Pi's built-in provider table. The section
lists connected providers with their source and Disconnect. Connect account
opens the provider catalog and searches its connection methods. A provider opens on one view with its first
method, an account where Pi signs in, Claude Code for Anthropic through
`pi-claude-bridge`, an API key, or a server URL, with its other methods one
switch away. The OpenAI redirect lands on the desktop's own page on port
1455, held on both loopback addresses, in muniment's mark and voice, and the
desktop hands Pi the code. A port another program holds stops the sign-in
with its number. A
connect adopts the provider's largest-context model that answers a probe as
the default, and an account sign-in brings the app to the front. A local server is a
first-class provider beside the hosted ones, never a fallback. The custom
endpoint form takes a name, a base URL, an optional key and a model list. The
chip's picker sets Pi's `defaultProvider` and `defaultModel` at once.

**Routing leads Settings → Models & routing.** Pi's `auth.json` holds one credential per
provider id, so a second account of one provider cannot live there. The router
holds those pools itself in `muniment-router.json` beside it at the same
`0600`, answers the OpenAI chat wire on `127.0.0.1` behind a random token, and
registers as the one Pi provider `muniment-router`. It is off by default, a
subscription that joins a pool turns it on, and off removes that entry and
nothing else. The families it pools are OpenAI,
Anthropic, Google, xAI, Kimi and Devin. An Anthropic key uses Messages. Other keys use the
OpenAI-compatible route, an account may name a base URL of its own for a
gateway or a region, and Devin is a subscription alone.

**A subscription joins the pool through Pi's own sign-in.** The sign-in runs
Pi in a directory of its own, so the credential never lands in Pi's one slot
for the provider, and the router lifts it from there as one more account of
the family, named by its email. Kimi, Antigravity and Devin have no Pi
sign-in, so the router runs its own: Kimi by device code at `auth.kimi.com`,
Antigravity and Devin through the browser, each landing on a loopback page in
muniment's mark. Antigravity is a Google account and pools into Google. A
Kimi, Google, xAI, Codex or Claude token refreshes before it dies, ahead of the turn that
would find it dead. The router asks the upstream
what the account has left: Codex at `chatgpt.com/backend-api/wham/usage`,
Anthropic at `api.anthropic.com/api/oauth/usage`, Kimi at
`api.kimi.com/coding/v1/usages`, Antigravity at
`cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary`, and Devin
at its seat service's `GetUserStatus`. xAI has no such route, and its card
says what the router cannot read. A window is known by its length in
seconds, never by its slot, because a Pro plan's primary window is the weekly
one. The card shows what is left, not what is used, with the reset, the plan,
and Codex's banked resets. The store in `muniment-router-quota.json` carries
no token and no prompt. Allowances refresh when the settings page opens and
every two minutes while it is visible, without overlapping probes. Manual
refresh remains available. Codex and xAI subscriptions use Responses. Claude
and Kimi subscriptions use Messages. Native streams preserve tool calls and
usage. Other subscriptions remain ineligible until their transport exists.

**The router balances, and a refusal moves the turn.** Among the accounts of a
family that serve the model and are not cooling or known to be exhausted,
the one whose served share plus active reservations
sits furthest below its weight takes the next turn. A `401`, `402`, `403`,
`429` or `5xx` is the account refused, not the request: it cools from fifteen
seconds doubling to fifteen minutes and the next account takes the turn, three
accounts at most. Every other status is the request, and it comes straight back
without spending a second account. The ledger in `muniment-router-usage.json`
counts turns, tokens and failures per account for thirty days, names the last
refusal, and carries no prompt and no reply. Settings shows a summary per
provider, the turns each account has in flight, and that thirty-day bar.

**Every active model is in the running.** The options the classifier chooses
between are the pool itself: each model an enabled account serves, keyed
`family/model`. An account that names no model serves every model the catalog
describes for its family, and an account that names models serves those. The
router serves `auto` and every option by name and nothing else, so a model it
does not list is not found rather than found and then unservable. Connect an
account and its models enter the running with no other step.

**A model is chosen by its statement, not its name.** The catalog carries one
statement per model: the work it wins, then the cost or weakness that should
send a query elsewhere. That statement is what the classifier reads, so it
names query shapes and never a benchmark. A user replaces any statement and
renames any option, and an untouched model keeps the catalog's words, which an
application update refreshes. A model outside the catalog runs with its id
alone until a user writes its statement.

**Routing is optional and the classifier is the user's own.** With no
classifier every turn takes the fallback and the pool is still balanced. With
one, the turn's last user message goes out as one choice question over the
statements, and the answer names the option. A confidence under the floor takes
the fallback, so a guess never picks the expensive model. The floor is `0.6`.
The fallback is the option the user names, else the cheapest model in the
running, and a model with no known price is never the cheapest. A classifier is
a network call to a service the user names, the screen says so, and its key is
write-only: nothing reads one back.

**The classifier catalog is models that classify.** TypeSafe's jev answers a
typed choice with a probability over every route. A small model on an account
the pools already hold answers the same choice as one JSON object, and it
spends that account, which the ledger counts. Any endpoint that answers the
same question shape stands beside them. A running classifier is the first row
of the model chip's picker, named by the model that picks, `jev-latest picks`,
and choosing it hands the turn to classification. The picker lists each model
once under its provider. A model two or more accounts serve carries the
balancer mark, and picking it balances across them with no other step.


**The Home chip.** It shows `~/Documents/muniment`, lowercase, with one control
to change it. A configured Home keeps its path. The four folders are created on
the first Send. When one Obsidian vault holds most of the files the scan found,
the chip offers that vault's parent and says why.

**The scan chip.** It reads the scan's summary, `Claude Code: 12 files · Pi: 3
files`, or `No assistant memory found`. Its panel lists one row per assistant
with a checkbox and a mode. *Copy* writes the files under
`memory/imports/<assistant>/` through the onboarding write plan with its secret
rejection and size caps. *Use in place* records the root as a read-only source
in `home.json`, and the memory index reads it beside `memory/`. Copy is the
default under 128 files, in place for vaults and for any store over the copy
caps. Muniment never writes into an in-place source. An export ZIP from ChatGPT
or claude.ai is a second source in the same panel. Imported text is untrusted
data under [ADR 0018](docs/decisions/0018-untrusted-content-boundary.md).

The scan reads names, never file bodies. It walks only the user-level roots
below, honors each root's environment override, stops at a depth of six, a two
second budget per root and 20,000 files per root, and says when it hit a cap.
It counts durable memory and instruction files only. The scan never opens,
counts or copies a file in the never column.

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
plant a credential file in every fixture root that must stay uncounted. The e2e
onboarding spec proves the composer, the three chips and a first Send.

## Operating constraints (LAW — the reviewer holds every diff against these)

1. **Local mode exists.** Local mode runs Pi, uses Pi's credential store, and
   reads and writes the local run journal without a cloud session. A cloud
   outage in cloud-backed use shows the full-surface notice.
2. **Cloud credentials stay scoped.** Local mode passes no cloud virtual key.
   Entitlement snapshots are display hints. The server enforces cloud use.
3. **One mode.** The desktop runs a single mode, **the thread surface**, with
   no mode switcher. Workspace tabs and the optional record panel are panels of
   it. This file and the ROADMAP name that mode identically and never as a
   chat, and [docs/desktop-single-mode.md](docs/desktop-single-mode.md) maps
   it to the cloud routing-surface name. Enforcer:
   `test/smoke.sh`.
4. **Color law.** Color means computation. `--signal` appears only on the
   mark's thinking state, the running-tool pulse, the streaming underline and
   caret, the provenance route segment, the voice polish flash, and
   workflow-run indicators. Everything else is ink on paper. No gradients,
   violet, glassmorphism, typing dots, avatars, sparkles, emoji, or pill radius outside Extend tabs.
5. **If it is a record, it is mono.** Provenance lines, tool cards, costs,
   model names and paths render in Commit Mono. Conversation renders in
   Schibsted Grotesk.
6. **Forbidden vocabulary in UI copy.** No "AI", "magic", "supercharge",
   "unlock", or "sovereignty", and no harness name in prose. Errors state what
   happened and the next step and never apologize. No em dash in user-facing
   text, in any form. Enforcer: `npm run lint:copy`.
7. **The provenance line ships under every response and is never optional.**
   The line reads `route → model` and the clock time, the model id with spaces
   for its hyphens. The rows carry Cost, Tokens, Turns, Tools, Memory and
   Capability, nothing twice. A local reply has no route; its cost reads estimate.
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
13. **Company graph writes use propose and commit.** A model never writes a graph row. The
    SQL tool is read-only. Kind validation applies to every write. No reader
    writes back to a source.
14. **Monetization, launch and publicity are owner-only.**

## Workspace panels

The title bar menu opens Browser, Files and Terminal in a shared tab panel.
Tabs keep a fixed width and scroll horizontally without a scrollbar. Edge fades
stay inside the tab strip, clear of the maximize and panel-collapse controls.
Browser labels show the host, path, query and fragment without the scheme or
leading `www.`. Long browser labels fade on the right. Terminal and Files labels
show the final folder name and fade on the left. Open file tabs retain file names.
Terminal and Files path headers share `OverflowText.svelte`: the full path stays
on one line, starts at its end, and scrolls horizontally without a scrollbar.
Fades mark clipped content and clear when the content fits.

**The file panel.** A read or edited file opens in the shared rail, exclusive
with records and artifacts, with the same resize and maximize controls.
Syntax colors follow the selected theme. The composer has a changed-files
chip with recorded addition and deletion counts. Hover or click opens its
file list. Unknown counts stay unnumbered. Only actions with details expand.
Subscription names default to the supplied sign-in email and remain editable
through a hover pencil and inline form. Grok allowance uses its credits window.

The composer highlights URLs and attached file references. Typing `@` searches
file names under the selected Files folder or the active workspace; selecting a result attaches it. Search excludes hidden
files, generated folders and symbolic links. Preferences controls automatic
context compaction and its token limits. Compaction preserves the conversation
and reports start, completion, cancellation or failure in the action feed.
Web search returns sources without opening a browser unless configured to do so.

## The company record behind its flag

This section defines the optional company graph. It is not a phase-one release gate.

A company is one SQLite graph at `~/.muniment/companies/<id>/graph.sqlite3`
beside a `company.json` that names it, opened through the rusqlite the desktop
carries, the one engine that runs inside the runtime with no window open.
`data` uses JSON1, `body_text` uses FTS5, vectors use sqlite-vec, and `event`
is not partitioned locally. The runtime holds one current company, and the SQL
tool, the record panel and every reader use that one. A free install holds any
number of companies. The paid tenant is one company. A backup is a `VACUUM
INTO` copy to a path the user picks, never a file copy while open.

**The catalogue.** The seventeen core kinds and seventeen relations seed every
company, with three more kinds: `mapping`, the import mapping, `workflow`, a
trigger and its steps, and `view`, a saved filter, sort, group, column set and
layout over one kind. A new table is an extension-only kind under `x_`, a new
column is a `kind_extension` property under `x_`, and a core property never
changes. propose validates against the kind, resolves every identity and
returns a diff, an id and warnings, writing nothing. commit applies the diff in
one transaction and appends one event naming the actor and `on_behalf_of`.

**The SQL tool** runs on a dedicated read-only connection: `PRAGMA query_only`,
an authorizer set at compile time and never changed on a live connection, a
progress handler or interrupt timer, a hard heap limit, the `sqlite3_limit`
values for untrusted SQL, `SQLITE_DBCONFIG_DEFENSIVE` on, a row cap and a byte
cap per result, CSV output, and an audit log of every query in a file beside
the graph, never in it. Curated views named in the system prompt cover the
joins the audit log shows the agent repeating. `muniment-cli mcp` serves `sql`,
`propose` and `commit` as one MCP server over stdio, to Claude Code and to the
desktop's Pi alike.

**The record panel.** The Record control at the app row's right end opens the
panel in the rail's grid area, exclusive with the artifact rail. Its table,
record view, board and saved views generate from `kind`, `kind_extension` and
`kind.states`, no screen is hand-written for one kind, and an edit calls
propose, shows the diff and its warnings, and commits on the user's confirm.

**Readers.** Every source implements Objects, Describe, Page and Delta, and
nothing else about it reaches the graph. File readers run in Rust inside the
runtime, one CSV file as one object. Network readers are Go: one bundled
sidecar, a subcommand per source, JSON over stdio, no SQLite access. The
runtime holds each cursor and does every write as the `reader` service
principal acting for the owner, through an `approved` `mapping` record whose
identity column keys each row, so a repeat run updates and never duplicates,
and it keeps each row it could not place in `resolve-<mapping>.json` beside the
graph. [ADR 0033](docs/decisions/0033-reader-contract.md) fixes the mapping and
the three `reader.*` operations, desktop only. Third-party data is queried from
the graph, never from the source API.

## Pi version policy

The desktop owns its Pi release cycle. Its agent harness is Pi. The optional Claude provider bridge uses the
user's Claude Code connection. Other providers use their configured transports. [ADR 0008](docs/decisions/0008-pi-runtime-distribution.md)
governs executable acquisition and rollback.

### Production pin and candidate

The **production pin** is the exact Pi version and extension versions a shipped
desktop carries. The **candidate** is the exact versions the nightly exercises
ahead of production, and only passing nightly evidence on Linux, macOS and
Windows qualifies it for promotion. Neither track follows npm `latest`, a
semver range, or a mutable release manifest. The pin moves to the version the
factory runs and keeps one verified predecessor for rollback. The production
executable pin is 0.73.1. The candidate is 0.85.1 with 0.73.1 as its verified
predecessor. The nightly and the local build select the candidate with the
build-time switch `MUNIMENT_PI_CANDIDATE=1`.

The harness installs only packages listed on [pi.dev/packages](https://pi.dev/packages)
and executables from the official [earendil-works/pi repository](https://github.com/earendil-works/pi)
or a Muniment-controlled mirror that keeps ADR 0008's provenance checks.

The Node floor is the pinned Pi package's `engines.node` range, met by an
Active LTS release. It moves with the pin or not at all, and it adds no system
Node dependency to ADR 0008's standalone executable distribution.

### Parity with the factory harness

The desktop's Pi carries what the factory's Pi carries, or the local harness is
weaker than the one that built it. Four items:

1. **The extension packages** the factory installs: `npm:pi-web-access`,
   `npm:pi-subagents`, `npm:pi-background-tasks`, and `npm:pi-claude-bridge`
   for Anthropic through the Claude Code sign-in. They give Pi web search and
   fetch, child agents, and `bg_run` with `bg_status`, `bg_logs` and
   `bg_result`. They render into `settings.json` as
   `{"packages": [...], "defaultTools": [...]}`. `pi-web-access` needs an
   Anthropic `claude-haiku` model and a Bright Data zone of type `serp`.
2. **`npm:pi-mcp-adapter`** (MIT). One proxy tool that discovers MCP tools on
   demand, reads `.pi/mcp.json` as the project override, and speaks stdio,
   HTTP with SSE fallback, and Unix sockets. Through it the local harness
   reaches the user's MCP servers and, on a paid account, the cloud graph's tools.
3. **The system prompt.** The desktop passes Pi its own prompt: purpose, tools, the bash
   timeout rule, and the launch facts: the model, earlier thread models, host and shell, and
   the working directory, which is the thread's project or session folder. Each message opens with its
   send time. No harness or product name. Pi reads `bash` `timeout` in seconds with no
   default, so the prompt says pass one on every call, 60 quick, 600 for a build, longer to `bg_run`.
4. **The version pin** moves to the version the factory runs.

The desktop enables every built-in tool the pinned registry defines and sets
`defaultTools` in `settings.json` to that full set. It never uses `--tools`,
because that allowlist also covers extension tools, and tool names come from
the exact pin.

## Local models

**The cloud-routing classifier is bundled.** It is separate from the optional
local provider classifier configured in Models & routing. Its taxonomy is three
classes, `route.cloud`, `route.local`, `route.proxy`. The shipped artifact is
granite-embedding-278m-multilingual, Apache-2.0, int8 per-channel ONNX, about
282 MB with its tokenizer, and [ADR
0028](docs/decisions/0028-bundled-router-classifier.md) names the bytes. The
desktop keeps this classifier's result off the cloud-bound wire. Its presence
does not enable cloud sign-in or require a company graph.

**The extractor is a generative model, an optional download, on request only.**
About 2.5 GB, it downloads only when the user enables extraction, and it gates
the `commitment` and `decision` kinds. Absent, the app works and extraction is
off. The extractor emits a verbatim quote and no offsets, and the deterministic
gate locates the span, strips wrapping quotation marks, and rejects a quote it
cannot find. The pin is Qwen3.5-4B, and local extraction ships on no model
measured so far.

**Rules for every bundled or downloaded model.** Apache-2.0 or MIT only,
verified against the HuggingFace model card, so Gemma and Llama-licensed
models are out. Per-channel int8 is mandatory and retested per model. The
head and the taxonomy version with the encoder, and the ADR names the bytes:
path, size, sha256. [ADR 0021](docs/decisions/0021-on-device-classifier-store.md)
and [ADR 0027](docs/decisions/0027-pinned-model-artifact-lifecycle.md) carry
the download: staging, resumable `.part` files, the `current`, `previous` and
`rejected` pointers, and the install lock.

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
   setup are the default download, need no administration and register for the
   user. The `-machine.msi` registers for the machine, the managed install for
   MDM and RMM. Enforcer: `test/windows-installers.ps1` in the desktop-build job.
4. macOS release builds are signed, notarized and stapled. The local build
   script signs and verifies the app but does not establish notarization. Enforcer: `docs/macos-signing.md` plus the
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

A directory holds at most 50 source files and splits along its largest naming
family at the ceiling. A new folder needs 5 files or a machine reader named in
the pull request. Depth stops at 3 levels below the source root. Related files
share a prefix stem, and a family of 15 or more is the designated split. Tests
mirror source except where a harness needs a layout. Append-only stores,
generated, vendored and asset trees are exempt. `docs/public-evidence/`,
`protocol-fixtures/` and `docs/decisions/` are frozen machine-read paths that
move only with a consumer sweep. A directory over the ceiling is a recorded
gap, closed gate-first with a check that holds the new shape, and related changes include their readers when a family moves.

## State on disk

Every file the app, the runtime and the agent harness keep sits under
`~/.muniment` or the directory `MUNIMENT_STATE_DIR` names. Home is the document
folder, a different thing, and logs keep their platform paths.

Thread names use a separate model request on the first message, limited to one
to three words. A failed request keeps a short prompt fallback. Manual names
win over generated names. Action feedback groups reads, searches, commands
and edits with expandable, bounded, credential-filtered details. Saved runs
retain those details. Cancel sign-in returns to local mode and rejects a late
browser result. Partial replies remain visible with their interruption cause.
Reports link possible duplicate groups to their records and a merge preview.
Record fields, states and actor names read as words. Raw evidence stays available.
### Extend
Settings → Extend manages MCP servers, skills and plugins without a Muniment account. The MCP catalog includes Anthropic's public connector directory, provider logos, compact responsive cards and popularity sorting.
Search matches names, publishers and categories. Category, connection type,
installed and setup-required filters combine before pagination. Provider account
requirements and unsupported setup methods remain visible.

Skills and plugins install from GitHub repositories, local folders or ZIP/TAR archives. A preview
lists supported skills, MCP servers and executable plugin components. The user
selects skills before installation. Versions stay pinned until an explicit
update, and the previous installed version supports rollback. Unsupported hooks
produce an error before installation. Plugin dependencies install with package
lifecycle scripts disabled.

The composer tools menu has MCPs, Plugins and Skills branches. File attachments use a paperclip.
MCP switches and inline slash commands apply to one turn and reset after submission.
All composer panels share the same anchor. Explicit selections take priority. Disabled
extensions stay outside the runtime MCP snapshot and skill context.
Optional extension routing uses the configured classifier and only installed,
enabled candidates. A timeout or low confidence adds no capability. Credentials
stay in the system credential store or use environment variable references.
