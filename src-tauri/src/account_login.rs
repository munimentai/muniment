//! Account sign-in for a Pi provider: Pi's own OAuth flow runs inside an RPC
//! process the desktop owns, through the `muniment-login` extension command,
//! and every step reaches the shell as a `local-mode-login` event.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;
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

struct ActiveLogin {
    child: Child,
    stdin: ChildStdin,
    generation: u64,
}

#[derive(Default)]
pub(crate) struct AccountLoginState {
    active: Mutex<Option<ActiveLogin>>,
    generation: Mutex<u64>,
}

impl AccountLoginState {
    fn stop(&self) {
        if let Some(mut login) = self.active.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = login.child.kill();
            let _ = login.child.wait();
        }
    }
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

pub(crate) fn start(app: &AppHandle, provider: &str) -> Result<(), String> {
    if !ACCOUNT_PROVIDERS.contains(&provider) {
        return Err("This provider has no account sign-in.".into());
    }
    let state = app.state::<AccountLoginState>();
    state.stop();
    let agent = crate::local_mode::harness_agent_directory(START_ERROR)?;
    let executable = crate::local_mode::pi_executable().ok_or_else(|| START_ERROR.to_string())?;
    let extension = write_extension(&agent)?;
    let mut child = Command::new(executable)
        .args(["--mode", "rpc", "--extension"])
        .arg(&extension)
        .env("PI_CODING_AGENT_DIR", &agent)
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
    });
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
        let mut active = state.active.lock().unwrap_or_else(|e| e.into_inner());
        if active
            .as_ref()
            .is_some_and(|login| login.generation == generation)
        {
            if let Some(mut login) = active.take() {
                let _ = login.child.kill();
                let _ = login.child.wait();
            }
            emit(
                &reader_app,
                json!({ "stage": "exit", "provider": provider, "answered": answered }),
            );
        }
    });
    let watchdog_app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(LOGIN_TIMEOUT);
        let state = watchdog_app.state::<AccountLoginState>();
        let mut active = state.active.lock().unwrap_or_else(|e| e.into_inner());
        if active
            .as_ref()
            .is_some_and(|login| login.generation == generation)
        {
            if let Some(mut login) = active.take() {
                let _ = login.child.kill();
            }
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
    fn only_account_providers_start_a_sign_in() {
        assert!(ACCOUNT_PROVIDERS.contains(&"openai-codex"));
        assert!(ACCOUNT_PROVIDERS.contains(&"xai"));
        assert!(!ACCOUNT_PROVIDERS.contains(&"google"));
    }
}
