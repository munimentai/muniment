# Local mode

The desktop can open the thread surface without a Muniment account. Select **Use local mode** from the signed-out screen.

Local mode does not contact the Muniment control plane for authentication or model access. Pi runs on the device and reads its provider credentials from Pi's credential store. The desktop does not pass a Muniment virtual key to Pi in local mode.

## Window size

The desktop opens at 1100 by 720 logical points unless a saved size exists.
It clamps saved sizes to at least 960 by 640 logical points before it shows the window.
Display scaling converts the saved physical pixels to logical points.

## Provider access

Open the sidebar's **Local mode** section from the thread surface. Select **Anthropic**, **Google**, or **OpenAI**. Enter the key in **Provider API key**, then select **Save key**. The desktop writes the key to Pi's `~/.pi/agent/auth.json` store with Pi's file lock.

Pi can also use provider credentials that the Pi CLI saved in the same store. Local mode removes inherited provider credential and endpoint variables from the Pi process environment.

## Local records

An empty journal shows the composer without a history alert.
If a history read fails, the alert includes the reader's cause. **Restore history** repeats the failed action.
A successful history read clears the alert.

Local mode writes run input, model output, tool activity, and completion records to the local run journal. A completed local run records its elapsed time and no cloud receipt. These records use the same event shapes as cloud-backed runs.

After Pi becomes ready, a prompt has 30 seconds to produce its first reply event. An acknowledgment alone does not satisfy this bound. If the bound expires, the shell shows the cause and records a failed reply in the local run journal.

A failed reply shows its recorded cause beside **Try again**, including after the desktop restores the thread.
The composer keeps its rest text instead of repeating the cause.

The runtime log records run lifecycle outcomes and Pi's stderr tail for failed replies. The Linux evidence envelope keeps this log in `pi-local-mode-stderr.log` beside `muniment-runtime.log`.

On macOS, the bundled runtime resolves the desktop executable at `Contents/MacOS/muniment-desktop` from `Contents/Library/LaunchServices/muniment-runtime`.
Attach setup failures record `step=desktop_executable_check`, `step=socket_bind`, or `step=state_open` in `~/Library/Logs/Muniment/runtime.log` and the unified log.

The desktop attempts registration once when macOS reports the runtime service as `NotFound` or `NotRegistered`.
Registration failures record the native error domain, code, and description in `runtime.log`.
The macOS evidence envelope includes `runtime.log` and `runtime-launchctl.log`.
The runner captures these logs before cleanup stops the runtime.

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
