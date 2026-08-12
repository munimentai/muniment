#[cfg(target_os = "linux")]
use muniment_core::attach::ApprovalRequest;
#[cfg(target_os = "linux")]
use muniment_core::attach::{
    bounded_claim, load_client_credentials, save_client_credentials as persist_client_credentials,
    ClientCredential, CompanionRegistry, WorkspaceContextMap,
};
use muniment_core::attach::{ApprovalCoordinator, ProtocolError};
use std::collections::HashMap;
#[cfg(target_os = "linux")]
use std::collections::{BTreeMap, BTreeSet};
#[cfg(target_os = "linux")]
use std::path::PathBuf;
use std::sync::Mutex;
#[cfg(target_os = "linux")]
use std::sync::{Arc, Condvar};
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use muniment_core::attach::linux::{
    approval_waiter_with_claims, attach_listener_start_diagnostic,
    run_authenticated_session_with_service_approvals_registry_and_migration, AttachAcceptError,
    AttachFilesystem, AttachListenerStartFailure, AttachStopHandle, AttachTransport,
    CompanionProvenance, LiveConnectionRegistry, MigrationControlSessionDependencies,
    PermissionAnswerAccepted, PermissionAnswerRequest, PermissionDecision, RunCancelAccepted,
    RunCancelRequest, RunStartAccepted, RunStartRequest as AttachRunStartRequest, RunStreamPage,
    ThreadCreateAccepted, ThreadListPage, ThreadListRequest, ThreadListService, ThreadOpenPage,
    ThreadOpenRequest,
};
#[cfg(all(target_os = "linux", test))]
use muniment_core::attach::linux::{
    run_authenticated_session_with_service_and_approvals,
    run_authenticated_session_with_service_approvals_and_registry,
};
#[cfg(target_os = "linux")]
use muniment_core::attach::{
    evaluate_quiesce, probe_handoff, verify_migration_control_peer, Approval,
    AttachListenerLifecycle, CommittedResult, ConfirmedHandoff, HandoffProbeError, Id,
    IdempotencyOutcome, IdempotencyStore, MigrationAuthorityError, Operation, PreparedHandoffSlot,
    Protocol, Request as AttachRequest, RuntimeActivity, RuntimeActivityRegistry,
    WorkspaceOnboardRequest, WorkspaceOnboarded,
};
#[cfg(target_os = "linux")]
use muniment_core::browser_control::ProcReader;
#[cfg(target_os = "linux")]
use muniment_core::journal::Provenance;
#[cfg(target_os = "linux")]
use muniment_core::permission_gate::ChatPermissionAnswer;
#[cfg(target_os = "linux")]
use serde_json::{json, Value};
#[cfg(target_os = "linux")]
use tauri::{Emitter, Manager};
#[cfg(target_os = "linux")]
use uuid::Uuid;

#[cfg(target_os = "linux")]
fn decide_migration_control(
    peer_result: Result<(), MigrationAuthorityError>,
    activity: RuntimeActivity,
    slot: &mut PreparedHandoffSlot,
    nonce: String,
    deadline_ms: u64,
    now: Instant,
) -> Result<(), ProtocolError> {
    peer_result.map_err(|_| ProtocolError::unauthorized())?;
    evaluate_quiesce(activity).map_err(|_| ProtocolError::migration_not_ready())?;
    slot.prepare(nonce, deadline_ms, now)
        .map_err(|_| ProtocolError::migration_not_ready())
}

#[cfg(target_os = "linux")]
pub(crate) fn control_desktop_migration<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    request: muniment_core::attach::linux::MigrationControlRequest,
    peer_pid: u32,
) -> Result<(), ProtocolError> {
    let nonce = request.handoff_nonce;
    let deadline_ms = request.deadline_ms;
    let prepared_at = Instant::now();
    let expected_executable = app
        .path()
        .resource_dir()
        .map_err(|_| ProtocolError::unauthorized())?
        .join("muniment-runtime");
    let peer_result = verify_migration_control_peer(peer_pid, &expected_executable).map(|_| ());
    let activity = app.state::<RuntimeActivityRegistry>().snapshot();
    let handoff_state = app.state::<Mutex<PreparedHandoffSlot>>();
    let mut slot = handoff_state
        .lock()
        .map_err(|_| ProtocolError::migration_not_ready())?;
    decide_migration_control(
        peer_result,
        activity,
        &mut slot,
        nonce.clone(),
        deadline_ms,
        prepared_at,
    )?;
    drop(slot);

    let handoff_app = app.clone();
    std::thread::spawn(move || {
        let Ok(filesystem) = AttachFilesystem::from_environment() else {
            stop_attach_listener(&handoff_app);
            cancel_handoff_and_restart(
                &handoff_app,
                &nonce,
                "runtime service readiness filesystem lookup failed",
                || start_attach_listener(handoff_app.clone()),
            );
            return;
        };
        let _ = release_prepared_handoff(
            &handoff_app,
            filesystem.endpoint_path(),
            &nonce,
            prepared_at + Duration::from_millis(deadline_ms),
            || start_attach_listener(handoff_app.clone()),
        );
    });
    Ok(())
}

#[cfg(target_os = "linux")]
fn release_prepared_handoff<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    endpoint: &std::path::Path,
    nonce: &str,
    deadline: Instant,
    restart: impl FnOnce(),
) -> Result<ConfirmedHandoff, HandoffProbeError> {
    stop_attach_listener(app);
    match probe_handoff(endpoint, nonce, deadline) {
        Ok(confirmed) => {
            eprintln!("runtime service handoff confirmed");
            Ok(confirmed)
        }
        Err(error) => {
            cancel_handoff_and_restart(app, nonce, &error.to_string(), restart);
            Err(error)
        }
    }
}

