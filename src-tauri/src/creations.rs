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

#[tauri::command]
pub fn creation_delete(webview: tauri::Webview, thread_id: String) -> Result<(), String> {
    crate::workspace_tools::shell(&webview)?;
    creations::remove(&profile()?, &thread_id)
}
#[tauri::command]
pub fn artifact_edit(webview: tauri::Webview, id: String, name: Option<String>) -> Result<(), String> {
    crate::workspace_tools::shell(&webview)?;
    creations::edit_artifact(&profile()?, &id, name.as_deref())
}
