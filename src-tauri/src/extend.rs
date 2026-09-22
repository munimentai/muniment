use serde_json::Value;
use tauri_plugin_opener::OpenerExt;
#[tauri::command]
pub async fn extend_command(
    app: tauri::AppHandle,
    action: String,
    data: Option<Value>,
) -> Result<Value, String> {
    let root = muniment_core::state_root::state_directory()
        .ok_or("The application folder is unavailable.")?;
    if action == "open_folder" {
        let folder = muniment_core::extend::package_folder(
            &root,
            data.as_ref().and_then(|value| value["id"].as_str()),
        )?;
        app.opener()
            .open_path(folder.to_string_lossy(), None::<String>)
            .map_err(|_| "The extension folder could not be opened.".to_string())?;
        return Ok(Value::Null);
    }
    tauri::async_runtime::spawn_blocking(move || {
        muniment_core::extend::command(&root, &action, data.unwrap_or(serde_json::json!({})))
    })
    .await
    .map_err(|_| "The extension operation stopped.".to_string())?
}
