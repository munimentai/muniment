# Runtime threat model

This document describes the desktop runtime boundary as built in this
repository. It is an internal engineering record, not a public security claim.

## Trust boundary

The runtime mediates model access and local effects for one signed-in user.
Its gates are entitlements, ask/allow/deny decisions, capability grants,
receipts, and the owner kill switch. The local run journal records governed
actions and uncertain outcomes.

The boundary defends against remote network clients, other OS users,
unapproved local clients, stale capabilities, and client-claimed browser
identities. Deny wins during entitlement resolution. Client entitlement
snapshots provide display hints, while servers and gateways enforce access.

The boundary does not defend a host from its administrator, root, kernel
compromise, or a compromised process running as the current OS user. It does
not make model output trustworthy. It cannot prevent a user from approving a
deceptive request. Permission gates are the default, not process isolation.
Full-auto isolation remains platform-dependent.

## Roles by capabilities

Roles are organization-scoped. A role does not become trusted through a
username or a local loopback claim.

| Role | Capabilities | Gate controlled |
| --- | --- | --- |
| `user` | Uses granted chat, models, agent runs, artifacts, and capabilities. | Answers ask/allow/deny gates for the current run within owner policy. |
| `admin` | Manages people, groups, artifacts, capability review and publication, and workflow oversight. | Manages grants and capability approval within owner policy. |
| `owner` | Has admin access and manages routing, provider credentials, budgets, and organization security policy. | Controls sandbox policy, local-model policy, local stdio allowlists, and organization kill switches. |

Entitlements still constrain every role. Receipts record privileged decisions
and capability use. The kill switch blocks new actions and revokes affected
runtime access.

## Surface capability matrix

| Surface | Runtime execution | Permission gates | Receipts | Local capabilities | State in this repository |
| --- | --- | --- | --- | --- | --- |
| Desktop | Starts and steers runs | Answers locally | Shows governed run records | May invoke granted runtime capabilities | Built |
| CLI | Starts and steers runs through attach | Answers in the terminal | Prints run receipts | May invoke granted runtime capabilities | **Deferred indefinitely under the 2026-07-29 owner ruling ([ADR 0022](docs/decisions/0022-acp-agent-interop.md))** |
| Editor extension | Starts and steers runs through attach | Answers in the editor | Shows run receipts | May invoke granted runtime capabilities | **Retired under the 2026-07-29 owner ruling ([ADR 0022](docs/decisions/0022-acp-agent-interop.md))** |
| ACP adapter | Pairs over `muniment.attach/1`. Persists its identity and credential under `$XDG_CONFIG_HOME/muniment/`. Onboards the request `cwd` as its workspace. Starts runs. Streams released assistant text. Cancels the current run for a matching `session/cancel`. | Forwards a pending gate to the editor as `session/request_permission` and submits the mapped answer over attach. An editor answer grants nothing by itself. The runtime validates the matching gate and current authority. | Shows runtime-owned receipts | No claimed local filesystem or terminal capability | Built ([ADR 0022](docs/decisions/0022-acp-agent-interop.md)) |
| Mobile companion | No local runtime | Planned remote answers for live desktop runs | Planned display | None | **Not built** |
| Agent access | Planned server-side headless access | Not defined here | Server-owned, when designed | No local surface claim | **Not built** |

The desktop uses the governed runtime boundary. Mobile is a companion, not a
local execution surface. The 2026-07-29 owner ruling retires the editor
extension and defers the CLI indefinitely.
[ADR 0022](docs/decisions/0022-acp-agent-interop.md) records both dispositions.

## Loopback identities

No loopback assigns authority to a magic username.

| Loopback | Identity trusted | Why a client cannot mint it |
| --- | --- | --- |
| Attach socket | Linux checks `SO_PEERCRED` against the runtime effective UID. The socket must have that owner and mode `0600`. Pairing then binds approval, a client credential, and a capability to the connection and workspace scopes. | A client-chosen ID grants nothing. The runtime creates the random challenge, credential, and capability. The desktop supplies the approval. Other OS users fail the peer and filesystem checks. |
| Sign-in redirect catcher | The catcher trusts a callback on an ephemeral `127.0.0.1` port only when its `state` matches the current sign-in attempt. | A callback cannot choose the expected random state. A wrong state ends the attempt. The listener accepts no LAN address or durable account name. |
| Browser-control relay | The relay trusts a single-use random token plus the browser process and canonical executable identity derived through OS process and socket APIs. | The client cannot assert its executable path. The OS supplies the process identity, and the desktop supplies the expected path and token. Missing or contradictory evidence fails closed. |
| ADR 0012 runtime service | The accepted design trusts one per-user service managed by the OS and the same attach peer checks. The desktop still owns the runtime today. | No service identity exists to mint today. The design defines no privileged username or client-claimed role. Its service boundary remains unimplemented. |
| Editor-spawned ACP adapter ([ADR 0022](docs/decisions/0022-acp-agent-interop.md)) | ADR 0009 peer checks establish the same OS user. Attach approval binds the adapter identity, connection nonces, signed-in profile, and workspace scopes to a revocable capability. | The claimed process name, ACP session ID, and `cwd` grant no authority. Missing, contradictory, or stale identity evidence fails closed. |

These checks do not distinguish hostile processes after the current OS account
is compromised. They prevent names supplied inside the protocol from becoming
privileged identities.

## Prompt injection

Pi assembles model context for prompts sent through
`src-tauri/core/src/sidecar/pi_chat.rs`. The cloud classifies each prompt at
ingress. The deleted resident path in `src-tauri/core/src/llama.rs` no longer
defines a model-context boundary.

## Known gaps

- **Runtime service extraction remains open.** The desktop still owns the
  runtime. [ADR 0012](docs/decisions/0012-user-level-runtime-service.md)
  defines the target boundary.
- **Default permission gates are not isolation.** Windows has no claimed
  native full-auto sandbox.
  [ADR 0009](docs/decisions/0009-companion-attach-protocol.md) records
  same-user compromise as a deferred risk.
- **Mobile has no implementation here.**
  [ADR 0007](docs/decisions/0007-mobile-companion-repo-strategy.md) assigns it
  to a separate repository and delivery lane.
- **Agent access has no implementation here.**
  [ADR 0009](docs/decisions/0009-companion-attach-protocol.md) assigns
  automation to a server-side layer.
