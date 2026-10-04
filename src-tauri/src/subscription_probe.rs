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
    command_failure: Option<muniment_core::model_router::subscription_probe::CommandFailure>,
) -> Result<(), &'static str> {
    let outcome = match stage.as_str() {
        "reply" => "pending",
        "render" | "complete" => "complete",
        _ => "not-started",
    };
    muniment_core::model_router::subscription_probe::record_command(
        &root()?,
        &stage,
        turn,
        outcome,
        &error_class,
        command_failure.as_ref(),
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
    write_observed(&root, turns, passed, features, &update_plan()?)
}

fn write_observed(
    root: &std::path::Path,
    turns: Vec<Turn>,
    passed: bool,
    mut features: std::collections::BTreeMap<String, Vec<String>>,
    plan: &serde_json::Value,
) -> Result<(), &'static str> {
    if turns.len() > 4 {
        return Err("The subscription probe returned too many turns.");
    }
    let invalid_features = features.len() > 20
        || features.iter().any(|(name, checks)| {
            name.len() > 40
                || checks.len() > 10
                || std::iter::once(name)
                    .chain(checks.iter())
                    .enumerate()
                    .any(|(index, value)| {
                        // Only the terminal failure's byte count accepts digits.
                        let byte_count = name == "terminal"
                            && checks.len() == 7
                            && checks[0] == "failed"
                            && index == 4
                            && value.strip_prefix("bytes-").is_some_and(|count| {
                                !count.is_empty()
                                    && (count == "0" || !count.starts_with('0'))
                                    && count.bytes().all(|byte| byte.is_ascii_digit())
                                    && count
                                        .parse::<u64>()
                                        .is_ok_and(|count| count <= 9_007_199_254_740_991)
                            });
                        value.is_empty()
                            || value.len() > 64
                            || (!byte_count
                                && !value.bytes().all(|byte| {
                                    byte.is_ascii_lowercase() || matches!(byte, b'-' | b'_')
                                }))
                    })
        });
    // Publish a bounded failure instead of making the runner wait for a missing result.
    // Do not copy invalid feature data into the evidence.
    if invalid_features {
        features.clear();
    }
    let mut result = serde_json::json!({
        "pid": std::process::id(),
        "update_parent_pid": plan["updateParentPid"],
        "phase": plan["phase"],
        "passed": passed && !invalid_features,
        "turns": turns,
        "features": features,
        "source_sha": env!("MUNIMENT_BUILD_SOURCE_SHA"),
        "webdriver": cfg!(feature = "e2e-webdriver"),
    });
    if invalid_features {
        result["error_class"] = "invalid-feature-checks".into();
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    struct Profile(PathBuf);

    impl Profile {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("subscription-probe-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }

        fn result(&self) -> serde_json::Value {
            serde_json::from_slice(
                &std::fs::read(self.0.join("subscription-probe-result.json")).unwrap(),
            )
            .unwrap()
        }
    }

    impl Drop for Profile {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn feature(name: &str, checks: &[&str]) -> BTreeMap<String, Vec<String>> {
        BTreeMap::from([(
            name.into(),
            checks.iter().map(|value| (*value).into()).collect(),
        )])
    }

    #[test]
    fn observed_preserves_command_reasons_and_result_identity() {
        for reason in [
            "model_router_settings",
            "model_router_save_routes",
            "model_router_test_route",
            "model_router_update_account",
            "memory_profile_read",
            "memory_profile_save",
            "terminal_start",
            "terminal_write",
            "terminal_read",
            "terminal_close",
            "chat_thread_open",
        ] {
            for passed in [true, false] {
                let profile = Profile::new();
                let features = feature("routing", &["failed", "check", reason]);
                let plan = serde_json::json!({ "phase": "features", "updateParentPid": 123 });
                write_observed(&profile.0, vec![], passed, features.clone(), &plan).unwrap();
                let result = profile.result();
                assert_eq!(result["features"], serde_json::json!(features));
                assert_eq!(result["passed"], passed);
                assert!(result.get("error_class").is_none());
                assert_eq!(result["phase"], "features");
                assert_eq!(result["update_parent_pid"], 123);
                assert_eq!(result["source_sha"], env!("MUNIMENT_BUILD_SOURCE_SHA"));
                assert_eq!(result["webdriver"], cfg!(feature = "e2e-webdriver"));
                assert!(
                    write_observed(&profile.0, vec![], !passed, BTreeMap::new(), &plan).is_err()
                );
                assert_eq!(profile.result(), result);
                assert!(!profile.0.join("subscription-probe-result.tmp").exists());
            }
        }
    }

    #[test]
    fn observed_preserves_bounded_terminal_diagnostics() {
        for count in ["0", "4096", "9007199254740991"] {
            let profile = Profile::new();
            let features = feature(
                "terminal",
                &[
                    "failed",
                    "check",
                    "shell-output",
                    &format!("bytes-{count}"),
                    if count == "0" {
                        "received-false"
                    } else {
                        "received-true"
                    },
                    "nonce-false",
                    "vt-false",
                ],
            );
            write_observed(
                &profile.0,
                vec![],
                true,
                features.clone(),
                &serde_json::json!({}),
            )
            .unwrap();
            assert_eq!(profile.result()["features"], serde_json::json!(features));
            assert!(profile.result().get("error_class").is_none());
        }
    }

    #[test]
    fn observed_rejects_numeric_data_outside_the_terminal_byte_count() {
        let mut invalid = vec![];
        for count in [
            "-1",
            "01",
            "1.5",
            "1e3",
            "9007199254740992",
            "18446744073709551616",
            "0\n",
        ] {
            invalid.push(feature(
                "terminal",
                &[
                    "failed",
                    "check",
                    "shell-output",
                    &format!("bytes-{count}"),
                    "received-true",
                    "nonce-false",
                    "vt-false",
                ],
            ));
        }
        for name in ["terminal", "files"] {
            invalid.push(feature(name, &["failed", "check", "bytes-10"]));
        }
        invalid.push(feature(
            "files",
            &[
                "failed",
                "check",
                "shell-output",
                "bytes-10",
                "received-true",
                "nonce-false",
                "vt-false",
            ],
        ));
        invalid.push(feature(
            "terminal",
            &[
                "failed",
                "check",
                "shell-output",
                "bytes-10",
                "received-1",
                "nonce-false",
                "vt-false",
            ],
        ));
        for features in invalid {
            let profile = Profile::new();
            write_observed(&profile.0, vec![], true, features, &serde_json::json!({})).unwrap();
            assert_eq!(profile.result()["error_class"], "invalid-feature-checks");
            assert_eq!(profile.result()["features"], serde_json::json!({}));
        }
    }

    #[test]
    fn observed_publishes_invalid_features_as_a_bounded_failure() {
        let mut invalid = vec![
            feature("", &["check"]),
            feature(&"a".repeat(41), &["check"]),
            feature("routing", &[""]),
            feature("routing", &[&"a".repeat(65)]),
            feature("routing", &["check"; 11]),
            (1..=21)
                .map(|length| ("a".repeat(length), vec![]))
                .collect(),
        ];
        for value in [
            "PRIVATE",
            "two words",
            "line\nbreak",
            "é",
            "reason1",
            "path/to/file",
            "bad.reason",
        ] {
            invalid.push(feature(value, &["check"]));
            invalid.push(feature("routing", &[value]));
        }
        for features in invalid {
            for passed in [true, false] {
                let profile = Profile::new();
                write_observed(
                    &profile.0,
                    vec![],
                    passed,
                    features.clone(),
                    &serde_json::json!({}),
                )
                .unwrap();
                let result = profile.result();
                assert_eq!(result["passed"], false);
                assert_eq!(result["error_class"], "invalid-feature-checks");
                assert_eq!(result["features"], serde_json::json!({}));
                assert!(!profile.0.join("subscription-probe-result.tmp").exists());
            }
        }
    }

    #[test]
    fn observed_keeps_empty_states_and_exact_bounds() {
        for features in [
            BTreeMap::new(),
            feature("routing", &[]),
            feature(&"a".repeat(40), &["a".repeat(64).as_str(); 10]),
            (1..=20)
                .map(|length| ("a".repeat(length), vec![]))
                .collect(),
        ] {
            let profile = Profile::new();
            let turns = (0..4)
                .map(|index| Turn {
                    index,
                    thread: uuid::Uuid::now_v7(),
                    run: uuid::Uuid::now_v7(),
                    rendered: true,
                    context: true,
                })
                .collect();
            write_observed(
                &profile.0,
                turns,
                true,
                features.clone(),
                &serde_json::json!({}),
            )
            .unwrap();
            let result = profile.result();
            assert_eq!(result["passed"], true);
            assert_eq!(result["features"], serde_json::json!(features));
            assert_eq!(result["turns"].as_array().unwrap().len(), 4);
        }
    }
}
