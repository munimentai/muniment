use muniment_core::attach::ProtocolError;
use std::collections::HashMap;
#[cfg(target_os = "linux")]
use std::collections::{BTreeMap, BTreeSet};
#[cfg(target_os = "linux")]
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::sync::Arc;
use std::sync::Mutex;
#[cfg(target_os = "linux")]
use std::time::Duration;

#[cfg(all(target_os = "linux", test))]
use muniment_core::attach::linux::run_authenticated_session_with_service_and_approvals;
#[cfg(target_os = "linux")]
use muniment_core::attach::linux::{
    approval_waiter_with_claims, run_authenticated_session_with_service_approvals_and_registry,
    AttachAcceptError, AttachFilesystem, AttachTransport, CompanionProvenance,
    LiveConnectionRegistry, PermissionAnswerAccepted, PermissionAnswerRequest, PermissionDecision,
    RunCancelAccepted, RunCancelRequest, RunStartAccepted,
    RunStartRequest as AttachRunStartRequest, RunStreamPage, ThreadCreateAccepted, ThreadListPage,
    ThreadListRequest, ThreadListService, ThreadOpenPage, ThreadOpenRequest,
};
#[cfg(target_os = "linux")]
use muniment_core::attach::{
    Approval, CommittedResult, Id, IdempotencyOutcome, IdempotencyStore, Operation, Protocol,
    Request as AttachRequest, WorkspaceOnboardRequest, WorkspaceOnboarded,
};
#[cfg(target_os = "linux")]
use muniment_core::journal::Provenance;
#[cfg(target_os = "linux")]
use serde_json::{json, Value};
#[cfg(target_os = "linux")]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
#[cfg(target_os = "linux")]
use tauri::{Emitter, Manager};
#[cfg(target_os = "linux")]
use uuid::Uuid;

#[cfg(target_os = "linux")]
use crate::chat::{
    prepare_desktop_run, ChatPermissionAnswer, RunStartBoundaries, RunStartRequest,
    TauriRunStartBoundaries,
};

#[cfg(all(target_os = "linux", not(test)))]
const PERMISSION_COMMIT_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(all(target_os = "linux", test))]
const PERMISSION_COMMIT_TIMEOUT: Duration = Duration::from_millis(50);

/// Production adapter from the authorized Linux attach seam into the desktop
/// coordinator. The listener lifecycle will own this service in a later slice.
#[cfg(target_os = "linux")]
trait RunStartIdempotency {
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

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
pub struct DesktopAttachService<B, I = IdempotencyStore> {
    boundaries: B,
    idempotency: I,
    home: PathBuf,
    workspace_contexts: Arc<Mutex<HashMap<String, HashMap<PathBuf, Option<String>>>>>,
    client_credentials: Arc<Mutex<HashMap<String, ClientCredential>>>,
    credential_path: Option<PathBuf>,
    client_identity: Option<String>,
}

#[cfg(target_os = "linux")]
pub(crate) struct AttachListenerState {
    workspace_contexts: Arc<Mutex<HashMap<String, HashMap<PathBuf, Option<String>>>>>,
    client_credentials: Arc<Mutex<HashMap<String, ClientCredential>>>,
    credential_path: PathBuf,
    live_connections: LiveConnectionRegistry,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClientCredential {
    credential: String,
    claimed_kind: String,
    claimed_version: String,
    #[serde(deserialize_with = "deserialize_approval_time")]
    approved_at: Option<String>,
}

#[cfg(target_os = "linux")]
fn deserialize_approval_time<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    <Option<String> as serde::Deserialize>::deserialize(deserializer)
}

#[cfg(target_os = "linux")]
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct ClientCredentialStore {
    version: u32,
    companions: HashMap<String, ClientCredential>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct AuthorizedCompanion {
    pub(crate) identity: String,
    pub(crate) claimed_kind: String,
    pub(crate) claimed_version: String,
    pub(crate) approved_at: Option<String>,
}

pub struct AttachCompanionState {
    #[cfg(target_os = "linux")]
    listener: Arc<AttachListenerState>,
}

#[cfg(target_os = "linux")]
impl AttachCompanionState {
    fn new(listener: Arc<AttachListenerState>) -> Self {
        Self { listener }
    }
}

#[cfg(not(target_os = "linux"))]
impl Default for AttachCompanionState {
    fn default() -> Self {
        Self {}
    }
}

#[tauri::command]
pub fn attach_companions(
    state: tauri::State<'_, AttachCompanionState>,
) -> Result<Vec<AuthorizedCompanion>, ProtocolError> {
    #[cfg(target_os = "linux")]
    return state.listener.list_companions();

    #[cfg(not(target_os = "linux"))]
    {
        let _ = state;
        Ok(Vec::new())
    }
}

#[tauri::command]
pub fn attach_revoke_companion(
    state: tauri::State<'_, AttachCompanionState>,
    client_identity: String,
) -> Result<(), ProtocolError> {
    #[cfg(target_os = "linux")]
    return state.listener.revoke_companion(&client_identity);

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (state, client_identity);
        Err(ProtocolError::unsupported_operation())
    }
}

#[cfg(target_os = "linux")]
impl AttachListenerState {
    fn load(credential_path: &std::path::Path) -> Result<Self, ProtocolError> {
        Ok(Self {
            workspace_contexts: Arc::new(Mutex::new(HashMap::new())),
            client_credentials: Arc::new(Mutex::new(load_client_credentials(credential_path)?)),
            credential_path: credential_path.to_owned(),
            live_connections: LiveConnectionRegistry::default(),
        })
    }

    pub(crate) fn revoke_companion(&self, client_identity: &str) -> Result<(), ProtocolError> {
        let mut credentials = self
            .client_credentials
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        let credential = credentials
            .get(client_identity)
            .cloned()
            .ok_or_else(ProtocolError::unauthorized)?;
        self.live_connections.block(&credential.credential);
        credentials.remove(client_identity);
        if let Err(error) = persist_client_credentials(&self.credential_path, &credentials) {
            credentials.insert(client_identity.to_owned(), credential.clone());
            self.live_connections.resume(&credential.credential);
            return Err(error);
        }
        self.live_connections.revoke(&credential.credential);
        Ok(())
    }

