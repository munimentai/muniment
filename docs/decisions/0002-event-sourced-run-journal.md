# 0002 — Local run journal is the source of truth for session state

- Status: accepted
- Date: 2026-07-10
- Context: ROADMAP Phase 2 item 9; harness-spec §11.3 and §12.2

## Decision

Each desktop run has an **append-only SQLite event journal**. It is the
authoritative record of live and resumed run state. UI state, receipts, the
mobile relay, and snapshots are disposable projections that can be rebuilt.
The local journal contract and its Pi integration are implemented. It adopts
event sourcing, not Temporal. A server thread store may own synced conversation
records, but cannot amend the history of a locally executed run.

## Identity, ordering, and appends

`run_id` and `event_id` are stable opaque UUIDv7 strings, created once and
never reused. Every event has a contiguous, one-based `run_seq`; `(run_id,
run_seq)` and `event_id` are unique. Sequence is the sole within-run ordering
authority. Wall clocks are evidence, not ordering keys; runs have no implied
global order.

The versioned envelope is:

```text
event_id, run_id, run_seq
event_type, event_version, envelope_version
recorded_at                  # UTC RFC 3339 journal-append time
occurred_at                  # optional UTC RFC 3339 producer time
correlation_id, causation_id # optional stable event/request identifiers
payload_json XOR payload_cas { sha256, media_type, byte_length }
provenance { source, source_version, actor_id?, device_id?,
             rpc_request_id?, capability_versions? }
```

`envelope_version` versions common fields and `event_version` one event
payload. Readers preserve unknown envelope fields, ignore unknown event types
only when safety is unaffected, and fail a projection closed when an unknown
event bears on effects or permissions. Pure, tested upcasters convert older
payloads in memory; stored events are never rewritten for schema upgrades. A
new writer must remain readable by the immediately previous released reader,
or use an explicit compatibility gate before append.

One SQLite transaction compares the current sequence with the caller's
`expected_last_seq`, inserts one event or ordered batch, and commits. SQLite's
single-writer serialization plus this optimistic check is the concurrency
boundary. Repeating an `event_id` with byte-equivalent canonical envelope
content is idempotent success. The same ID with different content, a reused
sequence, or an expected-sequence mismatch is a conflict: append nothing and
reload/reduce/retry. Batches are all-or-none.

## Events and telemetry boundary

Durable run-domain events align with Pi request/response/notification flow:

- lifecycle: `run.started`, `run.cancel.requested`, `run.cancelled`,
  `run.failed`, `run.completed`, `run.needs_attention`;
- conversation/model: `message.submitted`, `model.requested`,
  `model.stream.delta`, `model.response.completed`;
- tools/permissions: `tool.requested`, `permission.requested`,
  `permission.resolved`, `tool.effect.started`, `tool.effect.completed`,
  `tool.effect.failed`;
- provenance: `route.selected`, `capability.used`, `receipt.finalized`.

Pi request IDs belong in provenance/correlation fields, not event identity.
Streaming deltas are durable and ordered; adjacent, unobserved deltas may be
coalesced before append, but appended deltas are immutable.

Supervisor `Starting`, `Healthy`, `Restarting`, `Failed`, and `Stopped`, probe
output, stderr, backoff, and generations are **diagnostic telemetry**, not run
history. They enter the journal only when translated into a user-relevant fact
such as `run.failed` or `run.needs_attention`. Process noise never creates
messages, receipts, or permission decisions.

## Replay and effects

Reducers are deterministic, side-effect-free functions over envelope order.
They reconstruct streaming state, pending permission gates, cancellation,
failure, completion, receipts, and needs-attention. A terminal event ends
execution; later recovery metadata cannot silently resume effects.

Model/network calls, tool calls, filesystem writes, connection operations,
permission decisions, relay publication, and notifications are external
effects. Intent and outcome, including stable idempotency keys where supported,
are recorded. Replay consumes those facts and **never re-executes an effect**.
After a crash with `tool.effect.started` but no outcome, the run becomes
`run.needs_attention` with an unknown-outcome reason; a user may choose a new,
recorded action. It is neither guessed failed nor retried. Cancellation records
both request and observed result.

Representative ordered sequence:

```text
 1 run.started
 2 message.submitted
 3 model.requested
 4 model.stream.delta              "I’ll check…"
 5 tool.requested                  effect_id=e-17
 6 permission.requested            gate_id=g-4
 7 permission.resolved             allow, actor=user
 8 tool.effect.started             effect_id=e-17
 9 tool.effect.completed           effect_id=e-17, result=cas:sha256:…
10 model.stream.delta
11 model.response.completed
12 receipt.finalized
13 run.completed
   — crash; reopen replays 1…13 and executes nothing —
   — requested continuation finds an expired key and refresh fails —
14 run.needs_attention             stale_credential; action=sign_in
```

