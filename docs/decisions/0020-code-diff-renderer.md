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

| Candidate | Syntax highlighting | Side by side | Unified | Intra-line words | Large diffs | Shipped weight | License | Design-token theming | Framework coupling | React Native |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `diff2html` parser and HTML generator | Optional `highlight.js` adapter | Built in | Built in | Built-in word matching | Change, line-length, and comparison limits | Parser and generator can ship without the UI or highlighter bundles | MIT | CSS classes and replaceable templates need a token override sheet | Framework-neutral DOM output | No. Its output is HTML and CSS, not native `View` and `Text` components. |
| `react-diff-view` | Token API with optional Refractor and worker | Built in | Built in | Token enhancer | Lazy rendering is possible, but the published 2.2 MB example renders slowly without it | Core plus React, React DOM, and optional tokenizer | MIT | Custom classes and token rendering | Requires React | No. React alone is insufficient because the component renders DOM tables and imports CSS. |
| `react-diff-viewer` | Caller supplies a syntax renderer | Built in | Built in | Built in through `jsdiff` | No documented virtualization or input limits | Component plus React, Emotion, and `jsdiff` | MIT | Light and dark style objects can map tokens, but Emotion owns output styles | Requires React | No. The component renders DOM elements and uses Emotion's web styling. |
| Svelte renderer over `jsdiff` | Must be built and integrated | Must be built | Must be built | `diffWords` supplies data | Async diffing, timeouts, and edit limits exist, but virtualization must be built | Small differ plus all renderer and highlighter code we build | BSD-3-Clause | Full control through Svelte and design tokens | Couples only to the existing Svelte stack | No. `jsdiff` can supply data, but the Svelte web renderer cannot supply native components. |

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

### Terminal renderer bars

The CLI renderer must consume `CodeDiff` without computing another diff. It
must preserve the `muniment-cli` dependency boundary. It must support color
and plain output. It must define bounded, predictable output for every model
state. Syntax highlighting is optional because the shared segments already
carry the required intra-line emphasis.

The scores use `++` for a strong fit, `+` for a fit with some work, `-` for a
poor fit, and `--` for a conflict.

| Candidate | Shared-model fit | `muniment-cli` boundary | Color and plain output | Bounded behavior | Syntax highlighting |
| --- | --- | --- | --- | --- | --- |
| `similar` plus `anstyle` | `--` computes a diff the producer already supplied | `--` requires at least two new allowlist entries | `++` with `anstyle` policy | `+` supports diff deadlines, but rendering remains custom | `--` none |
| `imara-diff` plus `anstyle` | `--` computes a diff the producer already supplied | `--` requires at least two new allowlist entries | `++` with `anstyle` policy | `+` bounds pathological diff computation, but rendering remains custom | `--` none |
| `diffy` | `-` expects or computes patches instead of consuming `CodeDiff` | `--` requires a new allowlist entry and optional color dependencies | `+` has colored and plain patch formatters | `-` does not define this model's truncation or wrapping policy | `--` none |
| Hand-written renderer plus `console` | `++` reads the model directly | `--` requires `console` and its support crates in the allowlist | `++` delegates terminal styling | `++` keeps all output policy local | `--` none |
| Hand-written renderer plus `syntect` | `++` reads the model directly | `--` adds a large highlighting dependency tree to the allowlist | `+` still needs terminal styling policy | `+` must bound highlighting work | `++` built in |
| Hand-written standard-library renderer | `++` reads the model directly | `++` adds no crate and keeps the allowlist unchanged | `+` owns a small ANSI policy | `++` keeps all output policy local | `--` none |

The CLI selects the hand-written standard-library renderer. The CLI has no
need for a diff algorithm because `CodeDiff` contains hunks, lines, and
segments. Syntax highlighting does not justify `syntect` and its dependency
tree. The selection adds no crate, so `test/cli-dependency-boundary.sh` and
its allowlist remain unchanged.

The renderer uses unified mode. It writes ANSI SGR colors only when stdout is
a terminal and `NO_COLOR` is absent. It colors additions green, deletions red,
and file and hunk headers cyan. It emphasizes producer-supplied addition and
deletion segments with bold color. Plain output contains no ANSI bytes and
keeps the usual space, `+`, and `-` line markers.

The renderer never inserts hard wraps and never truncates a line. The terminal
may soft-wrap a long line at its viewport edge. This behavior preserves every
model character and keeps redirected output stable.

