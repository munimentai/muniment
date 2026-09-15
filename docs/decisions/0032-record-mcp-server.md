# 0032 — The record's MCP server speaks the 2026-07-28 revision only

- Status: accepted

## Context

The agent surface of the local product is one read-only SQL tool plus propose
and commit, reached from Claude Code and from the desktop's own Pi. Both
clients speak the Model Context Protocol revision of 2026-07-28: Claude Code
natively, and Pi through `pi-mcp-adapter`, which pins a `protocolVersion` per
server. That revision removed the `initialize` handshake and protocol-level
sessions, put the protocol version and the client's capabilities in `_meta`
on every request, added `server/discover`, required `resultType` on every
result, and replaced server-initiated requests with multi round-trip results.

## Decision

`muniment-cli mcp` is a stateless stdio server for that revision and no
earlier one. It answers `server/discover`, `tools/list` and `tools/call`, and
nothing else. A request without `io.modelcontextprotocol/protocolVersion` and
`io.modelcontextprotocol/clientCapabilities` in `_meta` is invalid params. A
request naming another version gets `UnsupportedProtocolVersion` with the one
supported version. An `initialize` request gets method not found with the
supported version in its data, so a legacy client can show why. Every result
carries `resultType` and the server's identity in `_meta`. `tools/list` comes
back in one fixed order with `ttlMs` and a private `cacheScope`.

The three tools are `sql`, `propose` and `commit`. A `propose` that lacks a
required field answers `input_required` with one `elicitation/create` form
request for that field, built from the kind's property schema, when the
client declares the elicitation capability. The retry carries the answer in
`inputResponses` and the server merges it into the operation. No
`requestState` travels, because the retry carries the whole operation and a
tampered retry can do nothing the client could not send outright. Without
the capability the missing field is a tool error that names it.

The server holds no state. Each call opens the runtime over attach on first
use as the `cli` companion, and the runtime holds the record, the proposals
and the SQL tool. The desktop's Pi reaches the same binary through the
`record` entry the runtime writes into the agent directory's `mcp.json`
with `protocolVersion` pinned, and the adapter moves to a release that knows
the revision.

## Consequences

A client on an earlier revision cannot use the server. Claude Code and the
desktop's Pi are the two clients the product names, and both speak this
revision. The CLI binary joins the bundle beside the runtime on every
platform, and the runtime finds it as its sibling. Windows has no companion
client yet, so the server answers there with a platform error until one
exists.

## References

- [ADR 0031](0031-company-record.md), the record the server reaches.
- [SPEC.md](../../SPEC.md), The local graph.
- The protocol revision at `modelcontextprotocol.io/specification/2026-07-28`.
