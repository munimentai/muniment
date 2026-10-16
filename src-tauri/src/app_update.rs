//! Signed application updates. Unconfigured builds never advertise an update.
//!
//! This computer's runtime reports the update's state to paired devices, so
//! the app saves each step in the runtime's update status. Before an install
//! the app asks the runtime to drain: it waits for runs to finish, or stops
//! them at once when the owner chose to, and records each run it stops. Core
//! resumes those runs after the restart.
use std::path::PathBuf;
use std::time::Instant;
use std::{sync::Mutex, time::Duration};

use muniment_core::attach::{
    read_update_request, read_update_status, update_timestamp, write_update_status,
    RuntimeUpdateState, RuntimeUpdateStatus,
};
use serde_json::Value;
use tauri::State;
use tauri_plugin_updater::{Update, UpdaterExt};

/// How long a requested or automatic install waits for runs to finish.
const DRAIN_GRACE: Duration = Duration::from_secs(120);
/// How long past the grace period the app waits for the runtime's answer.
const DRAIN_ANSWER: Duration = Duration::from_secs(15);
const DRAIN_POLL: Duration = Duration::from_millis(500);
/// The drain request this computer's runtime reads beside its update status.
const DRAIN_REQUEST_FILE: &str = "update-drain.json";
/// How long the runtime takes to report its blockers after the app queues an
/// update.
const BLOCKERS_SETTLE: Duration = Duration::from_millis(1200);

/// How an install treats the runs of this computer's runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InstallMode {
    /// The owner chose to install now. Runs stop at once and resume after
    /// the update.
    Interrupt,
    /// A requested or automatic install. Runs get up to two minutes to
    /// finish, and the ones still running then resume after the update.
    Wait,
}

