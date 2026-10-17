# muniment-core

The shared Rust core of Muniment. The Muniment desktop and headless router
hosts build on the same crates, so local runtime behavior, model routing and
runtime pins match in both.

This directory is the core of the `munimentai/muniment` repository. Core
releases are the `core-vX.Y.Z` tags and releases there. The desktop releases
use the `vX.Y.Z` tags.

## Crates

| Crate | Path | What it is |
| --- | --- | --- |
| `muniment-core` | `crates/core` | Local runtime logic and storage contracts: the run journal, Pi sidecar supervision and install, memory, projects, attach server, on-device speech and voice. |
| `muniment-models` | `crates/models` | Provider and model knowledge as data: the provider registry, local server kinds and their scan, provider and endpoint model discovery, context windows, the model catalog, and Pi's `models.json`. |
| `muniment-router` | `crates/router` | The multi-account model router: account pools, routing policy, the loopback OpenAI-compatible server, the `muniment-router` server binary (`server` feature) and the `muniment-router-shadow` binary. |
| `muniment-runtime-core` | `crates/runtime` | The platform-neutral runtime: attach serving and its accept loops, parallel runs, retention, the agent run queue with its cron, webhook and MCP routines, the company record and reader service, the model service that runs the model router and the web search server and writes provider settings, web search settings and credentials, and `run_runtime`, the entry a host without OS service registration runs. The host supplies cloud accounts, chat grants, credential storage, provider OAuth clients, the sign-in page and its executables through `RuntimeHost`. A host with a cloud account and a paired phone also gets the session relay publisher. The default `keyring` feature adds the keychain credential store and keychain prompt history. |
| `muniment-server` | `crates/server` | The headless runtime host: the `muniment-server` binary runs the runtime with no cloud account, no keychain and no OS service registration, for a container or a server. |
| `muniment-pins` | `crates/pins` | Typed constants compiled from `pins/pins.toml`, plus the embedded package lockfile. |
| `muniment-attach` | `crates/attach` | The reader and companion attach protocol and its client, with the pinned TLS transport, pairing links and device credentials of the remote route. |
| `muniment-code-diff` | `crates/code-diff` | Portable code-diff values and fixtures. |
| `muniment-atomic-file` | `crates/atomic-file` | Atomic file replacement, including the Windows retry path. |
| `muniment-search` | `crates/search` | Keyless web search: Brave, DuckDuckGo, Wikipedia, Yahoo and Bing parsers, paced per-engine sessions, metasearch, and a loopback server that answers the SearXNG JSON format. Brave uses the system `curl` program when one is present. `cargo run -p muniment-search --example live -- "query"` searches the real engines once. |

`muniment-core` re-exports `muniment-models` as `muniment_core::models` and
`muniment-router` as `muniment_core::model_router`, along with
`provider_models`, `endpoint_models`, `http` and `atomic_file`, so existing
`muniment_core::...` paths keep working. `muniment-router` re-exports the
models modules it routes with, such as `muniment_router::family`.

Desktop-only code (cloud sign-in, cloud chat grants and browser control) lives in
the desktop. The host supplies it through the `account` and `chat_launch` values,
the `PiLaunchBoundaries` and `RunAttachBoundaries` traits, and the runtime's
`RuntimeHost` value.
`docs/core-boundary.md` lists every module and crate and
`scripts/check-core-boundary.sh` enforces the boundary.

## Pins

`pins/pins.toml` pins the Pi release (current and rollback), one archive per
platform with its size and SHA-256, the Pi extension packages, and the Claude
Code version. `pins/packages.bun.lock` locks the packages. Platform names use
`<target_os>-<target_arch>`, for example `macos-aarch64`. A package entry's
`patch` field names a git diff under `pins/patches/` that Bun applies through
`patchedDependencies`, and `bump` refuses to move that package until the patch
is removed or remade for the new version.

The `muniment-pins` binary keeps the pins current:

```sh
cargo run -p muniment-pins --features cli -- check
cargo run -p muniment-pins --features cli -- bump --pi latest --packages --claude-code latest
cargo run -p muniment-pins --features cli -- compat --report compat.json
```

`check` prints each pin beside its newest release as JSON. `bump` downloads
every platform archive of the new Pi release and of its rollback, records their
sizes and SHA-256 digests, and resolves `pins/packages.bun.lock` with the new
Pi's Bun. `--package NAME@VERSION` pins one package. `compat` runs the
compatibility suite on the host platform:

- the pinned Pi in JSON and RPC modes against a fake OpenAI-compatible stream
  server (`scripts/pins/compat/`),
- a load check for every pinned package and for the tools the system prompt names,
- muniment-core's sidecar, launch and RPC frame tests against the real Pi,
- the router against a fake Anthropic upstream with the pinned Claude Code identity,
- one `pi-claude-bridge` turn through the pinned Claude Code CLI against a fake
  Anthropic endpoint.

The `core-pins-update` workflow runs daily. It bumps every pin that passes the
suite on Linux, then runs the suite and the workspace tests on macOS and the CI
checks on Linux against the bumped tree. When all of them pass, it rebases the
bump onto `main` and pushes it with a `feat:` or `fix:` message. It then tags
the next `core-v*` release with `pins.toml`, `packages.bun.lock` and the signed
server archives attached, as the release job does. A pin that fails gets one
`Pin update blocked: <component> <version>` issue and stays at its version.

## Consumers

The desktop in this repository builds the core crates by path, so a desktop
build always uses the core beside it. Another Rust host depends on a core
release tag:

```toml
muniment-core = { git = "https://github.com/munimentai/muniment", tag = "core-v1.15.0", features = ["tls"] }
```

The repository carries no third-party OAuth client secrets. A host that offers
Antigravity sign-in registers its client at startup with
`muniment_router::native_auth::set_provider_clients`, or sets
`MUNIMENT_ANTIGRAVITY_CLIENT_ID` and `MUNIMENT_ANTIGRAVITY_CLIENT_SECRET`.
Without a client, Antigravity sign-in and token refresh answer a configuration
error and every other provider keeps working.

A headless host builds the router binaries from a release tag and reads the
pins from the release asset:

```sh
cargo install --git https://github.com/munimentai/muniment --tag core-v1.15.0 --features server muniment-router
gh release download core-v1.15.0 --repo munimentai/muniment --pattern pins.toml
```

Core releases are never marked latest, so `releases/latest` on the repository
always names a desktop release. List the `core-v*` tags to find the newest core
release.

## Remote attach

A runtime can serve `muniment.attach/1` over TLS to clients on other machines,
such as over a LAN, a private overlay network or an SSH forward. Pairing needs no account. The
listener is off until the host binds it.

- `RemoteAccess::open(<state root>/remote, options)` creates, on first use, an
  ECDSA P-256 key, a self-signed certificate, a runtime id and a display name,
  all owner-only. Clients pin the certificate's SHA-256. The identity is never
  replaced: a missing key or certificate fails the open.
  `RuntimeAttachState::open_remote_access` opens it under the state root and
  shares it with the local desktop route.
