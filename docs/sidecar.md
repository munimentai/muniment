# Sidecar process supervision

`muniment_core::sidecar` is the protocol-neutral lifecycle layer shared by the
future Pi RPC and llama.cpp integrations. It has no Tauri, GUI, or network
dependency and uses only Rust's portable standard-process APIs.

## What exists

`SidecarSupervisor` starts a configured program with arguments, environment,
and piped stdin/stdout/stderr. `SidecarIo` exposes each stream as line-oriented
handles; the handles continue to work when a replacement process is started.
A caller-supplied health closure can perform the sidecar's own ping protocol.

Unexpected exits, spawn errors, and failed health checks restart the child
using capped exponential backoff. `RestartPolicy` limits restarts within a
rolling time window, after which status becomes `Failed`. Other observable
states are `Starting`, `Healthy`, `Restarting`, and `Stopped`.

Shutdown first closes stdin so a cooperative child can finish. If it has not
exited by the configured deadline, the supervisor kills and reaps it. Dropping
the supervisor performs the same shutdown, preventing orphan children.

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

The public `JsonRpcRequest`, `JsonRpcNotification`, `JsonRpcSuccess`,
`JsonRpcErrorResponse`, and `JsonRpcErrorObject` types expose the wire
envelopes. `JsonRpcTransportError` distinguishes timeouts, disconnection/I/O,
malformed JSON, invalid JSON-RPC version or response shape, mismatched IDs, and
valid remote error responses. The transport is deliberately synchronous; it
does not multiplex calls.

For supervised JSON-RPC peers, `JsonRpcTransport::health_probe` turns an
`Arc<JsonRpcTransport>` into the closure accepted by `SidecarSupervisor::spawn`.
The caller supplies the ping method name and per-call timeout; the helper sends
the method with no parameters and maps any remote error, timeout, or transport
failure to a descriptive health-check error. The probe uses the same call lock
as application calls, so it cannot consume their responses or notifications.
Consequently, a probe waits behind a long-running call (bounded by that call's
timeout) and stalls the supervisor loop while it does so. Keep probe timeouts
small relative to `health_interval`.

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
