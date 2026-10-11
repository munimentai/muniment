# 0032 — The record's MCP server speaks the 2026-07-28 and 2025-11-25 revisions

- Status: accepted

## Context

The agent surface of the local product is one read-only SQL tool plus propose
and commit, reached from Claude Code and from the desktop's own Pi. Claude Code speaks
the Model Context Protocol revision of 2026-07-28. That revision removed the
`initialize` handshake and protocol-level sessions, put the protocol version
and the client's capabilities in `_meta` on every request, added
`server/discover`, required `resultType` on every result, and replaced
server-initiated requests with multi round-trip results. Pi's own MCP client
speaks the session revisions, 2025-11-25 the newest, which open with
`initialize`.

## Decision

`muniment-cli mcp` is a stdio server for both. A request that carries
`io.modelcontextprotocol/protocolVersion` in `_meta` takes the 2026-07-28
path: it answers `server/discover`, `tools/list` and `tools/call`, and
nothing else. A request without `io.modelcontextprotocol/clientCapabilities`
there is invalid params, and one naming another version gets
`UnsupportedProtocolVersion` with the one supported version. Every result
carries `resultType` and the server's identity in `_meta`. `tools/list` comes
back in one fixed order with `ttlMs` and a private `cacheScope`.

An `initialize` opens a session on the connection in the revision it names,
2025-11-25, 2025-06-18 or 2025-03-26, or in 2025-11-25 when it names another.
The session answers `ping`, `tools/list` and `tools/call` with the same tools,
without `resultType` or the cache fields. A request with no `_meta` before
`initialize` is invalid params.

The three tools are `sql`, `propose` and `commit`. A `propose` that lacks a
required field answers `input_required` with one `elicitation/create` form
request for that field, built from the kind's property schema, when the
client declares the elicitation capability. The retry carries the answer in
`inputResponses` and the server merges it into the operation. No
`requestState` travels, because the retry carries the whole operation and a
tampered retry can do nothing the client could not send outright. Without
the capability, and on a session, where the server sends no requests of its
own, the missing field is a tool error that names it.

The server holds no record state, and a session holds only the client's
name. Each call opens the runtime over attach on first use as the `cli`
companion, and the runtime holds the record, the proposals and the SQL tool.
The desktop's Pi reaches the same binary through the `record` entry the
runtime writes into the agent directory's `mcp.json`, with `direct` exposure
so the three tools reach the model as its own.

## Consequences

A client on 2024-11-05 cannot use the server. Claude Code and the desktop's
Pi are the two clients the product names, and each speaks a revision the
server answers. Pi cannot answer an `input_required` result, so a propose
from Pi with a missing field asks again through the model. The CLI binary joins the bundle beside the runtime on every
platform, and the runtime finds it as its sibling. Windows has no companion
client yet, so the server answers there with a platform error until one
exists.

## References

- [ADR 0031](0031-company-record.md), the record the server reaches.
- [SPEC.md](../../SPEC.md), The local graph.
- The protocol revision at `modelcontextprotocol.io/specification/2026-07-28`.
