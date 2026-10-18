# muniment-desktop — Design standard

Muniment desktop is a local harness for models, tools and work. Phase one puts
chat, projects, agents, artifacts and workspace tools first. Paid cloud follows
in phase two. Cloud and the company record have independent flags, both off by
default. The interface is the user's territory and the model is a visitor.

## Tokens

`src/styles/tokens.css` is the token source. Code has theme-aware syntax tokens. Light and dark are both
first-class, Framed Moss vibrant dark is the default for a new install, and a user override follows the user to every machine in a fleet.
Every theme carries ten core color tokens in one `:root[data-theme]`
block. Appearance offers four colors, Moss, Harbor, Clay and Iris, in three
rows: Normal, Vibrant and High contrast. Each pick has a light and a dark
version, and Mode picks between them, so System follows the OS with the
picked palette. Normal sets are quiet neutrals tinted by the color, with the color as signal and accent. Vibrant sets tint paper, add a brighter accent from the color's own family and color selection and the composer's focus edge with it. High contrast sets
hold pure ink on pure paper, with the color on strong hairlines and hovers. The table holds Moss light and Moss dark.
Every set adds `--accent`, `--accent-soft` and `--on-accent`. `--success`, `--active`, `--danger` and `--warning` color the states core reports.

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

Vibrant accents stay in the color's hue family: Moss with teal, Harbor with azure, Clay with terracotta and Iris with violet. Moss neutrals carry a faint green cast. The one
exception is Moss dark's black paper: its surface and faint keep the cast, so the
frame is black and every panel on it still ties to signal.

Type: Schibsted Grotesk for everything human, Commit Mono for everything that
is evidence. Every component type size resolves through the named `--text-*`
register. Desktop scale 12 / 13 / 15 body / 17 / 22 / 28, mono one step
smaller than adjacent body text, line-height 1.55 body and 1.3 headings. No
display serif. No italic except semantic emphasis in user content.
Appearance moves two things and nothing else: a size step scales every
`--text-*` token and `--text-provenance` together by a tenth per step, from
two steps down to four up, and a family picked from the device's installed
fonts sits in front of the shipped stack in `--font-human`, `--font-heading` or `--font-mono`. Headers have a separate picker.
The super key with `=`, `-` and `0` moves the same step. Both follow the
theme across the fleet and the shipped pair stays the default.

Panel appearance uses `data-panel` and `src/styles/panels.css`. `data-panel-variant` selects overlay or embedded styling. Components own layout, not panel appearance.
Shape: Settings tabs and the update control use `--radius-pill`. Other radii are `--radius-chip` 2, `--radius-control` 6, `--radius-panel` 10.
Hairline borders do the work. `--shadow-window` and `--shadow-overlay` are the only depth tokens. Motion is purposeful and
rare: the mark's thinking state, the active action's text sheen, the streaming underscore, the panel slide, a running agent's outline. `prefers-reduced-motion` removes all of it.

## Laws

1. **Color means computation, action, state and agents.** The setup mark's folded ear also uses
   `--signal`. Otherwise `--signal` appears only on the mark's thinking
   state, the streaming underscore and caret on
   the active line, the route segment of the provenance line, the live voice
   polish flash, the green globe of a machine that answers in the machine menu, and workflow-run
   indicators. Settings tabs, selected capability filters, verified updates and the Confirm check on an unconfirmed memory row use the theme signal.
   `--accent` fills each view's primary action (a `Button` marked primary or a submit) and an enabled switch. `StatusChip` colors a reported state: success online and done, active running, danger failed, warning queued and waiting, neutral offline and idle.
   Other buttons, links, icons at rest and badges are ink on paper, and selection reads `--selected`. A starter card's icon takes its `--tint-*` hue over a soft fill of it.
   `src/styles/signal-allowlist.test.js` enforces the list.
2. **If it is a record, it is mono.** Provenance lines, receipt rows, audit
   entries, costs, ids, versions, code, file paths and keyboard chips render in Commit
   Mono. Conversation, labels, headings, states, times and model names render in Schibsted Grotesk.
