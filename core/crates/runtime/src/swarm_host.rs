//! What the runtime does for its swarm: the scope replica threads take,
//! steering a live run, answering a run's gate, starting queued agent runs
//! soon after a message queues one, and running a message another member
//! posted to an ordinary thread this runtime owns.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use muniment_core::active_run::{
    queue_message, queue_permission_answer_with_commit, ChatDelivery, ChatQueueRequest, RunRegistry,
};
use muniment_core::attach::{RuntimeActivityRegistry, SignedWorkspaceApproval};
use muniment_core::permission_gate::ChatPermissionAnswer;
use muniment_core::swarm::{ReplicaScope, SwarmHost};

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
    pub(crate) kicking: AtomicBool,
}

impl RuntimeSwarmHost {
    /// Lets the host start agent runs through the runtime state.
    pub(crate) fn bind(&self, state: &Arc<RuntimeAttachState>) {
        *self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Arc::downgrade(state);
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

    fn submit(&self, thread_id: &str, text: &str) -> Result<String, String> {
        // Before the runtime binds its state the post waits, as for a busy
        // thread.
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .upgrade()
            .ok_or(muniment_core::active_run::THREAD_BUSY)?;
        let boundaries = state.boundaries();
        let workspace = boundaries
            .signed_workspace_approval()
            .approval()
            .map(|approval| approval.workspace)
            .ok_or("Open Muniment and connect a model before running threads.")?;
        boundaries
            .submit_run_with_id(
                muniment_core::run_start::new_run_id(),
                &workspace,
                text.to_owned(),
                Vec::new(),
                Some(thread_id.to_owned()),
                None,
                None,
            )
            .map(|accepted| accepted.run_id)
            .map_err(|error| error.into_message())
    }

    fn kick_runs(&self) {
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
