# 0016 — Thread identity over the run journal

- Status: accepted
- Date: 2026-07-26
- Context: desktop spec §§2, 3, and 12; ADR 0002

## Context

The desktop specification gives a thread a stable title, recent-list entry,
model pin, rename/delete actions, and keyboard navigation. The journal instead
has only `run_id`: `run_summaries` derives a title from each run's first prompt,
the attach protocol aliases thread to run, and `chat_history` flattens every
run owned by the signed-in subject. One user-visible conversation therefore
cannot span multiple executions.

ADR 0002 makes run events immutable and rejects authoritative mutable
session/message rows. It permits mutable query shapes only as rebuildable
projections. The current schema also hard-fails when `PRAGMA user_version`
differs, while several tables are added after the version check with `CREATE
TABLE IF NOT EXISTS`. Finally, the shipped `<app-data>/runs.sqlite3` is neither
the versioned nor profile-scoped path decided by ADR 0002.

## Decision

### Identity and authoritative records

`thread_id` is a stable, opaque UUIDv7 created once and never reused. A thread
is authoritative as an append-only, independently sequenced `thread.*` event
stream in the run-journal database:

- `thread.created` records the creator/profile, creation time, and optional
  project/workspace scope;
- `thread.title.renamed` records the validated explicit title; and
- `thread.deleted` is the user deletion intent and tombstone.

Thread events have stable `event_id`, contiguous `thread_seq`, event and
envelope versions, timestamps, provenance, and optimistic
`expected_last_thread_seq` appends. They use ADR 0002's idempotency,
upcasting, unknown-event, and never-rewrite rules. A `thread_events` ledger is
not a mutable session table.

Each run is assigned by an immutable `run_threads(run_id PRIMARY KEY,
thread_id, thread_run_ordinal)` stamping edge in the same transaction that
appends `run.started`. `(thread_id, thread_run_ordinal)` is unique and the
ordinal is allocated contiguously under the thread append concurrency
boundary. It supplies within-thread run order without inventing a global run
order from wall clocks. The edge is authoritative identity, follows the
existing `run_workspaces` idiom, and cannot be updated to move or reorder
history. Foreign keys ensure both identities exist. Retrying run start with
the same run, thread, and ordinal is idempotent; a different stamp is a
conflict and appends nothing. New thread creation and its first run start are
one transaction. Continuing a thread creates a new run stamped to that
thread; it never reuses a `run_id`.

Rebuildable projections may join thread metadata, stamped runs, redacted
messages, and model-pin state for UI and attach reads. They are not identity
authority. Existing runs receive a distinct thread apiece during migration,
preserving today's run-as-thread behavior without guessing that unrelated
runs belong together.

### Titles

The effective title is the latest non-deleted `thread.title.renamed` value.
Rename is therefore stored as an event, including actor and time, rather than
overwriting a summary row. Titles are trimmed, non-empty user-visible strings
with a bounded scalar length; the implementation slice must choose and test
one shared limit for desktop and attach inputs.

Until a rename exists, the title projection falls back to `title_from` applied
to the earliest prompt across the thread's runs in run-stamp order, retaining
its whitespace normalization, 80-character truncation, and untitled fallback.
The derived value is never copied into authority: adding the first prompt may
change the fallback, while an explicit rename always wins. Renaming back to
the derived title is represented by another rename event; v1 has no separate
"clear custom title" operation.

### Schema versions, location, and migration

`PRAGMA user_version` becomes an explicit ordered migration boundary. Version
zero creates the current schema; each supported older version advances through
transactional, idempotent migration steps and is validated before commit.
Unknown newer versions and failed migrations remain fail-closed and
read-only/recoverable as ADR 0002 requires. All existing post-open `CREATE
TABLE IF NOT EXISTS` additions are assigned to a numbered baseline or
migration; no schema may evolve through an unversioned side channel.
Event-envelope and payload versions remain independent of SQLite schema
versions, and stored events are upcast in memory rather than rewritten.

ADR 0002's profile-scoping decision stands. New journals live at
`<app-data>/profiles/<opaque-profile-key>/run-journal/v1/journal.sqlite3`;
the extra profile component makes its previously implicit scope concrete.
The shipped `<app-data>/runs.sqlite3` is a legacy migration source, not the
continuing location. After sign-in, migration takes an exclusive application
lock, copies into a staged profile journal, performs schema/thread backfill
and full validation, then atomically publishes it. The legacy database is
preserved until successful publication and recovery/export policy permits
cleanup. A source containing events attributable to another subject, multiple
subjects, or ownership that cannot be assigned safely is not silently claimed
or split: opening the new journal fails closed and offers recovery/export.
Concurrent migration, an existing destination, or a partial staged copy must
never merge or duplicate histories.

### Contract impact map

