//! Local mode and Settings → Models, the provider half. The local-mode marker
//! and the provider registry are read here. Every provider setting, key,
//! endpoint, default and inventory read is a forward to the runtime's model
//! service.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use muniment_core::attach::ModelAction;
use muniment_core::local_mode::LOCAL_MODE_MARKER;
use muniment_core::models::registry;
use serde_json::{json, Value};

use crate::model_service::{self, data};

fn config_directory<R: tauri::Runtime>(_app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    // The runtime and the desktop read the same local-mode marker under one root.
    muniment_runtime::profile_directory()
        .map_err(|_| "Muniment cannot find the local mode folder.".to_string())
}

pub(crate) fn is_active<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<bool, String> {
    Ok(muniment_core::local_mode::is_local_mode(&config_directory(
        app,
    )?))
}

fn set_local_mode(config_directory: &Path, enabled: bool) -> Result<(), String> {
    fs::create_dir_all(config_directory)
        .map_err(|_| "Local mode could not be changed.".to_string())?;
    let marker = config_directory.join(LOCAL_MODE_MARKER);
    if enabled {
        OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(marker)
            .and_then(|mut file| file.write_all(b"1"))
            .map_err(|_| "Local mode could not be changed.".to_string())
    } else {
        match fs::remove_file(marker) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err("Local mode could not be changed.".into()),
        }
    }
}

/// Every provider fact the interface reads, from the model registry.
#[tauri::command]
pub(crate) fn model_registry() -> registry::Registry {
    registry::registry()
}

#[tauri::command]
pub(crate) fn local_mode_status(app: tauri::AppHandle) -> Result<bool, String> {
    is_active(&app)
}

#[tauri::command]
pub(crate) fn local_mode_enter(app: tauri::AppHandle) -> Result<(), String> {
    set_local_mode(&config_directory(&app)?, true)
}

#[tauri::command]
pub(crate) fn local_mode_leave(app: tauri::AppHandle) -> Result<(), String> {
    set_local_mode(&config_directory(&app)?, false)
}

#[tauri::command]
pub(crate) async fn local_mode_provider_status(app: tauri::AppHandle) -> Result<Value, String> {
    model_service::request(&app, ModelAction::ProviderStatus, json!({})).await
}

#[tauri::command]
pub(crate) async fn local_mode_store_provider_key(
    app: tauri::AppHandle,
    provider: String,
    key: String,
    env: Option<std::collections::BTreeMap<String, String>>,
) -> Result<(), String> {
    model_service::request(
        &app,
        ModelAction::StoreProviderKey,
        data([
            ("provider", json!(provider)),
            ("key", json!(key)),
            ("env", json!(env)),
        ]),
    )
    .await
    .map(drop)
}

#[tauri::command]
pub(crate) async fn local_mode_store_local_provider(
    app: tauri::AppHandle,
    base_url: String,
    api_key: Option<String>,
) -> Result<(), String> {
    model_service::request(
        &app,
        ModelAction::StoreLocalProvider,
        data([("base_url", json!(base_url)), ("api_key", json!(api_key))]),
    )
    .await
    .map(drop)
}

#[tauri::command]
pub(crate) async fn local_mode_provider_inventory(
    app: tauri::AppHandle,
    force: Option<bool>,
) -> Result<Value, String> {
    model_service::request(
        &app,
        ModelAction::ProviderInventory,
        data([("force", json!(force))]),
    )
    .await
}

/// The compaction settings apply when the next message starts. The runtime
/// reads and saves them through its engine adapter.
#[cfg(any(unix, target_os = "windows"))]
#[tauri::command]
pub(crate) async fn context_settings(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    crate::chat_threads::context_settings_request(app, |client| client.context_settings()).await
}

#[cfg(any(unix, target_os = "windows"))]
#[tauri::command]
pub(crate) async fn context_settings_save(
    app: tauri::AppHandle,
    enabled: bool,
    reserve_tokens: u64,
    keep_recent_tokens: u64,
) -> Result<serde_json::Value, String> {
    crate::chat_threads::context_settings_request(app, move |client| {
        client.save_context_settings(enabled, reserve_tokens, keep_recent_tokens)
    })
    .await
}

#[tauri::command]
pub(crate) async fn local_mode_set_default_model(
    app: tauri::AppHandle,
    provider: String,
    model: String,
) -> Result<(), String> {
    crate::subscription_probe::model_save(&provider, &model, "pending", None);
    let result = model_service::request(
        &app,
        ModelAction::SetDefaultModel,
        data([("provider", json!(provider)), ("model", json!(model))]),
    )
    .await
    .map(drop);
    crate::subscription_probe::model_save(
        &provider,
        &model,
        if result.is_ok() { "complete" } else { "failed" },
        None,
    );
    result
}

