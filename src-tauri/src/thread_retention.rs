use muniment_core::retention_record::{
    read_retention_choice, write_retention_choice, RetentionChoice,
};
use tauri::{AppHandle, Manager};

fn config_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_config_dir()
        .map_err(|_| "Muniment configuration storage is unavailable.".to_string())
}

#[tauri::command]
pub fn thread_retention_choice(app: AppHandle) -> Result<Option<RetentionChoice>, String> {
    Ok(read_retention_choice(&config_dir(&app)?))
}

#[tauri::command]
pub fn record_thread_retention_choice(
    app: AppHandle,
    choice: RetentionChoice,
) -> Result<(), String> {
    write_retention_choice(&config_dir(&app)?, choice).map_err(|error| error.to_string())
}
