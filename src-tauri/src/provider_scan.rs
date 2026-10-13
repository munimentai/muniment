//! The first-run scan for model servers on this computer and for coding tools
//! that already hold a provider sign-in. The runtime's model service runs the
//! scan and saves a found server, and these commands forward to it.

use muniment_core::attach::ModelAction;
use serde_json::{json, Value};

use crate::model_service::{self, data};

/// Finds model servers on this computer and the tools that hold a provider sign-in.
#[tauri::command]
pub(crate) async fn onboarding_provider_scan(app: tauri::AppHandle) -> Result<Value, String> {
    model_service::request(&app, ModelAction::ProviderScan, json!({})).await
}

/// Saves a found server as a provider through the same storage as Settings.
#[tauri::command]
pub(crate) async fn onboarding_connect_server(
    app: tauri::AppHandle,
    id: String,
    base_url: String,
    key: Option<String>,
    key_from_file: bool,
) -> Result<(), String> {
    model_service::request(
        &app,
        ModelAction::ConnectServer,
        data([
            ("id", json!(id)),
            ("base_url", json!(base_url)),
            ("key", json!(key)),
            ("key_from_file", json!(key_from_file)),
        ]),
    )
    .await
    .map(drop)
}
