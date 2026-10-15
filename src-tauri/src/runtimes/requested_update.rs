//! Applies an update that a paired client requested on this computer's
//! runtime: the app checks its release feed, downloads the verified update
//! and installs it once no runtime work is active. The runtime keeps the
//! request in `<state>/remote/runtime-update.json`, and the app removes it
//! once it runs the requested version or a newer one.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::Value;
use tauri::{Emitter, Manager};

use super::version::{is_older, APP_VERSION};
use crate::app_update::{AppUpdate, UpdateFailure};

const REQUEST_FILE: &str = "runtime-update.json";
/// How long an install waits before it checks again for idle runtime work.
const IDLE_RETRY: Duration = Duration::from_secs(30);

/// One requested update runs at a time.
static APPLYING: AtomicBool = AtomicBool::new(false);

fn request_path() -> Option<PathBuf> {
    muniment_runtime::profile_directory()
        .ok()
        .map(|profile| profile.join("remote").join(REQUEST_FILE))
}

/// The version a saved request names.
fn saved_version(path: &Path) -> Option<String> {
    let request: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    request["version"].as_str().map(str::to_owned)
}

/// What the app does about a requested version.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Plan {
    /// The app already runs that version or a newer one.
    Done,
    Update,
    /// The version is not semver, so the app cannot compare it.
    Ignore,
}

pub(crate) fn plan(requested: &str, app_version: &str) -> Plan {
    match is_older(app_version, requested) {
        Some(true) => Plan::Update,
        Some(false) => Plan::Done,
        None => Plan::Ignore,
    }
}

fn clear(path: Option<PathBuf>) {
    if let Some(path) = path {
        if let Err(error) = std::fs::remove_file(path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                eprintln!("muniment-desktop: runtime update request could not be cleared");
            }
        }
    }
}

/// Handles the `runtime.update_requested` event of this computer's runtime.
pub(crate) fn update_requested<R: tauri::Runtime>(app: &tauri::AppHandle<R>, request: Value) {
    if let Some(version) = request["version"].as_str() {
        apply(app, version.to_owned());
    }
}

/// Applies a request the runtime saved while the app was closed, and clears
/// one the app's own version already meets.
pub(crate) fn check_saved_request<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(version) = request_path().as_deref().and_then(saved_version) {
        apply(app, version);
    }
}

fn apply<R: tauri::Runtime>(app: &tauri::AppHandle<R>, version: String) {
    match plan(&version, APP_VERSION) {
        Plan::Done => return clear(request_path()),
        Plan::Ignore => return,
        Plan::Update => {}
    }
    if APPLYING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        install_requested(&app, &version);
        APPLYING.store(false, Ordering::SeqCst);
    });
}

fn install_requested<R: tauri::Runtime>(app: &tauri::AppHandle<R>, requested: &str) {
    let (Some(state), Some(activity)) = (
        app.try_state::<AppUpdate>(),
        app.try_state::<muniment_core::attach::RuntimeActivityRegistry>(),
    ) else {
        return;
    };
    let found = match tauri::async_runtime::block_on(crate::app_update::prepare(app, &state)) {
        Ok(Some(found)) => found,
        Ok(None) => {
            eprintln!("muniment-desktop: runtime update to {requested} has no in-app update");
            return;
        }
        Err(error) => {
            eprintln!(
                "muniment-desktop: runtime update to {requested} failed code={}",
                error.probe_code()
            );
            return;
        }
    };
    if plan(requested, &found) == Plan::Update {
        eprintln!("muniment-desktop: the release feed does not offer {requested} yet");
        return;
    }
    // The window shows its update control for the downloaded version.
    let _ = app.emit("app-update-ready", &found);
    loop {
        match tauri::async_runtime::block_on(crate::app_update::install(app, &activity, &state)) {
            Err(UpdateFailure::Busy) => std::thread::sleep(IDLE_RETRY),
            Err(error) => {
                eprintln!(
                    "muniment-desktop: runtime update to {requested} failed code={}",
                    error.probe_code()
                );
                return;
            }
            // A successful install restarts the app.
            Ok(()) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_the_app_already_meets_is_done() {
        assert_eq!(plan("0.2.0", "0.2.0"), Plan::Done);
        assert_eq!(plan("0.1.0", "0.2.0"), Plan::Done);
        assert_eq!(plan("0.3.0", "0.2.0"), Plan::Update);
        assert_eq!(plan("v0.3.0", "0.2.0"), Plan::Update);
        assert_eq!(plan("nightly", "0.2.0"), Plan::Ignore);
    }

    #[test]
    fn a_saved_request_names_its_version() {
        let directory =
            std::env::temp_dir().join(format!("muniment-update-request-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(REQUEST_FILE);
        assert_eq!(saved_version(&path), None);
        std::fs::write(
            &path,
            r#"{"version":"0.3.0","requested_at":"2030-01-01T00:00:00Z","requested_by":"local"}"#,
        )
        .unwrap();
        assert_eq!(saved_version(&path).as_deref(), Some("0.3.0"));
        clear(Some(path.clone()));
        assert!(!path.exists());
        clear(Some(path));
        std::fs::remove_dir_all(directory).unwrap();
    }
}
