# Local mode

The desktop can open the thread surface without a Muniment account. Select **Use local mode** from the signed-out screen.

Local mode does not contact the Muniment control plane for authentication or model access. Pi runs on the device and reads its provider credentials from Pi's credential store. The desktop does not pass a Muniment virtual key to Pi in local mode.

## Window size

The desktop opens at 1100 by 720 logical points unless a saved size exists.
It clamps saved sizes to at least 960 by 640 logical points before it shows the window.
Display scaling converts the saved physical pixels to logical points.

## Launcher

On macOS, press **Control Option Space** to open the launcher.
On Windows and Linux, press **Control Alt Space**.
The launcher opens above other apps in the upper third of the screen, including macOS fullscreen Spaces.
It follows the **Appearance** choice: **Light**, **Dark**, or **System**.

Type the first message of a new thread.
Press **Enter** to send it and bring the main window forward.
Press **Escape** to close the launcher without sending.

If the launcher cannot start, its alert names the cause and asks you to restart the app.
The desktop writes the cause to stderr, which the Linux evidence envelope captures in `driver-app.log`.
The installed specs select the `main` window by its Tauri label before they read the shell.
Failure captures use that window too.

## Provider access

Open the sidebar's **Local mode** section from the thread surface. Select **Anthropic**, **Google**, or **OpenAI**. Enter the key in **Provider API key**, then select **Save key**. The desktop writes the key to Pi's `~/.pi/agent/auth.json` store with Pi's file lock.

Pi can also use provider credentials that the Pi CLI saved in the same store. Local mode removes inherited provider credential and endpoint variables from the Pi process environment.

## Local records

An empty journal shows the composer without a history alert.
The empty thread shows model lookup errors, missing connections, or unavailable accounts before it offers chat guidance.
With a model selected and an account available, it reads **Ask a question or request a file.**
The model chip names the selection. It does not prove that the provider will answer.
Open **Artifacts** for file creation guidance. **Record** opens the separate company record panel.
If a history read fails, the alert includes the reader's cause. **Restore history** repeats the failed action.
A successful history read clears the alert.

The runtime saves prompt history in the OS keyring.
If the keyring refuses a write, Send still starts the run with the prompt in runtime memory.
The shell shows a mono line with the storage location.
Open the line to read the keyring error and its platform code.
The runtime log and the journal record the same notice without the prompt text.

After the shell restores the thread, it shows the reply and notice without prompt history.
The runtime does not add a plaintext prompt store.
The model provider and its session files still follow their own storage rules.

Local mode writes run start, model output, tool activity, and completion records to the local run journal. A completed local run records its elapsed time and no cloud receipt. These records use the same event shapes as cloud-backed runs.

After Pi becomes ready, a prompt has 30 seconds to produce its first reply event. An acknowledgment alone does not satisfy this bound. If the bound expires, the shell shows the cause and records a failed reply in the local run journal.

A failed reply shows its recorded cause beside **Try again**, including after the desktop restores the thread.
The composer keeps its rest text instead of repeating the cause.

The runtime log records run lifecycle outcomes and Pi's stderr tail for failed replies. The Linux evidence envelope keeps this log in `pi-local-mode-stderr.log` beside `muniment-runtime.log`.

Before Pi starts, a rejected launch config logs `run_id`, `step`, and the error in one runtime log line.
The step distinguishes the session root check, settings write, package install, cloud extension write, and acquisition journal update.
A failed package install includes the exit status or timeout and a bounded, redacted `stderr_tail` from Bun.
The Windows evidence envelope relays `runtime.log` in `runner-transcript.log`.

On Windows, the desktop starts the runtime task with `RunEx` and the calling process's session id.
The release runtime does not open a console window.
If the task stays `Queued` for five seconds, the runtime notice names the state and session id.
The desktop writes the same cause to stderr.

On macOS, the bundled runtime resolves the desktop executable at `Contents/MacOS/muniment-desktop` from `Contents/Library/LaunchServices/muniment-runtime`.
Attach setup failures record `step=desktop_executable_check`, `step=socket_bind`, or `step=state_open` in `~/Library/Logs/Muniment/runtime.log` and the unified log.