    pub(crate) fn list_companions(&self) -> Result<Vec<AuthorizedCompanion>, ProtocolError> {
        let credentials = self
            .client_credentials
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        let mut companions: Vec<_> = credentials
            .iter()
            .map(|(identity, entry)| AuthorizedCompanion {
                identity: identity.clone(),
                claimed_kind: entry.claimed_kind.clone(),
                claimed_version: entry.claimed_version.clone(),
                approved_at: entry.approved_at.clone(),
            })
            .collect();
        companions.sort_by(|left, right| left.identity.cmp(&right.identity));
        Ok(companions)
    }
}

#[derive(Default)]
pub struct AttachApprovalState {
    pending: Mutex<HashMap<String, std::sync::mpsc::SyncSender<bool>>>,
}

#[cfg(target_os = "linux")]
#[derive(serde::Serialize)]
struct AttachPairingRequest {
    challenge: String,
    claimed_kind: String,
    claimed_version: String,
}

#[cfg(target_os = "linux")]
fn bounded_claim(claim: &str) -> String {
    const MAX_CLAIM_LENGTH: usize = 80;
    if claim.chars().any(char::is_control)
        || claim.trim().is_empty()
        || claim.chars().take(MAX_CLAIM_LENGTH + 1).count() > MAX_CLAIM_LENGTH
    {
        "unknown".into()
    } else {
        claim.into()
    }
}

#[tauri::command]
pub fn attach_pairing_decide(
    state: tauri::State<'_, AttachApprovalState>,
    challenge: String,
    approve: bool,
) {
    if let Ok(mut pending) = state.pending.lock() {
        if let Some(sender) = pending.remove(&challenge) {
            let _ = sender.try_send(approve);
        }
    }
}

#[cfg(target_os = "linux")]
fn resolve_attach_home(
    documents: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Result<PathBuf, ProtocolError> {
    crate::home::choose_default_home(documents, home)
        .map_err(|_| ProtocolError::persistence_failed())
}

#[cfg(target_os = "linux")]
impl<R: tauri::Runtime> DesktopAttachService<TauriRunStartBoundaries<R>> {
    pub fn new(
        app: tauri::AppHandle<R>,
        workspace_contexts: Arc<Mutex<HashMap<String, HashMap<PathBuf, Option<String>>>>>,
        client_credentials: Arc<Mutex<HashMap<String, ClientCredential>>>,
    ) -> Result<Self, ProtocolError> {
        let home = resolve_attach_home(app.path().document_dir().ok(), app.path().home_dir().ok())?;
        let idempotency = IdempotencyStore::open(
            app.path()
                .app_data_dir()
                .map_err(|_| ProtocolError::persistence_failed())?
                .join("attach-idempotency.sqlite3"),
        )?;
        let credential_path = app
            .path()
            .app_data_dir()
            .map_err(|_| ProtocolError::persistence_failed())?
            .join("attach-client-credentials.json");
        Ok(Self {
            boundaries: TauriRunStartBoundaries {
                app,
                continue_session_thread: false,
            },
            idempotency,
            home,
            workspace_contexts,
            client_credentials,
            credential_path: Some(credential_path),
            client_identity: None,
        })
    }
}

#[cfg(target_os = "linux")]
fn desktop_attach_approval() -> Option<Approval> {
    Some(Approval {
        profile: "desktop-owner".into(),
        workspace: std::env::current_dir().ok()?.to_string_lossy().into_owned(),
        scopes: BTreeSet::from(["thread.read".into(), "run.write".into()]),
        lifetime: Duration::from_secs(60 * 60),
    })
}

#[cfg(target_os = "linux")]
fn should_retry_attach_accept(error: AttachAcceptError) -> bool {
    matches!(
        error,
        AttachAcceptError::Accept
            | AttachAcceptError::PeerCredentials
            | AttachAcceptError::WrongUid(_)
    )
}

#[cfg(target_os = "linux")]
pub fn start_attach_listener<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    let credential_path = match app.path().app_data_dir() {
        Ok(path) => path.join("attach-client-credentials.json"),
        Err(_) => return,
    };
    let state = match AttachListenerState::load(&credential_path) {
        Ok(state) => Arc::new(state),
        Err(_) => return,
    };
    app.manage(AttachCompanionState::new(state.clone()));
    std::thread::spawn(move || {
        let Ok(filesystem) = AttachFilesystem::from_environment() else {
            return;
        };
        let Ok(_instance_lock) = filesystem.acquire_instance_lock() else {
            return;
        };
        let Ok(listener) = AttachTransport::bind(&filesystem) else {
            return;
        };
        loop {
            let (stream, credentials) = match listener.accept() {
                Ok(accepted) => accepted,
                Err(error) if should_retry_attach_accept(error) => {
                    if error == AttachAcceptError::Accept {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    continue;
                }
                Err(_) => break,
            };
            let app = app.clone();
            let workspace_contexts = state.workspace_contexts.clone();
            let client_credentials = state.client_credentials.clone();
            let live_connections = state.live_connections.clone();
            std::thread::spawn(move || {
                let Ok(mut service) =
                    DesktopAttachService::new(app, workspace_contexts, client_credentials)
                else {
                    return;
                };
                let approval_app = service.boundaries.app.clone();
                let _ = run_authenticated_session_with_service_approvals_and_registry(
                    stream,
                    credentials,
                    env!("CARGO_PKG_VERSION"),
                    &mut service,
                    approval_waiter_with_claims(
                        move |challenge: &muniment_core::attach::PairingChallenge,
                              claimed_kind: &str,
                              claimed_version: &str,
                              remaining: Duration| {
                            let challenge = challenge.as_str().to_owned();
                            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
                            let state = approval_app.state::<AttachApprovalState>();
                            state.pending.lock().ok()?.insert(challenge.clone(), sender);
                            let request = AttachPairingRequest {
                                challenge: challenge.clone(),
                                claimed_kind: bounded_claim(claimed_kind),
                                claimed_version: bounded_claim(claimed_version),
                            };
                            if approval_app
                                .emit("attach-pairing-requested", &request)
                                .is_err()
                            {
                                state.pending.lock().ok()?.remove(&challenge);
                                return Some(muniment_core::attach::linux::ApprovalDecision::Deny);
                            }
                            let approved = receiver.recv_timeout(remaining).unwrap_or(false);
                            state.pending.lock().ok()?.remove(&challenge);
                            if !approved {
                                return Some(muniment_core::attach::linux::ApprovalDecision::Deny);
                            }
                            Some(muniment_core::attach::linux::ApprovalDecision::Approve(
                                desktop_attach_approval()?,
                            ))
                        },
                    ),
                    &live_connections,
                );
            });
        }
    });
}

#[cfg(target_os = "linux")]
impl<B: RunStartBoundaries, I: RunStartIdempotency> ThreadListService
    for DesktopAttachService<B, I>
{
    fn bind_authorized_client(&mut self, client_identity: &str) {
        self.client_identity = Some(client_identity.to_owned());
    }

    fn reconnect_approval(&self) -> Option<Approval> {
        desktop_attach_approval()
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
        request: WorkspaceOnboardRequest,
    ) -> Result<WorkspaceOnboarded, ProtocolError> {
        let opened = PathBuf::from(&request.opened_directory);
        let memory = PathBuf::from(&request.memory_location);
        if !opened.is_absolute() || !memory.is_absolute() {
            return Err(ProtocolError::invalid_request());
        }
        let instructions = muniment_core::onboard_companion_workspace(&opened, &memory)
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
        let contexts = contexts.entry(identity.clone()).or_default();
        contexts.insert(opened_canonical, instructions.clone());
        contexts.insert(memory_canonical, instructions.clone());
        Ok(WorkspaceOnboarded {
            opened_directory: opened.to_string_lossy().into_owned(),
            memory_location: memory.to_string_lossy().into_owned(),
            instructions,
        })
    }

    fn ensure_home(&mut self) -> Result<(), ProtocolError> {
        muniment_core::ensure_cross_project_home(&self.home)
            .map_err(|_| ProtocolError::persistence_failed())
    }

    fn authorized_workspace(&self, workspace: &str) -> Option<String> {
        let Some(identity) = &self.client_identity else {
            return None;
        };
        let canonical = PathBuf::from(workspace).canonicalize().ok()?;
        self.workspace_contexts.lock().ok().and_then(|contexts| {
            contexts.get(identity).and_then(|workspaces| {
                workspaces
                    .contains_key(&canonical)
                    .then(|| canonical.to_string_lossy().into_owned())
            })
        })
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
            .get(
                self.client_identity
                    .as_ref()
                    .ok_or_else(ProtocolError::unauthorized)?,
            )
            .and_then(|contexts| contexts.get(&PathBuf::from(workspace)))
            .cloned()
            .flatten();
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
    ) -> Result<
        Option<(
            u64,
            std::sync::mpsc::Receiver<muniment_core::journal::JournalCommitHint>,
        )>,
        ProtocolError,
    > {
        self.boundaries.subscribe_run_commits(run_id).map(Some)
    }
}

#[cfg(target_os = "linux")]
fn load_client_credentials(
    path: &std::path::Path,
) -> Result<HashMap<String, ClientCredential>, ProtocolError> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true).custom_flags(libc::O_NOFOLLOW);
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(_) => return Err(ProtocolError::persistence_failed()),
    };
    let metadata = file
        .metadata()
        .map_err(|_| ProtocolError::persistence_failed())?;
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
        return Err(ProtocolError::persistence_failed());
    }
    let value: Value =
        serde_json::from_reader(file).map_err(|_| ProtocolError::persistence_failed())?;
    let versioned = value.get("version").is_some();
    let credentials = if versioned {
        let store: ClientCredentialStore =
            serde_json::from_value(value).map_err(|_| ProtocolError::persistence_failed())?;
        if store.version != 1 {
            return Err(ProtocolError::persistence_failed());
        }
        store.companions
    } else {
        let legacy: HashMap<String, String> =
            serde_json::from_value(value).map_err(|_| ProtocolError::persistence_failed())?;
        legacy
            .into_iter()
            .map(|(identity, credential)| {
                (
                    identity,
                    ClientCredential {
                        credential,
                        claimed_kind: "unknown".into(),
                        claimed_version: "unknown".into(),
                        approved_at: None,
                    },
                )
            })
            .collect()
    };
    if credentials.iter().any(|(identity, entry)| {
        Id::new(identity).is_err()
            || entry.credential.len() != 64
            || !entry
                .credential
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || entry.claimed_kind != bounded_claim(&entry.claimed_kind)
            || entry.claimed_version != bounded_claim(&entry.claimed_version)
            || entry.approved_at.as_ref().is_some_and(|approved_at| {
                chrono::DateTime::parse_from_rfc3339(approved_at)
                    .map(|time| time.offset().local_minus_utc() != 0)
                    .unwrap_or(true)
            })
    }) {
        return Err(ProtocolError::persistence_failed());
    }
    Ok(credentials)
}

