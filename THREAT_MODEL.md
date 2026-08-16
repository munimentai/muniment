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
| ACP adapter | The Linux desktop package installs the adapter at `/usr/lib/muniment/muniment-acp`. Installation grants no authority. An editor must spawn the binary, and the desktop must approve pairing before authority exists. Pairs over `muniment.attach/1`. Persists its identity and credential under `$XDG_CONFIG_HOME/muniment/`. Persists versioned session records under `$XDG_CONFIG_HOME/muniment/acp-sessions/` as zero-authority locators. Treats the request `cwd` only as a local execution root that the attach owner canonicalizes and records under the signed workspace and client identity. The `cwd` grants no scope. Starts runs. Streams released assistant text. Forwards each tool effect's ID, released display name, and outcome from the authorized run stream. These fields are runtime-owned journal content under the [ADR 0009 tool-effect disclosure amendment](docs/decisions/0009-companion-attach-protocol.md#amendment--2026-08-03-tool-effect-disclosure). The editor performs no Muniment effect. Cancels the current run for a matching `session/cancel`. Provides record-checked `session/load` through `thread.open` replay and advertises `loadSession: true`. Each load re-runs the authorized attach checks. A copied or edited record cannot select a subject, expand workspace scope, open an unauthorized thread, or revive a revoked capability. | Forwards a pending gate to the editor as `session/request_permission` and submits the mapped answer over attach. An editor answer grants nothing by itself. The runtime validates the matching gate and current authority. | Shows runtime-owned receipts | No claimed local filesystem or terminal capability | Built ([ADR 0022](docs/decisions/0022-acp-agent-interop.md)) |
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
| Attach socket | Linux checks `SO_PEERCRED` against the runtime effective UID. The socket must have that owner and mode `0600`. Pairing then binds approval, a client credential, and a capability to the connection and workspace scopes. The credential store records the credential, claimed kind, claimed version, and approval time. The two claims carry no authority. Under the [ADR 0009 workspace-namespace amendment](docs/decisions/0009-companion-attach-protocol.md#amendment--2026-08-04-attach-workspace-namespace), the signed `grant.workspace` value is the only workspace authority for `thread.list`, `thread.open`, `thread.create`, and `run.start`. A companion-supplied directory is only a local execution root. The attach owner canonicalizes and records it under the signed workspace and client identity. The directory grants no scope. With no current cloud grant, approval and all four operations fail closed. Under the [ADR 0009 revocation amendment](docs/decisions/0009-companion-attach-protocol.md#amendment--2026-08-04-companion-revocation), revocation removes the persisted client credential before it invalidates the live connection capabilities. It emits one `capability.revoked` event per affected connection. The event discloses no companion identity, credential, or workspace value. A revoked companion regains authority only through a fresh visible approval. | A client-chosen ID grants nothing. The runtime creates the random challenge, credential, and capability. The desktop supplies the approval. Other OS users fail the peer and filesystem checks. |
| Sign-in redirect catcher | The catcher trusts a callback on an ephemeral `127.0.0.1` port only when its `state` matches the current sign-in attempt. | A callback cannot choose the expected random state. A wrong state ends the attempt. The listener accepts no LAN address or durable account name. |
| Browser-control relay | The relay trusts a single-use random token plus the browser process and canonical executable identity derived through OS process and socket APIs. | The client cannot assert its executable path. The OS supplies the process identity, and the desktop supplies the expected path and token. Missing or contradictory evidence fails closed. |
| ADR 0012 runtime service | The accepted design trusts one per-user service managed by the OS and the same attach peer checks. Only the waiting runtime service may send `migration.control`. On Linux, the desktop resolves the `SO_PEERCRED` peer PID to an executable path and requires the installed `muniment-runtime` payload. That peer check admits a connection-bound session without visible approval or a stored companion credential. The session authorizes only `migration.control`. It grants no workspace, thread, run, or other companion authority. In the opposite direction, the runtime routes a connection from the installed desktop payload by its first hello. A `desktop-client` hello takes a connection-bound desktop client session. The peer check is its only admission authority. Neither visible approval nor a stored companion credential applies. The session carries owner authority over the signed `grant.workspace` value, including its workspace, thread, run, permission, and subscription authority. It may send every companion operation that grant authorizes. It may also send `thread.rename`, `thread.delete`, `session.status`, `entitlement.snapshot`, `device.list`, `session.sign_out`, `companion.list`, `companion.revoke`, `thread.summaries`, `thread.history`, and `run.chat_events`. The event subscription uses a second desktop client connection and carries unredacted `ChatEvent` values. The two thread operations and the two write operations in the second tranche require an idempotency key. The four read operations in the second tranche, both third-tranche operations, and `run.chat_events` require none. Companion sessions receive `unauthorized` for desktop-only operations. The desktop client session still receives `unauthorized` for `migration.control` and `approval.present`. Browser sign-in and `home.ensure` remain outside the first two tranches. The run surface, session-thread tracker commands, and `home.ensure` remain outside the third tranche. The five run commands remain outside the fourth tranche. It carries neither migration control nor approval presentation. A `desktop` hello takes the presenter session. A `desktop-handoff-probe` hello keeps the readiness answer. Every other connection takes the companion route. The routing check consumes no byte because the admitted path reads the first hello. An unreadable, oversized, or late first frame takes the companion route and still requires visible pairing approval. The runtime admits at most one connection-bound presenter session. That session carries only `approval.present` over `muniment.attach/1` and ends with the connection. The desktop starts its presenter supervisor after a confirmed handoff or at launch when another process holds the per-profile instance lock. It dials no presenter connection while its listener owns the endpoint. The supervisor reconnects until a restarted desktop listener or desktop shutdown stops it. A dropped connection cancels no earlier approval. The presenting desktop uses the same visible pairing prompt as its local listener. The desktop checks runtime activity and prepares at most one handoff at a time. After the success answer, the desktop releases its listener and instance lock, then probes the runtime service with the prepared nonce. A confirmed probe leaves the runtime service as owner. A failed probe cancels the preparation and restarts the desktop listener. The landed ADR 0009 per-profile instance lock prevents another listener from binding the profile endpoint. The shipped `muniment-runtime` binary takes the instance lock and waits for a termination signal. Its dormant service entries open the journal and CAS and drive Pi only when a caller invokes them. No production path reaches them before the cutover. | An unresolved or mismatched peer fails closed. An approved client credential and a claimed kind each grant no migration control authority. A missing presenter, disconnect, denial, or expired decision fails approval closed. The checks do not defend against compromise by another process running as the current OS user. Windows and macOS still need their own peer-identity amendments. The [ADR 0012 fourth desktop client operation tranche amendment](docs/decisions/0012-user-level-runtime-service.md#amendment--2026-08-15-fourth-desktop-client-operation-tranche) records the event subscription. The [ADR 0012 third desktop client operation tranche amendment](docs/decisions/0012-user-level-runtime-service.md#amendment--2026-08-15-third-desktop-client-operation-tranche) records the third tranche. The [ADR 0012 second desktop client operation tranche amendment](docs/decisions/0012-user-level-runtime-service.md#amendment--2026-08-14-second-desktop-client-operation-tranche) records the second tranche. The [ADR 0012 desktop client operation surface amendment](docs/decisions/0012-user-level-runtime-service.md#amendment--2026-08-14-desktop-client-operation-surface) records the first tranche. The [ADR 0012 desktop client lifecycle amendment](docs/decisions/0012-user-level-runtime-service.md#amendment--2026-08-14-desktop-client-session-lifecycle) records when the desktop connects. The [ADR 0012 desktop client session amendment](docs/decisions/0012-user-level-runtime-service.md#amendment--2026-08-14-desktop-client-session-admission) records desktop client admission and authority. The [ADR 0012 presenter lifecycle amendment](docs/decisions/0012-user-level-runtime-service.md#amendment--2026-08-14-desktop-approval-presenter-lifecycle) records when the desktop presents. The [ADR 0012 routing amendment](docs/decisions/0012-user-level-runtime-service.md#amendment--2026-08-13-runtime-attach-connection-routing) records connection routing. The [ADR 0012 approval presentation amendment](docs/decisions/0012-user-level-runtime-service.md#amendment--2026-08-13-approval-presentation-session) records presenter admission and authority. The [ADR 0012 migration session amendment](docs/decisions/0012-user-level-runtime-service.md#amendment--2026-08-10-migration-control-session-admission) records admission. The [ADR 0012 migration control authority amendment](docs/decisions/0012-user-level-runtime-service.md#amendment--2026-08-05-migration-control-request-authority) records request authority. The [ADR 0012 extraction amendment](docs/decisions/0012-user-level-runtime-service.md#amendment--2026-08-04-runtime-service-extraction-sequence) records the staged handoff. |
| Editor-spawned ACP adapter ([ADR 0022](docs/decisions/0022-acp-agent-interop.md)) | ADR 0009 peer checks establish the same OS user. Attach approval binds the adapter identity, connection nonces, signed-in profile, and workspace scopes to a revocable capability. | The claimed process name, ACP session ID, and `cwd` grant no authority. Missing, contradictory, or stale identity evidence fails closed. |

The [ADR 0012 fifth desktop client operation tranche amendment](docs/decisions/0012-user-level-runtime-service.md#amendment--2026-08-16-fifth-desktop-client-operation-tranche)
adds `run.submit`, `run.resume`, `run.steer`, `run.follow_up`, and
`run.permission_answer` to the desktop client session. It reuses `run.cancel`.
The peer check and signed workspace grant remain the authority boundaries.
Companion sessions receive `unauthorized` for the five desktop-only operations.
The desktop converts all five run commands in one later slice.

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