The desktop attempts registration once when macOS reports the runtime service as `NotFound` or `NotRegistered`.
Registration failures record the native error domain, code, and description in `runtime.log`.
The macOS evidence envelope includes `runtime.log` and `runtime-launchctl.log`.
The runner captures these logs before cleanup stops the runtime.

A rejected run request carries the runtime sentence in `error.details.reason` on the attach wire.
The run-start probe and `runtime-service.log` print that sentence as `reason`.
On macOS, `driver-app.log` records `desktop approval presenter connected=true` when the presenter connects.
The runtime logs a presenter refusal once per holder, with `holder_claimed` and `holder_pending_requests`.
The runtime releases the presenter claim when its connection closes.

Read the macOS runtime log:

```sh
cat ~/Library/Logs/Muniment/runtime.log
```

Inspect the runtime agent:

```sh
launchctl print "gui/$(id -u)/ai.muniment.runtime"
```

## Cloud sign-in

Select **Sign in for cloud features** to leave local mode. Sign-in remains available for features that need the Muniment control plane.

The installed runtime uses HTTPS for cloud sign-in at `api.muniment.ai`. Its HTTP client uses rustls, the operating system certificate store, and proxy settings from the environment.

Run the local TLS handshake test from the repository root:

```sh
cargo test --manifest-path src-tauri/Cargo.toml -p muniment-runtime --locked --test native_https
```

The first message requests a one to three word thread name from the model.
A failed name request keeps a short prompt fallback. A manual rename wins.
Read, search, command and edit actions appear in expandable groups.
Open a group to see its actions. Only actions with details expand.
Select a read or edited file to open its current contents in the file panel.
The file panel resizes and maximizes like the record panel. Syntax colors follow the theme.
The changed-files chip above the composer opens a file list on hover or click.
Line totals describe recorded applied changes. Missing or partial counts stay unnumbered.
Active actions show a text sheen unless reduced motion is enabled.
Details are bounded and credential-filtered before the local journal saves them.
An interrupted reply keeps its text. **Try again** sends the same message again.
During browser sign-in, **Cancel sign-in** or **Escape** restores local mode.

Subscription accounts use the sign-in email when the provider supplies it.
Account balancing has a visible switch beside its explanation.
Accounts use two equal columns, including providers with one account, and one column in narrow panels.
Allowance meters use full-strength green, yellow and red with the remaining percentage and reset time.
Account cards show routing warnings. Usage and settings holds timestamps, usage totals and account controls.
Hover over an account name and select the pencil to edit it inline.
Enter saves the name. Escape cancels the edit. A failed save keeps the edit open.
Grok subscriptions show their reported credits window, remaining percentage and reset time.

Type `@` in the composer to find a file in Home. Select a result to attach it.
URLs and file references use the theme reference color and show icon links.
Preferences includes automatic context compaction and token limits.
Compaction keeps the conversation history and records its outcome in the action feed.
The web search default returns sources in the conversation without opening a browser.
An explicit web search workflow in the user configuration remains in effect.

Actions appear where they occur between assistant text blocks, including restored history.
Web search details list the queries without provider output or search parameters.
A lost reply acknowledgment keeps the reply in recovery until the journal read completes.

Hover over a user message to see its local send time and copy control.
The reply places execution time and an icon-only copy control at the right.

A single action opens its details directly. Multiple actions share a group dropdown.

Routed replies estimate cost per turn using the selected model’s catalog input and output rates. Cached input uses the standard input rate. Subscription allowances and provider discounts do not change this estimate. An unknown model price leaves cost unavailable. Saved receipts retain their recorded values.

Reply receipts list classifier tokens and estimated cost separately from the answering model. Each classifier total includes all calls in the run, including calls that select a fallback. Missing usage stays unavailable.

Reply receipts show cost, tokens, turns and tools before routing details.
Routing details opens one numbered section per turn, including fallback causes.
Preferences shows the selected conversation and record fonts in compact pickers.
Open a picker to search installed fonts or restore the default.
New agents use one Create agent action. Creating an agent does not start a run.
Chat, Run now and schedule controls become available after creation.
