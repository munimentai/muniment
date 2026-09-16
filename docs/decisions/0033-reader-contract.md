# 0033 — Readers fill the graph behind one contract, and the runtime does every write

- Status: accepted

## Context

The record panel can show and edit a company's graph, and the agent can
propose and commit through the record's MCP server, but nothing fills the
graph from the tools a business already runs. The spec names the reader
interface as the contract between the small, hand-guarded core and the
connector lane, because a reader that writes SQLite on its own breaks every
invariant the write path holds: kind validation, identity normalization,
one event per change, and an actor on every event.

## Decision

Every source implements four calls and nothing else reaches the graph:

- **Objects** lists what the source holds. A file reader holds one object,
  the file, named by its path and labelled by its file name.
- **Describe** answers one object's fields, each with the type its samples
  read as, three distinct samples and a filled count, plus the row count,
  the byte count and a hash of the object.
- **Page** answers rows from a cursor for a limit, with the next cursor
  when rows remain. A row is a map from field name to text.
- **Delta** answers whether the object changed since a cursor: unchanged,
  changed with the new hash, or gone.

The runtime holds the cursor and does every write. A mapping is a `mapping`
record in the graph, so the panel edits it and the agent proposes it like any
other record. It carries the source, the object, the kind, `fields` from each
source column to a property, `identity` as the column that keys each row, and
`approved`. The identity reads `<kind>:<column>` for an email, domain, phone,
handle or name key column, or `external:<system>:<object>:<column>` so the
external id keeps the record's `system:object:record_id` shape. A mapping
with no identity keys on the title's name key, so a repeat run over a file of
names updates instead of duplicating.

A run reads each row through the mapping into the data a propose takes,
coercing each cell by the property's schema: numbers with currency marks and
separators, booleans in their spoken forms, dates and times in the shapes
spreadsheets and exports write, enum values folded on case and separators.
It resolves the identity. A resolved row whose mapped data is unchanged is
skipped, a resolved row that differs becomes an update, and an unresolved
row becomes a create with that identity. A create the record warns about,
because a record with the same title exists, is not committed. Every commit
runs as the `reader` service principal acting for the company's owner, so
the history line names the reader and the human.

A run works for a bounded budget per request and answers its offset, and the
caller sends the offset back until the run is done, so one request never
holds the desktop client past its timeout. The run writes the cursor with the
object's hash into the mapping's `cursors` through propose and commit like
any other change. Every row the mapping could not place, with its row number,
title, reason and the cells the mapping read, is the resolve queue. It lives
in `resolve-<mapping id>.json` beside the company's graph, never in it, like
the SQL audit file, because it is a record of what did not land.

The CSV reader is the first reader and the proof of the contract. It parses
RFC 4180 with a sniffed delimiter, names empty and repeated header cells, and
reads a file up to 64 MiB. The attach protocol carries the contract as
`reader.describe`, `reader.run` and `reader.queue`, desktop only.

## Consequences

Every later reader, the JSON file reader and each Go sidecar subcommand,
implements the four calls and reuses the mapping, the run and the queue
without touching the core. A source that changes its schema breaks one
mapping, visible in the queue, and never a table. A mapping is data, so a
company can carry as many as it has exports, and the agent can propose one.
The panel gains an Import control per kind and a Run control on a mapping
record. The queue has no action per row yet: a person fixes the file or the
mapping and runs again.

## References

- [ADR 0031](0031-company-record.md), the record the readers fill.
- [SPEC.md](../../SPEC.md), Readers.
