# 0031 — The company record is one SQLite file per company

- Status: accepted

## Context

Muniment is a company system of record. The spec pages under `spec/` define
eight tables, seventeen core kinds and seventeen relations, and rule that a
model never writes a row: propose validates and commit applies. The plan puts
the single-player graph in SQLite inside the runtime service, because that is
the one engine that runs with no window open. A user may keep more than one
company on one machine, and the paid tenant is one company.

## Decision

### Files

A company is one directory, `~/.muniment/companies/<id>/`, under the state
root. `<id>` is a UUIDv7. The directory holds `graph.sqlite3`, a `company.json`
carrying the name, and `sql-audit.sqlite3`, the SQL tool's query log. The
graph file never holds app state, and the audit file never holds record data.
The runtime keeps the current company id in its state and exposes
`company.list`, `company.create`, `company.select` and `company.rename` over
attach. A backup is `VACUUM INTO` a path the user picks. The graph is never
copied while open and never placed in a synced folder.

### Dialect

The graph opens with `journal_mode = WAL`, `foreign_keys = on` and
`synchronous = NORMAL`. `tenant_id` is absent locally: one file is one tenant.
`entity.data`, `edge.props`, `kind.schema`, `kind_extension.schema` and the
event `before` and `after` columns are JSON1 text. Every id is a UUIDv7
string, `event.id` included, and nothing autoincrements. `event.seq` is the
append order the writer assigns inside its transaction, the way the run
journal assigns `run_seq`, and `event.at` is RFC 3339 text. `body_text` and
`title` feed an FTS5 table kept current by triggers. Vectors wait for
sqlite-vec when semantic search lands. The `principal` check constraint
`type = 'human' or on_behalf_of is not null` is in the schema. The bundled
SQLite is 3.46.0 through libsqlite3-sys, which carries JSON1, FTS5 and jsonb.

### The catalogue

The seed writes the seventeen core kinds and relations from `spec/schema.html`
and three kinds more: `mapping`, `workflow` and `view`. Each kind row holds its
JSON Schema, `title_template`, `text_template` and `states`. A user's new table
is an extension-only kind named `x_<name>`, and a new column on any kind is a
`kind_extension` property named `x_<name>`. A core property, enum or relation
never changes in a company file. A catalogue change is a schema revision that
ships with the app and migrates every company on open.

### The write path

propose takes a create, update, link, merge or delete. It validates the data against
the kind's core schema and the company's extension, resolves every identity to
an entity, and returns a diff, warnings and a proposal id. It writes nothing.
Proposals live in runtime memory and expire after one hour. commit takes the
id, applies the diff in one transaction, appends one `event` row naming the
actor principal and `on_behalf_of`, and returns the entity and event ids. A
second commit on the same id is a no-op that returns the first result. A merge
writes a `superseded_by` edge from loser to survivor, repoints every identity
row at the survivor, and appends one event carrying both sides. Nothing is
deleted. Creating a company creates its one human principal, and every agent
principal names that human.

### Readers

A reader implements Objects, Describe, Page and Delta and nothing else. File
readers run in Rust inside the runtime. Network readers are one Go sidecar
with a subcommand per source, bundled with the provenance checks of ADR 0008,
speaking JSON over stdio, holding a token bucket per source, and never opening
the graph. The runtime stores each cursor on the mapping entity, advances it
in the same transaction as the page it covers, and writes through a staging
merge: a temp table, an identity insert that does nothing on conflict, an
entity upsert only where data differs, one event per changed row, and edges in
a second pass. Imported events read `imported`, their actor is the job
principal, and `on_behalf_of` names the person who committed the mapping.

## Consequences

Two dialects of one schema exist, SQLite here and Postgres in the cloud,
behind one Rust data interface. The runtime is the single writer, so the event
log keeps its order without locks across processes. The record panel and the
MCP server read the same file through the same module. Go joins the build: a
toolchain in CI and one more signed binary in the bundle.

## References

- [SPEC.md](../../SPEC.md), The local graph and law 13.
- [ADR 0002](0002-event-sourced-run-journal.md), the append-only pattern the
  event table follows.
- [ADR 0008](0008-pi-runtime-distribution.md), sidecar provenance.
- [ADR 0025](0025-per-thread-permission-policy.md), the policy the unattended
  write rule builds on.
- `spec/schema.html`, `spec/agent.html` and `spec/ingest.html` in the muniment
  working folder.
