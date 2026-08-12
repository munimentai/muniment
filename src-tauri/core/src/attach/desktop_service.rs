//! Generic desktop attach service seam.

#![cfg(target_os = "linux")]

use super::linux::{
    CompanionProvenance, MigrationControlRequest, PermissionAnswerAccepted,
    PermissionAnswerRequest, PermissionDecision, RunCancelAccepted, RunCancelRequest,
    RunStartAccepted, RunStartRequest as AttachRunStartRequest, RunStreamPage,
    ThreadCreateAccepted, ThreadListPage, ThreadListRequest, ThreadListService, ThreadOpenPage,
    ThreadOpenRequest,
};
use super::{
    bounded_claim, save_client_credentials as persist_client_credentials, Approval,
    ClientCredential, CommittedResult, Id, IdempotencyOutcome, IdempotencyStore, Operation,
    Protocol, ProtocolError, Request as AttachRequest, WorkspaceContextMap,
    WorkspaceOnboardRequest, WorkspaceOnboarded,
};
use crate::journal::{JournalCommitHint, Provenance};
use crate::permission_gate::ChatPermissionAnswer;
use crate::run_start::{prepare_desktop_run, RunStartBoundaries, RunStartRequest};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg(not(test))]
const PERMISSION_COMMIT_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(test)]
const PERMISSION_COMMIT_TIMEOUT: Duration = Duration::from_millis(50);

pub type WorkspaceContexts = Arc<Mutex<WorkspaceContextMap>>;

/// Production adapter from the authorized Linux attach seam into the desktop coordinator.
pub trait RunStartIdempotency {
    fn execute<A, W>(
        &mut self,
        profile: &str,
        request: &AttachRequest,
        canonical_input: &Value,
        authorize: A,
        work: W,
    ) -> Result<IdempotencyOutcome, ProtocolError>
    where
        A: FnOnce() -> Result<(), ProtocolError>,
        W: FnOnce() -> Result<CommittedResult, ProtocolError>;
}

impl RunStartIdempotency for IdempotencyStore {
    fn execute<A, W>(
        &mut self,
        profile: &str,
        request: &AttachRequest,
        canonical_input: &Value,
        authorize: A,
        work: W,
    ) -> Result<IdempotencyOutcome, ProtocolError>
    where
        A: FnOnce() -> Result<(), ProtocolError>,
        W: FnOnce() -> Result<CommittedResult, ProtocolError>,
    {
        IdempotencyStore::execute(self, profile, request, canonical_input, authorize, |_| {
            work()
        })
    }
}

pub struct DesktopAttachService<B, I = IdempotencyStore> {
    pub boundaries: B,
    pub idempotency: I,
    pub home: PathBuf,
    pub workspace_contexts: WorkspaceContexts,
    pub client_credentials: Arc<Mutex<HashMap<String, ClientCredential>>>,
    pub credential_path: Option<PathBuf>,
    pub client_identity: Option<String>,
}

