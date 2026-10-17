//! The thread operations of the permission modes: reading and setting the
//! mode a thread runs in, and reading and answering the plan a `plan` run
//! waits to have approved.

use std::path::Path;

use serde::Deserialize;
use serde_json::{json, Value};

use super::settings;
use super::{PermissionMode, PLAN_ANSWERS};
use crate::active_run::RunRegistry;
use crate::journal::reducer::{ChatProjector, PermissionGate};
use crate::journal::Provenance;
use crate::run_events::SharedStorage;

/// How long a plan answer waits for the run to take it.
const ANSWER_COMMIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// What a thread operation needs from the runtime.
pub struct ThreadScope<'a> {
    pub profile: &'a Path,
    pub storage: &'a SharedStorage,
    pub active: &'a RunRegistry,
    /// The provenance a mode change records.
    pub provenance: Provenance,
    /// Whether another member runs the thread, so a mode set here would
    /// never reach its runs.
    pub replica_of: &'a dyn Fn(&str) -> Option<String>,
    /// Sends a mode set on a replica to the member that runs the thread.
    pub forward_mode: &'a dyn Fn(&str, PermissionMode) -> Result<(), String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ByThread {
    thread_id: String,
}

fn lock(
    storage: &SharedStorage,
) -> Result<std::sync::MutexGuard<'_, crate::run_events::ChatStorage>, String> {
    storage
        .lock()
        .map_err(|_| "Conversation history is busy.".to_string())
}

/// The mode a thread runs in, where it comes from, and the agent it
/// belongs to.
fn describe(scope: &ThreadScope<'_>, thread_id: &str) -> Result<Value, String> {
    let set = {
        let mut storage = lock(scope.storage)?;
        if storage
            .journal
            .last_thread_seq(thread_id)
            .map_err(|error| error.to_string())?
            == 0
        {
            return Err("The thread was not found.".into());
        }
        storage
            .journal
            .thread_permission_mode(thread_id)
            .map_err(|error| error.to_string())?
    };
    let agent = crate::agents::thread_agent(scope.profile, thread_id)?;
    let settings = settings::load(scope.profile);
    let agent_mode = agent
        .as_ref()
        .and_then(|agent| agent.permissions.as_ref())
        .and_then(|permissions| permissions.mode);
    let (mode, source) = match (&set, agent_mode) {
        (Some(set), _) => (set.mode, "thread"),
        (None, Some(mode)) => (mode, "agent"),
        (None, None) if agent.is_some() => (settings.default_agent_mode, "default"),
        (None, None) => (settings.default_thread_mode, "default"),
    };
    let mode = settings.effective(mode);
    let mut answer = json!({
        "thread_id": thread_id,
        "mode": mode,
        "title": mode.title(),
        "source": source,
    });
    if let Some(set) = &set {
        answer["set_by"] = json!(set.source);
    }
    if let Some(agent) = &agent {
        answer["agent_id"] = json!(agent.id);
    }
    Ok(answer)
}

/// The plan gate a thread's live run waits at: its run id and gate.
fn pending_plan(
    scope: &ThreadScope<'_>,
    thread_id: &str,
) -> Result<Option<(String, PermissionGate)>, String> {
    let Some(run_id) = scope.active.thread_run(thread_id) else {
        return Ok(None);
    };
    let events = lock(scope.storage)?
        .journal
        .events(&run_id)
        .map_err(|error| error.to_string())?;
    let mut projector = ChatProjector::new();
    for event in &events {
        projector.apply(event).map_err(|error| error.to_string())?;
    }
    Ok(projector
        .projection()
        .map_err(|error| error.to_string())?
        .pending_permission
        .filter(|gate| {
            gate.policy
                .as_ref()
                .is_some_and(|policy| policy.plan.is_some())
        })
        .map(|gate| (run_id, gate)))
}

/// Answers `thread.permission_mode`, `thread.permission_mode_set`,
/// `thread.plan` and `thread.plan_answer`.
pub fn operation(scope: &ThreadScope<'_>, operation: &str, body: Value) -> Result<Value, String> {
    let invalid = |_| "The request is invalid.".to_string();
    match operation {
        "thread.permission_mode" => {
            let request: ByThread = serde_json::from_value(body).map_err(invalid)?;
            describe(scope, &request.thread_id)
        }
        "thread.permission_mode_set" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Set {
                thread_id: String,
                mode: PermissionMode,
            }
            let request: Set = serde_json::from_value(body).map_err(invalid)?;
            if let Some(reason) = settings::load(scope.profile).unavailable(request.mode) {
                return Err(reason.into());
            }
            // The member that runs the thread sets the mode, and the set
            // replicates back here.
            if let Some(owner) = (scope.replica_of)(&request.thread_id) {
                (scope.forward_mode)(&request.thread_id, request.mode)?;
                let mut answer = describe(scope, &request.thread_id)?;
                answer["forwarded_to"] = Value::from(owner);
                answer["requested_mode"] =
                    serde_json::to_value(request.mode).map_err(|error| error.to_string())?;
                return Ok(answer);
            }
            describe(scope, &request.thread_id)?;
            {
                let mut storage = lock(scope.storage)?;
                super::set_thread_mode(
                    &mut storage.journal,
                    &request.thread_id,
                    request.mode,
                    "user",
                    scope.provenance.actor_id.as_deref().unwrap_or("owner"),
                    &scope.provenance,
                )?;
            }
            describe(scope, &request.thread_id)
        }
        "thread.plan" => {
            let request: ByThread = serde_json::from_value(body).map_err(invalid)?;
            let Some((run_id, gate)) = pending_plan(scope, &request.thread_id)? else {
                return Ok(json!({"plan": null}));
            };
            let policy = gate.policy.expect("a plan gate has a policy");
            Ok(json!({"plan": {
                "run_id": run_id,
                "gate_id": gate.gate_id,
                "approval_id": format!("{run_id}:{}", gate.gate_id),
                "summary": policy.summary,
                "plan": policy.plan,
                "answers": PLAN_ANSWERS.iter().map(|(id, _, _)| *id).collect::<Vec<_>>(),
            }}))
        }
        "thread.plan_answer" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Answer {
                thread_id: String,
                answer: String,
            }
            let request: Answer = serde_json::from_value(body).map_err(invalid)?;
            let option = PLAN_ANSWERS
                .iter()
                .find(|(id, _, _)| *id == request.answer)
                .map(|(_, option, _)| *option)
                .ok_or("That is not a plan answer.")?;
            let (run_id, gate) = pending_plan(scope, &request.thread_id)?
                .ok_or("No plan waits for approval in this thread.")?;
            crate::active_run::queue_permission_answer_with_commit(
                scope.active,
                None,
                run_id,
                gate.gate_id,
                super::ChatPermissionAnswer::Select(option.to_owned()),
            )?
            .recv_timeout(ANSWER_COMMIT_TIMEOUT)
            .map_err(|_| "The answer did not commit in time.".to_string())?
            .ok_or("The plan was already answered.")?;
            Ok(json!({"status": "answered", "answer": request.answer}))
        }
        _ => Err("The permission operation is unknown.".into()),
    }
}
