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

/// The field a runtime before it refuses, as it refuses every field it
/// does not know.
const ZONE_FIELD: &str = "time_zone";
const OFFLINE: &str = "That machine is offline.";

/// Asks again, without the time zone, each machine that refused a body that
/// holds one, so an older runtime still answers at its fixed offset. A
/// machine that is offline is not asked again.
fn without_zone_again(
    body: &Value,
    answers: Vec<Result<Value, String>>,
    ask: impl Fn(usize, &Value) -> Result<Value, String>,
) -> Vec<Result<Value, String>> {
    let Some(plain) = body.as_object().filter(|body| body.contains_key(ZONE_FIELD)).map(|body| {
        let mut plain = body.clone();
        plain.remove(ZONE_FIELD);
        Value::Object(plain)
    }) else {
        return answers;
    };
    answers
        .into_iter()
        .enumerate()
        .map(|(index, answer)| match answer {
            Err(error) if !error.starts_with(OFFLINE) => ask(index, &plain).or(Err(error)),
            answer => answer,
        })
        .collect()
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
        let answers = without_zone_again(&body, answers, |index, body| {
            crate::agents::request_runtime(
                &app,
                Some(machines[index].id.as_str()),
                "usage.summary",
                body.clone(),
            )
        });
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

/// The notifications page for the named machines, joined by core. The
/// filter, with its time zone, stays on this computer: each machine is asked
/// with an empty body, so a runtime of any version answers.
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::json;

    /// The `usage.summary` body of a runtime before the time zone field.
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct OlderRequest {
        range: String,
        #[serde(default)]
        provider: Option<String>,
        #[serde(default)]
        utc_offset_minutes: i32,
    }

    /// One machine's runtime: the body it gets and its answer.
    type Runtime = fn(&Value) -> Result<Value, String>;

    fn older(body: &Value) -> Result<Value, String> {
        serde_json::from_value::<OlderRequest>(body.clone())
            .map(|_| json!({"zone": false}))
            .map_err(|_| "The request is invalid.".to_owned())
    }

    fn current(body: &Value) -> Result<Value, String> {
        serde_json::from_value::<usage_report::Request>(body.clone())
            .map(|request| json!({"zone": request.time_zone.is_some()}))
            .map_err(|_| "The request is invalid.".to_owned())
    }

    #[test]
    fn an_older_runtime_that_refuses_the_time_zone_answers_without_it() {
        let body = json!({"range": "7d", "utc_offset_minutes": -300, "time_zone": "America/New_York"});
        let machines: [Runtime; 3] = [
            current,
            older,
            |_| Err("That machine is offline. Try again when it answers.".to_owned()),
        ];
        let asked = std::cell::RefCell::new(Vec::new());
        let first = machines.iter().map(|machine| machine(&body)).collect();
        let answers = without_zone_again(&body, first, |index, body| {
            asked.borrow_mut().push(index);
            machines[index](body)
        });
        assert_eq!(
            answers,
            vec![
                Ok(json!({"zone": true})),
                Ok(json!({"zone": false})),
                Err("That machine is offline. Try again when it answers.".to_owned()),
            ]
        );
        assert_eq!(*asked.borrow(), vec![1]);

        // A body with no zone is never asked twice, and a second refusal
        // keeps the first error.
        let plain = json!({"range": "7d"});
        let refused = without_zone_again(
            &plain,
            vec![Err("The request is invalid.".to_owned())],
            |_, _| panic!("asked again"),
        );
        assert_eq!(refused, vec![Err("The request is invalid.".to_owned())]);
        let still = without_zone_again(&body, vec![Err("first".to_owned())], |_, _| {
            Err("second".to_owned())
        });
        assert_eq!(still, vec![Err("first".to_owned())]);
    }
}