#[cfg(target_os = "linux")]
fn cancel_handoff_and_restart<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    nonce: &str,
    reason: &str,
    restart: impl FnOnce(),
) {
    let cancelled = app
        .state::<Mutex<PreparedHandoffSlot>>()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .cancel_if_matches(nonce);
    if cancelled {
        restart();
    }
    eprintln!("runtime service handoff failed: {reason}");
}

#[cfg(target_os = "linux")]
use crate::chat::TauriRunStartBoundaries;
#[cfg(target_os = "linux")]
pub use muniment_core::attach::{DesktopAttachService, RunStartIdempotency};
#[cfg(target_os = "linux")]
type WorkspaceContexts = Arc<Mutex<WorkspaceContextMap>>;

#[cfg(target_os = "linux")]
pub(crate) struct AttachListenerState {
    workspace_contexts: WorkspaceContexts,
    client_credentials: Arc<Mutex<HashMap<String, ClientCredential>>>,
    companion_registry: CompanionRegistry,
    workspace: Arc<Mutex<Option<String>>>,
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
    listener: Mutex<Option<Arc<AttachListenerState>>>,
    #[cfg(target_os = "linux")]
    workspace: Arc<Mutex<Option<String>>>,
    #[cfg(target_os = "linux")]
    listener_lifecycle: Mutex<AttachListenerLifecycle>,
    #[cfg(target_os = "linux")]
    listener_stop: Mutex<AttachListenerStopState>,
    #[cfg(target_os = "linux")]
    listener_stopped: Condvar,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct AttachListenerStatus {
    started: bool,
    failure: Option<&'static str>,
    pending: bool,
    stopped: bool,
}

#[cfg(target_os = "linux")]
enum AttachListenerStopState {
    Pending { stop_requested: bool },
    Listening(AttachStopHandle),
    Stopped,
}

#[cfg(target_os = "linux")]
impl AttachCompanionState {
    fn new(listener: Arc<AttachListenerState>) -> Self {
        Self {
            workspace: listener.workspace.clone(),
            listener: Mutex::new(Some(listener)),
            listener_lifecycle: Mutex::new(AttachListenerLifecycle::Listening),
            listener_stop: Mutex::new(AttachListenerStopState::Pending {
                stop_requested: false,
            }),
            listener_stopped: Condvar::new(),
        }
    }

    fn set_listener(&self, listener: Arc<AttachListenerState>) {
        *self
            .listener
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(listener);
    }

    fn listener(&self) -> Result<Arc<AttachListenerState>, ProtocolError> {
        self.listener
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?
            .clone()
            .ok_or_else(ProtocolError::persistence_failed)
    }

    fn record_listener_started(&self) {
        self.listener_lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .record_listening();
    }

    fn record_listener_pending(&self) {
        self.listener_lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .record_pending();
        *self
            .listener_stop
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            AttachListenerStopState::Pending {
                stop_requested: false,
            };
    }

    fn record_listener_start_failure(&self, failure: AttachListenerStartFailure) {
        self.listener_lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .record_failure(failure);
        self.record_listener_finished();
    }

    fn publish_listener_stop(&self, stop: AttachStopHandle) {
        let mut listener_stop = self
            .listener_stop
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(
            *listener_stop,
            AttachListenerStopState::Pending {
                stop_requested: true
            }
        ) {
            stop.stop();
        }
        *listener_stop = AttachListenerStopState::Listening(stop);
    }

    fn record_listener_stopped(&self) {
        self.listener_lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .record_stopped();
        self.record_listener_finished();
    }

    fn record_listener_finished(&self) {
        *self
            .listener_stop
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = AttachListenerStopState::Stopped;
        self.listener_stopped.notify_all();
    }

