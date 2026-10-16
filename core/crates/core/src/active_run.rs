use serde::Deserialize;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use crate::engine::{AgentEngine, PiRpcEngine};
use crate::permission_gate::{ChatPermissionAnswer, PendingPermissionAnswer};
use crate::run_start::ActiveRun;

/// The refusal when every run place is taken.
pub fn capacity_message(limit: usize) -> String {
    format!(
        "Muniment is already running {limit} {}. Try again when one finishes.",
        if limit == 1 { "reply" } else { "replies" }
    )
}
/// The refusal when a run already replies in the thread.
pub const THREAD_BUSY: &str = "This thread is already replying.";

/// The id, workspace and thread of one live run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveIdentity {
    pub run_id: String,
    pub workspace: String,
    pub thread_id: Option<String>,
}

/// The runs this runtime drives now: up to a limit at once, and at most one
/// in each thread. Each run drives its own engine process.
/// Each live run with the thread it replies in, once known.
type RegisteredRuns = Vec<(ActiveRun, Option<String>)>;

#[derive(Clone)]
pub struct RunRegistry {
    runs: Arc<Mutex<RegisteredRuns>>,
    limit: usize,
}

impl Default for RunRegistry {
    fn default() -> Self {
        Self::with_limit(crate::agent_runs::max_parallel_runs())
    }
}

