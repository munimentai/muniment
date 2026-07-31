# 0009 — Governed local attach protocol for companion surfaces

- Status: accepted
- Date: 2026-07-12
- Context: harness-spec §13 E0; ROADMAP standing gates

## Context

The CLI and editor extension are additional human-operated views of the
desktop-owned session. They must not create another runtime, credential store,
or recovery authority. Pi has one active stream, and accepting a Pi command is
distinct from receiving its later events. Under [ADR 0002](0002-event-sourced-run-journal.md),
only committed journal events own replay and crash recovery; raw Pi frames and
an attached process's memory do not. [ADR 0008](0008-pi-runtime-distribution.md)
makes the desktop supervisor the sole owner of Pi and its stdout dispatcher.

This ADR defines an implementation contract, not a generic plugin API. It adds
no listener, client, companion UI, marketplace work, or protocol implementation.

It governs the native IPC attach service used by the human-operated CLI and
editor companion surfaces. It does **not** govern the browser-control relay in
[harness-spec §6.8](../spec/harness-spec.md), whose MV3 extension pairing,
browser-executable check, connect-tab anchor, and loopback lifecycle form a
separate protocol and threat boundary. Implementations must not reuse this
ADR's socket discovery, same-user peer approval, or connection capability as
browser-relay authorization.

The two protocols do share desktop-owned invariants: current session and
entitlement authorization is checked before work and on every operation;
workspace/domain policy and ask/allow/deny permission decisions cannot be
bypassed; successful effects are represented by committed journal events and
receipt projections rather than raw transport acknowledgements; diagnostics
are redacted; and sign-out, lock, entitlement revocation, or the desktop/owner
kill switch stops new work, revokes ephemeral authority, and closes the
connection. Sharing those invariants does not make either protocol's pairing
credential valid on the other.

## Decision

The desktop exposes a versioned, local, message-oriented attach service. It is
available only while exactly one desktop instance owns the selected profile,
is unlocked, and has a valid session and entitlement snapshot. The service
mediates every operation through the same workspace policy, permission gates,
Pi dispatcher, and journal APIs as the desktop UI.

### Transport, endpoint, and instance ownership

On macOS and Linux the transport is a pathname Unix-domain stream socket. On
Windows it is a byte-mode named pipe with remote clients rejected.

| OS | Endpoint and discovery | Required peer check |
|---|---|---|
| Linux | `$XDG_RUNTIME_DIR/muniment/attach-v1.sock`; `$XDG_RUNTIME_DIR` must exist, be owned by the effective UID, and not be accessible by group/other. The app creates `muniment` as `0700` and the socket as `0600`. There is no `/tmp` fallback. | Read `SO_PEERCRED` on the accepted socket and require its UID to equal the desktop effective UID. Record PID/GID as diagnostics only; PID executable inspection is not an identity guarantee. |
| macOS | `~/Library/Application Support/Muniment/runtime/attach-v1.sock`, in an app-created `0700` directory with a `0600` socket. | Call `getpeereid` and require the peer effective UID to equal the desktop effective UID. macOS supplies UID/GID here, not a trustworthy peer PID; code-signature identity is therefore not claimed by v1. |
| Windows | `\\.\pipe\Muniment\attach-v1-<user-hash>`; `user-hash` is the first 128 bits of SHA-256 over the current user SID's canonical bytes, hex encoded. A client derives it from its own token; it is discovery, not a secret. | Create the pipe with `PIPE_REJECT_REMOTE_CLIENTS` and a protected DACL granting only the current logon user SID (not a broad Users group). Impersonate each connected client, read its `TokenUser`, require an exact SID match with the desktop process token, then revert before parsing data. |

The Unix parent and socket ownership, type, and mode are checked after bind and
before publish. Windows reads back and verifies the pipe DACL. Failure to obtain
peer credentials, impersonate/revert, establish the exact owner, apply/read
back permissions, or reject remote access prevents the service from starting
or rejects that connection. Supporting an OS without an equivalent peer and
owner-only endpoint guarantee requires a later ADR; it must not silently fall
back to TCP.

