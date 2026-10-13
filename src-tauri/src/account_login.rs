//! Account sign-in for a provider. The runtime's model service runs the
//! sign-in, and each step it reports on the chat-event subscription reaches
//! the window here as a `local-mode-login` event. A finished sign-in brings
//! the app to the front.

use muniment_core::attach::ModelAction;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::model_service::{self, data};

pub(crate) const LOGIN_EVENT: &str = "local-mode-login";

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
) -> Result<(), String> {
    model_service::request(
        &app,
        ModelAction::LoginStart,
        data([("provider", json!(provider))]),
    )
    .await
    .map(drop)
}

#[tauri::command]
pub(crate) async fn local_mode_account_login_answer(
    app: AppHandle,
    id: String,
    value: Option<String>,
    confirmed: Option<bool>,
) -> Result<(), String> {
    model_service::request(
        &app,
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
    if model_service::request(&app, ModelAction::LoginCancel, json!({}))
        .await
        .is_err()
    {
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
