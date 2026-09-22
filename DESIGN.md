# muniment-desktop — Design standard

Muniment desktop is a local harness for models, tools and work. Phase one puts
chat, projects, agents, artifacts and workspace tools first. Paid cloud follows
in phase two. Cloud and the company record have independent flags, both off by
default. The interface is the user's territory and the model is a visitor.

## Tokens

`src/styles/tokens.css` is the token source. Code has theme-aware syntax tokens. Light and dark are both
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
fonts sits in front of the shipped stack in `--font-human`, `--font-heading` or `--font-mono`. Headers have a separate picker.
The super key with `=`, `-` and `0` moves the same step. Both live on the
device beside the theme and the shipped pair stays the default.

Panel appearance uses `data-panel` and `src/styles/panels.css`. `data-panel-variant` selects overlay or embedded styling. Components own layout, not panel appearance.
Shape: Settings tabs and the update control use `--radius-pill`. Other radii are `--radius-chip` 2, `--radius-control` 6, `--radius-panel` 10.
Hairline borders do the work. `--shadow-window` and `--shadow-overlay` are the only depth tokens. Motion is purposeful and
rare: the mark's thinking state, the active action's text sheen, the streaming underscore, the panel slide. `prefers-reduced-motion` removes all of it.

## Laws

1. **Color means computation.** The static setup brand graph also uses
   `--signal`. Otherwise `--signal` appears only on the mark's thinking
   state, the streaming underscore and caret on
   the active line, the route segment of the provenance line, the live voice
   polish flash, the enabled state of the Models show switch, and workflow-run
   indicators. Settings tabs, verified updates and enabled extension switches use the theme signal.
   Other buttons, links, selection, icons at rest and badges are ink on paper.
   `src/styles/signal-allowlist.test.js` enforces the list.
2. **If it is a record, it is mono.** Provenance lines, receipt rows, audit
   entries, costs, model names, file paths and keyboard chips render in Commit
   Mono. Conversation renders in Schibsted Grotesk.
3. **Anti-patterns are hard fails.** No surface gradients, no violet, no glassmorphism
   or backdrop blur on a surface, no orbs or ambient animation, no assistant
   avatar, no typing dots, no decorative sparkles or wands, no emoji in UI
   copy, pill radius only on Settings tabs, no "AI", "magic", "supercharge" or "unlock" in copy.
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

The mark is the woven graph ring. Its canonical vertices and size reductions
live in `src/lib/graph-mark.js`. The 20px, 32px and 56px variants use 22, 33
and 55 nodes with two connections per node and thicker lines at small sizes.
At 96px and above the full graph has 110 nodes, 220 connections and node dots.
The setup lockup shows the static 160px graph with the wordmark in its center.
Static marks retain the original proportions. Native icons use size reductions
in verdigris on the dark brand card. Provider callbacks use the static 56px graph.
Only the 20px chat mark moves: irregular pulse, eased random rotation, and
an occasional outline trace. The inner graph contracts during the pulse so
the center opening shrinks. Visible chat marks share one clock. Reduced
motion keeps the original static pose. No light balls traverse the graph.

## Grammar

