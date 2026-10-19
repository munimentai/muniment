//! Anthropic accounts through Claude Code. Each account has its own Claude
//! Code config folder, and Claude Code's own `claude auth login` signs in to
//! it, exchanges the code and keeps the token in its own store. The runtime
//! starts that login, hands the client the sign-in URL Claude Code gives,
//! and passes on what the browser brings back: a forwarded redirect to Claude
//! Code's own listener, or a pasted code to its prompt. It asks Claude Code
//! who the account is and what its plan has left, and never reads a token.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use muniment_core::chrono;
use muniment_core::models::claude_accounts::{self, Account, Usage, Window};
use muniment_core::serde_json::{self, json, Value};

use super::callback;
use super::ModelService;

/// The provider id the sign-in steps carry, the catalog's Anthropic account.
pub(super) const PROVIDER: &str = "anthropic";
/// The path of Claude Code's own loopback callback.
const CALLBACK_PATH: &str = "/callback";
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// How long Claude Code may take to name the sign-in URL.
const URL_TIMEOUT: Duration = Duration::from_secs(20);
/// How long after the printed URL Claude Code may take to open its browser.
const BROWSER_WAIT: Duration = Duration::from_secs(3);
const STATUS_TIMEOUT: Duration = Duration::from_secs(15);
const USAGE_TIMEOUT: Duration = Duration::from_secs(45);
const START_ERROR: &str = "The sign-in could not start. Try again.";
const MISSING: &str =
    "Claude Code is not on this machine yet. Start a chat once so Muniment installs it, then sign in.";
const PASTE_TITLE: &str = "Paste the code Anthropic shows";
/// Variables that would point Claude Code at another credential or backend.
const FOREIGN_VARIABLES: [&str; 10] = [
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_REFRESH_TOKEN",
    "CLAUDE_CODE_OAUTH_SCOPES",
    "CLAUDE_SECURESTORAGE_CONFIG_DIR",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "BROWSER",
];

/// The Claude Code binary to run: the bundled build at the pinned version,
/// then any bundled build, then one the user installed.
pub(super) fn executable(agent: &Path) -> Option<PathBuf> {
    let bundled = claude_accounts::bundled_executables(agent);
    bundled
        .iter()
        .find(|path| version(path).is_some_and(|line| claude_accounts::pinned_version(&line)))
        .or(bundled.first())
        .cloned()
        .or_else(user_installed)
}

/// A Claude Code the user installed, on the PATH or in its own folders.
pub(super) fn user_installed() -> Option<PathBuf> {
    let home = muniment_core::state_root::home_directory_value().map(PathBuf::from);
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(paths) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&paths).map(|dir| dir.join("claude")));
    }
    if let Some(home) = &home {
        candidates.push(home.join(".local").join("bin").join("claude"));
        candidates.push(home.join(".claude").join("local").join("claude"));
    }
    candidates.push(PathBuf::from("/opt/homebrew/bin/claude"));
    candidates.push(PathBuf::from("/usr/local/bin/claude"));
    candidates.into_iter().find(|path| path.is_file())
}

