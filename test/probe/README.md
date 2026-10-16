# Browser probe

Run `npm run probe` from the repository root. The command builds the frontend and prints the local probe URLs.

Run `npm run probe:check` to build the frontend and check every fixture in headless Chromium.

Open `index.html` for an empty signed-in workspace. Open `history.html` for completed and interrupted fixture runs.
Open `local-mode.html?thinking` for local mode on a model with thinking levels, with the level control beside the model chip.
Open `in-flight.html` for a thread whose newest run still streams its reply while a tool runs.
Open `access.html` for the signed-in shell with the profile popover open.
Open `signed-out.html` for the signed-out screen.
Open `markdown.html` for a completed Markdown reply.
Open `onboarding.html` for first-run setup.
Open `approved-files.html` for the approved-files review.
Open `permission.html` for a run paused on a permission decision.
Open `code-diff.html` for a run paused on proposed file changes.
Open `code-diff-unavailable.html` for a run whose proposed file changes cannot be shown.
Open `applied-diff.html` for a settled run with applied file changes.
Open `select.html` for a run paused on a choice request.
Open `input.html` for a run paused on a single-line text request.
Open `editor.html` for a run paused on a multi-line text request.
Open `runtimes.html` for Settings → Runtimes with two paired runtimes, one offline and one older. `?state=local` holds this computer alone, `?state=pairing` and `?state=failed` paste a link and pair, `?state=ssh` fills the SSH form, `?state=switcher` opens the sidebar switcher, and `?state=remote` selects a paired runtime. This computer shows pairing off in `?state=pairing-off`, on with its addresses in `?state=pairing-on`, a fresh link with its QR code in `?state=link`, and paired devices in `?state=devices`.

Open `agents.html` for agents on three machines in the catalog and sidebar. `?state=profile` opens an agent's profile, `?state=cron`, `?state=webhook` and `?state=mcp` open a routine's trigger editor, `?state=proposals` shows routines proposed from instructions, and `?state=activity` opens Activity.

Inspect `window.__PROBE__.invokedCommands` and `window.__PROBE__.eventListeners` in the browser console. Emit an event with `window.__PROBE__.emit(event, payload)`.

Capture every fixture at the default desktop size:

```sh
for fixture in index history local-mode in-flight access markdown signed-out onboarding approved-files permission select input editor code-diff code-diff-unavailable applied-diff runtimes agents; do
  playwright screenshot --browser chromium --viewport-size "1100,720" --wait-for-selector "[data-probe-ready]" "http://127.0.0.1:4173/test/probe/$fixture.html" "/tmp/muniment-probe-$fixture.png"
done
```
