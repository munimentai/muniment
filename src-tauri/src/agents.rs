use std::time::Duration;

use muniment_core::agents;
use muniment_core::attach::{ClientError, DesktopClientHolder, Operation};
use serde_json::Value;
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

use crate::attach_service::{AttachCompanionState, DesktopClientSession};

fn profile() -> Result<std::path::PathBuf, String> {
    muniment_runtime::profile_directory().map_err(|_| "Agents are unavailable.".into())
}

/// The id the window gives this computer's runtime.
const LOCAL_RUNTIME: &str = "local";

/// How long an agent request waits for this computer's runtime connection.
const AGENT_CONNECT_WAIT: Duration = Duration::from_secs(3);

/// The runtime an agent request goes to: this computer's, or a paired one.
#[derive(Debug, PartialEq, Eq)]
enum Target<'a> {
    Local,
    Paired(&'a str),
}

fn target(runtime: Option<&str>) -> Target<'_> {
    match runtime {
        None | Some("") | Some(LOCAL_RUNTIME) => Target::Local,
        Some(id) => Target::Paired(id),
    }
}

/// One agent operation sent to a runtime. Core decides every request.
trait AgentClient {
    fn agent_request(&self, operation: Operation, body: Value) -> Result<Value, ClientError>;
}

impl AgentClient for DesktopClientHolder {
    fn agent_request(&self, operation: Operation, body: Value) -> Result<Value, ClientError> {
        self.request(operation, None, body)
            .map(|response| response.body)
    }
}

/// The core attach operation a window names, when it manages agents.
fn agent_operation(name: &str) -> Option<Operation> {
    serde_json::from_value::<Operation>(Value::String(name.to_owned()))
        .ok()
        .filter(|operation| operation.is_agent_operation())
}

fn agent_error(error: ClientError) -> String {
    match error {
        ClientError::RequestRejected | ClientError::DesktopFailed => {
            "The machine refused the agent request. Check the fields and try again.".into()
        }
        ClientError::CapabilityRevoked | ClientError::AuthorizationFailed => {
            "This device may not manage agents on that machine. Pair it again with agent access."
                .into()
        }
        _ => crate::auth::background_service_error(),
    }
}

/// Sends one agent operation to the runtime `runtime` names. `session`
/// connects to the target, and `None` means it does not answer.
fn forward<C: AgentClient>(
    runtime: Option<&str>,
    operation: &str,
    body: Value,
    session: impl FnOnce(Target<'_>) -> Option<C>,
) -> Result<Value, String> {
    let operation = agent_operation(operation).ok_or("The agent action is unknown.")?;
    if !body.is_object() {
        return Err("The agent request is malformed.".into());
    }
    let paired = matches!(target(runtime), Target::Paired(_));
    let client = session(target(runtime)).ok_or_else(|| {
        if paired {
            "That machine is offline. Try again when it answers.".to_owned()
        } else {
            crate::auth::background_service_error()
        }
    })?;
    client.agent_request(operation, body).map_err(agent_error)
}

/// Proxies one agent or routine operation to this computer's runtime or a
/// paired runtime by id. The runtime owns agents, routines and runs.
#[tauri::command]
pub async fn agent_request(
    app: tauri::AppHandle,
    runtime: Option<String>,
    operation: String,
    body: Option<Value>,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let attach = app.state::<AttachCompanionState>();
        forward(
            runtime.as_deref(),
            &operation,
            body.unwrap_or_else(|| serde_json::json!({})),
            |target| {
                let session = match target {
                    Target::Local => attach.local_client_session_within(AGENT_CONNECT_WAIT),
                    Target::Paired(id) => attach.paired_session(id),
                };
                match session {
                    DesktopClientSession::Connected(client) => Some(client),
                    DesktopClientSession::NoSupervisor | DesktopClientSession::Disconnected => None,
                }
            },
        )
    })
    .await
    .map_err(|_| crate::auth::background_service_error())?
}

#[tauri::command]
pub fn agent_open(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let folder = agents::folder(&profile()?, &id)?;
    app.opener()
        .open_path(folder.to_string_lossy(), None::<String>)
        .map_err(|_| "The agent folder could not be opened.".into())
}

#[tauri::command]
pub async fn agent_export_template(
    app: tauri::AppHandle,
    content: String,
    name: String,
    format: Option<String>,
) -> Result<bool, String> {
    use tauri_plugin_dialog::DialogExt;
    if content.len() > 131072 {
        return Err("The template exceeds 128 KB.".into());
    }
    let extension = if format.as_deref() == Some("markdown") {
        "md"
    } else {
        "json"
    };
    let file_name: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == ' ')
        .take(80)
        .collect();
    tauri::async_runtime::spawn_blocking(move || {
        let selected = app
            .dialog()
            .file()
            .set_file_name(format!(
                "{}.agent.{}",
                if file_name.is_empty() {
                    "agent"
                } else {
                    &file_name
                },
                extension
            ))
            .add_filter("Agent template", &[extension])
            .blocking_save_file();
        let Some(selected) = selected else {
            return Ok(false);
        };
        let path = selected.into_path().map_err(|_| "Choose a local file.")?;
        std::fs::write(path, content).map_err(|_| "The template could not be exported.")?;
        Ok(true)
    })
    .await
    .map_err(|_| "The template export could not finish.".to_string())?
}