#[cfg(target_os = "linux")]
fn persist_client_credentials(
    path: &std::path::Path,
    credentials: &HashMap<String, ClientCredential>,
) -> Result<(), ProtocolError> {
    let parent = path
        .parent()
        .ok_or_else(ProtocolError::persistence_failed)?;
    std::fs::create_dir_all(parent).map_err(|_| ProtocolError::persistence_failed())?;
    let temporary = parent.join(format!(".attach-client-credentials-{}.tmp", Uuid::now_v7()));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW);
        let file = options
            .open(&temporary)
            .map_err(|_| ProtocolError::persistence_failed())?;
        serde_json::to_writer(
            &file,
            &ClientCredentialStore {
                version: 1,
                companions: credentials.clone(),
            },
        )
        .map_err(|_| ProtocolError::persistence_failed())?;
        file.sync_all()
            .map_err(|_| ProtocolError::persistence_failed())?;
        std::fs::rename(&temporary, path).map_err(|_| ProtocolError::persistence_failed())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{append_test_event, FakeRunStartBoundaries};
    use muniment_core::attach::ErrorCode;
    use muniment_core::journal::reducer::reduce;
    use muniment_core::journal::RunJournal;

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_claims_share_the_prompt_and_store_bounds() {
        assert_eq!(bounded_claim(&"x".repeat(80)), "x".repeat(80));
        for claim in ["", "   ", "cli\nspoof", &"x".repeat(81)] {
            assert_eq!(bounded_claim(claim), "unknown");
        }
    }
    use std::sync::atomic::Ordering;

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_accept_errors_retry_unless_listener_is_closed() {
        use muniment_core::attach::linux::PeerCredentials;

        let credentials = PeerCredentials {
            pid: 42,
            uid: 1001,
            gid: 1001,
        };
        let cases = [
            (AttachAcceptError::Closed, false),
            (AttachAcceptError::Accept, true),
            (AttachAcceptError::PeerCredentials, true),
            (AttachAcceptError::WrongUid(credentials), true),
        ];

        for (error, expected) in cases {
            assert_eq!(should_retry_attach_accept(error), expected, "{error:?}");
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn companion_commands_list_revoke_and_reject_unknown_identity() {
        let root = std::env::temp_dir().join(format!(
            "muniment-attach-companion-commands-{}",
            Uuid::now_v7()
        ));
        let credential_path = root.join("credentials.json");
        let identity = "018f0000-0000-7000-8000-000000000001";
        let approved_at = "2026-08-04T12:00:00Z";
        let credentials = HashMap::from([(
            identity.into(),
            ClientCredential {
                credential: "ab".repeat(32),
                claimed_kind: "cli".into(),
                claimed_version: "1.2.3".into(),
                approved_at: Some(approved_at.into()),
            },
        )]);
        persist_client_credentials(&credential_path, &credentials).unwrap();

        let app = tauri::test::mock_app();
        let listener = Arc::new(AttachListenerState::load(&credential_path).unwrap());
        app.manage(AttachCompanionState::new(listener));

        assert_eq!(
            attach_companions(app.state()).unwrap(),
            vec![AuthorizedCompanion {
                identity: identity.into(),
                claimed_kind: "cli".into(),
                claimed_version: "1.2.3".into(),
                approved_at: Some(approved_at.into()),
            }]
        );
        attach_revoke_companion(app.state(), identity.into()).unwrap();
        assert!(attach_companions(app.state()).unwrap().is_empty());
        assert_eq!(
            attach_revoke_companion(app.state(), "unknown".into())
                .unwrap_err()
                .code(),
            ErrorCode::Unauthorized
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_home_uses_home_once_when_documents_directory_is_absent() {
        let root = std::env::temp_dir().join(format!("muniment-attach-home-{}", Uuid::now_v7()));
        std::fs::create_dir(&root).unwrap();

        assert_eq!(
            resolve_attach_home(None, Some(root.clone())).unwrap(),
            root.join("Muniment")
        );

        std::fs::remove_dir(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    struct FailingFinalization;

    #[cfg(target_os = "linux")]
    impl RunStartIdempotency for FailingFinalization {
        fn execute<A, W>(
            &mut self,
            _profile: &str,
            _request: &AttachRequest,
            _canonical_input: &Value,
            authorize: A,
            work: W,
        ) -> Result<IdempotencyOutcome, ProtocolError>
        where
            A: FnOnce() -> Result<(), ProtocolError>,
            W: FnOnce() -> Result<CommittedResult, ProtocolError>,
        {
            authorize()?;
            let _ = work()?;
            Err(ProtocolError::persistence_failed())
        }
    }

    #[cfg(target_os = "linux")]
    fn attach_start(
        boundaries: FakeRunStartBoundaries,
        context: Option<Value>,
    ) -> (
        Result<RunStartAccepted, ProtocolError>,
        DesktopAttachService<FakeRunStartBoundaries>,
    ) {
        let mut service = DesktopAttachService {
            boundaries,
            idempotency: IdempotencyStore::open(":memory:").unwrap(),
            home: PathBuf::new(),
            workspace_contexts: Arc::new(Mutex::new(HashMap::new())),
            client_credentials: Arc::new(Mutex::new(HashMap::new())),
            credential_path: None,
            client_identity: Some("default".into()),
        };
        let result = service.start_run(
            "workspace-a",
            AttachRunStartRequest {
                text: "hello".into(),
                context,
                thread_id: None,
            },
            &Id::new("018f0000-0000-7000-8000-000000000001").unwrap(),
            &Id::new("018f0000-0000-7000-8000-000000000002").unwrap(),
            CompanionProvenance {
                profile: "default".into(),
                companion_kind: "cli".into(),
                companion_version: "1.2.3".into(),
                peer_uid: 1000,
                peer_pid: 42,
            },
        );
        (result, service)
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_thread_create_replays_and_rejects_changed_workspace() {
        let database_path = std::env::temp_dir().join(format!(
            "muniment-attach-thread-create-{}.sqlite3",
            Uuid::now_v7()
        ));
        let mut boundaries = FakeRunStartBoundaries::accepting();
        boundaries.journal = Mutex::new(RunJournal::open(&database_path).unwrap());
        let mut service = DesktopAttachService {
            boundaries,
            idempotency: IdempotencyStore::open(":memory:").unwrap(),
            home: PathBuf::new(),
            workspace_contexts: Arc::new(Mutex::new(HashMap::new())),
            client_credentials: Arc::new(Mutex::new(HashMap::new())),
            credential_path: None,
            client_identity: Some("default".into()),
        };
        let key = Id::new("018f0000-0000-7000-8000-000000000002").unwrap();
        let companion = CompanionProvenance {
            profile: "default".into(),
            companion_kind: "cli".into(),
            companion_version: "1.2.3".into(),
            peer_uid: 1000,
            peer_pid: 42,
        };

        let first = service
            .create_thread(
                "workspace-a",
                &Id::new("018f0000-0000-7000-8000-000000000001").unwrap(),
                &key,
                companion.clone(),
            )
            .unwrap();
        let replay = service
            .create_thread(
                "workspace-a",
                &Id::new("018f0000-0000-7000-8000-000000000003").unwrap(),
                &key,
                companion.clone(),
            )
            .unwrap();
        let conflict = service
            .create_thread(
                "workspace-b",
                &Id::new("018f0000-0000-7000-8000-000000000004").unwrap(),
                &key,
                companion,
            )
            .unwrap_err();

        assert_eq!(replay.thread_id, first.thread_id);
        assert_eq!(conflict.code(), ErrorCode::IdempotencyConflict);
        let mut journal = service.boundaries.journal.lock().unwrap();
        let event_count = rusqlite::Connection::open(&database_path)
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM thread_events WHERE event_type='thread.created'",
                [],
                |row| row.get::<_, u64>(0),
            )
            .unwrap();
        assert_eq!(event_count, 1);
        let page = ThreadListService::list_threads(
            &mut *journal,
            "workspace-a",
            ThreadListRequest {
                cursor: None,
                limit: 50,
            },
        )
        .unwrap();
        assert!(page.threads.is_empty());
        let other_page = ThreadListService::list_threads(
            &mut *journal,
            "workspace-b",
            ThreadListRequest {
                cursor: None,
                limit: 50,
            },
        )
        .unwrap();
        assert!(other_page.threads.is_empty());
        drop(journal);
        drop(service);
        std::fs::remove_file(database_path).unwrap();
    }

    #[cfg(target_os = "linux")]
    fn cancel_request(
        service: &mut DesktopAttachService<FakeRunStartBoundaries>,
        workspace: &str,
        run_id: &str,
        request_id: &str,
        key: &str,
    ) -> Result<RunCancelAccepted, ProtocolError> {
        service.cancel_run(
            workspace,
            RunCancelRequest {
                run_id: run_id.into(),
            },
            &Id::new(request_id).unwrap(),
            &Id::new(key).unwrap(),
            CompanionProvenance {
                profile: "default".into(),
                companion_kind: "cli".into(),
                companion_version: "1.2.3".into(),
                peer_uid: 1000,
                peer_pid: 42,
            },
        )
    }

    #[cfg(target_os = "linux")]
    fn permission_service(
        boundaries: FakeRunStartBoundaries,
        active: Option<(&str, &str)>,
    ) -> DesktopAttachService<FakeRunStartBoundaries> {
        let run_id = "0190a100-0000-7000-8000-000000000001";
        let started = crate::chat::event_envelope(run_id, 1, "run.started", json!({}), None);
        boundaries
            .journal
            .lock()
            .unwrap()
            .append_new_run("workspace-a", &started)
            .unwrap();
        append_test_event(
            &mut boundaries.journal.lock().unwrap(),
            run_id,
            2,
            "permission.requested",
            json!({
                "gate_id": "gate-1",
                "kind": "confirm",
                "title": "Allow?",
                "message": "Proceed?"
            }),
            None,
        );
        *boundaries.active_run.lock().unwrap() =
            active.map(|(run, workspace)| (run.to_owned(), workspace.to_owned()));
        DesktopAttachService {
            boundaries,
            idempotency: IdempotencyStore::open(":memory:").unwrap(),
            home: PathBuf::new(),
            workspace_contexts: Arc::new(Mutex::new(HashMap::new())),
            client_credentials: Arc::new(Mutex::new(HashMap::new())),
            credential_path: None,
            client_identity: Some("default".into()),
        }
    }

    #[cfg(target_os = "linux")]
    fn permission_request(
        service: &mut DesktopAttachService<FakeRunStartBoundaries>,
        workspace: &str,
        run_id: &str,
        gate_id: &str,
        decision: PermissionDecision,
        request_id: &str,
        key: &str,
    ) -> Result<PermissionAnswerAccepted, ProtocolError> {
        service.answer_permission(
            workspace,
            PermissionAnswerRequest {
                run_id: run_id.into(),
                gate_id: gate_id.into(),
                decision,
            },
            &Id::new(request_id).unwrap(),
            &Id::new(key).unwrap(),
            CompanionProvenance {
                profile: "default".into(),
                companion_kind: "cli".into(),
                companion_version: "1.2.3".into(),
                peer_uid: 1000,
                peer_pid: 42,
            },
        )
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_permission_answer_resolves_and_replays_from_idempotency() {
        let run_id = "0190a100-0000-7000-8000-000000000001";
        let mut service = permission_service(
            FakeRunStartBoundaries::accepting(),
            Some((run_id, "workspace-a")),
        );
        let key = "018f0000-0000-7000-8000-000000000002";
        let first = permission_request(
            &mut service,
            "workspace-a",
            run_id,
            "gate-1",
            PermissionDecision::Allow,
            "018f0000-0000-7000-8000-000000000003",
            key,
        )
        .unwrap();
        let replay = permission_request(
            &mut service,
            "workspace-a",
            run_id,
            "gate-1",
            PermissionDecision::Allow,
            "018f0000-0000-7000-8000-000000000004",
            key,
        )
        .unwrap();

        assert_eq!(first.committed_seq, 3);
        assert_eq!(replay, first);
        let queued = service.boundaries.queued_permission_answers.lock().unwrap();
        assert_eq!(queued.len(), 1);
        assert!(matches!(queued[0].1, ChatPermissionAnswer::Confirm(true)));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_permission_answer_maps_deny_to_false() {
        let run_id = "0190a100-0000-7000-8000-000000000001";
        let mut service = permission_service(
            FakeRunStartBoundaries::accepting(),
            Some((run_id, "workspace-a")),
        );
        permission_request(
            &mut service,
            "workspace-a",
            run_id,
            "gate-1",
            PermissionDecision::Deny,
            "018f0000-0000-7000-8000-000000000003",
            "018f0000-0000-7000-8000-000000000002",
        )
        .unwrap();
        let queued = service.boundaries.queued_permission_answers.lock().unwrap();
        assert!(matches!(queued[0].1, ChatPermissionAnswer::Confirm(false)));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_permission_answer_rejects_run_workspace_and_gate_mismatches() {
        let requested_run = "0190a100-0000-7000-8000-000000000001";
        let other_run = "0190a100-0000-7000-8000-000000000009";
        let cases = [
            (None, "workspace-a", requested_run, "gate-1"),
            (
                Some((other_run, "workspace-a")),
                "workspace-a",
                requested_run,
                "gate-1",
            ),
            (
                Some((requested_run, "workspace-b")),
                "workspace-a",
                requested_run,
                "gate-1",
            ),
            (
                Some((requested_run, "workspace-b")),
                "workspace-b",
                requested_run,
                "gate-1",
            ),
            (
                Some((requested_run, "workspace-a")),
                "workspace-a",
                requested_run,
                "gate-2",
            ),
            (
                Some((requested_run, "workspace-a")),
                "workspace-a",
                other_run,
                "gate-1",
            ),
        ];
        for (index, (active, workspace, run_id, gate_id)) in cases.into_iter().enumerate() {
            let mut service = permission_service(FakeRunStartBoundaries::accepting(), active);
            assert!(permission_request(
                &mut service,
                workspace,
                run_id,
                gate_id,
                PermissionDecision::Allow,
                &format!("018f0000-0000-7000-8000-{:012x}", index + 10),
                &format!("018f0000-0000-7000-8000-{:012x}", index + 20),
            )
            .is_err());
            assert!(service
                .boundaries
                .queued_permission_answers
                .lock()
                .unwrap()
                .is_empty());
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_permission_answer_returns_an_error_when_commit_wait_expires() {
        let run_id = "0190a100-0000-7000-8000-000000000001";
        let boundaries = FakeRunStartBoundaries {
            permission_auto_commit: false,
            ..FakeRunStartBoundaries::accepting()
        };
        let mut service = permission_service(boundaries, Some((run_id, "workspace-a")));
        assert!(permission_request(
            &mut service,
            "workspace-a",
            run_id,
            "gate-1",
            PermissionDecision::Allow,
            "018f0000-0000-7000-8000-000000000003",
            "018f0000-0000-7000-8000-000000000002",
        )
        .is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_permission_answer_rejects_a_settled_run_without_queueing() {
        let run_id = "0190a100-0000-7000-8000-000000000001";
        let mut service = permission_service(
            FakeRunStartBoundaries::accepting(),
            Some((run_id, "workspace-a")),
        );
        append_test_event(
            &mut service.boundaries.journal.lock().unwrap(),
            run_id,
            3,
            "permission.resolved",
            json!({"gate_id": "gate-1", "decision": {"type": "confirm", "value": true}}),
            None,
        );

        assert!(permission_request(
            &mut service,
            "workspace-a",
            run_id,
            "gate-1",
            PermissionDecision::Allow,
            "018f0000-0000-7000-8000-000000000013",
            "018f0000-0000-7000-8000-000000000012",
        )
        .is_err());
        assert!(service
            .boundaries
            .queued_permission_answers
            .lock()
            .unwrap()
            .is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_permission_answer_rejects_a_competing_desktop_resolution() {
        let run_id = "0190a100-0000-7000-8000-000000000001";
        let boundaries = FakeRunStartBoundaries {
            permission_competing_answer: true,
            ..FakeRunStartBoundaries::accepting()
        };
        let mut service = permission_service(boundaries, Some((run_id, "workspace-a")));

        assert!(permission_request(
            &mut service,
            "workspace-a",
            run_id,
            "gate-1",
            PermissionDecision::Allow,
            "018f0000-0000-7000-8000-000000000023",
            "018f0000-0000-7000-8000-000000000022",
        )
        .is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_cancel_replays_without_a_second_cancellation() {
        let boundaries = FakeRunStartBoundaries::accepting();
        *boundaries.active_run.lock().unwrap() = Some((
            "0190a100-0000-7000-8000-000000000001".into(),
            "workspace-a".into(),
        ));
        let mut service = DesktopAttachService {
            boundaries,
            idempotency: IdempotencyStore::open(":memory:").unwrap(),
            home: PathBuf::new(),
            workspace_contexts: Arc::new(Mutex::new(HashMap::new())),
            client_credentials: Arc::new(Mutex::new(HashMap::new())),
            credential_path: None,
            client_identity: Some("default".into()),
        };
        let run_id = "0190a100-0000-7000-8000-000000000001";
        let key = "018f0000-0000-7000-8000-000000000002";
        let first = cancel_request(
            &mut service,
            "workspace-a",
            run_id,
            "018f0000-0000-7000-8000-000000000003",
            key,
        )
        .unwrap();
        let replay = cancel_request(
            &mut service,
            "workspace-a",
            run_id,
            "018f0000-0000-7000-8000-000000000004",
            key,
        )
        .unwrap();
        assert_eq!(replay, first);
        assert_eq!(service.boundaries.cancel_calls.load(Ordering::SeqCst), 1);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_cancel_rejects_unknown_settled_and_other_workspace_runs() {
        for active in [
            None,
            Some((
                "0190a100-0000-7000-8000-000000000002".into(),
                "workspace-a".into(),
            )),
            Some((
                "0190a100-0000-7000-8000-000000000001".into(),
                "workspace-b".into(),
            )),
        ] {
            let boundaries = FakeRunStartBoundaries::accepting();
            *boundaries.active_run.lock().unwrap() = active;
            let mut service = DesktopAttachService {
                boundaries,
                idempotency: IdempotencyStore::open(":memory:").unwrap(),
                home: PathBuf::new(),
                workspace_contexts: Arc::new(Mutex::new(HashMap::new())),
                client_credentials: Arc::new(Mutex::new(HashMap::new())),
                credential_path: None,
                client_identity: Some("default".into()),
            };
            assert!(cancel_request(
                &mut service,
                "workspace-a",
                "0190a100-0000-7000-8000-000000000001",
                "018f0000-0000-7000-8000-000000000003",
                "018f0000-0000-7000-8000-000000000002",
            )
            .is_err());
            assert_eq!(service.boundaries.cancel_calls.load(Ordering::SeqCst), 0);
        }
    }

    #[cfg(target_os = "linux")]
    fn attach_start_on(
        service: &mut DesktopAttachService<FakeRunStartBoundaries>,
        text: &str,
        request_id: &str,
        idempotency_key: &str,
        thread_id: Option<&str>,
    ) -> Result<RunStartAccepted, ProtocolError> {
        service.start_run(
            "workspace-a",
            AttachRunStartRequest {
                text: text.into(),
                context: None,
                thread_id: thread_id.map(str::to_owned),
            },
            &Id::new(request_id).unwrap(),
            &Id::new(idempotency_key).unwrap(),
            CompanionProvenance {
                profile: "default".into(),
                companion_kind: "cli".into(),
                companion_version: "1.2.3".into(),
                peer_uid: 1000,
                peer_pid: 42,
            },
        )
    }

    #[cfg(target_os = "linux")]
    fn attach_service_with_thread() -> (DesktopAttachService<FakeRunStartBoundaries>, String, String)
    {
        let boundaries = FakeRunStartBoundaries::accepting();
        let first_run_id = "0190a100-0000-7000-8000-000000000010".to_owned();
        let mut first = crate::chat::event_envelope(
            &first_run_id,
            1,
            "user.prompt.submitted",
            json!({"prompt": "first"}),
            Some("owner"),
        );
        first
            .provenance
            .extra
            .insert("attach_profile".into(), json!("default"));
        let thread_id = boundaries
            .journal
            .lock()
            .unwrap()
            .append_new_run("workspace-a", &first)
            .unwrap();
        (
            DesktopAttachService {
                boundaries,
                idempotency: IdempotencyStore::open(":memory:").unwrap(),
                home: PathBuf::new(),
                workspace_contexts: Arc::new(Mutex::new(HashMap::new())),
                client_credentials: Arc::new(Mutex::new(HashMap::new())),
                credential_path: None,
                client_identity: Some("default".into()),
            },
            thread_id,
            first_run_id,
        )
    }

    #[cfg(target_os = "linux")]
    fn attach_test_provenance() -> Provenance {
        Provenance {
            source: "test".into(),
            source_version: "1".into(),
            actor_id: None,
            device_id: None,
            rpc_request_id: None,
            capability_versions: None,
            extra: BTreeMap::new(),
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_adapter_returns_receipt_and_records_companion_provenance() {
        let (result, service) = attach_start(FakeRunStartBoundaries::accepting(), None);
        let accepted = result.unwrap();

        assert_eq!(accepted.committed_seq, 1);
        assert!(chrono::DateTime::parse_from_rfc3339(&accepted.accepted_at).is_ok());
        assert_eq!(
            service.boundaries.launched_run.lock().unwrap().as_deref(),
            Some(accepted.run_id.as_str())
        );
        let provenance = service
            .boundaries
            .prepared_provenance
            .lock()
            .unwrap()
            .clone()
            .unwrap();
        assert_eq!(
            provenance.rpc_request_id.as_deref(),
            Some("018f0000-0000-7000-8000-000000000001")
        );
        assert_eq!(
            provenance.extra["idempotency_key"],
            "018f0000-0000-7000-8000-000000000002"
        );
        assert_eq!(provenance.extra["companion_kind"], "cli");
        assert_eq!(provenance.extra["peer_uid"], 1000);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_adapter_lists_and_opens_only_requested_workspace_threads() {
        let root = std::env::temp_dir().join(format!("muniment-attach-threads-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&root).unwrap();
        let mut boundaries = FakeRunStartBoundaries::accepting();
        boundaries.journal = Mutex::new(RunJournal::open(root.join("runs.sqlite3")).unwrap());
        {
            let mut journal = boundaries.journal.lock().unwrap();
            for (run_id, workspace, prompt, reply) in [
                (
                    "018f0000-0000-7000-8000-0000000000a1",
                    "workspace-a",
                    "first prompt",
                    "first reply",
                ),
                (
                    "018f0000-0000-7000-8000-0000000000a2",
                    "workspace-a",
                    "second prompt",
                    "second reply",
                ),
                (
                    "018f0000-0000-7000-8000-0000000000b1",
                    "workspace-b",
                    "hidden prompt",
                    "hidden reply",
                ),
            ] {
                journal
                    .append_new_run(
                        workspace,
                        &crate::chat::event_envelope(
                            run_id,
                            1,
                            "run.started",
                            json!({}),
                            Some("owner"),
                        ),
                    )
                    .unwrap();
                journal
                    .append(
                        1,
                        &crate::chat::event_envelope(
                            run_id,
                            2,
                            "user.prompt.submitted",
                            json!({"prompt": prompt}),
                            Some("owner"),
                        ),
                    )
                    .unwrap();
                journal
                    .append(
                        2,
                        &crate::chat::event_envelope(
                            run_id,
                            3,
                            "model.stream.delta",
                            json!({"text": reply, "content_disclosure": "released"}),
                            Some("owner"),
                        ),
                    )
                    .unwrap();
            }
        }
        let mut service = DesktopAttachService {
            boundaries,
            idempotency: IdempotencyStore::open(":memory:").unwrap(),
            home: PathBuf::new(),
            workspace_contexts: Arc::new(Mutex::new(HashMap::new())),
            client_credentials: Arc::new(Mutex::new(HashMap::new())),
            credential_path: None,
            client_identity: Some("default".into()),
        };

        let first = service
            .list_threads(
                "workspace-a",
                ThreadListRequest {
                    limit: 1,
                    cursor: None,
                },
            )
            .unwrap();
        assert_eq!(first.threads.len(), 1);
        assert!(first.next_cursor.is_some());
        let second = service
            .list_threads(
                "workspace-a",
                ThreadListRequest {
                    limit: 1,
                    cursor: first.next_cursor,
                },
            )
            .unwrap();
        assert_eq!(second.threads.len(), 1);
        assert_ne!(first.threads[0].thread_id, second.threads[0].thread_id);
        assert!(second.next_cursor.is_none());

        let thread_id = first.threads[0].thread_id.clone();
        let opened = service
            .open_thread(
                "workspace-a",
                ThreadOpenRequest {
                    thread_id: thread_id.clone(),
                    limit: 1,
                    cursor: None,
                },
            )
            .unwrap();
        assert_eq!(opened.entries.len(), 1);
        assert!(opened.next_cursor.is_some());
        let rest = service
            .open_thread(
                "workspace-a",
                ThreadOpenRequest {
                    thread_id,
                    limit: 10,
                    cursor: opened.next_cursor,
                },
            )
            .unwrap();
        assert_eq!(rest.entries.len(), 1);
        assert!(rest.next_cursor.is_none());
        assert!(service
            .list_threads(
                "workspace-b",
                ThreadListRequest {
                    limit: 10,
                    cursor: None,
                },
            )
            .unwrap()
            .threads
            .iter()
            .all(|thread| thread.thread_id != first.threads[0].thread_id
                && thread.thread_id != second.threads[0].thread_id));

        assert_eq!(
            service
                .list_threads(
                    "workspace-a",
                    ThreadListRequest {
                        limit: 0,
                        cursor: None,
                    },
                )
                .unwrap_err()
                .code(),
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            service
                .list_threads(
                    "workspace-a",
                    ThreadListRequest {
                        limit: 1,
                        cursor: Some("invalid".into()),
                    },
                )
                .unwrap_err()
                .code(),
            ErrorCode::InvalidCursor
        );
        assert_eq!(
            service
                .open_thread(
                    "workspace-a",
                    ThreadOpenRequest {
                        thread_id: first.threads[0].thread_id.clone(),
                        limit: 1,
                        cursor: Some("invalid".into()),
                    },
                )
                .unwrap_err()
                .code(),
            ErrorCode::InvalidCursor
        );

        drop(service);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_onboarding_context_survives_connections_and_keys_distinct_external_roots() {
        let root = std::env::temp_dir().join(format!("muniment-attach-context-{}", Uuid::now_v7()));
        let first = root.join("repo-one");
        let second = root.join("repo-two");
        let first_memory = root.join("memory-one");
        let second_memory = root.join("memory-two");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        std::fs::write(first.join("AGENTS.md"), "first instructions").unwrap();
        std::fs::write(second.join("AGENTS.md"), "second instructions").unwrap();
        let contexts = Arc::new(Mutex::new(HashMap::new()));
        let mut first_connection = DesktopAttachService {
            boundaries: FakeRunStartBoundaries::accepting(),
            idempotency: IdempotencyStore::open(":memory:").unwrap(),
            home: root.join("home"),
            workspace_contexts: contexts.clone(),
            client_credentials: Arc::new(Mutex::new(HashMap::new())),
            credential_path: None,
            client_identity: Some("default".into()),
        };
        first_connection
            .onboard_workspace(WorkspaceOnboardRequest {
                opened_directory: first.to_string_lossy().into_owned(),
                memory_location: first_memory.to_string_lossy().into_owned(),
            })
            .unwrap();
        drop(first_connection);

        // The fake coordinator grants exactly the workspace the gateway would
        // hand back for this client -- here the canonical repository the run
        // targets. A run requesting any other workspace is rejected by
        // `configure_run`, mirroring the real capability check.
        let first_canonical = first.canonicalize().unwrap().to_string_lossy().into_owned();
        let mut second_connection = DesktopAttachService {
            boundaries: FakeRunStartBoundaries {
                granted_workspaces: vec![first_canonical.clone()],
                ..FakeRunStartBoundaries::accepting()
            },
            idempotency: IdempotencyStore::open(":memory:").unwrap(),
            home: root.join("home"),
            workspace_contexts: contexts.clone(),
            client_credentials: Arc::new(Mutex::new(HashMap::new())),
            credential_path: None,
            client_identity: Some("default".into()),
        };
        second_connection
            .onboard_workspace(WorkspaceOnboardRequest {
                opened_directory: second.to_string_lossy().into_owned(),
                memory_location: second_memory.to_string_lossy().into_owned(),
            })
            .unwrap();

        let guard = contexts.lock().unwrap();
        let stored = guard.get("default").unwrap();
        assert_eq!(
            stored
                .get(&first.canonicalize().unwrap())
                .unwrap()
                .as_deref(),
            Some("first instructions")
        );
        assert_eq!(
            stored
                .get(&second.canonicalize().unwrap())
                .unwrap()
                .as_deref(),
            Some("second instructions")
        );
        assert_eq!(
            stored
                .get(&first_memory.canonicalize().unwrap())
                .unwrap()
                .as_deref(),
            Some("first instructions")
        );
        // Release the shared `contexts` lock before the client-b/second/third
        // connections call back into the service -- those methods re-lock the
        // same mutex, so holding the guard here would deadlock (dropping the
        // former `stored` reference was a no-op that left the guard live).
        drop(guard);
        let client_b = DesktopAttachService {
            boundaries: FakeRunStartBoundaries::accepting(),
            idempotency: IdempotencyStore::open(":memory:").unwrap(),
            home: root.join("home"),
            workspace_contexts: contexts.clone(),
            client_credentials: Arc::new(Mutex::new(HashMap::new())),
            credential_path: None,
            client_identity: Some("client-b".into()),
        };
        // Grants are bound to the authorizing client: a second identity cannot
        // borrow another client's onboarded workspaces. `authorized_workspace`
        // is the gate the dispatcher applies before a run is ever started, so it
        // resolves nothing for client-b even though the contexts are shared.
        for workspace in [&first, &first_memory] {
            assert!(client_b
                .authorized_workspace(&workspace.to_string_lossy())
                .is_none());
        }
        assert!(client_b
            .boundaries
            .prepared_provenance
            .lock()
            .unwrap()
            .is_none());
        // The onboarding client resolves its own repository through the same
        // gate (canonicalizing the request) before starting the run.
        let first_authorized = second_connection
            .authorized_workspace(&first.to_string_lossy())
            .unwrap();
        assert_eq!(first_authorized, first_canonical);
        let run = second_connection
            .start_run(
                &first_authorized,
                AttachRunStartRequest {
                    text: "use repository context".into(),
                    context: None,
                    thread_id: None,
                },
                &Id::new("018f0000-0000-7000-8000-000000000011").unwrap(),
                &Id::new("018f0000-0000-7000-8000-000000000012").unwrap(),
                CompanionProvenance {
                    profile: "default".into(),
                    companion_kind: "editor-extension".into(),
                    companion_version: "1.2.3".into(),
                    peer_uid: 1000,
                    peer_pid: 42,
                },
            )
            .unwrap();
        assert!(!run.run_id.is_empty());
        let provenance = second_connection
            .boundaries
            .prepared_provenance
            .lock()
            .unwrap();
        assert_eq!(
            provenance.as_ref().unwrap().extra["repository_instructions"],
            "first instructions"
        );
        drop(provenance);
        let second_memory_canonical = second_memory
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let mut third_connection = DesktopAttachService {
            boundaries: FakeRunStartBoundaries {
                granted_workspaces: vec![second_memory_canonical.clone()],
                ..FakeRunStartBoundaries::accepting()
            },
            idempotency: IdempotencyStore::open(":memory:").unwrap(),
            home: root.join("home"),
            workspace_contexts: contexts.clone(),
            client_credentials: Arc::new(Mutex::new(HashMap::new())),
            credential_path: None,
            client_identity: Some("default".into()),
        };
        let second_memory_authorized = third_connection
            .authorized_workspace(&second_memory.to_string_lossy())
            .unwrap();
        assert_eq!(second_memory_authorized, second_memory_canonical);
        third_connection
            .start_run(
                &second_memory_authorized,
                AttachRunStartRequest {
                    text: "second context".into(),
                    context: None,
                    thread_id: None,
                },
                &Id::new("018f0000-0000-7000-8000-000000000021").unwrap(),
                &Id::new("018f0000-0000-7000-8000-000000000022").unwrap(),
                CompanionProvenance {
                    profile: "default".into(),
                    companion_kind: "editor-extension".into(),
                    companion_version: "1.2.3".into(),
                    peer_uid: 1000,
                    peer_pid: 42,
                },
            )
            .unwrap();
        assert_eq!(
            third_connection
                .boundaries
                .prepared_provenance
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .extra["repository_instructions"],
            "second instructions"
        );
        assert!(!root.join("home").exists());
        second_connection.ensure_home().unwrap();
        for child in ["memory", "agents", "projects", "sessions"] {
            assert!(root.join("home").join(child).join("README.md").is_file());
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_credentials_reject_impersonation_and_canonical_grants_reject_retargeting() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!("muniment-attach-grants-{}", Uuid::now_v7()));
        let first = root.join("first");
        let second = root.join("second");
        let memory = root.join("memory");
        let alias = root.join("alias");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        symlink(&first, &alias).unwrap();
        let state = AttachListenerState::load(&root.join("credentials.json")).unwrap();
        let contexts = state.workspace_contexts;
        let credentials = state.client_credentials;
        let make_service = || DesktopAttachService {
            boundaries: FakeRunStartBoundaries::accepting(),
            idempotency: IdempotencyStore::open(":memory:").unwrap(),
            home: root.join("home"),
            workspace_contexts: contexts.clone(),
            client_credentials: credentials.clone(),
            credential_path: None,
            client_identity: None,
        };

        let mut client_a = make_service();
        assert_eq!(
            client_a
                .authorize_client("client-a", None, &"aa".repeat(32), "cli", "1.2.3")
                .unwrap(),
            "aa".repeat(32)
        );
        client_a
            .onboard_workspace(WorkspaceOnboardRequest {
                opened_directory: alias.to_string_lossy().into_owned(),
                memory_location: memory.to_string_lossy().into_owned(),
            })
            .unwrap();
        assert_eq!(
            client_a.authorized_workspace(&alias.to_string_lossy()),
            Some(first.to_string_lossy().into_owned())
        );
        assert!(client_a
            .authorized_workspace(&root.join("missing").to_string_lossy())
            .is_none());
        std::fs::remove_file(&alias).unwrap();
        symlink(&second, &alias).unwrap();
        assert!(client_a
            .authorized_workspace(&alias.to_string_lossy())
            .is_none());

        let mut impersonator = make_service();
        for credential in [None, Some("cc".repeat(32))] {
            assert_eq!(
                impersonator
                    .authorize_client(
                        "client-a",
                        credential.as_deref(),
                        &"bb".repeat(32),
                        "cli",
                        "1.2.3",
                    )
                    .unwrap_err()
                    .code(),
                ErrorCode::Unauthorized
            );
        }
        assert!(impersonator.client_identity.is_none());
        assert!(impersonator
            .boundaries
            .prepared_provenance
            .lock()
            .unwrap()
            .is_none());

        let mut reconnect = make_service();
        assert_eq!(
            reconnect
                .authorize_client(
                    "client-a",
                    Some(&"aa".repeat(32)),
                    &"dd".repeat(32),
                    "cli",
                    "1.2.3",
                )
                .unwrap(),
            "aa".repeat(32)
        );
        assert_eq!(
            reconnect.authorized_workspace(&first.to_string_lossy()),
            Some(first.to_string_lossy().into_owned())
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_client_credentials_survive_restart_and_unsafe_state_fails_closed() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let root =
            std::env::temp_dir().join(format!("muniment-attach-credentials-{}", Uuid::now_v7()));
        let path = root.join("credentials.json");
        let identity = "018f0000-0000-7000-8000-000000000099";
        let credential = "ab".repeat(32);
        let make_service = |credentials| DesktopAttachService {
            boundaries: FakeRunStartBoundaries::accepting(),
            idempotency: IdempotencyStore::open(":memory:").unwrap(),
            home: root.join("home"),
            workspace_contexts: Arc::new(Mutex::new(HashMap::new())),
            client_credentials: Arc::new(Mutex::new(credentials)),
            credential_path: Some(path.clone()),
            client_identity: None,
        };

        let mut initial = make_service(HashMap::new());
        assert_eq!(
            initial
                .authorize_client(identity, None, &credential, "cli", "1.2.3")
                .unwrap(),
            credential
        );
        let metadata = std::fs::symlink_metadata(&path).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        drop(initial);

        let stored: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(stored["version"], 1);
        assert_eq!(stored["companions"][identity]["credential"], credential);
        assert_eq!(stored["companions"][identity]["claimed_kind"], "cli");
        assert_eq!(stored["companions"][identity]["claimed_version"], "1.2.3");
        let approved_at = stored["companions"][identity]["approved_at"]
            .as_str()
            .unwrap();
        assert!(approved_at.ends_with('Z'));
        assert!(chrono::DateTime::parse_from_rfc3339(approved_at).is_ok());

        let state = AttachListenerState::load(&path).unwrap();
        assert_eq!(
            state.list_companions().unwrap(),
            vec![AuthorizedCompanion {
                identity: identity.into(),
                claimed_kind: "cli".into(),
                claimed_version: "1.2.3".into(),
                approved_at: Some(approved_at.into()),
            }]
        );

        let loaded = load_client_credentials(&path).unwrap();
        let mut restarted = make_service(loaded);
        assert_eq!(
            restarted
                .authorize_client(
                    identity,
                    Some(&credential),
                    &"cd".repeat(32),
                    "changed",
                    "9.9.9",
                )
                .unwrap(),
            credential
        );
        for presented in [None, Some("00".repeat(32))] {
            let mut rejected = make_service(load_client_credentials(&path).unwrap());
            assert_eq!(
                rejected
                    .authorize_client(
                        identity,
                        presented.as_deref(),
                        &"ef".repeat(32),
                        "cli",
                        "1.2.3",
                    )
                    .unwrap_err()
                    .code(),
                ErrorCode::Unauthorized
            );
            assert!(rejected.client_identity.is_none());
        }
        let mut unknown = make_service(load_client_credentials(&path).unwrap());
        assert_eq!(
            unknown
                .authorize_client(
                    "018f0000-0000-7000-8000-000000000100",
                    Some(&credential),
                    &"ef".repeat(32),
                    "cli",
                    "1.2.3",
                )
                .unwrap_err()
                .code(),
            ErrorCode::Unauthorized
        );

        let mut unsupported = stored.clone();
        unsupported["version"] = json!(2);
        std::fs::write(&path, serde_json::to_vec(&unsupported).unwrap()).unwrap();
        assert!(load_client_credentials(&path).is_err());
        let mut incomplete = stored;
        incomplete["companions"][identity]
            .as_object_mut()
            .unwrap()
            .remove("approved_at");
        std::fs::write(&path, serde_json::to_vec(&incomplete).unwrap()).unwrap();
        assert!(load_client_credentials(&path).is_err());
        std::fs::write(&path, b"not-json").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(load_client_credentials(&path).is_err());
        std::fs::remove_file(&path).unwrap();
        let target = root.join("target");
        std::fs::write(&target, b"{}").unwrap();
        symlink(&target, &path).unwrap();
        assert!(load_client_credentials(&path).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn legacy_attach_credentials_authenticate_with_unknown_claims() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!("muniment-attach-legacy-{}", Uuid::now_v7()));
        let path = root.join("credentials.json");
        let identity = "018f0000-0000-7000-8000-000000000099";
        let credential = "ab".repeat(32);
        let new_identity = "018f0000-0000-7000-8000-000000000100";
        let new_credential = "cd".repeat(32);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            &path,
            serde_json::to_vec(&HashMap::from([(identity, &credential)])).unwrap(),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

        let state = AttachListenerState::load(&path).unwrap();
        assert_eq!(
            state.list_companions().unwrap(),
            vec![AuthorizedCompanion {
                identity: identity.into(),
                claimed_kind: "unknown".into(),
                claimed_version: "unknown".into(),
                approved_at: None,
            }]
        );
        let mut service = DesktopAttachService {
            boundaries: FakeRunStartBoundaries::accepting(),
            idempotency: IdempotencyStore::open(":memory:").unwrap(),
            home: root.join("home"),
            workspace_contexts: state.workspace_contexts,
            client_credentials: state.client_credentials,
            credential_path: Some(path.clone()),
            client_identity: None,
        };
        assert_eq!(
            service
                .authorize_client(identity, Some(&credential), "", "changed", "9.9.9")
                .unwrap(),
            credential
        );
        assert_eq!(
            service
                .authorize_client(new_identity, None, &new_credential, "desktop", "1.0.0")
                .unwrap(),
            new_credential
        );
        drop(service);

        let state = AttachListenerState::load(&path).unwrap();
        assert_eq!(
            state.list_companions().unwrap(),
            vec![
                AuthorizedCompanion {
                    identity: identity.into(),
                    claimed_kind: "unknown".into(),
                    claimed_version: "unknown".into(),
                    approved_at: None,
                },
                AuthorizedCompanion {
                    identity: new_identity.into(),
                    claimed_kind: "desktop".into(),
                    claimed_version: "1.0.0".into(),
                    approved_at: Some(
                        load_client_credentials(&path).unwrap()[new_identity]
                            .approved_at
                            .clone()
                            .unwrap(),
                    ),
                },
            ]
        );
        let mut restarted_service = DesktopAttachService {
            boundaries: FakeRunStartBoundaries::accepting(),
            idempotency: IdempotencyStore::open(":memory:").unwrap(),
            home: root.join("home"),
            workspace_contexts: state.workspace_contexts,
            client_credentials: state.client_credentials,
            credential_path: Some(path),
            client_identity: None,
        };
        assert!(restarted_service
            .authorize_client(identity, Some(&credential), "", "changed", "9.9.9")
            .is_ok());
        assert!(restarted_service
            .authorize_client(new_identity, Some(&new_credential), "", "changed", "9.9.9",)
            .is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn revoke_companion_persists_before_emitting_and_reconnect_fails_closed() {
        use muniment_attach::{decode_frame, handshake_stream_with_credential, ClientError};
        use muniment_core::attach::linux::{ApprovalDecision, AttachSessionError, PeerCredentials};
        use std::io::Read;

        let root = std::env::temp_dir().join(format!("muniment-attach-revoke-{}", Uuid::now_v7()));
        let credential_path = root.join("credentials.json");
        let identity = "018f0000-0000-7000-8000-000000000099";
        let credential = "ab".repeat(32);
        persist_client_credentials(
            &credential_path,
            &HashMap::from([(
                identity.to_owned(),
                ClientCredential {
                    credential: credential.clone(),
                    claimed_kind: "unknown".into(),
                    claimed_version: "unknown".into(),
                    approved_at: Some("2026-08-04T00:00:00Z".into()),
                },
            )]),
        )
        .unwrap();
        let mut state = AttachListenerState::load(&credential_path).unwrap();
        let client_credentials = state.client_credentials.clone();
        let live_connections = state.live_connections.clone();

        let connect = |presented: String| {
            let (client_stream, server_stream) = std::os::unix::net::UnixStream::pair().unwrap();
            let observer = client_stream.try_clone().unwrap();
            let credentials = client_credentials.clone();
            let registry = live_connections.clone();
            let worker = std::thread::spawn(move || {
                let mut service = DesktopAttachService {
                    boundaries: FakeRunStartBoundaries::accepting(),
                    idempotency: IdempotencyStore::open(":memory:").unwrap(),
                    home: std::env::temp_dir(),
                    workspace_contexts: Arc::new(Mutex::new(HashMap::new())),
                    client_credentials: credentials,
                    credential_path: None,
                    client_identity: None,
                };
                run_authenticated_session_with_service_approvals_and_registry(
                    server_stream,
                    PeerCredentials {
                        pid: std::process::id() as i32,
                        uid: unsafe { libc::geteuid() },
                        gid: unsafe { libc::getegid() },
                    },
                    "0.0.1",
                    &mut service,
                    |_: &muniment_core::attach::PairingChallenge, _: Duration| {
                        Some(ApprovalDecision::Approve(
                            desktop_attach_approval().unwrap(),
                        ))
                    },
                    &registry,
                )
            });
            let client = handshake_stream_with_credential(
                client_stream,
                "0.0.1",
                identity,
                Some(&presented),
                Duration::from_secs(1),
                Duration::from_secs(1),
                || {},
            );
            (client, observer, worker)
        };

        let (client, mut observer, worker) = connect(credential.clone());
        let client = client.unwrap();
        state.revoke_companion(identity).unwrap();
        assert!(!load_client_credentials(&credential_path)
            .unwrap()
            .contains_key(identity));

        observer
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut prefix = [0; 4];
        observer.read_exact(&mut prefix).unwrap();
        let mut frame = vec![0; 4 + u32::from_be_bytes(prefix) as usize];
        frame[..4].copy_from_slice(&prefix);
        observer.read_exact(&mut frame[4..]).unwrap();
        let event: Value = decode_frame(&frame).unwrap().unwrap().0;
        assert_eq!(event["event"], "capability.revoked");
        assert!(event["body"]["capability"]
            .as_str()
            .is_some_and(|capability| !capability.is_empty()));
        assert_eq!(event["body"]["reason"], "companion_revoked");
        assert!(event["subscription_id"].as_str().is_some());
        assert_eq!(observer.read(&mut [0]).unwrap(), 0);
        drop(client);
        assert!(worker.join().unwrap().is_ok());

        let (reconnect, _observer, worker) = connect(credential);
        assert_eq!(reconnect.unwrap_err(), ClientError::UnexpectedMessage);
        assert_eq!(
            worker.join().unwrap(),
            Err(AttachSessionError::Authorization)
        );

        let credential = "cd".repeat(32);
        state.client_credentials.lock().unwrap().insert(
            identity.to_owned(),
            ClientCredential {
                credential: credential.clone(),
                claimed_kind: "unknown".into(),
                claimed_version: "unknown".into(),
                approved_at: Some("2026-08-04T00:00:00Z".into()),
            },
        );
        persist_client_credentials(&credential_path, &state.client_credentials.lock().unwrap())
            .unwrap();
        let (client, mut observer, worker) = connect(credential.clone());
        let mut client = client.unwrap();
        let blocker = root.join("not-a-directory");
        std::fs::write(&blocker, b"blocked").unwrap();
        state.credential_path = blocker.join("credentials.json");
        assert_eq!(
            state.revoke_companion(identity).unwrap_err().code(),
            ErrorCode::PersistenceFailed
        );
        assert_eq!(
            state
                .client_credentials
                .lock()
                .unwrap()
                .get(identity)
                .map(|entry| &entry.credential),
            Some(&credential)
        );
        observer
            .set_read_timeout(Some(Duration::from_millis(150)))
            .unwrap();
        assert!(matches!(
            observer.read(&mut [0]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                )
        ));
        client.list_threads(None).unwrap();
        drop(client);
        drop(observer);
        assert!(worker.join().unwrap().is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn production_listener_state_isolates_workspace_grants_across_transport_connections() {
        use muniment_attach::{handshake_stream_with_credential, ClientError};
        use muniment_core::attach::linux::ApprovalDecision;
        use std::os::unix::fs::{symlink, PermissionsExt};

        let root =
            std::env::temp_dir().join(format!("muniment-listener-isolation-{}", Uuid::now_v7()));
        let opened = root.join("opened");
        let memory = root.join("memory");
        let other = root.join("other");
        let alias = root.join("alias");
        std::fs::create_dir_all(&opened).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(opened.join("AGENTS.md"), "private client A context").unwrap();
        symlink(&opened, &alias).unwrap();
        let contexts = Arc::new(Mutex::new(HashMap::new()));
        let credentials = Arc::new(Mutex::new(HashMap::new()));
        let identity_a = "018f0000-0000-7000-8000-0000000000a1";
        let identity_b = "018f0000-0000-7000-8000-0000000000b1";
        // The attach socket is bound at `<runtime>/muniment/attach-v1.sock`; a
        // runtime path nested under the descriptive `root` overruns the AF_UNIX
        // `sun_path` limit (108 bytes) when the client connects. Bind each
        // connection under a short, dedicated temp directory -- created 0700 to
        // satisfy the runtime-directory secrecy check the same way a real
        // XDG_RUNTIME_DIR is -- and remove them when the test finishes.
        let runtime_dirs = std::cell::RefCell::new(Vec::new());
        let connect = |identity: &'static str,
                       credential: Option<String>,
                       boundaries: FakeRunStartBoundaries| {
            let runtime =
                std::env::temp_dir().join(format!("mt-attach-{}", Uuid::now_v7().simple()));
            std::fs::create_dir(&runtime).unwrap();
            std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
            runtime_dirs.borrow_mut().push(runtime.clone());
            let filesystem = AttachFilesystem::from_runtime_directory(&runtime).unwrap();
            let transport = AttachTransport::bind(&filesystem).unwrap();
            let client_stream =
                std::os::unix::net::UnixStream::connect(transport.local_path()).unwrap();
            let (server_stream, peer) = transport.accept().unwrap();
            let contexts = contexts.clone();
            let credentials = credentials.clone();
            let service_root = root.clone();
            let approval_workspace = opened.clone();
            let worker = std::thread::spawn(move || {
                let mut service = DesktopAttachService {
                    boundaries,
                    idempotency: IdempotencyStore::open(":memory:").unwrap(),
                    home: service_root.join("home"),
                    workspace_contexts: contexts,
                    client_credentials: credentials,
                    credential_path: None,
                    client_identity: None,
                };
                let result = run_authenticated_session_with_service_and_approvals(
                    server_stream,
                    peer,
                    "0.0.1",
                    &mut service,
                    |_: &muniment_core::attach::PairingChallenge, _: Duration| {
                        Some(ApprovalDecision::Approve(Approval {
                            profile: "desktop-owner".into(),
                            workspace: approval_workspace.to_string_lossy().into_owned(),
                            scopes: BTreeSet::from(["thread.read".into(), "run.write".into()]),
                            lifetime: Duration::from_secs(3600),
                        }))
                    },
                );
                (result, service)
            });
            let client = handshake_stream_with_credential(
                client_stream,
                "0.0.1",
                identity,
                credential.as_deref(),
                Duration::from_secs(1),
                Duration::from_secs(1),
                || {},
            );
            (client, worker)
        };

        let (client_a, worker) = connect(identity_a, None, FakeRunStartBoundaries::accepting());
        let mut client_a = client_a.unwrap();
        let credential_a = client_a.authorized_client_credential().to_owned();
        client_a
            .onboard_workspace(&alias.to_string_lossy(), &memory.to_string_lossy())
            .unwrap();
        drop(client_a);
        assert!(worker.join().unwrap().0.is_ok());

        // Client A is authorized for both the opened repository and its memory
        // root, so the coordinator grants either when a run requests it.
        let a_dispatch = FakeRunStartBoundaries {
            granted_workspaces: vec![
                opened
                    .canonicalize()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                memory
                    .canonicalize()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            ],
            ..FakeRunStartBoundaries::accepting()
        };
        let (client_a, worker) = connect(identity_a, Some(credential_a.clone()), a_dispatch);
        let mut client_a = client_a.unwrap();
        client_a
            .start_run_in_workspace("opened", None, Some(&opened.to_string_lossy()))
            .unwrap();
        client_a
            .start_run_in_workspace("override", None, Some(&memory.to_string_lossy()))
            .unwrap();
        drop(client_a);
        let (_, service) = worker.join().unwrap();
        assert_eq!(
            service
                .boundaries
                .prepared_provenance
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .extra["repository_instructions"],
            "private client A context"
        );

        let (client_b, worker) = connect(identity_b, None, FakeRunStartBoundaries::accepting());
        let mut client_b = client_b.unwrap();
        let credential_b = client_b.authorized_client_credential().to_owned();
        client_b
            .onboard_workspace(&other.to_string_lossy(), &other.to_string_lossy())
            .unwrap();
        drop(client_b);
        assert!(worker.join().unwrap().0.is_ok());
        for workspace in [&opened, &memory] {
            let b_dispatch = FakeRunStartBoundaries::accepting();
            let (client_b, worker) = connect(identity_b, Some(credential_b.clone()), b_dispatch);
            let mut client_b = client_b.unwrap();
            // The workspace is not among client-b's grants, so the dispatcher
            // resolves it to an `Unauthorized` protocol error, which the client
            // surfaces as `AuthorizationExpired` (see `map_protocol_error`).
            assert_eq!(
                client_b.start_run_in_workspace("borrow", None, Some(&workspace.to_string_lossy())),
                Err(ClientError::AuthorizationExpired)
            );
            drop(client_b);
            let (_, service) = worker.join().unwrap();
            assert!(service
                .boundaries
                .prepared_provenance
                .lock()
                .unwrap()
                .is_none());
            assert!(service.boundaries.launched_run.lock().unwrap().is_none());
        }

        for (identity, credential) in [
            (identity_a, None),
            (identity_a, Some("malformed".into())),
            (identity_a, Some("00".repeat(32))),
            (
                "018f0000-0000-7000-8000-0000000000ff",
                Some(credential_a.clone()),
            ),
        ] {
            let denied = FakeRunStartBoundaries::accepting();
            let (client, worker) = connect(identity, credential, denied);
            assert!(client.is_err());
            let (_, service) = worker.join().unwrap();
            assert!(service
                .boundaries
                .prepared_provenance
                .lock()
                .unwrap()
                .is_none());
            assert!(service.client_identity.is_none());
        }

        for workspace in [root.join("missing"), alias.clone()] {
            if workspace == alias {
                std::fs::remove_file(&alias).unwrap();
                symlink(&other, &alias).unwrap();
            }
            let denied = FakeRunStartBoundaries::accepting();
            let (client, worker) = connect(identity_a, Some(credential_a.clone()), denied);
            let mut client = client.unwrap();
            // A missing directory or a workspace retargeted through a symlink
            // resolves to nothing authorized, so the run is denied the same way.
            assert_eq!(
                client.start_run_in_workspace("invalid", None, Some(&workspace.to_string_lossy())),
                Err(ClientError::AuthorizationExpired)
            );
            drop(client);
            let (_, service) = worker.join().unwrap();
            assert!(service
                .boundaries
                .prepared_provenance
                .lock()
                .unwrap()
                .is_none());
            assert!(service.boundaries.launched_run.lock().unwrap().is_none());
        }
        std::fs::remove_dir_all(root).unwrap();
        for runtime in runtime_dirs.borrow().iter() {
            let _ = std::fs::remove_dir_all(runtime);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_adapter_replays_exact_retry_without_second_coordinator_run() {
        let (mut service, thread_id, _) = attach_service_with_thread();
        let key = "018f0000-0000-7000-8000-000000000002";
        let first = attach_start_on(
            &mut service,
            "hello",
            "018f0000-0000-7000-8000-000000000001",
            key,
            Some(&thread_id),
        )
        .unwrap();
        service
            .boundaries
            .journal
            .lock()
            .unwrap()
            .append_thread_deleted(
                1,
                &thread_id,
                "2026-08-03T00:00:00Z",
                &attach_test_provenance(),
            )
            .unwrap();
        let replay = attach_start_on(
            &mut service,
            "hello",
            "018f0000-0000-7000-8000-000000000003",
            key,
            Some(&thread_id),
        )
        .unwrap();

        assert_eq!(replay, first);
        assert_eq!(
            service
                .boundaries
                .prompt_protection_calls
                .load(Ordering::SeqCst),
            1
        );
        assert_eq!(service.boundaries.prepare_calls.load(Ordering::SeqCst), 1);
        assert_eq!(service.boundaries.launch_calls.load(Ordering::SeqCst), 1);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_adapter_rejects_conflicting_key_without_second_coordinator_run() {
        let (mut service, thread_id, _) = attach_service_with_thread();
        let key = "018f0000-0000-7000-8000-000000000002";
        attach_start_on(
            &mut service,
            "hello",
            "018f0000-0000-7000-8000-000000000001",
            key,
            Some(&thread_id),
        )
        .unwrap();
        let conflict = attach_start_on(
            &mut service,
            "hello",
            "018f0000-0000-7000-8000-000000000003",
            key,
            None,
        )
        .unwrap_err();

        assert_eq!(
            serde_json::to_value(conflict).unwrap()["code"],
            "idempotency_conflict"
        );
        assert_eq!(
            service
                .boundaries
                .prompt_protection_calls
                .load(Ordering::SeqCst),
            1
        );
        assert_eq!(service.boundaries.prepare_calls.load(Ordering::SeqCst), 1);
        assert_eq!(service.boundaries.launch_calls.load(Ordering::SeqCst), 1);

        let changed = attach_start_on(
            &mut service,
            "hello",
            "018f0000-0000-7000-8000-000000000004",
            key,
            Some("0190a100-0000-7000-8000-000000000099"),
        )
        .unwrap_err();
        assert_eq!(
            serde_json::to_value(changed).unwrap()["code"],
            "idempotency_conflict"
        );
        assert_eq!(service.boundaries.prepare_calls.load(Ordering::SeqCst), 1);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_thread_binding_uses_next_ordinal_and_opens_both_runs() {
        let (mut service, thread_id, first_run_id) = attach_service_with_thread();
        let accepted = attach_start_on(
            &mut service,
            "hello",
            "018f0000-0000-7000-8000-000000000001",
            "018f0000-0000-7000-8000-000000000002",
            Some(&thread_id),
        )
        .unwrap();

        assert_eq!(accepted.thread_id, thread_id);
        let run_page = service
            .boundaries
            .journal
            .lock()
            .unwrap()
            .thread_run_ids(&thread_id, 10, None)
            .unwrap();
        assert_eq!(run_page.run_ids, [first_run_id, accepted.run_id.clone()]);
        let opened = service
            .open_thread(
                "workspace-a",
                ThreadOpenRequest {
                    thread_id: thread_id.clone(),
                    limit: 10,
                    cursor: None,
                },
            )
            .unwrap();
        assert_eq!(opened.thread_id, thread_id);
        assert_eq!(opened.entries.len(), 2);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_thread_binding_rejections_leave_the_journal_unchanged() {
        for case in [
            "unknown",
            "tombstoned",
            "wrong-workspace",
            "foreign-profile",
        ] {
            let (mut service, thread_id, first_run_id) = attach_service_with_thread();
            if case == "tombstoned" {
                service
                    .boundaries
                    .journal
                    .lock()
                    .unwrap()
                    .append_thread_deleted(
                        1,
                        &thread_id,
                        "2026-08-03T00:00:00Z",
                        &attach_test_provenance(),
                    )
                    .unwrap();
            }
            let event_count = service
                .boundaries
                .journal
                .lock()
                .unwrap()
                .run_event_types()
                .unwrap()
                .len();
            if case == "wrong-workspace" {
                service
                    .boundaries
                    .granted_workspaces
                    .push("workspace-b".into());
            }
            let selected = if case == "unknown" {
                "0190a100-0000-7000-8000-000000000099"
            } else {
                &thread_id
            };
            let workspace = if case == "wrong-workspace" {
                "workspace-b"
            } else {
                "workspace-a"
            };
            let result = service.start_run(
                workspace,
                AttachRunStartRequest {
                    text: "hello".into(),
                    context: None,
                    thread_id: Some(selected.into()),
                },
                &Id::new("018f0000-0000-7000-8000-000000000001").unwrap(),
                &Id::new("018f0000-0000-7000-8000-000000000002").unwrap(),
                CompanionProvenance {
                    profile: if case == "foreign-profile" {
                        "another-profile"
                    } else {
                        "default"
                    }
                    .into(),
                    companion_kind: "cli".into(),
                    companion_version: "1.2.3".into(),
                    peer_uid: 1000,
                    peer_pid: 42,
                },
            );
            assert_eq!(
                serde_json::to_value(result.unwrap_err()).unwrap()["code"],
                "thread_not_found",
                "{case}"
            );
            let mut journal = service.boundaries.journal.lock().unwrap();
            assert_eq!(journal.events(&first_run_id).unwrap().len(), 1, "{case}");
            assert_eq!(
                journal.run_event_types().unwrap().len(),
                event_count,
                "{case}"
            );
            assert_eq!(service.boundaries.launch_calls.load(Ordering::SeqCst), 0);
            assert_eq!(
                service
                    .boundaries
                    .prompt_protection_calls
                    .load(Ordering::SeqCst),
                0,
                "{case}"
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn prompt_protection_failure_leaves_bound_and_new_thread_ordinals_unchanged() {
        for bound in [false, true] {
            let (mut service, thread_id, first_run_id) = attach_service_with_thread();
            service.boundaries.protect_error = Some("keyring failed".into());
            let before = service
                .boundaries
                .journal
                .lock()
                .unwrap()
                .run_event_types()
                .unwrap();

            let result = attach_start_on(
                &mut service,
                "hello",
                "018f0000-0000-7000-8000-000000000001",
                "018f0000-0000-7000-8000-000000000002",
                bound.then_some(thread_id.as_str()),
            );

            assert_eq!(
                serde_json::to_value(result.unwrap_err()).unwrap()["code"],
                "persistence_failed"
            );
            let mut journal = service.boundaries.journal.lock().unwrap();
            assert_eq!(journal.run_event_types().unwrap(), before, "bound={bound}");
            let page = journal.thread_run_ids(&thread_id, 10, None).unwrap();
            assert_eq!(page.run_ids, [first_run_id], "bound={bound}");
            assert_eq!(
                service
                    .boundaries
                    .prompt_protection_calls
                    .load(Ordering::SeqCst),
                1
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_adapter_rejects_in_flight_and_unsupported_context() {
        let boundaries = FakeRunStartBoundaries {
            active: true,
            ..FakeRunStartBoundaries::accepting()
        };
        let (in_flight, _) = attach_start(boundaries, None);
        assert_eq!(
            serde_json::to_value(in_flight.unwrap_err()).unwrap()["code"],
            "invalid_request"
        );

        let (context, service) = attach_start(
            FakeRunStartBoundaries::accepting(),
            Some(json!({"cwd": "/private/path"})),
        );
        assert_eq!(
            serde_json::to_value(context.unwrap_err()).unwrap()["code"],
            "unsupported_operation"
        );
        assert_eq!(service.boundaries.auth_calls.load(Ordering::SeqCst), 0);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_adapter_rejects_a_grant_for_another_workspace() {
        let mut service = DesktopAttachService {
            boundaries: FakeRunStartBoundaries::accepting(),
            idempotency: IdempotencyStore::open(":memory:").unwrap(),
            home: PathBuf::new(),
            workspace_contexts: Arc::new(Mutex::new(HashMap::new())),
            client_credentials: Arc::new(Mutex::new(HashMap::new())),
            credential_path: None,
            client_identity: Some("default".into()),
        };
        let result = service.start_run(
            "workspace-b",
            AttachRunStartRequest {
                text: "hello".into(),
                context: None,
                thread_id: None,
            },
            &Id::new("018f0000-0000-7000-8000-000000000001").unwrap(),
            &Id::new("018f0000-0000-7000-8000-000000000002").unwrap(),
            CompanionProvenance {
                profile: "default".into(),
                companion_kind: "cli".into(),
                companion_version: "1.2.3".into(),
                peer_uid: 1000,
                peer_pid: 42,
            },
        );

        assert_eq!(
            serde_json::to_value(result.unwrap_err()).unwrap()["code"],
            "unauthorized"
        );
        assert_eq!(
            service
                .boundaries
                .prompt_protection_calls
                .load(Ordering::SeqCst),
            0
        );
        assert!(service.boundaries.launched_run.lock().unwrap().is_none());
        assert!(service
            .boundaries
            .prepared_provenance
            .lock()
            .unwrap()
            .is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_adapter_redacts_coordinator_failure_and_clears_active_run() {
        let boundaries = FakeRunStartBoundaries {
            prepare_error: Some(
                "token secret-token path /private/work sidecar socket unavailable".into(),
            ),
            ..FakeRunStartBoundaries::accepting()
        };
        let (result, service) = attach_start(boundaries, None);
        let encoded = serde_json::to_string(&result.unwrap_err()).unwrap();

        assert!(encoded.contains("persistence_failed"));
        assert!(!encoded.contains("secret-token"));
        assert!(!encoded.contains("/private/work"));
        assert!(!encoded.contains("sidecar"));
        assert_eq!(service.boundaries.clear_calls.load(Ordering::SeqCst), 1);
        assert!(service.boundaries.launched_run.lock().unwrap().is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_adapter_does_not_launch_when_receipt_finalization_fails() {
        let mut service = DesktopAttachService {
            boundaries: FakeRunStartBoundaries::accepting(),
            idempotency: FailingFinalization,
            home: PathBuf::new(),
            workspace_contexts: Arc::new(Mutex::new(HashMap::new())),
            client_credentials: Arc::new(Mutex::new(HashMap::new())),
            credential_path: None,
            client_identity: Some("default".into()),
        };
        let result = service.start_run(
            "workspace-a",
            AttachRunStartRequest {
                text: "private prompt".into(),
                context: None,
                thread_id: None,
            },
            &Id::new("018f0000-0000-7000-8000-000000000001").unwrap(),
            &Id::new("018f0000-0000-7000-8000-000000000002").unwrap(),
            CompanionProvenance {
                profile: "default".into(),
                companion_kind: "cli".into(),
                companion_version: "1.2.3".into(),
                peer_uid: 1000,
                peer_pid: 42,
            },
        );

        let encoded = serde_json::to_string(&result.unwrap_err()).unwrap();
        assert!(encoded.contains("persistence_failed"));
        assert!(!encoded.contains("private prompt"));
        assert_eq!(service.boundaries.launch_calls.load(Ordering::SeqCst), 0);
        assert_eq!(service.boundaries.clear_calls.load(Ordering::SeqCst), 1);
        let journaled_events = service.boundaries.journaled_events.lock().unwrap();
        let events = journaled_events.values().next().unwrap();
        assert_eq!(events.last().unwrap().event_type, "run.failed");
        assert!(reduce(events).unwrap().is_terminal());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_adapter_classifies_and_redacts_every_coordinator_failure_stage() {
        const DETAIL: &str =
            "prompt private-prompt token secret-token path /private/work sidecar socket";
        let cases = [
            (
                "unauthorized",
                FakeRunStartBoundaries {
                    auth_error: Some(DETAIL.into()),
                    ..FakeRunStartBoundaries::accepting()
                },
            ),
            (
                "persistence_failed",
                FakeRunStartBoundaries {
                    configure_error: Some(DETAIL.into()),
                    ..FakeRunStartBoundaries::accepting()
                },
            ),
            (
                "invalid_request",
                FakeRunStartBoundaries {
                    install_error: Some(DETAIL.into()),
                    ..FakeRunStartBoundaries::accepting()
                },
            ),
            (
                "persistence_failed",
                FakeRunStartBoundaries {
                    prepare_error: Some(DETAIL.into()),
                    ..FakeRunStartBoundaries::accepting()
                },
            ),
            (
                "persistence_failed",
                FakeRunStartBoundaries {
                    projection_error: Some(DETAIL.into()),
                    ..FakeRunStartBoundaries::accepting()
                },
            ),
        ];

        for (expected_code, boundaries) in cases {
            let (result, _) = attach_start(boundaries, None);
            let encoded = serde_json::to_string(&result.unwrap_err()).unwrap();
            assert!(encoded.contains(expected_code), "{encoded}");
            for secret in ["private-prompt", "secret-token", "/private/work", "sidecar"] {
                assert!(!encoded.contains(secret), "leaked {secret}: {encoded}");
            }
        }
    }
}
