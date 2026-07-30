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

### Follow-up slices

This slice changes no runtime, protocol, dependency, or companion code.
Follow-up work will decide the disposition of the existing
`editor-extension/` tree under the owner ruling. It will not treat that tree
as the planned first-party extension.

Slice 2 will decide all of these items:

- The supported ACP v1 method subset.
- The answers for ACP filesystem and terminal client capabilities.
- The protocol version stance.
- The exact pinned Rust or TypeScript SDK version under ADR 0019.
- The editors Muniment claims to support.

This ADR does not decide those items. In particular, it does not select an
SDK, pin a version, claim an editor, or adopt the ACP v2 draft.

### Sources

- [ACP transport specification](https://agentclientprotocol.com/protocol/v1/transports)
- [ACP initialization specification](https://agentclientprotocol.com/protocol/v1/initialization)
- [ACP session specification](https://agentclientprotocol.com/protocol/v1/session-setup)
- [ACP tool-call and permission specification](https://agentclientprotocol.com/protocol/v1/tool-calls)
- [ACP filesystem specification](https://agentclientprotocol.com/protocol/v1/file-system)
- [ACP terminal specification](https://agentclientprotocol.com/protocol/v1/terminals)
- [ACP v2 draft announcement](https://agentclientprotocol.com/announcements/acp-v2-draft)

## Consequences

- One thin adapter speaks ACP, while one runtime service remains the executor.
- Every ACP session has a service-owned subject, workspace, thread, and run.
- Third-party editor answers remain inputs to Muniment gates, not grants.
- ACP and native sessions share authorization, journal, and receipt paths.
- ACP remains interop, while `muniment.attach/1` remains first-party.
- The existing threat model gains a new local client identity.
- The first-party editor extension stops as a product direction.
- The CLI remains deferred indefinitely.
- Slice 2 owns method, capability, version, SDK, and editor decisions.
- This decision adds no dependency or code.