#[tauri::command]
pub async fn agent_import_link(url: String) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        muniment_core::agent_templates::fetch_public_template(&url)
    })
    .await
    .map_err(|_| "The template could not be loaded.".to_string())?
}

/// One agent's facts. The runtime owns the memory files, so each action
/// forwards to its model service with the agent named.
#[tauri::command]
pub async fn agent_memory(
    app: tauri::AppHandle,
    id: String,
    action: String,
    fact: Option<muniment_core::memory_files::Fact>,
    fact_id: Option<String>,
) -> Result<serde_json::Value, String> {
    use muniment_core::attach::ModelAction;
    use serde_json::json;
    let (action, data) = match action.as_str() {
        "memory_facts" => (ModelAction::MemoryFacts, json!({"agent": id})),
        "memory_deleted_facts" => (ModelAction::MemoryDeletedFacts, json!({"agent": id})),
        "memory_fact_save" => (
            ModelAction::MemoryFactSave,
            json!({"agent": id, "fact": fact.ok_or("Give the memory a title and fact.")?}),
        ),
        "memory_fact_delete" => (
            ModelAction::MemoryFactDelete,
            json!({"agent": id, "id": fact_id.ok_or("Choose a memory.")?}),
        ),
        "memory_fact_restore" => (
            ModelAction::MemoryFactRestore,
            json!({"agent": id, "id": fact_id.ok_or("Choose a memory.")?}),
        ),
        "memory_pending" => (ModelAction::MemoryPending, json!({"agent": id})),
        "memory_pending_apply" => (
            ModelAction::MemoryPendingApply,
            json!({"agent": id, "id": fact_id.ok_or("Choose a suggested change.")?}),
        ),
        "memory_pending_dismiss" => (
            ModelAction::MemoryPendingDismiss,
            json!({"agent": id, "id": fact_id.ok_or("Choose a suggested change.")?}),
        ),
        _ => return Err("The memory action is unknown.".into()),
    };
    crate::model_service::request(&app, action, data).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Recorder {
        runtime: String,
        calls: RefCell<Vec<(Operation, Value)>>,
    }

    impl AgentClient for &Recorder {
        fn agent_request(&self, operation: Operation, body: Value) -> Result<Value, ClientError> {
            self.calls.borrow_mut().push((operation, body));
            Ok(serde_json::json!({"runtime": self.runtime}))
        }
    }

    fn connect<'a>(
        local: &'a Recorder,
        paired: &'a Recorder,
    ) -> impl FnOnce(Target<'_>) -> Option<&'a Recorder> {
        move |target| match target {
            Target::Local => Some(local),
            Target::Paired(id) if id == paired.runtime => Some(paired),
            Target::Paired(_) => None,
        }
    }

    fn recorder(runtime: &str) -> Recorder {
        Recorder {
            runtime: runtime.into(),
            calls: RefCell::new(Vec::new()),
        }
    }

    #[test]
    fn a_request_goes_to_the_runtime_it_names() {
        let local = recorder("local");
        let paired = recorder("box");
        let body = serde_json::json!({"id": "agent-1"});
        assert_eq!(
            forward(
                Some("box"),
                "agent.read",
                body.clone(),
                connect(&local, &paired)
            )
            .unwrap(),
            serde_json::json!({"runtime": "box"})
        );
        assert_eq!(
            paired.calls.borrow().as_slice(),
            [(Operation::AgentRead, body.clone())]
        );
        assert!(local.calls.borrow().is_empty());

        for runtime in [None, Some(""), Some("local")] {
            assert_eq!(
                forward(
                    runtime,
                    "agent.list",
                    serde_json::json!({}),
                    connect(&local, &paired)
                )
                .unwrap(),
                serde_json::json!({"runtime": "local"})
            );
        }
        assert_eq!(local.calls.borrow().len(), 3);
        assert_eq!(paired.calls.borrow().len(), 1);
    }

    #[test]
    fn an_offline_machine_or_another_operation_sends_nothing() {
        let local = recorder("local");
        let paired = recorder("box");
        assert_eq!(
            forward(
                Some("studio"),
                "agent.list",
                serde_json::json!({}),
                connect(&local, &paired)
            )
            .unwrap_err(),
            "That machine is offline. Try again when it answers."
        );
        for operation in ["thread.delete", "run.submit", "agent.unknown"] {
            assert_eq!(
                forward(
                    None,
                    operation,
                    serde_json::json!({}),
                    connect(&local, &paired)
                )
                .unwrap_err(),
                "The agent action is unknown."
            );
        }
        assert_eq!(
            forward(
                None,
                "agent.list",
                serde_json::json!([]),
                connect(&local, &paired)
            )
            .unwrap_err(),
            "The agent request is malformed."
        );
        assert!(local.calls.borrow().is_empty());
        assert!(paired.calls.borrow().is_empty());
    }

    #[test]
    fn every_agent_and_routine_operation_passes_through() {
        for (name, operation) in [
            ("agent.list", Operation::AgentList),
            ("agent.read", Operation::AgentRead),
            ("agent.save", Operation::AgentSave),
            ("agent.delete", Operation::AgentDelete),
            ("agent.run_now", Operation::AgentRunNow),
            ("agent.runs", Operation::AgentRuns),
            ("routine.list", Operation::RoutineList),
            ("routine.save", Operation::RoutineSave),
            ("routine.delete", Operation::RoutineDelete),
        ] {
            assert_eq!(agent_operation(name), Some(operation));
        }
    }
}
