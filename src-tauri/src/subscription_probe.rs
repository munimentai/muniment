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

// This diagnostic never changes the installed app or the production update feed.
#[tauri::command]
pub(crate) async fn subscription_probe_update(app: tauri::AppHandle) -> Result<(), &'static str> {
    use sha2::{Digest, Sha256};
    use tauri_plugin_updater::{Error, UpdaterExt};
    let root = root()?;
    let plan: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("subscription-probe.json"))
            .map_err(|_| "The update plan is missing.")?,
    )
    .map_err(|_| "The update plan is invalid.")?;
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
        return Err("The update probe requires a loopback fixture.");
    }
    let updater = app
        .updater_builder()
        .pubkey(include_str!("../updater.pub").trim())
        .version_comparator(|current, release| release.version == current)
        .no_proxy()
        // Only this opt-in loopback fixture uses a disposable TLS certificate.
        // The compiled release key still verifies every downloaded package.
        .configure_client(|client| client.danger_accept_invalid_certs(true).https_only(true))
        .endpoints(vec![endpoint.clone()])
        .map_err(|_| "The update endpoint is invalid.")?
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|_| "The installed updater could not start.")?;
    let mut update = updater
        .check()
        .await
        .map_err(|_| "The installed updater could not read the fixture.")?
        .ok_or("The fixture does not match the installed version.")?;
    if update.download_url
        != endpoint
            .join("/package")
            .map_err(|_| "The update address is invalid.")?
    {
        return Err("The update package must stay on loopback.");
    }
    update.timeout = Some(std::time::Duration::from_secs(120));
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|_| "The installed updater rejected the signed package.")?;
    if plan["packageSha256"].as_str() != Some(format!("{:x}", Sha256::digest(&bytes)).as_str()) {
        return Err("The updater downloaded a different package.");
    }
    update.download_url = endpoint
        .join("/tampered")
        .map_err(|_| "The update address is invalid.")?;
    if !matches!(
        update.download(|_, _| {}, || {}).await,
        Err(Error::Minisign(_))
    ) {
        return Err("The installed updater did not reject the damaged package.");
    }
    update.download_url = endpoint
        .join("/package")
        .map_err(|_| "The update address is invalid.")?;
    update.version = "999999.0.0".into();
    if !matches!(
        update.download(|_, _| {}, || {}).await,
        Err(Error::SignedVersionMismatch { .. })
    ) {
        return Err("The installed updater did not reject the wrong version.");
    }
    Ok(())
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
    let result = serde_json::json!({
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