fn version(executable: &Path) -> Option<String> {
    let mut command = Command::new(executable);
    command.arg("--version").stdin(Stdio::null());
    let output = output_within(command, STATUS_TIMEOUT)?;
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// A Claude Code command for one account's folder, or for Claude Code's
/// default folder with none, with no variable that names another credential.
pub(super) fn command(executable: &Path, folder: Option<&Path>) -> Command {
    let mut command = Command::new(executable);
    for name in FOREIGN_VARIABLES {
        command.env_remove(name);
    }
    command
        .env("DISABLE_AUTOUPDATER", "1")
        .env("DISABLE_TELEMETRY", "1");
    if let Some(folder) = folder {
        command.env("CLAUDE_CONFIG_DIR", folder).current_dir(folder);
    }
    command
}

/// Runs `command` and answers its output, or none when it fails to start or
/// outlasts `timeout`, in which case it is killed.
pub(super) fn output_within(
    mut command: Command,
    timeout: Duration,
) -> Option<std::process::Output> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let read = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            bytes
        })
    };
    let stdout = read(
        child
            .stdout
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn Read + Send>),
    );
    let stderr = read(
        child
            .stderr
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn Read + Send>),
    );
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    Some(std::process::Output {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

/// What `claude auth status` reports for one folder.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Status {
    pub(super) logged_in: bool,
    email: Option<String>,
    org_id: Option<String>,
    org_name: Option<String>,
    plan: Option<String>,
}

pub(super) fn parse_status(stdout: &[u8]) -> Option<Status> {
    let value: Value = serde_json::from_slice(stdout).ok()?;
    let text = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    Some(Status {
        logged_in: value.get("loggedIn").and_then(Value::as_bool) == Some(true),
        email: text("email"),
        org_id: text("orgId"),
        org_name: text("orgName"),
        plan: text("subscriptionType"),
    })
}

/// Asks Claude Code who is signed in to `folder`, or to its default folder.
pub(super) fn status(executable: &Path, folder: Option<&Path>) -> Option<Status> {
    let mut command = command(executable, folder);
    command
        .args(["auth", "status", "--json"])
        .stdin(Stdio::null());
    parse_status(&output_within(command, STATUS_TIMEOUT)?.stdout)
}

/// The plan windows in a `get_usage` answer. `None` when the answer is not
/// one.
pub(super) fn parse_usage(response: &Value, now_ms: i64) -> Option<(Usage, Option<String>)> {
    let readable = response.get("rate_limits_available")?.as_bool()?;
    let plan = response
        .get("subscription_type")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let limits = response
        .get("rate_limits")
        .filter(|value| value.is_object());
    let mut windows = Vec::new();
    let mut push = |kind: &str, scope: &str, window: &Value| {
        let Some(used) = window.get("utilization").and_then(Value::as_f64) else {
            return;
        };
        let used = used.clamp(0.0, 100.0);
        windows.push(Window {
            kind: kind.into(),
            scope: scope.into(),
            remaining_percent: 100.0 - used,
            limit_reached: used >= 100.0,
            resets_at_ms: window
                .get("resets_at")
                .and_then(Value::as_str)
                .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
                .map(|at| at.timestamp_millis()),
        });
    };
    if let Some(limits) = limits {
        if let Some(window) = limits.get("five_hour").filter(|v| v.is_object()) {
            push("five_hour", "", window);
        }
        if let Some(window) = limits.get("seven_day").filter(|v| v.is_object()) {
            push("weekly", "", window);
        }
        match limits.get("model_scoped").and_then(Value::as_array) {
            Some(scoped) => {
                for window in scoped {
                    let name = window
                        .get("display_name")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    push("weekly", name, window);
                }
            }
            None => {
                for (key, name) in [("seven_day_opus", "Opus"), ("seven_day_sonnet", "Sonnet")] {
                    if let Some(window) = limits.get(key).filter(|v| v.is_object()) {
                        push("weekly", name, window);
                    }
                }
            }
        }
    }
    Some((
        Usage {
            observed_at_ms: now_ms,
            readable,
            windows,
        },
        plan,
    ))
}

/// Asks Claude Code for the account's plan limits through its control
/// protocol, the structured `/usage` answer, without sending a prompt.
pub(super) fn usage(executable: &Path, folder: &Path) -> Option<(Usage, Option<String>)> {
    let mut command = command(executable, Some(folder));
    command
        .args([
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().ok()?;
    let mut stdin = child.stdin.take()?;
    let stdout = child.stdout.take()?;
    let requests = [
        json!({"type": "control_request", "request_id": "initialize", "request": {"subtype": "initialize"}}),
        json!({"type": "control_request", "request_id": "usage", "request": {"subtype": "get_usage", "skip_behaviors": true}}),
    ];
    for request in requests {
        if writeln!(stdin, "{request}").is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            let Ok(frame) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if frame
                .pointer("/response/request_id")
                .and_then(Value::as_str)
                == Some("usage")
            {
                let _ = sender.send(frame);
                return;
            }
        }
    });
    let frame = receiver.recv_timeout(USAGE_TIMEOUT).ok();
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    let frame = frame?;
    if frame.pointer("/response/subtype").and_then(Value::as_str) != Some("success") {
        return None;
    }
    parse_usage(
        frame.pointer("/response/response")?,
        chrono::Utc::now().timestamp_millis(),
    )
}

/// Reads one account's plan limits and keeps them. Answers whether Claude
/// Code answered.
pub(super) fn refresh(agent: &Path, id: &str) -> bool {
    let Some(executable) = executable(agent) else {
        return false;
    };
    let folder = claude_accounts::folder(agent, id);
    let Some((reading, plan)) = usage(&executable, &folder) else {
        return false;
    };
    claude_accounts::update(agent, |store| {
        if let Some(account) = store.accounts.iter_mut().find(|account| account.id == id) {
            account.usage = Some(reading);
            if plan.is_some() {
                account.plan = plan;
            }
        }
    })
    .is_ok()
}

/// Reads every account's plan limits at once, or the one `id` names.
pub(super) fn refresh_all(agent: &Path, id: Option<&str>) {
    let ids: Vec<String> = claude_accounts::load(agent)
        .accounts
        .into_iter()
        .map(|account| account.id)
        .filter(|held| id.is_none_or(|id| id == held))
        .collect();
    let threads: Vec<_> = ids
        .into_iter()
        .map(|id| {
            let agent = agent.to_path_buf();
            std::thread::spawn(move || refresh(&agent, &id))
        })
        .collect();
    for thread in threads {
        let _ = thread.join();
    }
}

/// Whether `id` names a Claude account.
pub(super) fn is_account(agent: &Path, id: &str) -> bool {
    claude_accounts::load(agent).account(id).is_some()
}

/// Signs Claude Code out of one folder, so it drops its stored token, and
/// removes the folder.
fn forget_folder(agent: &Path, id: &str) {
    let folder = claude_accounts::folder(agent, id);
    if let Some(executable) = executable(agent) {
        let mut command = command(&executable, Some(&folder));
        command.args(["auth", "logout"]).stdin(Stdio::null());
        let _ = output_within(command, STATUS_TIMEOUT);
    }
    let _ = std::fs::remove_dir_all(&folder);
}

/// Disconnects one account: Claude Code signs out of its folder, and the
/// record drops it with its threads.
pub(super) fn remove(agent: &Path, id: &str) -> Result<(), String> {
    let removed = claude_accounts::update(agent, |store| store.remove(id))
        .map_err(|_| "Muniment could not save the account list. Try again.".to_string())?;
    if !removed {
        return Err("That account is no longer connected.".into());
    }
    forget_folder(agent, id);
    Ok(())
}

/// The one Claude Code login in flight.
#[derive(Default)]
pub(super) struct ClaudeSignIns(Mutex<Option<Active>>);

struct Active {
    account: String,
    stop: Arc<AtomicBool>,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    /// The loopback port Claude Code's own callback listens on.
    port: Option<u16>,
    prompt: String,
}

impl ClaudeSignIns {
    /// Stops the login in flight, when there is one.
    pub(super) fn stop(&self) {
        if let Some(active) = self.0.lock().unwrap_or_else(|e| e.into_inner()).take() {
            active.stop.store(true, Ordering::SeqCst);
        }
    }

    fn finish(&self, account: &str) {
        let mut active = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if active.as_ref().is_some_and(|held| held.account == account) {
            *active = None;
        }
    }

    /// Whether a login waits for the prompt `id`.
    pub(super) fn has_prompt(&self, id: &str) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|active| active.prompt == id)
    }

    /// Whether a login is in flight.
    pub(super) fn active(&self) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).is_some()
    }

    /// Types a pasted code into Claude Code's prompt.
    pub(super) fn answer(&self, value: &str) -> Result<(), String> {
        let code = value.trim();
        if code.is_empty() || code.chars().any(char::is_control) {
            return Err("Paste the code Anthropic shows.".into());
        }
        let stdin = self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|active| Arc::clone(&active.stdin))
            .ok_or("No sign-in is waiting for an answer.")?;
        let mut stdin = stdin.lock().unwrap_or_else(|e| e.into_inner());
        let pipe = stdin
            .as_mut()
            .ok_or("The sign-in stopped. Start it again.")?;
        writeln!(pipe, "{code}")
            .and_then(|()| pipe.flush())
            .map_err(|_| "The sign-in stopped. Start it again.".to_string())
    }

    /// Hands a redirect a client received on its own computer to Claude
    /// Code's own listener on this one.
    pub(super) fn deliver(&self, url: &str) -> Result<(), String> {
        let port = self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .and_then(|active| active.port)
            .ok_or("No sign-in is waiting for a browser redirect.")?;
        let redirect = callback::forwarded_redirect_on(url, port, CALLBACK_PATH)
            .ok_or("The redirect is not a sign-in callback with a code.")?;
        let target = redirect
            .strip_prefix(&format!("http://localhost:{port}"))
            .ok_or("The redirect is not a sign-in callback with a code.")?;
        hand_on(port, target)
    }
}