Layout is sidebar, thread, and one rail column for workspace tabs or the optional Record panel (⌘K).
Browser, Files, Terminal and artifact previews share tabs beside chat. Each agent or artifact has one dedicated chat with a goal and specified output, listed in its own sidebar section. Artifacts (⌘J) opens their catalog or a creation chat. Files has creation icons beside its filter, multi-selection, and a context menu with red Delete that moves items to Trash. User
messages sit right in `faint` bubbles at radius 10. Responses sit plain on
`paper` with no bubble and no avatar, run the thread's full width inside a
36px gutter, and render as Markdown from the first token. The composer keeps
its 760px column, and the transcript scrolls on under it and fades into the
surface above it. Streaming is one signal underscore caret,
never dots. Tool activity groups file reads, searches, commands and edits.
Hover or keyboard focus reveals a chevron. Each group opens its action list,
and each action opens its input, output, state and duration. A neutral text
sheen marks active actions and stops with reduced motion. Lucide icons name
the action type. The receipt's Tools row tallies the calls when the reply lands. The provenance line sits under every response in mono at
`--text-provenance`, with the route in signal. Composer focus shifts the
border to `muted`, never signal.
Platform chrome follows the OS and brand tokens stay identical across platforms.
On macOS the app row sits in the 36px band above the panels beside the native
traffic lights, and the row's controls and the lights center on that band. The
row is the drag region and holds the sidebar toggle, the thread
title with its actions menu, optional Record, then the vertical ellipsis for workspace tools.
Seti icons identify file types. Scrollbars share a faint 3px thumb. The title and Record use a quiet row
control with the same padding. State is background, never a border: the
composer and the thread title's rename control alone show hover, focus and
editing as the composer's muted hairline, and no control shows a focus ring or any
other focus state. Windows keeps its native caption
controls and Linux keeps its decorations. The sidebar is dense: 28px rows,
New thread leads the sidebar, followed by Agents and title search, with no static Threads heading.
Pinned threads precede recent threads, with a heading only when pins exist.
Search loads older titles and includes archives. Archived threads has Restore.
Pins and archives persist on the device. Projects group ordinary chats and automatically recall relevant sibling chat context before each reply. The Settings footer has one hairline. The cloud flag adds the account controls.
Hover or focus shows Rename, Pin or Unpin, Archive or Restore, and Delete
in one compact menu shared with the title. Shift and Command select rows. The count stays visible; Delete and Super+Delete open one dialog with Cancel focused.
Settings is a popup over the workspace with a
section list on its left: Models & routing, Extend, Preferences, Profile & Memory and Storage.
The company-record flag adds Companies. The cloud flag adds Account. It shows the
section on its right; the sidebar control, the composer's model chip and the
platform's settings shortcut, ⌘, on macOS and Ctrl+, on Windows and Linux,
open it. Dismissal returns focus to the opener.
With the company-record flag enabled, Companies lists every company on the machine with Open, Rename and Delete,
Delete asks once and names the company, and New company sits under the list.
Models & routing has Accounts, Models, and Routing pill tabs. Account
rows span the page, with allowances visible and usage and weight in details.
A searchable model list holds visibility and routing statements. Connect
account opens the provider catalog and its connection methods. The model
chip shows the provider's mark in its brand colors beside the model id, and
opens a picker over the shown models with Models & routing at its foot. The
sidebar is resizable by its divider and collapses to nothing: no rail.
The mark appears on the launch screen and in the thinking state, never in
the sidebar. Icons are Lucide, vendored as inline SVG at a 1.6px stroke. Sidebar, thread and rail sit
on `surface` inside a `paper` frame at `--radius-panel` with a hairline, and
the frame shows at every edge and between panels. The update control is a circular
down arrow beside the title-bar menu in `signal-soft` and `signal`. It reveals
`Update` on hover or focus after a signed download. Click installs and restarts.
The composer band is one mono row under the composer. The horizontal ellipsis,
model selector and capacity control share their height and spacing. Voice and
the attachment paperclip sit beside the send control. The band's one action control sits at its right
end: absent while the draft is empty, an ink up-arrow button once the draft has
text, and a muted stop square while a reply is in flight. Enter sends. A
message sent while a reply is in flight steers it: the reply picks it up at its
next check, and the stop control ends the reply. The band shows no hint in
flight and names no delivery mode. The provenance line stays under each reply with
the execution time and icon-only Copy at the far right. User messages show local send time and Copy on hover or focus. Copy controls show a hover label. An expanded
receipt sits plain under it in mono, with no box. A receipt whose
record holds one row is the plain line with a clock glyph in place of the
chevron, and it does not expand.
The launcher is a 600 by 80 window on `surface` with a hairline and one
composer line, nothing else. A global shortcut opens it above every app,
centered in the upper third of the screen. Enter sends the line as the first
message of a new thread and brings the shell forward. Escape closes it. A microphone closes the row at its right, and a missing speech model opens a popover over the composer like the model picker: one sentence, the download size, the free disk required and one Install control, with the source and licenses one Details disclosure away. An attached file sits above the text under a hairline the composer's full inner width.

The optional Record panel uses the shared rail. Tables, boards and saved views
follow the company kinds. Records, amounts and evidence use mono. Proposed
changes show their diff before Commit. Companies settings and every record
entry point disappear together when the company-record flag is off.
File previews, code diffs and permission gates remain part of the local harness.
A diff uses the stored change and stacks its sides below 480 pixels.

