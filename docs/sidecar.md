# Sidecar process supervision

`muniment_core::sidecar` is the protocol-neutral lifecycle layer shared by the
future Pi RPC and llama.cpp integrations. It has no Tauri, GUI, or network
dependency and uses only Rust's portable standard-process APIs.

## What exists

`SidecarSupervisor` starts a configured program with arguments, environment,
and piped stdin/stdout/stderr. `SidecarIo` exposes each stream as line-oriented
handles; the handles continue to work when a replacement process is started.
A caller-supplied probe closure can perform the sidecar's own protocol. Its
typed result is `ProbeOutcome::Loading` while startup work is in progress and
`ProbeOutcome::Ready` once the process can serve requests; errors represent an
unhealthy process.

Stderr is best-effort diagnostic output. `SidecarConfig::stderr_capacity`
(default: 256 lines) bounds it at the producer: when full, the oldest line is
evicted before a new one is retained, even if no caller reads stderr. Reading
`SidecarIo.stderr` returns retained lines in arrival order, skipping lines that
were evicted before they could be read. `SidecarSupervisor::recent_stderr()`
returns a non-consuming snapshot in the same order. The ring is cleared when a
replacement child is spawned, so snapshots never mix process generations.

Unexpected exits, spawn errors, and failed health checks restart the child
using capped exponential backoff. `RestartPolicy` limits restarts within a
rolling time window, after which status becomes `Failed`. Other observable
states are `Starting`, `Healthy`, `Restarting`, and `Stopped`.

Each spawned generation stays `Starting` while probes report `Loading`. Probes
begin immediately and repeat at `health_interval` (default: 5 seconds). The
first `Ready` emits `Healthy` exactly once; later probe errors use the ordinary
health-failure restart path. Loading does not consume restart attempts, but it
is bounded by `startup_timeout` (a nonzero default of 60 seconds). If readiness
does not arrive, the child is stopped and restarted under the existing rolling
budget and capped backoff.

Shutdown first closes stdin so a cooperative child can finish. If it has not
exited by the configured deadline, the supervisor kills and reaps it. Dropping
the supervisor performs the same shutdown, preventing orphan children.

## Lifecycle events

`SidecarSupervisor::subscribe` returns a standard-library `mpsc::Receiver` of
ordered `SidecarEvent` values. A new receiver first replays the transitions
already emitted by that supervisor, including generation-tagged `Starting`,
and then receives live transitions. This avoids races between the immediately-started
worker and callers attaching their first receiver. Multiple receivers each see
the complete stream; dropping one does not affect the others.

Each event contains the new `SidecarStatus`. Restart and failure events retain
their cause as a process exit (code and signal where available), process wait
error, spawn error, or health-probe failure message. A `Restarting` event also
contains its attempt number within the rolling restart window and selected
backoff. `Starting` and `Healthy` identify the active I/O generation, producing
that order for a ready process. `Stopped` records a
requested shutdown. The existing `status()` API remains the current snapshot
of this same event stream.

Process-exit, process-wait, health-probe, and startup-timeout causes include up
to the last 20 retained stderr lines. This tail is present on both restart events and the final
`Failed` event when the restart budget is exhausted. Startup-timeout causes also
record the configured timeout, distinguishing slow loading from a hard probe
error.

Subscriptions use unbounded channels. Publishing therefore never waits for a
slow receiver; queued events remain available until that receiver consumes or
drops them. Disconnected senders are pruned while publishing. After `Stopped`
or `Failed` is delivered, all receiver channels disconnect.

## Signed-in Pi chat

Chat requests a strict `{ gatewayUrl, virtualKey, model?, receiptUrl }` grant
from `POST /v1/desktop/chat/config` at the configured control-plane issuer,
using the refreshed OIDC access token. Both URLs must be HTTPS and unknown
members are rejected. The scoped LiteLLM virtual key and gateway URL exist
only in the supervised Pi child's environment; neither is returned to the
webview nor written to the run journal.

Each local run launches a new persistent Pi conversation beneath the app data
directory's owned `pi-sessions` root. Launch always supplies `--session-dir`;
an explicit reopen additionally supplies a session file that Rust has
canonicalized, verified as a regular file, and proven to remain beneath that
root. Journal values are root-relative JSONL filenames, never arbitrary paths.

After Pi accepts the prompt, the desktop reads `data.sessionFile` from the
pinned 0.73.1 `get_state` response, validates it against the owned root, and
appends one `runtime.pi_session.bound` event containing the local run id and
non-secret filename locator. Reducer replay rejects malformed, duplicate, or
conflicting bindings. Pi JSONL and its contents never cross Tauri and never
reconstruct chat text, receipts, or provenance; the append-only run journal
remains authoritative for those projections.

Startup marks interrupted nonterminal runs `run.needs_attention`; an unresolved
permission gate remains pending and is never converted into a resumable run.