/// Sends the browser's redirect to Claude Code's listener and answers
/// whether it took it.
fn hand_on(port: u16, target: &str) -> Result<(), String> {
    let failed = || "Claude Code did not take the sign-in. Start it again.".to_string();
    let mut stream = TcpStream::connect(("127.0.0.1", port)).map_err(|_| failed())?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));
    let request =
        format!("GET {target} HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).map_err(|_| failed())?;
    let mut head = [0u8; 16];
    let read = stream.read(&mut head).map_err(|_| failed())?;
    let line = String::from_utf8_lossy(&head[..read]);
    match line.split_whitespace().nth(1) {
        Some(code) if code.starts_with('2') || code.starts_with('3') => Ok(()),
        _ => Err(failed()),
    }
}

fn emit(service: &ModelService, mut payload: Value) {
    if let Some(object) = payload.as_object_mut() {
        object.insert("provider".into(), PROVIDER.into());
    }
    service.emit(payload);
}

/// The sign-in URL Claude Code prints for a code to paste.
fn manual_url(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let url = line[start..].split_whitespace().next()?;
    url.contains("oauth").then(|| url.to_owned())
}

/// The loopback port of the redirect a sign-in URL names.
fn redirect_port(url: &str) -> Option<u16> {
    let parsed = url::Url::parse(url).ok()?;
    let (_, redirect) = parsed
        .query_pairs()
        .find(|(key, _)| key == "redirect_uri")?;
    let redirect = url::Url::parse(&redirect).ok()?;
    (redirect.scheme() == "http"
        && matches!(redirect.host_str(), Some("localhost" | "127.0.0.1"))
        && redirect.path() == CALLBACK_PATH)
        .then(|| redirect.port())
        .flatten()
}

