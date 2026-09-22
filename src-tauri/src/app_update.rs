//! Signed application updates. Unconfigured builds never advertise an update.
use std::{sync::Mutex, time::Duration};
use tauri::State;
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
pub struct AppUpdate(Mutex<Option<(Update, Vec<u8>)>>);

#[tauri::command]
pub async fn app_update_prepare(
    app: tauri::AppHandle,
    state: State<'_, AppUpdate>,
) -> Result<Option<String>, String> {
    #[cfg(target_os = "linux")]
    if std::env::var_os("APPIMAGE").is_none() {
        return Ok(None);
    }
    if let Some((update, _)) = state
        .0
        .lock()
        .map_err(|_| "Update state is unavailable.")?
        .as_ref()
    {
        return Ok(Some(update.version.clone()));
    }
    let Some(key) = option_env!("MUNIMENT_UPDATER_PUBLIC_KEY").filter(|key| !key.trim().is_empty())
    else {
        return Ok(None);
    };
    let endpoint = option_env!("MUNIMENT_UPDATER_ENDPOINT")
        .unwrap_or("https://github.com/munimentai/muniment/releases/latest/download/latest.json");
    let updater = app
        .updater_builder()
        .pubkey(key)
        .endpoints(vec![endpoint
            .parse()
            .map_err(|_| "The update address is invalid.")?])
        .map_err(|_| "The update address is invalid.")?
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|_| "Updates are unavailable.")?;
    let Some(update) = updater
        .check()
        .await
        .map_err(|_| "Updates could not be checked.")?
    else {
        return Ok(None);
    };
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|_| "The update could not be downloaded or verified.")?;
    let version = update.version.clone();
    *state.0.lock().map_err(|_| "Update state is unavailable.")? = Some((update, bytes));
    Ok(Some(version))
}

#[tauri::command]
pub async fn app_update_install(
    app: tauri::AppHandle,
    activity: State<'_, muniment_core::attach::RuntimeActivityRegistry>,
    state: State<'_, AppUpdate>,
) -> Result<(), String> {
    muniment_core::attach::evaluate_quiesce(activity.snapshot())
        .map_err(|_| "Finish the current action before updating.")?;
    let ready = state
        .0
        .lock()
        .map_err(|_| "Update state is unavailable.")?
        .take()
        .ok_or("No verified update is ready.")?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let result = ready.0.install(&ready.1);
        (ready, result)
    })
    .await
    .map_err(|_| "The update could not be installed.")?;
    if result.1.is_err() {
        *state.0.lock().map_err(|_| "Update state is unavailable.")? = Some(result.0);
        return Err("The update could not be installed. Try again.".into());
    }
    app.restart();
}
