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

The grammar below operates on UTF-8 bytes. ASCII literals are case-sensitive
unless a rule says otherwise. `B64` is `[A-Za-z0-9_-]`, `ALNUM` is
`[A-Za-z0-9]`, and `VALUE` is `[A-Za-z0-9_./+=-]`. A token boundary is the
start or end of content, or a byte outside that token's alphabet. At each byte,
the projector selects the longest match, then the rule named first below.
Matches do not overlap. After a rule's fixed prefix matches, failure to find
its required terminator within its maximum span makes an over-span candidate.

`secret.assignment` matches, case-insensitively, one complete label from
`key`, `api_key`, `apikey`, `api-token`, `token`, `secret`, `client_secret`,
`passwd`, `password`, `auth`, `authorization`, or `access_token`. The label
needs an ASCII identifier boundary on each side, where an identifier byte is
`[A-Za-z0-9_-]`. Zero through 20 bytes from `[A-Za-z0-9_. \t-]` may follow.
Next comes one delimiter from `=`, `>`, `:`, `:=`, `=>`, `<=`, `?=`, `,`, or
`||`, then zero through five bytes from space, tab, `=`, single quote, double
quote, or backtick. The value is 10 through 150 `VALUE` bytes. It ends at
content end or before ASCII whitespace, single quote, double quote, backtick,
semicolon, or backslash. The complete match spans at most 192 UTF-8 bytes.

`secret.provider-token` is the following closed alternation. GitHub matches
`gh[pousr]_` plus 36 through 508 `ALNUM` bytes, or `github_pat_` plus 82
through 501 `[A-Za-z0-9_]` bytes. OpenAI matches `sk-` plus 20 through 509
`B64` bytes. Anthropic matches `sk-ant-` plus 20 through 505 `B64` bytes.
Slack matches `xox[baprs]-` plus 10 through 504 `[A-Za-z0-9-]` bytes. Stripe
matches `sk_live_` or `rk_live_` plus 16 through 504 `ALNUM` bytes. Hugging
Face matches `hf_` plus 20 through 509 `ALNUM` bytes. AWS matches `AKIA` or
`ASIA` plus exactly 16 `[A-Z0-9]` bytes. Each alternative spans at most 512
UTF-8 bytes. The preceding byte must be outside `[A-Za-z0-9_]`. The following
byte must be outside the credential alphabet for the selected alternative.

Those provider alternatives derive from Gitleaks 8.30.1 default rules, the
revision pinned by `.github/workflows/secret-scan.yml`. No other default
Gitleaks rule enters `assistant-text-v1`. Gitleaks entropy thresholds, keyword
prefilters, path allowlists, stopwords, and global allowlists do not apply at
runtime. In particular, the test and fixture allowlists in `.gitleaks.toml`
do not release assistant text.

`secret.jwt` matches three `B64` segments separated by literal periods. The
first two segments contain 17 through 2,726 bytes. The third contains zero
through 2,724 bytes followed by zero through two `=` bytes. The match needs a
`B64` token boundary on both sides and spans at most 8,192 UTF-8 bytes.

`secret.pem-private-key` recognizes exactly `PRIVATE KEY`,
`ENCRYPTED PRIVATE KEY`, `RSA PRIVATE KEY`, `DSA PRIVATE KEY`,
`EC PRIVATE KEY`, and `OPENSSH PRIVATE KEY` as header names. It matches a line
`-----BEGIN `, the name, and `-----`, followed by LF or CRLF. The line starts
at content start or after LF. The body has one through 65,460 bytes from
`[A-Za-z0-9+/=]`, LF, and CR. It then matches `-----END `, the same name, and
`-----`, followed by content end, LF, or CRLF. The body permits only LF and
CRLF line endings. The complete match spans at most 65,536 UTF-8 bytes. A
`BEGIN` line with no matching bounded `END` line is an over-span candidate.

