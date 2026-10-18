# Speed budgets

The desktop keeps two kinds of speed budget. Pull request checks count work
that runner noise cannot move. A nightly script times the installed app on one
dedicated Mac.

## Pull request gates

`npm run probe:check` runs these checks in headless Chromium against the probe
fixtures. Each one counts, so a slow runner cannot fail it and a fast one
cannot hide a regression. `test/probe/speed.mjs` holds the budgets.

| Check | What it counts | Budget |
| --- | --- | --- |
| Start | Requests the window sends until it settles, polls left out | 47 on an empty thread, 54 on a restored thread |
| Start | `sidebar_arrange` calls, and no identical input twice in a row | 2 empty, 5 restored |
| Streamed chunk | DOM writes outside the reply that streams | 0 |
| Streamed chunk | DOM writes inside the reply | 4 |
| Streamed chunk | Requests and `sidebar_arrange` calls | 0 |
| Settled reply | `sidebar_arrange` calls | 1 |
| Keystroke | DOM writes outside the composer | 0 |
| Keystroke | DOM writes inside the composer | 3 |
| Keystroke | Requests | 0 |
| Long thread | Rendered items, elements and Markdown blocks on open | 120, 2,000, 60 |
| Long thread | 500 turns and 5,000 turns render the same DOM | equal |

The streamed and keystroke checks run with reduced motion, so the run mark's
animation frames never count. A run's elapsed-time clock ticks each second and
does not count either. The 1-second `runtime_state` poll is left out of every
request count.

`long-thread.html` seeds 5,000 settled turns, or `?turns=` that many. The
transcript renders a window of at most 120 items, set in
`src/lib/transcript-window.js`. It opens on the newest 40, adds 40 older items
when the reader scrolls near the top, and drops items at the far end past the
cap. Jump to latest returns it to the newest items. The history pages
themselves still load whole: one `chat_thread_open` per 100 turns.

`test/probe/stability.mjs` records the boxes of the composer band's controls
and the sidebar head, then walks the window through hover, focus, a typed and a
cleared draft, Settings, a streamed reply and its end. A control that moves by
more than half a pixel fails the check. `test/probe/focus.mjs` tabs through the
window in four themes and fails when a focused control shows no 2px ring.

## Nightly timing

`scripts/speed-timing.mjs` times the app and writes a JSON report.
`scripts/speed-budgets.json` holds the budgets it compares against.

```sh
# Launch the e2e WebDriver build with an isolated home and time it.
node scripts/speed-timing.mjs --app "$APP/Contents/MacOS/muniment" --home "$RUN_HOME" --out speed-report.json

# Time an e2e build that already runs, and fail on a budget or a regression.
node scripts/speed-timing.mjs --webdriver http://127.0.0.1:4445 --baseline last-report.json --strict

# Dry run the page measurements on the probe fixtures, with no app.
npm run build && node scripts/speed-timing.mjs --probe
```

The e2e build comes from `tauri build --features e2e-webdriver --config
src-tauri/tauri.e2e.conf.json` and serves WebDriver on `TAURI_WEBDRIVER_PORT`,
4445 unless `--port` names another. Every page script runs through WebDriver's
async execute, because the synchronous one does not answer on macOS. Use a home
whose profile has finished first-run setup and connected one model, or the
composer never shows.

| Metric | Budget | How it is read |
| --- | --- | --- |
| `launchToComposerMs` | 1000 | Process start to the `muniment:composer-ready` performance mark |
| `pageToComposerMs` | 1000 | Page time origin to the same mark |
| `keystrokeP95Ms` | 16 | 60 keys: input event to settled effects and layout, 95th percentile |
| `keystrokeFrameP95Ms` | none | The same keys to the next frame, for reference |
| `sendToFirstTokenMs` | 3000 | Send to the first painted reply text, with `--prompt` |
| `firstTokenEventToPaintMs` | 50 | First text event to its painted frame, with `--prompt` |
| `idleCpuPercent` | 0.5 | CPU time of the app and runtime processes over `--idle` seconds, 60 by default |
| `idleFootprintMb` | 300 | macOS `footprint` of the same processes after the idle window |
| `idleRssMb` | 600 | Resident memory of the same processes, which overstates WebKit's share |

The script finds the app's process tree and every `muniment-runtime` process,
since the runtime can run as a background service outside that tree. With
`--baseline`, a metric more than 10% over the last report fails under
`--strict` even inside its budget. A failed nightly number then needs a written
reason in the pull request that caused it.
