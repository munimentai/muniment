use muniment_core::creations::{self, Creation};
fn profile() -> Result<std::path::PathBuf, String> {
    muniment_runtime::profile_directory().map_err(|e| e.to_string())
}
#[tauri::command]
pub fn creation_list(webview: tauri::Webview) -> Result<Vec<Creation>, String> {
    crate::workspace_tools::shell(&webview)?;
    creations::list(&profile()?)
}
#[tauri::command]
pub fn creation_save(webview: tauri::Webview, creation: Creation) -> Result<Creation, String> {
    crate::workspace_tools::shell(&webview)?;
    creations::save(&profile()?, creation)
}

/// A draft's folder lives in Home, so the delete runs off the main thread.
/// Answers the draft's folder when it stays because it holds the user's files.
#[tauri::command]
pub async fn creation_delete(webview: tauri::Webview, thread_id: String) -> Result<Option<std::path::PathBuf>, String> {
    crate::workspace_tools::shell(&webview)?;
    tauri::async_runtime::spawn_blocking(move || creations::delete(&profile()?, &thread_id))
        .await
        .map_err(|e| e.to_string())?
}
/// An artifact's folder lives in Home, so the edit runs off the main thread.
#[tauri::command]
pub async fn artifact_edit(webview: tauri::Webview, id: String, name: Option<String>) -> Result<(), String> {
    crate::workspace_tools::shell(&webview)?;
    tauri::async_runtime::spawn_blocking(move || creations::edit_artifact(&profile()?, &id, name.as_deref()))
        .await
        .map_err(|e| e.to_string())?
}
