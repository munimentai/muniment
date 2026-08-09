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

The core also creates an immutable write plan from the resolved operations.
The plan contains the exact output bytes, line endings, modes, renames, and
deletions that the effect will apply. It also records the observed state and
resolved parent identity for every affected path. The core stores the plan in
CAS before it opens an approval gate. Pi cannot supply the plan or its hash.

The core computes ordered files, hunks, lines, and segments from the current
tree and the staged tree. It then validates the value with the generated Rust
`code-diff/1` codec. Neither Pi nor a web surface may supply a `CodeDiff`, its
identifier, or trusted hashes.

### Identifier and approval binding

The producer assigns a new UUIDv7 as `id` for each proposal. The generated
codec creates its JSON value. The core applies RFC 8785 JSON canonicalization
and stores those exact bytes in CAS. Approval records `gate_id`, `effect_id`,
`code_diff_id`, the diff CAS SHA-256, and the write-plan CAS SHA-256. The
approval surface renders only that diff object and repeats the gate, effect,
diff, and write-plan identifiers in its answer.

The core accepts an answer only for the pending gate and matching identifiers.
It reloads both CAS objects and verifies both hashes. It decodes the diff with
the generated codec and compares the decoded `id`. A mismatch rejects the
answer. The effect executes only the verified write plan. It never rebuilds
operations from the displayed diff or asks Pi for output after approval.

Immediately before the write, the core re-resolves workspace authority and
every affected source, destination, rename target, and parent component. It
compares existence, bytes, file type, mode, link state, stable file identity,
and each resolved parent identity with the write plan. A new destination must
remain absent. A rename target must remain unchanged and free of collisions.
Replaced parents, new links, path escapes, changed metadata, and changed
authority make the approval stale. The core records the rejection and requests
a new proposal. The write uses the verified parent handles with no path
re-resolution gap. A concurrent identity or collision mismatch rejects the
effect. The core never reuses approval for regenerated content.

### Journal storage and replay

The core stores the complete canonical `CodeDiff` bytes in the existing CAS.
It uses media type `application/vnd.muniment.code-diff.v1+json`. The
`code.diff.proposed` version 1 event uses `payload_cas` for that object. A
separate `code.write-plan.staged` version 1 event uses `payload_cas` for the
write plan and media type `application/vnd.muniment.write-plan.v1+json`.

Both envelopes use the effect identifier as `correlation_id`. The diff event
uses the plan event's `event_id` as `causation_id`. The core appends the plan
event first and the diff event second in one SQLite transaction. It publishes
the proposal only after that transaction commits. An orphaned CAS object has
no journal authority and follows existing garbage collection.

This two-event structure keeps ADR 0002's payload XOR unchanged. Existing CAS
scanners see both objects through their events' standard `payload_cas` fields.
Retention preserves both events and objects while the proposal remains live.
Export includes both ordered envelopes and both objects. Deletion processes
both references through the existing deletion ledger and removes each object
only when no retained event references it.

The event versions define the link semantics. A future incompatible link or
payload change requires a new event version and an in-memory upcaster. Readers
that do not know either event type must treat it as bearing on effects and
permissions under ADR 0002. They fail both projections closed for the affected
run, even if they recognize a later `permission.requested` event. They expose
no approval and execute no effect from that run. Generic envelope scanning may
still retain, export, and delete each `payload_cas`.

The following `permission.requested` payload stores the five approval fields
named above. `permission.resolved` repeats them with the decision and actor.

After a restart, the reducer follows the diff event's `causation_id` to the
plan event. It requires the same `run_id` and `correlation_id`, the expected
event types and versions, and the hashes recorded by the permission event. It
then loads each object only through that event's `payload_cas` and verifies its
hash, media type, and byte length. A missing, duplicate, reversed, or mismatched
pair fails closed. Approval then resumes with the same stale check and verified
write plan. A resolved approval with no completed effect does not write
automatically after restart. The coordinator resumes it only through the
normal effect recovery policy and repeats every verification.

Reducers expose a diff only after CAS hash verification and generated-codec
validation. Live approval and replay use the same journal projection. Replay
never asks Pi to recreate a diff and never rereads the workspace for display.

The generated Rust type and codec are the only `CodeDiff` payload contract in
this repository. Journal code stores its serialized bytes in `payload_cas`.
It must not define a second `CodeDiff` struct, decoder, or field validator.

A missing or corrupt diff object makes the diff unavailable. A missing or
corrupt write plan makes the proposal non-executable. A pending approval then
fails closed. Receipt replay shows unavailable evidence and does not
reconstruct content from the plan or later workspace state. Replay exposes the
diff reference and write-plan hash, but never the plan's output bytes.

### Limits and failures

The producer accepts at most 200 files, 20,000 rendered lines, and 2 MiB of
canonical `CodeDiff` JSON. It reads at most 8 MiB per file and 64 MiB per
proposal. Pi may propose at most 400 operations, 8 MiB of output per operation,
and 64 MiB of output across the proposal. The core checks encoded lengths and
totals with overflow-safe arithmetic before it copies bytes, builds the staging
tree, reads current files, or reserves output storage.

The Pi message frame declares each payload length and the operation count. A
bounded streaming decoder rejects an excessive declaration before it buffers
the payload. It stops reading when the checked running total reaches a limit.

More than 400 operations, an oversized operation payload, an overflowing sum,
or crossing the total proposed-output limit rejects the input before staging
or allocation. More than 200 files or crossing a read limit also rejects the
proposal before staging. Crossing a rendered line or JSON limit produces a
valid value with `truncated: true`. The producer may omit only complete
trailing files or hunks, in stable path order.

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
limit tests in `src-tauri/core/src/code_diff.rs`. It defines CAS serialization
and verification for the immutable plan in `src-tauri/core/src/write_plan.rs`.

The slice exports that module from `src-tauri/core/src/lib.rs`. It adds the two
journal events, their atomic append, and replay validation in
`src-tauri/core/src/journal/mod.rs` and
`src-tauri/core/src/journal/reducer.rs`.

The slice connects proposals, permission answers, and stale checks in
`src-tauri/src/chat_coordinate.rs`. It pins the published generated Rust crate
in `src-tauri/core/Cargo.toml` and `src-tauri/Cargo.lock`.

This slice starts only after the Rust artifact from ADR 0019 publishes. It does
not add a local contract copy while that artifact is unavailable.

## Correction — 2026-08-09: local codec prerequisite

The producer slice waits for the local `muniment-code-diff` crate defined by
ADR 0020, not a published artifact. The codec crate lands first, the CLI ANSI
renderer lands second, and this producer slice lands third.

## Alternatives considered

**Pi produces `CodeDiff`.** Pi controls proposed input and cannot attest to the
desktop's current files or workspace authority.

**The Svelte adapter produces `CodeDiff`.** A presentation process cannot own
filesystem truth or authorize a native write.

**The core stores only the rendered diff.** A diff omits exact output bytes,
line endings, and binary content. It cannot define the approved write.

**The journal stores a handwritten diff payload.** That copy would violate ADR
0019 and could drift from every renderer's generated contract.

## Consequences

- One native boundary creates every trusted `CodeDiff`.
- Approval names exact diff and write-plan CAS objects.
- Workspace changes invalidate approval before an effect starts.
- Journal replay displays the original validated bytes.
- Truncated and binary diffs cannot authorize writes.
- Producer work waits for structured Pi operations and the local Rust codec.