An interrupted run with no pending permission gate may be explicitly resumed by
the signed-in owner when its journal-bound session file still validates beneath
the app-owned session directory. Resume obtains fresh native authentication and
a new scoped chat grant, launches pinned Pi with both `--session-dir` and the
validated `--session`, records `run.resumed` on the existing run, then sends one
fixed continuation request. It never resubmits the protected original prompt,
imports Pi JSONL into rendered history, replays journaled tool effects, or
automatically answers a permission request. Failures remain recoverable and do
not allow caller-provided session paths.
The presence of a valid binding does not submit a prompt, resolve a permission
gate, or repeat a tool effect. Automatic continuation is deliberately deferred
to the next resume slice.

After Pi emits `agent_end`, the desktop posts `{ runId }` to `receiptUrl` with
the OIDC access token. Only that authoritative response supplies optional
route, model, cost, time, and capability provenance. Pi event members are not
treated as billing or routing authority.

## JSON-RPC framing

`JsonRpcTransport` provides synchronous, one-request-at-a-time JSON-RPC 2.0
over a supervisor's `SidecarIo`. `call` allocates a numeric request ID;
`call_with_id` accepts a numeric or string ID. Both serialize one typed request
as a single newline-terminated JSON record and wait up to the caller's overall
timeout for a response with the same ID. Server notifications received first
are never treated as responses: plain calls skip them, while
`call_with_notifications` delivers each typed notification to its callback in
arrival order before returning the final result. A response with another ID is
still a protocol error.

`notify` sends a standard JSON-RPC notification containing `jsonrpc`, `method`,
and optional `params`, but no `id`, and does not wait for a response. Reader
lines are normalized at the process boundary: CRLF records have their trailing
carriage return removed and blank keepalive lines are dropped.

Each spawned process has a distinct I/O generation. Output is tagged at the
process boundary, and readers discard frames from earlier generations after a
restart. A JSON-RPC call is bound to the generation whose stdin accepted its
request, so a crash interrupts that call and output still draining from the
dead process cannot fail a later call or reach its notification callback.

The public `JsonRpcRequest`, `JsonRpcNotification`, `JsonRpcSuccess`,
`JsonRpcErrorResponse`, and `JsonRpcErrorObject` types expose the wire
envelopes. `JsonRpcTransportError` distinguishes timeouts, disconnection/I/O,
malformed JSON, invalid JSON-RPC version or response shape, mismatched IDs, and
valid remote error responses. The transport is deliberately synchronous; it
does not multiplex calls.

### Cancellation and late responses

A transport configured with `with_cancel_method` supports cancellable calls
through a cloneable `JsonRpcCancellationToken`. Another thread can signal the
token while a call is blocked; the transport promptly sends the configured
JSON-RPC cancellation notification with the request ID and returns
`JsonRpcTransportError::Cancelled`. The notification method is deliberately
peer-specific rather than fixed to the LSP convention.

Timed-out and cancelled request IDs are retained in a bounded, generation-aware
queue. If the live sidecar later sends either a success or error response for
one of those abandoned IDs, the transport silently discards it and continues
waiting for the current call. Entries from replaced process generations are
purged, and the oldest entry is evicted at the fixed capacity, preventing an
unresponsive peer from growing transport state without bound.

For supervised JSON-RPC peers, `JsonRpcTransport::health_probe` turns an
`Arc<JsonRpcTransport>` into the closure accepted by `SidecarSupervisor::spawn`.
Pi uses the public `PiRpcWiring` handle instead: retain it beside the supervisor,
pass `wiring.readiness_probe(...)` to `spawn`, and obtain that exact sole stdout
dispatcher through `wiring.transport()` after startup readiness.
The caller supplies the ping method name and per-call timeout; the helper sends
the method with no parameters and maps any remote error, timeout, or transport
failure to a descriptive health-check error. The probe uses the same call lock
as application calls, so it cannot consume their responses or notifications.
If an application call holds the lock, the probe promptly reports healthy
without sending a ping. The call's own timeout or cancellation bounds a hang;
after it releases the lock, the next probe performs a real ping. When no call is
in flight, an unanswered ping still fails after the probe timeout and follows
the normal supervisor restart path.

## Scope boundary

Desktop ASR is deliberately outside the sidecar boundary. As decided in
[ADR 0004](decisions/0004-desktop-asr-runtime.md), `muniment_core` will own
microphone PCM and invoke the pinned sherpa-onnx v1.13.2 C API in process with
the pinned Parakeet-TDT 0.6B v3 INT8 offline artifact. No ASR socket or child
process is introduced; only utterance-final transcript text proceeds to the
local Gemma dictation-polish contract, and voice bytes never enter a network
client, Pi, llama-server, telemetry, or crash reports. Model acquisition also
remains outside sidecar supervision: [ADR
0005](decisions/0005-asr-model-lifecycle.md) selects a Rust-native first-use
install with complete-set verification and atomic publication. Acquisition is
the only ASR-related network boundary, and model bytes never cross the webview.
Native desktop packaging and target-hardware validation remain follow-up work.