The endpoint name contains no token. A per-profile native instance lock in the
same owned runtime location is acquired before journal/Pi ownership or endpoint
creation. A second desktop focuses/signals the owner and does not listen or
spawn Pi. While holding the lock, startup may unlink a Unix entry only after
`lstat` confirms it is a socket owned by the current UID, its parent is still
the verified directory, and a connect attempt shows no live listener. It never
follows a symlink. Windows pipe names disappear with the last handle; an
existing pipe means another instance or a fail-closed startup error. Shutdown
stops accepts, revokes connections, closes the listener, and removes the Unix
socket with the same no-follow identity checks.

### Wire contract and handshake

`muniment.attach/1` uses UTF-8 JSON frames prefixed by an unsigned four-byte
big-endian length. Length is validated before allocation; v1 control frames
are at most 1 MiB, nesting/string/collection counts are bounded, invalid UTF-8
or JSON closes the connection, and compression is absent. Artifact bytes use
bounded base64 chunks of at most 256 KiB decoded (and the 1 MiB frame ceiling),
with declared total length and SHA-256; they never turn the socket into an
unbounded byte stream.

Every envelope is one of:

```text
request  { protocol:"muniment.attach/1", request_id, operation,
           capability, idempotency_key?, body }
response { protocol:"muniment.attach/1", request_id, ok:true, body }
error    { protocol:"muniment.attach/1", request_id?, ok:false,
           error:{ code, message, retryable, action?, details? } }
event    { protocol:"muniment.attach/1", subscription_id, event,
           run_id?, run_seq?, body }
```

IDs are opaque UUIDs with bounded text length. `request_id` correlates exactly
one response/error; later stream events use `subscription_id` and journal
sequence, never a delayed second response. Error messages/details are a closed,
redacted schema: no paths outside an approved workspace, environment values,
tokens, raw provider/Pi errors, SQL, or entitlement-signing material.
`idempotency_key` is conditionally required by the operation schema for every
effectful operation (`run.start`, `run.steer`, `run.follow_up`, `run.cancel`,
and `permission.answer`) and is rejected on operations where it has no meaning.

Immediately after transport authentication the client must send, within five
seconds and before any other operation:

```text
hello { protocol:"muniment.attach/1", client:{kind, version},
        supported:{min:1,max:1}, client_nonce }
welcome { selected:1, desktop_version, server_nonce,
          authorization:"pairing_required", approval_challenge }
authorized { capability, expires_at, idle_timeout_seconds, workspace_scopes }
```

The nonces are random and unique per connection. Every connection requires an
explicit, visible desktop approval naming the companion kind and requested
workspace scopes; prior approval never silently authorizes a new process in
v1. `approval_challenge` is a single-use, 128-bit random value delivered over
this already peer-checked connection and confirmed in
the desktop UI; it expires after two minutes and is never accepted from another
connection. Approval returns a random 256-bit connection capability in memory.
It is bound to both nonces, peer OS identity, client kind, profile, approved
workspace scopes, and connection; it is not a bearer token usable on a new
connection and is never written by the companion.

If version ranges do not overlap, the desktop returns
`protocol_incompatible`, its supported min/max and a non-secret action such as
`upgrade_companion` or `upgrade_desktop`, then closes. Within major version 1,
new optional fields and events may be added; clients ignore unknown optional
fields and events. Required semantics or field removal requires a new major
protocol. The desktop supports the current major and, for at least one desktop
release when safe, the immediately previous major; otherwise it gives the same
actionable error before authorization. No compatibility response exposes
profile, session, workspace, entitlement, or runtime state.

### Authorization and revocation

OS peer identity protects against another machine user; it does not prove that
an arbitrary process running as the same user is a trusted companion. Explicit
desktop approval and the connection-bound capability provide that second gate.
The capability lasts no more than eight hours, expires after 15 minutes with no
requests or delivered events, and is rechecked on every request. The desktop
may require approval again after reconnect and always does after sign-out,
screen/app lock, profile switch, entitlement/session revocation, or workspace
grant change. Those transitions atomically stop new work, cancel subscriptions,
zero capabilities/challenges, and close connections. In-flight external effects
are resolved through journal recovery rules, not assumed cancelled.

