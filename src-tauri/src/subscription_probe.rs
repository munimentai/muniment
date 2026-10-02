//! An opt-in diagnostic for the unmodified installed release candidate.
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

fn root() -> Result<PathBuf, &'static str> {
    if !std::env::args_os().any(|arg| arg == "--probe-subscription-chat")
        || std::env::var("MUNIMENT_SUBSCRIPTION_PROBE").as_deref() != Ok("1")
    {
        return Err("The subscription probe is inactive.");
    }
    std::env::var_os("MUNIMENT_STATE_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && path.join("subscription-probe.json").is_file())
        .ok_or("The subscription probe requires a disposable profile.")
}

pub(crate) fn install<R: tauri::Runtime>(webview: &tauri::Webview<R>) {
    if webview.label() != "main" {
        return;
    }
    let Ok(root) = root() else { return };
    let Ok(plan) = std::fs::read(root.join("subscription-probe.json")) else {
        return;
    };
    let Ok(plan) = serde_json::from_slice::<serde_json::Value>(&plan) else {
        return;
    };
    let features = include_str!("../../test/e2e/support/subscription-features.js");
    let script = include_str!("../../test/e2e/support/subscription-probe.js");
    let _ = webview.eval(&format!(
        "window.__MUNIMENT_SUBSCRIPTION_PLAN__ = {plan};\n{features}\n{script}"
    ));
}

// MSI relaunches the app outside the runner's process environment.
// The explicit probe argument restores the disposable state directory and its process job.
pub(crate) fn restore_profile() {
    if !std::env::args_os().any(|arg| arg == "--probe-subscription-chat") {
        return;
    }
    for arg in std::env::args() {
        if let Some(directory) = arg.strip_prefix("--probe-subscription-profile=") {
            let path = PathBuf::from(directory);
            #[cfg(windows)]
            if !path.is_absolute() || !path.join("subscription-probe.json").is_file() {
                eprintln!("The subscription probe requires its disposable profile.");
                std::process::exit(1);
            }
            if path.is_absolute() && path.join("subscription-probe.json").is_file() {
                #[cfg(windows)]
                if join_probe_job(&path).is_err() {
                    eprintln!("The subscription probe could not join its process job.");
                    std::process::exit(1);
                }
                std::env::set_var("MUNIMENT_STATE_DIR", &path);
                std::env::set_var("PI_CODING_AGENT_DIR", path.join("agent"));
                std::env::set_var("MUNIMENT_SUBSCRIPTION_PROBE", "1");
            }
        }
    }
}

// Join before CEF or the runtime can spawn descendants, including after an MSI restart.
#[cfg(windows)]
fn join_probe_job(root: &std::path::Path) -> Result<(), ()> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{
            JobObjects::{AssignProcessToJobObject, IsProcessInJob, OpenJobObjectW},
            SystemServices::{JOB_OBJECT_ASSIGN_PROCESS, JOB_OBJECT_QUERY},
            Threading::GetCurrentProcess,
        },
    };
    let bytes = std::fs::read(root.join("subscription-probe-job.json")).map_err(|_| ())?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| ())?;
    let name = value["name"].as_str().ok_or(())?;
    let id = name
        .strip_prefix(r"Local\MunimentSubscription-")
        .ok_or(())?;
    if id.len() != 36 || uuid::Uuid::parse_str(id).is_err() {
        return Err(());
    }
    let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    // The supervisor owns the job lifetime. This handle only admits the current process.
    unsafe {
        let job = OpenJobObjectW(
            JOB_OBJECT_ASSIGN_PROCESS | JOB_OBJECT_QUERY,
            0,
            name.as_ptr(),
        );
        if job.is_null() {
            return Err(());
        }
        let process = GetCurrentProcess();
        let mut member = 0;
        let joined = IsProcessInJob(process, job, &mut member) != 0
            && (member != 0 || AssignProcessToJobObject(job, process) != 0);
        CloseHandle(job);
        if joined {
            Ok(())
        } else {
            Err(())
        }
    }
}

fn update_plan() -> Result<serde_json::Value, &'static str> {
    serde_json::from_slice(
        &std::fs::read(root()?.join("subscription-probe.json"))
            .map_err(|_| "The update plan is missing.")?,
    )
    .map_err(|_| "The update plan is invalid.")
}

// Change only the feed and same-version policy. Production code selects the
// Windows installer, verifies the signature, installs the package, and restarts.
pub(crate) fn update_builder(
    builder: tauri_plugin_updater::UpdaterBuilder,
) -> Result<tauri_plugin_updater::UpdaterBuilder, String> {
    if root().is_err() {
        return Ok(builder);
    }
    let plan = update_plan()?;
    let endpoint: url::Url = plan["updateUrl"]
        .as_str()
        .ok_or("The update address is missing.")?
        .parse()
        .map_err(|_| "The update address is invalid.")?;
    if endpoint.scheme() != "https"
        || endpoint.host_str() != Some("127.0.0.1")
        || endpoint.port().is_none()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.path() != "/manifest"
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
    {
        return Err("The update probe requires a loopback fixture.".into());
    }
    let builder = builder
        .version_comparator(|current, release| release.version == current)
        .no_proxy()
        // Only this opt-in loopback fixture uses a disposable TLS certificate.
        // The compiled release key still verifies every downloaded package.
        .configure_client(|client| client.danger_accept_invalid_certs(true).https_only(true))
        .endpoints(vec![endpoint])
        .map_err(|_| "The update endpoint is invalid.")?;
    // Reinstall the exact candidate rather than inventing a signed version.
    #[cfg(windows)]
    let builder = builder.installer_args(["REINSTALL=ALL", "REINSTALLMODE=vomus"]);
    Ok(builder)
}

