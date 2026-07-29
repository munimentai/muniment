# 0020 — The shared code-diff model and web renderers

- Status: accepted
- Date: 2026-07-29
- Context: harness-spec §§6.1 and 13.1; ADR 0019

## Context

Harness-spec §6.1 puts tool activity, including diffs, on the conversation
surface. Section 13.1 makes the CLI and editor extension governed windows onto
that surface. Each surface must show the same edit before approval. Section
6.5 covers filesystem access and sandboxing, so it does not govern rendering.

The desktop uses Svelte, the editor extension runs inside VS Code, and the CLI
runs in a terminal. A DOM renderer cannot serve all three. The surfaces need
one serialized diff model and a renderer for each environment.

ADR 0019 governs published cross-surface contracts. The diff model follows
that decision instead of adding an editable schema to this repository.

## Decision

### Shared model

The shared contract is `code-diff/1`. Its canonical TypeSpec schema lives in
the cloud repository at `contracts/e0/code-diff/1/`. The publication lane
emits `muniment-e0-code-diff-v1` and `@muniment/e0-code-diff-v1`. Consumers pin
exact artifact versions and follow ADR 0019's generation, compatibility, and
migration rules.

The root `CodeDiff` has these fields:

- `schemaVersion`, the literal `1`.
- `id`, a stable identifier that binds display and approval to one edit.
- `files`, an ordered list of `DiffFile` values.
- `truncated`, which reports that the producer omitted content.

Each `DiffFile` has `oldPath`, `newPath`, `status`, `oldMode`, `newMode`,
`binary`, and `hunks`. A path or mode is absent when that side does not exist.
`status` is `added`, `deleted`, `modified`, or `renamed`. A binary file has no
hunks.

Each `DiffHunk` has `oldStart`, `oldCount`, `newStart`, `newCount`, `header`,
and ordered `lines`. Counts may be zero for an insertion or deletion.

Each `DiffLine` has `kind`, `oldLineNumber`, `newLineNumber`, `text`, and
`segments`. `kind` is `context`, `addition`, or `deletion`. The line number
for a missing side is absent. `text` excludes the diff marker and line ending.
`segments` is an ordered list of `{ kind, text }` values. Segment `kind` is
`plain`, `addition`, or `deletion`. Producers compute intra-line word changes,
so every renderer shows the same emphasis.

The contract rejects contradictory values, including hunks on binary files,
line numbers on a missing side, or counts that disagree with the lines.
An empty `files` list represents no change. A consumer must show a truncated
warning and must not treat omitted content as approval of the complete edit.
The `id` travels with approval, so a stale view cannot approve a newer edit.

### Web renderer bars

The desktop renderer must support syntax highlighting, side-by-side mode,
unified mode, and intra-line word diff. It must bound work for large diffs and
add little shipped bundle weight. It must use a permissive license. Its styles
must map to the design tokens without fighting global CSS. It must not require
a frontend framework that this repository does not use.

| Candidate | Syntax highlighting | Side by side | Unified | Intra-line words | Large diffs | Shipped weight | License | Design-token theming | Framework coupling |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `diff2html` parser and HTML generator | Optional `highlight.js` adapter | Built in | Built in | Built-in word matching | Change, line-length, and comparison limits | Parser and generator can ship without the UI or highlighter bundles | MIT | CSS classes and replaceable templates need a token override sheet | Framework-neutral DOM output |
| `react-diff-view` | Token API with optional Refractor and worker | Built in | Built in | Token enhancer | Lazy rendering is possible, but the published 2.2 MB example renders slowly without it | Core plus React, React DOM, and optional tokenizer | MIT | Custom classes and token rendering | Requires React |
| `react-diff-viewer` | Caller supplies a syntax renderer | Built in | Built in | Built in through `jsdiff` | No documented virtualization or input limits | Component plus React, Emotion, and `jsdiff` | MIT | Light and dark style objects can map tokens, but Emotion owns output styles | Requires React |
| Svelte renderer over `jsdiff` | Must be built and integrated | Must be built | Must be built | `diffWords` supplies data | Async diffing, timeouts, and edit limits exist, but virtualization must be built | Small differ plus all renderer and highlighter code we build | BSD-3-Clause | Full control through Svelte and design tokens | Couples only to the existing Svelte stack |

`diff2html` meets every display bar without adding React. Its parser accepts
unified diff text, but the application will adapt the shared model into its
`DiffFile` input. The shared model remains authoritative. The desktop will use
the parser and HTML generator, its base stylesheet, a local token override
sheet, and the base UI adapter with a curated syntax highlighter. It will not
ship the all-language UI bundle.

The desktop defaults to unified mode inside the conversation. It offers
side-by-side mode when the available width can show both panes. It skips
syntax and word highlighting past explicit size limits, virtualizes by file,
and shows the complete unstyled lines. It never hides content silently.

The editor extension will not draw another web diff. It will expose each side
through read-only virtual documents and run the built-in `vscode.diff`
command. This keeps VS Code navigation, syntax highlighting, themes, and
accessibility behavior. The virtual document provider reads the same
`CodeDiff` value and preserves its `id`.

The CLI will render the shared model with a Rust terminal renderer. It will
use unified mode, terminal colors, and the same producer-supplied segments.
This ADR does not select that renderer.

### Implementation slices

Slice 2 owns the TypeSpec schema, generated Rust and TypeScript artifacts,
compatibility fixtures, publication, and exact dependency pins. It also owns
validation for empty, binary, truncated, malformed, contradictory, and stale
diffs. Those changes follow ADR 0019 and span the cloud publication repository
and each consuming manifest.

The first implementation slice in this repository starts after those
artifacts publish. It adds `src/lib/code-diff.js`,
`src/lib/CodeDiff.svelte`, `src/lib/code-diff.test.js`, and
`src/styles/code-diff.css`. It updates `src/App.svelte`, `src/App.test.js`,
`src/main.js`, `package.json`, and `package-lock.json`. That slice does not add
the VS Code or CLI renderer.

A later editor slice adds the virtual document provider and `vscode.diff`
command wiring under `editor-extension/src/`. A later CLI slice adds the
terminal renderer under `src-tauri/cli/`.

### Sources

- [`diff2html` documentation](https://github.com/rtfpessoa/diff2html)
- [`react-diff-view` documentation](https://github.com/otakustay/react-diff-view)
- [`react-diff-viewer` documentation](https://github.com/praneshr/react-diff-viewer)
- [`jsdiff` documentation](https://github.com/kpdecker/jsdiff)
- [VS Code extension API](https://code.visualstudio.com/api/references/vscode-api)

## Consequences

- All three surfaces display one versioned diff value.
- Desktop rendering adds `diff2html` and a curated syntax highlighter.
- The editor extension uses VS Code's native diff editor.
- Approval binds to the displayed diff identifier.
- Large, binary, empty, and truncated diffs have explicit behavior.
- Slice 2 must publish the shared contract before renderer work starts.
- This decision adds no dependency, generated file, renderer, or runtime code.