The desktop alone reads refresh/access tokens, installation keys, provider
keys, the signed entitlement envelope/signing material, and journal/CAS storage.
Companions receive only mediated, redacted projections and never paths or file
handles for the journal database. They cannot initiate native auth or refresh,
copy a virtual/provider credential, or ask the desktop to reveal one.

Same-UID malware can interact with the user's desktop and attempt pairing; v1
cannot establish portable executable identity on all three OSes. The visible
approval, narrow scopes, short connection-bound capability, and audit/journal
provenance reduce this risk, but compromise of the logged-in OS account remains
an explicit deferred risk rather than a claim of sandbox isolation.

### Operations and events

The v1 vocabulary is closed. Each successful command records provenance with
`actor_id`, companion kind/version, OS peer identity (UID or SID; PID only when
available), request/idempotency IDs, profile, and approved workspace. The
desktop maps it to ADR 0002 provenance; it never trusts actor/workspace fields
supplied in a body.

| Operation | Result/events and constraints |
|---|---|
| `thread.list` | Paginated, redacted thread summaries for one approved workspace; bounded `limit` (max 100) and opaque cursor. |
| `thread.open` | A bounded page of the thread projection and cursor, after current workspace membership/grant checks. |
| `run.open` | Opens a bounded projection of an existing `run_id` in an approved workspace. It is read-only and never starts or resumes execution. |
| `run.start` | Starts a human-requested run from bounded text/context in an approved workspace. It requires valid session/entitlement, Pi availability, and an `idempotency_key`, and returns only after the `run.started`/`message.submitted` intent is committed. |
| `run.stream` | Subscribes from a supplied last committed `run_seq`; emits `run.event` projections in order, followed by live committed events. |
| `run.cursor_ack` | Acknowledges `through_run_seq` for one `subscription_id`; this is flow control only and has no journal effect. |
| `run.steer`, `run.follow_up` | Submit bounded human text to the one desktop-owned Pi stream. Acceptance is a committed journal event and does not imply completion. Both require an `idempotency_key`. |
| `run.cancel` | Requests cancellation of one `run_id`. It requires an `idempotency_key`; success means `run.cancel.requested` committed, not that Pi or an external effect stopped. |
| `permission.answer` | Resolves a named pending `gate_id` with `allow` or `deny`, current policy checks, and required `idempotency_key`; the actor/provenance and decision commit before success. |
| `artifact.fetch` | Opens a transfer for a journal `artifact_id` the actor may currently access. Its response returns bounded metadata and a `transfer_id`; no bytes are sent until `artifact.window`. There is no arbitrary path or CAS-hash lookup. |
| `artifact.window` | Acknowledges the prior artifact prefix and grants a bounded next chunk window for one `transfer_id`; this is flow control only. |
| `request.cancel` | Best-effort transport cancellation with exactly one target: `{kind:"request", request_id}` or `{kind:"subscription", subscription_id}`. It cannot request run cancellation; that uses `run.cancel`. |

Server events are `run.event`, `subscription.caught_up`, `permission.pending`,
`artifact.chunk`, `artifact.complete`, `request.cancelled`, `capability.revoked`,
and `stream.closed`. `run.event` is a stable, redacted projection of a committed
journal envelope, not a raw Pi frame. Permission descriptions, tool inputs and
outputs, editor context, and artifacts apply the same secret/path/content
redaction used by the desktop. Withheld data is explicit, not replaced with
invented content.

Every operation re-resolves the run/thread/artifact to the capability's profile
and workspace and rechecks current grants, including after a user-supplied
editor path is canonicalized beneath an approved workspace without following a
TOCTOU-prone stale decision. Text/context, page sizes, concurrent requests,
subscriptions, artifact size and rate all have implementation constants and
return `payload_too_large` or `rate_limited` before unbounded work. Each
connection has bounded outbound queues and byte budgets.

