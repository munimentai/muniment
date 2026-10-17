# Runtime threat model

This document describes the desktop runtime boundary and the headless hosts
built from `core/` in this repository. It is an internal engineering record, not
a public security claim.

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
| CLI | Starts and steers runs through attach | Answers in the terminal | Prints run receipts | May invoke granted runtime capabilities | **Deferred indefinitely** |
| Editor extension | Starts and steers runs through attach | Answers in the editor | Shows run receipts | May invoke granted runtime capabilities | **Retired** |
| ACP adapter | The Linux desktop package installs the adapter at `/usr/lib/muniment/muniment-acp`. Installation grants no authority. An editor must spawn the binary, and the desktop must approve pairing before authority exists. Pairs over `muniment.attach/1`. Persists its identity and credential under `$XDG_CONFIG_HOME/muniment/`. Persists versioned session records under `$XDG_CONFIG_HOME/muniment/acp-sessions/` as zero-authority locators. Treats the request `cwd` only as a local execution root that the attach owner canonicalizes and records under the signed workspace and client identity. The `cwd` grants no scope. Starts runs. Streams released assistant text. Forwards each tool effect's ID, released display name, and outcome from the authorized run stream. These fields are runtime-owned journal content. The editor performs no Muniment effect. Cancels the current run for a matching `session/cancel`. Provides record-checked `session/load` through `thread.open` replay and advertises `loadSession: true`. Each load re-runs the authorized attach checks. A copied or edited record cannot select a subject, expand workspace scope, open an unauthorized thread, or revive a revoked capability. | Forwards a pending gate to the editor as `session/request_permission` and submits the mapped answer over attach. An editor answer grants nothing by itself. The runtime validates the matching gate and current authority. | Shows runtime-owned receipts | No claimed local filesystem or terminal capability | Built |
| Mobile companion | No local runtime | Planned remote answers for live desktop runs | Planned display | None | **Not built** |
| Agent access | Planned server-side headless access | Not defined here | Server-owned, when designed | No local surface claim | **Not built** |

The desktop uses the governed runtime boundary. Mobile is a companion, not a
local execution surface. The editor extension is retired, and the CLI is
deferred indefinitely.

## Loopback identities

No loopback assigns authority to a magic username.