#[cfg(target_os = "linux")]
impl<B: RunStartBoundaries, I: RunStartIdempotency> ThreadListService
    for DesktopAttachService<B, I>
{
    fn bind_authorized_client(&mut self, client_identity: &str) {
        self.client_identity = Some(client_identity.to_owned());
    }

    fn reconnect_approval(&self) -> Option<Approval> {
        self.boundaries.attach_approval()
    }

    fn authorize_client(
        &mut self,
        client_identity: &str,
        presented_credential: Option<&str>,
        issued_credential: &str,
        claimed_kind: &str,
        claimed_version: &str,
    ) -> Result<String, ProtocolError> {
        let mut credentials = self
            .client_credentials
            .lock()
            .map_err(|_| ProtocolError::unauthorized())?;
        let credential = match credentials.get(client_identity) {
            Some(expected) if presented_credential == Some(expected.credential.as_str()) => {
                expected.credential.clone()
            }
            Some(_) => return Err(ProtocolError::unauthorized()),
            None if presented_credential.is_none() => {
                credentials.insert(
                    client_identity.to_owned(),
                    ClientCredential {
                        credential: issued_credential.to_owned(),
                        claimed_kind: bounded_claim(claimed_kind),
                        claimed_version: bounded_claim(claimed_version),
                        approved_at: Some(
                            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true),
                        ),
                    },
                );
                if let Some(path) = &self.credential_path {
                    if persist_client_credentials(path, &credentials).is_err() {
                        credentials.remove(client_identity);
                        return Err(ProtocolError::persistence_failed());
                    }
                }
                issued_credential.to_owned()
            }
            None => return Err(ProtocolError::unauthorized()),
        };
        self.client_identity = Some(client_identity.to_owned());
        Ok(credential)
    }

    fn onboard_workspace(
        &mut self,
        workspace: &str,
        request: WorkspaceOnboardRequest,
    ) -> Result<WorkspaceOnboarded, ProtocolError> {
        let opened = PathBuf::from(&request.opened_directory);
        let memory = PathBuf::from(&request.memory_location);
        if !opened.is_absolute() || !memory.is_absolute() {
            return Err(ProtocolError::invalid_request());
        }
        let instructions = crate::onboard_companion_workspace(&opened, &memory)
            .map_err(|_| ProtocolError::persistence_failed())?;
        let opened_canonical = opened
            .canonicalize()
            .map_err(|_| ProtocolError::persistence_failed())?;
        let memory_canonical = memory
            .canonicalize()
            .map_err(|_| ProtocolError::persistence_failed())?;
        let mut contexts = self
            .workspace_contexts
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        let identity = self
            .client_identity
            .as_ref()
            .ok_or_else(ProtocolError::unauthorized)?;
        contexts.record(identity, workspace, opened_canonical, instructions.clone());
        contexts.record(identity, workspace, memory_canonical, instructions.clone());
        Ok(WorkspaceOnboarded {
            opened_directory: opened.to_string_lossy().into_owned(),
            memory_location: memory.to_string_lossy().into_owned(),
            instructions,
        })
    }

    fn ensure_home(&mut self) -> Result<(), ProtocolError> {
        crate::ensure_cross_project_home(&self.home)
            .map_err(|_| ProtocolError::persistence_failed())
    }

    fn control_migration(
        &mut self,
        request: MigrationControlRequest,
        provenance: CompanionProvenance,
    ) -> Result<(), ProtocolError> {
        self.boundaries
            .control_migration(request, provenance.peer_pid)
    }

    fn authorized_workspace(&self, session_workspace: &str, workspace: &str) -> Option<String> {
        let Some(identity) = &self.client_identity else {
            return None;
        };
        let canonical = PathBuf::from(workspace).canonicalize().ok()?;
        self.workspace_contexts
            .lock()
            .ok()?
            .authorized_directory(identity, session_workspace, &canonical)
            .map(|directory| directory.to_string_lossy().into_owned())
    }

    fn list_threads(
        &mut self,
        workspace: &str,
        request: ThreadListRequest,
    ) -> Result<ThreadListPage, ProtocolError> {
        self.boundaries.list_threads(workspace, request)
    }

    fn open_thread(
        &mut self,
        workspace: &str,
        request: ThreadOpenRequest,
    ) -> Result<ThreadOpenPage, ProtocolError> {
        self.boundaries.open_thread(workspace, request)
    }

    fn create_thread(
        &mut self,
        workspace: &str,
        request_id: &Id,
        idempotency_key: &Id,
        companion: CompanionProvenance,
    ) -> Result<ThreadCreateAccepted, ProtocolError> {
        let canonical_input = json!({"workspace": workspace});
        let ledger_request = AttachRequest {
            protocol: Protocol,
            request_id: request_id.clone(),
            operation: Operation::ThreadCreate,
            capability: String::new(),
            idempotency_key: Some(idempotency_key.clone()),
            body: canonical_input.clone(),
        };
        let mut extra = BTreeMap::new();
        extra.insert("attach_profile".into(), json!(&companion.profile));
        extra.insert("companion_kind".into(), json!(companion.companion_kind));
        extra.insert(
            "companion_version".into(),
            json!(companion.companion_version),
        );
        extra.insert("peer_uid".into(), json!(companion.peer_uid));
        extra.insert("peer_pid".into(), json!(companion.peer_pid));
        extra.insert("idempotency_key".into(), json!(idempotency_key.as_str()));
        let provenance = Provenance {
            source: "muniment-attach".into(),
            source_version: env!("CARGO_PKG_VERSION").into(),
            actor_id: None,
            device_id: None,
            rpc_request_id: Some(request_id.as_str().to_owned()),
            capability_versions: None,
            extra,
        };
        let outcome = self.idempotency.execute(
            &companion.profile,
            &ledger_request,
            &canonical_input,
            || Ok(()),
            || {
                let thread_id = self.boundaries.create_thread(workspace, provenance)?;
                Ok(CommittedResult {
                    body: json!({"thread_id": thread_id}),
                    cursor: None,
                })
            },
        )?;
        let committed = match outcome {
            IdempotencyOutcome::Committed(result) | IdempotencyOutcome::Replayed(result) => result,
        };
        serde_json::from_value(committed.body).map_err(|_| ProtocolError::persistence_failed())
    }

    fn start_run(
        &mut self,
        workspace: &str,
        execution_root: &str,
        request: AttachRunStartRequest,
        request_id: &Id,
        idempotency_key: &Id,
        companion: CompanionProvenance,
    ) -> Result<RunStartAccepted, ProtocolError> {
        if request.context.is_some() {
            return Err(ProtocolError::unsupported_operation());
        }
        let canonical_input = json!({
            "workspace": workspace,
            "execution_root": execution_root,
            "text": &request.text,
            "context": &request.context,
            "thread_id": &request.thread_id,
        });
        let ledger_request = AttachRequest {
            protocol: Protocol,
            request_id: request_id.clone(),
            operation: Operation::RunStart,
            capability: String::new(),
            idempotency_key: Some(idempotency_key.clone()),
            body: canonical_input.clone(),
        };
        let mut extra = BTreeMap::new();
        let profile = companion.profile.clone();
        extra.insert("attach_profile".into(), json!(&companion.profile));
        extra.insert("companion_kind".into(), json!(companion.companion_kind));
        extra.insert(
            "companion_version".into(),
            json!(companion.companion_version),
        );
        extra.insert("peer_uid".into(), json!(companion.peer_uid));
        extra.insert("peer_pid".into(), json!(companion.peer_pid));
        extra.insert("idempotency_key".into(), json!(idempotency_key.as_str()));
        let instructions = self
            .workspace_contexts
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?
            .instructions(
                self.client_identity
                    .as_ref()
                    .ok_or_else(ProtocolError::unauthorized)?,
                workspace,
                &PathBuf::from(execution_root),
            )
            .map(str::to_owned);
        if let Some(instructions) = instructions {
            extra.insert("repository_instructions".into(), json!(instructions));
        }
        let provenance = Provenance {
            source: "muniment-attach".into(),
            source_version: env!("CARGO_PKG_VERSION").into(),
            actor_id: None,
            device_id: None,
            rpc_request_id: Some(request_id.as_str().to_owned()),
            capability_versions: None,
            extra,
        };
        let mut pending_launch = None;
        let outcome = self.idempotency.execute(
            &profile,
            &ledger_request,
            &canonical_input,
            || Ok(()),
            || {
                let (result, launch) = prepare_desktop_run(
                    &self.boundaries,
                    RunStartRequest {
                        prompt: request.text,
                        files: Vec::new(),
                        workspace: Some(workspace.to_owned()),
                        provenance: Some(provenance),
                        thread_id: request.thread_id,
                    },
                )
                .map_err(|error| error.protocol_error())?;
                pending_launch = Some(launch);
                let thread_id = self
                    .boundaries
                    .run_thread_id(&result.run_id)
                    .map_err(|error| error.protocol_error())?;
                Ok(CommittedResult {
                    body: json!({
                        "run_id": result.run_id,
                        "thread_id": thread_id,
                        "committed_seq": result.committed_seq,
                        "accepted_at": result.accepted_at,
                    }),
                    cursor: None,
                })
            },
        );
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => {
                if let Some(launch) = pending_launch {
                    let _ = self.boundaries.fail_prepared_run(&launch);
                    self.boundaries.clear_active_run(&launch.run_id);
                }
                return Err(error);
            }
        };
        let committed = match outcome {
            IdempotencyOutcome::Committed(result) => {
                let launch = pending_launch.ok_or_else(ProtocolError::persistence_failed)?;
                self.boundaries.launch(launch);
                result
            }
            IdempotencyOutcome::Replayed(result) => result,
        };
        serde_json::from_value(committed.body).map_err(|_| ProtocolError::persistence_failed())
    }

    fn answer_permission(
        &mut self,
        workspace: &str,
        request: PermissionAnswerRequest,
        request_id: &Id,
        idempotency_key: &Id,
        companion: CompanionProvenance,
    ) -> Result<PermissionAnswerAccepted, ProtocolError> {
        let canonical_input = json!({
            "workspace": workspace,
            "run_id": &request.run_id,
            "gate_id": &request.gate_id,
            "decision": request.decision,
        });
        let ledger_request = AttachRequest {
            protocol: Protocol,
            request_id: request_id.clone(),
            operation: Operation::PermissionAnswer,
            capability: String::new(),
            idempotency_key: Some(idempotency_key.clone()),
            body: canonical_input.clone(),
        };
        let outcome = self.idempotency.execute(
            &companion.profile,
            &ledger_request,
            &canonical_input,
            || Ok(()),
            || {
                let (high_water, commits) =
                    self.boundaries.subscribe_run_commits(&request.run_id)?;
                if high_water == 0 {
                    return Err(ProtocolError::invalid_request());
                }
                let page = self.boundaries.stream_run(
                    workspace,
                    &request.run_id,
                    high_water.saturating_sub(1),
                )?;
                let pending = page.events.last().and_then(|event| {
                    (event.event_type == "permission.requested")
                        .then_some(event.pending_permission.as_ref())
                        .flatten()
                });
                if pending.is_none_or(|gate| !gate.valid || gate.gate_id != request.gate_id) {
                    return Err(ProtocolError::invalid_request());
                }
                let answer = match request.decision {
                    PermissionDecision::Allow => ChatPermissionAnswer::Confirm(true),
                    PermissionDecision::Deny => ChatPermissionAnswer::Confirm(false),
                };
                let resolved = self
                    .boundaries
                    .queue_attach_permission_answer(
                        workspace,
                        &request.run_id,
                        &request.gate_id,
                        answer,
                    )
                    .map_err(|error| error.protocol_error())?;
                let deadline = std::time::Instant::now() + PERMISSION_COMMIT_TIMEOUT;
                let remaining = deadline
                    .checked_duration_since(std::time::Instant::now())
                    .ok_or_else(ProtocolError::persistence_failed)?;
                let expected_seq = resolved
                    .recv_timeout(remaining)
                    .map_err(|_| ProtocolError::persistence_failed())?
                    .ok_or_else(ProtocolError::invalid_request)?;
                let committed_seq = loop {
                    let remaining = deadline
                        .checked_duration_since(std::time::Instant::now())
                        .ok_or_else(ProtocolError::persistence_failed)?;
                    let hint = commits
                        .recv_timeout(remaining)
                        .map_err(|_| ProtocolError::persistence_failed())?;
                    if hint.run_id != request.run_id || hint.run_seq != expected_seq {
                        continue;
                    }
                    let page = self.boundaries.stream_run(
                        workspace,
                        &request.run_id,
                        hint.run_seq.saturating_sub(1),
                    )?;
                    if page.events.first().is_some_and(|event| {
                        event.run_seq == hint.run_seq && event.event_type == "permission.resolved"
                    }) {
                        break hint.run_seq;
                    }
                };
                Ok(CommittedResult {
                    body: json!({
                        "run_id": request.run_id,
                        "gate_id": request.gate_id,
                        "decision": request.decision,
                        "committed_seq": committed_seq,
                        "accepted_at": chrono::Utc::now().to_rfc3339_opts(
                            chrono::SecondsFormat::AutoSi,
                            true,
                        ),
                    }),
                    cursor: None,
                })
            },
        )?;
        let committed = match outcome {
            IdempotencyOutcome::Committed(result) | IdempotencyOutcome::Replayed(result) => result,
        };
        serde_json::from_value(committed.body).map_err(|_| ProtocolError::persistence_failed())
    }

    fn cancel_run(
        &mut self,
        workspace: &str,
        request: RunCancelRequest,
        request_id: &Id,
        idempotency_key: &Id,
        companion: CompanionProvenance,
    ) -> Result<RunCancelAccepted, ProtocolError> {
        let canonical_input = json!({"workspace": workspace, "run_id": &request.run_id});
        let ledger_request = AttachRequest {
            protocol: Protocol,
            request_id: request_id.clone(),
            operation: Operation::RunCancel,
            capability: String::new(),
            idempotency_key: Some(idempotency_key.clone()),
            body: canonical_input.clone(),
        };
        let outcome = self.idempotency.execute(
            &companion.profile,
            &ledger_request,
            &canonical_input,
            || Ok(()),
            || {
                self.boundaries
                    .cancel_run(workspace, &request.run_id)
                    .map_err(|error| error.protocol_error())?;
                Ok(CommittedResult {
                    body: json!({
                        "run_id": request.run_id,
                        "accepted_at": chrono::Utc::now().to_rfc3339_opts(
                            chrono::SecondsFormat::AutoSi,
                            true,
                        ),
                    }),
                    cursor: None,
                })
            },
        )?;
        let committed = match outcome {
            IdempotencyOutcome::Committed(result) | IdempotencyOutcome::Replayed(result) => result,
        };
        serde_json::from_value(committed.body).map_err(|_| ProtocolError::persistence_failed())
    }

    fn stream_run(
        &mut self,
        workspace: &str,
        run_id: &str,
        after_run_seq: u64,
    ) -> Result<RunStreamPage, ProtocolError> {
        self.boundaries.stream_run(workspace, run_id, after_run_seq)
    }

    fn subscribe_run_commits(
        &mut self,
        run_id: &str,
    ) -> Result<Option<(u64, std::sync::mpsc::Receiver<JournalCommitHint>)>, ProtocolError> {
        self.boundaries.subscribe_run_commits(run_id).map(Some)
    }
}