For a binary file, it prints the paths and `Binary file changed`, with no
hunks. For an empty `files` list, it prints `No changes.`. For a truncated
diff, it prints `Warning: This diff is truncated.` before any file output.
That warning remains present in color and plain output.

### Implementation slices

Slice 2 selects the hand-written standard-library terminal renderer. It also
records that none of the web renderers run unchanged under React Native.
`jsdiff` remains a possible data-layer input to a separate native renderer.

A later contract slice owns the TypeSpec schema, generated Rust and TypeScript
artifacts, compatibility fixtures, publication, and exact dependency pins. It
also owns validation for empty, binary, truncated, malformed, contradictory,
and stale diffs. Those changes follow ADR 0019 and span the cloud publication
repository and each consuming manifest.

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
- [`similar` documentation](https://docs.rs/similar/latest/similar/)
- [`imara-diff` documentation](https://docs.rs/imara-diff/latest/imara_diff/)
- [`diffy` documentation](https://docs.rs/diffy/latest/diffy/)
- [`anstyle` documentation](https://docs.rs/anstyle/latest/anstyle/)
- [`console` documentation](https://docs.rs/console/latest/console/)
- [`syntect` documentation](https://docs.rs/syntect/latest/syntect/)
- [`std::io::IsTerminal` documentation](https://doc.rust-lang.org/std/io/trait.IsTerminal.html)
- [`NO_COLOR` convention](https://no-color.org/)
- [React Native core components](https://reactnative.dev/docs/components-and-apis)
- [VS Code extension API](https://code.visualstudio.com/api/references/vscode-api)

## Amendment — 2026-08-01: desktop build order

Two measured facts block the implementation slices above. First, the public
npm registry returned 404 for `@muniment/e0-code-diff-v1` on 2026-08-01. The
expected TypeScript and Rust artifacts come from this document's declaration
at `docs/decisions/0020-code-diff-renderer.md:25-29`. No repository manifest
names either artifact. In particular, the desktop dependencies at
`package.json:36-42` name neither the TypeScript package nor `diff2html`.
The repository has no `.npmrc` that could select a private registry.

Second, this repository has no producer of a `CodeDiff` value.
`src-tauri/src/chat_coordinate.rs:690-713` journals only the effect identifier,
display name, and completion state. `src-tauri/core/src/sidecar/pi_chat.rs:221-238`
reads only `toolCallId` and `toolName` from `tool_execution_start`.
`src-tauri/core/src/journal/reducer.rs:178-204` defines four permission kinds.
None carries a file path, before text, after text, patch, or `CodeDiff`. The
`confirm` kind carries only a title, message, and optional timeout.

The desktop may add `src/lib/code-diff.js` and its renderer before the generated
contract artifacts publish. That slice is presentation-only and accepts an
already validated `CodeDiff` value at its boundary. It adds no runtime wiring.
ADR 0019 still forbids this repository from restating the wire type, validator,
or endpoint client. That ban includes a hand-vendored copy of the schema or
generated shape used as a substitute for the published artifact.

On the day both artifacts publish, the desktop pins the TypeScript package in
`package.json` and `package-lock.json`. The pin uses the exact published
version. The runtime boundary imports the generated types and codecs. Consumer
compatibility tests then read the published fixtures. A Rust surface pins the
crate in its manifest and lockfile when it first consumes `CodeDiff`. Neither
version-pin change infers or adds a producer.

The producer belongs to the follow-up decision in
`docs/decisions/0024-code-diff-producer.md`. That decision names the trusted
runtime boundary, journal replay rules, and approval binding.
The first implementation slice adds `src/lib/code-diff.js`, its tests, and the
desktop renderer named above. It can proceed before publication or ADR 0024.
Permission-gate and receipt-replay wiring wait for both prerequisites. They
consume the journal value that ADR 0024 decides.

ADR 0024 now selects the Tauri core as the trusted producer. It stores each
validated value in CAS and binds approval to its `id` and exact CAS hash.
Truncated and binary values remain display-only. This decision's
presentation adapter continues to accept only a validated value.

## Consequences

- All three surfaces display one versioned diff value.
- Desktop rendering adds `diff2html` and a curated syntax highlighter.
- The editor extension uses VS Code's native diff editor.
- The CLI renderer adds no dependency and leaves its dependency check unchanged.
- Approval binds to the displayed diff identifier.
- Large, binary, empty, and truncated diffs have explicit behavior.
- None of the evaluated web renderers can serve as a React Native renderer unchanged.
- Later contract work publishes the shared model before renderer implementation.
- This decision adds no dependency, generated file, renderer, or runtime code.