| Loopback | Identity trusted | Why a client cannot mint it |
| --- | --- | --- |
| Attach socket | Linux checks `SO_PEERCRED` against the runtime effective UID. The socket must have that owner and mode `0600`. Pairing then binds approval, a client credential, and a capability to the connection and workspace scopes. The credential store records the credential, claimed kind, claimed version, and approval time. The two claims carry no authority. The signed `grant.workspace` value is the only workspace authority for `thread.list`, `thread.open`, `thread.create`, and `run.start`. A companion-supplied directory is only a local execution root. The attach owner canonicalizes and records it under the signed workspace and client identity. The directory grants no scope. With no current cloud grant, approval and all four operations fail closed. Revocation removes the persisted client credential before it invalidates the live connection capabilities. It emits one `capability.revoked` event per affected connection. The event discloses no companion identity, credential, or workspace value. A revoked companion regains authority only through a fresh visible approval. | A client-chosen ID grants nothing. The runtime creates the random challenge, credential, and capability. The desktop supplies the approval. Other OS users fail the peer and filesystem checks. |
| Sign-in redirect catcher | The catcher trusts a callback on an ephemeral `127.0.0.1` port only when its `state` matches the current sign-in attempt. | A callback cannot choose the expected random state. A wrong state ends the attempt. The listener accepts no LAN address or durable account name. |
| Browser-control relay | The relay trusts a single-use random token plus the browser process and canonical executable identity derived through OS process and socket APIs. | The client cannot assert its executable path. The OS supplies the process identity, and the desktop supplies the expected path and token. Missing or contradictory evidence fails closed. |
| Runtime service | The design trusts one per-user service managed by the OS and the same attach peer checks. The desktop serves no attach endpoint, so it admits no `migration.control` session. The runtime routes a connection from the installed desktop payload by its first hello. A `desktop-client` hello takes a connection-bound desktop client session. The peer check is its only admission authority. Neither visible approval nor a stored companion credential applies. The runtime admits this session while it holds no signed workspace. Its capability carries no workspace scope until the first resolved run grant records the signed workspace. The session then carries owner authority over the signed `grant.workspace` value, including its workspace, thread, run, permission, and subscription authority. It may send every companion operation that grant authorizes. It may also send `thread.rename`, `thread.delete`, `session.status`, `entitlement.snapshot`, `device.list`, `session.sign_out`, `companion.list`, `companion.revoke`, `thread.summaries`, `thread.history`, `run.chat_events`, and `thread.select`. The event subscription uses a second desktop client connection and carries unredacted `ChatEvent` values. It delivers every run in the workspace the runtime recorded, including a run another client started. The broadcast reads that recorded workspace at delivery and sends nothing while the runtime holds none. The two thread operations and the two write operations in the second tranche require an idempotency key. The four read operations in the second tranche, both third-tranche operations, and `run.chat_events` require none. Companion sessions receive `unauthorized` for desktop-only operations. The desktop client session still receives `unauthorized` for `migration.control` and `approval.present`. Browser sign-in and `home.ensure` remain outside the first two tranches. The run surface, session-thread tracker commands, and `home.ensure` remain outside the third tranche. The five run commands remain outside the fourth tranche. It carries neither migration control nor approval presentation. A `desktop` hello takes the presenter session. A `desktop-handoff-probe` hello keeps the readiness answer. Every other connection takes the companion route. The routing check consumes no byte because the admitted path reads the first hello. An unreadable, oversized, or late first frame takes the companion route and still requires visible pairing approval. The runtime admits at most one connection-bound presenter session. That session carries only `approval.present` over `muniment.attach/1` and ends with the connection. The desktop starts its presenter supervisor at launch. The supervisor reconnects until desktop shutdown stops it. A dropped connection cancels no earlier approval. The presenting desktop shows the visible pairing prompt. The desktop holds no listener or instance lock, so it prepares no handoff. The per-profile instance lock prevents another listener from binding the profile endpoint. The shipped `muniment-runtime` binary takes the instance lock and waits for a termination signal. Its dormant service entries open the journal and CAS and drive Pi only when a caller invokes them. No production path reaches them before the cutover. | An unresolved or mismatched peer fails closed. An approved client credential and a claimed kind each grant no migration control authority. A missing presenter, disconnect, denial, or expired decision fails approval closed. The Windows section below defines the Windows peer check. A verified Windows desktop-image peer takes the route selected by its first Hello. A `desktop` kind takes the approval-presenter route, a `desktop-client` kind takes the desktop-client route, and every other kind takes the companion route. The claim selects a route only after the desktop-image check and grants no authority. The presenter route serves `approval.present`. The companion route serves pairing through the live approval waiter and shared connection registry. The same-user check does not defend against a compromised process running as the current OS user. The macOS section below defines that platform's peer identity. |
| Editor-spawned ACP adapter | The attach peer checks establish the same OS user. Attach approval binds the adapter identity, connection nonces, signed-in profile, and workspace scopes to a revocable capability. | The claimed process name, ACP session ID, and `cwd` grant no authority. Missing, contradictory, or stale identity evidence fails closed. |

The desktop client session also carries `run.submit`, `run.resume`, `run.steer`, `run.follow_up`, and
`run.permission_answer`. It reuses `run.cancel`.
The peer check and signed workspace grant remain the authority boundaries.
Companion sessions receive `unauthorized` for the five desktop-only operations.

The desktop client session also carries the desktop-only `thread.select` operation. It makes the desktop tracker
the only thread selection authority for desktop prompts. A null `thread_id`
starts a fresh thread on both service halves. The runtime validates ownership
through `subject_owns_first_run`. A companion session receives `unauthorized`.
The operation appends nothing, so it requires no idempotency key.
The desktop converts all five run commands in one later slice.