| Contract | Decided direction |
| --- | --- |
| `run_summaries` and HMAC cursors | Replace run grouping with one summary per non-deleted `thread_id`. `updated_at` is the maximum committed run or thread-metadata event time; ties use `thread_id`. A new versioned HMAC cursor authenticates `(snapshot boundary, updated_at, thread_id, scope)` so pages are scoped and stable while concurrent activity is deferred to a fresh listing. Existing run-summary cursors are deliberately invalid after the contract change. |
| Attach `thread.list` / `thread.open` | End thread≡run aliasing. `thread.list` pages thread summaries in the authorized workspace/profile scope. `thread.open` pages the redacted projection across all stamped runs in deterministic run-stamp order, then `run_seq`; its cursor binds `thread_id`, scope, and snapshot. `DesktopAttachService.list_threads` implements the same journal contract rather than returning `unsupported_operation`. |
| `chat_history` | Replace the unbounded all-run response with scoped, cursor-paginated thread summaries. Opening a selected thread uses a separately paginated thread projection; the frontend no longer flattens unrelated runs. Every read requires the signed-in profile plus authorized personal/project workspace scope. |
| Run start | `start_desktop_run`/`prepare_opened_run` accepts an existing authorized `thread_id` or creates one for New Thread, and transactionally stamps it with `run.started`. Stale, deleted, foreign-profile, or wrong-scope thread IDs fail before execution and append nothing. Concurrent starts may create separate ordinal runs in one thread; immutable stamps prevent lost membership. Start and delete serialize at the thread concurrency boundary: a start committed first is included in deletion, while delete committed first makes start fail. |
| Retention, delete, export, compaction | Retention evaluates runs but preserves thread metadata while any member run remains. Delete appends `thread.deleted`, hides the thread immediately, and drives ADR 0002's crash-safe deletion ledger across every stamped run, projection/snapshot row, and unreferenced CAS object; startup resumes partial deletion. Export snapshots one thread, its metadata events, stamps, ordered run envelopes, and referenced CAS with a versioned manifest. Compaction preserves live thread events/stamps and pending deletion work; an empty deleted thread leaves no reusable identity. |

Project/workspace authorization remains an access boundary, not inferred from
a caller-supplied ID. Thread deletion is whole-thread deletion in v1; deleting
one run or detaching a run would require a later contract.

### Scope and first implementation slice

The first implementation slice is **thread identity and metadata foundation**:
numbered schema/path migration, legacy one-run-per-thread backfill, the
append-only thread ledger and run stamp transaction, title projection, and
contract tests for conflicts, migration failure, profile/scope rejection, and
projection rebuild equivalence. It exposes only the primitives needed for
list, create, switch/open, rename, and delete; subsequent UI slices consume
those settled contracts.

V1 does not define thread fork/branch lineage, sharing, or search. It also
does not add project transfer, per-run deletion, title-clear semantics, or the
specified per-thread model-pin UI. A later pin slice stores pin changes as
thread metadata events keyed by this identity, never as mutable run state.
Those features require separate authorization, provenance, and indexing
decisions.

## Alternatives considered

**Treat each run as a thread.** This preserves current attach behavior but
cannot continue a conversation across runs or provide per-thread title and
model state, so it contradicts the desktop contract.

**Use only a `run_threads` stamping edge.** The edge gives stable grouping but
has nowhere immutable to record creation, rename, deletion, or their
provenance. Adding mutable columns would recreate the rejected session table.

**Infer membership as a rebuildable projection.** Prompts, timestamps, and
resume data do not unambiguously establish user intent. A rebuild could group
history differently, making identity, authorization, and deletion unsafe.

**Use an authoritative mutable `threads` row.** Queries and rename are simple,
but overwriting title/deletion state loses history and violates ADR 0002.
Such a row may exist only as a projection rebuilt from thread events and
stamps.

**Put `thread.*` events into an arbitrary member run.** A thread can exist
before its first run and after individual run retention, and concurrent runs
have no global order. Choosing one run as metadata authority would make rename
ordering and deletion depend on unrelated run lifecycle.

**Keep the flat shared database path.** This avoids migration but leaves
cross-profile reads and deletion structurally possible and directly conflicts
with ADR 0002's privacy boundary.

## Consequences

- Threads gain durable identity and auditable metadata without rewriting run
  events or making projections authoritative.
- One transaction at run start and one additional identity join are required;
  projections keep common list/open reads bounded.
- Cursor formats, attach fixtures, desktop history APIs, and exports require
  explicit contract-version changes rather than compatibility aliases.
- Migration must handle legacy ownership and partial failure conservatively;
  unsafe attribution makes history unavailable until recovery/export instead
  of exposing it to the wrong profile.
- Whole-thread deletion spans multiple runs and CAS references, increasing the
  deletion ledger's work while retaining ADR 0002's crash-safety guarantees.
