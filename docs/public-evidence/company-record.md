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

Nothing writes a row directly. A proposal names a create, an update, a link or a merge. Muniment validates it against the kind, resolves every reference to an entity, and returns a diff with a proposal id and warnings. A commit applies that diff in one transaction and appends one event that names the actor and, for an agent, the human it acts for. A second commit of the same proposal returns the first result and writes nothing. A merge keeps the losing entity reachable through a `superseded_by` edge and moves its identities to the survivor.

## Runtime operations

The runtime answers four operations on its attach protocol:

| Operation | Body | Answer |
| --- | --- | --- |
| `company.list` | `{}` | `companies`, oldest first, and `current` |
| `company.create` | `{"name": "..."}` | the new `company` |
| `company.select` | `{"company_id": "..."}` | the `company`, now current |
| `company.rename` | `{"company_id": "...", "name": "..."}` | the renamed `company` |

`company.create`, `company.select` and `company.rename` carry an idempotency key. A companion client cannot call any of the four.