Event 9 prevents repeating the tool. Event 14 is an explicit state, not fake
offline behavior. Expired sessions/keys, revoked entitlements, unavailable
capabilities/connections, and unsupported required event versions similarly
become needs-attention with a machine reason and safe action. Recovery appends
an event after restoration; it never edits history.

## Storage, payloads, and recovery

The database is `<app-data>/run-journal/v1/journal.sqlite3`, outside the CAS
tree and scoped to the signed-in profile. Connections enable foreign keys,
`journal_mode=WAL`, `synchronous=FULL`, and a finite busy timeout. An append
commits before exposure to UI or relay. Database/WAL files use platform owner-
only permissions. Secrets and reusable credentials are never payloads.

Small non-sensitive payloads may be inline JSON. Large bodies and payloads
requiring independent deletion use the `docs/cas.md` store; an event records
lowercase SHA-256, media type, length, and optionally a redacted summary. Reads
verify hashes. Missing/corrupt CAS bodies project as explicitly unavailable
without invalidating unrelated history.

On open, SQLite integrity and envelope/sequence validation precede replay.
Corruption makes the affected journal read-only: preserve original files,
report needs-attention, and offer export/recovery from the last verified
prefix. Never skip corruption and continue effects. CAS is checked separately.

## Retention, export, deletion, and compaction

Completed runs remain until user/org retention expires; pending gates,
nonterminal runs, and needs-attention runs never age out. Export uses a read
transaction and emits a versioned manifest, canonical envelopes in order, and
referenced CAS objects with hashes, omitting credentials.

Deletion is crash-safe: commit a tombstone in a separate deletion ledger,
transactionally remove selected events and projection/snapshot rows, garbage-
collect CAS objects only when unreferenced, then mark the tombstone complete.
Startup resumes incomplete deletion. Profile deletion applies this to all runs
and removes encryption/keychain material. Prior exports remain the user's
responsibility.

V1 compaction does **not** discard source events. It may checkpoint and
atomically replace the database (`VACUUM INTO`, fsync, rename) and collect
unreferenced CAS only after reference scanning. Auditable history is therefore
complete inside the retention period. Snapshots contain reducer version, last
sequence, and source-prefix hash; mismatch discards them and replays event 1.
Snapshots and projections have no retention authority.

## Projections and tests

UI/session state, receipt/provenance, and full-fidelity mobile relay are
independent versioned projections with checkpoints. Each consumes committed
events in sequence, treats duplicate delivery idempotently, and can rebuild
from event 1. Relay publication records cursor/acknowledgement but never becomes
a second history; reconnect resumes at its journal cursor.

Projection tests use golden streams, old-version/upcast fixtures, duplicates,
crashes at every effect boundary, unknown events, missing/corrupt CAS bodies,
and rebuild equivalence against incremental reduction.

## Alternatives considered

**Mutable session/message rows.** Easy to query, but overwrite the evidence
needed for pending gates, unknown outcomes, and replay safety. They may exist
only as rebuildable projections.

**Raw Pi transcript persistence.** Useful diagnostically, but lacks stable
domain versions, atomic effect boundaries, receipt provenance, and compatibility
across Pi changes. Raw frames may be diagnostic CAS objects, never authority.

**Temporal or server-backed execution.** Durable orchestration would add a
service dependency and change the desktop execution/security boundary. It may
serve later server Flue runs; neither it nor the relay is authoritative for a
local Pi run.

## Consequences

- Session code has one recovery path and can explain incomplete effects.
- Schema evolution, storage, privacy deletion, corruption recovery, and
  projection compatibility are release obligations.
- Full delta retention costs disk; retention and CAS bound it without weakening
  audit history during the retained period.
- Journal-backed rebuild and effect handling are production behavior; relay
  publication remains a deferred projection.

## Follow-up implementation slices

1. **Implemented:** SQLite schema, envelope types, atomic append API, migrations/
upcasters, integrity checks, and contract tests, without Pi wiring.
2. **Implemented:** Deterministic UI/run reducer and crash fixtures. Disposable
   snapshots remain optional and unimplemented; no implementation slice is
   selected.
3. **Implemented:** Pi domain/effect translation and receipt projection.
4. **Implemented:** Retention, export/deletion, CAS collection, and crash-safe
   compaction.
5. **Deferred:** A journal-backed relay projection/cursor when the existing
   full-fidelity relay backlog ticket is implemented; this ADR does not
   duplicate it.

Relay publication remains explicitly deferred to slice 5.
