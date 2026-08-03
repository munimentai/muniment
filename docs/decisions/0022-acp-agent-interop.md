# 0022 — ACP agent interoperability and session governance

- Status: accepted
- Date: 2026-07-29
- Context: owner ruling on ACP agent direction, ADRs 0009, 0011, 0012,
  and 0019, and `THREAT_MODEL.md`

## Context

The product owner ruled on 2026-07-29 that Muniment takes the Agent Client
Protocol agent direction. Any compatible ACP editor may drive the Muniment
runtime. The first-party editor extension will not be built, and the Muniment
CLI surface is deferred indefinitely.

ACP makes the editor the client. The client spawns the agent as a subprocess
and exchanges newline-delimited JSON-RPC 2.0 over standard input and output.
Each message is UTF-8 and contains no embedded newline. [ACP transport
specification](https://agentclientprotocol.com/protocol/v1/transports)

That process boundary cannot create another executor. ADR 0012 gives one
per-user runtime service exclusive ownership of runtime state, sessions, the
Pi child, effects, and the journal. ADRs 0009 and 0011 keep
`muniment.attach/1` as the first-party local client path. ACP adds an interop
adapter without replacing that path.

## Decision

### Component and process boundary

The **Muniment ACP adapter** is the only component that speaks ACP. An ACP
editor spawns one adapter process and communicates with it over ACP standard
input and output. The adapter is a thin `muniment.attach/1` client of the
ADR 0012 runtime service. It never opens the journal or credential store,
spawns Pi, executes a tool, or derives authoritative run state.

The adapter first discovers and attaches to the one per-user service under
ADR 0009. It may ask the platform service manager to start that service as
ADR 0012 permits. It never starts a private runtime or falls back to local
execution. Concurrent adapters remain clients of the same service. If no
authorized service attach exists, the ACP request fails closed.

This boundary composes with ADR 0011's one-way companion dependency rule. ACP
is an interoperability path and never the first-party path. Native Muniment
surfaces continue to use the first-party E0 attach contract from ADRs 0009 and
0011.

### Session identity and scope

The runtime service binds each ACP session to one Muniment identity tuple:

- The signed-in subject from the service-owned device session.
- The canonical workspace root derived from the ACP session `cwd`.
- The durable Muniment thread and current run that the session stamps.

ACP supplies `cwd` in `session/new` and `session/load`. Relative paths resolve
against that root. [ACP session
specification](https://agentclientprotocol.com/protocol/v1/session-setup)
The adapter treats `cwd` as a requested scope, not trusted identity. The
runtime canonicalizes it, rejects an absent or unauthorized root, and binds it
to the attach capability before it creates or loads a thread.

The service creates the ACP-session-to-thread mapping and stores it with the
authoritative journal state. A loaded session must resolve to the same signed-in
subject and workspace root. A sign-out, profile change, revoked attach
capability, mismatched `cwd`, unknown thread, or stale run binding fails closed.
The adapter cannot choose a subject or restamp an event onto another thread or
run.

### Grants, permission answers, and receipts

Every ACP operation enters the runtime through the same authorization and run
coordinate paths as a native operation. The service checks the current
subject, entitlement, workspace scope, capability grant, owner policy, and
kill switch before each effect. ACP client capabilities only state which
editor callbacks exist. They do not grant Muniment authority.

ACP lets an agent request permission from its client with
`session/request_permission`. The offered option kinds are `allow_once`,
`allow_always`, `reject_once`, and `reject_always`. The client returns a
selected option ID or `cancelled`. The published v1 permission contract
defines cancellation. It specifies no permission timeout. [ACP tool-call and
permission
specification](https://agentclientprotocol.com/protocol/v1/tool-calls)

An answer inside a third-party editor does **not**, by itself, satisfy a
Muniment grant check. The adapter may submit that answer only as an answer to
the matching pending Muniment gate. The runtime validates the request, gate,
subject, workspace, thread, run, operation, and current policy just as
`ExtensionUiResponse` and `chat_answer_permission` do for a native session.
An unknown option, cancelled answer, contradictory answer, duplicate answer,
expired gate, or stale run cannot authorize an effect.

The runtime decides whether an offered ACP choice maps to a valid Muniment
one-time or durable grant. The adapter never turns `allow_always` into a grant.
If current Muniment policy cannot represent or authorize the selected choice,
the runtime rejects it. A missing editor capability also cannot authorize a
fallback call. ACP forbids agent calls to client filesystem and terminal
methods when the matching client capability is absent. [ACP filesystem
specification](https://agentclientprotocol.com/protocol/v1/file-system) and
[ACP terminal
specification](https://agentclientprotocol.com/protocol/v1/terminals)

The run coordinate loop appends the same `permission.requested` and
`permission.resolved` events used by native sessions. Successful effects,
denials, cancellation, and uncertain outcomes follow the same journal and
receipt rules. Transport acknowledgements and editor state never become
receipts. Retries, reconnects, duplicate answers, and process loss cannot
repeat an effect or recover authority from adapter memory.

### Local identity and threat model

The editor-spawned adapter is a new local client identity under
`THREAT_MODEL.md`. Its claimed process name, ACP session ID, and `cwd` grant no
authority. ADR 0009 peer checks establish the same OS user, while attach
approval binds the adapter identity, connection nonces, signed-in profile, and
workspace scopes to a revocable capability.

The runtime must name the ACP adapter in approval and receipts. Missing,
contradictory, or stale identity evidence fails closed. This follows the threat
model rule that a client-chosen name or loopback location cannot mint
authority. The model still does not defend against a compromised process
running as the current OS user.

### ACP v1 method subset

The adapter implements this exact client-to-agent subset:

- `initialize`.
- `session/new` and `session/load`.
- `session/prompt`.
- The `session/cancel` notification.

It advertises `loadSession: true`. The 2026-08-03 permission-gate amendment
below supersedes that advertisement with the interim `loadSession: false`
stance until `session/load` works. The runtime creates, loads, prompts, and
cancels the bound Muniment thread and run. The adapter translates those
requests and returns the runtime result.

The adapter implements this exact agent-to-client subset:

- `session/request_permission`.
- The `session/update` notification.

The adapter uses `session/update` for text, tool-call, plan, and command
updates that the runtime can project without granting authority. It uses
`session/request_permission` only for the gate flow defined above.
ADR 0009's 2026-07-30 amendment defines the assistant text projection that the
adapter maps to ACP `agent_message_chunk` updates.

The adapter does not implement these remaining ACP v1 methods:

- `authenticate` and `logout`, because the runtime owns one shared device
  session and exposes no ACP authentication method.
- `session/list` and `session/delete`, because this slice exposes only the
  session named by `session/new` or `session/load`. Muniment remains the owner
  of thread discovery and deletion.
- `session/fork`, `session/resume`, and `session/close`, because the adapter uses
  `session/load` for a journal-backed restore. A client process exit does not
  own, copy, or close the service-owned session.
- `session/set_mode` and `session/set_config_option`, because Muniment has no
  ACP mode or configuration-option contract. The adapter advertises neither.
- `fs/read_text_file` and `fs/write_text_file`, because the runtime performs
  governed filesystem effects.
- `terminal/create`, `terminal/output`, `terminal/release`,
  `terminal/wait_for_exit`, and `terminal/kill`, because the runtime performs
  governed tool execution.
- `elicitation/create` and the `elicitation/complete` notification, because
  Muniment uses its permission-gate contract for user input and offers no ACP
  elicitation mode.

The adapter defines no custom ACP method. An unknown request gets the standard
JSON-RPC method-not-found error. An unknown notification has no effect.

### Client capabilities and effect ownership

The filesystem client-capability answer is no for both `fs.readTextFile` and
`fs.writeTextFile`. The terminal client-capability answer is no. The adapter
never calls these client methods, even when an editor advertises them. Because
the editor sends these flags in `initialize`, no means the adapter requires
neither capability and treats either advertised value the same way.

The runtime service reads and writes files through its governed filesystem
path. It also starts, observes, stops, and releases commands through its
governed tool path. The adapter projects resulting tool calls and updates to
the editor. An editor never performs a Muniment effect or supplies its
receipt.

### Protocol version and SDK pin

The adapter negotiates the integer `1` in `initialize`. ACP version negotiation
uses one integer for the wire protocol, independent of an SDK package version.
The stable protocol version is 1.

Muniment does not adopt the ACP v2 draft. The draft exists to make breaking
changes and may change incompatibly during development. A stable adapter needs
the v1 wire contract and capability negotiation instead.

The implementation will pin the Rust crate `agent-client-protocol` at exactly
`2.0.0`. That release comes from `agentclientprotocol/rust-sdk`, requires Rust
1.88.0, and implements the stable v1 protocol despite its crate major version.
The later implementation slice will add the exact manifest and lockfile pin.

ADR 0019 governs this external generated protocol artifact by composition.
The consuming manifest and lockfile pin one immutable version. A pin update
must review generated types and wire changes, run compatibility fixtures, and
ship as a deliberate dependency update. This repository does not edit the
SDK's generated schema or infer wire compatibility from its crate version.

### Claimed editors

The first release claims Zed and JetBrains IDEs. Zed documents custom external
agents as separate ACP processes configured with a command and arguments.
JetBrains documents custom ACP agents in `~/.jetbrains/acp.json`, including
their command and arguments. Both paths can launch the Muniment ACP adapter
without companion code.

These claims cover current generally available editor releases that expose
the cited custom-agent paths. Release validation must run the method subset
above in each editor before shipment. A limitation found during validation
blocks that editor claim rather than expanding the method subset silently.

An unlisted ACP editor gets standards-based best-effort interoperability. It
gets the same v1 negotiation, advertised subset, unsupported capabilities,
authorization, and failure behavior. Muniment makes no compatibility or
release-validation claim for that editor.

### Follow-up slices

This slice changes no runtime, protocol, dependency, or companion code.
Follow-up work will decide the disposition of the existing
`editor-extension/` tree under the owner ruling. It will not treat that tree
as the planned first-party extension.

### Sources

- [ACP transport specification](https://agentclientprotocol.com/protocol/v1/transports)
- [ACP initialization specification](https://agentclientprotocol.com/protocol/v1/initialization)
- [ACP session specification](https://agentclientprotocol.com/protocol/v1/session-setup)
- [ACP tool-call and permission specification](https://agentclientprotocol.com/protocol/v1/tool-calls)
- [ACP filesystem specification](https://agentclientprotocol.com/protocol/v1/file-system)
- [ACP terminal specification](https://agentclientprotocol.com/protocol/v1/terminals)
- [ACP v2 draft announcement](https://agentclientprotocol.com/announcements/acp-v2-draft)
- [ACP v1 method overview](https://agentclientprotocol.com/protocol/v1/overview)
- [ACP v1 authentication specification](https://agentclientprotocol.com/protocol/v1/authentication)
- [ACP v1 session-list specification](https://agentclientprotocol.com/protocol/v1/session-list)
- [ACP v1 session-delete specification](https://agentclientprotocol.com/protocol/v1/session-delete)
- [ACP v1 session-resume announcement](https://agentclientprotocol.com/announcements/session-resume-stabilized)
- [ACP v1 session-close announcement](https://agentclientprotocol.com/updates)
- [ACP v1 session-mode specification](https://agentclientprotocol.com/protocol/v1/session-modes)
- [ACP v1 session-config-option specification](https://agentclientprotocol.com/protocol/v1/session-config-options)
- [ACP v1 elicitation specification](https://agentclientprotocol.com/protocol/v1/elicitation)
- [`agent-client-protocol` 2.0.0 release entry](https://docs.rs/crate/agent-client-protocol/2.0.0)
- [`agent-client-protocol` 2.0.0 source and Rust requirement](https://github.com/agentclientprotocol/rust-sdk/blob/v2.0.0/Cargo.toml)
- [ACP Rust SDK repository](https://github.com/agentclientprotocol/rust-sdk)
- [Zed external-agent documentation](https://zed.dev/docs/ai/external-agents)
- [JetBrains ACP documentation](https://www.jetbrains.com/help/ai-assistant/acp.html)

## Consequences

- One thin adapter speaks ACP, while one runtime service remains the executor.
- Every ACP session has a service-owned subject, workspace, thread, and run.
- Third-party editor answers remain inputs to Muniment gates, not grants.
- ACP and native sessions share authorization, journal, and receipt paths.
- ACP remains interop, while `muniment.attach/1` remains first-party.
- The existing threat model gains a new local client identity.
- The first-party editor extension stops as a product direction.
- The CLI remains deferred indefinitely.
- The adapter implements a bounded ACP v1 subset and delegates every effect.
- Filesystem and terminal client capabilities remain unused.
- The implementation will pin `agent-client-protocol` 2.0.0 under ADR 0019.
- The release claims Zed and JetBrains IDEs after release validation.
- This decision adds no dependency or code.

## Amendment — 2026-08-03: first `session/prompt` translation

The adapter maps `run.completed` to a successful `session/prompt` response
with `StopReason::EndTurn`. It maps `run.cancelled` to a successful response
with `StopReason::Cancelled`. These are the pinned SDK's v1
[`end_turn` and `cancelled` stop-reason values][sdk-stop-reason]. The adapter
maps `run.failed` to an ACP `InternalError` response and includes only a
redacted failure message. The SDK defines no failed stop reason. The rejected
alternative was `StopReason::Refusal`, whose SDK meaning excludes the prompt
from future context and misrepresents a runtime failure.

Until the permission-gate bridge exists, the adapter answers every
`PermissionPending` event with `deny` through the matching attach
`permission.answer` operation. It acknowledges the event only after the
runtime accepts that denial. This answer grants no one-time or durable
authority under this ADR's gate rules. The rejected alternative was to wait
for an editor answer, because no bridge could deliver or validate that answer.

The adapter keeps a process-local table from each ACP session ID to its
canonical paired workspace. `session/new` inserts the mapping only after the
runtime authorizes and pairs that workspace. Each later session call must find
the ID and use the same workspace. An unknown ID returns the SDK's ACP
`ResourceNotFound` error and starts no run. The rejected alternative was to
derive scope from each request, because an ACP session ID and client-supplied
`cwd` grant no authority.

The first slice does not translate `session/cancel` to attach `run.cancel`.
The adapter ignores that notification, as ACP requires for an unsupported
notification. Ignoring it does not cancel or detach the active prompt. The
prompt keeps streaming until the runtime emits a terminal event. The rejected
alternative was a partial cancellation bridge without the run binding and
race handling needed to target one current run.

For each streamed event with a `session/update` projection, the adapter sends
that notification before it acknowledges the event with `run.cursor_ack`. A
send failure leaves the event unacknowledged for replay. This follows ADR
0009's rule that a consumer persists its cursor only after it processes the
event. The rejected alternative was to acknowledge first, which could lose an
update if the ACP connection failed between those actions.

The first implementation slice is **ACP prompt translation**. It adds the
process-local session table, starts a run for a bound prompt, translates its
updates and terminal result, denies pending permissions, and acknowledges
each event in the order above. It does not add cancellation or the permission
gate bridge. This amendment changes no code.

[sdk-stop-reason]: https://docs.rs/agent-client-protocol/2.0.0/agent_client_protocol/schema/v1/enum.StopReason.html

## Amendment — 2026-08-03: `session/request_permission` gate translation

For each attach `permission.pending` event, the adapter sends one ACP
`session/request_permission` request. Its tool call uses `gate_id` as the tool
call ID and `title` as the title. When `message` is present, the content
contains one text block with that value. When `message` is absent, the content
is empty.

The request offers only `allow_once` and `reject_once`. The adapter does not
offer `allow_always` or `reject_always`, because ADR 0025's durable-policy
consumers remain parked on the Pi resource contract. The runtime cannot map a
durable choice without that contract.

The selected `allow_once` option maps to the attach `allow` decision. The
selected `reject_once` option maps to `deny`. A `cancelled` outcome, an unknown
option ID, or a malformed response also maps to `deny`. No editor outcome
creates a grant by itself. The runtime still validates the matching gate and
current authority under this ADR.

After it gets an editor outcome, the adapter submits the mapped decision
through the matching attach `permission.answer` operation. It acknowledges
the `permission.pending` event only after the attach answer is accepted. An
attach failure leaves the event unacknowledged for replay.

The adapter advertises `loadSession: false` until `session/load` works. This
interim stance supersedes the `loadSession: true` statement above.

The implementation slice is **ACP permission gate translation**. It adds the
request projection, one-time option mapping, deny-closed response handling,
and acknowledge order defined above. This amendment changes no code.