- `RemoteAccess::create_pairing_link` answers
  `muniment://pair?v=1&id=…&name=…&fp=…&host=…#<secret>`. The secret is 128
  random bits in the fragment, works once, expires after ten minutes, and is
  stored only as its SHA-256. At most 16 links wait at once.
- Redeeming a link, through the optional `pairing` field of `hello`, issues a
  device credential: an id and a 256-bit secret, stored only as its SHA-256
  with the device's name, scopes and times. The client sends it as
  `authorized_client_id` and `authorized_client_credential` on later
  connections. `welcome` carries `runtime_id`, `runtime_name`,
  `runtime_version`, `install_kind` and, when the hardware says,
  `form_factor`: `laptop`, `all_in_one`, `desktop` or `server`.
- `RuntimeAttachState::serve_remote_listener` or `run_remote_listener` serves
  the route: TLS 1.3 only, a connection cap, handshake and first-frame
  deadlines, a 16 KiB first frame, and a growing hold on a peer address after
  repeated refusals.
- Scopes are `thread.read`, `run.write`, `runtime.update`,
  `runtime.admin`, `agents.manage`, which `runtime.admin` holds too, and
  `swarm.peer`, which a linked runtime holds alone and which reaches only
  `swarm.exchange`. A link
  grants `thread.read`, `run.write` and `runtime.update` unless its creator
  names others, and a client can ask for fewer, never more. Only the local owner and `runtime.admin` devices create
  links, list devices and revoke other devices, and a device delegates only
  scopes it holds. Any device may revoke itself. `runtime.update` or `runtime.admin` sends `runtime.update`.
- `remote_rule` names what a remote client may send: the desktop client
  operations except workspace onboarding, migration control, approval
  presentation, same-machine companions, and the host's cloud account and
  phone pairing. Reader connections and model changes need `runtime.admin`,
  except the choices a run makes, its model, thinking level, the models the
  picker hides and a capacity reading, which `run.write` covers.
  A refusal for a missing scope names what the device lacks.
- `models.share` replaces the model connections the calling device shares
  and needs `run.write`. The runtime writes the device's Pi entries beside its
  own in `auth.json`, keeps its own entry for a provider it already has, adds
  the device's router accounts as `shared:<device>:<id>`, and records each in
  `<state root>/agent/muniment-shared.json`. A sign-in whose provider rotates
  refresh tokens arrives without one, so only the sharing machine refreshes
  it. Revoking the device removes what it shared. `models.share_bundle`, on
  the local owner route only, answers this runtime's own connections, each
  rotating sign-in refreshed when it has under 30 minutes left.
- With a MuniCloud account, `models.vault` on the local owner route uploads
  this computer's connections to the account's model vault, releases them
  back, or issues a machine token for one paired machine. The vault then
  holds each rotating refresh token alone. A `models.share` bundle may carry
  `cloud`, the vault's base URL and a machine token, and the runtime then
  fetches the vault's leases at once and every five minutes, under the device
  `municloud`. The computer that uploaded fetches them with its own session.
  The route refuses requests that name a file on the runtime's machine: the
  `csv` reader source, and `run.submit` with `files`.
- Revoking a device closes its live sessions. `runtime.update` takes a
  version newer than the one the runtime announces, records it in
  `<state root>/remote/runtime-update.json` and sends it to
  `RemoteAccess::update_requests`. The host's updater applies it only from
  its own signed releases.
- `runtime.info` answers the runtime's identity, `certificate_sha256`,
  `update_request`, `update_capability` and `update_status`.
  `update_capability` names who installs a requested version: `launcher`
  (the server's launcher), `desktop` (the desktop app's updater) or `manual`
  (nothing on the runtime). A desktop install reports `desktop`, and a server
  or container `manual` unless its host calls
  `RemoteAccess::set_update_capability`. `update_status` is
  `{"state", "target_version", "blockers", "error"?, "updated_at"}`, where
  `state` is `idle`, `queued`, `downloading`, `draining`, `installing`,
  `restarting` or `failed`, `target_version` names the version or is null,
  and while draining `blockers` lists the work the runtime waits for:
  `active_run`, `pending_permission_gate`, `authentication_operation`,
  `session_refresh` or `in_flight_external_effect`. The updater saves it in
  `<state root>/remote/update-status.json` with
  `RemoteAccess::set_update_status`. A pending request with no updater
  status reads as `queued`.
- The client side is `pair_with_link`, `connect_remote_runtime` and
  `RemoteRuntimeRecord`, which saves the credential to an owner-only file.
  `cargo run -p muniment-attach --features client --example remote-smoke --
  <link-or-record> [--record <path>]` pairs or reconnects once and prints the
  runtime's info and its thread count.

`RuntimeAttachState::start_remote_route` opens remote access and serves the
remote route as `<state root>/remote/settings.json` directs:
`{"version":1,"enabled":false,"listen":null,"endpoints":[],"allow_public":false}`
by default. A null `listen` binds port 7443 on every interface. Unless
`allow_public` is true, the listener closes a connection from a public
address before the handshake, and only loopback, private LAN, link-local and
overlay peers reach it. Empty `endpoints` let the runtime list its own
non-loopback interface addresses with the bound port, IPv4 first, at most
eight, and public addresses only when `allow_public` is true. Each listed endpoint carries a `kind`:
`configured`, `loopback`, `lan` (RFC 1918 or IPv6 unique local), `overlay`
(100.64.0.0/10 and fd7a:115c:a1e0::/48, which private overlay networks
assign) or `public`. Windows lists no interfaces, so a link there names
configured endpoints. A save starts, stops or rebinds the listener without a
restart. The serving listener publishes its address and endpoints in
`<state root>/remote/listener.json` for commands in other processes.

The local owner route, and never the remote route or a companion, also
answers:

- `remote.settings` with `{}`: `{"settings": {"enabled", "listen",
  "endpoints", "allow_public"}, "overridden", "runtime_id", "runtime_name",
  "certificate_sha256", "listening", "endpoints": [{"endpoint", "kind"}],
  "error"}`. `listening` is the bound address or null, and `error` says why
  the listener is not serving.
- `remote.settings_save` with `{"enabled": true, "listen": "0.0.0.0:7443" or
  null, "endpoints": ["host:port"], "allow_public": false}`, answered like
  `remote.settings`.
- `remote.pairing_create`, `remote.device_list`, `remote.device_revoke`,
  `runtime.info` and `runtime.update`, as a `runtime.admin` device sends them.

A local desktop session's welcome carries the same `runtime_id`,
`runtime_name`, `runtime_version` and `install_kind` once the host opened
remote access. Its chat-event subscription (`run.chat_events`) also carries
`runtime.update_requested` events with the body `{"version",
"requested_at", "requested_by"}`, so the desktop runs its updater. A remote
device never receives them.

