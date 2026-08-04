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
second journal.

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

## References

- [ADR 0008 — Pi runtime distribution](0008-pi-runtime-distribution.md)
- [ADR 0009 — companion attach protocol](0009-companion-attach-protocol.md)
- [ADR 0011 — companion surface repository strategy](0011-companion-surface-repo-strategy.md)
- [Harness specification](../spec/harness-spec.md)
- [Native authentication contract](../auth.md)

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
