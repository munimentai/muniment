use std::time::Duration;

use muniment_core::agents;
use muniment_core::attach::{
    ClientError, DesktopClientHolder, ErrorDetails, Operation, ProtocolError,
};
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

/// A refused request: the client's error and the runtime's protocol error,
/// which carries core's reason when the runtime gave one.
type Refusal = (ClientError, Option<ProtocolError>);

/// One agent operation sent to a runtime. Core decides every request.
trait AgentClient {
    fn agent_request(&self, operation: Operation, body: Value) -> Result<Value, Refusal>;
}

impl AgentClient for DesktopClientHolder {
    fn agent_request(&self, operation: Operation, body: Value) -> Result<Value, Refusal> {
        if operation.is_agent_operation() {
            DesktopClientHolder::agent_request(self, operation, body)
        } else {
            self.swarm_request(operation, body)
        }
    }
}

/// The core attach operation a window names, when it manages agents or the
/// swarm. The exchange between linked runtimes is theirs alone.
fn agent_operation(name: &str) -> Option<Operation> {
    serde_json::from_value::<Operation>(Value::String(name.to_owned()))
        .ok()
        .filter(|operation| {
            operation.is_agent_operation()
                || (operation.is_swarm_operation() && *operation != Operation::SwarmExchange)
        })
}

/// The line a refused request shows: core's reason when the runtime gave one,
/// else a line for the kind of failure.
fn agent_error((error, refusal): Refusal) -> String {
    if let Some(ErrorDetails::RequestReason { reason }) =
        refusal.as_ref().and_then(ProtocolError::details)
    {
        if !reason.trim().is_empty() {
            return reason.trim().to_owned();
        }
    }
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

/// Proxies one agent, routine or swarm operation to this computer's runtime
/// or a paired runtime by id. The runtime owns agents, routines, runs, links,
/// groups and approvals.
#[tauri::command]
pub async fn agent_request(
    app: tauri::AppHandle,
    runtime: Option<String>,
    operation: String,
    body: Option<Value>,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        request_runtime(
            &app,
            runtime.as_deref(),
            &operation,
            body.unwrap_or_else(|| serde_json::json!({})),
        )
    })
    .await
    .map_err(|_| crate::auth::background_service_error())?
}

/// Sends one agent operation to this computer's runtime or a paired one and
/// waits for the answer. It blocks, so callers run it off the async runtime.
pub(crate) fn request_runtime(
    app: &tauri::AppHandle,
    runtime: Option<&str>,
    operation: &str,
    body: Value,
) -> Result<Value, String> {
    let attach = app.state::<AttachCompanionState>();
    forward(runtime, operation, body, |target| {
        let session = match target {
            Target::Local => attach.local_client_session_within(AGENT_CONNECT_WAIT),
            Target::Paired(id) => attach.paired_session(id),
        };
        match session {
            DesktopClientSession::Connected(client) => Some(client),
            DesktopClientSession::NoSupervisor | DesktopClientSession::Disconnected => None,
        }
    })
}

/// The `thread.create` body that starts a thread on the fleet member
/// `runtime_id` with its first message. Core validates the rest.
fn offload_body(
    runtime_id: &str,
    text: &str,
    permission_mode: Option<&str>,
) -> Result<Value, String> {
    if runtime_id.trim().is_empty() {
        return Err("Choose a machine for the thread.".into());
    }
    if text.trim().is_empty() {
        return Err("Enter a message before sending.".into());
    }
    let mut body = serde_json::json!({"runtime_id": runtime_id, "text": text});
    if let Some(mode) = permission_mode.filter(|mode| !mode.is_empty()) {
        body["permission_mode"] = Value::String(mode.to_owned());
    }
    Ok(body)
}