3. **Anti-patterns are hard fails.** No surface gradients, no violet, no glassmorphism
   or backdrop blur on a surface, no orbs or ambient animation, no assistant
   avatar, no typing dots, no decorative sparkles or wands, no emoji in UI
   copy, pill radius only on Settings tabs and switches, no "AI", "magic", "supercharge" or "unlock" in copy.
   The one blur is the routing details scrim: the workspace under it blurs
   behind the theme's paper, dark in dark mode and light in light mode, while
   the overlay covers most of it.
4. **Voice.** Sentence case everywhere. Buttons say what happens, in a label
   or, for the composer's send and stop control, in a glyph with an accessible
   name that says it. Errors state what happened and the next step and never
   apologize. Empty states are one line and no illustration, or a few starter cards that each start a flow: a new thread offers Make a page, Research a topic, Summarize a file and Set up an agent. The one
   exception is the record panel's first screen on a machine that holds no
   company: it carries a headline, one sentence, a proof line in mono, the
   create form, the sources and the sample company, and it still
   carries no illustration. No em dash in user-facing text. The shell never
   names its harness. A state line, an empty state and a composer hint are one
   line each and under twelve words. A state on a card or row is a short label, such as Manual or Never, not a sentence. A failed reply shows one sentence and Try again, and a Details disclosure holds the provider's reason and any raw record.

## The mark

The mark is the pocket-fold elephant from `brand/v2`, and its shapes live in `src/lib/pocket-fold.js`.
The setup lockup stands the 96px symbol over the wordmark, the body in `ink` and the ear in `signal`.
The record seal keeps the woven graph ring, whose vertices live in `src/lib/graph-mark.js`.
Native app icons are the pocket-fold symbol in white with its verdigris ear, centered on the dark brand card.
Provider sign-in pages show the 56px symbol. A finished sign-in plays it once: the elephant rears, curls the J,
flaps its ear and settles. Reduced motion shows it still.
The 20px chat mark is the pocket-fold symbol from `brand/v2`: the body in `muted` and its ear in `signal`. Appearance picks one of four indicators: the
motion elephant, ear flap only, the still elephant, or the stage word with a text sheen. The motion elephant moves with the run: it stands while the model routes or
writes, raises its front on the hind hip with both feet down and curls its trunk up into a J while it thinks, walks in the same raised pose while a tool runs, and runs in it through a burst of tools. Reduced motion stills every choice.

## Grammar