/// The reason Claude Code gave for a failed login, when it gave one.
fn failure(lines: &[String]) -> String {
    lines
        .iter()
        .rev()
        .find_map(|line| {
            let index = line.find("failed:")?;
            let reason = line[index + "failed:".len()..].trim();
            (!reason.is_empty()).then(|| reason.chars().take(200).collect::<String>())
        })
        .map_or_else(
            || "Claude Code did not finish the sign-in. Try again.".to_owned(),
            |reason| format!("Claude Code did not finish the sign-in: {reason}"),
        )
}

/// Writes the program Claude Code runs to open a browser. It records the URL
/// in `url` beside it, so the runtime hands it to the client's browser.
#[cfg(unix)]
fn browser_helper(directory: &Path) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let helper = directory.join("open");
    let target = directory.join("url");
    let quoted = target.to_string_lossy().replace('\'', r"'\''");
    std::fs::write(
        &helper,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$1\" > '{quoted}.tmp' && mv '{quoted}.tmp' '{quoted}'\n"
        ),
    )?;
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700))?;
    Ok(helper)
}

/// Starts Claude Code's login for a new account folder. Answers the
/// callback a client on another computer holds, when Claude Code listens
/// for the browser's redirect.
pub(super) fn start(service: &Arc<ModelService>) -> Result<Value, String> {
    service.sign_ins.stop();
    service.pool_sign_ins.stop();
    service.claude_sign_ins.stop();
    let agent = service.agent.clone();
    let executable = executable(&agent).ok_or(MISSING)?;
    let id = uuid::Uuid::now_v7().to_string();
    let folder = claude_accounts::folder(&agent, &id);
    let scratch = agent
        .join(claude_accounts::FOLDERS)
        .join(format!(".login-{id}"));
    let prepared = (|| {
        std::fs::create_dir_all(&folder)?;
        std::fs::create_dir_all(&scratch)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700))?;
            std::fs::set_permissions(&scratch, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok::<_, std::io::Error>(())
    })();
    let cleanup = {
        let folder = folder.clone();
        let scratch = scratch.clone();
        move |keep_folder: bool| {
            let _ = std::fs::remove_dir_all(&scratch);
            if !keep_folder {
                let _ = std::fs::remove_dir_all(&folder);
            }
        }
    };
    if prepared.is_err() {
        cleanup(false);
        return Err(START_ERROR.into());
    }
    let mut login = command(&executable, Some(&folder));
    login.args(["auth", "login", "--claudeai"]);
    #[cfg(unix)]
    match browser_helper(&scratch) {
        Ok(helper) => {
            login.env("BROWSER", helper);
        }
        Err(_) => {
            cleanup(false);
            return Err(START_ERROR.into());
        }
    }
    let mut child = match login
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => {
            cleanup(false);
            return Err(START_ERROR.into());
        }
    };
    let lines = Arc::new(Mutex::new(Vec::<String>::new()));
    for pipe in [
        child
            .stdout
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let lines = Arc::clone(&lines);
        std::thread::spawn(move || {
            for line in BufReader::new(pipe).lines() {
                let Ok(line) = line else { break };
                lines.lock().unwrap_or_else(|e| e.into_inner()).push(line);
            }
        });
    }
    // Claude Code hands its browser the URL whose redirect comes back to its
    // own listener, and prints another whose page shows a code to paste.
    let mut deadline = Instant::now() + URL_TIMEOUT;
    let mut opened = None;
    let mut printed: Option<String> = None;
    while Instant::now() < deadline {
        opened = std::fs::read_to_string(scratch.join("url"))
            .ok()
            .map(|text| text.trim().to_owned())
            .filter(|url| redirect_port(url).is_some());
        if printed.is_none() {
            printed = lines
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .find_map(|line| manual_url(line));
            // Claude Code opens its browser as it prints the code URL, so a
            // browser that is not called soon after leaves the code to paste.
            if printed.is_some() {
                deadline = deadline.min(Instant::now() + BROWSER_WAIT);
            }
        }
        let exited = matches!(child.try_wait(), Ok(Some(_)));
        if opened.is_some() || (printed.is_some() && !cfg!(unix)) || exited {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if opened.is_none() && printed.is_none() {
        let _ = child.kill();
        let _ = child.wait();
        cleanup(false);
        return Err(START_ERROR.into());
    }
    let port = opened.as_deref().and_then(redirect_port);
    let prompt = uuid::Uuid::new_v4().to_string();
    let stop = Arc::new(AtomicBool::new(false));
    let stdin = Arc::new(Mutex::new(child.stdin.take()));
    *service
        .claude_sign_ins
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(Active {
        account: id.clone(),
        stop: Arc::clone(&stop),
        stdin: Arc::clone(&stdin),
        port,
        prompt: prompt.clone(),
    });
    emit(service, json!({ "stage": "start" }));
    match (&opened, &printed) {
        (Some(url), _) => emit(
            service,
            json!({ "stage": "event", "event": { "type": "auth_url", "url": url,
                "instructions": "Approve the sign-in in your browser. Claude Code keeps the account's token." } }),
        ),
        (None, Some(url)) => {
            emit(
                service,
                json!({ "stage": "event", "event": { "type": "auth_url", "url": url,
                    "instructions": "Approve the sign-in in your browser, then paste the code it shows." } }),
            );
            emit(
                service,
                json!({ "stage": "prompt", "id": prompt, "kind": "input", "title": PASTE_TITLE, "placeholder": "Code" }),
            );
        }
        (None, None) => {}
    }
    let app = Arc::clone(service);
    std::thread::Builder::new()
        .name("claude-login".into())
        .spawn(move || {
            let started = Instant::now();
            let exit = loop {
                if stop.load(Ordering::SeqCst) || started.elapsed() > SIGN_IN_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                match child.try_wait() {
                    Ok(Some(exit)) => break Some(exit),
                    Ok(None) => std::thread::sleep(Duration::from_millis(200)),
                    Err(_) => break None,
                }
            };
            stdin.lock().unwrap_or_else(|e| e.into_inner()).take();
            let cancelled = stop.load(Ordering::SeqCst);
            let outcome = match exit {
                Some(exit) if exit.success() => finish(&app.agent, &executable, &id),
                Some(_) => Err(failure(&lines.lock().unwrap_or_else(|e| e.into_inner()))),
                None if cancelled => Err(String::new()),
                None => Err("The sign-in timed out. Start it again.".into()),
            };
            cleanup(outcome.is_ok());
            app.claude_sign_ins.finish(&id);
            match outcome {
                Ok(()) => {
                    emit(
                        &app,
                        json!({ "stage": "done", "type": "oauth", "account": id }),
                    );
                    let agent = app.agent.clone();
                    std::thread::spawn(move || refresh(&agent, &id));
                }
                // A cancel already told the client.
                Err(message) if message.is_empty() => {}
                Err(message) => emit(&app, json!({ "stage": "failed", "message": message })),
            }
            if !cancelled {
                emit(&app, json!({ "stage": "exit", "answered": true }));
            }
        })
        .map_err(|_| START_ERROR.to_string())?;
    Ok(match port {
        Some(port) => json!({ "callback": { "port": port, "path": CALLBACK_PATH } }),
        None => Value::Null,
    })
}

/// Records the account Claude Code just signed in to, by what Claude Code
/// reports, and drops an earlier sign-in to the same account.
fn finish(agent: &Path, executable: &Path, id: &str) -> Result<(), String> {
    let folder = claude_accounts::folder(agent, id);
    let status = status(executable, Some(&folder))
        .filter(|status| status.logged_in)
        .ok_or("Claude Code finished without a signed-in account. Try again.")?;
    let account = Account {
        id: id.to_owned(),
        email: status.email,
        org_id: status.org_id,
        org_name: status.org_name,
        plan: status.plan,
        added_ms: chrono::Utc::now().timestamp_millis(),
        usage: None,
    };
    let replaced = claude_accounts::update(agent, |store| store.add(account))
        .map_err(|_| "Muniment could not save the account list. Try again.".to_string())?;
    for old in replaced {
        forget_folder(agent, &old);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_printed_url_and_the_redirect_port_read_from_claude_code_output() {
        let line = "If the browser didn't open, visit: https://claude.com/cai/oauth/authorize?code=true&redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback&state=s";
        let printed = manual_url(line).unwrap();
        assert!(printed.starts_with("https://claude.com/cai/oauth/authorize?"));
        assert_eq!(redirect_port(&printed), None);
        assert_eq!(manual_url("Opening browser to sign in…"), None);
        let opened = "https://claude.com/cai/oauth/authorize?code=true&redirect_uri=http%3A%2F%2Flocalhost%3A37029%2Fcallback&state=s";
        assert_eq!(redirect_port(opened), Some(37029));
        let elsewhere = "https://claude.com/cai/oauth/authorize?redirect_uri=http%3A%2F%2Fexample.com%3A37029%2Fcallback";
        assert_eq!(redirect_port(elsewhere), None);
    }

    #[test]
    fn a_failed_login_names_the_reason_claude_code_gave() {
        let lines = vec![
            "Opening browser to sign in…".to_owned(),
            "Paste code here if prompted > Login failed: Request failed with status code 400"
                .to_owned(),
        ];
        assert_eq!(
            failure(&lines),
            "Claude Code did not finish the sign-in: Request failed with status code 400"
        );
        assert_eq!(
            failure(&[]),
            "Claude Code did not finish the sign-in. Try again."
        );
    }

    #[test]
    fn the_status_names_the_account_and_its_plan() {
        let status = parse_status(br#"{"loggedIn":true,"authMethod":"claude.ai","email":"a@example.test","orgId":"o1","orgName":"Work","subscriptionType":"max"}"#).unwrap();
        assert!(status.logged_in);
        assert_eq!(status.email.as_deref(), Some("a@example.test"));
        assert_eq!(status.org_id.as_deref(), Some("o1"));
        assert_eq!(status.plan.as_deref(), Some("max"));
        let out = parse_status(br#"{"loggedIn":false,"authMethod":"none"}"#).unwrap();
        assert_eq!(out, Status::default());
        assert!(parse_status(b"not json").is_none());
    }

    #[test]
    fn the_usage_answer_reads_the_session_and_weekly_windows() {
        let answer = json!({
            "subscription_type": "max",
            "rate_limits_available": true,
            "rate_limits": {
                "five_hour": {"utilization": 38.0, "resets_at": "2026-10-19T18:00:00Z"},
                "seven_day": {"utilization": 100.0, "resets_at": null},
                "seven_day_opus": {"utilization": 10.0, "resets_at": null},
                "model_scoped": [{"display_name": "Fable", "utilization": 5.0, "resets_at": null}]
            }
        });
        let (usage, plan) = parse_usage(&answer, 7).unwrap();
        assert_eq!(plan.as_deref(), Some("max"));
        assert!(usage.readable);
        assert_eq!(usage.observed_at_ms, 7);
        assert_eq!(usage.windows.len(), 3);
        assert_eq!(usage.windows[0].kind, "five_hour");
        assert_eq!(usage.windows[0].remaining_percent, 62.0);
        assert_eq!(usage.windows[0].resets_at_ms, Some(1_792_432_800_000));
        assert!(usage.windows[1].limit_reached);
        assert_eq!(usage.windows[2].scope, "Fable");
        let key =
            json!({"subscription_type": null, "rate_limits_available": false, "rate_limits": null});
        let (usage, plan) = parse_usage(&key, 1).unwrap();
        assert!(!usage.readable && usage.windows.is_empty() && plan.is_none());
        assert!(parse_usage(&json!({}), 1).is_none());
    }

    /// A stand-in `claude` at the bundled path. `auth login` names its
    /// URLs, then signs in when it reads `good` or once `mark` exists, which
    /// the browser's forwarded redirect creates. `auth status` and the
    /// control protocol answer from that sign-in.
    #[cfg(unix)]
    fn fake_claude(agent: &Path, port: u16, mark: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let path = claude_accounts::bundled_paths(agent).remove(0);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let version = "0.0.0";
        let mark = mark.display();
        let script = format!(
            r#"#!/bin/sh
case "$1 $2" in
  "--version "*) echo "{version} (Claude Code)" ;;
  "auth login")
    if [ "{port}" != 0 ]; then "$BROWSER" "https://claude.com/cai/oauth/authorize?code=true&redirect_uri=http%3A%2F%2Flocalhost%3A{port}%2Fcallback&state=s"; fi
    echo "If the browser didn't open, visit: https://claude.com/cai/oauth/authorize?code=true&redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback&state=s"
    printf 'Paste code here if prompted > '
    if [ "{port}" != 0 ]; then
      i=0; while [ ! -f "{mark}" ] && [ $i -lt 100 ]; do sleep 0.1; i=$((i+1)); done
      [ -f "{mark}" ] || exit 1
    else
      read code; [ "$code" = good ] || {{ echo "Login failed: bad code"; exit 1; }}
    fi
    touch "$CLAUDE_CONFIG_DIR/.signed-in"; echo "Login successful." ;;
  "auth status")
    if [ -f "$CLAUDE_CONFIG_DIR/.signed-in" ]; then echo '{{"loggedIn":true,"authMethod":"claude.ai","email":"a@example.test","orgId":"o1","orgName":"Org","subscriptionType":"pro"}}'; else echo '{{"loggedIn":false}}'; exit 1; fi ;;
  "auth logout") rm -f "$CLAUDE_CONFIG_DIR/.signed-in" ;;
  "-p "*)
    while read line; do case "$line" in *get_usage*) echo '{{"type":"control_response","response":{{"subtype":"success","request_id":"usage","response":{{"subscription_type":"max","rate_limits_available":true,"rate_limits":{{"five_hour":{{"utilization":25.0,"resets_at":null}}}}}}}}}}' ;; esac; done ;;
esac
"#
        );
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(unix)]
    fn service(profile: &Path) -> Arc<ModelService> {
        Arc::new(ModelService::new(
            profile,
            Arc::new(|_: Option<&str>| String::new()),
        ))
    }

    #[cfg(unix)]
    fn next(events: &std::sync::mpsc::Receiver<Value>, stage: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let event = events.recv_timeout(left).expect(stage);
            assert_eq!(event["provider"], PROVIDER);
            if event["stage"] == stage {
                return event;
            }
            assert_ne!(event["stage"], "failed", "{event}");
        }
    }

    #[cfg(unix)]
    fn wait_for(what: &str, mut check: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !check() {
            assert!(Instant::now() < deadline, "{what}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_pasted_code_signs_in_an_account_that_reads_its_usage_and_disconnects() {
        let profile = std::env::temp_dir().join(format!("claude-paste-{}", uuid::Uuid::new_v4()));
        let service = service(&profile);
        let agent = service.agent.clone();
        fake_claude(&agent, 0, &profile.join("unused"));
        let events = service.subscribe();
        assert_eq!(start(&service).unwrap(), Value::Null);
        let shown = next(&events, "event");
        assert!(shown["event"]["url"]
            .as_str()
            .unwrap()
            .contains("oauth%2Fcode%2Fcallback"));
        let prompt = next(&events, "prompt");
        let id = prompt["id"].as_str().unwrap();
        assert!(service.claude_sign_ins.has_prompt(id));
        service.claude_sign_ins.answer("good").unwrap();
        let done = next(&events, "done");
        let account = done["account"].as_str().unwrap().to_owned();
        let store = claude_accounts::load(&agent);
        let held = store.account(&account).unwrap();
        assert_eq!(held.email.as_deref(), Some("a@example.test"));
        assert_eq!(held.plan.as_deref(), Some("pro"));
        assert!(claude_accounts::folder(&agent, &account)
            .join(".signed-in")
            .exists());
        // The scratch folder with the browser helper is gone.
        let left: Vec<_> = std::fs::read_dir(agent.join(claude_accounts::FOLDERS))
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, vec![account.clone()]);
        wait_for("usage", || {
            claude_accounts::load(&agent)
                .account(&account)
                .is_some_and(|account| account.usage.is_some())
        });
        let held = claude_accounts::load(&agent)
            .account(&account)
            .cloned()
            .unwrap();
        assert_eq!(held.plan.as_deref(), Some("max"));
        assert_eq!(held.usage.unwrap().windows[0].remaining_percent, 75.0);
        remove(&agent, &account).unwrap();
        assert!(claude_accounts::load(&agent).accounts.is_empty());
        assert!(!claude_accounts::folder(&agent, &account).exists());
        let _ = std::fs::remove_dir_all(&profile);
    }

    #[cfg(unix)]
    #[test]
    fn a_wrong_code_fails_with_claude_codes_reason_and_leaves_no_folder() {
        let profile = std::env::temp_dir().join(format!("claude-wrong-{}", uuid::Uuid::new_v4()));
        let service = service(&profile);
        let agent = service.agent.clone();
        fake_claude(&agent, 0, &profile.join("unused"));
        let events = service.subscribe();
        start(&service).unwrap();
        next(&events, "prompt");
        service.claude_sign_ins.answer("bad").unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        let failed = loop {
            let event = events.recv_timeout(deadline - Instant::now()).unwrap();
            if event["stage"] == "failed" {
                break event;
            }
        };
        assert_eq!(
            failed["message"],
            "Claude Code did not finish the sign-in: bad code"
        );
        assert!(claude_accounts::load(&agent).accounts.is_empty());
        wait_for("no folder", || {
            std::fs::read_dir(agent.join(claude_accounts::FOLDERS))
                .map(|entries| entries.count() == 0)
                .unwrap_or(true)
        });
        let _ = std::fs::remove_dir_all(&profile);
    }

    #[cfg(unix)]
    #[test]
    fn a_forwarded_redirect_reaches_claude_codes_own_listener() {
        let profile = std::env::temp_dir().join(format!("claude-forward-{}", uuid::Uuid::new_v4()));
        let service = service(&profile);
        let agent = service.agent.clone();
        // This listener stands in for Claude Code's own callback.
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mark = profile.join("redirected");
        fake_claude(&agent, port, &mark);
        let seen = std::thread::spawn({
            let mark = mark.clone();
            move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut buffer = [0u8; 512];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    let read = stream.read(&mut buffer).unwrap();
                    assert!(read > 0);
                    request.extend_from_slice(&buffer[..read]);
                }
                let line = String::from_utf8_lossy(&request)
                    .lines()
                    .next()
                    .unwrap()
                    .to_owned();
                std::fs::write(&mark, "").unwrap();
                stream
                    .write_all(b"HTTP/1.1 302 Found\r\nContent-Length: 0\r\n\r\n")
                    .unwrap();
                line
            }
        });
        let events = service.subscribe();
        let started = start(&service).unwrap();
        assert_eq!(
            started,
            json!({"callback": {"port": port, "path": "/callback"}})
        );
        let shown = next(&events, "event");
        assert!(shown["event"]["url"]
            .as_str()
            .unwrap()
            .contains(&format!("localhost%3A{port}")));
        assert!(service
            .claude_sign_ins
            .deliver(&format!(
                "http://localhost:{}/callback?code=abc&state=s",
                port + 1
            ))
            .is_err());
        service
            .claude_sign_ins
            .deliver(&format!(
                "http://localhost:{port}/callback?code=abc&state=s"
            ))
            .unwrap();
        assert_eq!(
            seen.join().unwrap(),
            "GET /callback?code=abc&state=s HTTP/1.1"
        );
        next(&events, "done");
        assert_eq!(claude_accounts::load(&agent).accounts.len(), 1);
        let _ = std::fs::remove_dir_all(&profile);
    }

    #[test]
    fn a_pasted_code_needs_a_sign_in_and_one_line() {
        let sign_ins = ClaudeSignIns::default();
        assert!(sign_ins.answer("abc").is_err());
        assert!(sign_ins.answer("a\u{7}b").is_err());
        assert!(sign_ins
            .deliver("http://localhost:1/callback?code=c")
            .is_err());
        assert!(!sign_ins.active());
    }
}
