# Sidecar process supervision

`muniment_core::sidecar` is the protocol-neutral lifecycle layer shared by the
future Pi RPC and llama.cpp integrations. It has no Tauri, GUI, or network
dependency and uses only Rust's portable standard-process APIs.

## What exists

`SidecarSupervisor` starts a configured program with arguments, environment,
and piped stdin/stdout/stderr. `SidecarIo` exposes each stream as line-oriented
handles; the handles continue to work when a replacement process is started.
A caller-supplied health closure can perform the sidecar's own ping protocol.

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

Shutdown first closes stdin so a cooperative child can finish. If it has not
exited by the configured deadline, the supervisor kills and reaps it. Dropping
the supervisor performs the same shutdown, preventing orphan children.

## Lifecycle events

`SidecarSupervisor::subscribe` returns a standard-library `mpsc::Receiver` of
ordered `SidecarEvent` values. A new receiver first replays the transitions
already emitted by that supervisor, including the initial `Starting`, and then
receives live transitions. This avoids races between the immediately-started
worker and callers attaching their first receiver. Multiple receivers each see
the complete stream; dropping one does not affect the others.

Each event contains the new `SidecarStatus`. Restart and failure events retain
their cause as a process exit (code and signal where available), process wait
error, spawn error, or health-probe failure message. A `Restarting` event also
contains its attempt number within the rolling restart window and selected
backoff. `Healthy` identifies the active I/O generation. `Stopped` records a
requested shutdown. The existing `status()` API remains the current snapshot
of this same event stream.

Process-exit, process-wait, and health-probe causes include up to the last 20
retained stderr lines. This tail is present on both restart events and the final
`Failed` event when the restart budget is exhausted.

Subscriptions use unbounded channels. Publishing therefore never waits for a
slow receiver; queued events remain available until that receiver consumes or
drops them. Disconnected senders are pruned while publishing. After `Stopped`
or `Failed` is delivered, all receiver channels disconnect.

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

This remains transport groundwork for roadmap items 9 and 10, not a mock
implementation of either sidecar. There are still no Pi-specific methods, no
network or cloud handshake, and no llama.cpp integration. Virtual keys,
control-plane version negotiation, model download, and model residency also
remain deliberately blocked and absent from this module.

## Tests

Run `cargo test --manifest-path src-tauri/core/Cargo.toml`. Integration tests
launch a local stub executable and cover line and JSON-RPC round-trips, protocol
failures, crash restart and backoff, restart-window exhaustion, health-probe
restart, and child reaping on shutdown. They require no GUI, network, sleeps as
assertions, or Docker service.
