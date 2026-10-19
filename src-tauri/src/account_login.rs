//! Account sign-in for a provider. The runtime's model service runs the
//! sign-in, and each step it reports on the chat-event subscription reaches
//! the window here as a `local-mode-login` event. A finished sign-in brings
//! the app to the front.
//!
//! A paired runtime runs a sign-in on its own computer, but the browser's
//! `localhost` redirect lands on this one. While such a sign-in runs, this
//! computer holds the callback port the runtime names and forwards each
//! redirect to it.
//!
//! A sign-in stays on the machine the window started it on. Its forwarded
//! redirect, its answers and its cancel go there, and the window takes its
//! steps from there alone, even when another machine is selected meanwhile.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use muniment_core::attach::{ModelAction, SubscriptionEvent};
use muniment_runtime::sign_in_callback::{self, CallbackServer};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::attach_service::AttachCompanionState;
use crate::model_service::{self, data};

pub(crate) const LOGIN_EVENT: &str = "local-mode-login";
/// The id a window names this computer's runtime by.
const LOCAL_RUNTIME: &str = "local";
/// How long this computer holds the port when the sign-in never ends.
const FORWARD_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// The callback port held for a paired runtime's sign-in, by its generation.
static FORWARD: Mutex<Option<(u64, CallbackServer)>> = Mutex::new(None);
static FORWARD_GENERATION: AtomicU64 = AtomicU64::new(0);
/// The machine the sign-in in flight runs on: `Some(None)` for this
/// computer's runtime, `Some(Some(id))` for a paired one, `None` while none runs.
static SIGN_IN_RUNTIME: Mutex<Option<Option<String>>> = Mutex::new(None);

fn pin_sign_in(runtime: Option<Option<String>>) {
    *SIGN_IN_RUNTIME.lock().unwrap_or_else(|e| e.into_inner()) = runtime;
}

/// The machine a sign-in command goes to: the one the sign-in in flight runs
/// on, else the selected one.
fn sign_in_runtime<R: tauri::Runtime>(app: &AppHandle<R>) -> Option<String> {
    SIGN_IN_RUNTIME
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_else(|| app.state::<AttachCompanionState>().remote_runtime_id())
}

/// The machine a window names: `local` for this computer, a connected paired
/// runtime's id, or none for the selected machine.
fn named_runtime<R: tauri::Runtime>(
    app: &AppHandle<R>,
    runtime: Option<&str>,
) -> Result<Option<String>, String> {
    let attach = app.state::<AttachCompanionState>();
    match runtime {
        None => Ok(attach.remote_runtime_id()),
        Some(LOCAL_RUNTIME) => Ok(None),
        Some(id) if attach.has_remote(id) => Ok(Some(id.to_owned())),
        Some(_) => Err("That machine is not connected. Pick it again, then sign in.".into()),
    }
}

/// Releases the callback port, when this computer holds it for `generation`
/// or, with none, for any sign-in. The threads stop off the caller's thread,
/// because one may be waiting on the runtime's answer.
fn release_callback(generation: Option<u64>) {
    let held = {
        let mut forward = FORWARD.lock().unwrap_or_else(|e| e.into_inner());
        match (&*forward, generation) {
            (Some((held, _)), Some(generation)) if *held != generation => None,
            _ => forward.take(),
        }
    };
    if let Some((_, server)) = held {
        std::thread::spawn(move || drop(server));
    }
}

/// The callback port a sign-in's start answer names, when it names one.
fn callback_port(started: &Value) -> Option<u16> {
    started
        .pointer("/callback/port")
        .and_then(Value::as_u64)
        .and_then(|port| u16::try_from(port).ok())
}

/// The path the redirect lands on: the one the start answer names, else
/// OpenAI's.
fn callback_path(started: &Value) -> String {
    started
        .pointer("/callback/path")
        .and_then(Value::as_str)
        .filter(|path| path.starts_with('/') && !path.contains(['?', '#', ' ']))
        .unwrap_or(sign_in_callback::CALLBACK_PATH)
        .to_owned()
}

