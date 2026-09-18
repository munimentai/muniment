# muniment-desktop — Design standard

muniment desktop is the local app for muniment, the company system of record.
The interface is the user's territory and the model is a visitor. The design
language comes from institutions that hold things in trust: registries,
standards bodies, ledgers. Calm, permanent, meticulous about records. The
thread surface, the provenance line, the receipt and the local report all
render the same graph, and nothing in the shell is decoration.

## Tokens

`src/styles/tokens.css` is the token source. Light and dark are both
first-class, the OS picks the default, and a user override persists per device.
Every theme carries the same ten color tokens in one `:root[data-theme]`
block. The house sets are Paper, Vellum, Ledger and Foolscap in light and
Moss, Vault, Graphite and Inkwell in dark. Parchment, Manila and Linen carry
the light neutrals of Solarized, Gruvbox and Catppuccin, and Lagoon, Umber,
Fjord, Plum, Nocturne, Nightshade, Basalt and Obsidian carry the dark
neutrals of Solarized, Gruvbox, Nord, Catppuccin, Tokyo Night, Dracula,
Monokai and One Dark. Broadsheet and Carbon are the high contrast sets, pure
ink on pure paper with strong hairlines. Paper and Vault are the two defaults
below.

| Token | Light | Dark | Use |
| --- | --- | --- | --- |
| `--paper` | `#F6F7F6` | `#000000` | App background |
| `--surface` | `#FFFFFF` | `#0E1110` | Cards, composer, bars |
| `--faint` | `#EDEFEE` | `#161A18` | User bubbles, kbd chips, hover |
| `--ink` | `#1A1D1C` | `#ECEFED` | Text, primary buttons |
| `--muted` | `#5C6461` | `#9AA29E` | Secondary text, icons at rest |
| `--border` | `#E2E5E3` | `#262B28` | Hairlines |
| `--signal` | `#2A7264` | `#58B39F` | Computation only |
| `--signal-soft` | `rgba(42,114,100,.10)` | `rgba(88,179,159,.12)` | Signal backgrounds |
| `--oxide` | `#B4483E` | `#C96A61` | Deny, critical |
| `--ochre` | `#B98A2F` | `#CBA14E` | Caution, budget |

Every neutral carries a faint green cast that ties it to signal. The one
exception is Vault's black paper: its surface and faint keep the cast, so the
frame is black and every panel on it still ties to signal.

Type: Schibsted Grotesk for everything human, Commit Mono for everything that
is evidence. Every component type size resolves through the named `--text-*`
register. Desktop scale 12 / 13 / 15 body / 17 / 22 / 28, mono one step
smaller than adjacent body text, line-height 1.55 body and 1.3 headings. No
display serif. No italic except semantic emphasis in user content.
Preferences moves two things and nothing else: a size step scales every
`--text-*` token and `--text-provenance` together by a tenth per step, from
two steps down to four up, and a family picked from the device's installed
fonts sits in front of the shipped stack in `--font-human` or `--font-mono`.
The super key with `=`, `-` and `0` moves the same step. Both live on the
device beside the theme and the shipped pair stays the default.

Shape: radius `--radius-chip` 2, `--radius-control` 6, `--radius-panel` 10.
Nothing pill-shaped. Hairline borders do the work, and `--shadow-window` and
`--shadow-overlay` are the only two depth tokens. Motion is purposeful and
rare: the mark's thinking state, the streaming underline, the
panel slide. `prefers-reduced-motion` removes all of it.

## Laws

1. **Color means computation.** `--signal` appears only on the mark's thinking
   state, the streaming underline and caret on
   the active line, the route segment of the provenance line, the live voice
   polish flash, the enabled state of the Models show switch, and workflow-run
   indicators. Buttons, links,
   selection, icons at rest, badges and the mark at rest are ink on paper.
   `src/styles/signal-allowlist.test.js` enforces the list.
2. **If it is a record, it is mono.** Provenance lines, receipt rows, audit
   entries, costs, model names, file paths and keyboard chips render in Commit
   Mono. Conversation renders in Schibsted Grotesk.
3. **Anti-patterns are hard fails.** No gradients, no violet, no glassmorphism
   or backdrop blur on a surface, no orbs or ambient animation, no assistant
   avatar, no typing dots, no sparkles or wand iconography, no emoji in UI
   copy, no pill radius, no "AI", "magic", "supercharge" or "unlock" in copy.
   The one blur is the Settings scrim: the workspace under Settings blurs
   behind the theme's paper, dark in dark mode and light in light mode, while
   the popup covers most of it.