impl InstallMode {
    fn grace(self) -> Duration {
        match self {
            Self::Interrupt => Duration::ZERO,
            Self::Wait => DRAIN_GRACE,
        }
    }
}

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
    Check(UpdateCheckFailure),
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
            Self::Check(failure) => failure.probe_code(),
            Self::Download => "update-download",
            Self::Busy => "update-busy",
            Self::NotPrepared => "update-not-prepared",
            Self::InstallTask => "update-install-task",
            Self::Install => "update-install",
            Self::Restart => "update-restart",
        }
    }

    pub(crate) fn message(&self) -> &str {
        match self {
            Self::Configuration(message) | Self::DownloadAddress(message) => message,
            Self::State => "Update state is unavailable.",
            Self::Address => "The update address is invalid.",
            Self::Builder => "Updates are unavailable.",
            Self::Check(_) => "Updates could not be checked.",
            Self::Download => "The update could not be downloaded or verified.",
            Self::Busy => "Finish the current action before updating.",
            Self::NotPrepared => "No verified update is ready.",
            Self::InstallTask => "The update could not be installed.",
            Self::Install => "The update could not be installed. Try again.",
            Self::Restart => "The updated app did not restart.",
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum UpdateCheckFailure {
    Network,
    TargetNotFound,
    ManifestParse,
    ReleaseNotFound,
    Version,
    Address,
    Other,
}

impl UpdateCheckFailure {
    fn probe_code(&self) -> &'static str {
        match self {
            Self::Network => "update-check-network",
            Self::TargetNotFound => "update-check-target-not-found",
            Self::ManifestParse => "update-check-manifest-parse",
            Self::ReleaseNotFound => "update-check-release-not-found",
            Self::Version => "update-check-version",
            Self::Address => "update-check-address",
            Self::Other => "update-check-other",
        }
    }
}

impl From<tauri_plugin_updater::Error> for UpdateCheckFailure {
    fn from(error: tauri_plugin_updater::Error) -> Self {
        use tauri_plugin_updater::Error;
        match error {
            // The updater reads JSON through reqwest before it parses the release.
            Error::Reqwest(error) if error.is_decode() => Self::ManifestParse,
            Error::Reqwest(_) | Error::Network(_) => Self::Network,
            Error::TargetNotFound(_) | Error::TargetsNotFound(_) => Self::TargetNotFound,
            Error::Serialization(_) => Self::ManifestParse,
            Error::ReleaseNotFound => Self::ReleaseNotFound,
            Error::Semver(_) => Self::Version,
            Error::UrlParse(_) => Self::Address,
            _ => Self::Other,
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
    let version = prepare(&app, &state).await?;
    // A request the feed did not offer before installs once it does.
    if version.is_some() {
        crate::runtimes::check_saved_request(&app);
    }
    Ok(version)
}

/// Checks the release feed and downloads a newer verified update, answering
/// its version, or `None` when this build takes no in-app update.
pub(crate) async fn prepare<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppUpdate,
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
    let Some(mut update) = updater
        .check()
        .await
        .map_err(|error| UpdateFailure::Check(error.into()))?
    else {
        return Ok(None);
    };
    crate::subscription_probe::check_update_download(&update)
        .map_err(UpdateFailure::DownloadAddress)?;
    update.timeout = Some(Duration::from_secs(120));
    let version = update.version.clone();
    report(app, RuntimeUpdateState::Downloading, Some(&version), None);
    let bytes = match update.download(|_, _| {}, || {}).await {
        Ok(bytes) => bytes,
        Err(_) => {
            let failure = UpdateFailure::Download;
            report(
                app,
                RuntimeUpdateState::Failed,
                Some(&version),
                Some(failure.message()),
            );
            return Err(failure);
        }
    };
    *state.0.lock().map_err(|_| UpdateFailure::State)? = Some((update, bytes));
    // The verified version waits for its install.
    report(app, RuntimeUpdateState::Queued, Some(&version), None);
    Ok(Some(version))
}

/// The owner's install: runs on this computer's runtime stop at once and
/// resume after the update. The window confirms with the count
/// `app_update_interruptions` answers first.
#[tauri::command]
pub async fn app_update_install(
    app: tauri::AppHandle,
    activity: State<'_, muniment_core::attach::RuntimeActivityRegistry>,
    state: State<'_, AppUpdate>,
) -> Result<(), UpdateFailure> {
    install(&app, &activity, &state, InstallMode::Interrupt).await
}

/// How many runs on this computer's runtime an install now would stop.
#[tauri::command]
pub async fn app_update_interruptions(state: State<'_, AppUpdate>) -> Result<u32, String> {
    let version = state.prepared().ok().map(|(update, _)| update.version);
    tauri::async_runtime::spawn_blocking(move || interrupted_runs(version.as_deref()))
        .await
        .map_err(|_| "The runtime did not answer.".to_owned())
}

/// Installs the prepared update and restarts, once the app's own sign-in
/// work is idle and this computer's runtime drained as `mode` directs.
pub(crate) async fn install<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    activity: &muniment_core::attach::RuntimeActivityRegistry,
    state: &AppUpdate,
    mode: InstallMode,
) -> Result<(), UpdateFailure> {
    muniment_core::attach::evaluate_quiesce(activity.snapshot())
        .map_err(|_| UpdateFailure::Busy)?;
    let version = state.prepared()?.0.version;
    let grace = mode.grace();
    tauri::async_runtime::spawn_blocking(move || drain_local_runtime(grace))
        .await
        .map_err(|_| UpdateFailure::InstallTask)?;
    let ready = state
        .0
        .lock()
        .map_err(|_| UpdateFailure::State)?
        .take()
        .ok_or(UpdateFailure::NotPrepared)?;
    report(app, RuntimeUpdateState::Installing, Some(&version), None);
    let result = tauri::async_runtime::spawn_blocking(move || {
        let result = ready.0.install(&ready.1);
        (ready, result)
    })
    .await
    .map_err(|_| UpdateFailure::InstallTask)?;
    let outcome = finish_install(&state.0, result.0, result.1, || {
        report(app, RuntimeUpdateState::Restarting, Some(&version), None);
        app.restart()
    });
    if outcome == Err(UpdateFailure::Install) {
        let message = UpdateFailure::Install.message().to_owned();
        report(
            app,
            RuntimeUpdateState::Failed,
            Some(&version),
            Some(&message),
        );
    }
    outcome
}

/// The remote access directory of this computer's runtime, where it keeps
/// its update request and status.
fn remote_directory() -> Option<PathBuf> {
    muniment_runtime::profile_directory()
        .ok()
        .map(|profile| profile.join("remote"))
}

/// This computer's update status as its runtime reports it: the saved status
/// while an update works or after one failed, else a pending request as
/// queued.
pub(crate) fn local_status() -> Option<RuntimeUpdateStatus> {
    let directory = remote_directory()?;
    match (
        read_update_status(&directory),
        read_update_request(&directory),
    ) {
        (Some(status), _) if status.state != RuntimeUpdateState::Idle => Some(status),
        (_, Some(request)) => Some(RuntimeUpdateStatus {
            state: RuntimeUpdateState::Queued,
            target_version: Some(request.version),
            blockers: Vec::new(),
            error: None,
            updated_at: request.requested_at,
        }),
        (stored, None) => stored,
    }
}

/// Saves one step of this app's update for its runtime to report.
pub(crate) fn report<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: RuntimeUpdateState,
    target: Option<&str>,
    error: Option<&str>,
) {
    let Some(directory) = remote_directory() else {
        return;
    };
    let status = RuntimeUpdateStatus {
        state,
        target_version: target.map(str::to_owned),
        blockers: Vec::new(),
        error: error.map(str::to_owned),
        updated_at: update_timestamp(),
    };
    if write_update_status(&directory, &status).is_err() {
        eprintln!("muniment-desktop: update status not saved");
    }
    crate::runtimes::emit_changed(app);
}

