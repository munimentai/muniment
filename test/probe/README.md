# Browser probe

Run `npm run probe` from the repository root. The command builds the frontend and prints the local probe URLs.

Run `npm run probe:check` to build the frontend and check every fixture in headless Chromium.

Open `index.html` for an empty signed-in workspace with the new thread's starter cards. Add `?theme=` with a theme id, such as `clay-vibrant-dark`, to open it in that color scheme. Open `history.html` for completed and interrupted fixture runs.
Open `local-mode.html?thinking` for local mode on a model with thinking levels, with the level control beside the model chip.
Open `index.html?accounts&fleet` for Settings → Models on a fleet: one sign-in on this machine, one on Studio with Hold sign-in here, and Signed in on other machines with a sign-in arriving from Studio and one Pod keeps. Add `&theme=light` or `&theme=dark`.
Open `in-flight.html` for a thread whose newest run still streams its reply while a tool runs.
Open `access.html` for the signed-in shell with the profile popover open.
Open `signed-out.html` for the signed-out screen.
Open `markdown.html` for a completed Markdown reply.
Open `long-thread.html` for a thread of 5,000 settled turns, or `?turns=` that many. The transcript renders a bounded window of them.
Open `onboarding.html` for first-run setup.
Open `approved-files.html` for the approved-files review.
Open `permission.html` for a run paused on a permission decision.
Open `permission-modes.html` for a composer thread in Ask before changes, paused on core's gate for a command, with the mode on the composer button. `?state=menu` opens the composer's Modes menu, `?state=new` opens it on a new thread, which starts in Bypass permissions, `?state=plan` pauses the thread in Plan first on a plan with its four answers, and `?state=settings` opens Settings → Permissions.
Open `code-diff.html` for a run paused on proposed file changes.
Open `code-diff-unavailable.html` for a run whose proposed file changes cannot be shown.
Open `applied-diff.html` for a settled run with applied file changes.
Open `run-trust.html` for two turns that edited a folder through file tools and shell commands. `?state=receipt` opens each turn's receipt, where only the turn that changed files offers Review changes, `review` opens Review changes on the notes, `rewind` previews a rewind to before the second message, `blocked` previews one while another reply uses the folder, and `rewound` runs it with one file that fails.

Open `tools.html` for the tools gallery: one turn that called every kind of tool core describes, opened to each call's details, and a turn waiting at a web search gate. The presentations come from `core/protocol-fixtures/tool-presentation/presented.json`, which a core test writes from `calls.json` with `MUNIMENT_WRITE_FIXTURES=1`. `agents.html?state=tool-approvals` shows the gallery's gates as agent approvals.
Open `select.html` for a run paused on a choice request.
Open `input.html` for a run paused on a single-line text request.
Open `editor.html` for a run paused on a multi-line text request.
Open `runtimes.html` for Settings → Machines with two paired runtimes, one offline and one older. `?state=local` holds this computer alone, `?state=scope` opens Settings on All machines with the foot row on the Settings side, `?state=pairing` and `?state=failed` open Add machine and paste a link, `?state=ssh` fills its SSH form, `?state=switcher` opens a new thread's machine menu, and `?state=remote` selects a paired runtime and opens the thread that lives on it. This computer's row shows connections off beside an open Add machine in `?state=pairing-off`, and on with its addresses in `?state=pairing-on`. `?state=link` shows a fresh code with its QR code, and `?state=devices` merges the devices that use this computer into the list.

Open `agents.html` for agents on three machines in the catalog and sidebar, with a synced copy from a fourth swarm member that does not answer. `?state=profile` opens an agent's profile, `?state=cron` and `?state=mcp` open a routine's trigger editor, `?state=webhook` opens a webhook routine's address with a rotated secret, `?state=proposals` shows routines proposed from instructions, `?state=notifications` opens Notifications with approvals, a question, a usage stop, an offline machine, two runs in flight and runs by day, and `?state=usage`, `usage-tokens`, `usage-cost` and `usage-partial` open Usage at Limits, Tokens, Cost over 30 days, and Tokens with the build box not answering. `?state=composer-limit` shows the composer's limit banner. `?state=machines` opens Settings → Machines with every fleet member in one list, `?state=move` opens a forced move off an offline primary, `?state=transcript` opens a thread with another agent's message, a handoff and a silent run, `?state=group-create` fills New group, `?state=meeting` opens a group's meeting rounds with the members in the composer band, `?state=approvals` lists approval cards in Notifications (Allow once and Deny, a folded long input, an auto review's reason, a plan with its four answers and an older runtime's gate), `?state=approval-inline` shows one in the agent's thread, `?state=replays` opens an agent's run history with its failed runs and their Retry actions, `?state=import` previews an imported agent, `?state=absence` shows the resume prompt, `?state=preferences` opens General at the Agent avatars choice, `?state=empty` shows the empty catalog and `?state=notifications-empty` shows Nothing needs you with the next scheduled runs. `?state=builder` describes an agent in one sentence and shows the drafted form, `?state=builder-question` shows the one question beside its draft, and `?state=builder-thread` drafts from a thread through Make this an agent. `?state=cards` shows the catalog's action cards; add `&panel=overview`, `model`, `permissions`, `routines` or `history` to open that panel on every card, or `&edit=name` to open the first card's name field. Add `&models` to load the probe's providers into the model pickers. `?state=cards-offline` takes the Studio machine offline, so its agent shows a read-only card with a note, and `&studio-offline` takes it offline in any state, such as Notifications with Studio's runs on a red machine chip. The sidebar and catalog show each agent activity state: running, waiting, idle and error. Add `&avatars=colorful` or `&avatars=muniment` and `&theme=light` or `&theme=dark` to any state.

Open `fleet-sync.html` for a laptop in a fleet with a Studio desktop that is away and a Pod container, reached only through the swarm. `?state=synced`, `syncing` and `offline` show the sidebar, whose foot names no sync state, `bootstrap` the Sync group on Settings → Machines with each class and the sign-ins arriving or waiting, `owners` the owner chips on threads and projects, `queued` a message queued for offline Studio with Cancel and Move here, `replica` a thread whose history still arrives from Pod, `offload` the machine menu starting a new thread on Pod, `offline-settings` this computer's settings on Machines at Available offline, `log` the machine log, and `projects` the Projects catalog with Studio's and Pod's projects after this laptop's. Add `&theme=light` or `&theme=dark`.

Open `avatars.html` for 48 agent heads of each style, Colorful and Muniment, at the sidebar, card and profile sizes in Moss light and Moss dark, with center guides and each drawing's bounds. `?style=` and `?theme=` narrow it to one panel, and `&compact` shows the profile size alone.

`npm run probe:check` also runs the speed budgets in `speed.mjs` and the layout stability walk in `stability.mjs`. `docs/speed-budgets.md` lists each budget.

Inspect `window.__PROBE__.invokedCommands` and `window.__PROBE__.eventListeners` in the browser console. Emit an event with `window.__PROBE__.emit(event, payload)`.

Capture every fixture at the default desktop size:

```sh
for fixture in index fleet-sync history local-mode in-flight access markdown signed-out onboarding approved-files permission permission-modes select input editor code-diff code-diff-unavailable applied-diff runtimes agents; do
  playwright screenshot --browser chromium --viewport-size "1100,720" --wait-for-selector "[data-probe-ready]" "http://127.0.0.1:4173/test/probe/$fixture.html" "/tmp/muniment-probe-$fixture.png"
done
```
