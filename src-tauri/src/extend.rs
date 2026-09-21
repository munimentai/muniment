use serde_json::Value;
#[tauri::command]
pub async fn extend_command(action: String, data: Option<Value>) -> Result<Value, String> {
    let root = muniment_core::state_root::state_directory().ok_or("The application folder is unavailable.")?;
    tauri::async_runtime::spawn_blocking(move || muniment_core::extend::command(&root, &action, data.unwrap_or(serde_json::json!({}))))
        .await.map_err(|_| "The extension operation stopped.".to_string())?
}
