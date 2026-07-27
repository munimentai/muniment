# Browser probe

Run `npm run probe` from the repository root. The command builds the frontend and prints both local probe URLs.

Open `index.html` for an empty signed-in workspace. Open `history.html` for completed and interrupted fixture runs.

Inspect `window.__PROBE__.invokedCommands` and `window.__PROBE__.eventListeners` in the browser console. Emit an event with `window.__PROBE__.emit(event, payload)`.

Capture the restored history at the default desktop size:

```sh
playwright screenshot --browser chromium --viewport-size "1100,720" http://127.0.0.1:4173/test/probe/history.html /tmp/muniment-probe.png
```