`muniment_core::llama` owns the local llama.cpp boundary. The typed resident
descriptor pins the Gemma artifact identity, stable API alias, and context
limit selected in [ADR 0003](decisions/0003-resident-gemma-model.md).
`LlamaServerConfig` turns an explicit executable, installed artifact path, and port into
separate process arguments and always supplies `--host 127.0.0.1` (or the
explicit IPv6 loopback `::1`). `LlamaServer` delegates spawning, restart
budget/backoff, stderr diagnostics, and shutdown to `SidecarSupervisor`; it is
not a second process manager. Dropping it therefore retains the supervisor's
forced child cleanup guarantee.

The corresponding base URL is deliberately restricted to numeric loopback
addresses and plain HTTP. Wildcard, LAN, hostname, path-bearing, and HTTPS URLs
are rejected. This matters even though llama-server currently defaults to
loopback: its public health API does not perform an API-key check, and a future
upstream default must not silently widen local access.

The bounded `GET /health` probe follows llama.cpp's documented states: HTTP
503 with its `Loading model` error is `ProbeOutcome::Loading`, while HTTP 200
with `{"status":"ok"}` is `ProbeOutcome::Ready`. Connection failures, other
statuses, and malformed or unexpected envelopes are probe errors and enter the
existing supervisor restart path. Diagnostics identify the status or parsing
failure but never copy the response body, which may contain user data.

`LlamaChatClient` uses that same validated loopback boundary for one bounded,
synchronous `POST /v1/chat/completions` call. Its typed request supplies model,
system/user messages, maximum output tokens, and temperature, and always sends
`stream: false`. The client applies a finite caller-selected timeout, bounds the
body before JSON decoding, and accepts exactly one non-empty assistant message.
Available prompt, completion, and total token counts are returned with the text.
Status, transport, size, JSON, and response-shape failures are typed diagnostics
that never include prompt or raw response-body content.

The dictation-polish role wraps this boundary with typed transcript input and
polished-text output. Its fixed, zero-temperature prompt removes speech fillers
and false starts, applies explicit self-corrections, and corrects mechanics while
requiring meaning and detail to be preserved. Streaming remains out of scope.
UI wiring and cloud or Pi behavior are also deferred, as are virtual keys and
control-plane version negotiation.

The routing-classifier role is a separate typed, zero-temperature contract. It
returns only `task_type` and `difficulty`, plus llama.cpp token usage. The closed
task vocabulary is `general`, `analysis`, `code-plan`, `code-edit`, `extraction`,
`vision`, and `long-context`; difficulty is `low`, `medium`, or `high`. Its prompt
serializes the user's request as one JSON string explicitly identified as
untrusted data. JSON escaping keeps request-controlled tag-like text, quotes,
and line breaks inside that string, making the framing unambiguous and testable;
this separation reduces ambiguity but does not eliminate prompt-injection risk.
The classifier requires one compact JSON object. Application-side decoding
rejects malformed JSON, missing or extra fields, unknown labels, and surrounding
prose without including the prompt or raw assistant response in diagnostics.

This is classification evidence, not a routing decision. The response cannot
name a model, provider, route, policy, entitlement, capability, or cost; those
decisions belong to gateway policy as specified in §5 of the harness spec.
Carrying these labels as request metadata and wiring the role into Pi or the
gateway are explicitly deferred.

Before producing launch arguments, the core requires the installed artifact to
be a regular file with the descriptor's exact byte size and SHA-256. Hashing is
streamed, and missing, unreadable, wrong-size, and digest-mismatch failures do
not disclose file contents or installation paths. llama-server receives the
model path and `muniment-resident-gemma` alias as separate arguments; resident
chat requests always use that alias rather than a caller-selected model name.

Artifact acquisition and update/rollback policy remain out of scope.
Dictation-polish and routing-classifier contract evaluation use deterministic
golden fixtures and mock HTTP responses; CI never loads the model.

Supervisor lifecycle events and stderr are diagnostic telemetry, not durable
user-session history. Pi integration will translate only user-relevant domain
facts into the append-only journal defined by [ADR 0002](decisions/0002-event-sourced-run-journal.md).
That journal—not the supervisor replay buffer, raw JSON-RPC transcript, or
current status—is the future source for resume, receipt, and mobile-relay
projections; none of those projections is implemented here.

## Tests

Run `cargo test --manifest-path src-tauri/core/Cargo.toml`. Integration tests
launch a local stub executable and cover line and JSON-RPC round-trips, protocol
failures, crash restart and backoff, restart-window exhaustion, health-probe
restart, and child reaping on shutdown. They require no GUI, network, sleeps as
assertions, or Docker service.
# Pi chat frames

The pinned Pi runtime uses its own LF-delimited JSON protocol, not the generic
JSON-RPC 2.0 transport. `PiRpcTransport` is the sole stdout reader: correlated
command acknowledgements return to the caller while interleaved agent events
remain ordered for subscribers. Chat frames are narrowed by `pi_chat` into
prompt acceptance, text delta, completion receipt, cancellation, or failure.
Raw upstream errors are intentionally discarded at that boundary.

Prompt text and gateway credentials are process-memory-only inputs. Journal
events contain only prompt acceptance, renderable deltas, terminal state, and
the authoritative receipt fields. Missing receipt fields remain absent.
