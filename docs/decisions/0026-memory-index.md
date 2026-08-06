# 0026 — Build the memory index in-house

- Status: accepted
- Date: 2026-08-06
- Context: harness-spec §17

## Context

Muniment needs bounded retrieval from the visible Markdown files in the
Muniment Home. The index must remain a disposable cache, and the signed desktop
binary must contain the complete retrieval runtime.

Candidate memory frameworks fail both constraints. Every candidate owns its
store, which contradicts files as the source of truth. Every candidate also
runs Python or Node, which cannot ship inside the signed desktop binary.

## Decision

Build the memory index and retrieval path in-house. Phase one uses SQLite FTS5
for lexical search and adds no dependency. The design follows harness-spec §17
for limits, stable selection, secret filtering, tool delivery, and receipts.

Three capabilities already in the repository keep this work small:

1. `rusqlite` with its `bundled` feature already compiles FTS5 and extension
   loading into the binary.
2. The ONNX runtime already links through the speech stack.
3. The pinned, verified model acquisition path already ships production
   artifacts.

Phase two can use the existing ONNX and artifact foundations for embeddings.
The model artifact must follow the established pinning, verification,
acquisition, and atomic publication rules.

## Consequences

- Users can delete and rebuild the index without losing memory.
- The desktop adds no Python or Node runtime for memory retrieval.
- Phase one adds no dependency and remains lexical only.
- Phase two adds vector search and one pinned embedding artifact.

## Rejected alternatives

- Adopt a memory framework. Rejected because it owns the store and requires a
  Python or Node runtime.
- Make the index the primary store. Rejected because users could not inspect,
  edit, sync, or recover their durable context as ordinary files.
