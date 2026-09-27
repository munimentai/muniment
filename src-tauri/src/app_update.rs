//! Signed application updates. Unconfigured builds never advertise an update.
use std::{sync::Mutex, time::Duration};
use tauri::State;
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
pub struct AppUpdate(Mutex<Option<(Update, Vec<u8>)>>);

impl AppUpdate {
    pub(crate) fn prepared(&self) -> Result<(Update, Vec<u8>), String> {
        self.0
            .lock()
            .map_err(|_| "Update state is unavailable.")?
            .clone()
            .ok_or_else(|| "No verified update is ready.".into())
    }
}

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
    let key = option_env!("MUNIMENT_UPDATER_PUBLIC_KEY")
        .unwrap_or(include_str!("../updater.pub"))
        .trim();
    let endpoint = option_env!("MUNIMENT_UPDATER_ENDPOINT")
        .unwrap_or("https://github.com/munimentai/muniment/releases/latest/download/latest.json");
    let builder = app.updater_builder();
    #[cfg(target_os = "windows")]
    let builder = builder.target(windows_update_target()?);
    let builder = builder
        .pubkey(key)
        .endpoints(vec![endpoint
            .parse()
            .map_err(|_| "The update address is invalid.")?])
        .map_err(|_| "The update address is invalid.")?
        .timeout(Duration::from_secs(120));
    let updater = crate::subscription_probe::update_builder(builder)?
        .build()
        .map_err(|_| "Updates are unavailable.")?;
    let Some(mut update) = updater
        .check()
        .await
        .map_err(|_| "Updates could not be checked.")?
    else {
        return Ok(None);
    };
    crate::subscription_probe::check_update_download(&update)?;
    update.timeout = Some(Duration::from_secs(120));
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
    finish_install(&state.0, result.0, result.1, || app.restart())
}

fn finish_install<T, E>(
    state: &Mutex<Option<T>>,
    ready: T,
    result: Result<(), E>,
    restart: impl FnOnce(),
) -> Result<(), String> {
    if result.is_err() {
        *state.lock().map_err(|_| "Update state is unavailable.")? = Some(ready);
        return Err("The update could not be installed. Try again.".into());
    }
    restart();
    Err("The updated app did not restart.".into())
}

// The CEF sandbox bootstrap loads the application DLL, so Tauri's executable
// bundle marker cannot reliably identify the installed Windows package format.
#[cfg(target_os = "windows")]
fn windows_update_target() -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    let exe = std::env::current_exe().map_err(|_| "The installed app path is unavailable.")?;
    let directory = exe
        .parent()
        .ok_or("The installed app path is unavailable.")?;
    let script = include_str!("../../scripts/windows-update-target.ps1");
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("MUNIMENT_UPDATE_INSTALL_DIR", directory)
        .creation_flags(0x08000000)
        .output()
        .map_err(|_| "The installed app package is unavailable.")?;
    let target = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !output.status.success()
        || !matches!(
            target.as_str(),
            "windows-x86_64-msi-user" | "windows-x86_64-msi-machine" | "windows-x86_64-nsis"
        )
    {
        return Err("The installed app package is unavailable.".into());
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::finish_install;
    use std::sync::Mutex;

    #[test]
    fn failed_install_preserves_candidate_without_restart() {
        let state = Mutex::new(None);
        let result = finish_install(&state, vec![1, 2, 3], Err(()), || {
            panic!("Unexpected restart.")
        });
        assert_eq!(
            result.unwrap_err(),
            "The update could not be installed. Try again."
        );
        assert_eq!(*state.lock().unwrap(), Some(vec![1, 2, 3]));
    }

    #[test]
    fn successful_install_requires_restart() {
        let state = Mutex::new(None);
        let mut restarted = false;
        let result = finish_install(&state, (), Ok::<_, ()>(()), || restarted = true);
        assert!(restarted);
        assert_eq!(result.unwrap_err(), "The updated app did not restart.");
        assert!(state.lock().unwrap().is_none());
    }

    #[test]
    fn unconfigured_build_has_valid_plugin_configuration() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let updater: tauri_plugin_updater::Config =
            serde_json::from_value(config["plugins"]["updater"].clone()).unwrap();
        assert!(updater.pubkey.is_empty());
        assert!(updater.require_signed_version);
        assert!(updater.endpoints.is_empty());
    }
}
