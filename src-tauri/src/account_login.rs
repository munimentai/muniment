//! Account sign-in for a Pi provider: Pi's own OAuth flow runs inside an RPC
//! process the desktop owns, through the `muniment-login` extension command,
//! and every step reaches the shell as a `local-mode-login` event.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

const LOGIN_EXTENSION: &str = include_str!("muniment_login.mjs");
const LOGIN_EXTENSION_FILE: &str = "muniment-login.mjs";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(15 * 60);
pub(crate) const LOGIN_EVENT: &str = "local-mode-login";
const ACCOUNT_PROVIDERS: [&str; 5] = [
    "openai-codex",
    "xai",
    "openrouter",
    "github-copilot",
    "anthropic",
];
const START_ERROR: &str = "The sign-in could not start. Try again.";
/// The provider whose OAuth redirect lands on a fixed local port, and that port.
/// The desktop holds the port first, so the browser lands on its own page and
/// the code reaches Pi through its paste prompt.
const CALLBACK_PROVIDER: &str = "openai-codex";
const CALLBACK_PORT: u16 = 1455;
const CALLBACK_PATH: &str = "/auth/callback";
const MARK_SVG: &str = include_str!("../icons/muniment-milled-ring.svg");

struct ActiveLogin {
    child: Child,
    stdin: ChildStdin,
    generation: u64,
    callback: Option<Arc<Callback>>,
    /// The callback servers, one per loopback address the port is held on.
    servers: Vec<JoinHandle<()>>,
}

/// The redirect the desktop's callback page received and the paste prompt Pi
/// opened for it. Whichever arrives second sends the redirect to Pi.
#[derive(Default)]
struct Callback {
    stop: AtomicBool,
    state: Mutex<CallbackState>,
}

#[derive(Default)]
struct CallbackState {
    redirect: Option<String>,
    prompt_id: Option<String>,
    answered: bool,
}

impl Callback {
    /// Records one side and returns the prompt answer once both sides are known.
    fn take_answer(
        &self,
        redirect: Option<String>,
        prompt_id: Option<String>,
    ) -> Option<(String, String)> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if redirect.is_some() {
            state.redirect = redirect;
        }
        if prompt_id.is_some() {
            state.prompt_id = prompt_id;
        }
        if state.answered {
            return None;
        }
        let answer = match (&state.prompt_id, &state.redirect) {
            (Some(id), Some(url)) => Some((id.clone(), url.clone())),
            _ => None,
        };
        state.answered = answer.is_some();
        answer
    }
}

fn provider_display_name(provider: &str) -> &str {
    match provider {
        "openai-codex" => "OpenAI",
        "xai" => "xAI",
        "openrouter" => "OpenRouter",
        "github-copilot" => "GitHub Copilot",
        "anthropic" => "Anthropic",
        other => other,
    }
}