The path rules share these terms. `PATH_END` is ASCII NUL, space, tab, CR, LF,
single quote, double quote, backtick, `<`, `>`, `|`, or content end. A path
boundary before a match is content start or one of `(`, `[`, `{`, `:`, `=`,
`,`, `;`, or ASCII whitespace. A path component is one or more UTF-8 scalars
other than `PATH_END`, `/`, or `\`. Dot and dot-dot are components. The
terminating `PATH_END` byte is not part of the match.

`path.posix-absolute` matches `/` followed by zero or more path components
separated by `/`. It needs the path boundary before its leading slash and ends
at `PATH_END`. Its maximum match span is 4,096 UTF-8 bytes.
`path.windows-absolute` matches a drive root `[A-Za-z]:\`, a UNC root
`\\component\component\`, an extended drive root `\\?\[A-Za-z]:\`, or an
extended UNC root `\\?\UNC\component\component\`. A root may be followed by
components separated by `\`. It needs the path boundary before its root and
ends at `PATH_END`. Its maximum match span is 131,068 UTF-8 bytes.
A path candidate that reaches its span without `PATH_END` is over-span.
Both rules withhold a path outside the companion's approved workspace. They
release a path inside that workspace after current canonical scope validation.

`content.withheld` is a metadata rule, not a text pattern. The journal writer
commits each `model.stream.delta` with `content_disclosure:"released"` or
`content_disclosure:"withheld"` and, for withheld content, one reason from
`workspace`, `connector`, `artifact`, `permission`, or `content_policy`.
The projector receives those committed fields beside the delta text. It feeds
no bytes from a withheld delta to the text matchers and withholds the complete
delta. Its maximum match span is the 65,536 UTF-8-byte delta limit. Missing,
unknown, or contradictory disclosure metadata withholds the complete delta.

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

The first implementation slice adds the committed disclosure fields, pure-core
`assistant-text-v1` matcher, stateful retained-suffix projector, and golden
boundary fixtures. It then makes the attach run stream and `thread.open` read
that projector. This amendment changes no runtime code.

[nemo-streaming]: https://developer.nvidia.com/blog/stream-smarter-and-safer-learn-how-nvidia-nemo-guardrails-enhance-llm-output-streaming/
[livekit-output]: https://docs.livekit.io/reference/recipes/replacing_llm_output/

## Amendment — 2026-08-03: tool-effect disclosure

An authorized companion receives tool activity through the existing
`run.event` stream. Each committed tool-effect envelope has this projection:

| Journal event | `run.event` payload |
| --- | --- |
| `tool.effect.started` | `{effect_id, display_name?}` |
| `tool.effect.completed` | `{effect_id}` |
| `tool.effect.failed` | `{effect_id}` |

`payload.effect_id` is nonempty and contains at most 65,536 UTF-8 bytes.
`payload.display_name` is optional and contains at most 65,536 UTF-8 bytes.
The existing 1 MiB frame ceiling and event-count and byte windows also apply.
An absent display name stays absent. An empty or overlong effect ID, or an
overlong display name, makes the projection fail closed. It emits no partial
identity.
No tool input, output, error detail, artifact content, or permission description
enters these payloads.

The run stream and `thread.open` disclose the same display name. Both project
the `display_name` committed on `tool.effect.started`, so live delivery adds no
content beyond the authorized replay. The terminal events carry no display
name. Their `effect_id` associates them with the started event and its released
display name.

Tool-effect projection is stateless because each journal envelope contains all
fields that its stream event discloses. A resubscription supplies the last
processed `run_seq` and rereads committed envelopes after that cursor. It does
not rebuild projector state. Transport retries may redeliver an unacknowledged
event, and the companion deduplicates it by `(run_id, run_seq)`.

The first implementation slice is **attach tool-effect projection**. It adds
the three payload shapes, field validation, byte-window accounting, and
ACP tool-call translation. It also adds contract tests. The tests compare live,
replayed, and resubscribed events with `thread.open`. This amendment changes no
runtime or companion code.

## Amendment — 2026-08-03: named thread continuation on run start

An authorized companion may supply `thread_id` in the `run.start` body. The
field is optional. When present, it is a valid UUID string of at most 36 UTF-8
bytes. A malformed or overlong value returns non-retryable `invalid_request`.
The desktop creates a new run in that thread at its next ordinal. When
the field is absent, the desktop creates a new thread as before. Every accepted
`run.start` response contains `{run_id, thread_id, committed_seq, accepted_at}`.
This lets the first prompt learn its desktop-created thread identity.

The desktop resolves the selected workspace from the authorized capability
before it resolves `thread_id`. The thread must exist, must not have a
`thread.deleted` tombstone, and must belong to that exact profile and workspace.
For a run-less thread, the runtime gets its profile from the durable
`thread.created` provenance defined below. After the first run, existing
first-run ownership remains authoritative. An unknown, tombstoned,
foreign-profile, or wrong-workspace thread returns the non-retryable
`thread_not_found` protocol error. The error uses the same redacted body for
every case and appends nothing. Unlike the desktop chat path, the attach path
never falls back to a fresh thread after this rejection.

The supplied `thread_id` is part of the canonical `run.start` input for
idempotency. An exact retry returns the original acceptance, including its
`run_id` and `thread_id`, without creating or dispatching another run. This
rule also applies when the original request omitted `thread_id`. A retry that
changes whether `thread_id` is present, or changes its value, returns
non-retryable `idempotency_conflict`. Current capability and workspace checks
still run before replay. Later deletion of the accepted thread does not change
the stored acceptance.

The ACP adapter extends its process-local session table with the accepted
`thread_id`. Its first `session/prompt` omits `thread_id`, then stores the
returned value. Later prompts supply that value. If a stored binding gets
`thread_not_found`, the adapter clears only that thread binding. It retries the
prompt once without `thread_id` and with a new idempotency key, then stores the
new accepted thread. It does not retry another error or loop on a second
rejection.

This binding governs journal identity and the pages returned by `thread.open`.
It does not change the run path's model-context behavior. In particular,
continuing a thread does not load earlier journal messages into the model
context or alter the current `session/prompt` context translation.

The first implementation slice is **attach run thread continuation**. It adds
the request field, validates and stamps the selected thread, returns the
accepted thread, and adds exact-replay and rejection contract tests. Both
`RunStartAccepted` structs use `deny_unknown_fields`, so the accept field and
the attach client struct must move in one diff. The adapter binding and stale
binding recovery follow after that wire-compatible slice. This amendment
changes no runtime or companion code.

## Amendment — 2026-08-03: thread creation and profile disclosure

The attach vocabulary adds the effectful `thread.create` operation. Its body is
the empty object `{}`. Unknown body fields return non-retryable
`invalid_request`. The desktop derives the workspace from the authorized
connection and never accepts a client-supplied workspace or profile. The
operation requires `run.write` for that workspace and a valid current session,
entitlement, capability, and workspace grant.

`thread.create` requires an `idempotency_key`. The desktop applies the durable
idempotency rules in this ADR under `(profile, "thread.create",
idempotency_key)`. Its canonical input is the server-resolved workspace. An
exact retry returns the original result without creating another thread. A
retry under another resolved workspace returns non-retryable
`idempotency_conflict`. A missing key returns `idempotency_key_required` before
the desktop appends anything.

The desktop creates the durable thread by appending its `thread.created` event
with the resolved workspace. It stamps the authorized profile identifier as
`provenance.attach_profile` on that event. The existing durable
`thread_events.envelope_json` stores this provenance, so this rule needs no new
column or table. It returns success only after that event commits. The accepted
response body is `{thread_id}`, where `thread_id` is the created thread's opaque
UUID. A commit failure returns a retryable storage error and no accepted
response.

The post-approval `authorized` message adds `profile_id`, which contains the
signed-in profile identifier. The desktop sends that message only after the
user approves the peer and only over the approved connection. Welcome and all
pairing or compatibility errors omit `profile_id`. An unauthorized peer never
receives `authorized`, so same-UID endpoint access alone does not disclose the
identifier. This placement follows the `THREAT_MODEL.md` rule that same-UID
identity does not grant authority.

A run-less thread has no first-run subject owner. Its creator profile is the
`provenance.attach_profile` value on its committed `thread.created` event. For
both `thread.open` and a named-thread `run.start`, the runtime loads that event
from the journal and requires its profile to equal the current authorized
connection's profile. It also requires the event's workspace to equal the
connection's exact workspace. This lookup runs on every request, including a
request from a fresh authorized connection. A missing or malformed creator
profile fails closed as `thread_not_found`.

The change adds no owner column, table, or other schema. `thread.list` omits
run-less threads. The desktop sidebar also omits them and does not call
`subject_owns_first_run` until a first run exists. The first run stamps its
actor through existing provenance. Existing first-run ownership then governs
desktop reads, later attach operations, and sidebar visibility.

Implementation proceeds in three slices. First, **authorized profile
disclosure** adds `profile_id` after approval and its negative-handshake tests.
Second, **attach thread creation** adds `thread.create` routing, journal commit,
idempotency, visibility, and contract tests. Third, **ACP session record** uses
the disclosed profile identifier and created thread ID when `session/new`
writes the record from ADR 0022. This amendment changes no code.

## Amendment — 2026-08-04: companion revocation

Companion revocation is a local decision by the attach owner. It removes the
selected companion's persisted client credential from
`attach-client-credentials.json`. The attach owner owns this decision and the
credential store even after the ADR 0012 service extraction. The desktop only
requests the decision through an owner API.

The attach owner blocks request admission for the selected credential while it
persists the removal. After success, it invalidates every connection capability
authenticated with that credential before request admission resumes. No new
request can cross that boundary. A request admitted before the boundary follows
the existing journal recovery rules. The owner then cancels the affected
subscriptions. If persistence fails, revocation fails, authority remains valid,
request admission resumes, and the owner emits no revocation event.

The owner emits exactly one `capability.revoked` event on each affected live
connection. It emits the event only after it persists removal of the client
credential that authenticated that connection. The event body is
`{capability, reason:"companion_revoked"}`. It names that connection's current
capability and discloses no companion, profile, workspace, or credential data.
At authorization, the owner assigns each connection a random UUID named its
`connection_event_id`. The revocation event uses that UUID as its required
`subscription_id`. The UUID stays fixed for the connection lifetime, does not
identify a subscription, and is invalid as a `request.cancel` target.

The owner delivers the complete framed event after it stops accepting requests.
It closes the connection only after the transport write and flush complete.
It sends no `stream.closed` event for the cancelled subscriptions. A connection
that is absent or already closed receives no event. Expiry, idle timeout,
sign-out, lock, and profile switch do not emit the event. Entitlement revocation,
session revocation, workspace grant change, and transport failure also do not
emit it. These operations emit the event only if they also remove that
connection's persisted client credential.

Removing the credential makes every later authentication attempt with it fail
closed. A revoked companion can reconnect only through a fresh visible
approval that creates a new client credential and connection capability.

Implementation follows in two slices. **Desktop companion management** adds
the list-and-revoke surface backed by the attach owner API. **Attach companion
revocation emitter** adds atomic live invalidation, event delivery, connection
closure, and contract tests. This amendment changes no code.

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