/// A thread's model and where it comes from: the thread, its agent, its
/// project or the default.
#[tauri::command]
pub(crate) async fn thread_settings(app: tauri::AppHandle, thread_id: String) -> Result<Value, String> {
    model_service::request(&app, ModelAction::ThreadSettings, json!({ "thread_id": thread_id })).await
}

/// Changes one thread's model, or returns it to the one it started with. The
/// default model stays as it is.
#[tauri::command]
pub(crate) async fn thread_settings_set(
    app: tauri::AppHandle,
    thread_id: String,
    model: Option<Value>,
    reset: Option<bool>,
) -> Result<Value, String> {
    let mut body = json!({ "thread_id": thread_id });
    if let Some(model) = model {
        body["model"] = model;
    }
    if reset == Some(true) {
        body["reset"] = json!(true);
    }
    model_service::request(&app, ModelAction::ThreadSettingsSet, body).await
}

/// Saves the thinking level a model runs at when a chat names none. No level
/// restores the runtime's own default for the model.
#[tauri::command]
pub(crate) async fn local_mode_set_thinking_level(
    app: tauri::AppHandle,
    provider: String,
    model: String,
    level: Option<String>,
) -> Result<(), String> {
    model_service::request(
        &app,
        ModelAction::SetThinkingLevel,
        data([
            ("provider", json!(provider)),
            ("model", json!(model)),
            ("level", json!(level)),
        ]),
    )
    .await
    .map(drop)
}

#[tauri::command]
pub(crate) async fn local_mode_set_model_hidden(
    app: tauri::AppHandle,
    provider: String,
    model: String,
    hidden: bool,
) -> Result<(), String> {
    model_service::request(
        &app,
        ModelAction::SetModelHidden,
        data([
            ("provider", json!(provider)),
            ("model", json!(model)),
            ("hidden", json!(hidden)),
        ]),
    )
    .await
    .map(drop)
}

#[tauri::command]
pub(crate) async fn local_mode_disconnect_provider(
    app: tauri::AppHandle,
    provider: String,
) -> Result<(), String> {
    model_service::request(
        &app,
        ModelAction::DisconnectProvider,
        data([("provider", json!(provider))]),
    )
    .await
    .map(drop)
}

#[tauri::command]
pub(crate) async fn local_mode_store_endpoint(
    app: tauri::AppHandle,
    kind: String,
    name: String,
    base_url: String,
    key: String,
    models: Vec<String>,
) -> Result<String, String> {
    let provider = model_service::request(
        &app,
        ModelAction::StoreEndpoint,
        data([
            ("kind", json!(kind)),
            ("name", json!(name)),
            ("base_url", json!(base_url)),
            ("key", json!(key)),
            ("models", json!(models)),
        ]),
    )
    .await?;
    serde_json::from_value(provider)
        .map_err(|_| "The background service returned an invalid response. Restart Muniment.".into())
}

#[tauri::command]
pub(crate) async fn local_mode_claude_code_status(app: tauri::AppHandle) -> Result<Value, String> {
    model_service::request(&app, ModelAction::ClaudeCodeStatus, json!({})).await
}

#[tauri::command]
pub(crate) async fn local_mode_connect_claude_code(
    app: tauri::AppHandle,
    executable: String,
) -> Result<(), String> {
    model_service::request(
        &app,
        ModelAction::ConnectClaudeCode,
        data([("executable", json!(executable))]),
    )
    .await
    .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_directory() -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("muniment-local-mode-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        path
    }

    /// The interface tests and the browser probe read the registry from this
    /// fixture. `MUNIMENT_WRITE_FIXTURES=1` rewrites it from the registry.
    #[test]
    fn the_registry_fixture_matches_the_registry() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../test/probe/fixtures/model-registry.json");
        if std::env::var_os("MUNIMENT_WRITE_FIXTURES").is_some() {
            let text = serde_json::to_string_pretty(&model_registry()).unwrap() + "\n";
            fs::write(&fixture, text).unwrap();
        }
        let registry = serde_json::to_value(model_registry()).unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&fixture).unwrap()).unwrap();
        assert_eq!(
            saved, registry,
            "Rewrite the fixture with MUNIMENT_WRITE_FIXTURES=1 when the registry changes."
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn desktop_local_mode_directory_matches_the_runtime() {
        let app = tauri::test::mock_app();
        assert_eq!(
            config_directory(app.handle()).unwrap(),
            muniment_runtime::config_directory().unwrap()
        );
    }

    #[test]
    fn marker_changes_local_mode_without_network_access() {
        let directory = temporary_directory();
        set_local_mode(&directory, true).unwrap();
        assert!(directory.join(LOCAL_MODE_MARKER).is_file());
        set_local_mode(&directory, false).unwrap();
        assert!(!directory.join(LOCAL_MODE_MARKER).exists());
        fs::remove_dir_all(directory).unwrap();
    }
}