Layout is sidebar, thread, and one rail column for workspace tabs or the optional Record panel (⌘K).
Browser, Files, Terminal and artifact previews share tabs beside chat. Each agent or artifact has one dedicated chat with a goal and specified output, listed in its own sidebar section. Artifacts (⌘J) opens their catalog or a creation chat. The Agents, Artifacts and Projects catalogs share PanelPage and PageHead with Settings, so a title leads each, with its explanation behind an info control. An agent's action card takes the model picker's geometry: the avatar and the name, which shows a pencil on hover and edits in place, over a hairline, a column of 34px panel icons named on hover with the picked one on the selection fill and edge, and the picked panel's 30px rows beside it, the chosen row on the selection fill with a check. Each agent has a color, its colorful head's fill (a white or paper head draws in a hued fill from its seed), which tints its card header under a band of it, its template tile, its open sidebar row and its thread title. Cards keep one row height and their panels scroll inside. Files has creation icons beside its filter, multi-selection, and a context menu with red Delete that moves items to Trash. User
messages sit right in `faint` bubbles at radius 10. Another agent's message sits in a hairline card with its sender's avatar, a mono intent chip and its hop count, and a group reply carries its agent's avatar and name. Agent avatars are heads with no tile of their own from the brand's two generators, Colorful by default or Muniment as Appearance picks, in the theme's light or dark tone; their palettes stand outside the color law. An outline on the head names what the agent does: signal breathing while it runs, ochre while it waits, oxide after a failure. Responses sit plain on
`paper` with no bubble and no avatar, run the thread's full width inside a
36px gutter, and render as Markdown from the first token. The composer keeps
its 760px column, and the transcript scrolls on under it and fades into the
surface above it. Streaming is one signal underscore caret,
never dots. Tool activity folds under one `N tools used` line with a chevron.
It opens to groups of file reads, searches, commands and edits, indented. Each group opens its action list,
and each action opens its input, output, state and duration. A neutral text
sheen marks active actions and stops with reduced motion. Lucide icons name
the action type. The receipt's Tools row tallies the calls only when the transcript shows none, and the receipt never lists memory reads. Routing details appear only for a turn whose model the router chose, not for a model the user picked, and open in an overlay: the blurred scrim, a 1080 by 760 panel, and the turns scrolling inside. The provenance line sits under every response in mono at
`--text-provenance`, with the route in signal. Composer focus shifts the
border to `--focus-edge`: `muted`, or the accent in a Vibrant set, never signal.
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
controls and Linux keeps its decorations. The sidebar is dense: 28px rows, and a thread row carries a small meta line over its title, the machine's shape and name once a machine is paired, or `on Studio` for a thread another fleet member owns with an ochre slashed globe while that member is away, then the time ago. Hover is a lighter wash than the open thread,
New thread leads the sidebar, followed by Agents, Groups once a machine is in a fleet, Artifacts, Activity and title search, with no static Threads heading. Activity counts the agent runs that raised a notification since it last opened and the approvals waiting, in a mono chip, and lists those approvals above the runs. Once a runtime is paired, a runtime switcher row under New thread names the runtime in use. Agents then lists the agents of every machine that answers and every agent the fleet knows, each card with its primary machine's shape icon, an ochre slashed globe when that machine does not answer and a sync icon on a synced copy, and each row with one status mark, success while the machine answers, muted when it does not and a ring on a synced copy, whose hover names the machine and the activity, and Artifacts and Projects open on this computer.
Pinned threads precede recent threads, with a heading only when pins exist.
Search loads older titles and includes archives. Archived threads has Restore.
Pins and archives persist on the device. Projects group ordinary chats and automatically recall relevant sibling chat context before each reply. The Settings footer has one hairline. In a fleet a mono sync line sits above Settings, Synced, Syncing N or Offline with the last sync time, and opens the bootstrap panel: each class with a bar and each sign-in arriving from its holder or waiting for one. The cloud flag adds the account controls.
Hover or focus shows Rename, Pin or Unpin, Archive or Restore, and Delete
in one compact menu shared with the title. Shift and Command select rows. The count stays visible; Delete and Super+Delete open one dialog with Cancel focused.
Settings keeps the sidebar, its width, divider and collapse state, and swaps its threads for pages in the same rows: Back to threads where New thread sits, a search field, then Account, Models, Tools, Machines, Permissions, Data and General, with no group headings. Each setting lives on one page. While the search holds words, the rows keep the matching pages with their matching parts indented under them. A page of several parts, such as Data, gives each part a smaller head and a hairline above it. Each page has a Lucide icon and takes its sidebar label as its title, and builds from `src/lib/ui/` PanelPage, PageHead, Group and Row: the title and its info control with its actions at the right, then semibold headings over bordered cards of rows. A heading or row whose explanation adds to its label carries it behind the same `InfoTip`, and a row shows a support line only when it adds what a user needs to act.
The company-record flag adds Companies to Data. The cloud flag adds MuniCloud to Account, with phone pairing. It shows the
section across the thread and rail; the sidebar control, the composer's model chip and the
platform's settings shortcut, ⌘, on macOS and Ctrl+, on Windows and Linux,
open it. Back to threads or Escape returns focus to the opener.
Companies lists each company as a row with Open, Rename and Delete, where Delete asks once and names it, and New company last, then Sources and the sample company,
which open the record panel, and Backups. Account leads with the signed-in card, Sign out and the plan tiers. Memory carries Recall, the saved facts and Export.
Machines is one list: This computer first with its hostname in mono, then every machine and device by its own name, or by its kind while it announces only the generic runtime name, each with its shape icon, a hairline kind chip, its version, one state chip and Limited for a device without admin access: the update state the machine reports in place of Online while it answers, with blockers or the error in a tooltip, and the last-seen time while it does not answer. This computer's chevron opens Network, Available offline and Log under its row. The heading carries Add machine and, while an update waits, Update all. Add machine opens one panel with this computer's code and QR and a field for the other machine's link, with SSH inside it. Each other row's menu holds Update when the machine takes a request, Rename and a red Remove. Share model connections is one row under the list with its explanation behind its info control. Update all reports the machines it asked and one line of skips with their reasons.
Models leads with Roles, the routing and assistance switches and their decision models, then Connections. Account
rows span the page, with allowances visible and usage in details.
Routing statements sit between them. Connect
account opens the provider catalog and its connection methods. The model
chip shows the provider's mark in its brand colors beside the model id, and
opens a picker: a column of full-color provider marks, each named on hover, that scroll to their provider, every connected provider's current generation under its name, each provider's Legacy models folded under one row,
a search over every model, and Manage models at its foot. Each provider heading in the picker and the model on the provenance line carry the provider's mark. The
sidebar is resizable by its divider and collapses to nothing: no rail. Each divider
shows Lucide's vertical ellipsis as its grip, and on hover, focus or drag its gap fills with `border` that fades out at both ends.
The mark appears on the launch screen and in the thinking state, never in
the sidebar. Icons are Lucide, vendored as inline SVG at a 1.6px stroke. Appearance picks a frame beside the theme, and `src/styles/panels.css` derives both from the theme's tokens.
Both share one geometry, a one-pixel frame and a 35px title band, so a switch moves no content. Framed fills the frame with `paper`, sets the sidebar on it with no panel and quieter text,
and draws the thread and rail as `surface` cards clipped `--card-ring` inside their boxes with a hairline. Frameless fills it with `border` seams, a sidebar panel a step toward `ink`, and flat panels. The update control is a circular
down arrow beside the title-bar menu in `signal-soft` and `signal`. It reveals
`Update` on hover or focus after a signed download. Click installs and restarts.
The composer band is one row under the composer. The plus, the menu buttons and the context meter share their height and spacing. The machine, model, thinking level, usage and permission mode menus share one button: muted at rest, the hover fill in ink while hovered or open, and arrows that flip on machine, model and mode. The mode menu sits under a Modes heading: a row per mode with its Lucide icon (hand, pencil, map, zap, shield-check, triangle-alert), its title over one muted line, a check on the current one, Bypass permissions on a new thread by default, and a Shift+Tab chip at its foot. A narrow composer keeps each to its icon with a hover label.
The meter is a mono ring and percent that shows once a known window is a quarter full, ochre near the compaction point. A window no source knows shows no meter. The model picker shows no window and marks a model the conversation compacts on with the combine icon and `will compact` in ochre. The plus menu
opens with Add files and folders. Voice sits beside the send control. The band's one action control sits at its right
end: absent while the draft is empty, an ink up-arrow button once the draft has
text, and the same button with a filled stop square while a reply is in flight. Enter sends. A
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