A chat-event subscription on either route that sends
`{"update_status": true}` also receives `runtime.update_status` events, whose
body is the `update_status` of `runtime.info`: first the current status, then
each change, checked once a second. A runtime that predates the field refuses
the request as `invalid_request`, and
`DesktopClient::subscribe_chat_events_with_update_status` then subscribes
without it. A run the runtime resumed on its own carries `resumeNotice` in
its chat events and thread history.

## Agents

An agent is `<Home>/agents/<folder>/agent.md`: a JSON header in a fence and
the instructions below it. `muniment_core::agents` reads and writes it. The
header's v2 fields are all optional, so a v1 file loads unchanged:

- `model`: `{"provider", "id", "thinking"?}`, a reference to a model the
  runtime already serves. A local run in the agent's thread starts with it in
  place of the default model, and at its thinking level unless the run names
  one. A cloud grant names its own model.
- `tools`: `{"disabled": [tool names]}`, the Pi tool names, MCP tools
  included, that the agent's runs exclude. It is an advanced, deprecated
  control kept for compatibility; permission modes and rules are the main
  control.
- `skills`: installed skill and plugin ids from the extension inventory,
  picked on every turn of the agent's thread as an automatic pick is.
- `permissions`: `{"mode"?, "always_allow": [request], "ask_first":
  [request], "deny"?: [request]}`. `mode` is a permission mode (see
  [Permission modes](#permission-modes)); a new agent saved without one is
  saved in the runtime's `default_agent_mode`, and an older agent without
  one runs in it. The old ids read as the new ones: `ask` is
  `manual`, `auto` is `auto` and `strict` is `dont_ask`, and the next save
  writes the new id. A request is a thread permission policy's
  `{"request_kind", "resource"}`: an exact absolute path and operation, an
  exact command and working directory, or an exact tool name and input. The
  lists hold in every thread of the agent as preset deny, ask and allow
  policies beside the global rules and the thread's own journaled policies.
  The gate shows the call's exact request and a summary and offers "Allow
  once", "Allow for this thread", "Always allow for this agent" and "Deny".
  "Allow for this thread" journals an allow policy in the thread. "Always
  allow for this agent" also appends the request to `always_allow` through
  the agent save path. A call an ask policy covers never offers it, a
  protected path or critical command offers neither, and the save refuses a
  request `ask_first` names.
- `primary_runtime`, the runtime id that runs the agent's routines;
  `messaging`, `{"allow": "any"}` or a list of agent ids; `routines`; and
  `paused_reason`, which stops the routines while set.

A v1 `schedule` reads as the cron routine `schedule`, and a client that
edits `schedule` edits that routine. A folder whose `agent.md` does not load
is left out of `list` and named in its `errors`. Each agent's state history
keeps its last 200 runs. `delete_agent` removes the agent's private memory
only when the caller confirms it.

A routine is `{"id", "name", "enabled", "trigger", "prompt",
"created_from": "user"|"instructions"|"agent", "last_fired_at",
"last_result", "proposed"?, "last_poll"?, "next_fire_at"?}`. The runtime
keeps `last_fired_at`, `last_result`, `last_poll` and `next_fire_at`, the
Unix second a cron routine fires or an MCP routine polls next, and a save
ignores them. Its trigger is `{"kind": "cron",
"expr", "tz"}`, a five-field cron expression in an IANA time zone that runs
at most every five minutes; `{"kind": "webhook", "hook_id", "preset":
"github"|"forgejo"|"generic", "secret_sha256"?}`; or `{"kind": "mcp",
"server", "tool", "args", "every": {"expr", "tz"}, "condition"}`, where a
condition is `{"all": [...]}`, `{"any": [...]}` or `{"path":
"/json/pointer", "op", "value"?}` and `op` is `eq`, `ne`, `gt`, `gte`, `lt`,
`lte`, `contains`, `exists`, `changed` or `nonempty`.

- **Cron.** The runtime's schedule loop queues a run when the routine comes
  due. A routine missed while the runtime was off runs once.
- **Webhook.** `POST /hooks/<hook_id>` on the remote-access listener, beside
  the attach route, queues a run. The credential store keeps each hook's
  secret under the service `ai.muniment.webhook`; the routine keeps only its
  SHA-256. A delivery proves the secret with `Authorization: Bearer`, an
  HMAC-SHA256 of the body in `X-Muniment-Signature`, or its preset's header:
  `X-Hub-Signature-256` for `github`, `X-Forgejo-Signature` or
  `X-Gitea-Signature` for `forgejo`. Payloads stop at 256 KB, each hook takes
  30 deliveries a minute and holds 10 runs at most, and a delivery id
  (`X-GitHub-Delivery`, `X-Forgejo-Delivery`, `X-Gitea-Delivery` or
  `X-Muniment-Delivery`) is refused when it repeats within seven days. A
  refused credential counts toward the peer's backoff. The listener answers
  `202` with `{"run_id"}`, or `401`, `404`, `409`, `411`, `413` or `429`.
  Senders pin the runtime's certificate, and senders on public addresses
  reach the listener only while remote access allows public peers.
- **MCP.** On its `every` schedule the runtime calls the tool with no model:
  Pi's MCP client calls it with the server definition and token a chat turn
  uses, through a provider extension that answers the one turn with that
  tool call. The text result read as JSON, or else the server's structured
  content or the text, must pass the condition. `changed` compares the value
  at its path with the previous poll's, which the routine's state keeps, and
  never holds on the first poll. `last_poll` is `{"at", "fired", "result"?,
  "error"?, "failures", "retry_at"?}`; each failed poll doubles the wait
  before the next, from one minute to six hours.

A webhook payload or an MCP result reaches the run's prompt in an untrusted
`<webhook_payload>` or `<mcp_result>` block, and the run asks before every
write as any event-triggered run does.

Saving an agent's instructions proposes routines. A deterministic reader
turns clear schedule phrases ("every morning at 9am", "every weekday at
8:30", "every 15 minutes", "on the 1st of every month") into cron routines
in the computer's time zone; work that names an installed MCP server and one
of its tools polls that tool with a `nonempty` condition, and an "if ..."
clause stays in the prompt, whose run replies `NO_RESPONSE` when it does not
hold. On the attach route a cheap model call (the memory pass's model)
refines the proposals once per version of the instructions, with a strict
JSON answer whose routines pass the same checks. Proposals are saved
disabled with `"proposed": true` and `"created_from": "instructions"`; a
dismissed one is not proposed again.

### Templates

`muniment_core::agent_templates` reads public Grok Bot share links (the
page's serialized data, never its scripts), Grok Bot JSON, Markdown with a
`# Name` heading or a fenced `json` configuration, and Muniment agent files
(versions 1 to 3), all under 128 KB. A template's routines become disabled
proposals: schedules become cron routines, GitHub, Forgejo and webhook
events become webhook routines, and the rest are kept as notes. Its skills
match installed skills by name, and the rest stay as reference text. Its
plugins and tools match installed extensions or are listed for setup. Its
memories become facts from outside the conversation in the agent's memory
folder. `agent.export` writes a Grok Bot setup in Markdown or a Muniment
agent file that imports without loss. An export holds no webhook secret or
hook id, no primary machine, no private memory and nothing the secret
scanner flags.

### Run queue

Every run an agent makes without a user message is a `Run` in
`<profile>/agent-runs.sqlite3` before it starts, under the id its journal run
takes: `{"id", "agent_id", "thread_id", "trigger": {"kind", "routine_id"?,
"source"?}, "status", "attempt", "lease_owner", "lease_expires_at", "hop",
"origin_run_id", "created_at", "updated_at", "result_summary", "silent"}`.
`status` is `queued`, `leased`, `running`, `waiting_input`, `completed`,
`failed`, `cancelled` or `interrupted`, and a trigger kind is `manual`,
`cron`, `webhook`, `mcp` or `agent_message`. The journal stays the record of
a started run: every pass of the runtime's schedule loop reads each run's
outcome from it, renews the leases of the runs it drives, queues again a run
whose lease expired before it started (three attempts at most) and marks
`interrupted` one that started and lost its driver. A run that waits for an
answer gets `reminded_at` after four hours and is cancelled after 24. A
routine run whose reply is exactly `NO_RESPONSE` completes with
`"silent": true` and leaves no entry in its thread's history.

An agent's `activity`, `{"state": "idle"|"running"|"waiting"|"error",
"action"?, "since"}`, derives from its runs in the queue and the gate a run
waits at. A run that waits for an answer shows `waiting` with the approval's
summary, a running or queued run shows `running`, and with none the last run
shows `error` with its reason when it failed, else `idle`. `since` is null
for an agent that never ran.

A failed run's inputs stay in the agent's folder as
`replays/<run id>.json`, the newest 20: the prompt, with a webhook, MCP or
agent message payload still in its untrusted block, the trigger, the hop, the
error, `agent_version` (the SHA-256 of the agent definition the run started
with), the agent's `model` reference and `replayed_by`. Text the secret
scanner flags is removed first and `redacted` says so.

The runtime drives up to `MUNIMENT_MAX_PARALLEL_RUNS` runs at once (3 by
default, at most 16), chat replies included, each with its own Pi process and
at most one in each thread. Queued agent runs start oldest first in their
agent's primary thread when a place and the thread are free. An update drain
records every live run, and the next start resumes each of them.

### Agent operations

Both routes answer `agent.list` (`{}` → `{"agents", "primary_threads",
"latest_runs", "errors"}`), `agent.read` (`{"id"}` → `{"agent",
"primary_thread_id"}`), `agent.save` (`{"agent"}` → `{"agent"}`),
`agent.delete` (`{"id", "remove_memory"?}`), `agent.run_now` (`{"id",
"prompt"?}` → `{"run"}`), `agent.runs` (`{"agent_id"?, "limit"?, "cursor"?}`
→ `{"runs", "next_cursor"}`, newest first), `routine.list` (`{"agent_id"}`),
`routine.save` (`{"agent_id", "routine"}` → `{"routine"}`),
`routine.delete` (`{"agent_id", "routine_id"}`),
`agent.routine_proposals` (`{"agent_id"}` → `{"proposals"}`),
`agent.routine_proposal_accept` and `agent.routine_proposal_dismiss`
(`{"agent_id", "routine_id"}`; accept answers `{"routine"}`),
`agent.import` (`{"source"}`, a share link or a template's text →
`{"agent", "memories", "skills": {"matched", "proposals"}, "tools":
{"matched", "setup"}, "routine_notes", "source", "coverage"}`),
`agent.import_commit` (`{"agent", "memories"?}` → `{"agent"}`),
`agent.export` (`{"id", "format": "grok_bot"|"muniment"}` → `{"format",
"file_name", "content"}`), `routine.webhook_create` (`{"agent_id",
"routine"}`) and `routine.webhook_rotate` (`{"agent_id", "routine_id"}`),
which answer `{"routine", "secret", "hook"}` and are the only answers that
carry the secret, and `routine.webhook_info` (`{"agent_id", "routine_id"}` →
`{"hook"}`). A hook is `{"hook_id", "preset", "urls", "certificate_sha256",
"listening"}`. `agent.threads` (`{"id"}` → `{"threads"}`) lists every thread
of one agent that the journal holds, newest first, as `{"thread_id",
"title", "updated_at", "primary"}`. `agent.tool_catalog` (`{}` →
`{"builtin", "mcp", "skills"}`) lists what an agent on this runtime can use:
the built-in tools a launch offers as `{"id", "label", "read_only"}`, each
enabled MCP server as `{"server", "tools": [{"name"}], "checked_at"?}` with
the tools its last connection test listed, and each enabled skill as `{"id",
"name", "description"}`, where `id` is what an agent's `skills` names.
`agent.list` adds each agent's `activity`. `agent.replays` (`{"id"}` →
`{"replays", "agent_version"}`) lists an agent's saved failed runs, newest
first, each with `prompt_preview`, `prompt_bytes` and `definition_changed`
in place of the prompt. `agent.replay` (`{"id", "replay_id"}` → `{"run",
"replay"}`) queues the saved prompt again as a run of the agent's current
definition with the failed run's trigger kind, `source: "replay"` and
`origin_run_id` set to the replay id.
Remotely the reads, `agent.routine_proposals`, `agent.threads`,
`agent.tool_catalog`, `agent.replays` and `routine.webhook_info` take
`thread.read`, `agent.run_now` and `agent.replay` take
`run.write`, and the changes, imports and exports take `agents.manage`. A chat-event subscription that sends `{"agent_runs": true}`
also receives `agent.run_updated` events whose body is the changed `Run`
with `notify`, true when the change warrants a notification: the run
completed with a reply, failed, or waits for an answer. A run another swarm
member reports carries its `runtime_id`. A
runtime without the queue refuses the request, and
`DesktopClient::subscribe_chat_events_with_agent_runs` then subscribes
without it. The model reaches the same agents through the `agent-*` and
`routine-list`, `routine-save` and `routine-delete` tools. A refused agent or
swarm operation answers `invalid_request` with the reason in the error's
`details`; `DesktopClientHolder::agent_request`, `swarm_request` and
`request_with_error` return that `ProtocolError` beside the `ClientError`.

### Permission modes

One permission gate decides every tool call of every run, composer threads
and agent threads alike. `muniment_core::permission_gate` holds it, on the
journal's thread permission policies. A run loads the gate extension, which
asks the runtime before each call. The rules apply in this order:

1. Deny rules: global, then the agent's, then the thread's. They refuse.
2. Ask-first rules, in the same order. They ask; `dont_ask` denies instead.
3. Protected paths and critical commands. They ask in every mode but
   `dont_ask`, which denies, `plan`, which refuses a change, and `bypass`,
   which runs a protected path that is not critical.
4. Always-allow rules, which `plan` ignores for changes.
5. The mode's baseline.

| Mode | `title` | Baseline |
| --- | --- | --- |
| `manual` | Ask before changes | Reads and read-only commands run; edits, other commands, web and MCP calls ask. |
| `edit_auto` | Edit automatically | Also file edits and `mkdir`, `touch`, `mv`, `cp` and `rm` inside the working folders run. |
| `plan` | Plan first | Reads only; changes are refused. The agent submits a plan with `plan_submit`, and the approval picks the mode the thread continues in. |
| `auto` | Auto review | A cheap model reviews each action that is not a read and allows it or pauses it with its reason. |
| `dont_ask` | Don't ask | Reads and always-allowed actions run; the rest is denied without asking, and the denial is the tool's result. |
| `bypass` | Bypass permissions | Everything runs past the first three steps. It is the default while `bypass_enabled` is on, and refused while it is off. |

- **Reads.** File reads, Muniment's memory and plan tools, and shell commands
  the reader proves read-only: the read-only command core (`cat`, `grep`,
  `rg`, `ls`, `find` and the rest) unless an option withdraws it (`find
  -exec`, `fd -x`, `sort -o`, a writing `sed` or `awk`), plus `git status`,
  `log`, `diff`, `show`, `rev-parse`, `ls-files` and `blame`. A pipeline or
  compound command reads only when every part reads. A command the reader
  cannot follow, a variable assignment, and anything behind a wrapper (`bash
  -c`, `eval`, `sudo`, `env`, `xargs` and the like) counts as a command that
  may do anything.
- **Paths.** Checks separate reads from writes and match both the given path
  and its symlink-resolved form. A write to `.git`, `.muniment`, an MCP
  configuration file, a shell startup file, or the runtime's state folder
  (outside the working folders) is protected; so is a read in the state
  folder, apart from its installed extensions and skills. A write counts as
  inside the working folders only when both forms are.
- **Critical commands.** Removing `/`, the home folder or a system folder
  recursively, wiping or formatting a disk, writing a disk device, and
  shutting the computer down, also behind wrappers.
- **Auto review.** The reviewer is the cheap model path the memory pass uses
  (router cheapest, then Pi). It reads the run's request, all of it marked
  untrusted when a webhook, an MCP poll or another agent started the run, and
  answers exactly `{"verdict": "allow"|"block", "reason"}`. A failed call or
  any other reply asks. After 3 blocks in a row or 20 in a run, every action
  asks. Each verdict is logged as `permission_auto outcome=...`, and a pause
  carries its reason in the gate title and in its approval's `reason`.
- **Unattended runs.** A gate of a run a routine, a webhook, an MCP poll or
  another agent started, or of any run while no client used the runtime for
  two minutes, waits as an agent approval. When nobody answers within
  `unattended_timeout_seconds` (four hours unless changed), the gate closes
  unanswered, which the agent reads as "nobody answered", never as a
  refusal or an approval, and the run stops with that note.

The thread's mode is journal state: `thread.permission.mode.set`
(`{"mode", "actor", "source": "user"|"plan"}`), the latest one winning. A
thread with none runs in its agent's mode, else the runtime's default.
`run.submit` takes an optional `permission_mode`. The runtime journals it,
else the default thread mode for a new thread that is no agent's, before the
run starts, so it decides the run's first tool call. The runtime's settings
live in `<profile>/permission-settings.json`. Bypass is on and both default
modes are `bypass` until the owner saves other settings. A field the file
leaves out takes that default, or `manual` for a mode while bypass is off.
Turning bypass off moves a default that names it, and that the same save
does not set, to `manual`.

Operations: `permission.modes` (`{}` → `{"modes": [{"id", "title",
"description", "available", "reason"?}], "default_thread_mode",
"default_agent_mode"}`); `permission.settings` (`{}` →
`{"default_thread_mode", "default_agent_mode", "bypass_enabled",
"unattended_timeout_seconds", "deny", "ask_first", "always_allow"}`);
`permission.settings_save` (any of those fields → the settings); and
`thread.permission_mode` (`{"thread_id"}`) and `thread.permission_mode_set`
(`{"thread_id", "mode"}`), which answer `{"thread_id", "mode", "title",
"source": "thread"|"agent"|"default", "set_by"?, "agent_id"?}`. A plan gate
waits as an approval with `plan` and the answers `plan_manual`,
`plan_edit_auto`, `plan_auto` and `plan_keep`. `thread.plan`
(`{"thread_id"}` → `{"plan": null|{"run_id", "gate_id", "approval_id",
"summary", "plan", "answers"}}`) reads it, and `thread.plan_answer`
(`{"thread_id", "answer"}` → `{"status": "answered", "answer"}`) answers it,
as `agent.approval_answer` does for an agent's thread. They are agent
operations, so `DesktopClient::agent_request` sends them. Remotely the reads
take `thread.read`, the thread changes take `run.write`, and
`permission.settings_save` takes `runtime.admin`.

### Swarm

`muniment_core::swarm` links one owner's runtimes so their agents work
together. Each runtime keeps its records in `<profile>/swarm.sqlite3` and the
thread content in its run journal. The runtime supplies replica scope,
steering, gate answers and run starts through `SwarmHost`.

- **Links.** A `runtime.admin` client calls `swarm.invite` (`{"endpoints"?}`
  → `{"link", "expires_at", "runtime_id"}`) on one runtime, which issues a
  `swarm.peer` pairing link, and `swarm.join` (`{"link"}` → `{"member"}`) on
  the other, which redeems it, saves the record under
  `<profile>/swarm/links/` and dials with backoff from one second to a
  minute. Data flows both ways in each `swarm.exchange`, so one reachable end
  suffices. The answering runtime lists the link as a `swarm-peer` device, the
  dialing one as an `outbound` device, and `remote.device_revoke` drops it on
  either side; `swarm.leave` (`{"runtime_id"}`) drops every link to a member.
- **Relay.** Members gossip to every linked member, which passes everything
  on, so each member reaches every other through any chain of links.
  `swarm.members` (`{}` → `{"self", "members", "absence"}`) lists each member
  with `online` (heard from within a minute) and this runtime's `link` to it.
- **Directory.** Each runtime describes the agents it runs as `{"agent_id",
  "name", "label", "avatar", "primary_runtime", "epoch", "updated_at",
  "deleted", "primary_thread", "activity"}`; the highest `epoch`, then the
  latest `updated_at`, wins. Each member sends `agent.activity`
  (`{"agent_id", "runtime_id", "activity"}`) to its clients when an agent it
  runs changes activity or the directory brings another member's change. `swarm.agents` answers it, and `swarm.changed` events
  (`{"kind"}`) report changes to members, agents, groups, threads and the
  absence pause.
- **Threads.** Agent and group threads replicate as journal events with
  `(origin, seq)`: the replica keeps the thread, run and event ids, so the
  normal thread reads answer it on every member. Only the primary writes
  runs; `run.submit` into a replica is refused. `thread.post`
  (`{"thread_id"}` or `{"agent_id"}`, `"text"` → `{"message_id",
  "thread_id", "origin_runtime", "seq"}`) appends a `thread.message.posted`
  event with the poster's vector clock, and the primary consumes it in causal
  order: it steers a live run or queues one, then appends
  `thread.message.consumed`. `thread.history` lists posts in `posted`.
  Attachments and stored payloads travel on demand when a member opens the
  thread.
- **Moves.** `agent.move` (`{"id", "to", "force"?}` → `{"status", "agent"}`)
  raises the agent's `epoch` on its primary, which then fires none of its
  routines, and hands the definition, memory folder, routine state, thread
  clocks and saved conversations to the target, which takes over once its
  replicas caught up. The hand-over also carries the secrets of the agent's
  webhook routines, which the target keeps in its credential store, and the
  saved MCP servers its MCP routines call, with their bearer tokens, which
  the target adds unless it has a server of that id or name. With `force`, the target takes the agent from the
  directory's copy while the old primary is offline, and the old primary
  steps down when it sees the higher epoch.
- **Messaging.** The `agent-message` (`{"to", "intent", "text",
  "reply_to"?}`) and `agent-handoff` (`{"to", "task", "context"?}`) tools
  send through a durable outbox that every member relays until the target
  acknowledges. The target wakes in its primary thread with an untrusted
  `<agent_message>` or `<agent_handoff>` block. A request or question returns
  the target's final reply as a `result`. Each step adds one hop, a user
  message starts at zero, and a seventh step is refused. `messaging.allow` is
  checked at send, a handoff's key is `handoff:<run id>`, and a handoff back
  to the agent that sent the task is refused.
- **Groups.** `group.create` (`{"name", "members", "lead"?,
  "meeting_mode"?, "debate_rounds"?}`), `group.list` and `group.update` (`{"group_id", ...}`)
  answer `{"group"}` or `{"groups"}`. A group holds two to six agents and its
  creator's runtime owns its thread. `@everyone` wakes every member, `@Name`
  the named ones, and no mention the first. A meeting runs a blind round, up
  to two debate rounds and the lead's summary. Each agent works in its own
  thread; only its reply, without credentials and with the start of a long
  draft, reaches the group. A reply's author carries `round` and `phase`:
  `chat` at round 0 outside a meeting, else `blind`, `debate` or `summary`.
  Each member sends `group.posted` (`{"group_id", "thread_id", "post"}`, the
  post as `thread.history` lists it) to its clients for every post to a
  group thread it writes or replicates.
- **Approvals.** A gate an agent run waits at reaches every member's clients
  as `agent.approval_pending`, and the phone relay of any member shows it.
  `agent.approvals` lists the pending ones and `agent.approval_answer`
  (`{"approval_id", "answer"}` → `{"status": "answered"|"already_answered"|"sent",
  "approval"}`) answers one; the member that runs the gate applies the first
  answer. `agent.approval_resolved` follows. An agent gate's approval carries
  `rule`, the exact `{"request_kind", "resource"}` an `always_allow` entry
  takes, `summary`, `reason`, why the gate asks, and `answers`, the answer
  ids it offers: `allow_once`, `allow_thread`, `always_allow_agent` and
  `deny`. A plan gate's approval carries `plan` and no `rule`, and offers
  `plan_manual`, `plan_edit_auto`, `plan_auto` and `plan_keep`. `answer` is
  one of those ids or a gate answer such as `{"type": "confirm", "value":
  true}`.
- **Absence.** When no client used any member for 14 days, each member sets
  `paused_reason: "absence"` on the agents it runs. `swarm.resume` clears it
  on every member.

A chat-event subscription that sends `{"swarm": true}` receives
`swarm.changed`, `agent.approval_pending` and `agent.approval_resolved`.
With `"swarm_live": true` too it also receives `agent.activity` and
`group.posted`; `DesktopClient::subscribe_chat_events_with_swarm_live` falls
back to the swarm events alone on a runtime that refuses the flag.
Remotely, linking takes `runtime.admin`, the reads take `thread.read`,
`thread.post`, group changes and answers take `run.write`, and `agent.move`
and `swarm.resume` take `agents.manage`.

## Phone relay

A desktop host with a cloud account relays its live runs to the one phone
paired with it, end to end encrypted under `muniment.relay-encryption/1`.
The cloud orders, stores and forwards the sealed payloads and reads only
their plaintext headers.

- `muniment_core::relay_encryption` holds the contract: the X25519 agreement
  of pairing revision 2, the phone's HMAC-SHA256 proof over the pairing
  transcript, HKDF-SHA256 for the desktop-to-phone and phone-to-desktop keys,
  AES-256-GCM sealing with a random 12-byte nonce and the canonical header as
  associated data, the per-direction counter and the 64-message command
  replay window. Its tests reproduce every vector of the contract's
  conformance package, kept at
  `protocol-fixtures/muniment.relay-encryption/1/relay-encryption-v1.json`.
- `muniment_core::relay_publisher` is the desktop side of
  `muniment.session-relay/1`. While the host's `AccountService` answers relay
  keys and a signed-in session, it keeps one WebSocket to the cloud's
  `/v1/events` with the native access token, waits for `connection.ready`,
  answers heartbeats and reconnects with backoff. It registers each live run
  as a relay session and publishes assistant text, tool activity, run status
  and permission asks as sealed events. It opens the phone's sealed stop,
  steer, follow-up and permission answers, checks their header binding,
  counter and expiry, applies them through the runtime's own operations and
  answers with a receipt or a failure. When the cloud answers that the pair
  is unavailable, it closes the connection and asks the account for the pair
  again before it reconnects.
- The runtime starts the publisher with its schedules when the host has a
  cloud. A host whose account answers no relay keys, such as
  `muniment-server`, opens no connection. The pair's counters and replay
  window live in `remote-control-relay.json` under the profile, and the host
  keeps the keys.

## Router server

`muniment-router serve` runs the router as a network server for headless agent
runs. It builds only with
the `server` feature, so the desktop never compiles Postgres, OpenBao or the
run-token code. `deploy/router/Containerfile` builds a non-root image whose
health check runs `muniment-router health`.

Settings come from a TOML file (`--config` or `MUNIMENT_ROUTER_CONFIG`), and a
`MUNIMENT_ROUTER_*` variable overrides each one: `openbao.role_id` is
`MUNIMENT_ROUTER_OPENBAO_ROLE_ID`.

| Setting | Default | Meaning |
| --- | --- | --- |
| `listen` | `127.0.0.1:8790` | Bind address. |
| `admin_token` | required | Bearer for `/v1/runs` and `/v1/outcomes`, 16 characters or more. |
| `run_token_signing_key` | required | HMAC-SHA256 key for run tokens: 64 hex characters, or 32 bytes of text. |
| `database_url` | | Postgres store. Migrations run at start. |
| `state_dir` | | The desktop's JSON files instead of Postgres, for development. |
| `openbao.address`, `.mount`, `.prefix` | `secret`, `muniment-router/accounts` | KV v2 secret per account at `<mount>/data/<prefix>/<account id>`. |
| `openbao.token` or `.role_id` and `.secret_id` | | Token or AppRole login (`openbao.approle_mount`, default `approle`). |
| `openbao.cache_ttl_s` | `60` | How long a read credential is reused. |
| `openbao.tls_pin_sha256` | | SHA-256 of a self-signed OpenBao certificate to trust instead of the OS store. |
| `catalog`, `catalog_poll_s` | embedded, `5` | Catalog file, reloaded when its contents change. |
| `policy_mode` | `adaptive` | `current`, `strong`, `ratchet` or `adaptive`. |
| `classifier.kind` | `none` | `typesafe`, `endpoint` or `pooled`, with `base_url`, `api_key`, `model`, `family`. |
| `success.half_life_days`, `.min_samples` | `7`, `5` | Decay and sample floor of measured success rates. |
| `quota_probe_interval_s` | `900` | Subscription quota probes. `0` turns them off. |
| `drain_timeout_s` | `100` | How long SIGTERM waits for open streams. |
| `metrics` | `true` | Serve `/metrics`. |
| `langfuse.host`, `.public_key`, `.secret_key` | | Langfuse ingestion. Off unless all three are set. A key that starts with `PLACEHOLDER` counts as unset. |

Endpoints: `POST /v1/runs`, `GET` and `DELETE /v1/runs/{run_id}` and
`POST /v1/outcomes` take the admin token. A run's usage carries `last_status`,
`last_error_type` and `last_status_at` for the run's most recent request when
the router answered it with an error: the status, one of `budget_exhausted`,
`routing_constraints`, `routing_budget`, `upstream_unavailable`,
`rate_limited` or `auth` (or null), and the RFC 3339 time. A later request
the router answers without an error sets all three back to null. `POST /v1/chat/completions` and
`GET /v1/models` take a run token and read the `x-muniment-task` and
`x-muniment-validation-failures` headers. With Langfuse on, each chat
completion becomes one generation with its model, usage, cost, latency, run id,
task id and role. It joins the trace that the `x-muniment-trace` header names,
or the task's own trace when the header is absent. A background thread posts
the generations in batches, so Langfuse never delays a turn. `budget_usd` on
`POST /v1/runs` is optional. A run's turn reserves its uncached cost estimate
before it goes upstream and settles the priced cost after, and the run's usage
records the spend either way. A run without `budget_usd` is never refused on
spend. A run with one gets 402 with
`{"error":{"type":"budget_exhausted",…}}` once spend reaches it, and a turn
that no eligible model's estimate fits answers 422 with
`{"error":{"type":"routing_constraints",…}}`. Routing constraints run before
any classifier sees text. The server ignores `policy.task_budget_usd`. The
desktop keeps `policy.offline_only` (loopback endpoints only) and its own
`policy.task_budget_usd`. `GET /healthz` and `GET /metrics` take no token.

`muniment-router accounts list|add-key|login|import-pi-auth|set-weight|disable|enable|remove|probe`
manages the pool in the configured store and secret source. `muniment-router
catalog check <file>` checks a catalog file.

The catalog (`crates/models/catalog.toml`) is a list of `[[model]]` tables with
`family`, `model`, `name`, `tier` (`deep`, `balanced` or `fast`), `price` and
`output` in US dollars per million tokens, `context` (`400K`, `1M`),
`strengths` and `limits`. A file that fails the check keeps the last good
catalog in force.

## Runtime server

`muniment-server serve` runs the runtime headless on Linux and stops cleanly on
SIGTERM or SIGINT. It builds `muniment-runtime-core` without its `keyring`
feature, so it never opens a keychain: prompt history lives in files under the
state root and secrets live in `FileCredentialStore`. State lives in the
directory `MUNIMENT_STATE_DIR` names, or `~/.muniment`. Its Home, which
holds agents, memory and projects, is the folder `home.json` in the state root
names; `serve` saves `<state>/home` there when none is saved, so a container
keeps its agents on its volume. It runs in local mode, whose `local`
workspace is the one its threads and agent runs use. The attach endpoint
sits under `XDG_RUNTIME_DIR`, or under `run` in the state root when that is
unset. One instance holds the endpoint's instance lock. `muniment-server
launch` runs `serve` under the launcher that installs requested updates, as
[Self-update](#self-update) describes.

`muniment-server health` exits 0 when the runtime's attach endpoint answers a
handshake. `muniment-server prepare` installs the pinned Pi into the directory
`MUNIMENT_PI_ROOT` names, or `harness` in the state root, checking the
archive's size and SHA-256 against the pins, then installs the pinned extension
packages and prints the Pi executable's path.

A secret named `name` of service `service` lives in
`<state>/credentials/<service>/<name>` with mode 0600. The variable
`MUNIMENT_SECRET_<SERVICE>_<NAME>` overrides it: the service and the name
upper-cased, with each character other than an ASCII letter or digit written
as `_`. The reader's `ai.muniment.reader` secret `stripe` is
`MUNIMENT_SECRET_AI_MUNIMENT_READER_STRIPE`.

`serve` starts the remote route. These variables override the stored remote
access settings without changing them: `MUNIMENT_REMOTE_ACCESS` (`on` or
`off`), `MUNIMENT_REMOTE_LISTEN` (an IP address and port, such as
`0.0.0.0:7443`), `MUNIMENT_REMOTE_ENDPOINTS` (a comma list of `host:port`
the links name) and `MUNIMENT_REMOTE_ALLOW_PUBLIC` (`on` or `off`). `install_kind` is `server`, or `container` when
`MUNIMENT_INSTALL_KIND=container`. `MUNIMENT_REMOTE_NAME` sets the name links
and paired devices show, which defaults to `Muniment runtime`. The runtime
announces the release it was built from: a build sets
`MUNIMENT_RUNTIME_VERSION`, and a build without it announces the crate
version. At start `serve` logs the runtime id, the certificate fingerprint and
the listen address.

These commands work on the state directory while `serve` runs in another
process, such as over SSH or `docker exec`:

- `muniment-server pair [--scopes thread.read,run.write] [--json]` prints a
  one-time pairing link that names the endpoints the serving listener
  published. `--json` prints `{"link", "expires_at", "scopes", "endpoints",
  "fingerprint", "runtime_id", "runtime_name"}`.
- `muniment-server devices [--json]` lists the paired devices.
- `muniment-server revoke <device-id>` revokes one. Its live sessions close
  within a second.
- `muniment-server info [--json]` prints the runtime id, name, version,
  install kind, certificate fingerprint, remote access state, any update
  request and the launcher's update status.

`deploy/runtime/Containerfile` builds a non-root image with the server, the
pinned Pi for the image's architecture and the pinned packages. State lives in
the `/data` volume, and the health check runs `muniment-server health`. The
image sets `MUNIMENT_INSTALL_KIND=container` and `MUNIMENT_REMOTE_ACCESS=on`
and exposes port 7443. Publish the port on the address clients reach, such
as a LAN or tailnet address rather than every host interface, and set
`MUNIMENT_REMOTE_ENDPOINTS` to it:

```sh
docker build -f deploy/runtime/Containerfile \
  --build-arg MUNIMENT_RUNTIME_VERSION=1.13.1 -t muniment-runtime .
docker run -d --name muniment -p 192.0.2.20:7443:7443 \
  -e MUNIMENT_REMOTE_ENDPOINTS=runtime.example.net:7443 \
  -e MUNIMENT_REMOTE_NAME="Build box" \
  -v muniment-data:/data muniment-runtime
docker exec muniment muniment-server pair
```

Each core release publishes the image for linux/amd64 and linux/arm64 as
`ghcr.io/munimentai/muniment-runtime:<version>` and `:latest`:

```sh
docker run -d --name muniment -p 192.0.2.20:7443:7443 \
  -e MUNIMENT_REMOTE_ENDPOINTS=runtime.example.net:7443 \
  -v muniment-data:/data ghcr.io/munimentai/muniment-runtime:latest
```

The CI `runtime-image-publish` job runs `deploy/runtime/publish.sh` after the
release job tags a `core-v*` version. It pushes to each image repository in the
`RUNTIME_IMAGE_REPOSITORIES` variable (a space list) and logs in with the
`RUNTIME_IMAGE_LOGINS` secret, one `<registry> <user> <password>` line per
registry. A repository whose registry has no login is skipped with a warning.
For the platforms in `RUNTIME_IMAGE_PLATFORMS` (default
`linux/amd64,linux/arm64`), buildx builds each platform from the release's
signed server archive for its architecture, so no platform compiles under
emulation. Only `muniment-server prepare` runs emulated, to install the
pinned Pi for that architecture. A runner without buildx or an emulator for
the other platform publishes its own platform from source and warns.

A `runtime.update` request writes `/data/remote/runtime-update.json`
(`<state root>/remote/runtime-update.json` outside the image), owner-only:

```json
{"version":"0.2.0","requested_at":"2026-01-01T00:00:00Z","requested_by":"<device id or local>"}
```

`version` starts with a digit and holds letters, digits, `.`, `-` and `+`.
`deploy/runtime/smoke.sh <image>` builds the `remote-smoke` target of the
same file, pairs it with the runtime, reconnects across a restart and checks
that a revoked device is refused.

### Self-update

The image runs `muniment-server launch`, which runs the server under a
launcher that installs the requested version itself. A server outside a
container gets the same with `launch` in place of `serve`. The launcher is
the binary it started from, so it stays the same across the versions it
installs, and a new image replaces it. It keeps each version under
`<state root>/runtime/versions/<version>/` with `current` and `previous`
pointers, and runs the current version, or its own when that is newer or
nothing is installed. It reports `update_capability: launcher` when it can
verify releases and write its versions directory, and `manual` otherwise.

Each core release publishes, beside `pins.toml`,
`muniment-server-<version>-linux-x86_64.tar.gz` and
`muniment-server-<version>-linux-aarch64.tar.gz`, each the server and the
speech runtime libraries it loads from its own directory. `SHA256SUMS` lists
them, and `SHA256SUMS.minisig` is a minisign signature over it. The release
job and the pins update both run `deploy/runtime/release-assets.sh`, which
cross-compiles both with cargo-zigbuild against glibc 2.36 and signs with the
`MUNIMENT_RELEASE_SIGNING_KEY` secret, and the optional
`MUNIMENT_RELEASE_SIGNING_PASSWORD`. It fails before tagging when the key is
missing or `RELEASE_PUBLIC_KEY` in `crates/server/src/release.rs` does not
verify the signature, so no release goes out unsigned.

While a request names a version newer than the running one, the launcher:

1. Downloads the archive for its architecture, `SHA256SUMS` and the
   signature from `<feed>/core-v<version>/`.
2. Checks the signature against the release public key, then the archive's
   SHA-256, and unpacks its regular files into a staging directory.
3. Runs the new server's `--version`, which must name the version.
4. Sends the server `SIGUSR1`. The server refuses new runs with the
   retryable `runtime_draining` error and waits up to the grace period for
   running work to finish, reporting `draining` with its blockers. It
   records the runs still active, then stops.
5. Stops what is left of the server's process group, so no agent of the
   old version runs beside the new one, and snapshots the SQLite files under
   the state root.
6. Starts the new version as a trial. It keeps it when `health` passes
   within the trial period. Otherwise it restores the snapshot, starts the
   earlier version again, reports `failed` and never retries that version.
   A newer release installs as usual.
7. Clears the request and reports `idle`.

A failed download leaves the request queued, with the error in
`update_status`, and retries after the retry delay, doubling each time up to
an hour, so a runtime that was offline still converges. When a release
that carries `pins.toml` lacks the archive, `SHA256SUMS` or the signature,
the launcher logs `launcher update skipped`, clears the request, reports
`idle` with the reason and keeps its version. A launcher that
stops during a trial rolls it back when it starts again.

After the new version starts and the launcher keeps it, the runtime resumes
each run the update interrupted from its Pi session, one at a time, and the
thread shows that the reply resumed after an update. A run that waited on a
permission gate stays as it was for its owner to answer or resume, and a
thread resumes on its own at most three times in a row. A run that stopped
in the middle of a tool call needs a manual decision, since the tool's
outcome is unknown, so it stays interrupted too.

| Variable | Default | Meaning |
| --- | --- | --- |
| `MUNIMENT_UPDATE_GRACE_SECS` | 120 | How long a drain waits for running work. |
| `MUNIMENT_UPDATE_SHUTDOWN_SECS` | 60 | How long the server has to stop before it is killed. |
| `MUNIMENT_UPDATE_TRIAL_SECS` | 120 | How long a new version has to pass `health`. |
| `MUNIMENT_UPDATE_RETRY_SECS` | 60 | The first retry delay after a failed download. |
| `MUNIMENT_UPDATE_FEED` | `https://github.com/munimentai/muniment/releases/download` | The release feed base URL. `file://` reads a local directory. |
| `MUNIMENT_UPDATE_PUBLIC_KEY` | `RELEASE_PUBLIC_KEY` | The minisign public key releases verify against, for tests and mirrors that sign their own releases. |

## Development

Run these from `core/`:

```sh
cargo build --workspace
cargo test --workspace -- --test-threads=1
cargo test -p muniment-router --features server -- --test-threads=1
scripts/check-core-boundary.sh
```

The router's Postgres tests run when `MUNIMENT_ROUTER_TEST_DATABASE_URL` names a
database they may create schemas in. The OpenBao integration test runs when
`MUNIMENT_ROUTER_TEST_OPENBAO_ADDR` and `MUNIMENT_ROUTER_TEST_OPENBAO_TOKEN` are set.

`muniment-core` links `sherpa-onnx`. Its build script downloads the prebuilt
shared libraries on first build. `third-party/` holds the ONNX Runtime
libraries that the on-device tests load.

Pushes to `main` that touch `core/` cut a `core-v*` release. The conventional
prefixes of those commits pick the version: `feat:` is minor, `fix:` and
`perf:` are patch, `feat!:` or a `BREAKING CHANGE:` trailer is major, and other
prefixes cut no release. Commits outside `core/` never move the core version.

## License

FSL-1.1-ALv2. See `../LICENSE.md`.