4. **Voice.** Sentence case everywhere. Buttons say what happens, in a label
   or, for the composer's send and stop control, in a glyph with an accessible
   name that says it. Errors state what happened and the next step and never
   apologize. Empty states are one line and no illustration. The one
   exception is the record panel's first screen on a machine that holds no
   company: it carries a headline, one sentence, a proof line in mono, the
   create form, the sources and the sample company, and it still
   carries no illustration. No em dash in user-facing text. The shell never
   names its harness. A state line, an empty state and a composer hint are one
   line each and under twelve words.

## The ring

The mark is a ring with a milled edge: a circle whose radius is modulated by a
uniform 22-tooth wave, `r(t) = 16.5 + 1.6·sin(22t)` in a 48-unit viewBox,
monoline stroke, round caps. At rest it is static ink. Thinking, it is
verdigris and animated: an irregular breath that flexes scale, stroke and
milling depth together, a spin that eases toward a new random target and often
stops, and a rare trace that runs the outline once. All visible instances
animate in sync as one organism. At 20px and below it renders as a solid
two-edge reduction. That reduction is the seal: the application icon is the
seal in verdigris on the dark brand card, and the lockup on the launch
screen renders the seal in ink at 34px so the mark there matches the icon.
[docs/design-reference/ring/muniment-ring-pulse-spin.html](docs/design-reference/ring/muniment-ring-pulse-spin.html)
is the reference geometry and animation engine.

## Grammar

Layout is sidebar, thread, and one rail column that the artifact rail (⌘J)
or the record panel (⌘K) fills, both closed by default. User
messages sit right in `faint` bubbles at radius 10. Responses sit plain on
`paper` with no bubble and no avatar, run the thread's full width inside a
36px gutter, and render as Markdown from the first token. The composer keeps
its 760px column, and the transcript scrolls on under it and fades into the
surface above it. Streaming is a 2px signal underline and a signal caret,
never dots. Tool activity draws no card: the mark in flight
names the running tool's verb, and the receipt's Tools row tallies the calls
when the reply lands. The provenance line sits under every response in mono at
`--text-provenance`, with the route in signal. Composer focus shifts the
border to `muted`, never signal.
Platform chrome follows the OS and brand tokens stay identical across platforms.
On macOS the app row sits in the 36px band above the panels beside the native
traffic lights, and the row's controls and the lights center on that band. The
row is the drag region and holds the sidebar toggle, New thread, the thread
title, and Artifacts then Record flush right with the update control beside
them. New thread, the thread title, Artifacts and Record are one quiet row
control with the same padding. State is background, never a border: the
composer and the thread title's rename control alone show hover, focus and
editing as the composer's muted hairline, and no control shows a focus ring or any
other focus state. Windows keeps its native caption
controls and Linux keeps its decorations. The sidebar is dense: 28px rows,
threads under project headings with one `Untitled` project by default,
hairlines edge to edge, and a foot of Settings above the account row, which
reads `Sign in to cloud` until an account signs in. A thread row shows a delete
control on hover and focus, Shift and Command clicks select rows, and the
right-click menu or the Delete key removes the selection after one confirm.
Settings is a popup over the workspace with a
section list on its left, Models, Appearance, Home, Companies and Account, and the
section on its right; the sidebar control, the composer's model chip and the
platform's settings shortcut, ⌘, on macOS and Ctrl+, on Windows and Linux,
open it, and Escape or its close control returns focus to the opener.
Companies lists every company on the machine with Open, Rename and Delete,
Delete asks once and names the company, and New company sits under the list. Models
lists connected providers with their real marks, a source tag, their models
with a show switch and Disconnect, then the popular providers not yet
connected as rows; Connect provider searches the rest, and a provider opens
on one view with its first method and the others one switch away. The model
chip shows the provider's mark in its brand colors beside the model id, and
opens a picker over the shown models with Manage models at its foot. The
sidebar is resizable by its divider and collapses to nothing: no rail.
The mark appears on the launch screen and in the thinking state, never in
the sidebar. Icons are Lucide, vendored as inline SVG at a 1.6px stroke. Sidebar, thread and rail sit
on `surface` inside a `paper` frame at `--radius-panel` with a hairline, and
the frame shows at every edge and between panels. The update control is a 20px
ink glyph that widens on hover or focus to read `Update` in mono, and it
appears only when a newer build is downloaded.
The composer band is one mono row under the composer: an Add files plus at
its left, then the model source chip,
the Home path, the context meter and the running cost, with the scan chip
beside them on the first run. The band's one action control sits at its right
end: absent while the draft is empty, an ink up-arrow button once the draft has
text, and a muted stop square while a reply is in flight. Enter sends. A
message sent while a reply is in flight steers it: the reply picks it up at its
next check, and the stop control ends the reply. The band shows no hint in
flight and names no delivery mode. The provenance line stays under each reply with
Copy at its right on the same line, the time takes no hover, and an expanded
receipt sits plain under it in mono, with no box. A receipt whose
record holds one row is the plain line with a clock glyph in place of the
chevron, and it does not expand.
The launcher is a 600 by 80 window on `surface` with a hairline and one
composer line, nothing else. A global shortcut opens it above every app,
centered in the upper third of the screen. Enter sends the line as the first
message of a new thread and brings the shell forward. Escape closes it. A microphone closes the row at its right, and a missing speech model opens a popover over the composer like the model picker: one sentence, the download size, the free disk required and one Install control, with the source and licenses one Details disclosure away. An attached file sits above the text under a hairline the composer's full inner width.