The installed-payload refresh makes the runtime the only process that may request its upgrade exit. The
runtime drains new quiesce-blocking work, then waits for `evaluate_quiesce`.
Its distinct failure exit triggers the shipped `Restart=on-failure` policy.
An explicit service stop leaves the service stopped. The owner-only endpoint,
process signaling rules, and per-user service manager prevent another OS user
from triggering the refresh. Package replacement grants no runtime authority.

Passive chat-event delivery scopes the chat-event broadcast by the workspace the runtime recorded. A
subscriber receives every run in that workspace, including a run another
client started. The broadcast reads the recorded workspace at delivery, so a
connection admitted before the first run grant needs no resubscribe.
`run.chat_events` stays desktop-only, so a companion session still receives
`unauthorized` for the unredacted `ChatEvent` value. The event body gains only
a thread id, which adds no operation and no wire version.

These checks do not distinguish hostile processes after the current OS account
is compromised. They prevent names supplied inside the protocol from becoming
privileged identities.

On macOS, both attach peers call `getpeereid` before they exchange any protocol
frame. The kernel-supplied effective UID must match the process effective UID.
A failed call, changed result, or mismatch closes the socket without a protocol
response. Client kinds, PIDs, paths, and UID fields supplied by a client grant
no authority. Owner-only endpoint modes, pairing, capabilities, and signed
workspace grants still apply after this check. This boundary rejects another
OS user, but it does not distinguish hostile processes under the same account.

The macOS user-domain LaunchAgent runs the bundled, signed runtime from
`/Applications/muniment.app/Contents/Library/LaunchServices/muniment-runtime`.
Each user must run the app once to request `SMAppService` registration. Denial
or missing approval prevents activation and grants no alternate runtime path.
The app directs that user to System Settings > General > Login Items. The
LaunchAgent sends standard streams to `/dev/null`. The runtime resolves the
effective user's home through the OS user record. It writes redacted, bounded
diagnostics to the resulting absolute
`/Users/<user>/Library/Logs/Muniment/runtime.log` path and the
`ai.muniment.desktop` unified log subsystem. Diagnostics contain no secrets,
content, local paths, workspace values, connection nonces, or peer identifiers.

The served macOS attach endpoint is `<profile>/muniment/attach-v1.sock`. The
listener creates the parent `muniment` directory with mode `0700`, holds an
open descriptor for it, and rechecks that descriptor at every step. It rejects
a symlink, a non-directory, and a parent the effective user does not own. It
probes the path before it binds, fails as contended when a live listener
answers, and removes only an owner-matching stale socket. After the bind it
sets mode `0600`, then reads the owner, mode, and inode identity back. The
bound socket is the only exclusion, because this path takes no separate
instance lock.

The runtime resolves the expected desktop executable from the installed
runtime payload layout at bind time, and a failed resolution fails the bind.
The accept path runs the `getpeereid` check above before it names a route.
The runtime then reads the kernel-supplied peer process ID through
`LOCAL_PEERPID` and reads that process image path through `proc_pidpath`. A
peer whose absolute path differs from the installed desktop payload takes the
companion route without a frame read. A failed process ID or image path read
also takes that route. The runtime also reads the peer audit token through
`LOCAL_PEERTOKEN`, which the kernel records at connect. A desktop-image peer
takes a desktop route only when the process that token names satisfies the
designated code requirement of the installed desktop payload, checked through
`SecCodeCopyGuestWithAttributes`. The token carries the process ID version,
which changes on exec, so a process that connects and then execs the desktop
binary takes the companion route.

For a verified desktop-image peer, the runtime refines the route from the first
Hello frame. A `desktop` kind takes the approval-presenter route. A
`desktop-client` kind takes the desktop-client route. Every other kind takes
the companion route. The claimed kind selects a route only for that verified
peer. It grants no other authority.