## Tools

Settings → Tools holds MCP servers, Skills, Plugins and Search behind one `SettingsTabs` type filter,
with one Add in its head that opens the form for the picked type.
Their sidebar icons are the MCP mark,
`pencil-sparkles` and `plug-zap`, the composer menu uses the same three, `unplug` means disconnect only, and catalog cards keep the MCP mark. Search, category, installation and sort filters
search names, descriptions, publishers and categories in the official remote-server catalog.
Details open an in-app dialog with descriptions, connection details and provider links.
Directory relays and example servers stay out. Popularity uses the source catalog score.
Twelve popular entries precede the remaining catalog. Compact cards use three
columns, two in narrower panels, and one when needed. Each card shows its
provider logo with initials as a fallback. Pagination appears only for multiple pages. Provider marks show their brand colors on the first run and the model Settings pages, and the composer shows the light or dark mark.
Installed entries stay in catalog order and use the theme signal-soft surface.
An unlabeled accent switch controls availability, with an accessible name.
The Show filter includes Installed. Connect starts provider authentication when available.
A sliders icon reveals filters. Custom opens a dialog and attempts to load the service favicon.
Package review separates instructions, MCP servers and executable code.
Icon notices live in the bundled third-party notices, outside the catalog.

The composer plus sits first at the left and matches the composer menu buttons. The Agents, Artifacts and Projects rows reveal a plus on hover that creates one.
It has a background only on hover or keyboard focus.
Its three branches are MCPs, Plugins and Skills, followed by Manage tools.
The paperclip opens the OS picker directly. macOS accepts multiple files and folders together. Attachment chips are one 28px row: icon, name with extension, hover-only X. Folder paths reach local tools. All composer panels share one
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
Appearance spaces Browse color schemes below the frame control. Skills and plugins use
managed storage under `~/.muniment`, with Open folder in Tools.
Threads, agent chats and artifact chats use one-to-three-word generated titles.
Project cards keep their vertical ellipsis menu for actions, including Rename.