    fn stop_listener(&self) {
        let mut stop = self
            .listener_stop
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match &mut *stop {
            AttachListenerStopState::Pending { stop_requested } => *stop_requested = true,
            AttachListenerStopState::Listening(handle) => handle.stop(),
            AttachListenerStopState::Stopped => return,
        }
        while !matches!(*stop, AttachListenerStopState::Stopped) {
            stop = self
                .listener_stopped
                .wait(stop)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    fn listener_status(&self) -> AttachListenerStatus {
        let lifecycle = *self
            .listener_lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let failure = match lifecycle {
            AttachListenerLifecycle::Failed(failure) => Some(failure),
            _ => None,
        };
        AttachListenerStatus {
            started: lifecycle == AttachListenerLifecycle::Listening,
            failure: failure.map(|failure| match failure {
                AttachListenerStartFailure::Filesystem => "filesystem",
                AttachListenerStartFailure::InstanceLock => "instance_lock",
                AttachListenerStartFailure::Bind => "bind",
            }),
            pending: lifecycle == AttachListenerLifecycle::Pending,
            stopped: lifecycle == AttachListenerLifecycle::Stopped,
        }
    }

    pub(crate) fn record_workspace(&self, workspace: String) {
        *self
            .workspace
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(workspace);
    }

    pub(crate) fn clear_workspace(&self) {
        *self
            .workspace
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    pub(crate) fn approval(&self) -> Option<Approval> {
        let workspace = self
            .workspace
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()?;
        Some(desktop_attach_approval(workspace))
    }
}

#[cfg(target_os = "linux")]
impl Default for AttachCompanionState {
    fn default() -> Self {
        Self {
            listener: Mutex::new(None),
            workspace: Arc::new(Mutex::new(None)),
            listener_lifecycle: Mutex::new(AttachListenerLifecycle::Pending),
            listener_stop: Mutex::new(AttachListenerStopState::Pending {
                stop_requested: false,
            }),
            listener_stopped: Condvar::new(),
        }
    }
}

#[cfg(not(target_os = "linux"))]
impl Default for AttachCompanionState {
    fn default() -> Self {
        Self {}
    }
}

#[cfg(not(target_os = "linux"))]
impl AttachCompanionState {
    pub(crate) fn record_workspace(&self, _workspace: String) {}

    pub(crate) fn clear_workspace(&self) {}
}

#[tauri::command]
pub fn attach_companions(
    state: tauri::State<'_, AttachCompanionState>,
) -> Result<Vec<AuthorizedCompanion>, ProtocolError> {
    #[cfg(target_os = "linux")]
    return state.listener()?.list_companions();

    #[cfg(not(target_os = "linux"))]
    {
        let _ = state;
        Ok(Vec::new())
    }
}

#[tauri::command]
pub fn attach_listener_status(
    state: tauri::State<'_, AttachCompanionState>,
) -> AttachListenerStatus {
    #[cfg(target_os = "linux")]
    return state.listener_status();

    #[cfg(not(target_os = "linux"))]
    {
        let _ = state;
        AttachListenerStatus {
            started: true,
            failure: None,
            pending: false,
            stopped: false,
        }
    }
}

#[tauri::command]
pub fn attach_revoke_companion(
    state: tauri::State<'_, AttachCompanionState>,
    client_identity: String,
) -> Result<(), ProtocolError> {
    #[cfg(target_os = "linux")]
    return state.listener()?.revoke_companion(&client_identity);

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (state, client_identity);
        Err(ProtocolError::unsupported_operation())
    }
}

#[cfg(target_os = "linux")]
impl AttachListenerState {
    fn load(credential_path: &std::path::Path) -> Result<Self, ProtocolError> {
        Self::load_with_workspace(credential_path, Arc::new(Mutex::new(None)))
    }

    fn load_with_workspace(
        credential_path: &std::path::Path,
        workspace: Arc<Mutex<Option<String>>>,
    ) -> Result<Self, ProtocolError> {
        let client_credentials = Arc::new(Mutex::new(load_client_credentials(credential_path)?));
        let companion_registry = CompanionRegistry::new(
            client_credentials.clone(),
            credential_path,
            LiveConnectionRegistry::default(),
        );
        Ok(Self {
            workspace_contexts: Arc::new(Mutex::new(WorkspaceContextMap::default())),
            client_credentials,
            companion_registry,
            workspace,
        })
    }

    fn approval(&self) -> Option<Approval> {
        let workspace = self
            .workspace
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()?;
        Some(desktop_attach_approval(workspace))
    }

    pub(crate) fn revoke_companion(&self, client_identity: &str) -> Result<(), ProtocolError> {
        self.companion_registry.revoke(client_identity)
    }

    pub(crate) fn list_companions(&self) -> Result<Vec<AuthorizedCompanion>, ProtocolError> {
        self.companion_registry.list().map(|companions| {
            companions
                .into_iter()
                .map(|companion| AuthorizedCompanion {
                    identity: companion.identity,
                    claimed_kind: companion.claimed_kind,
                    claimed_version: companion.claimed_version,
                    approved_at: companion.approved_at,
                })
                .collect()
        })
    }
}

pub type AttachApprovalState = ApprovalCoordinator;

#[cfg(target_os = "linux")]
#[derive(serde::Serialize)]
struct AttachPairingRequest {
    challenge: String,
    claimed_kind: String,
    claimed_version: String,
    workspace: String,
    scopes: BTreeSet<String>,
}

#[tauri::command]
pub fn attach_pairing_decide(
    state: tauri::State<'_, AttachApprovalState>,
    challenge: String,
    approve: bool,
) {
    state.decide(&challenge, approve);
}

#[cfg(target_os = "linux")]
fn resolve_attach_home(
    documents: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Result<PathBuf, ProtocolError> {
    muniment_core::home::choose_default_home(documents, home)
        .map_err(|_| ProtocolError::persistence_failed())
}

#[cfg(target_os = "linux")]
trait TauriDesktopAttachService<R: tauri::Runtime> {
    fn new(
        app: tauri::AppHandle<R>,
        workspace_contexts: WorkspaceContexts,
        client_credentials: Arc<Mutex<HashMap<String, ClientCredential>>>,
    ) -> Result<Self, ProtocolError>
    where
        Self: Sized;
}

#[cfg(target_os = "linux")]
impl<R: tauri::Runtime> TauriDesktopAttachService<R>
    for DesktopAttachService<TauriRunStartBoundaries<R>>
{
    fn new(
        app: tauri::AppHandle<R>,
        workspace_contexts: WorkspaceContexts,
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
fn desktop_attach_approval(workspace: String) -> Approval {
    Approval {
        profile: "desktop-owner".into(),
        workspace,
        scopes: BTreeSet::from(["thread.read".into(), "run.write".into()]),
        lifetime: Duration::from_secs(60 * 60),
    }
}

#[cfg(target_os = "linux")]
fn request_attach_pairing_approval(
    approval_state: &AttachListenerState,
    approvals: &AttachApprovalState,
    challenge: &str,
    claimed_kind: &str,
    claimed_version: &str,
    remaining: Duration,
) -> muniment_core::attach::linux::ApprovalDecision {
    let Some(approval) = approval_state.approval() else {
        return muniment_core::attach::linux::ApprovalDecision::Deny;
    };
    let approved = approvals.request(
        ApprovalRequest {
            challenge: challenge.to_owned(),
            claimed_kind: bounded_claim(claimed_kind),
            claimed_version: bounded_claim(claimed_version),
            workspace: approval.workspace.clone(),
            scopes: approval.scopes.clone(),
        },
        remaining,
    );
    if !approved {
        return muniment_core::attach::linux::ApprovalDecision::Deny;
    }
    match approval_state.approval() {
        Some(current) if current.workspace == approval.workspace => {
            muniment_core::attach::linux::ApprovalDecision::Approve(current)
        }
        _ => muniment_core::attach::linux::ApprovalDecision::Deny,
    }
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
fn initialize_attach_listener<R, F>(
    app: &tauri::AppHandle<R>,
    credential_path: F,
) -> Option<Arc<AttachListenerState>>
where
    R: tauri::Runtime,
    F: FnOnce() -> Option<PathBuf>,
{
    app.manage(AttachCompanionState::default());
    let Some(credential_path) = credential_path() else {
        app.state::<AttachCompanionState>()
            .record_listener_start_failure(AttachListenerStartFailure::Filesystem);
        return None;
    };
    let workspace = app.state::<AttachCompanionState>().workspace.clone();
    let Ok(state) = AttachListenerState::load_with_workspace(&credential_path, workspace) else {
        app.state::<AttachCompanionState>()
            .record_listener_start_failure(AttachListenerStartFailure::Filesystem);
        return None;
    };
    let state = Arc::new(state);
    app.state::<AttachCompanionState>()
        .set_listener(state.clone());
    Some(state)
}

#[cfg(target_os = "linux")]
pub fn start_attach_listener<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    if app.try_state::<AttachCompanionState>().is_some() {
        app.state::<AttachCompanionState>()
            .record_listener_pending();
    }
    let Some(state) = initialize_attach_listener(&app, || {
        app.path()
            .app_data_dir()
            .ok()
            .map(|path| path.join("attach-client-credentials.json"))
    }) else {
        return;
    };
    let approval_app = app.clone();
    app.state::<AttachApprovalState>()
        .register_presenter(move |request| {
            let request = AttachPairingRequest {
                challenge: request.challenge.clone(),
                claimed_kind: request.claimed_kind.clone(),
                claimed_version: request.claimed_version.clone(),
                workspace: request.workspace.clone(),
                scopes: request.scopes.clone(),
            };
            approval_app
                .emit("attach-pairing-requested", &request)
                .is_ok()
        });
    std::thread::spawn(move || {
        let Ok(filesystem) = AttachFilesystem::from_environment() else {
            app.state::<AttachCompanionState>()
                .record_listener_start_failure(AttachListenerStartFailure::Filesystem);
            eprintln!(
                "{}",
                attach_listener_start_diagnostic(AttachListenerStartFailure::Filesystem)
            );
            return;
        };
        run_attach_listener(app, state, filesystem);
    });
}

#[cfg(target_os = "linux")]
fn run_attach_listener<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: Arc<AttachListenerState>,
    filesystem: AttachFilesystem,
) {
    run_attach_listener_with_hooks(app, state, filesystem, || {}, || {});
}

#[cfg(target_os = "linux")]
fn run_attach_listener_with_hooks<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: Arc<AttachListenerState>,
    filesystem: AttachFilesystem,
    after_lock: impl FnOnce(),
    after_bind_failure: impl FnOnce(),
) {
    let Ok(instance_lock) = filesystem.acquire_instance_lock() else {
        app.state::<AttachCompanionState>()
            .record_listener_start_failure(AttachListenerStartFailure::InstanceLock);
        eprintln!(
            "{}",
            attach_listener_start_diagnostic(AttachListenerStartFailure::InstanceLock)
        );
        return;
    };
    after_lock();
    let Ok(listener) = AttachTransport::bind(&filesystem) else {
        drop(instance_lock);
        app.state::<AttachCompanionState>()
            .record_listener_start_failure(AttachListenerStartFailure::Bind);
        eprintln!(
            "{}",
            attach_listener_start_diagnostic(AttachListenerStartFailure::Bind)
        );
        after_bind_failure();
        return;
    };
    let companion_state = app.state::<AttachCompanionState>();
    companion_state.publish_listener_stop(listener.stop_handle());
    companion_state.record_listener_started();
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
        let live_connections = state.companion_registry.live_connections();
        let approval_state = state.clone();
        std::thread::spawn(move || {
            let Ok(mut service) =
                DesktopAttachService::new(app, workspace_contexts, client_credentials)
            else {
                return;
            };
            let approvals = service
                .boundaries
                .app
                .state::<AttachApprovalState>()
                .inner()
                .clone();
            let expected_executable = service
                .boundaries
                .app
                .path()
                .resource_dir()
                .ok()
                .map(|directory| directory.join("muniment-runtime"))
                .unwrap_or_else(|| PathBuf::from("muniment-runtime"));
            let process_reader = ProcReader;
            let _ = run_authenticated_session_with_service_approvals_registry_and_migration(
                stream,
                credentials,
                env!("CARGO_PKG_VERSION"),
                &mut service,
                approval_waiter_with_claims(
                    move |challenge: &muniment_core::attach::PairingChallenge,
                          claimed_kind: &str,
                          claimed_version: &str,
                          remaining: Duration| {
                        Some(request_attach_pairing_approval(
                            &approval_state,
                            &approvals,
                            challenge.as_str(),
                            claimed_kind,
                            claimed_version,
                            remaining,
                        ))
                    },
                ),
                &live_connections,
                MigrationControlSessionDependencies {
                    expected_executable: &expected_executable,
                    process_reader: &process_reader,
                },
            );
        });
    }
    drop(listener);
    drop(instance_lock);
    companion_state.record_listener_stopped();
}

#[cfg(target_os = "linux")]
pub fn stop_attach_listener<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    app.state::<AttachCompanionState>().stop_listener();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{append_test_event, FakeRunStartBoundaries};
    use muniment_core::attach::{decode_frame, encode_frame, Authorization, ErrorCode, Welcome};
    use muniment_core::journal::reducer::reduce;
    use muniment_core::journal::RunJournal;

    #[cfg(target_os = "linux")]
    fn handoff_test_runtime(name: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let runtime = std::env::temp_dir().join(format!("mt-{name}-{}", Uuid::now_v7().simple()));
        std::fs::create_dir(&runtime).unwrap();
        std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
        runtime
    }

    #[cfg(target_os = "linux")]
    fn wait_for_listener(state: &AttachCompanionState) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !state.listener_status().started {
            assert!(Instant::now() < deadline, "attach listener did not start");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(target_os = "linux")]
    fn answer_handoff_probe(stream: &mut std::os::unix::net::UnixStream, nonce: &str) {
        use std::io::{Read, Write};

        let mut prefix = [0_u8; 4];
        stream.read_exact(&mut prefix).unwrap();
        let mut frame = vec![0_u8; 4 + u32::from_be_bytes(prefix) as usize];
        frame[..4].copy_from_slice(&prefix);
        stream.read_exact(&mut frame[4..]).unwrap();
        let _: muniment_core::attach::Hello = decode_frame(&frame).unwrap().unwrap().0;
        let welcome = Welcome {
            selected: 1,
            desktop_version: "1.0.0".into(),
            server_nonce: "server-nonce".into(),
            authorization: Authorization::Authorized,
            approval_challenge: "challenge".into(),
            handoff_nonce: Some(nonce.into()),
        };
        stream.write_all(&encode_frame(&welcome).unwrap()).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn migration_control_prepares_a_handoff() {
        let now = Instant::now();
        let mut slot = PreparedHandoffSlot::new();

        decide_migration_control(
            Ok(()),
            RuntimeActivity::default(),
            &mut slot,
            "nonce-a".into(),
            1_000,
            now,
        )
        .unwrap();

        assert!(slot.matches("nonce-a", now));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn migration_control_rejects_a_quiesce_blocker() {
        let error = decide_migration_control(
            Ok(()),
            RuntimeActivity {
                active_run: true,
                ..RuntimeActivity::default()
            },
            &mut PreparedHandoffSlot::new(),
            "nonce-a".into(),
            1_000,
            Instant::now(),
        )
        .unwrap_err();

        assert_eq!(error.code(), ErrorCode::MigrationNotReady);
        assert!(error.retryable());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn migration_control_rejects_an_unverified_peer() {
        let error = decide_migration_control(
            Err(MigrationAuthorityError::ExecutableMismatch),
            RuntimeActivity::default(),
            &mut PreparedHandoffSlot::new(),
            "nonce-a".into(),
            1_000,
            Instant::now(),
        )
        .unwrap_err();

        assert_eq!(error.code(), ErrorCode::Unauthorized);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn migration_control_rejects_a_second_live_preparation() {
        let now = Instant::now();
        let mut slot = PreparedHandoffSlot::new();
        decide_migration_control(
            Ok(()),
            RuntimeActivity::default(),
            &mut slot,
            "nonce-a".into(),
            1_000,
            now,
        )
        .unwrap();

        let error = decide_migration_control(
            Ok(()),
            RuntimeActivity::default(),
            &mut slot,
            "nonce-b".into(),
            1_000,
            now,
        )
        .unwrap_err();

        assert_eq!(error.code(), ErrorCode::MigrationNotReady);
        assert!(slot.matches("nonce-a", now));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn release_step_confirms_a_runtime_listener_after_desktop_release() {
        use std::os::unix::net::UnixListener;

        let runtime = handoff_test_runtime("handoff-confirmed");
        let filesystem = AttachFilesystem::from_runtime_directory(&runtime).unwrap();
        let endpoint = filesystem.endpoint_path().to_owned();
        let listener = Arc::new(
            AttachListenerState::load(&runtime.join("attach-client-credentials.json")).unwrap(),
        );
        let app = tauri::test::mock_app();
        app.manage(AttachCompanionState::default());
        app.manage(Mutex::new(PreparedHandoffSlot::new()));
        app.state::<AttachCompanionState>()
            .set_listener(listener.clone());
        app.state::<Mutex<PreparedHandoffSlot>>()
            .lock()
            .unwrap()
            .prepare("handoff-nonce", 2_000, Instant::now())
            .unwrap();

        let listener_app = app.handle().clone();
        let desktop = std::thread::spawn(move || {
            run_attach_listener(listener_app, listener, filesystem);
        });
        wait_for_listener(&app.state::<AttachCompanionState>());

        let runtime_endpoint = endpoint.clone();
        let runtime_service = std::thread::spawn(move || {
            while runtime_endpoint.exists() {
                std::thread::sleep(Duration::from_millis(5));
            }
            let listener = UnixListener::bind(runtime_endpoint).unwrap();
            let (mut stream, _) = listener.accept().unwrap();
            answer_handoff_probe(&mut stream, "handoff-nonce");
        });

        assert!(release_prepared_handoff(
            app.handle(),
            &endpoint,
            "handoff-nonce",
            Instant::now() + Duration::from_secs(2),
            || panic!("a confirmed handoff must not restart the desktop listener"),
        )
        .is_ok());
        desktop.join().unwrap();
        runtime_service.join().unwrap();
        assert_eq!(
            app.state::<AttachCompanionState>().listener_status(),
            AttachListenerStatus {
                started: false,
                failure: None,
                pending: false,
                stopped: true,
            }
        );
        std::fs::remove_dir_all(runtime).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn release_step_reacquires_listener_and_instance_lock_after_probe_failure() {
        use std::os::unix::net::UnixStream;
        use std::sync::mpsc;

        let runtime = handoff_test_runtime("handoff-restart");
        let filesystem = AttachFilesystem::from_runtime_directory(&runtime).unwrap();
        let endpoint = filesystem.endpoint_path().to_owned();
        let listener = Arc::new(
            AttachListenerState::load(&runtime.join("attach-client-credentials.json")).unwrap(),
        );
        let app = tauri::test::mock_app();
        app.manage(AttachCompanionState::default());
        app.manage(Mutex::new(PreparedHandoffSlot::new()));
        app.state::<AttachCompanionState>()
            .set_listener(listener.clone());
        app.state::<Mutex<PreparedHandoffSlot>>()
            .lock()
            .unwrap()
            .prepare("handoff-nonce", 50, Instant::now())
            .unwrap();

        let listener_app = app.handle().clone();
        let desktop = std::thread::spawn(move || {
            run_attach_listener(listener_app, listener.clone(), filesystem);
            listener
        });
        wait_for_listener(&app.state::<AttachCompanionState>());
        let (restart_tx, restart_rx) = mpsc::channel();
        let restart_app = app.handle().clone();
        let restart_runtime = runtime.clone();
        assert_eq!(
            release_prepared_handoff(
                app.handle(),
                &endpoint,
                "handoff-nonce",
                Instant::now() + Duration::from_millis(50),
                move || {
                    let listener = desktop.join().unwrap();
                    restart_app
                        .state::<AttachCompanionState>()
                        .record_listener_pending();
                    let filesystem =
                        AttachFilesystem::from_runtime_directory(&restart_runtime).unwrap();
                    let worker_app = restart_app.clone();
                    restart_tx
                        .send(std::thread::spawn(move || {
                            run_attach_listener(worker_app, listener, filesystem)
                        }))
                        .unwrap();
                },
            ),
            Err(HandoffProbeError::ReadinessDeadlineReached)
        );

        wait_for_listener(&app.state::<AttachCompanionState>());
        assert!(UnixStream::connect(&endpoint).is_ok());
        assert!(AttachFilesystem::from_runtime_directory(&runtime)
            .unwrap()
            .acquire_instance_lock()
            .is_err());
        assert!(!app
            .state::<Mutex<PreparedHandoffSlot>>()
            .lock()
            .unwrap()
            .matches("handoff-nonce", Instant::now()));
        stop_attach_listener(app.handle());
        restart_rx.recv().unwrap().join().unwrap();
        std::fs::remove_dir_all(runtime).unwrap();
    }

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
    fn listener_stop_releases_transport_and_instance_lock() {
        use std::os::unix::fs::PermissionsExt;

        let runtime = std::env::temp_dir().join(format!("mt-stop-{}", Uuid::now_v7().simple()));
        std::fs::create_dir(&runtime).unwrap();
        std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
        let filesystem = AttachFilesystem::from_runtime_directory(&runtime).unwrap();
        let listener = Arc::new(
            AttachListenerState::load(&runtime.join("attach-client-credentials.json")).unwrap(),
        );
        let app = tauri::test::mock_app();
        app.manage(AttachCompanionState::default());
        app.state::<AttachCompanionState>()
            .set_listener(listener.clone());

        let listener_app = app.handle().clone();
        let worker = std::thread::spawn(move || {
            run_attach_listener(listener_app, listener, filesystem);
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while !app
            .state::<AttachCompanionState>()
            .listener_status()
            .started
        {
            assert!(Instant::now() < deadline, "attach listener did not start");
            std::thread::sleep(Duration::from_millis(10));
        }

        stop_attach_listener(app.handle());
        worker.join().unwrap();
        stop_attach_listener(app.handle());

        let filesystem = AttachFilesystem::from_runtime_directory(&runtime).unwrap();
        assert!(!filesystem.endpoint_path().exists());
        let _instance_lock = filesystem.acquire_instance_lock().unwrap();
        assert_eq!(
            app.state::<AttachCompanionState>()
                .listener_status()
                .failure,
            None
        );
        drop(_instance_lock);
        std::fs::remove_dir_all(runtime).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn listener_stop_requested_before_publication_closes_listener_and_waits_for_release() {
        use std::os::unix::fs::PermissionsExt;

        let runtime = std::env::temp_dir().join(format!("mt-early-stop-{}", Uuid::now_v7()));
        std::fs::create_dir(&runtime).unwrap();
        std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
        let filesystem = AttachFilesystem::from_runtime_directory(&runtime).unwrap();
        let instance_lock = filesystem.acquire_instance_lock().unwrap();
        let listener = AttachTransport::bind(&filesystem).unwrap();
        let state = Arc::new(AttachCompanionState::default());
        let stop_state = state.clone();
        let stopper = std::thread::spawn(move || stop_state.stop_listener());

        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let requested = matches!(
                *state
                    .listener_stop
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
                AttachListenerStopState::Pending {
                    stop_requested: true
                }
            );
            if requested {
                break;
            }
            assert!(Instant::now() < deadline, "early stop was not recorded");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!stopper.is_finished());

        state.publish_listener_stop(listener.stop_handle());
        assert_eq!(listener.accept().unwrap_err(), AttachAcceptError::Closed);
        assert!(!stopper.is_finished());
        drop(listener);
        drop(instance_lock);
        state.record_listener_stopped();
        stopper.join().unwrap();

        let filesystem = AttachFilesystem::from_runtime_directory(&runtime).unwrap();
        assert!(!filesystem.endpoint_path().exists());
        let _instance_lock = filesystem.acquire_instance_lock().unwrap();
        std::fs::remove_dir_all(runtime).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn listener_bind_failure_releases_instance_lock_before_stopper_returns() {
        use std::os::unix::fs::PermissionsExt;
        use std::sync::mpsc;

        let runtime = std::env::temp_dir().join(format!("mt-bind-stop-{}", Uuid::now_v7()));
        std::fs::create_dir(&runtime).unwrap();
        std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
        let filesystem = AttachFilesystem::from_runtime_directory(&runtime).unwrap();
        std::fs::write(filesystem.endpoint_path(), "not a socket").unwrap();
        let listener = Arc::new(
            AttachListenerState::load(&runtime.join("attach-client-credentials.json")).unwrap(),
        );
        let app = tauri::test::mock_app();
        app.manage(AttachCompanionState::default());
        app.state::<AttachCompanionState>()
            .set_listener(listener.clone());

        let (locked_tx, locked_rx) = mpsc::channel();
        let (bind_tx, bind_rx) = mpsc::channel();
        let (failed_tx, failed_rx) = mpsc::channel();
        let (finish_tx, finish_rx) = mpsc::channel();
        let listener_app = app.handle().clone();
        let worker = std::thread::spawn(move || {
            run_attach_listener_with_hooks(
                listener_app,
                listener,
                filesystem,
                || {
                    locked_tx.send(()).unwrap();
                    bind_rx.recv().unwrap();
                },
                || {
                    failed_tx.send(()).unwrap();
                    finish_rx.recv().unwrap();
                },
            );
        });
        locked_rx.recv().unwrap();

        let stop_app = app.handle().clone();
        let stopper = std::thread::spawn(move || stop_attach_listener(&stop_app));
        let deadline = Instant::now() + Duration::from_secs(2);
        while !matches!(
            *app.state::<AttachCompanionState>()
                .listener_stop
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            AttachListenerStopState::Pending {
                stop_requested: true
            }
        ) {
            assert!(Instant::now() < deadline, "early stop was not recorded");
            std::thread::sleep(Duration::from_millis(10));
        }

        bind_tx.send(()).unwrap();
        failed_rx.recv().unwrap();
        stopper.join().unwrap();
        let filesystem = AttachFilesystem::from_runtime_directory(&runtime).unwrap();
        let _instance_lock = filesystem.acquire_instance_lock().unwrap();
        finish_tx.send(()).unwrap();
        worker.join().unwrap();

        drop(_instance_lock);
        std::fs::remove_file(filesystem.endpoint_path()).unwrap();
        std::fs::remove_dir_all(runtime).unwrap();
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
    fn attach_listener_status_reports_started_and_each_start_failure() {
        let state = AttachCompanionState::default();

        for (failure, name) in [
            (AttachListenerStartFailure::Filesystem, "filesystem"),
            (AttachListenerStartFailure::InstanceLock, "instance_lock"),
            (AttachListenerStartFailure::Bind, "bind"),
        ] {
            state.record_listener_start_failure(failure);
            assert_eq!(
                state.listener_status(),
                AttachListenerStatus {
                    started: false,
                    failure: Some(name),
                    pending: false,
                    stopped: false,
                }
            );
        }

        state.record_listener_started();
        assert_eq!(
            state.listener_status(),
            AttachListenerStatus {
                started: true,
                failure: None,
                pending: false,
                stopped: false,
            }
        );

        let pending = AttachCompanionState::default().listener_status();
        assert_eq!(
            pending,
            AttachListenerStatus {
                started: false,
                failure: None,
                pending: true,
                stopped: false,
            }
        );

        state.record_listener_stopped();
        assert_eq!(
            state.listener_status(),
            AttachListenerStatus {
                started: false,
                failure: None,
                pending: false,
                stopped: true,
            }
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_approval_uses_recorded_workspace_and_fails_closed_without_one() {
        let credential_path =
            std::env::temp_dir().join(format!("muniment-attach-workspace-{}.json", Uuid::now_v7()));
        let state = AttachCompanionState::new(Arc::new(
            AttachListenerState::load(&credential_path).unwrap(),
        ));

        assert!(state.approval().is_none());
        state.record_workspace("signed-workspace".into());
        assert_eq!(state.approval().unwrap().workspace, "signed-workspace");
        state.clear_workspace();
        assert!(state.approval().is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_pairing_denies_without_a_grant_and_does_not_present() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let credential_path =
            std::env::temp_dir().join(format!("muniment-attach-no-grant-{}.json", Uuid::now_v7()));
        let state = AttachListenerState::load(&credential_path).unwrap();
        let approvals = AttachApprovalState::default();
        let presentations = Arc::new(AtomicUsize::new(0));
        let presenter_count = presentations.clone();
        approvals.register_presenter(move |_| {
            presenter_count.fetch_add(1, Ordering::SeqCst);
            true
        });

        assert!(matches!(
            request_attach_pairing_approval(
                &state,
                &approvals,
                "challenge",
                "cli",
                "1.0.0",
                Duration::from_secs(1),
            ),
            muniment_core::attach::linux::ApprovalDecision::Deny
        ));
        assert_eq!(presentations.load(Ordering::SeqCst), 0);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_pairing_denies_when_grant_clears_during_approval() {
        let credential_path = std::env::temp_dir().join(format!(
            "muniment-attach-cleared-grant-{}.json",
            Uuid::now_v7()
        ));
        let state = AttachListenerState::load(&credential_path).unwrap();
        *state.workspace.lock().unwrap() = Some("workspace-a".into());
        let workspace = state.workspace.clone();
        let approvals = AttachApprovalState::default();
        approvals.register_presenter(move |_| {
            *workspace.lock().unwrap() = None;
            true
        });

        assert!(matches!(
            request_attach_pairing_approval(
                &state,
                &approvals,
                "challenge",
                "cli",
                "1.0.0",
                Duration::from_secs(1),
            ),
            muniment_core::attach::linux::ApprovalDecision::Deny
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_pairing_denies_when_grant_changes_during_approval() {
        let credential_path = std::env::temp_dir().join(format!(
            "muniment-attach-changed-grant-{}.json",
            Uuid::now_v7()
        ));
        let state = AttachListenerState::load(&credential_path).unwrap();
        *state.workspace.lock().unwrap() = Some("workspace-a".into());
        let workspace = state.workspace.clone();
        let approvals = AttachApprovalState::default();
        approvals.register_presenter(move |_| {
            *workspace.lock().unwrap() = Some("workspace-b".into());
            true
        });

        assert!(matches!(
            request_attach_pairing_approval(
                &state,
                &approvals,
                "challenge",
                "cli",
                "1.0.0",
                Duration::from_secs(1),
            ),
            muniment_core::attach::linux::ApprovalDecision::Deny
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn workspace_state_survives_failed_listener_initialization() {
        let assert_state_works = |app: &tauri::App<tauri::test::MockRuntime>| {
            let state = app.state::<AttachCompanionState>();
            assert!(state.approval().is_none());
            state.record_workspace("signed-workspace".into());
            assert_eq!(state.approval().unwrap().workspace, "signed-workspace");
            state.clear_workspace();
            assert!(state.approval().is_none());
        };

        let app = tauri::test::mock_app();
        assert!(initialize_attach_listener(app.handle(), || None).is_none());
        assert_eq!(
            app.state::<AttachCompanionState>().listener_status(),
            AttachListenerStatus {
                started: false,
                failure: Some("filesystem"),
                pending: false,
                stopped: false,
            }
        );
        assert_state_works(&app);

        let credential_path = std::env::temp_dir().join(format!(
            "muniment-invalid-attach-credentials-{}.json",
            Uuid::now_v7()
        ));
        std::fs::write(&credential_path, "invalid").unwrap();
        let app = tauri::test::mock_app();
        assert!(
            initialize_attach_listener(app.handle(), || Some(credential_path.clone())).is_none()
        );
        assert_eq!(
            app.state::<AttachCompanionState>().listener_status(),
            AttachListenerStatus {
                started: false,
                failure: Some("filesystem"),
                pending: false,
                stopped: false,
            }
        );
        assert_state_works(&app);
        std::fs::remove_file(credential_path).unwrap();
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
    #[test]
    fn attach_listener_lists_persisted_client_credentials() {
        let root =
            std::env::temp_dir().join(format!("muniment-attach-credentials-{}", Uuid::now_v7()));
        let path = root.join("credentials.json");
        let identity = "018f0000-0000-7000-8000-000000000099";
        persist_client_credentials(
            &path,
            &HashMap::from([(
                identity.to_owned(),
                ClientCredential {
                    credential: "ab".repeat(32),
                    claimed_kind: "cli".into(),
                    claimed_version: "1.2.3".into(),
                    approved_at: Some("2026-08-04T00:00:00Z".into()),
                },
            )]),
        )
        .unwrap();
        let state = AttachListenerState::load(&path).unwrap();
        assert_eq!(
            state.list_companions().unwrap(),
            vec![AuthorizedCompanion {
                identity: identity.into(),
                claimed_kind: "cli".into(),
                claimed_version: "1.2.3".into(),
                approved_at: Some("2026-08-04T00:00:00Z".into()),
            }]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn legacy_attach_credentials_list_unknown_claims() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!("muniment-attach-legacy-{}", Uuid::now_v7()));
        let path = root.join("credentials.json");
        let identity = "018f0000-0000-7000-8000-000000000099";
        let credential = "ab".repeat(32);
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
        let live_connections = state.companion_registry.live_connections();

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
                    workspace_contexts: Arc::new(Mutex::new(WorkspaceContextMap::default())),
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
                        Some(ApprovalDecision::Approve(desktop_attach_approval(
                            "workspace-a".into(),
                        )))
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
        state.companion_registry = CompanionRegistry::new(
            state.client_credentials.clone(),
            blocker.join("credentials.json"),
            live_connections,
        );
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
        let contexts = Arc::new(Mutex::new(WorkspaceContextMap::default()));
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
}
