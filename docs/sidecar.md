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
as a single newline-terminated JSON record and wait up to the caller's timeout
for exactly one response with the same ID.

The public `JsonRpcRequest`, `JsonRpcSuccess`, `JsonRpcErrorResponse`, and
`JsonRpcErrorObject` types expose the wire envelopes. `JsonRpcTransportError`
distinguishes timeouts, disconnection/I/O, malformed JSON, invalid JSON-RPC
version or response shape, mismatched IDs, and valid remote error responses.
The transport is deliberately synchronous; it does not multiplex calls.

## Scope boundary

This is groundwork for roadmap items 9 and 10, not a mock implementation of
either sidecar. Pi-specific methods, the cloud handshake, virtual keys,
control-plane version negotiation, llama.cpp integration, model download, and
model residency remain deliberately blocked and absent from this module.

## Tests

Run `cargo test --manifest-path src-tauri/core/Cargo.toml`. Integration tests
launch a local stub executable and cover line and JSON-RPC round-trips, protocol
failures, crash restart and backoff, restart-window exhaustion, health-probe
restart, and child reaping on shutdown. They require no GUI, network, sleeps as
assertions, or Docker service.
