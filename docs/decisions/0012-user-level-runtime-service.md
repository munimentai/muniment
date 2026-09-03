# 0012 — Promote the Pi runtime to a user-level background service

- Status: accepted
- Date: 2026-07-17
- Context: owner-ratified ops decision; harness-spec §§2.8, 6.2, 13, 14;
  ADRs 0008, 0009, and 0011

## Context

ADRs 0008 and 0009 made the desktop process the owner and supervisor of Pi,
the run journal, credentials, and the local attach endpoint. ADR 0011 therefore
described the CLI and editor extension as clients of a desktop-owned runtime.
That makes an otherwise independent execution surface depend on an installed,
running, and signed-in desktop app. It also gives the desktop UI a lifecycle
role that does not belong to a UI: closing its windows can remove the executor
and the outbound Remote Control leg.

The product owner ratified a different boundary on 2026-07-17. The governed
runtime is one shared, per-user background service. Desktop, CLI (E1), and the
editor extension (E2) are equal clients over the accepted
[`muniment.attach/1`](0009-companion-attach-protocol.md) contract. This ADR
supersedes the desktop-ownership and desktop-required statements in ADRs 0008,
0009, and 0011; their Pi distribution, wire, endpoint-security, authorization,
idempotency, and journal contracts otherwise stand.

## Decision

### Ownership and platform lifecycle

Exactly one Muniment runtime service runs for a logged-in OS user. It owns the
Pi child and RPC dispatcher, authoritative run journal and CAS, authenticated
device session and entitlement snapshot, runtime capabilities, attach
listener, and Remote Control outbound relay. Execution surfaces own only their
presentation state and attach connections. Closing every desktop or editor
window, or exiting the CLI, does not stop active work or Remote Control.

The service is installed and managed without administrator or root authority:

| Platform | Per-user registration | Start, restart, and logs |
|---|---|---|
| macOS | A `launchd` agent in the user's domain. | `launchd` starts it on demand and restarts unexpected exits according to a bounded backoff policy. Service stdout/stderr use the existing redacted, bounded diagnostic policy and the user-domain unified/system log integration. |
| Windows | A per-user startup/on-demand Scheduled Task running only as that user, never a machine service. | Task Scheduler starts it at logon or on an idempotent start request from a surface and applies bounded restart/backoff. Output goes to the service's bounded, redacted per-user diagnostic log and Windows event diagnostics where configured. |
| Linux | A `systemd --user` unit with socket/service activation. No system unit is installed. | The user manager starts it on socket demand (or an explicit surface request), applies bounded restart/backoff, and records redacted diagnostics in the user journal. |

Any surface first attempts ADR 0009 discovery and attach. If the service is
installed but stopped, that surface asks the native user service manager to
start it and retries with a bounded readiness deadline. Concurrent start
requests converge through the service manager and the existing per-profile
instance lock. The process that cannot acquire the lock does not open an
endpoint, journal, or Pi process; it exits successfully only after confirming
the lock owner is healthy. There is no fallback that spawns a private Pi or a
second journal. On Windows, a held attach instance lock records an
`instance_lock_wait` diagnostic and exits with status 0.

Normal idle policy may stop an inactive service only when it has no attached
surface, active or recoverable run, pending permission gate, Remote Control
leg, or other durable work requiring ownership. The next attach activates it
again. Logoff/OS shutdown requests a bounded graceful stop; journal commits,
not process memory, determine recovery.

### Installation and upgrade

The runtime is a shared dependency of every execution-surface installer. The
desktop installer, CLI package, and editor-extension distribution each declare
a minimum compatible runtime version and perform the same idempotent check:

1. If no service registration exists, install the signed runtime payload and
   its per-user platform registration.
2. If the installed version satisfies the surface's minimum, leave it in
   place. Installing another surface never installs or registers a second
   runtime.
3. If it is older, stage and verify the newer signed payload, ask the running
   service to quiesce, atomically replace it in place, restart it, and verify
   protocol/readiness. Concurrent installers serialize on a per-user install
   lock and re-read installed state after acquiring it.
4. A failed activation restores the previous verified payload and restarts it.
   An installer never downgrades a newer compatible runtime.

The service and its platform registration ride the existing desktop
codesigning/notarization and release pipeline even when delivered by another
surface. There is no system-level installation. ADR 0008's verified,
upgrade/rollback-aware acquisition of the pinned Pi executable remains the
service's responsibility after installation; the Muniment service payload is
the signed shared dependency, while Pi remains its single managed child.

### Attach security and authorization

ADR 0009's endpoints and peer checks are unchanged, including Linux
`SO_PEERCRED`, macOS `getpeereid`, the Windows owner-only named-pipe DACL and
token check, owner-only directories, and no TCP or `/tmp` fallback. Endpoint
and instance ownership move from the desktop process to the service process;
the endpoint names and wire version do not change.

Visible connection approval can no longer depend on a desktop window. The
service owns the single-use ADR 0009 approval challenge and opens a
service-mediated system-browser consent page when a new surface needs
approval; a capable desktop UI may render the same service-owned challenge.
Approval remains explicit, names the requesting surface and workspace scopes,
and is bound to the peer identity, connection nonces, and connection exactly as
ADR 0009 requires. A requesting surface cannot mark its own challenge approved.
This changes the approval presenter, not the same-user threat model or the
connection-bound capability.

### Authentication and the shared device session

Sign-in is a service capability, not a desktop prerequisite. On a machine with
only the CLI installed, `muniment sign-in` asks the service to perform the
existing installation-bound native-auth flow: bind the ephemeral loopback
callback, open the system browser, validate state, and exchange the code with
PKCE as specified by harness-spec §2.8 and `docs/auth.md`. The desktop may
initiate that identical service operation from its sign-in UI. The editor may
direct the user to either supported initiator; it does not invent a token flow.

The service alone persists and refreshes the shared device session and holds
the signed entitlement snapshot, installation keys, and virtual/provider
credentials. Surfaces receive only status and mediated projections and never
tokens. Sign-out from any surface revokes the one device session, attach
capabilities, and Remote Control legs for all surfaces. Concurrent sign-in,
refresh, sign-out, and profile changes serialize in the service so stale
completion cannot resurrect a revoked session.

### Crash, recovery, and Remote Control

Service-manager restart does not authorize replay. ADR 0009's journal-backed
crash rules compose unchanged: committed events are authoritative; an accepted
intent with unknown outcome or an effect without a committed terminal outcome
becomes `run.needs_attention`; permission answers and external effects are
never guessed or repeated. Clients resume by journal cursor and tolerate
duplicate delivery. Restart backoff is bounded so a crash loop becomes a
redacted needs-attention state rather than unbounded respawning.

The service owns the single outbound-only HTTPS Remote Control relay leg. It
continuously enforces the shared session, entitlement, policy, journal, and
kill-switch rules. Because its lifecycle is independent of every UI, a live
remote-controlled session continues when all UI windows are closed. Service
loss follows §14's bounded reconnect and stale-presence rules; the relay never
becomes an execution or recovery authority.

## Sequencing and consequences

Service extraction is its own cross-platform build and installer line. E1 CLI
does **not** wait for it: its first slices may attach to the current
app-managed implementation because ADR 0009's endpoint and wire contract are
identical. Those slices must keep ownership behind the attach boundary and
must not encode desktop process discovery, desktop paths, or a private-runtime
fallback. Extraction later moves the existing owner behind the same protocol.

The CLI and editor extension become usable without the desktop app, but remain
interactive, human-operated surfaces. Headless or scripted agent use remains
the server-side agent access layer's responsibility. Mobile remains a
non-executing companion. This decision creates neither a root/system service
nor a second runtime per profile or machine user.

