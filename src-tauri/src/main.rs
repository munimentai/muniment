#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use keyring::Entry;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

const API: &str = "https://api.muniment.ai";
const CREDENTIAL_SERVICE: &str = "ai.muniment.desktop";
const CREDENTIAL_USER: &str = "session";

#[derive(Clone, Serialize)]
struct ShellSession {
    org: Value,
    user: Value,
    entitlement_snapshot: Value,
}

#[derive(Deserialize)]
struct AuthResponse {
    org: Value,
    user: Value,
    session: SessionToken,
    entitlement_snapshot: Option<Value>,
}

#[derive(Deserialize)]
struct SessionToken {
    token: String,
}

struct ControlPlaneError {
    message: String,
    invalid_session: bool,
}

impl ControlPlaneError {
    fn unavailable(error: impl ToString) -> Self {
        Self {
            message: error.to_string(),
            invalid_session: false,
        }
    }
}

fn credential() -> Result<Entry, String> {
    Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_USER).map_err(|e| e.to_string())
}

async fn response_json(response: reqwest::Response) -> Result<Value, ControlPlaneError> {
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(ControlPlaneError::unavailable)?;
    if status.is_success() {
        serde_json::from_str(&body).map_err(|_| ControlPlaneError {
            message: "The control plane returned an unreadable response.".into(),
            invalid_session: false,
        })
    } else {
        let message = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| {
                v.pointer("/error/message")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| format!("Control plane request failed ({status})."));
        Err(ControlPlaneError {
            message,
            invalid_session: status == reqwest::StatusCode::UNAUTHORIZED,
        })
    }
}

async fn load_shell(token: &str) -> Result<ShellSession, ControlPlaneError> {
    let client = reqwest::Client::new();
    let auth = format!("Bearer {token}");
    let context = response_json(
        client
            .get(format!("{API}/v1/auth/session"))
            .header("Authorization", &auth)
            .send()
            .await
            .map_err(ControlPlaneError::unavailable)?,
    )
    .await?;
    let snapshot = response_json(
        client
            .get(format!("{API}/v1/entitlements/snapshot"))
            .header("Authorization", &auth)
            .send()
            .await
            .map_err(ControlPlaneError::unavailable)?,
    )
    .await?;
    Ok(ShellSession {
        org: context.get("org").cloned().unwrap_or(Value::Null),
        user: context.get("user").cloned().unwrap_or(Value::Null),
        entitlement_snapshot: snapshot
            .get("entitlement_snapshot")
            .cloned()
            .unwrap_or(Value::Null),
    })
}

#[tauri::command]
async fn restore_session() -> Result<Option<ShellSession>, String> {
    let token = match credential()?.get_password() {
        Ok(token) => token,
        Err(keyring::Error::NoEntry) => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    match load_shell(&token).await {
        Ok(shell) => Ok(Some(shell)),
        Err(error) => {
            if error.invalid_session {
                let _ = credential()?.delete_credential();
            }
            Err(error.message)
        }
    }
}

#[tauri::command]
async fn begin_oidc(app: tauri::AppHandle, org_id: String) -> Result<(), String> {
    let org_id = org_id.trim();
    if org_id.is_empty() {
        return Err("Enter your organization ID.".into());
    }
    let start =
        url::Url::parse_with_params(&format!("{API}/v1/auth/oidc/start"), [("org_id", org_id)])
            .map_err(|e| e.to_string())?;
    if let Some(existing) = app.get_webview_window("sign-in") {
        let _ = existing.close();
    }

    let callback_app = app.clone();
    let completing = Arc::new(AtomicBool::new(false));
    let callback_completing = completing.clone();
    let sign_in = WebviewWindowBuilder::new(&app, "sign-in", WebviewUrl::External(start))
        .title("Sign in to muniment")
        .inner_size(560.0, 720.0)
        .navigation_handler(move |url| {
            if url.host_str() == Some("api.muniment.ai") && url.path() == "/v1/auth/oidc/callback" {
                callback_completing.store(true, Ordering::Relaxed);
                let callback = url.to_string();
                let app = callback_app.clone();
                tauri::async_runtime::spawn(async move {
                    let result: Result<ShellSession, String> = async {
                        let value = response_json(
                            reqwest::get(callback).await.map_err(|e| e.to_string())?,
                        )
                        .await
                        .map_err(|e| e.message)?;
                        let auth: AuthResponse = serde_json::from_value(value)
                            .map_err(|_| "The sign-in response was incomplete.".to_string())?;
                        credential()?
                            .set_password(&auth.session.token)
                            .map_err(|e| e.to_string())?;
                        if let Some(snapshot) = auth.entitlement_snapshot {
                            Ok(ShellSession {
                                org: auth.org,
                                user: auth.user,
                                entitlement_snapshot: snapshot,
                            })
                        } else {
                            load_shell(&auth.session.token)
                                .await
                                .map_err(|e| e.message)
                        }
                    }
                    .await;
                    let event = match result {
                        Ok(shell) => serde_json::json!({ "session": shell }),
                        Err(error) => serde_json::json!({ "error": error }),
                    };
                    let _ = app.emit("auth-complete", event);
                    if let Some(window) = app.get_webview_window("sign-in") {
                        let _ = window.close();
                    }
                });
                false
            } else {
                true
            }
        })
        .build()
        .map_err(|e| e.to_string())?;
    let close_app = app.clone();
    sign_in.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::CloseRequested { .. })
            && !completing.load(Ordering::Relaxed)
        {
            let _ = close_app.emit(
                "auth-complete",
                serde_json::json!({ "error": "Sign in was canceled." }),
            );
        }
    });
    Ok(())
}

#[tauri::command]
async fn sign_out() -> Result<(), String> {
    if let Ok(token) = credential()?.get_password() {
        let _ = reqwest::Client::new()
            .post(format!("{API}/v1/auth/logout"))
            .bearer_auth(token)
            .send()
            .await;
    }
    match credential()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            restore_session,
            begin_oidc,
            sign_out
        ])
        .run(tauri::generate_context!())
        .expect("error while running muniment");
}