`run.stream` returns a `subscription_id`, the resolved `run_id`, and the first
available/current sequence bounds. Each `run.event` carries that
`subscription_id`, `run_id`, and `run_seq`. The client sends
`run.cursor_ack {subscription_id, through_run_seq}` monotonically after
processing events. The server responds with the accepted
`through_run_seq` and releases that portion of the window. An acknowledgement
below the prior acknowledgement or above the highest sequence sent returns
`invalid_cursor` and closes that subscription with
`stream.closed {code:"invalid_cursor", resumable:true}`. A missing/unknown
subscription returns `subscription_not_found`; it never affects another
stream. The implementation advertises a bounded initial event/byte window in
the `run.stream` response and sends no more events when it is exhausted.

`artifact.fetch` responds with `{transfer_id, artifact_id, total_bytes,
sha256, chunk_bytes, chunk_count}`. A client then sends
`artifact.window {transfer_id, ack_through_chunk, max_chunks}` where the first
acknowledgement is `-1`, later acknowledgements are monotonic contiguous chunk
indexes actually received, and `max_chunks` is between 1 and the advertised
server maximum. Its correlated response confirms `{ack_through_chunk,
granted_chunks}`; the server may then emit at most that many
`artifact.chunk` events carrying `subscription_id:transfer_id`, `artifact_id`,
`chunk_index`, `offset`, decoded `byte_length`, `chunk_sha256`, and base64
`data`. After the final acknowledged window it emits `artifact.complete` with
the transfer ID, total length, and whole-artifact SHA-256. An ack beyond a sent
chunk, a non-contiguous/regressing ack, or an invalid window returns
`invalid_artifact_cursor` and closes only the transfer
with a resumable `stream.closed`; an unknown/expired transfer returns
`transfer_not_found`, requiring a new authorized `artifact.fetch`. No window
means no chunks. `transfer_id` is also the transfer's `subscription_id`, so it
is cancelled with the subscription target form. A receiver rejects a chunk
whose index, offset, decoded length, or hashes contradict the advertised
metadata and closes that transfer without consuming the bytes.

`request.cancel` validates that exactly one target union is present and belongs
to this connection. Unknown or already-terminal targets return
`request_not_found` or `subscription_not_found`. On success its correlated
response says cancellation was accepted. A pending target request terminates
with a correlated `cancelled` error; a subscription/transfer emits
`request.cancelled` and then `stream.closed {code:"cancelled",
resumable:true}`. Cancellation races are resolved by the first terminal state;
the cancel request then reports `already_completed`. Malformed target unions
return `invalid_request` and affect no target.

A consumer that exhausts an acknowledgement window or stops requesting
artifact windows is simply paused. If retained unacknowledged output exceeds
the advertised time/byte budget, it receives `slow_consumer` and
`stream.closed {code:"slow_consumer", resumable:true}` for that stream, while
the desktop continues journaling. Malformed flow-control requests count toward
the connection violation limit; exceeding it closes the connection.

There is no arbitrary Pi command/frame passthrough, database query, filesystem
API, runtime spawn/control, credential/auth flow, background daemon mode,
headless/scripted execution, generic plugin registration, or marketplace/
distribution API. CLI commands require an interactive terminal and user
gesture; editor requests require an active window/user gesture. Server-side
agent access, not this attach service, owns automation.

### Commit, reconnect, and crash semantics

For state-changing requests the desktop validates authorization and policy,
then commits the accepted intent/decision with its required stable idempotency
key and actor provenance before returning success. A Pi acknowledgement is not
a journal commit, and no raw Pi event is exposed before its domain event
commits. Every effectful operation (`run.start`, `run.steer`, `run.follow_up`,
`run.cancel`, and `permission.answer`) stores uniqueness by
`(profile, operation, idempotency_key)` together with a hash of its canonical,
server-resolved input and the committed result/cursor. Canonical input includes
the resolved workspace/run/gate identity and all effect-relevant body fields,
not `request_id`. An exact retry, including `run.start`, returns the original
run/result/cursor without dispatching again. Reuse with any different canonical
input returns non-retryable `idempotency_conflict` and performs no work. These
records are retained for the life of the profile journal; run archival retains
an idempotency tombstone, and profile deletion is the only event that removes
it. After connection authorization and workspace-scope validation, the desktop
checks this durable record before mutable preconditions such as Pi availability
or whether a gate remains pending; thus an exact retry still returns its
original result after state has advanced. Current authorization is never
bypassed by a key. A missing key is `idempotency_key_required` before any intent
or effect.

