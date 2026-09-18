# The company record

Muniment keeps each company's record in one SQLite file on the machine. The runtime service owns the file and opens it with no window open.

## Files

Every company is one directory under the state root, `~/.muniment/companies/<id>/`, or under the directory `MUNIMENT_STATE_DIR` names. The id is a UUID. The directory holds:

- `graph.sqlite3`, the record: entities, identities, edges, events, fact sources, the kind catalogue, the company's kind extensions and its principals.
- `company.json`, the company's name, creation time and the id of its owner principal.

The file `~/.muniment/companies/current` holds the id of the current company. The first company you create becomes current. Do not place the state root in a synced folder.

## The catalogue

Every company starts with the same catalogue: seventeen core kinds, `person`, `org`, `deal`, `thread`, `message`, `ticket`, `task`, `project`, `document`, `meeting`, `subscription`, `invoice`, `service`, `incident`, `deploy`, `commitment` and `decision`, and three more, `mapping`, `workflow` and `view`. Each kind row carries its JSON Schema, a title template, a prose template and its states. Seventeen relations join entities. Core kinds and relations are the same in every company. A company adds a property to a kind under the `x_` prefix, or adds a kind whose name starts with `x_`.

## Identity

Every way an entity is named is one identity row: `email`, `domain`, `phone`, `handle`, `external` or `name_key`. Values are normalized before they are stored. An email is lowercased and loses its plus tag. A domain keeps its registrable labels. A phone becomes E.164, with ten digits read as a United States number. A handle reads `platform:id`. An external id reads `system:object:record_id` and is the key every import uses.

## Writes

Nothing writes a row directly. A proposal names a create, an update, a link, a merge or a delete. Muniment validates it against the kind, resolves every reference to an entity, and returns a diff with a proposal id and warnings. A commit applies that diff in one transaction and appends one event that names the actor and, for an agent, the human it acts for. A second commit of the same proposal returns the first result and writes nothing. A merge keeps the losing entity reachable through a `superseded_by` edge and moves its identities to the survivor. A delete takes the entity out of every table, view and search while its data, identities and history stay, and a later proposal cannot name it.

## Runtime operations

The runtime answers four operations on its attach protocol:

| Operation | Body | Answer |
| --- | --- | --- |
| `company.list` | `{}` | `companies`, oldest first, and `current` |
| `company.create` | `{"name": "..."}` | the new `company` |
| `company.select` | `{"company_id": "..."}` | the `company`, now current |
| `company.rename` | `{"company_id": "...", "name": "..."}` | the renamed `company` |

`company.create`, `company.select` and `company.rename` carry an idempotency key. A companion client cannot call any of the four.

Five more operations read a source into the record:

| Operation | Body | Answer |
| --- | --- | --- |
| `reader.objects` | `{"source": "stripe"}` | the `objects` the connected source holds, each with a `name` and a `label`, or the error `not_connected` |
| `reader.connect` | `{"source": "stripe", "secret": "<secret key>"}` | `connected` with the source's `objects`, after one read proved the key; a refused key is not kept |
| `reader.describe` | `{"source": "csv", "object": "<file path>"}` or `{"source": "stripe", "object": "customers"}` | the `description`: the object's `label`, `rows`, `counted`, `bytes`, `hash` and `fields`, each with its `name`, the type its samples read as in `guess`, three `samples` and its `filled` count |
| `reader.run` | `{"mapping": "<entity id>", "offset": 0}` | the `run`: `created`, `updated`, `unchanged` and `unplaced` counts, `next_offset`, `done`, `total`, `counted`, `changed` and the first hundred `queue` rows |
| `reader.queue` | `{"mapping": "<entity id>"}` | the `queue` rows the last run could not place, each with `row`, `title`, `reason` and `cells` |

`reader.run` and `reader.connect` carry an idempotency key. `reader.run` works for about two and a half seconds per call and answers `done` false with a `next_offset`, and the caller sends that offset back until `done` is true. A source that counts nothing, such as Stripe, answers `counted` false and `total` as the rows read so far. A companion client cannot call any of the five.

## Sources

The CSV reader runs inside the runtime and reads one file as one object. Network sources run in `muniment-reader`, a Go program beside the runtime with one subcommand per source: the runtime starts it once per call, writes one JSON request on its standard input and reads one JSON answer, and the source's secret travels in the environment variable `MUNIMENT_READER_SECRET`, never on the command line. The secret lives in the platform keychain under the service `ai.muniment.reader` with the source name as the account. The Stripe source reads `customers`, `subscriptions` and `invoices` over Stripe's REST API with the secret key as a bearer token, flattens each object to text fields, adds `email_domain` to a customer and `state` to a subscription so they land on the `org` and `subscription` kinds, and pages with `starting_after`. It writes nothing back to Stripe. The other sources follow the same contract: HubSpot, Pipedrive, Salesforce, Zendesk, Intercom, Freshdesk, Zoho CRM, Outreach, Salesloft, Apollo, Gong, ZoomInfo, Notion, Airtable, Google Sheets, Calendly, Mailchimp, Kit, Square, Shopify, PayPal, FreshBooks, QuickBooks, Wave, Xero, Dynamics 365 and Marketo. Each names the credentials it connects with on its connect screen, and none writes back.