The runtime opens one `RuntimeAttachState` per activation and composes one
`DesktopAttachService` for each connection. The approval-presenter route runs
the approval-presentation session. The desktop-client route writes
`reconnect_welcome`, then `DesktopClientAuthorizedGrant`, and serves the
desktop client session. That session carries no workspace scope until the
first resolved run grant records the signed workspace.

The companion route runs the platform-neutral pairing and authorization
exchange with the live approval waiter and shared connection registry. A new
pairing writes a `welcome` challenge and asks the user through the visible
approval-presentation session. Approval produces an authorized grant and a
registered connection that serves companion requests. A valid reconnect uses
`reconnect_welcome` and its stored client credential. With no signed workspace,
the approval waiter denies the request and pairing fails closed. Each stored
credential records the subject that approved it: the signed-in organization and
user, or local mode. A reconnect under another subject, or with a credential
that records no subject, requires a fresh visible approval. Sign-out revokes
every live companion connection.

The OS-supplied path proves which installed path served the connection at the
check. It does not prove binary integrity or defend against compromise under
the same account. A claimed Hello kind carries no authority beyond route
selection for a verified desktop-image peer.

On Windows, runtime activation registers one per-user Scheduled Task named `\Muniment\Runtime-{user-sid}`.
The task principal uses the user's canonical SID, an interactive token, and
least privilege. Its action runs the signed payload directly, with no arguments
or shell command. It uses either
`%ProgramFiles%\muniment\muniment-runtime.exe` for the machine installation or
`%LocalAppData%\muniment\muniment-runtime.exe` for the per-user installation.
A Windows runtime start with invalid arguments exits before it opens the
instance lock, journal, CAS, Pi, or attach endpoint. The served Windows attach
endpoint is `\\.\pipe\Muniment\attach-v1-<user-hash>-<suffix>`. The runtime
generates the random suffix once and stores the full pipe path in an owner-only
file, `%LocalAppData%\ai.muniment.desktop\attach-pipe-name`. The desktop and
every client read the path from that file, so another OS user cannot create the
pipe first. The listener holds the per-profile instance lock and reads back
each pipe instance's owner and owner-only DACL.
For each connection, it reads the four-byte length prefix and verifies the peer
through impersonation. It then gets the peer process ID with
`GetNamedPipeClientProcessId` and gets that process's image path with
`QueryFullProcessImageNameW` before it reads the frame body. A peer whose image
path differs from the installed desktop executable takes the companion route.
A failed process ID, image path, or expected desktop path read also takes that
route.

The runtime refines the route from the verified desktop-image peer's first
Hello frame. A `desktop` kind takes the approval-presenter route. A
`desktop-client` kind takes the desktop-client route. Every other kind takes
the companion route. The claimed kind selects a route only for that verified
peer. It grants no other authority.

The approval-presenter route runs the approval-presentation session.
The desktop-client route writes `reconnect_welcome`, then
`DesktopClientAuthorizedGrant`, and serves the desktop client session.
The companion route runs the platform-neutral pairing and authorization
exchange with the live approval waiter and shared connection registry. A new
pairing writes a `welcome` challenge and requires visible approval. A valid
reconnect uses `reconnect_welcome` and its stored client credential. A missing
presenter, denial, expired decision, invalid credential, or missing signed
workspace fails pairing closed.

The OS-supplied path proves which installed path served the connection at the
check. It does not prove binary integrity or defend against compromise under
the same account. A claimed Hello kind carries no authority beyond route
selection for a verified desktop-image peer. The runtime writes owner-only,
redacted, bounded diagnostics to
`%LocalAppData%\ai.muniment.desktop\logs\runtime.log`. These records omit tokens,
credentials, prompts, model output, local paths, workspace values, connection
nonces, and peer identifiers.

## Headless hosts

The core ships two network servers and an updater for hosts without the desktop.
`core/README.md` describes their settings. Each one trusts its operator, who
holds the host, its state directory and its environment.

### Runtime server and remote attach

