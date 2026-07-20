use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeStatus {
    configured: bool,
    home_path: String,
}

fn config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map_err(|_| "Muniment configuration storage is unavailable.".to_string())
}

fn default_home(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .document_dir()
        .map(|path| path.join("Muniment"))
        .map_err(|_| "The Documents folder is unavailable.".to_string())
}

#[tauri::command]
pub fn home_status(app: AppHandle) -> Result<HomeStatus, String> {
    let config = config_dir(&app)?;
    let selected =
        muniment_core::home::configured_home(&config).map_err(|error| error.to_string())?;
    let configured = selected.is_some();
    let home = match selected {
        Some(home) => home,
        None => default_home(&app)?,
    };
    Ok(HomeStatus {
        configured,
        home_path: home.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub fn home_confirm(app: AppHandle, home_path: String) -> Result<HomeStatus, String> {
    let config = config_dir(&app)?;
    let home = PathBuf::from(home_path);
    muniment_core::home::confirm_home(&config, &home).map_err(|error| error.to_string())?;
    Ok(HomeStatus {
        configured: true,
        home_path: home.to_string_lossy().into_owned(),
    })
}
