//! What the runtime does for its swarm: the scope replica threads take,
//! steering a live run, answering a run's gate, starting queued agent runs
//! soon after a message queues one, and running actions other members
//! forwarded through the runtime's own request dispatch.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use muniment_core::active_run::{
    queue_message, queue_permission_answer_with_commit, ChatDelivery, ChatQueueRequest, RunRegistry,
};
use muniment_core::attach::desktop_service_message::CompanionProvenance;
use muniment_core::attach::{
    ErrorDetails, Id, Protocol, ProtocolError, Request, RuntimeActivityRegistry,
    SignedWorkspaceApproval,
};
use muniment_core::permission_gate::ChatPermissionAnswer;
use muniment_core::serde_json;
use muniment_core::swarm::{Forwarded, ReplicaScope, SwarmHost};

use crate::host::RuntimeHost;
use crate::RuntimeAttachState;

const ANSWER_COMMIT_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) struct RuntimeSwarmHost {
    pub(crate) active: RunRegistry,
    pub(crate) approval: SignedWorkspaceApproval,
    pub(crate) config_directory: PathBuf,
    pub(crate) host: RuntimeHost,
    pub(crate) runtime_activity: RuntimeActivityRegistry,
    pub(crate) state: Mutex<Weak<RuntimeAttachState>>,
    /// Whether the host starts queued agent runs itself.
    pub(crate) kicks: AtomicBool,
    pub(crate) kicking: AtomicBool,
}

impl RuntimeSwarmHost {
    /// Lets the host run forwarded actions through the runtime state.
    pub(crate) fn bind(&self, state: &Arc<RuntimeAttachState>) {
        *self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Arc::downgrade(state);
    }
}

/// The reason a request failed, as a sentence.
fn error_text(error: &ProtocolError) -> String {
    match error.details() {
        Some(ErrorDetails::RequestReason { reason }) => reason.clone(),
        _ => serde_json::to_value(error.message())
            .ok()
            .and_then(|message| message.as_str().map(str::to_owned))
            .unwrap_or_else(|| "The action failed.".into()),
    }
}

impl SwarmHost for RuntimeSwarmHost {
    fn replica_scope(&self) -> Option<ReplicaScope> {
        crate::attach_boundaries::record_local_workspace_approval(
            &self.approval,
            &self.config_directory,
        );
        let approval = self.approval.approval()?;
        let subject = if muniment_core::local_mode::is_local_mode(&self.config_directory) {
            None
        } else {
            self.host
                .account
                .session(&self.runtime_activity)
                .ok()?
                .tokens?
                .subject
        };
        Some(ReplicaScope {
            workspace: approval.workspace,
            attach_profile: approval.profile,
            subject,
        })
    }

    fn steer(&self, thread_id: &str, text: &str) -> Option<String> {
        let run_id = self.active.thread_run(thread_id)?;
        queue_message(
            &self.active,
            ChatQueueRequest {
                run_id: run_id.clone(),
                workspace: None,
                delivery: ChatDelivery::Steer,
                message: text.to_owned(),
            },
        )
        .ok()
        .map(|()| run_id)
    }

    fn answer_permission(
        &self,
        run_id: &str,
        gate_id: &str,
        answer: ChatPermissionAnswer,
    ) -> Result<(), String> {
        queue_permission_answer_with_commit(
            &self.active,
            None,
            run_id.to_owned(),
            gate_id.to_owned(),
            answer,
        )?
        .recv_timeout(ANSWER_COMMIT_TIMEOUT)
        .map_err(|_| "The answer did not commit in time.".to_string())?
        .map(drop)
        .ok_or_else(|| "The gate refused the answer.".to_string())
    }

    fn hook_secret(&self, hook_id: &str) -> Option<String> {
        self.host
            .credentials
            .read(muniment_core::agent_hooks::SECRET_SERVICE, hook_id)
            .filter(|secret| !secret.is_empty())
    }

    fn store_hook_secret(&self, hook_id: &str, secret: &str) -> Result<(), String> {
        self.host
            .credentials
            .write(muniment_core::agent_hooks::SECRET_SERVICE, hook_id, secret)
    }

    fn settings_applied(&self, class: &str) {
        let Some(state) = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .upgrade()
        else {
            return;
        };
        match class {
            "route" | "decision-model" => state.models().apply_router(),
            "search" => state.models().restore_search(),
            "retention" => {
                if let Err(error) = state.apply_recorded_retention() {
                    muniment_core::runtime_eprintln!(
                        "muniment-runtime: retention from another machine failed: {error}"
                    );
                }
            }
            _ => {}
        }
    }

    fn run_forwarded(&self, action: &Forwarded) -> Result<serde_json::Value, String> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .upgrade()
            .ok_or("The runtime is stopping.")?;
        crate::attach_boundaries::record_local_workspace_approval(
            &self.approval,
            &self.config_directory,
        );
        let approval = self
            .approval
            .approval()
            .ok_or("Open Muniment on this machine before you act on it from another one.")?;
        let mut service = state
            .forwarded_service(&action.from_runtime)
            .map_err(|error| error_text(&error))?;
        let invalid = |_| "The action is invalid.".to_string();
        let request = Request {
            protocol: Protocol,
            request_id: Id::new(uuid::Uuid::new_v4().to_string()).map_err(invalid)?,
            operation: action.operation,
            capability: String::new(),
            idempotency_key: action
                .operation
                .requires_idempotency_key()
                .then(|| Id::new(action.idempotency_key.clone()))
                .transpose()
                .map_err(invalid)?,
            body: action.body.clone(),
        };
        let provenance = CompanionProvenance {
            profile: approval.profile,
            companion_kind: muniment_core::swarm::PEER_KIND.into(),
            companion_version: state.runtime_version().into(),
            peer_uid: 0,
            peer_pid: 0,
        };
        muniment_core::attach::dispatch_forwarded(
            &mut service,
            request,
            &approval.workspace,
            provenance,
        )
        .map_err(|error| error_text(&error))
    }

    fn kick_runs(&self) {
        if !self.kicks.load(Ordering::SeqCst) {
            return;
        }
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .upgrade();
        let Some(state) = state else {
            return;
        };
        if self.kicking.swap(true, Ordering::SeqCst) {
            return;
        }
        let host = Arc::clone(&state.swarm_host);
        std::thread::spawn(move || {
            host.kicking.store(false, Ordering::SeqCst);
            if let Err(error) = state.check_agent_schedules() {
                muniment_core::runtime_eprintln!("agent_schedule: {error}");
            }
        });
    }
}
