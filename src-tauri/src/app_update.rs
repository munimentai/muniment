//! Signed application updates. Unconfigured builds never advertise an update.
use std::{sync::Mutex, time::Duration};
use tauri::State;
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
pub struct AppUpdate(Mutex<Option<(Update, Vec<u8>)>>);

impl AppUpdate {
    pub(crate) fn prepared(&self) -> Result<(Update, Vec<u8>), UpdateFailure> {
        self.0
            .lock()
            .map_err(|_| UpdateFailure::State)?
            .clone()
            .ok_or(UpdateFailure::NotPrepared)
    }
}

#[derive(Debug, PartialEq)]
pub enum UpdateFailure {
    Configuration(String),
    DownloadAddress(String),
    State,
    Address,
    Builder,
    Check,
    Download,
    Busy,
    NotPrepared,
    InstallTask,
    Install,
    Restart,
}

impl UpdateFailure {
    pub(crate) fn probe_code(&self) -> &'static str {
        match self {
            Self::Configuration(_) => "update-builder",
            Self::DownloadAddress(_) => "update-address",
            Self::State => "update-state",
            Self::Address => "update-address",
            Self::Builder => "update-builder",
            Self::Check => "update-check",
            Self::Download => "update-download",
            Self::Busy => "update-busy",
            Self::NotPrepared => "update-not-prepared",
            Self::InstallTask => "update-install-task",
            Self::Install => "update-install",
            Self::Restart => "update-restart",
        }
    }

    fn message(&self) -> &str {
        match self {
            Self::Configuration(message) | Self::DownloadAddress(message) => message,
            Self::State => "Update state is unavailable.",
            Self::Address => "The update address is invalid.",
            Self::Builder => "Updates are unavailable.",
            Self::Check => "Updates could not be checked.",
            Self::Download => "The update could not be downloaded or verified.",
            Self::Busy => "Finish the current action before updating.",
            Self::NotPrepared => "No verified update is ready.",
            Self::InstallTask => "The update could not be installed.",
            Self::Install => "The update could not be installed. Try again.",
            Self::Restart => "The updated app did not restart.",
        }
    }
}

impl From<String> for UpdateFailure {
    fn from(message: String) -> Self {
        Self::Configuration(message)
    }
}

// Public commands keep their message strings. The probe reads only fixed codes.
impl serde::Serialize for UpdateFailure {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.message())
    }
}

#[tauri::command]
pub async fn app_update_prepare(
    app: tauri::AppHandle,
    state: State<'_, AppUpdate>,
) -> Result<Option<String>, UpdateFailure> {
    #[cfg(target_os = "linux")]
    if std::env::var_os("APPIMAGE").is_none() {
        return Ok(None);
    }
    if let Some((update, _)) = state.0.lock().map_err(|_| UpdateFailure::State)?.as_ref() {
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
            .map_err(|_| UpdateFailure::Address)?])
        .map_err(|_| UpdateFailure::Address)?
        .timeout(Duration::from_secs(120));
    let updater = crate::subscription_probe::update_builder(builder)?
        .build()
        .map_err(|_| UpdateFailure::Builder)?;
    let Some(mut update) = updater.check().await.map_err(|_| UpdateFailure::Check)? else {
        return Ok(None);
    };
    crate::subscription_probe::check_update_download(&update)
        .map_err(UpdateFailure::DownloadAddress)?;
    update.timeout = Some(Duration::from_secs(120));
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|_| UpdateFailure::Download)?;
    let version = update.version.clone();
    *state.0.lock().map_err(|_| UpdateFailure::State)? = Some((update, bytes));
    Ok(Some(version))
}

#[tauri::command]
pub async fn app_update_install(
    app: tauri::AppHandle,
    activity: State<'_, muniment_core::attach::RuntimeActivityRegistry>,
    state: State<'_, AppUpdate>,
) -> Result<(), UpdateFailure> {
    muniment_core::attach::evaluate_quiesce(activity.snapshot())
        .map_err(|_| UpdateFailure::Busy)?;
    let ready = state
        .0
        .lock()
        .map_err(|_| UpdateFailure::State)?
        .take()
        .ok_or(UpdateFailure::NotPrepared)?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let result = ready.0.install(&ready.1);
        (ready, result)
    })
    .await
    .map_err(|_| UpdateFailure::InstallTask)?;
    finish_install(&state.0, result.0, result.1, || app.restart())
}

fn finish_install<T, E>(
    state: &Mutex<Option<T>>,
    ready: T,
    result: Result<(), E>,
    restart: impl FnOnce(),
) -> Result<(), UpdateFailure> {
    if result.is_err() {
        *state.lock().map_err(|_| UpdateFailure::State)? = Some(ready);
        return Err(UpdateFailure::Install);
    }
    restart();
    Err(UpdateFailure::Restart)
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
    use super::{finish_install, UpdateFailure};
    use std::sync::Mutex;

    #[test]
    fn update_failures_keep_public_messages_and_fixed_probe_codes() {
        for (failure, code, message) in [
            (
                UpdateFailure::State,
                "update-state",
                "Update state is unavailable.",
            ),
            (
                UpdateFailure::Address,
                "update-address",
                "The update address is invalid.",
            ),
            (
                UpdateFailure::Builder,
                "update-builder",
                "Updates are unavailable.",
            ),
            (
                UpdateFailure::Check,
                "update-check",
                "Updates could not be checked.",
            ),
            (
                UpdateFailure::Download,
                "update-download",
                "The update could not be downloaded or verified.",
            ),
            (
                UpdateFailure::Busy,
                "update-busy",
                "Finish the current action before updating.",
            ),
            (
                UpdateFailure::NotPrepared,
                "update-not-prepared",
                "No verified update is ready.",
            ),
            (
                UpdateFailure::InstallTask,
                "update-install-task",
                "The update could not be installed.",
            ),
            (
                UpdateFailure::Install,
                "update-install",
                "The update could not be installed. Try again.",
            ),
            (
                UpdateFailure::Restart,
                "update-restart",
                "The updated app did not restart.",
            ),
            (
                UpdateFailure::Configuration("PRIVATE".into()),
                "update-builder",
                "PRIVATE",
            ),
            (
                UpdateFailure::DownloadAddress("PRIVATE".into()),
                "update-address",
                "PRIVATE",
            ),
        ] {
            assert_eq!(failure.probe_code(), code);
            assert_eq!(serde_json::to_value(&failure).unwrap(), message);
            assert!(!failure.probe_code().contains("PRIVATE"));
        }
    }

    #[test]
    fn failed_install_preserves_candidate_without_restart() {
        let state = Mutex::new(None);
        let result = finish_install(&state, vec![1, 2, 3], Err(()), || {
            panic!("Unexpected restart.")
        });
        assert_eq!(result.unwrap_err(), UpdateFailure::Install);
        assert_eq!(*state.lock().unwrap(), Some(vec![1, 2, 3]));
    }

    #[test]
    fn successful_install_requires_restart() {
        let state = Mutex::new(None);
        let mut restarted = false;
        let result = finish_install(&state, (), Ok::<_, ()>(()), || restarted = true);
        assert!(restarted);
        assert_eq!(result.unwrap_err(), UpdateFailure::Restart);
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
