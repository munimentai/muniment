# 0024 — Trusted code-diff producer and journal boundary

- Status: accepted
- Date: 2026-08-01
- Context: ADR 0002, ADR 0019, ADR 0020

## Context

ADR 0020 defines `code-diff/1` and its renderers. It leaves production,
approval, and replay for this decision.

Pi currently reports tool identifiers, names, and error state. It does not
report a proposed file operation or a diff. The journal records the same
limited tool state. An agent-authored patch cannot serve as trusted approval
evidence.

## Decision

### Trusted producer

The Tauri core is the sole trusted `CodeDiff` producer. Pi remains an
untrusted source of proposed file operations.

Pi must send structured operations with workspace-relative paths and proposed
bytes before any write starts. The core resolves each path under the granted
workspace and rejects links or path escapes. It reads the current bytes and
applies the operations to an in-memory staging tree. It does not change the
workspace during this step.

The core computes ordered files, hunks, lines, and segments from the current
tree and the staged tree. It then validates the value with the generated Rust
`code-diff/1` codec. Neither Pi nor a web surface may supply a `CodeDiff`, its
identifier, or trusted hashes.

### Identifier and approval binding

The producer assigns a new UUIDv7 as `id` for each proposal. The generated
codec creates its JSON value. The core applies RFC 8785 JSON canonicalization
and stores those exact bytes in CAS. Approval records `gate_id`, `effect_id`,
`code_diff_id`, and the CAS SHA-256 for those bytes. The approval surface
renders only that CAS object and repeats both identifiers in its answer.

The core accepts an answer only for the pending gate and matching identifiers.
It reloads the CAS object, verifies its hash, decodes it with the generated
codec, and compares the decoded `id`. A mismatch rejects the answer.

Immediately before the write, the core rereads every source path. Changed
bytes, file types, modes, links, or workspace authority make the approval
stale. The core records the rejection and requests a new proposal. It never
reuses approval for regenerated content.

### Journal storage and replay

The core stores the complete canonical `CodeDiff` bytes in the existing CAS.
It uses media type `application/vnd.muniment.code-diff.v1+json`. The
`code.diff.proposed` journal event uses `payload_cas` for that object.

The event's envelope links the proposal to the effect through
`correlation_id`. The following `permission.requested` payload stores the four
approval fields named above. `permission.resolved` repeats them with the
decision and actor.

Reducers expose a diff only after CAS hash verification and generated-codec
validation. Live approval and replay use the same journal projection. Replay
never asks Pi to recreate a diff and never rereads the workspace for display.

The generated Rust type and codec are the only payload contract in this
repository. Journal code stores their serialized bytes and typed references.
It must not define a second `CodeDiff` struct, decoder, or field validator.

A missing or corrupt CAS object makes the diff unavailable. A pending approval
then fails closed. Receipt replay shows unavailable evidence and does not
reconstruct content from later workspace state.

### Limits and failures

The producer accepts at most 200 files, 20,000 rendered lines, and 2 MiB of
canonical `CodeDiff` JSON. It reads at most 8 MiB per file and 64 MiB per
proposal. The implementation checks byte totals with overflow-safe arithmetic.

More than 200 files or crossing a read limit rejects the proposal before
staging. Crossing a rendered line or JSON limit produces a valid value with
`truncated: true`. The producer may omit only complete trailing files or hunks,
in stable path order.

A truncated value may appear as evidence, with ADR 0020's warning. It cannot
open an approval gate or authorize a write. The user must narrow the proposal
until the core produces a complete value.

Files containing a NUL byte use `binary: true` and contain no hunks. A binary
change may appear as evidence, but diff approval cannot authorize it. A later
binary-content gate may define separate preview and approval rules.

Invalid UTF-8 without a NUL byte is also binary. File-type changes involving
directories, devices, sockets, or links fail validation. Conflicting
operations, duplicate paths, invalid modes, and contradictory generated model
values also fail validation.

On any validation, hashing, staging, CAS, or journal failure, the core opens no
gate and starts no effect. A CAS write without a journal append is unreferenced
and follows existing garbage collection. A journal append commits before the
core publishes the proposal to any surface.

### First implementation slice

The first slice extends Pi file-tool messages with structured proposed
operations in `src-tauri/core/src/sidecar/pi_chat.rs`. It adds production and
limit tests in `src-tauri/core/src/code_diff.rs`.

The slice exports that module from `src-tauri/core/src/lib.rs`. It adds typed
journal references and replay validation in `src-tauri/core/src/journal/mod.rs`
and `src-tauri/core/src/journal/reducer.rs`.

The slice connects proposals, permission answers, and stale checks in
`src-tauri/src/chat_coordinate.rs`. It pins the published generated Rust crate
in `src-tauri/core/Cargo.toml` and `src-tauri/Cargo.lock`.

This slice starts only after the Rust artifact from ADR 0019 publishes. It does
not add a local contract copy while that artifact is unavailable.

## Alternatives considered

**Pi produces `CodeDiff`.** Pi controls proposed input and cannot attest to the
desktop's current files or workspace authority.

**The Svelte adapter produces `CodeDiff`.** A presentation process cannot own
filesystem truth or authorize a native write.

**The core stores only source operations.** Replay would recompute against
changed files and could show evidence that the user never approved.

**The journal stores a handwritten diff payload.** That copy would violate ADR
0019 and could drift from every renderer's generated contract.

## Consequences

- One native boundary creates every trusted `CodeDiff`.
- Approval names one diff identifier and its exact CAS object.
- Workspace changes invalidate approval before an effect starts.
- Journal replay displays the original validated bytes.
- Truncated and binary diffs cannot authorize writes.
- Producer work waits for structured Pi operations and the generated Rust
  artifact.