/// The status a start leaves after the last run of the app: an update that
/// reached this version, or one that never started, reads as idle, and one
/// that stopped while it installed reads as failed.
fn settled(status: &RuntimeUpdateStatus, app_version: &str) -> Option<RuntimeUpdateStatus> {
    use RuntimeUpdateState::*;
    let reached = status
        .target_version
        .as_deref()
        .is_none_or(|target| crate::runtimes::is_older_version(app_version, target) != Some(true));
    let state = match status.state {
        Idle => return None,
        Failed if !reached => return None,
        Draining | Installing | Restarting if !reached => Failed,
        _ => Idle,
    };
    Some(RuntimeUpdateStatus {
        state,
        target_version: (state == Failed)
            .then(|| status.target_version.clone())
            .flatten(),
        blockers: Vec::new(),
        error: (state == Failed).then(|| {
            format!(
                "Version {} did not start, so the app runs {app_version} again.",
                status.target_version.as_deref().unwrap_or_default()
            )
        }),
        updated_at: update_timestamp(),
    })
}

/// Settles the saved status after the app starts, once per start.
pub(crate) fn settle_status_after_start(app_version: &str) {
    let Some(directory) = remote_directory() else {
        return;
    };
    if let Some(status) =
        read_update_status(&directory).and_then(|status| settled(&status, app_version))
    {
        if write_update_status(&directory, &status).is_err() {
            eprintln!("muniment-desktop: update status not saved");
        }
    }
}

/// The `runtime.info` answer of this computer's runtime, or `None` when it
/// does not answer.
fn local_runtime_info() -> Option<Value> {
    crate::model_service::connect()
        .ok()?
        .read_runtime_info()
        .ok()
}

/// How many runs the blockers name. The runtime runs one reply at a time,
/// and a permission request belongs to that reply.
fn runs_in(blockers: &[Value]) -> u32 {
    u32::from(
        blockers
            .iter()
            .any(|blocker| blocker == "active_run" || blocker == "pending_permission_gate"),
    )
}

/// How many runs an install now would stop. The runtime keeps its blockers
/// current while an update waits, so the app queues the prepared version
/// first when the status says nothing waits.
fn interrupted_runs(version: Option<&str>) -> u32 {
    let waiting = |info: &Value| {
        matches!(
            info["update_status"]["state"].as_str(),
            Some("queued" | "downloading")
        )
    };
    let Some(mut info) = local_runtime_info() else {
        return 0;
    };
    if !waiting(&info) {
        if let (Some(directory), Some(version)) = (remote_directory(), version) {
            let queued = RuntimeUpdateStatus {
                state: RuntimeUpdateState::Queued,
                target_version: Some(version.to_owned()),
                blockers: Vec::new(),
                error: None,
                updated_at: update_timestamp(),
            };
            if write_update_status(&directory, &queued).is_ok() {
                std::thread::sleep(BLOCKERS_SETTLE);
                info = local_runtime_info().unwrap_or(info);
            }
        }
    }
    info["update_status"]["blockers"]
        .as_array()
        .map_or(0, |blockers| runs_in(blockers))
}