pub(crate) fn check_update_download(update: &tauri_plugin_updater::Update) -> Result<(), String> {
    if root().is_err() {
        return Ok(());
    }
    let plan = update_plan()?;
    let endpoint: url::Url = plan["updateUrl"]
        .as_str()
        .ok_or("The update address is missing.")?
        .parse()
        .map_err(|_| "The update address is invalid.")?;
    if update.download_url
        != endpoint
            .join("/package")
            .map_err(|_| "The update address is invalid.")?
    {
        return Err("The update package must stay on loopback.".into());
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn subscription_probe_update(app: tauri::AppHandle) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    use tauri::Manager;
    use tauri_plugin_updater::Error;
    let root = root().map_err(|_| "update-profile")?;
    let mut plan = update_plan().map_err(|_| "update-plan")?;
    if plan["phase"] != "update" {
        return Err("update-phase".into());
    }
    let endpoint: url::Url = plan["updateUrl"]
        .as_str()
        .ok_or("update-address")?
        .parse()
        .map_err(|_| "update-address")?;
    let state = app.state::<crate::app_update::AppUpdate>();
    crate::app_update::app_update_prepare(app.clone(), state.clone())
        .await
        .map_err(|error| error.probe_code())?
        .ok_or("update-unavailable")?;
    let (mut update, bytes) = state.prepared().map_err(|error| error.probe_code())?;
    if plan["packageSha256"].as_str() != Some(format!("{:x}", Sha256::digest(&bytes)).as_str()) {
        return Err("update-package-digest".into());
    }
    update.download_url = endpoint.join("/tampered").map_err(|_| "update-address")?;
    if !matches!(
        update.download(|_, _| {}, || {}).await,
        Err(Error::Minisign(_))
    ) {
        return Err("update-tamper-rejection".into());
    }
    update.download_url = endpoint.join("/package").map_err(|_| "update-address")?;
    update.version = "999999.0.0".into();
    if !matches!(
        update.download(|_, _| {}, || {}).await,
        Err(Error::SignedVersionMismatch { .. })
    ) {
        return Err("update-version-rejection".into());
    }
    let activity = app.state::<muniment_core::attach::RuntimeActivityRegistry>();
    let busy = activity.mark_active_run();
    let refused =
        crate::app_update::app_update_install(app.clone(), activity.clone(), state.clone()).await;
    drop(busy);
    if refused != Err(crate::app_update::UpdateFailure::Busy) {
        return Err("update-active-work-refusal".into());
    }
    plan["phase"] = "update-restart".into();
    plan["updateParentPid"] = std::process::id().into();
    std::fs::write(
        root.join("subscription-probe.json"),
        serde_json::to_vec(&plan).map_err(|_| "update-checkpoint-encode")?,
    )
    .map_err(|_| "update-checkpoint-write")?;
    crate::app_update::app_update_install(app.clone(), activity, state)
        .await
        .map_err(|error| error.probe_code().into())
}

pub(crate) fn model_save(provider: &str, model: &str, outcome: &str, os_error: Option<i32>) {
    let Ok(root) = root() else { return };
    let _ = muniment_core::model_router::subscription_probe::record_model_save(
        &root, provider, model, outcome, os_error,
    );
}

#[tauri::command]
pub(crate) fn subscription_probe_progress(
    stage: String,
    turn: Option<usize>,
    error_class: String,
) -> Result<(), &'static str> {
    let outcome = match stage.as_str() {
        "reply" => "pending",
        "render" | "complete" => "complete",
        _ => "not-started",
    };
    muniment_core::model_router::subscription_probe::record(
        &root()?,
        &stage,
        turn,
        outcome,
        &error_class,
    )
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Turn {
    index: u8,
    thread: uuid::Uuid,
    run: uuid::Uuid,
    rendered: bool,
    context: bool,
}

#[tauri::command]
pub(crate) fn subscription_probe_observed(
    turns: Vec<Turn>,
    passed: bool,
    features: std::collections::BTreeMap<String, Vec<String>>,
) -> Result<(), &'static str> {
    let root = root()?;
    if turns.len() > 4 {
        return Err("The subscription probe returned too many turns.");
    }
    if features.len() > 20
        || features.iter().any(|(name, checks)| {
            name.len() > 40
                || checks.len() > 10
                || std::iter::once(name).chain(checks.iter()).any(|value| {
                    value.is_empty()
                        || value.len() > 64
                        || !value
                            .bytes()
                            .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
                })
        })
    {
        return Err("The subscription probe returned invalid feature checks.");
    }
    let plan = update_plan()?;
    let result = serde_json::json!({
        "pid": std::process::id(),
        "update_parent_pid": plan["updateParentPid"],
        "phase": plan["phase"],
        "passed": passed,
        "turns": turns,
        "features": features,
        "source_sha": env!("MUNIMENT_BUILD_SOURCE_SHA"),
        "webdriver": cfg!(feature = "e2e-webdriver"),
    });
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let destination = root.join("subscription-probe-result.json");
    if destination.exists() {
        return Err("The subscription probe already has a result.");
    }
    let temporary = root.join("subscription-probe-result.tmp");
    let file = options
        .open(&temporary)
        .map_err(|_| "The subscription probe could not create its result.")?;
    serde_json::to_writer(&file, &result)
        .map_err(|_| "The subscription probe could not save its result.")?;
    file.sync_all()
        .map_err(|_| "The subscription probe could not save its result.")?;
    drop(file);
    std::fs::rename(temporary, destination)
        .map_err(|_| "The subscription probe could not publish its result.")
}