`muniment-server` runs the runtime with no keychain and no cloud account. It
keeps secrets in owner-only files under the state root, or reads them from
`MUNIMENT_SECRET_*` variables. Anyone who can read the state directory or the
process environment holds every secret and can pair a device, so the state
volume is the trust boundary of a container.

The remote route serves `muniment.attach/1` over TLS 1.3 only. A client pins the
runtime certificate by its SHA-256 from the pairing link. A pairing link carries
a 128-bit secret in its fragment, works once, expires after ten minutes, and the
runtime stores only its hash. Redeeming it issues a device credential with a
256-bit secret, also stored only as a hash. Each device holds the scopes its link
granted and can delegate only those. Revocation closes the device's live sessions.
The listener refuses a public peer address before the handshake unless
`allow_public` is set. It caps connections, sets handshake and first-frame
deadlines, limits the first frame to 16 KiB, and holds back a peer address after
repeated refusals. The remote route refuses the local-only operations and any
request that names a file on the runtime's machine.

The route does not defend against a stolen device credential before its
revocation, or against an operator who publishes the listener on a public
address with `allow_public` set.

### Fleet keys and sealed credentials

Linked runtimes share model connections, MCP tokens and model vault tokens
as sealed secrets. Each member keeps an X25519 fleet key in its credential
store. Its member record carries a card with the public key and its
certificate digest, signed with its P-256 identity key, the key behind its
pinned TLS certificate.

A signed roster decides who receives secrets. `swarm.invite {role}` records a
`full` or `worker` role for the invitation. The joiner presents the hash of
the invitation secret it redeemed, over the device that redemption created.
The inviter then signs an entry with the joiner's card and that role. The
joiner signs an entry for the inviter whose certificate it pinned. A member
trusts itself and each member that a trusted full member's entry names.
Entries signed by a worker, by an unknown key, or with a changed body count
for nothing. A forged member record, or a replay of another member's card
from a different device, therefore receives no sealed secret.
`swarm.leave` and the revocation of a linked device append a signed removal.
A removal also voids every entry the removed member signed, and the remover
first signs fresh entries for members that only the removed one vouched for.

An owner seals each version of a secret to each full member's fleet key. The
scheme derives a key per recipient with X25519 from a fresh ephemeral key,
then HKDF-SHA256, and encrypts with AES-256-GCM. Each copy authenticates a
header that names the secret, its version, its owner and its recipient. The
owner signs the whole secret. Members keep the newest signed version. A
member sends sealed secrets only to a full member over a channel that proves
that member: its own pinned outbound link, or the device the member was
admitted through. A relay holds copies it cannot open. Secrets never enter
the journal or the replicated records. A worker receives only the secrets
sealed to it: leases of the connections its own agents name, without refresh
tokens.

Every member is full unless an invitation or `swarm.set_role` makes it a
worker. One module filters every exchange in both directions. A worker holds
the content of the agents it runs and the fleet's metadata, nothing of other
threads, and no audit record. It relays nothing: members take from it only
what it wrote about its own agents, and its forwarded actions lose
`agents.manage` and `runtime.admin`. A device whose channel does not prove a
roster place, such as one that replays a worker's card, gets the roster
alone. A demoted member drops what a worker may not hold, and owners reseal
without it. The append-only `audit` class records seals, forwarded actions,
role changes, removals and snapshots on every full member; a member cannot
rewrite another's entries. A worker that turns hostile keeps what it already
held.

A sign-in whose provider rotates refresh tokens has one refresher. Without
the model vault, that is the machine that signed in. It reseals a fresh lease
without the refresh token before the lease margin. `fleet.hold_sign_in` moves
the refresher: the holder removes the sign-in from its own store under the
refresh lock, seals it to the new holder alone, and stops refreshing it. With
the model vault, a desktop with its own account session issues a machine
token only to a full member that uses connections the vault manages. Swarm
membership and sealed secrets have no member limit. A removal makes every member reseal without the
removed member, drop what it shared, and revoke the vault token issued for
it. Each member logs the connections the removed member held, so the owner
can rotate them at the provider.