/// Holds the callback port the paired runtime's sign-in names on this
/// computer's loopback, and forwards each redirect to the runtime.
fn hold_callback<R: tauri::Runtime>(
    app: &AppHandle<R>,
    runtime: Option<String>,
    started: &Value,
    provider: &str,
) -> Result<(), String> {
    release_callback(None);
    let Some(port) = callback_port(started) else {
        return Ok(());
    };
    let listeners = sign_in_callback::bind(port)?;
    let forward_app = app.clone();
    let server = CallbackServer::start_on(
        listeners,
        port,
        callback_path(started),
        Arc::new(muniment_runtime::sign_in_page),
        sign_in_callback::provider_display_name(provider).to_owned(),
        Arc::new(move |url| {
            tauri::async_runtime::block_on(model_service::request_on(
                &forward_app,
                runtime.clone(),
                ModelAction::LoginCallback,
                data([("url", json!(url))]),
            ))
            .is_ok()
        }),
    );
    let generation = FORWARD_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    *FORWARD.lock().unwrap_or_else(|e| e.into_inner()) = Some((generation, server));
    std::thread::spawn(move || {
        std::thread::sleep(FORWARD_TIMEOUT);
        release_callback(Some(generation));
    });
    Ok(())
}

/// Starts a sign-in through `action` on the machine `runtime` names, or on
/// the selected one without it. A paired runtime's sign-in gets its callback
/// port held here, and a port this computer cannot hold cancels it with the
/// reason.
pub(crate) async fn start_sign_in<R: tauri::Runtime>(
    app: &AppHandle<R>,
    action: ModelAction,
    provider: String,
    runtime: Option<String>,
) -> Result<(), String> {
    release_callback(None);
    let runtime = named_runtime(app, runtime.as_deref())?;
    pin_sign_in(Some(runtime.clone()));
    let started = match model_service::request_on(
        app,
        runtime.clone(),
        action,
        data([("provider", json!(provider))]),
    )
    .await
    {
        Ok(started) => started,
        Err(message) => {
            pin_sign_in(None);
            return Err(message);
        }
    };
    if runtime.is_none() {
        return Ok(());
    }
    if let Err(message) = hold_callback(app, runtime.clone(), &started, &provider) {
        let _ = model_service::request_on(app, runtime, ModelAction::LoginCancel, json!({})).await;
        pin_sign_in(None);
        return Err(message);
    }
    Ok(())
}

/// Hands a sign-in step from `runtime`, a paired runtime's id or none for
/// this computer's, to the window when it comes from the machine the sign-in
/// runs on, or from the selected machine while none runs. Any other event
/// comes back to the caller.
pub(crate) fn route_step<R: tauri::Runtime>(
    app: &AppHandle<R>,
    runtime: Option<&str>,
    event: SubscriptionEvent,
) -> Option<SubscriptionEvent> {
    let SubscriptionEvent::ModelsLogin(step) = event else {
        return Some(event);
    };
    if sign_in_runtime(app).as_deref() == runtime {
        deliver_step(app, step);
    }
    None
}

fn present_main_window<R: tauri::Runtime>(app: &AppHandle<R>) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.unminimize();
        let _ = main.show();
        let _ = main.set_focus();
    }
}

/// Hands one sign-in step to the window. A finished sign-in, or a pool
/// sign-in whose credential the router could not hold, brings the app to the
/// front, so the user sees the result.
pub(crate) fn deliver_step<R: tauri::Runtime>(app: &AppHandle<R>, step: Value) {
    let stage = step.get("stage").and_then(Value::as_str);
    let pool = step.get("pool").and_then(Value::as_bool) == Some(true);
    let present = stage == Some("done") || (stage == Some("failed") && pool);
    if matches!(stage, Some("done" | "failed" | "cancelled" | "exit")) {
        release_callback(None);
        pin_sign_in(None);
    }
    let _ = app.emit(LOGIN_EVENT, step);
    if present {
        present_main_window(app);
    }
}

/// A first sign-in may install Pi, and the runtime answers once it starts.
#[tauri::command]
pub(crate) async fn local_mode_account_login_start(
    app: AppHandle,
    provider: String,
    runtime: Option<String>,
) -> Result<(), String> {
    start_sign_in(&app, ModelAction::LoginStart, provider, runtime).await
}

#[tauri::command]
pub(crate) async fn local_mode_account_login_answer(
    app: AppHandle,
    id: String,
    value: Option<String>,
    confirmed: Option<bool>,
) -> Result<(), String> {
    model_service::request_on(
        &app,
        sign_in_runtime(&app),
        ModelAction::LoginAnswer,
        data([
            ("id", json!(id)),
            ("value", json!(value)),
            ("confirmed", json!(confirmed)),
        ]),
    )
    .await
    .map(drop)
}

