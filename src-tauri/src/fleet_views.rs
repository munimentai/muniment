//! Usage and notifications across machines. Each machine's runtime answers
//! for itself through core's `usage.summary` and `notifications.list`, and
//! core's merge joins the answers into one page. This module only asks the
//! machines the window names, at the same time, and hands core the answers.

use muniment_core::chrono::Utc;
use muniment_core::{notifications, usage_report};
use serde::Deserialize;
use serde_json::Value;

/// One machine the window shows: its runtime id and its name.
#[derive(Clone, Debug, Deserialize)]
pub struct MachineRef {
    id: String,
    name: String,
}

/// Asks every machine `operation` with `body` at once and answers each
/// machine's reply or why it gave none, in the order given.
fn ask_all(
    app: &tauri::AppHandle,
    machines: &[MachineRef],
    operation: &str,
    body: &Value,
) -> Vec<Result<Value, String>> {
    std::thread::scope(|scope| {
        let asks: Vec<_> = machines
            .iter()
            .map(|machine| {
                scope.spawn(move || {
                    crate::agents::request_runtime(
                        app,
                        Some(machine.id.as_str()),
                        operation,
                        body.clone(),
                    )
                })
            })
            .collect();
        asks.into_iter()
            .map(|ask| {
                ask.join()
                    .unwrap_or_else(|_| Err(crate::auth::background_service_error()))
            })
            .collect()
    })
}

/// Usage on the named machines, joined by core.
#[tauri::command]
pub async fn usage_summary(
    app: tauri::AppHandle,
    machines: Vec<MachineRef>,
    body: Value,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let answers = ask_all(&app, &machines, "usage.summary", &body);
        let answers = machines
            .into_iter()
            .zip(answers)
            .map(|(machine, answer)| usage_report::MachineAnswer {
                id: machine.id,
                name: machine.name,
                summary: answer.and_then(|value| {
                    serde_json::from_value(value)
                        .map_err(|_| "The machine answered usage in a form this app cannot read.".into())
                }),
            })
            .collect();
        serde_json::to_value(usage_report::merge(answers, Utc::now().timestamp_millis()))
            .map_err(|_| "Usage could not be shown.".to_string())
    })
    .await
    .map_err(|_| crate::auth::background_service_error())?
}

/// The notifications page for the named machines, joined by core.
#[tauri::command]
pub async fn notifications_list(
    app: tauri::AppHandle,
    machines: Vec<MachineRef>,
    filter: Value,
) -> Result<Value, String> {
    let filter: notifications::Filter =
        serde_json::from_value(filter).map_err(|_| "The notification filter is invalid.")?;
    tauri::async_runtime::spawn_blocking(move || {
        let body = serde_json::json!({});
        let answers = ask_all(&app, &machines, "notifications.list", &body);
        let answers = machines
            .into_iter()
            .zip(answers)
            .map(|(machine, answer)| notifications::MachineAnswer {
                machine: notifications::Machine {
                    id: machine.id,
                    name: machine.name,
                },
                listing: answer.and_then(|value| {
                    serde_json::from_value(value).map_err(|_| {
                        "The machine answered notifications in a form this app cannot read.".into()
                    })
                }),
            })
            .collect();
        serde_json::to_value(notifications::merge(
            answers,
            &filter,
            Utc::now().timestamp_millis(),
        ))
        .map_err(|_| "Notifications could not be shown.".to_string())
    })
    .await
    .map_err(|_| crate::auth::background_service_error())?
}
