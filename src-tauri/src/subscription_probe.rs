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
    let script = include_str!("../../test/e2e/support/subscription-probe.js");
    let _ = webview.eval(&format!(
        "window.__MUNIMENT_SUBSCRIPTION_PLAN__ = {plan};\n{script}"
    ));
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
) -> Result<(), &'static str> {
    let root = root()?;
    if turns.len() > 4 {
        return Err("The subscription probe returned too many turns.");
    }
    let result = serde_json::json!({
        "passed": passed,
        "turns": turns,
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