These measures do not take back what a removed member already opened: it
keeps the leases and keys it held until they expire or the owner rotates
them. A member that holds a valid device credential and the matching fleet
key can read every secret sealed to it. Roster clocks come from the signers,
so a full member that turns hostile can backdate an entry before its own
removal arrives. A linked device can fill the roster with entries that fail
verification, up to a cap of 4,096 entries.

### Router gateway

`muniment-router serve` binds loopback unless its operator sets `listen`. The
admin token, 16 characters or more, guards run creation, run revocation and
outcomes, and the router compares it in constant time. Each run gets a run token:
claims for the run, task, role, budget and expiry, signed with HMAC-SHA256 under
the `run_token_signing_key`. The router checks the signature and expiry, then the
claims against the stored run, so a revoked run stops at once. An admin token
routes no model turn, and a run token creates no run. `/healthz` and `/metrics`
take no token. The metrics carry counts, costs and account labels but no secrets
or prompt content, so the listener stays off untrusted networks. Provider credentials live in
the host credential store or state directory, never in the request path.
On a Postgres store the gateway keeps each credential in the owner-only files
of `FileCredentialStore` under `credentials_dir`, or reads it from a
`MUNIMENT_SECRET_*` variable, so anyone who can read that directory or the
process environment holds those credentials. Exported OTLP spans carry run,
task, model, token and account labels but no prompt content, and the OTLP
headers may hold the collector's credentials. A leaked signing key lets its holder mint run tokens
until the key changes.

### Release signing key

Each `core-v*` release attaches `SHA256SUMS` and a minisign signature over it.
The `MUNIMENT_RELEASE_SIGNING_KEY` secret exists only in the release job, which
fails before it tags when the key is missing or when `RELEASE_PUBLIC_KEY` in
`core/crates/server/src/release.rs` does not verify the signature. The public key
is in source control. The private key never is. A holder of the private key can
publish an update that every launcher accepts. A key change needs a new launcher
build, because each launcher trusts the public key it was built with.

### Self-update launcher

`muniment-server launch` installs a version only when a device with
`runtime.update` or `runtime.admin`, or the local owner, requests one newer than
the running version. It downloads the archive, `SHA256SUMS` and the signature from
the feed, checks the signature against the release public key, checks the
archive's SHA-256, and unpacks only regular files into a staging directory. The
new binary must report the requested version. It runs as a trial and keeps the
version only when `health` passes. Otherwise the launcher restores the state
snapshot, starts the earlier version and never retries the failed one.

`MUNIMENT_UPDATE_FEED` and `MUNIMENT_UPDATE_PUBLIC_KEY` replace the feed and the
trusted key. They serve tests and mirrors that sign their own releases, and they
grant nothing beyond the operator's existing control of the environment. A
compromised feed can withhold updates but cannot install an unsigned build.

## Prompt injection

Pi assembles model context for prompts sent through
`core/crates/core/src/sidecar/pi_chat.rs`. The cloud classifies each prompt at
ingress. The deleted resident path in `src-tauri/core/src/llama.rs` no longer
defines a model-context boundary.

## Known gaps

- **Runtime service extraction remains open.** The desktop still owns the
  runtime. The target boundary is a user-level runtime
  service.
- **A same-user process can take the desktop-client and presenter routes on
  Linux and Windows.** On Linux, the runtime reads the peer executable and start
  time at accept, before any frame, and again when the hello arrives, and
  requires both reads to match the installed desktop payload. Linux has no
  process identity that survives exec, so a process that execs the desktop
  binary before the first read passes both reads. The Windows image path check
  has the same limit. The boundary excludes a compromised process running as
  the current OS user, so these routes carry no defense against one.
- **Default permission gates are not isolation.** Windows has no claimed
  native full-auto sandbox.
  Same-user compromise is a deferred risk.
- **Mobile has no implementation here.** It belongs
  to a separate repository and delivery lane.
- **Agent access has no implementation here.**
  Automation belongs to a
  server-side layer.