The record panel takes the rail column at a 480px minimum and its own
remembered width, and one control maximizes it over the sidebar and the
thread until ⌘K or Escape restores them. Its header is one mono row: the
company name as a picker, the kind list, a search field and Maximize. The
table view sets column headers, identifiers, dates, amounts, states and
sources in Commit Mono and the title and prose fields in Schibsted Grotesk,
with 28px hairline rows and no zebra fill. A cell in edit shows the
composer's muted hairline. A proposed change renders its diff in mono under
the row with Commit and Discard, and a warning from propose sits in ochre
text above them. The board's columns are the kind's states, its cards are
`surface` on `paper` at `--radius-control`, and a card in flight shows no
color. A workflow run in progress is the one place the panel shows signal.
A company with no records opens on Connect your data: one paragraph that
says what the company record is, then the sources as tiles, each under the
company's own mark with CSV file among them, and a tile opens the import on
that source with the kind chosen from the object. A kind list row reads the
kind's record count, or `none yet`. The kind toolbar is one mono row: the layout switch, Table or Board, is two
quiet controls with `aria-pressed`, the state filter and the saved-view
picker are 24px native selects, and Import, Save view and Ask are quiet
controls. A saved view is a `view` record and Save view proposes it like any
change. Ask sends the open view's SQL to the composer as a fenced block. The
empty state of a kind is one line. Under a table that holds more rows than it
shows, Show more is one quiet mono control that appends the next page. An
open record carries Link, Merge and Delete as quiet controls at the header's
right, each opening one form in the record's place: a relation and target
kind as 24px selects, a search field with a list of matches by title, the
diff, and Commit and Discard. On the kind list, Rename beside the company
picker turns the picker into one text field with Save. Import first lists
the sources as one quiet list, CSV file and every network source the sidecar
reads, with a mono note beside each. A file opens the file dialog. A network
source not yet connected shows one sentence and one field per credential it
asks for, a password field for a secret, then its objects as
the same list. The object then fills the kind body with one mono table of its
columns: the column name,
what its samples read as with three examples, and a 24px select of the
property it fills, with `skip` first. One select under it names the column
that keys each row. Propose mapping shows the mapping record's diff, Commit
and run applies it, and the run reports `n of total rows` as one mono status
line. The result is three mono lines, rows, counts and rows not placed, then
one mono table of row, title and reason for every row the mapping could not
place, with Done and Run again. A mapping record's view carries Run at the
header's right, where New sits for a kind.

Conversation, tool, permission, and receipt state is rebuilt from the
append-only local run journal. Reopen reduces committed events; snapshots are
disposable, and uncertain external effects require explicit attention rather
than silent replay.
A memory recall renders as one row with its query inside the expanded receipt and nowhere else.
Each saved attachment shows its media type when the record provides one. The image delivery rule appears once under the attachment list.
A code-diff gate renders the stored diff. It never re-reads the workspace for display.
A card that renders code sizes its layout from its own width rather than the window width. It stacks diff sides below 480 pixels.
A card that cannot show a stored change says whether Muniment applied the change.

The signed-in shell has one workspace `h1`, a headed thread list, and a transcript region named for the open thread.
An error message names the failure. The control beside it names and repeats the action that failed.
The background service notice reuses the auth error state's mono record register.
One owner starts, watches and stops the runtime for every window. When the runtime exits, every window shows the same one-sentence notice and one control that starts it again.
The notice waits out a two second dwell, so a drop shorter than that leaves the workspace on screen. A first status that already reports the service unreachable shows the notice at once.
An error that rejects one item from a set names that item.
A surface that renders model or user text wraps an unbreakable string.
A control renders as a control at rest.
A control presents a hit area of at least 24 by 24 CSS pixels.
The first run is the composer with three mono chips under it, the model source, the Home path and the scan result. A chip is a control at rest, opens its own panel, and never blocks Send. A scan row reads `Name: N files` in mono with a checkbox at rest.

Remote control: [docs/design-reference/remote-control-ux.md](docs/design-reference/remote-control-ux.md)
records the desktop session UX reference that mobile drives.
Components: [docs/design-reference/components.md](docs/design-reference/components.md)
lists every control and surface, its states, and the token each state reads.