/// Starts a new thread on another fleet member, containers included.
/// This computer's runtime forwards `thread.create` to that member, which
/// owns the thread and runs its first message. The answer names the thread
/// and the forwarded action.
#[tauri::command]
pub async fn fleet_thread_create(
    app: tauri::AppHandle,
    runtime_id: String,
    text: String,
    permission_mode: Option<String>,
) -> Result<Value, String> {
    let body = offload_body(&runtime_id, &text, permission_mode.as_deref())?;
    tauri::async_runtime::spawn_blocking(move || {
        let attach = app.state::<AttachCompanionState>();
        let DesktopClientSession::Connected(client) =
            attach.local_client_session_within(AGENT_CONNECT_WAIT)
        else {
            return Err(crate::auth::background_service_error());
        };
        let key = muniment_core::attach::Id::new(uuid::Uuid::now_v7().simple().to_string())
            .map_err(|_| crate::auth::background_service_error())?;
        client
            .request_with_error(Operation::ThreadCreate, Some(key), body)
            .map(|response| response.body)
            .map_err(agent_error)
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

/// Saves the file `agent.export` answered where the user picks. Core builds
/// the content and its file name.
#[tauri::command]
pub async fn agent_export_template(
    app: tauri::AppHandle,
    content: String,
    file_name: String,
) -> Result<bool, String> {
    use tauri_plugin_dialog::DialogExt;
    if content.len() > 131072 {
        return Err("The template exceeds 128 KB.".into());
    }
    let file_name = export_file_name(&file_name);
    let extension = file_name
        .rsplit_once('.')
        .map_or("md", |(_, extension)| extension)
        .to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        let selected = app
            .dialog()
            .file()
            .set_file_name(&file_name)
            .add_filter("Agent template", &[extension.as_str()])
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

/// A file name the save dialog can offer: letters, digits, dashes, spaces
/// and dots, at most 120 characters, never empty or hidden.
fn export_file_name(name: &str) -> String {
    let name: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | ' ' | '.' | '_'))
        .take(120)
        .collect();
    let name = name.trim_start_matches('.').trim();
    if name.is_empty() {
        "agent.md".into()
    } else {
        name.into()
    }
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
        refusal: Option<Refusal>,
    }

    impl AgentClient for &Recorder {
        fn agent_request(&self, operation: Operation, body: Value) -> Result<Value, Refusal> {
            self.calls.borrow_mut().push((operation, body));
            match &self.refusal {
                Some(refusal) => Err(refusal.clone()),
                None => Ok(serde_json::json!({"runtime": self.runtime})),
            }
        }
    }

    fn refusal(reason: Option<&str>) -> Refusal {
        let error = match reason {
            Some(reason) => ProtocolError::invalid_request_with_reason(reason),
            None => ProtocolError::invalid_request(),
        };
        (ClientError::RequestRejected, Some(error))
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
            refusal: None,
        }
    }

    #[test]
    fn a_refusal_shows_the_runtime_reason() {
        let mut local = recorder("local");
        let paired = recorder("box");
        local.refusal = Some(refusal(Some(
            "A cron routine runs at most every five minutes.",
        )));
        assert_eq!(
            forward(
                None,
                "routine.save",
                serde_json::json!({"agent_id": "a"}),
                connect(&local, &paired)
            )
            .unwrap_err(),
            "A cron routine runs at most every five minutes."
        );
        local.refusal = Some(refusal(None));
        assert_eq!(
            forward(
                None,
                "agent.save",
                serde_json::json!({}),
                connect(&local, &paired)
            )
            .unwrap_err(),
            "The machine refused the agent request. Check the fields and try again."
        );
        local.refusal = Some((ClientError::RequestRejected, None));
        assert_eq!(
            forward(
                None,
                "thread.post",
                serde_json::json!({}),
                connect(&local, &paired)
            )
            .unwrap_err(),
            "The machine refused the agent request. Check the fields and try again."
        );
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
    fn an_offloaded_thread_names_its_machine_and_first_message() {
        assert_eq!(
            offload_body("runtime-pod", "Summarize the logs.", Some("manual")).unwrap(),
            serde_json::json!({"runtime_id": "runtime-pod", "text": "Summarize the logs.", "permission_mode": "manual"})
        );
        assert_eq!(
            offload_body("runtime-pod", "Hi", None).unwrap(),
            serde_json::json!({"runtime_id": "runtime-pod", "text": "Hi"})
        );
        assert_eq!(
            offload_body("", "Hi", None).unwrap_err(),
            "Choose a machine for the thread."
        );
        assert_eq!(
            offload_body("runtime-pod", "  ", None).unwrap_err(),
            "Enter a message before sending."
        );
    }

    #[test]
    fn an_export_file_name_stays_plain() {
        assert_eq!(
            export_file_name("Lead-scout.grok-bot.md"),
            "Lead-scout.grok-bot.md"
        );
        assert_eq!(export_file_name("../../etc/passwd"), "etcpasswd");
        assert_eq!(export_file_name("..."), "agent.md");
        assert_eq!(export_file_name(""), "agent.md");
    }

    #[test]
    fn every_swarm_operation_but_the_exchange_passes_through() {
        for (name, operation) in [
            ("swarm.invite", Operation::SwarmInvite),
            ("swarm.join", Operation::SwarmJoin),
            ("swarm.leave", Operation::SwarmLeave),
            ("swarm.members", Operation::SwarmMembers),
            ("swarm.agents", Operation::SwarmAgents),
            ("swarm.resume", Operation::SwarmResume),
            ("thread.post", Operation::ThreadPost),
            ("agent.move", Operation::AgentMove),
            ("agent.approvals", Operation::AgentApprovals),
            ("agent.approval_answer", Operation::AgentApprovalAnswer),
            ("group.create", Operation::GroupCreate),
            ("group.list", Operation::GroupList),
            ("group.update", Operation::GroupUpdate),
            ("fleet.op_status", Operation::FleetOpStatus),
            ("fleet.op_cancel", Operation::FleetOpCancel),
            ("fleet.ops", Operation::FleetOps),
            ("fleet.sync_status", Operation::FleetSyncStatus),
            ("fleet.offline_set", Operation::FleetOfflineSet),
            ("thread.history_fetch", Operation::ThreadHistoryFetch),
            ("agent.routine_proposals", Operation::AgentRoutineProposals),
            (
                "agent.routine_proposal_accept",
                Operation::AgentRoutineProposalAccept,
            ),
            (
                "agent.routine_proposal_dismiss",
                Operation::AgentRoutineProposalDismiss,
            ),
            ("agent.import", Operation::AgentImport),
            ("agent.import_commit", Operation::AgentImportCommit),
            ("agent.export", Operation::AgentExport),
            ("routine.webhook_create", Operation::RoutineWebhookCreate),
            ("routine.webhook_rotate", Operation::RoutineWebhookRotate),
            ("routine.webhook_info", Operation::RoutineWebhookInfo),
        ] {
            assert_eq!(agent_operation(name), Some(operation), "{name}");
        }
        assert_eq!(agent_operation("swarm.exchange"), None);
    }

    #[test]
    fn a_swarm_request_goes_to_the_runtime_it_names() {
        let local = recorder("local");
        let paired = recorder("box");
        let body = serde_json::json!({"link": "muniment://pair?v=1"});
        forward(
            Some("box"),
            "swarm.join",
            body.clone(),
            connect(&local, &paired),
        )
        .unwrap();
        assert_eq!(
            paired.calls.borrow().as_slice(),
            [(Operation::SwarmJoin, body)]
        );
        assert_eq!(
            forward(
                None,
                "swarm.exchange",
                serde_json::json!({}),
                connect(&local, &paired)
            )
            .unwrap_err(),
            "The agent action is unknown."
        );
        assert!(local.calls.borrow().is_empty());
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
            ("agent.threads", Operation::AgentThreads),
            ("agent.tool_catalog", Operation::AgentToolCatalog),
            ("agent.draft", Operation::AgentDraft),
            ("agent.replays", Operation::AgentReplays),
            ("agent.replay", Operation::AgentReplay),
            ("usage.summary", Operation::UsageSummary),
            ("notifications.list", Operation::NotificationsList),
        ] {
            assert_eq!(agent_operation(name), Some(operation));
        }
    }
}
