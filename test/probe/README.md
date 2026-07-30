# Browser probe

Run `npm run probe` from the repository root. The command builds the frontend and prints the local probe URLs.

Open `index.html` for an empty signed-in workspace. Open `history.html` for completed and interrupted fixture runs.
Open `onboarding.html` for first-run setup.
Open `approved-files.html` for the approved-files review.
Open `permission.html` for a run paused on a permission decision.
Open `input.html` for a run paused on a single-line text request.
Open `editor.html` for a run paused on a multi-line text request.

Inspect `window.__PROBE__.invokedCommands` and `window.__PROBE__.eventListeners` in the browser console. Emit an event with `window.__PROBE__.emit(event, payload)`.

Capture the restored history at the default desktop size:

```sh
playwright screenshot --browser chromium --viewport-size "1100,720" --wait-for-selector "[data-probe-ready]" http://127.0.0.1:4173/test/probe/history.html /tmp/muniment-probe.png
```