A subscriber supplies `(run_id, after_run_seq)`. The desktop replays committed
events from `after_run_seq + 1`, signals `subscription.caught_up`, and then
continues live without a gap by using one journal cursor boundary. Duplicate
delivery is allowed; clients deduplicate by `(run_id, run_seq)`. A cursor ahead
of the journal or before retained history fails with `invalid_cursor` or
`cursor_expired` and an action to reopen the run. Disconnect loses only
ephemeral subscriptions/capabilities, never accepted work.

On desktop or Pi restart, the journal reducer reconstructs the run. It never
replays a command into Pi or repeats an external effect merely because a client
reconnects. An accepted intent with no known outcome, or
`tool.effect.started` without a committed terminal outcome, projects to a
committed `run.needs_attention` with a safe machine reason/action. Permission
answers are not guessed or repeated. Slow-consumer closure follows the same
cursor replay path when the client reconnects.

### Threat model

| Threat | Mitigation / residual risk |
|---|---|
| Endpoint spoofing/replacement | Verified owner-only parent/DACL, native peer checks, instance lock, no-follow stale cleanup, post-bind verification, and connection nonces. Same-UID account compromise is deferred as above. |
| Unauthorized local or other-user client | Exact UID/SID match plus explicit desktop approval and connection-bound capability; fail closed when checks are unavailable. |
| Confused-deputy workspace access | Server-derived actor/profile/scope, per-operation current grant and canonical-path checks, no client-asserted provenance. |
| Request/capability replay | Per-connection nonce binding, expiry/revocation, required durable idempotency keys and canonical-input conflict detection. |
| Credential or sensitive-data leakage | No credential/database operations; closed redacted projections/errors; bounded diagnostics and artifact authorization. |
| Malicious/oversized frames | Length-before-allocation framing, strict JSON/schema/depth/count limits, bounded chunks/rates/concurrency, close on malformed input. |
| Slow consumers | Bounded queues, cursor acknowledgements, pull/window artifact chunks, deterministic closure and journal-backed resume. |
| TOCTOU endpoint/path replacement | Owned non-writable parent, `lstat`/no-follow cleanup and read-back checks; workspace resources are re-opened/canonicalized and authorization rechecked at use. |

## Implementation and contract-test slices

1. **Now unblocked:** add pure-core protocol envelope/operation/event types,
   strict codecs and limits, version negotiation, authorization state machine,
   cursor/idempotency semantics, and redaction tests. No native listener or UI.
2. Add platform adapters and contract tests: Unix permission/owner/stale-socket
   fixtures plus Linux `SO_PEERCRED` and macOS `getpeereid`; Windows DACL,
   impersonated `TokenUser`, remote rejection, stale-handle, and second-instance
   fixtures. Each adapter must include negative identity/permission cases.
3. Wire desktop session/workspace policy, journal replay and revocation using a
   fake companion; test commit-before-ack, duplicates, gaps, restart at every
   effect boundary, unknown outcomes, slow consumers, and redaction goldens.
4. Build the interactive E1 CLI, then the E2 editor surface, once their
   repository and CI lanes are decided by [ADR 0011](0011-companion-surface-repo-strategy.md)
   (§13 E0.5). Distribution and marketplace publication remain separately
   owner-gated.

Cross-platform golden byte fixtures cover hello/welcome, every request/event,
errors, unknown optional fields, incompatible versions, malformed/oversized
frames, exact/conflicting retries for every effectful operation, monotonic and
invalid run acknowledgements, artifact window/ack/continuation, both
cancellation target forms, slow-consumer closure, and reconnect. The same
behavior suite runs against an in-memory pure-core session and each native
adapter so platform transports do not change protocol semantics.

## Amendment — 2026-07-30: assistant reply text projection

An authorized companion receives assistant reply text through the existing
`run.event` stream. Each committed `model.stream.delta` envelope projects as a
`run.event` whose `payload.text` field contains the assistant text. The
alternative was to wait for the terminal event, call `thread.open`, and emit
one ACP `agent_message_chunk`. That choice would suppress live output and add a
second read with a race against revocation. The text-bearing `run.event` keeps
text ordered with every other committed event and uses the existing replay,
authorization, and flow-control rules.