## Reach it from a harness

The runtime serves the record to any MCP client through `muniment-cli mcp`, a stdio server for the protocol revision of 2026-07-28. It offers three tools: `sql`, one read-only query that answers CSV, `propose`, which validates a change and returns its diff, and `commit`, which applies a proposal. The binary sits beside the runtime:

| Platform | Path |
| --- | --- |
| macOS | `/Applications/muniment.app/Contents/Library/LaunchServices/muniment-cli` |
| Linux | `/usr/lib/muniment/muniment-cli` |

Add it to Claude Code with one command, using the path for your platform:

```sh
claude mcp add muniment -- /Applications/muniment.app/Contents/Library/LaunchServices/muniment-cli mcp
```

The first call pairs the harness with the desktop. The desktop shows the pairing and you approve it once. On Linux the harness needs `XDG_RUNTIME_DIR` set, as a desktop session sets it.

The desktop's own assistant reaches the same server without setup. The runtime writes a `record` entry into `~/.muniment/agent/mcp.json` with `protocolVersion` pinned to `2026-07-28` and keeps every other server you list there.

Every query lands in `sql-audit.sqlite3` beside the company's graph, with its text, its row count and any error. A query stops after five seconds, and a result is cut at 500 rows or 64 KiB. A commit through a harness is recorded as an agent principal named for that harness, acting for the company's owner.

Windows has no companion client yet, so the server answers there with a platform error.

## The Record panel

The **Record** control sits at the right end of the app row, after **Artifacts**. Press **Command K** on macOS or **Control K** on Windows and Linux to open or close it, and **Escape** to close it. The panel opens beside the thread, and opening it closes the artifact rail. **Maximize** gives the panel the whole window until you press the shortcut or **Escape** again.

The header shows the current company as a picker. With no company yet, the panel opens on its first screen: a headline, one sentence, the three largest findings of a sample company, the create form and the sources that read in. A company with no records opens on **Connect your data** with the sources under their marks. A company with records opens on its report: one line with the company, the open finding count and how it moved since the last read, then each finding as one sentence with its magnitude, a claim and a consequence. **Review** opens the evidence behind a finding, a few record lines, and **Open person** or **Open org** opens the kind the finding is about. The findings are queries over the live graph: duplicate people, duplicate organizations, domain collisions, dead fields, free text fields that are lists, and stale values. **Kinds** in the header opens the kind list, which names every kind with its record count. Selecting a kind shows its table.

Selecting a kind opens its table. The columns are the entity's title, its state when the kind has one, the time it changed, and then every property of the kind, with typed values in mono. A header click sorts. The search field runs full text over titles and prose, and the start of a word is enough: `north` finds Northwind. The table shows 200 rows at a time and **Show more** appends the next 200, so the count above the table is the count in the record. Double-click a cell to edit it and press **Enter**: the panel shows the change as one line per property with any warning, and **Commit** applies it while **Escape** discards it. A title opens the record: its prose, fields, identities, relations with their validity windows, and history. **New** opens a form generated from the kind with the required properties first, and **Propose** then **Commit** creates the record.

An open record offers three actions in the header. **Link** picks a relation the vocabulary allows from this record's kind, a target kind it reaches and a target found by title, and proposes the edge. **Merge** finds a survivor of the same kind by title and proposes the merge, which shows how many identities move. **Delete** proposes the soft delete. Each shows its diff, and **Commit** applies it: a link keeps the record open, a merge opens the survivor, and a delete returns to the table without the row. The panel rereads what it shows when a run ends and when the window regains focus, so a record the assistant or a harness committed appears without reopening the kind. **Rename** beside the company picker, on the kind list, renames the current company.

**Import** on a kind reads a source into it. Pick **CSV file** and then the file, or pick **Stripe**, paste its secret key once, and pick **Customers**, **Subscriptions** or **Invoices**. The panel then shows the first record as a card, the way a profile page reads: the row's name on top, then each field the row holds beside the property it fills, with the likely one chosen and **skip** for the rest, and the arrows page through the first twelve records. A source that answers no rows lists the columns with what each one reads as and three sample values instead. **Key each row on** names the column that identifies a row: an email, a website or a phone the file carries, an id column, or the record's title. **Propose mapping** shows the `mapping` record the panel will commit, **Commit and run** applies it and runs it, and the run reports its progress as `n of total rows`. The result names how many rows were created, updated or unchanged, and lists every row the mapping could not place with its row number, title and reason, such as an empty identity cell, a value that is not a whole number, or a free mail domain in a company's website column. Run the same file again and the rows it keyed update, with none added twice. A `mapping` record's view offers **Run** to read the file again, so a refreshed export lands with one click. The rows not placed also wait in `resolve-<mapping id>.json` beside the company's graph.

A kind with states, such as `deal` or `task`, offers **Board** beside **Table** and a state filter. The board shows one column per state. Drag a card to another column to propose the change, then **Commit**. **Save view** stores the layout, sort and filters as a `view` record you can pick again. **Ask** puts the open view's SQL into the composer, so a question to the assistant starts from what you see.
