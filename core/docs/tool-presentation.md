# Tool presentation

`muniment_core::tool_presentation` turns each tool call into what a person
reads. Hosts render the result and compute none of it.

## The presentation

| Field | Holds |
| --- | --- |
| `category` | `web`, `files`, `shell`, `code`, `agent`, `memory`, `mcp:<server>` or `other` |
| `kind` | The group a transcript folds the call into: `read`, `search`, `list`, `edit`, `command`, `web`, `compact`, `agent`, `memory`, `mcp` or `other` |
| `icon` | A Lucide icon name |
| `headline` | What the call does, in the imperative: ``Run `npm test` in muniment-desktop`` |
| `progress` | What it does while it runs: `Running npm test` |
| `details` | At most six `{label, value, mono}` rows, without empty, false or default values |
| `result` | How it ended: `Exit 0 in 4.2 s`, `Found 8 results`, `Failed: permission denied` |
| `path` | The file a host may open |
| `via` | How a call another call made ran: `via script` |

A headline marks code in backticks and never holds raw JSON. The raw input and
output stay with the call for its details.

`chat_view` adds a presentation to each `ChatToolActivity` and to a pending
permission gate, and the swarm adds one to each agent approval. A gate's
summary is its headline.

Pi runs a codemode script's tool calls as calls of their own, each naming
the script's call. A `ChatToolActivity` names that call in
`parent_effect_id`, an earlier call of the run, and its presentation
carries `via`. A script that made calls takes their `kind`, or `other` when
they differ, and is `uncounted`: its calls count in its place. A host shows
the script as the parent row with its calls nested under it.

## Describers

The runtime's shipped tools have describers: the file, shell and script tools,
web search and fetch, background tasks, child agents, memory, agents, routines,
artifacts, the company record, questions and plans.

Any other tool takes the generic describer:

1. The action is the tool's title, else its label, else its name in words. An
   MCP tool `mcp__<server>__<tool>` reads `Ask <Server>: <action>`.
2. The subject is the most salient argument: `query`, `prompt`, `title`,
   `name`, `path`, `url`, `command`, `id` and similar names, else the one
   required text argument, else the only text argument. A path reads as its
   file name, a command as code and free text in quotes.
3. The other arguments become details, labeled by their schema title or their
   name in words. Enum values take their titles, arrays and objects are
   counted, long text is clipped and secret-looking values read `Hidden`.
4. A result counts a JSON list or a list field, names a returned record, or
   reads the first line of text. A failure reads its message.

## Metadata and hints

Each run reports its tools' descriptions, input schemas and annotations when it
starts, and the runtime keeps them by tool name. A tool may opt in to its own
presentation with hints under `x-muniment-presentation` in its input schema,
or under `muniment/presentation` in an MCP tool's `_meta` or `annotations`:

```json
{
  "type": "object",
  "properties": { "summary": { "type": "string" }, "board": { "type": "string" } },
  "x-muniment-presentation": {
    "headline": "File {summary} on {board}",
    "progress": "Filing an issue",
    "icon": "list-checks",
    "category": "agent",
    "subject": "summary",
    "result": "Filed {key}",
    "hidden": ["board"]
  }
}
```

A template names input properties in braces, and `result` names fields of a
JSON result. A template whose property the call leaves out is skipped.

## Fixtures

`protocol-fixtures/tool-presentation/calls.json` holds a gallery of calls and
gates. A core test checks `presented.json` against the describers, and
`MUNIMENT_WRITE_FIXTURES=1` writes it again. The desktop's tests and its tools
probe page render it.