/// Stops the sign-in in flight. When the runtime cannot answer, the window
/// still hears the cancel, so it leaves the sign-in view.
#[tauri::command]
pub(crate) async fn local_mode_account_login_cancel(app: AppHandle) {
    release_callback(None);
    let runtime = sign_in_runtime(&app);
    if model_service::request_on(&app, runtime, ModelAction::LoginCancel, json!({}))
        .await
        .is_err()
    {
        pin_sign_in(None);
        let _ = app.emit(LOGIN_EVENT, json!({ "stage": "cancelled" }));
    }
}

fn sign_in_url(url: &str) -> Result<url::Url, String> {
    let parsed = url::Url::parse(url).map_err(|_| "The sign-in link is invalid.".to_string())?;
    if parsed.scheme() != "https" || parsed.host().is_none() {
        return Err("The sign-in link is invalid.".into());
    }
    Ok(parsed)
}

/// Opens a provider's sign-in page in the default browser. The opener hands
/// the URL to the OS as one argument, so no shell reads its query.
#[tauri::command]
pub(crate) fn local_mode_open_url(app: AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let parsed = sign_in_url(&url)?;
    app.opener()
        .open_url(parsed.as_str(), None::<String>)
        .map_err(|_| "The browser could not open. Copy the link instead.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Listener;

    #[test]
    fn sign_in_links_accept_only_https_with_a_host() {
        let url = "https://example.com/login?a=1&b=2|calc";
        assert_eq!(sign_in_url(url).unwrap().as_str(), url);
        for bad in [
            "http://example.com",
            "file:///etc/passwd",
            "https://",
            "calc.exe",
        ] {
            assert!(sign_in_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_start_answer_names_the_callback_port_to_hold() {
        assert_eq!(
            callback_port(&json!({"callback": {"port": 1455, "path": "/auth/callback"}})),
            Some(1455)
        );
        assert_eq!(callback_port(&Value::Null), None);
        assert_eq!(callback_port(&json!({"callback": {"port": 70000}})), None);
        assert_eq!(
            callback_path(&json!({"callback": {"port": 40259, "path": "/callback"}})),
            "/callback"
        );
        assert_eq!(
            callback_path(&json!({"callback": {"port": 1455}})),
            "/auth/callback"
        );
        assert_eq!(
            callback_path(&json!({"callback": {"path": "/x?y"}})),
            "/auth/callback"
        );
    }

    #[test]
    fn a_sign_in_takes_its_steps_from_its_own_machine_alone() {
        let app = tauri::test::mock_app();
        app.manage(AttachCompanionState::default());
        let (sender, receiver) = std::sync::mpsc::channel();
        app.listen(LOGIN_EVENT, move |event| {
            let _ = sender.send(serde_json::from_str::<Value>(event.payload()).unwrap());
        });
        let step = |stage: &str| SubscriptionEvent::ModelsLogin(json!({"stage": stage, "provider": "anthropic"}));
        let heard = || receiver.recv_timeout(std::time::Duration::from_millis(200)).ok();
        pin_sign_in(Some(Some("box".into())));
        // This computer is selected, but the sign-in runs on the paired machine.
        assert!(route_step(app.handle(), None, step("done")).is_none());
        assert_eq!(heard(), None);
        assert!(route_step(app.handle(), Some("box"), step("done")).is_none());
        assert_eq!(heard().unwrap()["stage"], "done");
        // A finished sign-in pins nothing, so the selected machine's steps arrive.
        assert_eq!(sign_in_runtime(app.handle()), None);
        assert!(route_step(app.handle(), None, step("exit")).is_none());
        assert_eq!(heard().unwrap()["stage"], "exit");
        let other = SubscriptionEvent::Chat(json!({}));
        assert!(route_step(app.handle(), None, other).is_some());
    }

    #[test]
    fn each_sign_in_step_reaches_the_window_as_the_login_event() {
        let app = tauri::test::mock_app();
        let (sender, receiver) = std::sync::mpsc::channel();
        app.listen(LOGIN_EVENT, move |event| {
            let _ = sender.send(serde_json::from_str::<Value>(event.payload()).unwrap());
        });
        let step = json!({"stage": "event", "event": {"type": "auth_url", "url": "https://x"}, "provider": "xai"});
        deliver_step(app.handle(), step.clone());
        assert_eq!(
            receiver
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap(),
            step
        );
    }
}