The workspace has one `h1`, a headed thread list, and a transcript region named for the open thread.
An error message names the failure. The control beside it names and repeats the action that failed.
The background service notice reuses the auth error state's mono record register.
One owner starts, watches and stops the runtime for every window. When the runtime exits, every window shows the same one-sentence notice and one control that starts it again.
The notice waits out a two second dwell, so a drop shorter than that leaves the workspace on screen. A first status that already reports the service unreachable shows the notice at once.
An error that rejects one item from a set names that item.
Prose wraps unbreakable strings. Tab labels and path headers stay on one line.
A control renders as a control at rest.
A control presents a hit area of at least 24 by 24 CSS pixels.
The first run is the composer with three mono chips under it, the model source, the Home path and the scan result. A chip is a control at rest, opens its own panel, and never blocks Send. A scan row reads `Name: N files` in mono with a checkbox at rest.

Remote control: [docs/design-reference/remote-control-ux.md](docs/design-reference/remote-control-ux.md)
records the desktop session UX reference that mobile drives.
Components: [docs/design-reference/components.md](docs/design-reference/components.md)
lists every control and surface, its states, and the token each state reads.

The file panel shares the record panel rail, resize and maximize behavior.
The changed-files chip sits above the composer and opens its list on hover or click.
File links open an editable Monaco tab with syntax highlighting. Only actions with more details use disclosure arrows.
The account name shows a pencil on hover or focus and edits inline.

Composer URLs and file references use the theme-aware reference color.
The `@` file list supports arrows, Enter, Tab and Escape.

Workspace tabs keep a fixed width. Their scroll area hides its scrollbar and
fades clipped tabs without covering panel controls. Browser labels preserve the
host and URL path, query and fragment, with a right fade. Folder labels use the
last folder with a left fade. Terminal and Files share the full-path scroll
component. Hidden features leave no controls, Settings entries or empty space.

## Extend

Extend uses the shared Settings panel. MCP servers, Skills and Plugins occupy
three theme-colored pill tabs through `SettingsTabs`, shared with Models & routing.
Tab icons stay neutral on selection. MCPs use the MCP mark, Skills use
`pencil-sparkles`, and Plugins use `unplug`. Nonzero counts follow labels in smaller type. Search, category, installation and sort filters
search names, descriptions, publishers and categories in the official remote-server catalog.
Details open an in-app dialog with descriptions, connection details and provider links.
Directory relays and example servers stay out. Popularity uses the source catalog score.
Twelve popular entries precede the remaining catalog. Compact cards use three
columns, two in narrower panels, and one when needed. Each card shows its
provider logo with initials as a fallback. Pagination appears only for multiple pages.
Installed entries stay in catalog order and use the theme signal-soft surface.
An unlabeled signal toggle controls availability, with an accessible name.
The Show filter includes Installed. Connect starts provider authentication when available.
A sliders icon reveals filters. Custom opens a dialog and attempts to load the service favicon.
Package review separates instructions, MCP servers and executable code.
Icon notices live in the bundled third-party notices, outside the catalog.

The horizontal composer ellipsis sits first at the left and matches the model and capacity controls.
It has a background only on hover or keyboard focus.
Its three branches are MCPs, Plugins and Skills, followed by Manage extensions.
The paperclip beside Voice offers files and folders. Folder paths reach local tools. All composer panels share one
anchor above the full composer with an eight-pixel gap. Only one opens at a time.
Skill and plugin commands appear inline as slash commands in the reference color.
MCP switches and optional classifier selection apply to one turn. The next turn
starts without selected extensions. No extension chips sit below the message.

## Popups and names

Dialog headers use `PopupClose`: an X with an accessible name, a transparent
background at rest, and a theme hover background. Escape, X and a click outside
close the topmost popup. Clicking panel whitespace or dragging from inside to
outside does not close it. Nested dismissal leaves the parent open. A pending
operation blocks all dismissal paths together. Dismissal never approves an action.
Confirmation dialogs keep explicit decision buttons and focus the safe choice.
Menus close on selection, Escape or outside click without an extra close row.
Dialogs fit their content, cap height to the viewport, and scroll only as needed.
Preferences spaces Browse themes below the mode control. Skills and plugins use
managed storage under `~/.muniment`, with Open folder in Extend.
Threads, agent chats and artifact chats use one-to-three-word generated titles.
Project cards keep their vertical ellipsis menu for actions, including Rename.