/// Asks this computer's runtime to drain and waits for its answer. The
/// runtime waits up to `grace` for its runs, records the ones it stops for a
/// resume after the restart, and reports `installing`. A runtime that does
/// not answer has no runs to keep, so the install goes on.
fn drain_local_runtime(grace: Duration) {
    let Some(directory) = remote_directory() else {
        return;
    };
    if local_runtime_info().is_none() {
        return;
    }
    let request = directory.join(DRAIN_REQUEST_FILE);
    let body = serde_json::json!({"grace_seconds": grace.as_secs()}).to_string();
    if std::fs::write(&request, body).is_err() {
        return;
    }
    let deadline = Instant::now() + grace + DRAIN_ANSWER;
    while Instant::now() < deadline {
        std::thread::sleep(DRAIN_POLL);
        if !request.exists()
            && local_runtime_info()
                .is_some_and(|info| info["update_status"]["state"] == "installing")
        {
            return;
        }
    }
    let _ = std::fs::remove_file(&request);
    eprintln!("muniment-desktop: the runtime did not answer the update drain");
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
    use super::{finish_install, runs_in, settled, InstallMode, UpdateCheckFailure, UpdateFailure};
    use muniment_core::attach::{RuntimeUpdateState, RuntimeUpdateStatus};
    use serde_json::json;
    use std::sync::Mutex;
    use std::time::Duration;

    #[test]
    fn the_install_confirm_counts_the_reply_the_runtime_runs() {
        assert_eq!(runs_in(&[]), 0);
        assert_eq!(runs_in(&[json!("session_refresh")]), 0);
        assert_eq!(runs_in(&[json!("active_run")]), 1);
        // A permission request belongs to the one reply in progress.
        assert_eq!(
            runs_in(&[json!("active_run"), json!("pending_permission_gate")]),
            1
        );
        assert_eq!(runs_in(&[json!("pending_permission_gate")]), 1);
    }

    #[test]
    fn an_owner_install_stops_runs_at_once_and_a_requested_one_waits_two_minutes() {
        assert_eq!(InstallMode::Interrupt.grace(), Duration::ZERO);
        assert_eq!(InstallMode::Wait.grace(), Duration::from_secs(120));
    }

    #[test]
    fn a_start_settles_the_status_the_last_run_left() {
        let status = |state, target: Option<&str>| RuntimeUpdateStatus {
            state,
            target_version: target.map(str::to_owned),
            blockers: Vec::new(),
            error: None,
            updated_at: "2030-01-01T00:00:00Z".into(),
        };
        use RuntimeUpdateState::*;
        // The update reached this version.
        for state in [Queued, Downloading, Draining, Installing, Restarting] {
            assert_eq!(
                settled(&status(state, Some("0.4.0")), "0.4.0")
                    .unwrap()
                    .state,
                Idle
            );
        }
        // The install stopped before the new version ran.
        let failed = settled(&status(Restarting, Some("0.4.0")), "0.3.0").unwrap();
        assert_eq!(failed.state, Failed);
        assert_eq!(failed.target_version.as_deref(), Some("0.4.0"));
        assert_eq!(
            failed.error.as_deref(),
            Some("Version 0.4.0 did not start, so the app runs 0.3.0 again.")
        );
        // A download the restart dropped waits for the next check.
        assert_eq!(
            settled(&status(Queued, Some("0.4.0")), "0.3.0")
                .unwrap()
                .state,
            Idle
        );
        assert_eq!(settled(&status(Idle, None), "0.3.0"), None);
        assert_eq!(settled(&status(Failed, Some("0.4.0")), "0.3.0"), None);
        assert_eq!(
            settled(&status(Failed, Some("0.4.0")), "0.4.0")
                .unwrap()
                .state,
            Idle
        );
    }

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
                UpdateFailure::Check(UpdateCheckFailure::Other),
                "update-check-other",
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
    fn update_check_errors_drop_private_details() {
        use tauri_plugin_updater::Error;
        for (error, code) in [
            (Error::Network("PRIVATE".into()), "update-check-network"),
            (
                Error::TargetNotFound("PRIVATE".into()),
                "update-check-target-not-found",
            ),
            (
                Error::TargetsNotFound(vec!["PRIVATE".into()]),
                "update-check-target-not-found",
            ),
            (
                Error::Serialization(
                    serde_json::from_str::<serde_json::Value>("PRIVATE").unwrap_err(),
                ),
                "update-check-manifest-parse",
            ),
            (Error::ReleaseNotFound, "update-check-release-not-found"),
            (
                Error::Semver(semver::Version::parse("PRIVATE").unwrap_err()),
                "update-check-version",
            ),
            (
                Error::UrlParse(url::Url::parse("PRIVATE").unwrap_err()),
                "update-check-address",
            ),
            (
                Error::Io(std::io::Error::other("PRIVATE")),
                "update-check-other",
            ),
        ] {
            let failure = UpdateFailure::Check(error.into());
            assert_eq!(failure.probe_code(), code);
            assert_eq!(
                serde_json::to_value(&failure).unwrap(),
                "Updates could not be checked."
            );
            assert!(!format!("{failure:?}").contains("PRIVATE"));
        }
    }

    #[test]
    fn updater_request_errors_keep_only_fixed_codes() {
        use super::UpdaterExt;
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context.config_mut().plugins.0.insert(
            "updater".into(),
            serde_json::json!({"pubkey": "", "requireSignedVersion": true}),
        );
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .unwrap();
        for (tls, code) in [
            (false, "update-check-manifest-parse"),
            (true, "update-check-network"),
        ] {
            let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
            let address = server.server_addr();
            let mut responder = Some(std::thread::spawn(move || {
                if !tls {
                    server
                        .recv_timeout(std::time::Duration::from_secs(5))
                        .unwrap()
                        .unwrap()
                        .respond(tiny_http::Response::from_string("PRIVATE"))
                        .unwrap();
                }
            }));
            // Close the TLS listener before the request. No remote host receives traffic.
            if tls {
                responder.take().unwrap().join().unwrap();
            }
            let scheme = if tls { "https" } else { "http" };
            let updater = app
                .updater_builder()
                .endpoints(vec![format!("{scheme}://{address}/PRIVATE")
                    .parse()
                    .unwrap()])
                .unwrap()
                .no_proxy()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap();
            let error = tauri::async_runtime::block_on(updater.check())
                .err()
                .unwrap();
            if let Some(responder) = responder {
                responder.join().unwrap();
            }
            assert!(matches!(error, tauri_plugin_updater::Error::Reqwest(_)));
            let failure = UpdateFailure::Check(error.into());
            assert_eq!(failure.probe_code(), code);
            assert_eq!(
                serde_json::to_value(&failure).unwrap(),
                "Updates could not be checked."
            );
            assert!(!format!("{failure:?}").contains("PRIVATE"));
        }
    }

    #[test]
    fn windows_target_checks_and_downloads_over_loopback_https() {
        use std::io::BufRead;
        use std::process::{Command, Stdio};
        use tauri_plugin_updater::{Error, UpdaterExt};

        struct Fixture(std::process::Child);
        impl Drop for Fixture {
            fn drop(&mut self) {
                drop(self.0.stdin.take());
                let _ = self.0.wait();
            }
        }
        let mut fixture = Fixture(
            Command::new("node")
                .arg(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../test/e2e/support/subscription-update-fixture.mjs"
                ))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let mut line = String::new();
        std::io::BufReader::new(fixture.0.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let receipt: serde_json::Value = serde_json::from_str(&line).unwrap();
        let endpoint: url::Url = receipt["url"].as_str().unwrap().parse().unwrap();
        assert_eq!(endpoint.scheme(), "https");
        assert_eq!(endpoint.host_str(), Some("127.0.0.1"));
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context.config_mut().plugins.0.insert(
            "updater".into(),
            serde_json::json!({"pubkey": receipt["pubkey"], "requireSignedVersion": true}),
        );
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .unwrap();
        let builder = || {
            app.updater_builder()
                .endpoints(vec![endpoint.clone()])
                .unwrap()
                .no_proxy()
                .timeout(std::time::Duration::from_secs(5))
                .configure_client(|client| {
                    client.danger_accept_invalid_certs(true).https_only(true)
                })
        };
        tauri::async_runtime::block_on(async {
            // An HKLM uninstall entry must not select the machine feed for a per-user MSI.
            let wrong = builder()
                .target("windows-x86_64-msi-machine")
                .build()
                .unwrap()
                .check()
                .await;
            assert!(matches!(wrong, Err(Error::TargetNotFound(_))));
            let mut update = builder()
                .target("windows-x86_64-msi-user")
                .build()
                .unwrap()
                .check()
                .await
                .unwrap()
                .unwrap();
            assert_eq!(update.download_url, endpoint.join("/package").unwrap());
            update.timeout = Some(std::time::Duration::from_secs(5));
            assert_eq!(
                update.download(|_, _| {}, || {}).await.unwrap(),
                b"signed update test package"
            );
            update.download_url = endpoint.join("/tampered").unwrap();
            assert!(matches!(
                update.download(|_, _| {}, || {}).await,
                Err(Error::Minisign(_))
            ));
            update.download_url = endpoint.join("/package").unwrap();
            update.version = "999999.0.0".into();
            assert!(matches!(
                update.download(|_, _| {}, || {}).await,
                Err(Error::SignedVersionMismatch { .. })
            ));
        });
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