## Rejected alternatives

**Keep the desktop as owner.** This prevents CLI-only installation and makes
window lifecycle an execution dependency.

**Let each surface spawn a runtime.** This creates competing journals,
credentials, endpoints, effects, and recovery authorities.

**Install a machine-wide service.** It requires elevated installation and
introduces cross-user isolation and ownership problems absent from the
per-user product boundary.

**Block E1 until extraction.** The attach protocol already supplies the stable
boundary; coupling the two build lines delays usable client work without
reducing the later migration.

**Route browser opening through the approval presenter.** Asking the
registered desktop approval presenter to open the authorization URL adds a
second wire operation and a presenter dependency to every sign-in. The service
opens the browser itself, as the authentication decision requires.

## References

- [ADR 0008 — Pi runtime distribution](0008-pi-runtime-distribution.md)
- [ADR 0009 — companion attach protocol](0009-companion-attach-protocol.md)
- [ADR 0011 — companion surface repository strategy](0011-companion-surface-repo-strategy.md)
- [Harness specification](../spec/harness-spec.md)
- [Native authentication contract](../auth.md)
- [Apple Service Management](https://developer.apple.com/documentation/servicemanagement/)
- [Apple launchd job guide](https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPSystemStartup/Chapters/CreatingLaunchdJobs.html)

## Amendment — 2026-08-04: runtime-service extraction sequence

Extraction starts on Linux. The service is the `muniment-runtime` binary crate
at `src-tauri/runtime/`, beside the existing `core`, `attach`, `cli`, and `acp`
workspace members. Windows Scheduled Task registration and macOS `launchd`
registration require a later amendment. Phase one does not claim that either
platform can run the service.

`muniment-runtime` may depend directly on `muniment-core`,
`muniment-attach`, and the narrow platform and serialization crates needed to
host them. It must not depend on `muniment-desktop`, Tauri, the frontend, the
CLI, or the ACP adapter. It has its own format, lint, test, and dependency-tree
CI steps. This follows ADR 0011's companion-crate shape while preserving the
opposite dependency direction: clients depend only on `muniment-attach`, while
the service may compose the governed core with the attach protocol.

Phase one moves the Pi child and dispatcher, journal and CAS, device session,
entitlement snapshot, credentials, workspace authorization, permission gates,
and ADR 0009 listener into `muniment-runtime`. The desktop keeps Tauri windows,
navigation, composer state, selected local paths, audio capture, shortcuts, and
other presentation state. A workspace authority is the signed
`grant.workspace` value. A surface working directory is only a requested local
execution root. The service validates and records their mapping instead of
treating the directory as workspace authority.

During phase one, the running desktop presents the service-owned approval
challenge. The desktop reports an explicit user choice and cannot manufacture
approval, including approval of its own connection. If no desktop can present
the challenge, a new connection fails closed. The service-mediated browser
presenter in the base decision follows in a later slice. Existing approved
connections and active runs do not require a desktop window.

The service and the legacy desktop use ADR 0009's existing per-profile instance
lock. They never use separate migration locks or endpoint names. A legacy
desktop that already holds the lock keeps the listener until an updated
desktop reaches a safe handoff point. The service waits without opening the
endpoint, journal, CAS, or Pi.

The safe handoff point has no active run, pending permission gate, authentication
operation, session refresh, or in-flight external effect. The waiting service
connects to the desktop-owned ADR 0009 endpoint and sends a migration control
request in an existing `muniment.attach/1` request envelope. The request asks
the desktop to quiesce and carries a single-use handoff nonce and bounded
deadline. This migration operation adds no endpoint or wire version.

The desktop rejects the request and remains the owner when it cannot reach the
safe handoff point. Otherwise, it pauses new requests and returns a prepared
response on that connection. It then stops accepting connections, closes the
listener and all moved state, and releases the lock. The prepared response is
the service's only authority to attempt that handoff. The service must acquire
the lock before it opens the journal, CAS, Pi, device session, or listener.

After opening all moved state and the unchanged endpoint, the service returns
the nonce in its `welcome` message. The desktop opens a probe connection and
checks the nonce and service readiness. It closes that probe before it
reconnects as an ordinary client. Other clients treat the gap as an ordinary
bounded reconnect and resume from committed cursors.

The desktop does not release ownership while a permission answer or an
external effect has an unknown outcome. If the service misses its readiness
deadline, it closes any listener and moved state, releases the lock, and does
not retry that handoff. The desktop restores ownership only by acquiring the
same lock. It then reopens all moved state before it reopens the listener. A
service failure response on the control connection cancels preparation before
the desktop releases the lock. Desktop ownership then remains unchanged. A
stale desktop that does not implement handoff keeps ownership until it exits.
These rules allow a temporary interval with no owner, but never an interval
with two owners.

Implementation proceeds in reviewable slices. The first slice is **Linux
runtime ownership scaffold**. It adds the workspace member, bounded manifest,
Linux instance-lock acquisition, and mutual-exclusion tests. The scaffold does
not open the attach endpoint, journal, CAS, or Pi. Later slices move the
approval coordinator, journal and Pi execution, shared device session, desktop
client conversion, and Linux user-unit registration behind dormant service
entry points. The desktop remains the owner throughout those slices. The final
cutover slice activates the listener, approval coordinator, journal, CAS, Pi,
device session, credentials, authorization, and permission gates together.
Remote Control follows the cutover. This amendment changes no runtime code.

## Amendment — 2026-08-05: migration control request authority

Only the waiting runtime service may send the migration control request. An
approved client credential grants no migration control authority. A claimed
kind also grants no migration control authority.

On Linux, the desktop reads the connection peer PID from `SO_PEERCRED`. It
resolves that PID to its executable path and requires the installed
`muniment-runtime` payload. It rejects the request when it cannot resolve the
path or the path does not identify that payload. The desktop prepares at most
one handoff at a time. It rejects a second request while a handoff is prepared.

This rule excludes an approved companion from migration control authority. It
does not defend against compromise by another process running as the current
OS user. Windows needs a later amendment for its own peer identity. The macOS
peer rule appears in the 2026-08-20 amendment.

## Amendment — 2026-08-10: migration control session admission

On Linux, a connection whose `SO_PEERCRED` peer resolves to the installed
`muniment-runtime` payload enters a dedicated migration control session. This
session needs no visible approval and receives no stored companion credential.
The peer check, not a claimed kind, client ID, or reconnect credential, is the
only admission authority.

The session authorizes only `migration.control`. It has no workspace scope and
no thread, run, permission, subscription, authentication, or other companion
authority. The dispatcher rejects every other companion operation on that
session. The session ends with the connection and cannot authorize a later
connection.

Admission fails closed when the desktop cannot resolve the peer or cannot
match its executable to the installed payload. The request still passes the
migration control peer check and the single-prepared-handoff rule from the
2026-08-05 amendment. This admission path shares that amendment's limitation:
it does not defend against compromise by another process running as the
current OS user. Windows needs a later peer-identity amendment before it can
use this admission path. The macOS peer rule appears in the 2026-08-20
amendment.

## Amendment – 2026-08-13: approval presentation session

During phase one, the runtime presents its ADR 0009 approval challenge to the
desktop over the existing `muniment.attach/1` connection. The runtime sends an
`approval.present` request envelope containing the requesting surface,
workspace scopes, single-use challenge, and its decision deadline. The desktop
returns its explicit approve or deny choice in the correlated response. This
operation adds no endpoint and no wire version.

On Linux, the runtime admits a presenter session only when the connection's
`SO_PEERCRED` peer PID resolves to the installed desktop payload. This uses the
same executable-resolution primitive as migration control in the opposite
direction. An unresolved, changed, or mismatched peer fails closed. The peer
check, not a claimed kind, client ID, or companion credential, is the only
admission authority.

The presenter session authorizes only approval presentation. It has no
workspace, thread, run, permission, subscription, authentication, migration
control, or other companion authority. The runtime rejects every other
operation on that session. The session ends with the connection and cannot
authorize a later connection.

The runtime admits at most one presenter session at a time and rejects a
second while the first remains connected. It keeps at most one pending
decision for each challenge. If no presenter session exists when a new
connection needs approval, that connection fails closed. A disconnected
presenter, a deny choice, no choice by the challenge's two-minute deadline, or
a late, repeated, unknown, or mismatched choice also fails closed. Existing
approved connections and active runs do not depend on the presenter session.

This admission proves only the installed desktop payload under the same OS
user. It does not defend against compromise by another process running as that
user. Windows needs a later peer-identity amendment before it can admit a
presenter session. The macOS peer rule appears in the 2026-08-20 amendment.

## Amendment – 2026-08-13: runtime attach connection routing

The runtime routes a new attach connection to the presenter session only when
both checks pass. The connection's `SO_PEERCRED` peer PID must resolve to the
installed desktop payload, and its first hello must claim the client kind
`desktop`. For a connection from that payload, a first hello that claims
`desktop-handoff-probe` keeps the existing readiness answer. Every other
connection takes the companion route.

The routing check consumes no byte from the connection. The admitted path
reads the first hello itself. An unreadable, oversized, or late first frame
falls to the companion route, which still requires visible pairing approval.

## Amendment – 2026-08-14: desktop approval presenter lifecycle

The desktop starts its approval presenter supervisor in two states. It starts
after a handoff probe confirms that the runtime owns the profile endpoint. It
also starts at launch when another process holds the per-profile instance
lock. The desktop dials no presenter connection while its own listener owns
the endpoint.

The supervisor reconnects until the desktop stops it. A restarted desktop
listener stops the supervisor, as does desktop shutdown. A dropped presenter
connection cancels no approval that the user already made.

While presenting, the desktop asks the user through the same visible pairing
prompt that its local listener uses. The runtime still applies the admission,
routing, single-presenter, and decision rules from the two 2026-08-13
amendments.

## Amendment – 2026-08-14: desktop client session admission

The converted desktop opens a second `muniment.attach/1` connection whose
first hello claims the client kind `desktop-client`. On Linux, the runtime
routes that connection to a dedicated desktop client session only when its
`SO_PEERCRED` peer PID resolves to the installed desktop payload. A connection
from that payload whose first hello claims `desktop` still takes the presenter
route. A `desktop-handoff-probe` claim still keeps the readiness answer. Every
other connection still takes the companion route.

The peer check is the only admission authority. Neither visible approval nor
a stored companion credential applies. A claimed kind, client ID, or reconnect
credential grants no authority without the peer check.

The desktop client session carries owner authority over the signed
`grant.workspace` value, including its workspace, thread, run, permission, and
subscription authority. It carries neither migration control nor approval
presentation. The session ends with the connection and cannot authorize a
later connection.

Admission fails closed when the runtime cannot resolve the peer or cannot
match its executable to the installed payload. This admission proves only the
installed desktop payload under the same OS user. It does not defend against
compromise by another process running as that user. Windows needs its own
peer-identity amendment before it can admit a desktop client session. The
macOS peer rule appears in the 2026-08-20 amendment.

## Amendment – 2026-08-14: desktop client session lifecycle

The desktop starts its client supervisor in two states. It starts after a
handoff probe confirms that the runtime owns the profile endpoint. It also
starts at launch when another process holds the per-profile instance lock. The
desktop opens no client connection while its own listener owns the endpoint.

The desktop keeps at most one client connection. Every desktop command rides
that connection. A dropped connection fails each in-flight request, and the
supervisor reconnects until the desktop stops it.

A restarted desktop listener stops the client connection, as does desktop
shutdown. While the desktop holds no client session, it shows the existing
server-unreachable notice rather than an empty surface.

## Amendment – 2026-08-14: desktop client operation surface

The desktop client session may send every companion operation that its signed
`grant.workspace` value authorizes. It may also send the desktop-only
operations named by this amendment. The first desktop-only pair consists of
`thread.rename` and `thread.delete`. Each request requires an idempotency key.

A companion session receives `unauthorized` for a desktop-only operation. The
desktop client session still receives `unauthorized` for `migration.control`
and `approval.present`.

Session, entitlement, device, sign-out, and home commands remain outside this
tranche. They require their own amendment before the desktop client session may
send them.

## Amendment – 2026-08-14: second desktop client operation tranche

The second desktop-only tranche maps these wire operations to the existing
runtime service entries:

| Operation | Runtime service entry |
|---|---|
| `session.status` | `service::session::session_status` |
| `entitlement.snapshot` | `service::session::entitlement_snapshot` |
| `device.list` | `service::session::list_devices` |
| `session.sign_out` | `service::session::sign_out` |
| `companion.list` | `service::workspace::list_companions` |
| `companion.revoke` | `service::workspace::revoke_companion` |

A desktop client session may send these six operations. A companion session
receives `unauthorized` for each operation.

`session.sign_out` and `companion.revoke` require an idempotency key.
`session.status`, `entitlement.snapshot`, `device.list`, and `companion.list`
are read operations and require no idempotency key.

Browser sign-in and `home.ensure` remain outside this tranche. Sign-in opens a
visible system browser through the injected `BrowserOpener`. A background
service that starts that browser needs its own decision before the desktop
client session may request sign-in.

## Amendment – 2026-08-15: runtime-owned browser sign-in

The desktop client session may send `session.sign_in`. The request requires an
idempotency key. A companion session receives `unauthorized` for this
operation.

The runtime service runs the native sign-in flow and opens the system browser
through its own injected `BrowserOpener`. It permits one sign-in attempt per
service. A second concurrent request receives the typed
`sign_in_in_progress` refusal. The response carries `AuthStatus` alone. It
carries no token material or authorization URL.

This request uses its own deadline instead of the attach client's five-second
I/O deadline. The deadline covers the 300-second sign-in window and protocol
overhead. A running sign-in blocks quiesce as an authentication operation.

This tranche drops the existing rate-limit retry notice. The service sends no
progress event through the single request, so the desktop cannot show that
notice while sign-in waits to retry. A later operation may add progress
reporting without changing sign-in ownership.

## Amendment – 2026-08-15: third desktop client operation tranche

The third desktop-only tranche maps these wire operations to the existing
runtime service entries:

| Operation | Runtime service entry |
|---|---|
| `thread.summaries` | `service::thread_summaries` |
| `thread.history` | `service::thread_page` |

A desktop client session may send these two operations. Both are read
operations and require no idempotency key. A companion session receives
`unauthorized` for each operation. Both requests use the attach client's
five-second I/O deadline.

The existing `thread.list` and `thread.open` operations remain companion
operations. They scope reads by the signed `grant.workspace` value and return
the redacted `ThreadListPage` and `ThreadOpenPage` projections. The new
operations return the subject-scoped desktop projections instead.

Each service entry shrinks the requested page until its encoded response fits
the `MAX_FRAME_LENGTH` bound. It returns `persistence_failed` only when a
one-entry page still does not fit.

The run surface, session-thread tracker commands, and `home.ensure` remain
outside this tranche. The Linux cutover follows conversion of the whole chat
surface. The desktop opens the profile journal and CAS when it creates
`ChatState`, so an earlier listener cutover would create two owners for one
profile.

## Amendment – 2026-08-15: fourth desktop client operation tranche

The fourth desktop-only tranche contains the single `run.chat_events` wire
operation. It requires no idempotency key. A companion session receives
`unauthorized` for this operation.

The desktop opens a second desktop client connection for this subscription.
The session serving that connection answers no other operation. The existing
request path reads exactly one envelope and rejects an event in place of its
response. Its holder also locks the socket for each request, so a blocking
event read would stall every desktop command. Each accepted connection already
runs on its own thread with its own service, so the second connection needs no
new admission rule.

The runtime keeps one chat-event broadcast per profile. Every run the service
drives feeds that broadcast. A subscriber receives only the events delivered
after it subscribes.

Each subscription queue holds at most 256 events under
`CHAT_EVENT_SUBSCRIBER_QUEUE_CAPACITY`. Delivery never blocks. If delivery
finds the queue at that bound, the runtime drops the subscription. A run never
stalls on a subscriber.

Each event carries the same `ChatEvent` value that the desktop renders, with no
redaction. The desktop client peer check supplies the authority to receive that
value.

The five run commands `chat_submit`, `chat_resume`, `chat_queue`, `chat_cancel`,
and `chat_answer_permission` remain outside this tranche.

## Amendment – 2026-08-16: fifth desktop client operation tranche

The fifth desktop-only tranche maps these wire operations to the existing
runtime service entries:

| Operation | Runtime service entry |
|---|---|
| `run.submit` | `service::accept_prompt`, then `service::drive_prompt` |
| `run.resume` | `service::resume_run` |
| `run.steer` | `service::queue_run_message` with `ChatDelivery::Steer` |
| `run.follow_up` | `service::queue_run_message` with `ChatDelivery::FollowUp` |
| `run.permission_answer` | `service::answer_permission` |

A desktop client session may send these five operations. A companion session
receives `unauthorized` for each operation. Each operation requires an
idempotency key.

`run.submit` returns after `service::accept_prompt` accepts the prompt. The
runtime then calls `service::drive_prompt` on its own thread. The request does
not wait for the run to finish.

The existing `permission.answer` operation cannot carry a desktop answer. Its
wire `decision` uses the two-variant `PermissionDecision`, while the desktop
sends the six-variant `ChatPermissionAnswer`. The new
`run.permission_answer` operation carries the desktop answer without reducing
it to allow or deny. Its permission-answer commit wait is two seconds, below
the attach client's five-second I/O deadline.

`run.steer` and `run.follow_up` reuse their existing operation names and
fixtures. They add desktop dispatch to `service::queue_run_message`.
`run.cancel` is also reused. It already dispatches to `service::cancel_run`
through the session workspace, so `chat_cancel` needs only a typed client
method.

A later slice converts all five desktop run commands in one atomic flip:
`chat_submit`, `chat_resume`, `chat_queue`, `chat_cancel`, and
`chat_answer_permission`. Until that slice lands, the desktop keeps all five
commands on the current path.

## Amendment – 2026-08-16: sixth desktop client operation tranche

The desktop session-thread tracker is the only thread selection authority on
the desktop client path. The runtime session-thread tracker holds no authority
over a desktop prompt.

A `run.submit` request with a null `thread_id` starts a fresh thread on both
service halves. The desktop records the accepted `thread_id` from the response
in its tracker. The next prompt then continues that thread.

The sixth desktop-only tranche contains the single `thread.select` wire
operation. It maps to the `service::select_thread` runtime entry, which uses
the `subject_owns_first_run` predicate. The service validates that the subject
owns the named thread before the desktop records the selection. A companion
session receives `unauthorized` for this operation.

`thread.select` appends nothing, so it requires no idempotency key.

The existing `chat_current_thread` and `chat_new_thread` commands remain local
session-thread tracker operations.

## Amendment – 2026-08-17: Linux cutover rules

### Activation

The `muniment-runtime` main composes the attach state and serves it. For a
fresh profile, it binds the profile endpoint through `run_attach_listener`.
When another process holds the instance lock, it follows
`run_migration_takeover`. A termination signal ends the service cleanly.

### Admission

The runtime admits the installed desktop client while it holds no signed
workspace. The granted capability then carries no workspace scope. The first
resolved run grant records the signed workspace for the runtime.

Companion pairing still fails closed while the runtime holds no signed
workspace. This rule supersedes the MUNIDESK-1253 whole-session refusal. That
refusal would reject the desktop client that must sign in and resolve the first
run grant, so a fresh runtime could never record a workspace.

### Launch ownership

A desktop that finds the instance lock held opens no profile journal and does
not run `reconcile_interrupted_runs`. It rides the desktop client connection.
The desktop opens the profile journal only while it owns the listener.

### Sign-out

Sign-out clears the runtime's recorded signed workspace before the existing
revocation step.

### Packaging

The Linux package ships a `systemd` user unit that starts
`muniment-runtime` at session login. The unit merges only after activation,
admission, and launch ownership are built. Where no user manager runs, the
desktop keeps its own listener path.

## Amendment – 2026-08-17: installed-payload refresh

The runtime owns installed-payload refresh. It watches the identity of its own
executable. When an upgrade replaces that executable, it exits with a distinct
nonzero upgrade-refresh status. The shipped `Restart=on-failure` policy then
starts the installed payload. A manager-requested service stop does not use the
upgrade-refresh status, so the manager leaves the service stopped. Package
scripts do not enumerate user managers or restart their services.

On macOS, the runtime watches its executable identity once each second. A
replacement stops the attach accept loop and exits with status 75. The runtime
records that stop as an orderly exit, so it does not count toward the failed
start bound. A `SIGTERM` stop still exits with status zero.

A refresh request does not interrupt a live run. After detecting replacement,
the runtime enters a drain state and rejects new work that could add any
`evaluate_quiesce` blocker. This rule applies to desktop and companion clients.
The runtime continues the active run and accepts `run.permission_answer` and
`run.cancel`. It also keeps event subscriptions and read operations available
so clients can finish or observe existing work. The runtime exits only when
`evaluate_quiesce` accepts its current `RuntimeActivity`: no active run,
pending permission gate, authentication operation, session refresh, or
in-flight external effect may remain. It re-evaluates quiesce as activity
changes. Concurrent replacements collapse into that single pending refresh.

The desktop compares the version in the attach welcome with its minimum
compatible runtime version. While the runtime is older, the desktop keeps the
connection available for operations needed to finish existing work and reach
quiesce. It sends no operation that requires the newer runtime and starts no
new run. It shows that the runtime upgrade is pending, reconnects after the
service exits, and resumes normal requests only after a compatible welcome.

Only the running runtime may turn an observed replacement into its
upgrade-refresh exit. A client cannot request this refresh through the attach
protocol. On Linux, another OS user cannot signal the process or control its
`systemd --user` manager, and the owner-only endpoint rejects that user's
connection. An administrator may replace the installed package, but package
authority does not grant attach or runtime authority.

## Amendment – 2026-08-18: desktop run rejoin

The Linux cutover lets `muniment-runtime` continue a run after the desktop
window closes. A relaunched desktop then opens a thread whose newest run still
executes. The desktop rejoins that run. It does not treat the thread as idle.

`projection_phase` answers `thinking`, `streaming`, or `pending-permission` for
an unsettled run. On thread open, the desktop takes the newest unsettled run in
that thread as its active run. Where the thread holds no unsettled run, the
desktop leaves the active run null, as it does today.

A rejoined run keeps its recorded prompt, text, tool activity, and pending
permission gate. The desktop renders those recorded values as the starting state
of the run. Further chat events reach that run by run id, so the existing
chat-event match carries the rejoined run forward.

The desktop starts no prompt, resume, or other execution for a rejoined run. The
runtime already drives it. The desktop offers the same stop, steer, and
follow-up controls that it offers a run it started. The composer, the thread
switch, and the thread delete follow the existing active-run rules.

`projection_phase` answers `complete`, `cancelled`, `failed`, or `interrupted`
for a settled run. A settled run never rejoins. An `interrupted` run keeps the
existing resume control on its history entry.

Where the desktop owns the profile journal, `reconcile_interrupted_runs` settles
every unfinished run at open. That path meets no unsettled run, so this rule
changes nothing on it. The rule governs only the path where the runtime owns
the journal.

## Amendment – 2026-08-18: passive chat-event delivery

### What a client receives for a run it did not start

A chat-event subscriber receives every event of every run in its workspace. The
runtime applies no filter for the client that started the run. The fourth
tranche already feeds every run the service drives into the one per-profile
broadcast. A subscriber therefore sees a run another client started exactly as
it sees its own. A subscriber still receives only the events the runtime
delivers after it subscribes. The journal, not the broadcast, carries the
history before that point.

The `submit_run` and `resume_run` boundaries that serve `run.submit` and
`run.resume` break that rule today. Both pass an absent subscriber to
`service::accept_prompt` and `service::resume_run`, so a run the desktop starts
through the runtime reaches no subscriber. The slice passes the broadcast on
those two boundaries, as the companion `launch` boundary already does.

### Authorization

The broadcast delivers an event only inside the workspace the signed grant
names. `RuntimeChatEventBroadcast::deliver` takes the workspace of the run's
resolved grant. It compares that value against the workspace the runtime
recorded in its `SignedWorkspaceApproval`. A mismatch delivers nothing and
keeps the subscription open. The broadcast delivers no event at all while the
runtime holds no recorded workspace.

The runtime reads the recorded workspace at delivery, not at subscribe. The
2026-08-17 cutover admits the desktop client before the first run grant
resolves, so a subscribe-time read would freeze an empty workspace on that
connection. The first resolved run grant records the workspace before the run
emits its first event. The desktop therefore receives that first run in full.

`run.chat_events` stays a desktop-only operation. A companion session still
receives `unauthorized`, because each event carries the unredacted `ChatEvent`
value that the fourth tranche defined. A companion reads a run it did not start
through the redacted ADR 0009 `run.event` stream instead.

### How a passive client learns that the thread list changed

Each broadcast event carries the thread id of its run. ADR 0016 makes new
thread creation and its first run start one transaction, so every new thread
produces chat events. A client that receives an event naming a thread its list
does not hold refreshes the thread list. The sidebar then shows an ACP or CLI
thread without a relaunch. A rename or a delete needs no signal, because
`thread.rename` and `thread.delete` stay desktop-only operations.

Three rules bound that delivery.

The client refreshes once for each thread id it has not yet signaled, and not
once for each event. A run emits an event for each streamed delta, so a
per-event refresh would query the runtime on every token. The client marks the
thread id when it signals, whether or not the refreshed page holds that thread.
It keeps the signaled ids in an insertion-ordered set bounded at 256 entries and
drops the oldest entry past that bound. The bound stops the same unbounded map
that MUNIDESK-1362 removed from this handler.

The client keeps at most one thread refresh in flight. It collapses every
signal that arrives during a refresh into one follow-up refresh.

The fourth tranche subscriber queue rule stands unchanged. Each subscription
holds at most 256 events under `CHAT_EVENT_SUBSCRIBER_QUEUE_CAPACITY`. Delivery
never blocks, and the runtime drops a subscription whose queue is full. A run
never stalls on a subscriber. A dropped subscription loses its pending signals.
The client refreshes the whole thread list when its next subscription opens, so
that loss lasts one reconnect.

### Operation and wire version

This rule adds no attach operation and no wire version. The existing
`run.chat_events` subscription already carries the `ChatEvent` body inside the
current event envelope. The thread id joins that body as an optional field, and
ADR 0009 already accepts an unknown optional field. An older desktop ignores the
thread id and keeps today's behavior. An older runtime sends no thread id, so a
newer desktop signals no refresh. The workspace stays inside the runtime,
because the broadcast applies the filter before delivery.

### Code sites for the implementation slice

The later slice touches these sites:

1. `ChatEvent` and `chat_event` (`src-tauri/core/src/run_events.rs`) carry the
   optional thread id.
2. `RuntimeChatEventSink::deliver` (`src-tauri/runtime/src/sink.rs`) stamps the
   thread id of its own run.
3. `RuntimeChatEventBroadcast::deliver` and its constructor (same file) apply
   the workspace filter against the recorded `SignedWorkspaceApproval`.
4. `launch`, `submit_run`, and `resume_run`
   (`src-tauri/runtime/src/attach_boundaries.rs`) pass the run's thread id,
   `grant.workspace`, and the broadcast into the sink.
5. `RuntimeAttachState` (`src-tauri/runtime/src/attach_state.rs`) hands the
   broadcast the recorded `SignedWorkspaceApproval`.
6. `handleEvent` and `refreshThreads` (`src/lib/chat-controller.js`) hold the
   signaled-thread set and the single-refresh rule.
7. The golden fixtures in `src-tauri/attach/tests/protocol.rs` and
   `src-tauri/core/tests/attach_desktop_client_session.rs` record the added
   body field.

This amendment changes no runtime code.

## Amendment – 2026-08-19: open-thread passive run

### What a passive run owes the open thread

The 2026-08-18 passive chat-event delivery amendment gives a subscriber every
event of every run in its workspace. It names how a passive client learns that
the thread list changed. It names nothing for the open thread.

`handleEvent` (`src/lib/chat-controller.js`) finds no message for a run id the
transcript does not hold. It drops the event. A run another client started
inside the open thread renders nothing until the user reopens that thread.

The desktop re-reads the open thread in place once. The first event that names
that thread with a run id the transcript does not hold triggers the re-read.
When the transcript already holds the run id, `handleEvent` applies the event
as it does today. When the event names another thread, the desktop does not
re-read the open thread.

`refreshOpenThread` already re-reads those pages in place and selects no
thread. `unsettledRun` (`src/lib/chat-state.js`) hands the re-read pages the
run the runtime still drives. The 2026-08-18 desktop run rejoin amendment
already covers that run. The runtime admits one active run per profile
(`src-tauri/runtime/src/attach_boundaries.rs`). At most one such run exists at
a time.

The desktop marks that run id signaled whether or not the re-read finds it. It
never re-reads for that run again. A run emits an event for each streamed
delta, so a per-event re-read would query the runtime on every token.

### Bounds

Three rules bound that re-read.

The desktop re-reads once for each run id it has not yet signaled, and not
once for each event. It marks the run id when it signals, whether or not the
re-read finds that run. It keeps the signaled run ids in an insertion-ordered
set of at most 256 entries. It drops the oldest entry past that bound. The
bound stops the same unbounded map that MUNIDESK-1362 removed from this
handler.

The desktop keeps at most one open-thread re-read in flight. It collapses
every signal that arrives during a re-read into one follow-up re-read.

The fourth tranche subscriber queue rule stands unchanged. A dropped
subscription loses its pending signals.

### Operation and wire version

This rule adds no attach operation and no wire version. The 2026-08-18
passive chat-event delivery amendment already carries the thread id on the
event body. This rule reads that field. An older desktop drops the event and
keeps today's behavior. An older runtime sends no thread id, so a newer
desktop re-reads the open thread for no such run.

### Code sites for the implementation slice

The later slice touches these sites:

1. `handleEvent` (`src/lib/chat-controller.js`) holds the signaled-run set
   and the single-re-read rule.
2. `refreshOpenThread` (same file) keeps one re-read in flight and collapses
   every signal during a re-read into one follow-up re-read.

The later slice changes no attach boundary, no sink, and no `ChatEvent`
field. This amendment changes no runtime code.

## Amendment – 2026-08-20: macOS activation and peer identity

### Registration and activation

The macOS app bundle carries the runtime at
`/Applications/muniment.app/Contents/Library/LaunchServices/muniment-runtime`.
It carries `ai.muniment.runtime.plist` in `Contents/Library/LaunchAgents`.
The plist runs the runtime in the foreground and labels the job
`ai.muniment.runtime`.

Each user registers that bundled LaunchAgent in their login domain with
`SMAppService.agent(plistName: "ai.muniment.runtime.plist").register()` while
that user runs the desktop app. The app checks `status` at every launch. It
calls `register()` once for `notRegistered`, then rechecks the status. A system
package installs the bundle but cannot register it for absent users.

An `enabled` status permits activation. A `requiresApproval` status or a
`kSMErrorLaunchDeniedByUser` result leaves the runtime inactive. The app shows
that approval is required. It offers a button that calls
`SMAppService.openSystemSettingsLoginItems()`. A `notFound` status or another
registration error also leaves the runtime inactive and records a redacted
diagnostic. The app never kick-starts or privately spawns the runtime unless
the status is `enabled`.

Successful registration immediately bootstraps the job. `launchd` bootstraps
it again at later logins. A surface with an `enabled` status and no attach
endpoint asks the `gui/<uid>` domain to kick-start `ai.muniment.runtime`, then
uses the existing bounded readiness retry. Registration and activation never
use a system LaunchDaemon or elevated helper.

The job uses `KeepAlive` only for unsuccessful exits and sets
`ThrottleInterval` to five seconds. The runtime records starts in its per-user
state and returns a successful status after five failures within five minutes.
That successful exit stops the restart loop and records a needs-attention
diagnostic. A later explicit kick-start starts a new bounded window. Normal
idle, logout, manager stop, and uninstall exits are successful and do not
restart the job.

### Replacement, logout, and removal

Package replacement follows the installed-payload refresh amendment. The
installer verifies and atomically replaces the signed app bundle without
changing the registered label or payload path. The old runtime drains durable
work and exits with status 75 after it detects replacement. `launchd` then
starts the new payload. A failed readiness check creates an owner-only rollback
marker. The marker makes every new payload exit successfully before it opens
state. The installer requests a graceful stop and waits for the bounded stop
deadline. It then sends `SIGTERM` and waits again. It sends `SIGKILL` if the
same job remains active. The installer verifies that no runtime holds the
instance lock before it restores the old verified bundle. It removes the
marker, kick-starts the same job, and verifies old-payload readiness. Any stop
or old-readiness failure leaves the marker and signed bundle in place. The
installer reports the failure. The per-user install lock serializes
replacement, rollback, and concurrent surface installers.

At logout, `launchd` sends `SIGTERM`. The runtime stops accepting new work,
uses the existing bounded graceful-stop deadline, commits recoverable state,
and exits. If the deadline expires, launchd may kill it. The journal remains
the only recovery authority at the next login.

Managed uninstall calls `unregister()` in each logged-in registered user
context and waits for each bounded completion. It cannot unregister an absent
user. Removal stays pending until each absent registered user logs in and the
app completes unregistration in that user's context. Unregistration stops
each job and prevents later login activation. The package removes the app
bundle only after every registration clears and every runtime stops. Any
unregister or stop failure leaves the signed payload in place and reports the
failure. Uninstall never removes a running payload or user journals,
credentials, CAS data, or diagnostics. Service Management registration
remains per-user.

### Peer admission and diagnostics

Both ends of every macOS attach connection call `getpeereid` on the connected
Unix-domain socket. They require the kernel-supplied effective UID to equal
their own effective UID before either end reads or writes a
`muniment.attach/1` frame. A failed call, a changed result, or a UID mismatch
closes the socket without a protocol response. Socket ownership, directory
modes, pairing, capabilities, signed workspace grants, and operation-specific
checks remain unchanged. A claimed client kind, PID, path, or UID grants no
authority. This check rejects other OS users but does not defend against a
compromised process running as the current user.

The LaunchAgent sends stdout and stderr to the absolute path `/dev/null`. The
runtime resolves the effective user's home through the OS user record. It
opens the resulting absolute
`/Users/<user>/Library/Logs/Muniment/runtime.log` path itself and writes only
redacted, bounded diagnostics there. Structured records use the macOS unified
log subsystem `ai.muniment.desktop` with category `runtime`. Diagnostics omit
tokens, credentials, prompts, model output, local paths, workspace values,
connection nonces, and peer identifiers. Rotation and retention use the
existing bounded diagnostic policy.

## Amendment – 2026-08-21: Windows activation registration and startup

- Status: accepted

### Task identity and activation

Windows registers one Task Scheduler task at
`\Muniment\Runtime-{user-sid}`. The suffix is the current user's canonical
SID. The task URI and principal use that SID. Its DACL grants that user task
control and grants administrators their standard control.

The principal uses `TASK_LOGON_INTERACTIVE_TOKEN` with least privilege and no
stored password. The task runs only while that user has an interactive session.
Its logon trigger starts the runtime at each login.

The per-user NSIS payload is
`%LocalAppData%\muniment\muniment-runtime.exe`. The per-machine MSI payload is
`%ProgramFiles%\muniment\muniment-runtime.exe`. The registrar resolves the
applicable known folder and writes that absolute path as the task action.
The action passes no shell command and runs the signed executable directly.

A surface requests a start through `IRegisteredTask::Run` after it validates
the task URI, principal, and action. `AllowStartOnDemand` is true, and
`MultipleInstancesPolicy` is `IgnoreNew`. A successful `Run` call creates a new
task instance. `SCHED_E_ALREADY_RUNNING` is also a successful start result.
After either result, the surface uses discovery and bounded readiness checks to
find the runtime that holds the profile instance lock.

The task has no execution time limit. It starts without network or AC-power
conditions and does not stop when power changes. `StartWhenAvailable` is true.
The existing profile instance lock remains the final authority against a second
runtime.

### Restarts, stops, and diagnostics

The task sets `RestartCount` to four and `RestartInterval` to `PT1M`. The
interval defines the period during which Task Scheduler attempts those
restarts. The initial start and four retries allow at most five starts for one
task run. Task Scheduler makes no more retries after that bound.

The runtime also records starts in its owner-only per-user state. Five failed
starts within five minutes record a needs-attention diagnostic and make the
fifth process exit with status zero. If no instance or retry remains, an
explicit surface start clears that crash window before it calls `Run`. This
permits one new bounded task run. Concurrent requests clear it once under the
per-user install lock. A request for a running task does not clear the window.

Status zero means normal idle, logoff, OS shutdown, an installer stop, or the
pre-admission gate below. Task Scheduler does not restart those stops. Status
75 means installed-payload refresh. Any other nonzero status means failure.
Task Scheduler applies the same four-retry bound to status 75 and failed exits.

The runtime writes redacted, bounded diagnostics to
`%LocalAppData%\muniment\logs\runtime.log`. Task registration and launch
failures also appear in the Task Scheduler operational log when enabled.
Diagnostics omit tokens, credentials, prompts, model output, local paths,
workspace values, connection nonces, and peer identifiers.

### Installer ownership and replacement

This Windows scope split supersedes the base decision's unelevated-install rule
for the MSI payload only. The MSI needs elevation, but every task and runtime
process remains per-user.

The NSIS installer runs in the target user's session. It installs the signed
runtime under `%LocalAppData%\muniment`, then creates or updates that user's
stable task. A registration failure rolls back the new files and keeps the
previous verified payload when one exists.

The per-machine MSI installs the signed runtime under `%ProgramFiles%\muniment`.
It does not create a task for an absent user. The installed desktop creates or
updates the stable task when each user first launches it.

Initial registration and desktop-led updates always run in the task principal's
user context. During MSI removal, the elevated uninstaller may update or delete
a matching task through Task Scheduler. An update preserves the user's SID
principal, `TASK_LOGON_INTERACTIVE_TOKEN`, and least-privilege run level. It
never runs the runtime as `SYSTEM` or an administrator. A machine installation
takes precedence while present, so a later NSIS launch cannot redirect the task
from the machine payload.

Both scopes serialize registration and replacement with the per-user install
lock. They update the existing task in place and never register a second task.
The registrar re-reads the task after locking and leaves an equal or newer
compatible registration unchanged.

Replacement follows the installed-payload refresh amendment. The installer
stages and verifies the signed payload, then atomically replaces the payload at
that path without changing the task URI. The old runtime drains durable work and
exits with status 75, so the same task starts the new payload.

A failed readiness check restores the previous verified payload. The installer
first writes an owner-only rollback marker, which makes the new payload exit
successfully before opening state. It stops the task, confirms release of the
instance lock, restores the payload, removes the marker, and calls `Run`.
Any stop, restore, or old-readiness failure leaves the marker and signed payload
in place and reports the failure.

### Removal

The NSIS uninstaller acts only when the current user's URI, principal, and
action match that installation. If a machine installation remains, it repoints
the stable task to that verified payload. Otherwise, it stops and deletes the
task. It removes the NSIS payload only after no running task references it.

The elevated MSI uninstaller enumerates `\Muniment\Runtime-*` tasks. It acts
only when the URI, SID principal, no-argument action, and MSI path all match.
If that user has a verified NSIS payload, it repoints the task to that payload.
Otherwise, it stops and deletes the task. It leaves NSIS-owned tasks unchanged.

Each uninstaller waits for the bounded graceful-stop deadline before it ends a
remaining process. A stop or task-removal failure aborts payload removal and
reports the failure. Removal keeps journals, credentials, CAS data, and
diagnostics.

### Attach admission gate

This amendment grants startup authority only. It grants no Windows attach peer
identity or session authority. Windows activation must not publish the named
pipe attach endpoint before a later peer-identity amendment lands.

Until that amendment lands, surfaces do not call `Run`. An accidental logon or
manual start exits successfully before it opens the instance lock, journal,
CAS, Pi, or attach endpoint. Registration can therefore ship without activating
an unsafe listener.

The Windows contract uses the documented
[`RestartOnFailure`](https://learn.microsoft.com/windows/win32/taskschd/taskschedulerschema-restartonfailure-settingstype-element),
[`TASK_INSTANCES_IGNORE_NEW`](https://learn.microsoft.com/windows/win32/api/taskschd/ne-taskschd-task_instances_policy),
and
[`TASK_LOGON_INTERACTIVE_TOKEN`](https://learn.microsoft.com/windows/win32/api/taskschd/ne-taskschd-task_logon_type)
Task Scheduler settings.

## Amendment – 2026-08-22: Windows attach peer identity

- Status: accepted

The 2026-08-21 attach admission gate remains in force until the **Windows
attach peer identity implementation** slice lands. This amendment defines that
slice and changes no runtime code.

### Endpoint ownership and discovery

The per-user pipe path is
`\\.\pipe\Muniment\attach-v1-<user-hash>`. `user-hash` is the first 128 bits
of SHA-256 over the current user SID's canonical bytes, hex encoded. The hashed
input is the uppercase canonical SID string encoded as UTF-8. Each surface reads
its own process token user SID and derives the path directly. The
hash supports discovery and grants no authority.

The listener acquires the existing profile instance lock before it creates the
pipe. That lock remains the final authority against a second listener. The
listener uses a protected DACL that grants access to the creating user's exact
SID alone. It creates a byte-mode pipe with `PIPE_REJECT_REMOTE_CLIENTS` and
claims the first pipe instance with `FILE_FLAG_FIRST_PIPE_INSTANCE`. Every
instance uses `PIPE_UNLIMITED_INSTANCES` as the instance count. After each
connection, the listener creates an unconnected instance without
`FILE_FLAG_FIRST_PIPE_INSTANCE` before it returns the connected stream. It
reads back the owner and DACL of every instance before it uses that instance.

### Peer admission

For every connection, the listener calls `ImpersonateNamedPipeClient`,
`OpenThreadToken`, and `GetTokenInformation` with `TokenUser`. It copies the
client SID, calls `RevertToSelf`, and compares that SID with its own process
token user SID. It completes this check before it reads or writes any
`muniment.attach/1` frame.

The connecting client opens the pipe with `SECURITY_SQOS_PRESENT` and
`SECURITY_IDENTIFICATION`. Before it reads or writes the first frame, it calls
`GetSecurityInfo` with `OWNER_SECURITY_INFORMATION` on the pipe handle. It
compares the returned owner SID with its own process token user SID.

A failed identity or security call, a result that changes during validation,
or any SID mismatch closes the pipe handle without a protocol response. This
rule includes a failed `RevertToSelf`. A claimed client kind, PID, path, SID,
or pipe name grants no authority. The checks reject other OS users. They do not
defend against a compromised process running as the same user.

## Amendment – 2026-08-23: Windows runtime state directory

The Windows runtime state directory is
`%APPDATA%\ai.muniment.desktop`. The runtime opens the journal and CAS there.
This path equals both directories that the desktop opens through Tauri.

The author checked Tauri 2.11.5 and its `dirs` 6.0.0 dependency. On Windows,
Tauri's `app_data_dir` uses `dirs::data_dir`, which resolves to
`FOLDERID_RoamingAppData`. Tauri's `app_config_dir` uses `dirs::config_dir`,
which also resolves to `FOLDERID_RoamingAppData`. Tauri appends the application
identifier to each known-folder path.

## Amendment – 2026-08-24: Windows attach peer check read order

- Status: accepted

This amendment refines the 2026-08-22 Windows attach peer identity amendment.
It changes no runtime code. The ADR 0012 attach admission gate remains in force
until the Windows attach peer identity implementation slice lands.

### Listener read order

For every connection, the listener reads exactly the unsigned four-byte
frame-length prefix before it calls
[`ImpersonateNamedPipeClient`](https://learn.microsoft.com/windows/win32/api/namedpipeapi/nf-namedpipeapi-impersonatenamedpipeclient).
This prefix is the sole exception to the earlier rule that the peer check
precedes every `muniment.attach/1` frame read.

Microsoft documents
[`ERROR_CANNOT_IMPERSONATE`](https://learn.microsoft.com/windows/win32/debug/system-error-codes--1300-1699-)
when a named-pipe server tries to impersonate before it reads from the pipe.
`ImpersonateNamedPipeClient` uses the security context of the last message that
the server read. Reading the prefix supplies that required read for the
byte-mode pipe.

The prefix carries no protocol authority. The protected owner-only DACL bars
another OS user from opening the pipe, so this read grants no admission or
session authority.

The listener completes the peer check before it reads any frame body or writes
any byte. A rejected peer closes the pipe handle without a protocol response.
A prefix above `MAX_FRAME_LENGTH` also closes the pipe handle without a
frame-body read, byte write, or protocol response.

## Amendment – 2026-08-28: Windows attach admission gate lifted

- Status: accepted

The Windows attach admission gate from the 2026-08-21 Windows activation
amendment is lifted. The two conditions that lifted it are the 2026-08-22
**Windows attach peer identity** amendment and the 2026-08-24 **Windows attach
peer check read order** amendment.

Three checks now guard the endpoint. The listener takes the per-profile
instance lock before it publishes the pipe path. After it creates each pipe
instance, it reads back the owner and owner-only DACL. For each connection, it
reads only the four-byte length prefix, runs the impersonation peer check, and
then reads the frame body.

A surface may now call `IRegisteredTask::Run` after it validates the task URI,
principal, and action. A runtime start with invalid arguments still exits before
it opens runtime state, including the instance lock, journal, CAS, Pi, or attach
endpoint.

## Amendment – 2026-08-29: Windows attach connection route

- Status: accepted

This amendment names the route served by a Windows attach connection. It
changes no runtime code.

### Route identity and read order

After the peer SID check succeeds, the listener calls
`GetNamedPipeClientProcessId` to get the peer process ID. It opens that process
for limited query access and calls `QueryFullProcessImageNameW` to get the peer
image path. This query occurs before the listener reads the frame body, so the
2026-08-24 Windows attach peer check read order still holds.

The expected desktop image is `muniment.exe` beside the installed
`muniment-runtime.exe`. It resolves to
`%LocalAppData%\muniment\muniment.exe` for a per-user installation or
`%ProgramFiles%\muniment\muniment.exe` for a machine installation. A peer image
path that matches the expected installed desktop executable takes the
desktop-client route. Every other peer, including one whose process ID or image
path cannot be read, takes the companion route.

The route check reads no Hello frame field. A claimed client kind in the Hello
frame grants no route authority.

## Amendment – 2026-08-29: macOS attach connection route

- Status: accepted

After the peer UID check succeeds, the listener reads `LOCAL_PEERPID` from the
connected Unix-domain socket. It passes that process ID to `proc_pidpath` to
read the live peer image path. A peer whose absolute image path matches the
expected desktop executable takes the desktop-client route. Every other peer,
including one whose process ID or image path cannot be read, takes the
companion route.

The route check reads no Hello frame field. A claimed client kind in the Hello
frame grants no route authority.

## Amendment – 2026-08-29: Windows desktop-client route handling

- Status: accepted

This amendment defines the exchange for each Windows attach connection route.
It changes no runtime code.

### Route exchanges

The desktop-client route runs the desktop-client admission exchange. It writes
a `reconnect_welcome` frame and then a `DesktopClientAuthorizedGrant` frame.
The companion route keeps the plain `welcome` frame.

The route check reads no Hello frame field. The peer check and the read order
from the 2026-08-24 amendment still run first, before any admission byte.

### Service gap and following lane

The Windows runtime composes no journal, CAS, Pi, or attach service. An admitted
session therefore answers no operation today. Composing those services is the
lane that follows this amendment.

## Amendment – 2026-08-29: Windows attach accept loop wake and stop

- Status: accepted

This amendment defines how the Windows attach accept loop waits and stops. It
changes no runtime code.

### Wait and stop

The loop waits on the pipe connect event and a manual-reset stop event together.
An idle loop wakes only when a connection arrives or the stop event signals. It
never wakes on a timer.

When the stop event signals, the listener cancels the pending pipe connect and
then ends the loop.

### Failed accept bound

Five consecutive failed accepts end the loop with failure, as
`MAX_CONSECUTIVE_FAILED_ACCEPTS` already enforces. A successful accept resets
the consecutive failure count.

## Amendment – 2026-08-30: macOS attach route refinement and handling

- Status: accepted

This amendment supersedes the Hello-field rule in the 2026-08-29 macOS attach
connection route amendment. It defines the exchange for each macOS route and
changes no runtime code.

### Route identity and refinement

After the peer UID check succeeds, the listener reads `LOCAL_PEERPID` from the
connected Unix-domain socket. It passes that process ID to `proc_pidpath` to
read the live peer image path. Every peer whose image does not match the
expected installed desktop executable takes the companion route without a
frame read. This includes a peer whose process ID or image path cannot be read.

For a verified desktop-image peer, the listener reads the first Hello frame to
refine the route. A `desktop` kind takes the approval-presenter route. A
`desktop-client` kind takes the desktop-client route. Every other kind takes
the companion route.

A claimed Hello kind selects a route only after the desktop-image check
succeeds. The claim grants no admission, workspace, operation, or other
authority.

### Route exchanges

The approval-presenter route writes `reconnect_welcome` and a
`DesktopClientAuthorizedGrant`, then runs the approval-presentation session.
That session carries only the approval-presentation authority defined by the
2026-08-13 amendment.

The desktop-client route writes `reconnect_welcome` and a
`DesktopClientAuthorizedGrant`, then serves desktop client requests. Without a
signed workspace, the session carries no workspace scope.

The companion route runs the platform-neutral pairing and authorization
exchange with the activation's live approval waiter and shared connection
registry. A new pairing writes a `welcome` challenge and requires the visible
approval-presentation exchange. After approval, it registers the connection,
writes the authorized grant, and serves companion requests. A valid reconnect
uses `reconnect_welcome` and the stored client credential. If the runtime has no
signed workspace, the approval waiter denies the request and pairing fails
closed.

## Amendment – 2026-09-03: Windows attach route refinement and handling

- Status: accepted

This amendment supersedes the Hello-field rules in the two 2026-08-29 Windows
attach route amendments. It defines the exchange for each Windows route and
changes no runtime code.

### Route identity and refinement

After the peer SID check succeeds, the listener reads the peer process ID and
live image path. Every peer whose image does not match the expected installed
desktop executable takes the companion route. This includes a peer whose
process ID, image path, or expected desktop path cannot be read.

For a verified desktop-image peer, the listener reads the first Hello frame to
refine the route. A `desktop` kind takes the approval-presenter route. A
`desktop-client` kind takes the desktop-client route. Every other kind takes
the companion route.

A claimed Hello kind selects a route only after the desktop-image check
succeeds. The claim grants no admission, workspace, operation, or other
authority. An unreadable, oversized, or malformed first frame opens no session.

### Route exchanges

The approval-presenter route writes `reconnect_welcome` and a
`DesktopClientAuthorizedGrant`, then runs the approval-presentation session.
That session carries only the approval-presentation authority defined by the
2026-08-13 amendment. Failed admission or a second live presenter opens no
presenter session. A disconnect ends the presenter's connection-bound authority.

The desktop-client route writes `reconnect_welcome` and a
`DesktopClientAuthorizedGrant`, then serves desktop client requests. Without a
signed workspace, the session carries no workspace scope.

The companion route runs the platform-neutral pairing and authorization
exchange with the activation's live approval waiter and shared connection
registry. A new pairing writes a `welcome` challenge and requires the visible
approval-presentation exchange. After approval, it registers the connection,
writes the authorized grant, and serves companion requests. A valid reconnect
uses `reconnect_welcome` and the stored client credential. A missing presenter,
denial, expired decision, invalid credential, or missing signed workspace fails
pairing closed.