`payload.text` contains at most 65,536 UTF-8 bytes. The journal writer splits a
larger model delta at UTF-8 scalar boundaries into ordered `model.stream.delta`
envelopes before commit. It emits no empty envelope. Text counts against a
262,144-byte text budget in each advertised acknowledgement window. The
existing 1 MiB frame ceiling and event-count window also apply. The server
pauses before an event that would exceed either window bound.

One stateful canonical assistant-text projector applies the desktop secret,
path, and content redaction rule across ordered deltas. It retains a bounded
candidate suffix until later text proves that the suffix cannot form part of a
secret. Each redaction rule declares a finite maximum match span. The suffix
bound is one UTF-8 scalar less than the greatest declared span. A rule without
a finite span withholds the remaining assistant content. The projector
withholds every matched secret, including a match split across two or more
deltas. It then assigns released text to its original `model.stream.delta` and
preserves that envelope's `run_seq`. The stream does not pass a pending delta.
The terminal event makes the projector resolve its retained suffix before
delivery. Redaction therefore delays an event when needed. It never adds,
removes, or renumbers a journal cursor.

The projector never truncates, summarizes, or invents replacement text. If the
rule withholds a whole delta, the event retains `payload.withheld:true` and
omits `payload.text`. A partly withheld delta carries only its released,
redacted `payload.text`.

The run stream and `thread.open` disclose the same assistant content.
The canonical assistant-text projection rule enforces this invariant:
both consumers read the output of the same stateful projector.
`thread.open` concatenates its redacted `model.stream.delta` text in `run_seq`
order. It does not read or redact a second source. A withheld stream event
contributes no text to the thread projection.

Each committed `model.stream.delta` envelope has one `run_seq` and produces one
text projection. A resubscription supplies the last committed `run_seq` that
the companion processed. The server rebuilds the projector state through that
sequence without emitting it, then emits projections after that sequence.
Thus, boundary-spanning redaction has the same result without changing cursor
semantics and emits each later text projection exactly once. A companion
persists its cursor only after processing the event. Transport retries may
redeliver an unacknowledged event, and the companion deduplicates it by
`(run_id, run_seq)` as the base reconnect rule requires.

An unauthorized or out-of-scope companion receives an authorization error and
no stream event. Revocation sends only `capability.revoked`, closes the
subscription and connection, and discloses no later text. Reconnect and
resubscribe require fresh approval and current workspace scope. Every failure
path stays fail-closed and never falls back to `thread.open`.

The first implementation slice adds `payload.text` to the attach run-event
type and canonical projector. It also adds journal splitting, shared
`thread.open` projection, byte-window accounting, ACP
`agent_message_chunk` translation, and contract tests. The tests cover secrets
split at every adjacent-delta boundary, across three deltas, beside safe text,
and at the terminal boundary. They also compare live, replayed, resubscribed,
and `thread.open` output. This amendment changes no runtime or companion code.

## Amendment — 2026-07-31: assistant-text redaction rule set

The canonical projector uses the `assistant-text-v1` rule set. The repository's
existing secret-pattern source is [`.gitleaks.toml`](../../.gitleaks.toml).
That file protects committed source, not runtime assistant text. This rule set
adapts its generic API key, JWT, and private-key classes to bounded runtime
matching. It also defines the path and withheld-content classes required by
this ADR. The set is closed and versioned. Changing a pattern or span requires
another amendment and new golden fixtures.