impl RunRegistry {
    /// A registry that drives at most `limit` runs at once.
    pub fn with_limit(limit: usize) -> Self {
        Self {
            runs: Arc::new(Mutex::new(Vec::new())),
            limit: limit.max(1),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, RegisteredRuns> {
        self.runs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    /// Whether a run in `thread`, or a run in a new thread, may start now.
    pub fn admits(&self, thread: Option<&str>) -> Result<(), String> {
        let runs = self.lock();
        if thread.is_some_and(|thread| runs.iter().any(|(_, held)| held.as_deref() == Some(thread)))
        {
            return Err(THREAD_BUSY.into());
        }
        if runs.len() >= self.limit {
            return Err(capacity_message(self.limit));
        }
        Ok(())
    }

    /// Adds a run, in `thread` when the thread is known.
    pub fn install(&self, run: ActiveRun, thread: Option<&str>) -> Result<(), String> {
        let mut runs = self.lock();
        if runs.iter().any(|(held, _)| held.id == run.id) {
            return Err("That reply is already running.".into());
        }
        if thread.is_some_and(|thread| runs.iter().any(|(_, held)| held.as_deref() == Some(thread)))
        {
            return Err(THREAD_BUSY.into());
        }
        if runs.len() >= self.limit {
            return Err(capacity_message(self.limit));
        }
        runs.push((run, thread.map(str::to_owned)));
        Ok(())
    }

    /// Places a run in its thread once the thread is known. Another run in
    /// the thread refuses it.
    pub fn bind_thread(&self, run_id: &str, thread: &str) -> Result<(), String> {
        let mut runs = self.lock();
        if runs
            .iter()
            .any(|(held, held_thread)| held.id != run_id && held_thread.as_deref() == Some(thread))
        {
            return Err(THREAD_BUSY.into());
        }
        let (_, held_thread) = runs
            .iter_mut()
            .find(|(held, _)| held.id == run_id)
            .ok_or_else(|| "That reply is no longer active.".to_string())?;
        *held_thread = Some(thread.to_owned());
        Ok(())
    }

    /// Removes a run. Its activity mark ends with it.
    pub fn clear(&self, run_id: &str) {
        let removed = {
            let mut runs = self.lock();
            runs.iter()
                .position(|(held, _)| held.id == run_id)
                .map(|index| runs.remove(index))
        };
        drop(removed);
    }

    pub fn contains(&self, run_id: &str) -> bool {
        self.lock().iter().any(|(held, _)| held.id == run_id)
    }

    /// The live run in `thread`, if any.
    pub fn thread_run(&self, thread: &str) -> Option<String> {
        self.lock()
            .iter()
            .find(|(_, held)| held.as_deref() == Some(thread))
            .map(|(run, _)| run.id.clone())
    }

    /// Every live run, in the order they started.
    pub fn identities(&self) -> Vec<ActiveIdentity> {
        self.lock()
            .iter()
            .map(|(run, thread)| ActiveIdentity {
                run_id: run.id.clone(),
                workspace: run.workspace.clone(),
                thread_id: thread.clone(),
            })
            .collect()
    }

    /// Calls `action` with the live run `run_id` in `workspace`, when the
    /// caller names one.
    pub fn with_run<T>(
        &self,
        run_id: &str,
        workspace: Option<&str>,
        action: impl FnOnce(&ActiveRun) -> T,
    ) -> Result<T, String> {
        let runs = self.lock();
        let run = runs
            .iter()
            .map(|(run, _)| run)
            .find(|run| run.id == run_id && workspace.is_none_or(|value| value == run.workspace))
            .ok_or_else(|| "That reply is no longer active.".to_string())?;
        Ok(action(run))
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatDelivery {
    Steer,
    FollowUp,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatQueueRequest {
    pub run_id: String,
    #[serde(default)]
    pub workspace: Option<String>,
    pub delivery: ChatDelivery,
    pub message: String,
}

pub fn queue_message(active: &RunRegistry, request: ChatQueueRequest) -> Result<(), String> {
    let (transport, adapter) =
        active.with_run(&request.run_id, request.workspace.as_deref(), |run| {
            (
                run.transport
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone(),
                run.adapter
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone(),
            )
        })?;
    let (transport, adapter) = transport
        .zip(adapter)
        .ok_or_else(|| "The reply is not ready for messages yet.".to_string())?;
    // An empty message stays empty, so the adapter still refuses it.
    let message = if request.message.trim().is_empty() {
        request.message.clone()
    } else {
        crate::launch_facts::stamp_message(&request.message)
    };
    PiRpcEngine::default()
        .steer(&transport, &adapter, request.delivery, &message)
        .map_err(|error| {
            if error == "Pi queued message must not be empty" {
                "Enter a message before sending.".to_string()
            } else {
                "The message could not be queued. Try again.".to_string()
            }
        })
}

pub fn cancel_active_run(
    active_runs: &RunRegistry,
    run_id: &str,
    workspace: Option<&str>,
) -> Result<(), String> {
    // The coordinator owns Pi cancellation and its bounded shutdown fallback.
    // A queued stop is accepted even when Pi is blocked on an extension UI.
    active_runs.with_run(run_id, workspace, |run| {
        run.cancelled.store(true, Ordering::SeqCst)
    })
}

pub fn queue_permission_answer(
    active: &RunRegistry,
    run_id: String,
    gate_id: String,
    answer: ChatPermissionAnswer,
) -> Result<(), String> {
    active.with_run(&run_id, None, |run| {
        run.permission_answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_back(PendingPermissionAnswer {
                gate_id,
                answer,
                resolved: None,
            })
    })
}

pub fn queue_permission_answer_with_commit(
    active: &RunRegistry,
    workspace: Option<&str>,
    run_id: String,
    gate_id: String,
    answer: ChatPermissionAnswer,
) -> Result<Receiver<Option<u64>>, String> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    active.with_run(&run_id, workspace, |run| {
        run.permission_answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_back(PendingPermissionAnswer {
                gate_id,
                answer,
                resolved: Some(sender),
            })
    })?;
    Ok(receiver)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::AtomicBool;

    use serde_json::json;

    use super::*;
    use crate::attach::RuntimeActivityRegistry;
    use std::sync::Arc;

    fn inactive_transport_run(
        id: &str,
        workspace: &str,
        runtime_activity: &RuntimeActivityRegistry,
    ) -> ActiveRun {
        ActiveRun {
            id: id.into(),
            workspace: workspace.into(),
            cancelled: Arc::new(AtomicBool::new(false)),
            transport: Arc::new(Mutex::new(None)),
            adapter: Arc::new(Mutex::new(None)),
            permission_answers: Arc::new(Mutex::new(VecDeque::new())),
            _activity: runtime_activity.mark_active_run(),
        }
    }

    #[test]
    fn queue_request_is_closed_and_typed() {
        for value in [
            json!({"runId":"run-1", "delivery":"later", "message":"hello"}),
            json!({"runId":"run-1", "delivery":"steer", "message":"hello", "extra":true}),
        ] {
            assert!(serde_json::from_value::<ChatQueueRequest>(value).is_err());
        }
        assert!(serde_json::from_value::<ChatQueueRequest>(json!({
            "runId":"run-1", "delivery":"followUp", "message":"hello"
        }))
        .is_ok());
    }

    #[test]
    fn queue_rejects_mismatched_and_not_ready_runs_safely() {
        let runtime_activity = RuntimeActivityRegistry::new();
        let active = RunRegistry::with_limit(3);
        active
            .install(
                inactive_transport_run("run-1", "workspace-a", &runtime_activity),
                None,
            )
            .unwrap();
        let request = |run_id: &str| ChatQueueRequest {
            run_id: run_id.into(),
            workspace: None,
            delivery: ChatDelivery::Steer,
            message: "hello".into(),
        };
        assert_eq!(
            queue_message(&active, request("stale-run")).unwrap_err(),
            "That reply is no longer active."
        );
        assert_eq!(
            queue_message(&active, request("run-1")).unwrap_err(),
            "The reply is not ready for messages yet."
        );
    }

    #[test]
    fn queue_rejects_a_run_in_another_workspace() {
        let runtime_activity = RuntimeActivityRegistry::new();
        let active = RunRegistry::with_limit(3);
        active
            .install(
                inactive_transport_run("run-1", "workspace-a", &runtime_activity),
                None,
            )
            .unwrap();
        let request = ChatQueueRequest {
            run_id: "run-1".into(),
            workspace: Some("workspace-b".into()),
            delivery: ChatDelivery::Steer,
            message: "hello".into(),
        };

        assert_eq!(
            queue_message(&active, request).unwrap_err(),
            "That reply is no longer active."
        );
    }

    #[test]
    fn cancel_rejects_a_run_in_another_workspace() {
        let runtime_activity = RuntimeActivityRegistry::new();
        let active = RunRegistry::with_limit(3);
        active
            .install(
                inactive_transport_run("run-1", "workspace-a", &runtime_activity),
                None,
            )
            .unwrap();

        assert_eq!(
            cancel_active_run(&active, "run-1", Some("workspace-b")).unwrap_err(),
            "That reply is no longer active."
        );
        let cancelled = |active: &RunRegistry| {
            active
                .with_run("run-1", None, |run| run.cancelled.load(Ordering::SeqCst))
                .unwrap()
        };
        assert!(!cancelled(&active));
        cancel_active_run(&active, "run-1", Some("workspace-a")).unwrap();
        assert!(cancelled(&active));
    }

    #[test]
    fn permission_answer_handoff_rejects_stale_runs_and_queues_typed_answers() {
        let runtime_activity = RuntimeActivityRegistry::new();
        let active = RunRegistry::with_limit(3);
        active
            .install(
                inactive_transport_run("run-1", "workspace-a", &runtime_activity),
                None,
            )
            .unwrap();
        assert_eq!(
            queue_permission_answer(
                &active,
                "stale-run".into(),
                "gate-1".into(),
                ChatPermissionAnswer::Confirm(true),
            )
            .unwrap_err(),
            "That reply is no longer active."
        );
        queue_permission_answer(
            &active,
            "run-1".into(),
            "gate-1".into(),
            ChatPermissionAnswer::Select("A".into()),
        )
        .unwrap();
        let queued = active
            .with_run("run-1", None, |run| {
                run.permission_answers.lock().unwrap().pop_front().unwrap()
            })
            .unwrap();
        assert_eq!(queued.gate_id, "gate-1");
        assert!(matches!(queued.answer, ChatPermissionAnswer::Select(value) if value == "A"));
    }

    #[test]
    fn permission_answer_with_commit_checks_workspace_and_returns_receiver() {
        let runtime_activity = RuntimeActivityRegistry::new();
        let active = RunRegistry::with_limit(3);
        active
            .install(
                inactive_transport_run("run-1", "workspace-a", &runtime_activity),
                None,
            )
            .unwrap();

        assert_eq!(
            queue_permission_answer_with_commit(
                &active,
                Some("workspace-b"),
                "run-1".into(),
                "gate-1".into(),
                ChatPermissionAnswer::Confirm(true),
            )
            .unwrap_err(),
            "That reply is no longer active."
        );
        let receiver = queue_permission_answer_with_commit(
            &active,
            Some("workspace-a"),
            "run-1".into(),
            "gate-1".into(),
            ChatPermissionAnswer::Confirm(true),
        )
        .unwrap();
        let queued = active
            .with_run("run-1", None, |run| {
                run.permission_answers.lock().unwrap().pop_front().unwrap()
            })
            .unwrap();

        queued.resolved.unwrap().send(Some(7)).unwrap();
        assert_eq!(receiver.recv().unwrap(), Some(7));
    }

    #[test]
    fn the_registry_runs_up_to_its_limit_with_one_run_per_thread() {
        let activity = RuntimeActivityRegistry::new();
        let active = RunRegistry::with_limit(3);
        for (run, thread) in [("run-1", "thread-a"), ("run-2", "thread-b")] {
            active
                .install(
                    inactive_transport_run(run, "local", &activity),
                    Some(thread),
                )
                .unwrap();
        }
        assert_eq!(
            active
                .install(
                    inactive_transport_run("run-3", "local", &activity),
                    Some("thread-a")
                )
                .unwrap_err(),
            THREAD_BUSY
        );
        assert_eq!(active.admits(Some("thread-a")).unwrap_err(), THREAD_BUSY);
        active
            .install(inactive_transport_run("run-3", "local", &activity), None)
            .unwrap();
        assert_eq!(
            active.bind_thread("run-3", "thread-b").unwrap_err(),
            THREAD_BUSY
        );
        active.bind_thread("run-3", "thread-c").unwrap();
        assert_eq!(
            active.admits(Some("thread-d")).unwrap_err(),
            capacity_message(3)
        );
        assert!(active
            .install(
                inactive_transport_run("run-4", "local", &activity),
                Some("thread-d")
            )
            .unwrap_err()
            .contains("3 replies"));
        assert_eq!(active.thread_run("thread-c").as_deref(), Some("run-3"));
        assert!(activity.snapshot().active_run);
        active.clear("run-1");
        active
            .install(
                inactive_transport_run("run-4", "local", &activity),
                Some("thread-a"),
            )
            .unwrap();
        assert_eq!(
            active
                .identities()
                .into_iter()
                .map(|identity| identity.run_id)
                .collect::<Vec<_>>(),
            ["run-2", "run-3", "run-4"]
        );
        for run in ["run-2", "run-3", "run-4"] {
            active.clear(run);
        }
        assert!(active.is_empty());
        assert!(!activity.snapshot().active_run);
    }
}