/// The page the browser lands on after the provider redirects: muniment's mark
/// and voice, on the app's paper.
pub(crate) fn callback_page(connected: Option<&str>) -> String {
    let (heading, body) = match connected {
        Some(provider) => (
            "Signed in".to_owned(),
            format!(
                "{provider} is connected. Muniment is ready in the app, and this window can close."
            ),
        ),
        None => (
            "Sign-in did not finish".to_owned(),
            "Muniment did not get a code from this page. Return to the app and try again."
                .to_owned(),
        ),
    };
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Muniment</title>
<style>
:root {{ color-scheme: light dark; --paper: #F6F7F6; --ink: #1A1D1C; --muted: #5C6461; }}
@media (prefers-color-scheme: dark) {{ :root {{ --paper: #000000; --ink: #ECEFED; --muted: #9AA29E; }} }}
* {{ box-sizing: border-box; }}
body {{ margin: 0; min-height: 100vh; display: grid; place-items: center; padding: 24px; background: var(--paper); color: var(--ink); font-family: system-ui, -apple-system, "Segoe UI", sans-serif; -webkit-font-smoothing: antialiased; text-align: center; }}
main {{ display: grid; justify-items: center; gap: 12px; max-width: 480px; }}
main svg {{ width: 56px; height: 56px; margin-bottom: 12px; }}
h1 {{ margin: 0; font-size: 28px; font-weight: 600; line-height: 1.3; letter-spacing: -0.01em; }}
p {{ margin: 0; color: var(--muted); font-size: 15px; line-height: 1.55; }}
</style>
</head>
<body>
<main>
{MARK_SVG}
<h1>{heading}</h1>
<p>{body}</p>
</main>
</body>
</html>
"#
    )
}

/// The redirect URL for a request line that hits the callback path with a code, else nothing.
fn callback_redirect(request_line: &str) -> Option<String> {
    let mut parts = request_line.split_whitespace();
    if parts.next()? != "GET" {
        return None;
    }
    let target = parts.next()?;
    let (path, query) = target.split_once('?')?;
    if path != CALLBACK_PATH
        || !query
            .split('&')
            .any(|pair| pair.starts_with("code=") && pair.len() > "code=".len())
    {
        return None;
    }
    Some(format!("http://localhost:{CALLBACK_PORT}{target}"))
}

pub(crate) fn write_response(stream: &mut TcpStream, status: &str, body: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status}
Content-Type: text/html; charset=utf-8
Content-Length: {}
Connection: close
Cache-Control: no-store

{body}",
        body.len()
    );
    let _ = stream.flush();
}

/// Answers Pi's paste prompt with the redirect once both are known.
fn deliver_callback(app: &AppHandle, generation: u64, answer: Option<(String, String)>) {
    let Some((id, url)) = answer else {
        return;
    };
    let state = app.state::<AccountLoginState>();
    let mut active = state.active.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(login) = active
        .as_mut()
        .filter(|login| login.generation == generation)
    {
        let response = json!({ "type": "extension_ui_response", "id": id, "value": url });
        let _ = writeln!(login.stdin, "{response}");
    }
}

fn serve_callback(
    listener: TcpListener,
    callback: Arc<Callback>,
    app: AppHandle,
    provider: String,
    generation: u64,
) {
    let _ = listener.set_nonblocking(true);
    while !callback.stop.load(Ordering::SeqCst) {
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(_) => break,
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let mut request = Vec::new();
        let mut buffer = [0u8; 1024];
        while request.len() < 16 * 1024 {
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    request.extend_from_slice(&buffer[..read]);
                    if request.windows(4).any(|window| {
                        window
                            == b"

"
                    }) {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let text = String::from_utf8_lossy(&request);
        let redirect = text.lines().next().and_then(callback_redirect);
        match redirect {
            Some(url) => {
                write_response(
                    &mut stream,
                    "200 OK",
                    &callback_page(Some(provider_display_name(&provider))),
                );
                deliver_callback(&app, generation, callback.take_answer(Some(url), None));
            }
            None => write_response(&mut stream, "404 Not Found", &callback_page(None)),
        }
    }
}

/// Whether Pi's input prompt is the paste fallback for the browser redirect.
fn is_paste_prompt(title: &str) -> bool {
    let title = title.to_ascii_lowercase();
    title.contains("authorization code") || title.contains("redirect url")
}

pub(crate) fn present_main_window(app: &AppHandle) {
    if let Some(main) = app.get_window("main") {
        let _ = main.unminimize();
        let _ = main.show();
        let _ = main.set_focus();
    }
}

#[derive(Default)]
pub(crate) struct AccountLoginState {
    active: Mutex<Option<ActiveLogin>>,
    generation: Mutex<u64>,
}

impl AccountLoginState {
    fn stop(&self) {
        let login = self.active.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(login) = login {
            close(login);
        }
    }
}

/// Ends one sign-in: the callback servers stop, Pi exits, and the listeners
/// close with their threads, so the next sign-in binds the port at once. It
/// runs outside the state lock, because a server mid-redirect takes that lock.
fn close(mut login: ActiveLogin) {
    if let Some(callback) = &login.callback {
        callback.stop.store(true, Ordering::SeqCst);
    }
    let _ = login.child.kill();
    let _ = login.child.wait();
    for server in login.servers.drain(..) {
        let _ = server.join();
    }
}

/// Holds the callback port on both loopback addresses, so the browser's
/// `localhost` redirect lands here whichever one it resolves first. A wildcard
/// listener of another program yields to the specific bind, a program on the
/// loopback itself is named, and a host without an IPv6 loopback keeps the
/// IPv4 one alone.
fn bind_callback(port: u16) -> Result<Vec<TcpListener>, String> {
    let mut listeners = Vec::new();
    for address in [
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(Ipv6Addr::LOCALHOST),
    ] {
        match TcpListener::bind((address, port)) {
            Ok(listener) => listeners.push(listener),
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                return Err(port_error(port));
            }
            Err(_) => {}
        }
    }
    if listeners.is_empty() {
        return Err(port_error(port));
    }
    Ok(listeners)
}

fn port_error(port: u16) -> String {
    format!("Port {port} is in use by another program. Stop it and try again, or use an API key.")
}

fn emit(app: &AppHandle, payload: Value) {
    let _ = app.emit(LOGIN_EVENT, payload);
}

/// Turns one RPC frame into the shell's event, and says whether the login command answered.
fn login_event(frame: &Value) -> (Option<Value>, bool) {
    match frame.get("type").and_then(Value::as_str) {
        Some("extension_ui_request") => {
            let id = frame.get("id").and_then(Value::as_str).unwrap_or_default();
            let title = frame
                .get("title")
                .or_else(|| frame.get("message"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            match frame.get("method").and_then(Value::as_str) {
                Some("notify") => {
                    let payload = serde_json::from_str::<Value>(title).ok();
                    (
                        payload.filter(|payload| {
                            payload.get("muniment").and_then(Value::as_str) == Some("login")
                        }),
                        false,
                    )
                }
                Some("select") => (
                    Some(json!({
                        "stage": "prompt",
                        "id": id,
                        "kind": "select",
                        "title": title,
                        "options": frame.get("options").cloned().unwrap_or_else(|| json!([])),
                    })),
                    false,
                ),
                Some("input") => (
                    Some(json!({
                        "stage": "prompt",
                        "id": id,
                        "kind": "input",
                        "title": title,
                        "placeholder": frame.get("placeholder").cloned().unwrap_or(Value::Null),
                    })),
                    false,
                ),
                Some("confirm") => (
                    Some(json!({ "stage": "prompt", "id": id, "kind": "confirm", "title": title })),
                    false,
                ),
                _ => (None, false),
            }
        }
        Some("response") => (
            None,
            frame.get("command").and_then(Value::as_str) == Some("prompt"),
        ),
        _ => (None, false),
    }
}

fn write_extension(agent: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(agent).map_err(|_| START_ERROR.to_string())?;
    let path = agent.join(LOGIN_EXTENSION_FILE);
    if std::fs::read(&path).ok().as_deref() != Some(LOGIN_EXTENSION.as_bytes()) {
        let temporary = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
        std::fs::write(&temporary, LOGIN_EXTENSION).map_err(|_| START_ERROR.to_string())?;
        muniment_core::atomic_file::replace(&temporary, &path).map_err(|_| {
            let _ = std::fs::remove_file(&temporary);
            START_ERROR.to_string()
        })?;
    }
    Ok(path)
}

/// Where a sign-in lands. Pi's own slot holds one account per provider; the
/// router's pool holds many, so a pool sign-in runs Pi in a directory of its
/// own and the credential is lifted from there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    /// Pi's `auth.json`, as one provider's single account.
    Pi,
    /// The router's pool, as one more account of the provider's family.
    Pool,
}

pub(crate) fn start(app: &AppHandle, provider: &str) -> Result<(), String> {
    start_with(app, provider, target_for(provider))
}

/// Where the Connect list's sign-in lands: the pool for a family the router
/// pools, so the account gets its card with what it has served, and Pi's own
/// slot for a provider the router does not pool.
fn target_for(provider: &str) -> Target {
    if muniment_core::model_router::family::family_for_pi_provider(provider).is_some() {
        Target::Pool
    } else {
        Target::Pi
    }
}

/// A sign-in whose credential joins the router's pool.
pub(crate) fn start_into_pool(app: &AppHandle, provider: &str) -> Result<(), String> {
    if muniment_core::model_router::family::family_for_pi_provider(provider).is_none() {
        return Err("This provider has no subscription the router can pool.".into());
    }
    // Kimi, Antigravity and Devin have no Pi sign-in. The router runs its own.
    if muniment_core::model_router::family::native_sign_in(provider) {
        app.state::<AccountLoginState>().stop();
        return crate::pool_login::start(app, provider);
    }
    start_with(app, provider, Target::Pool)
}

fn start_with(app: &AppHandle, provider: &str, target: Target) -> Result<(), String> {
    if !ACCOUNT_PROVIDERS.contains(&provider) {
        return Err("This provider has no account sign-in.".into());
    }
    let state = app.state::<AccountLoginState>();
    state.stop();
    let home = crate::local_mode::harness_agent_directory(START_ERROR)?;
    // A pool sign-in runs Pi in a directory of its own, so the credential
    // never lands in Pi's one slot for the provider.
    let agent = match target {
        Target::Pi => home,
        Target::Pool => {
            let scratch = home.join(format!("pool-login-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir_all(&scratch).map_err(|_| START_ERROR.to_string())?;
            scratch
        }
    };
    let executable = crate::local_mode::pi_executable().ok_or_else(|| START_ERROR.to_string())?;
    let extension = write_extension(&agent)?;
    // The port is held before Pi starts, so Pi's own callback server yields and
    // its paste prompt carries the redirect the desktop's page received. A
    // port another program holds stops the sign-in here with its number, never
    // with a wait on a redirect that lands elsewhere.
    let listeners = if provider == CALLBACK_PROVIDER {
        bind_callback(CALLBACK_PORT)?
    } else {
        Vec::new()
    };
    let callback = (!listeners.is_empty()).then(|| Arc::new(Callback::default()));
    // The sign-in needs the login extension alone. Discovery stays off, so a
    // package that fails to load cannot stop the sign-in.
    let mut child = Command::new(executable)
        .args(["--mode", "rpc", "--no-extensions", "--extension"])
        .arg(&extension)
        .env("PI_CODING_AGENT_DIR", &agent)
        .env("PI_OFFLINE", "1")
        .env_remove("BUN_BE_BUN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| START_ERROR.to_string())?;
    let mut stdin = child.stdin.take().ok_or_else(|| START_ERROR.to_string())?;
    let stdout = child.stdout.take().ok_or_else(|| START_ERROR.to_string())?;
    let prompt = json!({ "id": "login", "type": "prompt", "message": format!("/muniment-login {provider} oauth") });
    writeln!(stdin, "{prompt}").map_err(|_| START_ERROR.to_string())?;
    let generation = {
        let mut counter = state.generation.lock().unwrap_or_else(|e| e.into_inner());
        *counter += 1;
        *counter
    };
    *state.active.lock().unwrap_or_else(|e| e.into_inner()) = Some(ActiveLogin {
        child,
        stdin,
        generation,
        callback: callback.clone(),
        servers: Vec::new(),
    });
    if let Some(callback) = callback.clone() {
        let servers: Vec<JoinHandle<()>> = listeners
            .into_iter()
            .map(|listener| {
                let callback = Arc::clone(&callback);
                let server_app = app.clone();
                let server_provider = provider.to_owned();
                std::thread::spawn(move || {
                    serve_callback(listener, callback, server_app, server_provider, generation)
                })
            })
            .collect();
        let mut active = state.active.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(login) = active
            .as_mut()
            .filter(|login| login.generation == generation)
        {
            login.servers = servers;
        }
    }
    let provider = provider.to_owned();
    let reader_app = app.clone();
    std::thread::spawn(move || {
        let mut answered = false;
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let Ok(frame) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let (event, finished) = login_event(&frame);
            if let Some(mut event) = event {
                let stage = event
                    .get("stage")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let is_paste = event.get("kind").and_then(Value::as_str) == Some("input")
                    && event
                        .get("title")
                        .and_then(Value::as_str)
                        .is_some_and(is_paste_prompt);
                if let (Some(callback), true) = (&callback, is_paste) {
                    // The desktop's page answers this prompt. The shell never sees it.
                    let id = event.get("id").and_then(Value::as_str).map(str::to_owned);
                    deliver_callback(&reader_app, generation, callback.take_answer(None, id));
                    continue;
                }
                if stage == "done" {
                    match target {
                        Target::Pi => {
                            let _ =
                                crate::local_mode::adopt_provider_default(&agent, &provider, true);
                        }
                        Target::Pool => {
                            match crate::model_router::import_pi_sign_in(
                                &reader_app,
                                &agent,
                                &provider,
                            ) {
                                Ok(account) => {
                                    if let Some(object) = event.as_object_mut() {
                                        object.insert("pool".into(), true.into());
                                        object.insert("account".into(), account.into());
                                    }
                                }
                                Err(message) => {
                                    event = json!({ "stage": "failed", "message": message, "pool": true });
                                }
                            }
                        }
                    }
                    present_main_window(&reader_app);
                }
                if let Some(object) = event.as_object_mut() {
                    object.insert("provider".into(), provider.clone().into());
                }
                emit(&reader_app, event);
            }
            if finished {
                answered = true;
                break;
            }
        }
        let state = reader_app.state::<AccountLoginState>();
        let finished = {
            let mut active = state.active.lock().unwrap_or_else(|e| e.into_inner());
            if active
                .as_ref()
                .is_some_and(|login| login.generation == generation)
            {
                active.take()
            } else {
                None
            }
        };
        if let Some(login) = finished {
            close(login);
            emit(
                &reader_app,
                json!({ "stage": "exit", "provider": provider, "answered": answered }),
            );
        }
        if target == Target::Pool {
            // The scratch directory held one credential for one sign-in, and
            // the pool holds it now.
            let _ = std::fs::remove_dir_all(&agent);
        }
    });
    let watchdog_app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(LOGIN_TIMEOUT);
        let state = watchdog_app.state::<AccountLoginState>();
        let expired = {
            let mut active = state.active.lock().unwrap_or_else(|e| e.into_inner());
            if active
                .as_ref()
                .is_some_and(|login| login.generation == generation)
            {
                active.take()
            } else {
                None
            }
        };
        if let Some(login) = expired {
            close(login);
        }
    });
    Ok(())
}

pub(crate) fn answer(
    app: &AppHandle,
    id: &str,
    value: Option<String>,
    confirmed: Option<bool>,
) -> Result<(), String> {
    let state = app.state::<AccountLoginState>();
    let mut active = state.active.lock().unwrap_or_else(|e| e.into_inner());
    let login = active
        .as_mut()
        .ok_or("No sign-in is waiting for an answer.")?;
    let mut response = json!({ "type": "extension_ui_response", "id": id });
    match (value, confirmed) {
        (Some(value), _) => response["value"] = value.into(),
        (None, Some(confirmed)) => response["confirmed"] = confirmed.into(),
        (None, None) => response["cancelled"] = true.into(),
    }
    writeln!(login.stdin, "{response}")
        .map_err(|_| "The sign-in stopped. Start it again.".to_string())
}

pub(crate) fn cancel(app: &AppHandle) {
    let state = app.state::<AccountLoginState>();
    state.stop();
    crate::pool_login::stop();
    emit(app, json!({ "stage": "cancelled" }));
}

#[tauri::command]
pub(crate) fn local_mode_account_login_start(
    app: AppHandle,
    provider: String,
) -> Result<(), String> {
    start(&app, &provider)
}

#[tauri::command]
pub(crate) fn local_mode_account_login_answer(
    app: AppHandle,
    id: String,
    value: Option<String>,
    confirmed: Option<bool>,
) -> Result<(), String> {
    answer(&app, &id, value, confirmed)
}

#[tauri::command]
pub(crate) fn local_mode_account_login_cancel(app: AppHandle) {
    cancel(&app)
}

/// Opens a provider's sign-in page in the default browser.
#[tauri::command]
pub(crate) fn local_mode_open_url(url: String) -> Result<(), String> {
    let parsed = url::Url::parse(&url).map_err(|_| "The sign-in link is invalid.".to_string())?;
    if parsed.scheme() != "https" || parsed.host().is_none() {
        return Err("The sign-in link is invalid.".into());
    }
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(parsed.as_str());
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", parsed.as_str()]);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(parsed.as_str());
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|_| "The browser could not open. Copy the link instead.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notify_frames_carry_the_login_stage_and_prompts_carry_their_id() {
        let notify = json!({
            "type": "extension_ui_request", "id": "n1", "method": "notify",
            "message": "{\"muniment\":\"login\",\"stage\":\"event\",\"event\":{\"type\":\"auth_url\",\"url\":\"https://x\"}}",
            "notifyType": "info"
        });
        let (event, finished) = login_event(&notify);
        assert_eq!(event.unwrap()["event"]["url"], "https://x");
        assert!(!finished);

        let stray = json!({ "type": "extension_ui_request", "id": "n2", "method": "notify", "message": "3 entries" });
        assert_eq!(login_event(&stray), (None, false));

        let select = json!({ "type": "extension_ui_request", "id": "s1", "method": "select", "title": "Method", "options": ["Browser", "Device"] });
        let (event, _) = login_event(&select);
        let event = event.unwrap();
        assert_eq!(event["stage"], "prompt");
        assert_eq!(event["kind"], "select");
        assert_eq!(event["id"], "s1");
        assert_eq!(event["options"], json!(["Browser", "Device"]));

        let input = json!({ "type": "extension_ui_request", "id": "i1", "method": "input", "title": "Paste the code", "placeholder": "http://localhost:1455/auth/callback" });
        let (event, _) = login_event(&input);
        assert_eq!(event.unwrap()["kind"], "input");

        let done =
            json!({ "id": "login", "type": "response", "command": "prompt", "success": true });
        assert_eq!(login_event(&done), (None, true));
        let other = json!({ "type": "response", "command": "get_state", "success": true });
        assert_eq!(login_event(&other), (None, false));
    }

    #[test]
    fn the_extension_registers_the_login_command_and_reports_each_stage() {
        assert!(LOGIN_EXTENSION.contains(".registerCommand(\"muniment-login\""));
        for stage in ["\"start\"", "\"event\"", "\"done\"", "\"failed\""] {
            assert!(LOGIN_EXTENSION.contains(stage), "{stage}");
        }
        assert!(LOGIN_EXTENSION.contains("ctx.ui.select("));
        assert!(LOGIN_EXTENSION.contains("ctx.ui.input("));
    }

    #[test]
    fn the_callback_page_is_muniment_and_the_redirect_parses() {
        let page = callback_page(Some("OpenAI"));
        assert!(page.contains("<title>Muniment</title>"));
        assert!(page.contains("aria-label=\"muniment\""));
        assert!(page.contains("OpenAI is connected."));
        assert!(!page.contains("Authentication successful"));
        let missing = callback_page(None);
        assert!(missing.contains("Sign-in did not finish"));
        assert_eq!(
            callback_redirect("GET /auth/callback?code=ac_1&state=s HTTP/1.1"),
            Some("http://localhost:1455/auth/callback?code=ac_1&state=s".to_owned())
        );
        assert_eq!(
            callback_redirect("GET /auth/callback?state=s HTTP/1.1"),
            None
        );
        assert_eq!(callback_redirect("GET /favicon.ico HTTP/1.1"), None);
        assert_eq!(
            callback_redirect("POST /auth/callback?code=x HTTP/1.1"),
            None
        );
    }

    #[test]
    fn the_paste_prompt_answers_once_both_sides_are_known() {
        let callback = Callback::default();
        assert_eq!(callback.take_answer(None, Some("i1".into())), None);
        assert_eq!(
            callback.take_answer(
                Some("http://localhost:1455/auth/callback?code=c".into()),
                None
            ),
            Some((
                "i1".into(),
                "http://localhost:1455/auth/callback?code=c".into()
            ))
        );
        assert_eq!(callback.take_answer(None, Some("i1".into())), None);
        assert!(is_paste_prompt(
            "Complete login in your browser, or paste the authorization code / redirect URL here:"
        ));
        assert!(!is_paste_prompt("Paste your API key"));
    }

    #[test]
    fn the_callback_port_is_held_on_both_loopbacks_and_a_holder_is_named() {
        let probe = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let listeners = bind_callback(port).unwrap();
        let addresses: Vec<IpAddr> = listeners
            .iter()
            .map(|listener| listener.local_addr().unwrap().ip())
            .collect();
        assert_eq!(addresses[0], IpAddr::V4(Ipv4Addr::LOCALHOST));
        if let Some(second) = addresses.get(1) {
            assert_eq!(*second, IpAddr::V6(Ipv6Addr::LOCALHOST));
        }
        assert!(listeners
            .iter()
            .all(|listener| listener.local_addr().unwrap().port() == port));
        drop(listeners);
        let holder = TcpListener::bind(("127.0.0.1", port)).unwrap();
        let error = bind_callback(port).unwrap_err();
        assert!(error.contains(&port.to_string()), "{error}");
        drop(holder);
    }

    #[test]
    fn a_pooled_family_signs_in_to_the_pool_and_the_rest_to_the_one_slot() {
        assert_eq!(target_for("xai"), Target::Pool);
        assert_eq!(target_for("openai-codex"), Target::Pool);
        assert_eq!(target_for("openrouter"), Target::Pi);
        assert_eq!(target_for("github-copilot"), Target::Pi);
    }

    #[test]
    fn only_account_providers_start_a_sign_in() {
        assert!(ACCOUNT_PROVIDERS.contains(&"openai-codex"));
        assert!(ACCOUNT_PROVIDERS.contains(&"xai"));
        assert!(!ACCOUNT_PROVIDERS.contains(&"google"));
    }
}