`secret.assignment` matches an ASCII secret label such as `api_key`, `token`,
`secret`, `password`, or `authorization`, its assignment punctuation, and an
ASCII credential value. Its maximum match span is 192 UTF-8 bytes.
`secret.provider-token` matches a provider-specific literal prefix and its
credential alphabet from the Gitleaks default rules enabled by `useDefault`.
Its maximum match span is 512 UTF-8 bytes. The implementation pins the default
rule revision and records it in the `assistant-text-v1` golden fixtures.
`secret.jwt` matches a three-segment compact JWT with base64url segments and
optional trailing base64 padding. Its maximum match span is 8,192 UTF-8 bytes.
`secret.pem-private-key` matches a PEM private-key block from a recognized
`BEGIN ... PRIVATE KEY` line through its matching `END` line. Its maximum match
span is 65,536 UTF-8 bytes. These rules use the corresponding
`generic-api-key`, `jwt`, and `private-key` classes in `.gitleaks.toml` as their
pattern source. The runtime grammar makes every repetition finite and treats a
candidate that exceeds its declared span as an unbounded match.

`path.posix-absolute` matches a slash-rooted absolute path through the next
ASCII NUL, line break, quote, or spacing delimiter. Its maximum match span is
4,096 UTF-8 bytes. `path.windows-absolute` matches a drive-rooted, UNC, or
extended-length Windows absolute path through the same delimiters. Its maximum
match span is 131,068 UTF-8 bytes. Both rules withhold a path outside the
companion's approved workspace.
They release an absolute path inside that workspace because its current grant
already authorizes disclosure.

`content.withheld` matches text in a committed delta that the desktop projection
has already marked as withheld by workspace, connector, artifact, permission,
or content policy. Its maximum match span is the 65,536 UTF-8-byte delta limit.
The projector does not infer new content policy from assistant prose. It
preserves the desktop's committed withheld classification.

The greatest declared span is 131,068 UTF-8 bytes. The projector therefore
retains at most 131,067 UTF-8 bytes, ending only at a UTF-8 scalar boundary.
A configured rule that declares no finite span withholds the remaining
assistant content through the terminal event. An over-span candidate has the
same result. A withheld match contributes no replacement text. If an event has
no released text, the companion receives `payload.withheld:true` with no
`payload.text`. A partly withheld event contains only released text and no
inline placeholder.

Today, `thread.open` returns stored assistant text without assistant-text
redaction. Adopting `assistant-text-v1` changes that authorized disclosure.
The canonical projector reprojects all committed assistant deltas when
`thread.open` reads them, including text committed before this rule set lands.
It does not rewrite the journal. Live, replayed, and historical reads therefore
disclose the same redacted projection under the active rule-set version.

NVIDIA NeMo Guardrails documents a recent-token buffer for violations that
span streamed chunks in [its streaming design][nemo-streaming]. LiveKit's
[LLM output replacement recipe][livekit-output] holds a trailing partial
prefix so a stateful filter can match a tag split across chunks. These sources
support bounded cross-delta retention, but neither defines this rule set.

The first implementation slice adds the pure-core `assistant-text-v1` matcher,
stateful retained-suffix projector, and golden boundary fixtures. It then makes
the attach run stream and `thread.open` read that projector. This amendment
changes no runtime code.

[nemo-streaming]: https://developer.nvidia.com/blog/stream-smarter-and-safer-learn-how-nvidia-nemo-guardrails-enhance-llm-output-streaming/
[livekit-output]: https://docs.livekit.io/reference/recipes/replacing_llm_output/

## Rejected alternatives

**TCP loopback alone.** Loopback limits network reach but supplies no portable
owner identity or filesystem/DACL endpoint boundary; a port can be raced or
occupied. Adding a bearer secret file recreates discovery and permission
problems and remains weaker than native IPC.

**Direct journal/database access.** It bypasses current authorization,
redaction and the single writer, couples clients to SQLite/WAL/schema details,
and creates a second recovery authority.

**Direct Pi attachment.** Pi has one stdio stream, exposes raw implementation
frames, and does not own Muniment session, entitlement, permission, provenance,
or commit semantics. Multiple readers would lose/corrupt ordering.

**Standalone companion runtime.** A second Pi/session/credential store creates
a second trust and effect boundary and permits divergent recovery. It remains a
future owner decision, not v1 fallback behavior.

## Consequences

Companions can share the desktop's governed execution without receiving its
secrets or storage. Native listener work carries platform-specific security
tests, and reconnecting clients must tolerate duplicate journal events. The
portable inability to attest arbitrary same-user executables remains visible
and bounded by approval instead of being hidden behind a claim that local IPC
is inherently trusted.
