# muniment-desktop — Design standard

muniment desktop is the local app for muniment, the company system of record.
The interface is the user's territory and the model is a visitor. The design
language comes from institutions that hold things in trust: registries,
standards bodies, ledgers. Calm, permanent, meticulous about records. The
thread surface, the provenance line, the tool cards and the local report all
render the same graph, and nothing in the shell is decoration.

## Tokens

`src/styles/tokens.css` is the token source. Light and dark are both
first-class, the OS picks the default, and a user override persists per device.

| Token | Light | Dark | Use |
| --- | --- | --- | --- |
| `--paper` | `#F6F7F6` | `#141716` | App background |
| `--surface` | `#FFFFFF` | `#1C201E` | Cards, composer, bars |
| `--faint` | `#EDEFEE` | `#222624` | User bubbles, kbd chips, hover |
| `--ink` | `#1A1D1C` | `#E8EBE9` | Text, primary buttons |
| `--muted` | `#5C6461` | `#8A928E` | Secondary text, icons at rest |
| `--border` | `#E2E5E3` | `#2A2F2C` | Hairlines |
| `--signal` | `#2A7264` | `#58B39F` | Computation only |
| `--signal-soft` | `rgba(47,126,109,.10)` | `rgba(88,179,159,.12)` | Signal backgrounds |
| `--oxide` | `#B4483E` | `#C96A61` | Deny, critical |
| `--ochre` | `#B98A2F` | `#CBA14E` | Caution, budget |

Every neutral carries a faint green cast that ties it to signal. Never pure
`#FFF` or `#000`.

Type: Schibsted Grotesk for everything human, Commit Mono for everything that
is evidence. Every component type size resolves through the named `--text-*`
register. Desktop scale 12 / 13 / 15 body / 17 / 22 / 28, mono one step
smaller than adjacent body text, line-height 1.55 body and 1.3 headings. No
display serif. No italic except semantic emphasis in user content.

Shape: radius `--radius-chip` 2, `--radius-control` 6, `--radius-panel` 10.
Nothing pill-shaped. Hairline borders do the work, and `--shadow-window` and
`--shadow-overlay` are the only two depth tokens. Motion is purposeful and
rare: the mark's thinking state, the streaming underline, the tool pulse, the
panel slide. `prefers-reduced-motion` removes all of it.

## Laws

1. **Color means computation.** `--signal` appears only on the mark's thinking
   state, the running-tool status pulse, the streaming underline and caret on
   the active line, the route segment of the provenance line, the live voice
   polish flash, and workflow-run indicators. Buttons, links, focus rings,
   selection, icons at rest, badges and the mark at rest are ink on paper.
   `src/styles/signal-allowlist.test.js` enforces the list.
2. **If it is a record, it is mono.** Provenance lines, tool activity, audit
   entries, costs, model names, file paths and keyboard chips render in Commit
   Mono. Conversation renders in Schibsted Grotesk.
3. **Anti-patterns are hard fails.** No gradients, no violet, no glassmorphism
   or backdrop blur, no orbs or ambient animation, no assistant avatar, no
   typing dots, no sparkles or wand iconography, no emoji in UI copy, no pill
   radius, no "AI", "magic", "supercharge" or "unlock" in copy.
4. **Voice.** Sentence case everywhere. Buttons say what happens. Errors state
   what happened and the next step and never apologize. Empty states are one
   line and no illustration. No em dash in user-facing text. The shell never
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
two-edge reduction. [docs/design-reference/ring/muniment-ring-pulse-spin.html](docs/design-reference/ring/muniment-ring-pulse-spin.html)
is the reference geometry and animation engine.

## Grammar

Layout is sidebar, thread, and artifact rail (⌘J, closed by default). User
messages sit right in `faint` bubbles at radius 10. Responses sit plain on
`paper` with no bubble and no avatar. Streaming is a 2px signal underline and a
signal caret, never dots. Tool activity is an inline mono card with a status
dot that pulses while running and collapses to its header when done. The
provenance line sits under every response in mono at `--text-provenance`, with
the route in signal. Composer focus shifts the border to `muted`, never signal.
Platform chrome follows the OS and brand tokens stay identical across platforms.
On macOS the app row sits in the title bar band beside the native traffic
lights, is the drag region, and holds the sidebar toggle, New thread, the
thread title, Artifacts and the update control. Windows keeps its native
caption controls and Linux keeps its decorations. Sidebar, thread and rail sit
on `surface` inside a `paper` frame at `--radius-panel` with a hairline, and
the frame shows at every edge and between panels. The update control is a 20px
ink glyph that widens on hover or focus to read `Update` in mono, and it
appears only when a newer build is downloaded.
The composer band is one mono row under the composer: the model source chip,
the Home path, the context meter and the running cost, with the scan chip
beside them on the first run. The provenance line stays under each reply.
The launcher is a 600 by 80 window on `surface` with a hairline and one
composer line, nothing else. A global shortcut opens it above every app,
centered in the upper third of the screen. Enter sends the line as the first
message of a new thread and brings the shell forward. Escape closes it.

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
