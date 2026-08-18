#[cfg(target_os = "linux")]
use muniment_core::attach::ApprovalRequest;
#[cfg(target_os = "linux")]
use muniment_core::attach::ClientError;
#[cfg(target_os = "linux")]
use muniment_core::attach::{
    answer_presented_approval, handshake_desktop_client_stream, interruptible_connect_with_state,
    serve_approval_presenter_at, serve_desktop_client_at, ApprovalPresenterStopHandle,
    DesktopClientHolder, DesktopClientStopHandle, InterruptibleConnectState,
};
#[cfg(target_os = "linux")]
use muniment_core::attach::{
    bounded_claim, load_client_credentials, save_client_credentials as persist_client_credentials,
    ClientCredential, CompanionRegistry, WorkspaceContextMap, COMPANION_CREDENTIAL_FILE_NAME,
};
use muniment_core::attach::{ApprovalCoordinator, ProtocolError};
use std::collections::HashMap;
#[cfg(target_os = "linux")]
use std::collections::{BTreeMap, BTreeSet};
#[cfg(target_os = "linux")]
use std::os::unix::net::UnixStream;
#[cfg(target_os = "linux")]
use std::path::{Path, PathBuf};
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
    evaluate_quiesce, name_attach_connection_route, probe_handoff, verify_migration_control_peer,
    Approval, AttachConnectionRoute, AttachListenerLifecycle, CommittedResult, ConfirmedHandoff,
    HandoffProbeError, Id, IdempotencyOutcome, IdempotencyStore, Operation, PeerAuthorityError,
    PreparedHandoffSlot, Protocol, Request as AttachRequest, RuntimeActivity,
    RuntimeActivityRegistry, SignedWorkspaceApproval, WorkspaceOnboardRequest, WorkspaceOnboarded,
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
pub(crate) const MINIMUM_COMPATIBLE_RUNTIME_VERSION: &str = "0.0.1";

#[cfg(target_os = "linux")]
pub(crate) fn runtime_upgrade_pending(client: &DesktopClientHolder) -> bool {
    runtime_version_upgrade_pending(client.connected_version().as_deref())
}

#[cfg(target_os = "linux")]
fn runtime_version_upgrade_pending(connected_version: Option<&str>) -> bool {
    let minimum = semver::Version::parse(MINIMUM_COMPATIBLE_RUNTIME_VERSION)
        .expect("minimum compatible runtime version must be valid");
    connected_version
        .and_then(|version| semver::Version::parse(version).ok())
        .map_or(true, |version| version < minimum)
}

#[cfg(target_os = "linux")]
fn decide_migration_control(
    peer_result: Result<(), PeerAuthorityError>,
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
            start_approval_presenter(app);
            start_desktop_client(app);
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
    approval: SignedWorkspaceApproval,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
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
    approval: SignedWorkspaceApproval,
    #[cfg(target_os = "linux")]
    listener_lifecycle: Mutex<AttachListenerLifecycle>,
    #[cfg(target_os = "linux")]
    listener_stop: Mutex<AttachListenerStopState>,
    #[cfg(target_os = "linux")]
    listener_stopped: Condvar,
    #[cfg(target_os = "linux")]
    approval_presenter: Mutex<Option<ApprovalPresenterStopHandle>>,
    #[cfg(target_os = "linux")]
    presenting: Mutex<bool>,
    #[cfg(target_os = "linux")]
    desktop_supervisor_lifecycle: Mutex<()>,
    #[cfg(target_os = "linux")]
    desktop_client: Mutex<Option<DesktopClientSupervisor>>,
    #[cfg(target_os = "linux")]
    desktop_client_holder: DesktopClientHolder,
    #[cfg(target_os = "linux")]
    chat_events: Mutex<Option<ChatEventSupervisor>>,
    #[cfg(target_os = "linux")]
    connected: Mutex<bool>,
    #[cfg(target_os = "linux")]
    chat_events_connected: Mutex<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct AttachListenerStatus {
    started: bool,
    failure: Option<&'static str>,
    pending: bool,
    stopped: bool,
    presenting: bool,
    supervisor_running: bool,
    connected: bool,
    chat_events_connected: bool,
    runtime_upgrade_pending: bool,
}

#[cfg(target_os = "linux")]
enum AttachListenerStopState {
    Pending { stop_requested: bool },
    Listening(AttachStopHandle),
    Stopped,
}

#[cfg(target_os = "linux")]
struct DesktopClientSupervisor {
    stop: DesktopClientStopHandle,
    worker: std::thread::JoinHandle<()>,
}

#[cfg(target_os = "linux")]
struct ChatEventSupervisor {
    stop: ChatEventStopHandle,
    worker: std::thread::JoinHandle<()>,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Default)]
struct ChatEventStopHandle {
    inner: Arc<(Mutex<ChatEventStopState>, Condvar)>,
}

#[cfg(target_os = "linux")]
#[derive(Default)]
struct ChatEventStopState {
    stopped: bool,
    stream: Option<UnixStream>,
}

#[cfg(target_os = "linux")]
impl InterruptibleConnectState for ChatEventStopState {
    fn stopped(&self) -> bool {
        self.stopped
    }

    fn set_stream(&mut self, stream: Option<UnixStream>) {
        self.stream = stream;
    }
}

#[cfg(target_os = "linux")]
impl ChatEventStopHandle {
    fn stop(&self) {
        let (state, wake) = &*self.inner;
        let mut state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.stopped = true;
        if let Some(stream) = state.stream.take() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        wake.notify_all();
    }
}

#[cfg(target_os = "linux")]
pub(crate) enum DesktopClientSession {
    NoSupervisor,
    Connected(DesktopClientHolder),
    Disconnected,
}

#[cfg(target_os = "linux")]
impl AttachCompanionState {
    fn new(listener: Arc<AttachListenerState>) -> Self {
        Self {
            approval: listener.approval.clone(),
            listener: Mutex::new(Some(listener)),
            listener_lifecycle: Mutex::new(AttachListenerLifecycle::Listening),
            listener_stop: Mutex::new(AttachListenerStopState::Pending {
                stop_requested: false,
            }),
            listener_stopped: Condvar::new(),
            approval_presenter: Mutex::new(None),
            presenting: Mutex::new(false),
            desktop_supervisor_lifecycle: Mutex::new(()),
            desktop_client: Mutex::new(None),
            desktop_client_holder: DesktopClientHolder::new(),
            chat_events: Mutex::new(None),
            connected: Mutex::new(false),
            chat_events_connected: Mutex::new(false),
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
        self.stop_approval_presenter();
        self.stop_desktop_client();
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
            presenting: *self
                .presenting
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            supervisor_running: self
                .desktop_client
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_some(),
            connected: *self
                .connected
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            chat_events_connected: *self
                .chat_events_connected
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            runtime_upgrade_pending: *self
                .connected
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                && runtime_upgrade_pending(&self.desktop_client_holder),
        }
    }

    fn start_approval_presenter(&self, start: impl FnOnce(ApprovalPresenterStopHandle)) {
        let mut presenter = self
            .approval_presenter
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if presenter.is_some() {
            return;
        }
        let stop = ApprovalPresenterStopHandle::new();
        *presenter = Some(stop.clone());
        drop(presenter);
        start(stop);
    }

    fn stop_approval_presenter(&self) {
        if let Some(stop) = self
            .approval_presenter
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            stop.stop();
        }
    }

    fn start_desktop_client(
        &self,
        start: impl FnOnce(DesktopClientStopHandle, DesktopClientHolder) -> std::thread::JoinHandle<()>,
    ) {
        let _lifecycle = self
            .desktop_supervisor_lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.start_desktop_client_locked(start);
    }

    fn start_desktop_client_locked(
        &self,
        start: impl FnOnce(DesktopClientStopHandle, DesktopClientHolder) -> std::thread::JoinHandle<()>,
    ) {
        let mut client = self
            .desktop_client
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if client.is_some() {
            return;
        }
        let stop = DesktopClientStopHandle::new();
        let holder = self.desktop_client_holder.clone();
        let worker = start(stop.clone(), holder);
        *client = Some(DesktopClientSupervisor { stop, worker });
    }

    fn start_chat_events(
        &self,
        start: impl FnOnce(ChatEventStopHandle) -> std::thread::JoinHandle<()>,
    ) {
        let _lifecycle = self
            .desktop_supervisor_lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.start_chat_events_locked(start);
    }

    fn start_chat_events_locked(
        &self,
        start: impl FnOnce(ChatEventStopHandle) -> std::thread::JoinHandle<()>,
    ) {
        let mut supervisor = self
            .chat_events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if supervisor.is_some() {
            return;
        }
        let stop = ChatEventStopHandle::default();
        let worker = start(stop.clone());
        *supervisor = Some(ChatEventSupervisor { stop, worker });
    }

    fn start_desktop_supervisors(
        &self,
        start_client: impl FnOnce(
            DesktopClientStopHandle,
            DesktopClientHolder,
        ) -> std::thread::JoinHandle<()>,
        start_chat_events: impl FnOnce(ChatEventStopHandle) -> std::thread::JoinHandle<()>,
    ) {
        let _lifecycle = self
            .desktop_supervisor_lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.start_desktop_client_locked(start_client);
        self.start_chat_events_locked(start_chat_events);
    }

    fn stop_desktop_client(&self) {
        let _lifecycle = self
            .desktop_supervisor_lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.stop_chat_events_locked();
        let supervisor = self
            .desktop_client
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(supervisor) = supervisor {
            supervisor.stop.stop();
            let _ = supervisor.worker.join();
        }
        self.record_connected(false);
    }

    fn stop_chat_events(&self) {
        let _lifecycle = self
            .desktop_supervisor_lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.stop_chat_events_locked();
    }

    fn stop_chat_events_locked(&self) {
        let supervisor = self
            .chat_events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(supervisor) = supervisor {
            supervisor.stop.stop();
            let _ = supervisor.worker.join();
        }
    }

    pub(crate) fn desktop_client_session(&self) -> DesktopClientSession {
        if self
            .desktop_client
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_none()
        {
            return DesktopClientSession::NoSupervisor;
        }
        if *self
            .connected
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            DesktopClientSession::Connected(self.desktop_client_holder.clone())
        } else {
            DesktopClientSession::Disconnected
        }
    }

    fn record_connected(&self, connected: bool) {
        *self
            .connected
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = connected;
    }

    fn record_chat_events_connected(&self, connected: bool) {
        *self
            .chat_events_connected
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = connected;
    }

    #[cfg(test)]
    pub(crate) fn set_desktop_client_for_test(
        &self,
        connected: bool,
        start: impl FnOnce(DesktopClientStopHandle, DesktopClientHolder) -> std::thread::JoinHandle<()>,
    ) {
        self.start_desktop_client(start);
        self.record_connected(connected);
    }

    #[cfg(test)]
    pub(crate) fn stop_desktop_client_for_test(&self) {
        self.stop_desktop_client();
    }

    fn record_presenting(&self, presenting: bool) {
        *self
            .presenting
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = presenting;
    }

    pub(crate) fn record_workspace(&self, workspace: String) {
        self.approval.record(workspace);
    }

    pub(crate) fn clear_workspace(&self) {
        self.approval.clear();
    }

    pub(crate) fn approval(&self) -> Option<Approval> {
        self.approval.approval()
    }
}

#[cfg(target_os = "linux")]
impl Default for AttachCompanionState {
    fn default() -> Self {
        Self {
            listener: Mutex::new(None),
            approval: SignedWorkspaceApproval::default(),
            listener_lifecycle: Mutex::new(AttachListenerLifecycle::Pending),
            listener_stop: Mutex::new(AttachListenerStopState::Pending {
                stop_requested: false,
            }),
            listener_stopped: Condvar::new(),
            approval_presenter: Mutex::new(None),
            presenting: Mutex::new(false),
            desktop_supervisor_lifecycle: Mutex::new(()),
            desktop_client: Mutex::new(None),
            desktop_client_holder: DesktopClientHolder::new(),
            chat_events: Mutex::new(None),
            connected: Mutex::new(false),
            chat_events_connected: Mutex::new(false),
        }
    }
}

#[cfg(target_os = "linux")]
impl Drop for AttachCompanionState {
    fn drop(&mut self) {
        if let Some(stop) = self
            .approval_presenter
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            stop.stop();
        }
        if let Some(supervisor) = self
            .desktop_client
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            supervisor.stop.stop();
            let _ = supervisor.worker.join();
        }
        if let Some(supervisor) = self
            .chat_events
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            supervisor.stop.stop();
            let _ = supervisor.worker.join();
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
    return match state.desktop_client_session() {
        DesktopClientSession::NoSupervisor => state.listener()?.list_companions(),
        DesktopClientSession::Connected(client) => {
            #[derive(serde::Deserialize)]
            struct CompanionList {
                companions: Vec<AuthorizedCompanion>,
            }

            let response = client.list_companions().map_err(companion_client_error)?;
            serde_json::from_value::<CompanionList>(response)
                .map(|response| response.companions)
                .map_err(|_| ProtocolError::persistence_failed())
        }
        DesktopClientSession::Disconnected => Err(ProtocolError::persistence_failed()),
    };

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
            presenting: false,
            connected: false,
            chat_events_connected: false,
            supervisor_running: false,
            runtime_upgrade_pending: false,
        }
    }
}

#[tauri::command]
pub fn attach_revoke_companion(
    state: tauri::State<'_, AttachCompanionState>,
    client_identity: String,
) -> Result<(), ProtocolError> {
    #[cfg(target_os = "linux")]
    return match state.desktop_client_session() {
        DesktopClientSession::NoSupervisor => state.listener()?.revoke_companion(&client_identity),
        DesktopClientSession::Connected(client) => client
            .revoke_companion(&client_identity)
            .map(|_| ())
            .map_err(companion_client_error),
        DesktopClientSession::Disconnected => Err(ProtocolError::persistence_failed()),
    };

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (state, client_identity);
        Err(ProtocolError::unsupported_operation())
    }
}

#[cfg(target_os = "linux")]
fn companion_client_error(error: ClientError) -> ProtocolError {
    match error {
        ClientError::DesktopBusy => ProtocolError::desktop_busy(),
        _ => ProtocolError::persistence_failed(),
    }
}

#[cfg(target_os = "linux")]
impl AttachListenerState {
    fn load(credential_path: &std::path::Path) -> Result<Self, ProtocolError> {
        Self::load_with_approval(credential_path, SignedWorkspaceApproval::default())
    }

    fn load_with_approval(
        credential_path: &std::path::Path,
        approval: SignedWorkspaceApproval,
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
            approval,
        })
    }

    fn approval(&self) -> Option<Approval> {
        self.approval.approval()
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
    config: &Path,
    documents: Option<PathBuf>,
    home: Option<PathBuf>,
) -> muniment_core::attach::AttachHome {
    muniment_core::attach::AttachHome::configured(config.to_path_buf(), documents, home)
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
        let drain_state = app
            .state::<muniment_core::attach::DrainState>()
            .inner()
            .clone();
        let config = app
            .path()
            .app_config_dir()
            .map_err(|_| ProtocolError::persistence_failed())?;
        let home = resolve_attach_home(
            &config,
            app.path().document_dir().ok(),
            app.path().home_dir().ok(),
        );
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
            .join(COMPANION_CREDENTIAL_FILE_NAME);
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
            drain_state,
        })
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
    let approval = app.state::<AttachCompanionState>().approval.clone();
    let Ok(state) = AttachListenerState::load_with_approval(&credential_path, approval) else {
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
fn start_approval_presenter<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let Ok(filesystem) = AttachFilesystem::from_environment() else {
        eprintln!("approval presenter filesystem lookup failed");
        return;
    };
    let endpoint = filesystem.endpoint_path().to_owned();
    let presenter_app = app.clone();
    app.state::<AttachCompanionState>()
        .start_approval_presenter(move |stop| {
            std::thread::spawn(move || {
                let approvals = presenter_app.state::<AttachApprovalState>().inner().clone();
                let observer_app = presenter_app.clone();
                serve_approval_presenter_at(
                    &endpoint,
                    env!("CARGO_PKG_VERSION"),
                    Duration::from_secs(5),
                    Duration::from_millis(250),
                    stop,
                    move |presenting| {
                        observer_app
                            .state::<AttachCompanionState>()
                            .record_presenting(presenting);
                    },
                    move |request| answer_presented_approval(&approvals, request),
                );
            });
        });
}

#[cfg(target_os = "linux")]
fn start_desktop_client<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let Ok(filesystem) = AttachFilesystem::from_environment() else {
        eprintln!("desktop client filesystem lookup failed");
        return;
    };
    let endpoint = filesystem.endpoint_path().to_owned();
    let client_endpoint = endpoint.clone();
    let client_app = app.clone();
    let event_app = app.clone();
    let event_status_app = app.clone();
    app.state::<AttachCompanionState>()
        .start_desktop_supervisors(
            move |stop, holder| {
                std::thread::spawn(move || {
                    let observer_app = client_app.clone();
                    serve_desktop_client_at(
                        &client_endpoint,
                        env!("CARGO_PKG_VERSION"),
                        Duration::from_secs(5),
                        Duration::from_millis(250),
                        stop,
                        holder,
                        move |connected| {
                            observer_app
                                .state::<AttachCompanionState>()
                                .record_connected(connected);
                            let status = observer_app
                                .state::<AttachCompanionState>()
                                .listener_status();
                            let _ = observer_app.emit("desktop-client-status-changed", status);
                        },
                    );
                })
            },
            move |stop| {
                std::thread::spawn(move || {
                    serve_chat_events_at(
                        &endpoint,
                        env!("CARGO_PKG_VERSION"),
                        Duration::from_secs(5),
                        Duration::from_millis(250),
                        stop,
                        move |connected| {
                            event_status_app
                                .state::<AttachCompanionState>()
                                .record_chat_events_connected(connected);
                            let status = event_status_app
                                .state::<AttachCompanionState>()
                                .listener_status();
                            let _ = event_status_app.emit("desktop-client-status-changed", status);
                        },
                        move |event| {
                            let _ = event_app.emit("chat-event", event);
                        },
                    );
                })
            },
        );
    let status = app.state::<AttachCompanionState>().listener_status();
    let _ = app.emit("desktop-client-status-changed", status);
}

#[cfg(target_os = "linux")]
fn serve_chat_events_at(
    endpoint: &Path,
    client_version: &str,
    io_timeout: Duration,
    retry_interval: Duration,
    stop: ChatEventStopHandle,
    mut observe: impl FnMut(bool),
    mut deliver: impl FnMut(Value),
) {
    loop {
        let stream = interruptible_connect_with_state(endpoint, &stop.inner);
        if let Some(stream) = stream {
            if let Ok(mut client) =
                handshake_desktop_client_stream(stream, client_version, io_timeout)
            {
                if client.subscribe_chat_events().is_ok() {
                    observe(true);
                    while let Ok(event) = client.read_chat_event() {
                        deliver(event);
                    }
                    observe(false);
                }
            }
            stop.inner
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .stream = None;
        }

        let (state, wake) = &*stop.inner;
        let state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.stopped {
            return;
        }
        let (state, _) = wake
            .wait_timeout_while(state, retry_interval, |state| !state.stopped)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.stopped {
            return;
        }
    }
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
            .map(|path| path.join(COMPANION_CREDENTIAL_FILE_NAME))
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
        start_approval_presenter(&app);
        start_desktop_client(&app);
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
    if let Some(chat_state) = app.try_state::<crate::chat::ChatState>() {
        if chat_state.open_storage(&app).is_err() {
            drop(listener);
            drop(instance_lock);
            app.state::<AttachCompanionState>()
                .record_listener_start_failure(AttachListenerStartFailure::Filesystem);
            eprintln!("Muniment could not open chat storage.");
            return;
        }
    }
    let companion_state = app.state::<AttachCompanionState>();
    companion_state.publish_listener_stop(listener.stop_handle());
    companion_state.record_listener_started();
    let status = companion_state.listener_status();
    let _ = app.emit("desktop-client-status-changed", status);
    let expected_desktop_executable = std::env::current_exe().ok();
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
        let expected_desktop_executable = expected_desktop_executable.clone();
        std::thread::spawn(move || {
            let route = expected_desktop_executable.as_ref().map_or(
                AttachConnectionRoute::Companion,
                |expected_desktop_executable| {
                    name_attach_connection_route(
                        &stream,
                        credentials,
                        expected_desktop_executable,
                        &ProcReader,
                        Duration::from_secs(5),
                    )
                },
            );
            if matches!(
                route,
                AttachConnectionRoute::ApprovalPresenter | AttachConnectionRoute::DesktopClient
            ) {
                return;
            }
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
    use muniment_core::attach::{
        decode_frame, encode_frame, Authorization, ErrorCode, ErrorMessage, Id, Protocol, Response,
        Success, Welcome,
    };
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
    #[test]
    fn approval_presenter_starts_once_and_stops_when_listener_starts() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let state = AttachCompanionState::default();
        let starts = Arc::new(AtomicUsize::new(0));
        let worker = Arc::new(Mutex::new(None));
        let endpoint =
            std::env::temp_dir().join(format!("mt-presenter-{}", Uuid::now_v7().simple()));

        for _ in 0..2 {
            let starts = starts.clone();
            let worker = worker.clone();
            let endpoint = endpoint.clone();
            state.start_approval_presenter(move |stop| {
                starts.fetch_add(1, Ordering::SeqCst);
                *worker.lock().unwrap() = Some(std::thread::spawn(move || {
                    serve_approval_presenter_at(
                        &endpoint,
                        "0.0.1",
                        Duration::from_millis(10),
                        Duration::from_secs(30),
                        stop,
                        |_| {},
                        |_| unreachable!("the test endpoint has no listener"),
                    );
                }));
            });
        }

        assert_eq!(starts.load(Ordering::SeqCst), 1);
        state.record_listener_started();
        worker.lock().unwrap().take().unwrap().join().unwrap();
        assert!(state.approval_presenter.lock().unwrap().is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn desktop_client_starts_once_and_stops_when_listener_starts() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let state = AttachCompanionState::default();
        let starts = Arc::new(AtomicUsize::new(0));
        let endpoint = std::env::temp_dir().join(format!("mt-client-{}", Uuid::now_v7().simple()));

        for _ in 0..2 {
            let starts = starts.clone();
            let endpoint = endpoint.clone();
            state.start_desktop_client(move |stop, holder| {
                starts.fetch_add(1, Ordering::SeqCst);
                std::thread::spawn(move || {
                    serve_desktop_client_at(
                        &endpoint,
                        "0.0.1",
                        Duration::from_millis(10),
                        Duration::from_secs(30),
                        stop,
                        holder,
                        |_| {},
                    );
                })
            });
        }

        assert_eq!(starts.load(Ordering::SeqCst), 1);
        assert!(state.listener_status().supervisor_running);
        state.record_connected(true);
        assert!(state.listener_status().connected);
        state.record_listener_started();
        assert!(state.desktop_client.lock().unwrap().is_none());
        assert!(!state.listener_status().supervisor_running);
        assert!(!state.listener_status().connected);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn desktop_client_stop_finishes_before_restart() {
        use std::sync::mpsc;

        let state = Arc::new(AttachCompanionState::default());
        let (old_started, old_started_rx) = mpsc::channel();
        let (release_old, release_old_rx) = mpsc::channel();
        let old_state = state.clone();
        state.start_desktop_client(move |_, _| {
            std::thread::spawn(move || {
                old_started.send(()).unwrap();
                release_old_rx.recv().unwrap();
                old_state.record_connected(false);
            })
        });
        old_started_rx.recv().unwrap();
        state.record_connected(true);

        let stop_state = state.clone();
        let stopper = std::thread::spawn(move || stop_state.record_listener_started());
        let deadline = Instant::now() + Duration::from_secs(2);
        while state.desktop_client.try_lock().is_ok() {
            assert!(
                Instant::now() < deadline,
                "desktop client stop did not start"
            );
            std::thread::yield_now();
        }
        let (new_started, new_started_rx) = mpsc::channel();
        let restart_state = state.clone();
        let restarter = std::thread::spawn(move || {
            let new_state = restart_state.clone();
            restart_state.start_desktop_client(move |_, _| {
                std::thread::spawn(move || {
                    new_state.record_connected(true);
                    new_started.send(()).unwrap();
                })
            });
        });

        assert!(new_started_rx.try_recv().is_err());
        release_old.send(()).unwrap();
        stopper.join().unwrap();
        restarter.join().unwrap();
        new_started_rx.recv().unwrap();
        assert!(state.listener_status().connected);
        state.stop_desktop_client();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn listener_start_stops_both_supervisors_during_startup() {
        use std::sync::mpsc;

        let state = Arc::new(AttachCompanionState::default());
        let (between_starts, between_starts_rx) = mpsc::channel();
        let (continue_start, continue_start_rx) = mpsc::channel();
        let start_state = state.clone();
        let starter = std::thread::spawn(move || {
            start_state.start_desktop_supervisors(
                |_, _| std::thread::spawn(|| {}),
                move |_| {
                    between_starts.send(()).unwrap();
                    continue_start_rx.recv().unwrap();
                    std::thread::spawn(|| {})
                },
            );
        });
        between_starts_rx.recv().unwrap();

        let listener_state = state.clone();
        let (listener_attempted, listener_attempted_rx) = mpsc::channel();
        let (listener_started, listener_started_rx) = mpsc::channel();
        let listener = std::thread::spawn(move || {
            listener_attempted.send(()).unwrap();
            listener_state.record_listener_started();
            listener_started.send(()).unwrap();
        });
        listener_attempted_rx.recv().unwrap();
        assert!(
            listener_started_rx
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "listener startup passed the supervisor lifecycle lock"
        );

        continue_start.send(()).unwrap();
        starter.join().unwrap();
        listener.join().unwrap();
        listener_started_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        assert!(state.desktop_client.lock().unwrap().is_none());
        assert!(state.chat_events.lock().unwrap().is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn chat_event_supervisor_reports_disconnect_after_stream_ends() {
        use muniment_core::attach::reconnect_welcome;
        use std::io::{Read, Write};
        use std::os::unix::net::UnixListener;
        use std::sync::mpsc;

        fn read_value(stream: &mut impl Read) -> Value {
            let mut prefix = [0; 4];
            stream.read_exact(&mut prefix).unwrap();
            let mut frame = vec![0; 4 + u32::from_be_bytes(prefix) as usize];
            frame[..4].copy_from_slice(&prefix);
            stream.read_exact(&mut frame[4..]).unwrap();
            decode_frame(&frame).unwrap().unwrap().0
        }

        let endpoint =
            std::env::temp_dir().join(format!("muniment-chat-events-{}.sock", Uuid::now_v7()));
        let listener = UnixListener::bind(&endpoint).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            assert_eq!(read_value(&mut stream)["client"]["kind"], "desktop-client");
            stream
                .write_all(
                    &encode_frame(&reconnect_welcome(1, "0.0.1", "11".repeat(16), "")).unwrap(),
                )
                .unwrap();
            stream
                .write_all(
                    &encode_frame(&json!({
                        "profile_id": "profile-1",
                        "capability": "33".repeat(32),
                        "expires_at": 60,
                        "idle_timeout_seconds": 30,
                        "workspace_scopes": {"/work/signed": []}
                    }))
                    .unwrap(),
                )
                .unwrap();
            let request = read_value(&mut stream);
            let request_id = Id::new(request["request_id"].as_str().unwrap()).unwrap();
            stream
                .write_all(
                    &encode_frame(&Response {
                        protocol: Protocol,
                        request_id,
                        ok: Success,
                        body: json!({"subscription_id": "44".repeat(16)}),
                    })
                    .unwrap(),
                )
                .unwrap();
            stream
                .write_all(
                    &encode_frame(&json!({
                        "protocol": "muniment.attach/1",
                        "subscription_id": "44".repeat(16),
                        "event": "chat.event",
                        "body": {"phase": "running", "text": "forwarded"}
                    }))
                    .unwrap(),
                )
                .unwrap();
        });

        let state = AttachCompanionState::default();
        let worker_endpoint = endpoint.clone();
        let (delivered, received) = mpsc::channel();
        let (observed, observations) = mpsc::channel();
        state.start_chat_events(move |stop| {
            std::thread::spawn(move || {
                serve_chat_events_at(
                    &worker_endpoint,
                    "0.0.1",
                    Duration::from_secs(1),
                    Duration::from_millis(10),
                    stop,
                    move |connected| observed.send(connected).unwrap(),
                    move |event| delivered.send(event).unwrap(),
                )
            })
        });
        assert_eq!(
            received.recv_timeout(Duration::from_secs(1)).unwrap(),
            json!({"phase": "running", "text": "forwarded"})
        );
        assert!(observations.recv_timeout(Duration::from_secs(1)).unwrap());
        let disconnected = observations.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(!disconnected);
        state.stop_chat_events();
        assert!(state.chat_events.lock().unwrap().is_none());
        server.join().unwrap();
        std::fs::remove_file(endpoint).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn chat_event_supervisor_stops_while_connect_is_pending() {
        use std::os::fd::AsRawFd;
        use std::os::unix::net::{UnixListener, UnixStream};
        use std::sync::mpsc;

        unsafe extern "C" {
            fn listen(socket: i32, backlog: i32) -> i32;
        }

        let endpoint =
            std::env::temp_dir().join(format!("muniment-chat-events-{}.sock", Uuid::now_v7()));
        let listener = UnixListener::bind(&endpoint).unwrap();
        // SAFETY: `listener` owns a valid Unix socket descriptor.
        assert_eq!(unsafe { listen(listener.as_raw_fd(), 0) }, 0);
        let queued_stream = UnixStream::connect(&endpoint).unwrap();
        let stop = ChatEventStopHandle::default();
        let worker_stop = stop.clone();
        let worker_endpoint = endpoint.clone();
        let (finished, finished_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            serve_chat_events_at(
                &worker_endpoint,
                "0.0.1",
                Duration::from_secs(1),
                Duration::from_millis(10),
                worker_stop,
                |_| {},
                |_| {},
            );
            finished.send(()).unwrap();
        });

        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let (state, _) = &*stop.inner;
            if state.lock().unwrap().stream.is_some() {
                break;
            }
            assert!(Instant::now() < deadline, "connect did not remain pending");
            std::thread::yield_now();
        }
        stop.stop();
        finished_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        worker.join().unwrap();
        drop(queued_stream);
        drop(listener);
        std::fs::remove_file(endpoint).unwrap();
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
            Err(PeerAuthorityError::ExecutableMismatch),
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
            AttachListenerState::load(&runtime.join(COMPANION_CREDENTIAL_FILE_NAME)).unwrap(),
        );
        let app = tauri::test::mock_app();
        app.manage(AttachCompanionState::default());
        app.manage(AttachApprovalState::default());
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
                presenting: false,
                supervisor_running: false,
                connected: false,
                chat_events_connected: false,
                runtime_upgrade_pending: false,
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
            AttachListenerState::load(&runtime.join(COMPANION_CREDENTIAL_FILE_NAME)).unwrap(),
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
            AttachListenerState::load(&runtime.join(COMPANION_CREDENTIAL_FILE_NAME)).unwrap(),
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
    fn held_instance_lock_leaves_chat_storage_deferred_without_reconciliation() {
        let runtime = handoff_test_runtime("storage-deferred");
        let filesystem = AttachFilesystem::from_runtime_directory(&runtime).unwrap();
        let instance_lock = filesystem.acquire_instance_lock().unwrap();
        let listener = Arc::new(
            AttachListenerState::load(&runtime.join(COMPANION_CREDENTIAL_FILE_NAME)).unwrap(),
        );
        let app = tauri::test::mock_app();
        app.manage(AttachCompanionState::default());
        app.manage(AttachApprovalState::default());
        app.manage(crate::chat::ChatState::new(RuntimeActivityRegistry::new()));
        let journal_path = app.path().app_data_dir().unwrap().join("runs.sqlite3");
        let run_id = Uuid::now_v7().to_string();
        let mut journal = RunJournal::open(&journal_path).unwrap();
        append_test_event(&mut journal, &run_id, 1, "run.started", json!({}), None);
        drop(journal);

        run_attach_listener(app.handle().clone(), listener, filesystem);

        assert!(app.state::<crate::chat::ChatState>().storage().is_err());
        let mut journal = RunJournal::open(journal_path).unwrap();
        assert_eq!(journal.events(&run_id).unwrap().len(), 1);
        app.state::<AttachCompanionState>()
            .record_listener_started();
        drop(instance_lock);
        std::fs::remove_dir_all(runtime).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn listener_ownership_opens_chat_storage_and_reconciles_once() {
        let runtime = handoff_test_runtime("storage-owned");
        let filesystem = AttachFilesystem::from_runtime_directory(&runtime).unwrap();
        let listener = Arc::new(
            AttachListenerState::load(&runtime.join(COMPANION_CREDENTIAL_FILE_NAME)).unwrap(),
        );
        let app = tauri::test::mock_app();
        app.manage(AttachCompanionState::default());
        app.manage(AttachApprovalState::default());
        app.manage(crate::chat::ChatState::new(RuntimeActivityRegistry::new()));
        let journal_path = app.path().app_data_dir().unwrap().join("runs.sqlite3");
        let run_id = Uuid::now_v7().to_string();
        let mut journal = RunJournal::open(&journal_path).unwrap();
        append_test_event(&mut journal, &run_id, 1, "run.started", json!({}), None);
        drop(journal);

        let worker_app = app.handle().clone();
        let worker = std::thread::spawn(move || {
            run_attach_listener(worker_app, listener, filesystem);
        });
        wait_for_listener(&app.state::<AttachCompanionState>());

        let storage = app.state::<crate::chat::ChatState>();
        assert!(storage.storage().is_ok());
        assert_eq!(
            storage
                .storage()
                .unwrap()
                .lock()
                .unwrap()
                .journal
                .events(&run_id)
                .unwrap()
                .len(),
            2
        );
        stop_attach_listener(app.handle());
        worker.join().unwrap();
        std::fs::remove_dir_all(runtime).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn listener_refuses_an_approval_presenter_without_pairing() {
        use muniment_attach::connect_approval_presenter_at;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let runtime = handoff_test_runtime("presenter-refused");
        let filesystem = AttachFilesystem::from_runtime_directory(&runtime).unwrap();
        let endpoint = filesystem.endpoint_path().to_owned();
        let listener = Arc::new(
            AttachListenerState::load(&runtime.join(COMPANION_CREDENTIAL_FILE_NAME)).unwrap(),
        );
        listener.approval.record("signed-workspace".into());
        let app = tauri::test::mock_app();
        app.manage(AttachCompanionState::default());
        let approvals = AttachApprovalState::default();
        let presentations = Arc::new(AtomicUsize::new(0));
        let presenter_count = presentations.clone();
        approvals.register_presenter(move |_| {
            presenter_count.fetch_add(1, Ordering::SeqCst);
            true
        });
        app.manage(approvals);
        app.state::<AttachCompanionState>()
            .set_listener(listener.clone());

        let listener_app = app.handle().clone();
        let worker = std::thread::spawn(move || {
            run_attach_listener(listener_app, listener, filesystem);
        });
        wait_for_listener(&app.state::<AttachCompanionState>());

        assert!(connect_approval_presenter_at(
            &endpoint,
            env!("CARGO_PKG_VERSION"),
            Duration::from_secs(1),
        )
        .is_err());
        assert_eq!(presentations.load(Ordering::SeqCst), 0);

        stop_attach_listener(app.handle());
        worker.join().unwrap();
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
            AttachListenerState::load(&runtime.join(COMPANION_CREDENTIAL_FILE_NAME)).unwrap(),
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
    fn companion_commands_use_each_desktop_client_session_state() {
        use muniment_core::attach::{reconnect_welcome, serve_desktop_client_at};
        use std::io::{Read, Write};
        use std::os::unix::net::UnixListener;
        use std::sync::mpsc;

        fn read_value(stream: &mut impl Read) -> Value {
            let mut prefix = [0; 4];
            stream.read_exact(&mut prefix).unwrap();
            let mut frame = vec![0; 4 + u32::from_be_bytes(prefix) as usize];
            frame[..4].copy_from_slice(&prefix);
            stream.read_exact(&mut frame[4..]).unwrap();
            decode_frame(&frame).unwrap().unwrap().0
        }

        let endpoint =
            std::env::temp_dir().join(format!("muniment-companion-client-{}.sock", Uuid::now_v7()));
        let listener = UnixListener::bind(&endpoint).unwrap();
        let (request_tx, request_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            assert_eq!(read_value(&mut stream)["client"]["kind"], "desktop-client");
            stream
                .write_all(
                    &encode_frame(&reconnect_welcome(1, "0.0.1", "11".repeat(16), "")).unwrap(),
                )
                .unwrap();
            stream
                .write_all(
                    &encode_frame(&json!({
                        "profile_id": "profile-1",
                        "capability": "33".repeat(32),
                        "expires_at": 60,
                        "idle_timeout_seconds": 30,
                        "workspace_scopes": {"/work/signed": ["threads:read"]}
                    }))
                    .unwrap(),
                )
                .unwrap();

            for (index, body) in [
                json!({"companions": [{
                    "identity": "companion-1",
                    "claimed_kind": "cli",
                    "claimed_version": "1.2.3",
                    "approved_at": "2026-08-04T12:00:00Z"
                }]}),
                json!({}),
                json!({"companions": [{"identity": "partial"}]}),
            ]
            .into_iter()
            .enumerate()
            {
                let request = read_value(&mut stream);
                if index == 0 {
                    request_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                }
                let request_id = Id::new(request["request_id"].as_str().unwrap()).unwrap();
                stream
                    .write_all(
                        &encode_frame(&Response {
                            protocol: Protocol,
                            request_id,
                            ok: Success,
                            body,
                        })
                        .unwrap(),
                    )
                    .unwrap();
            }
        });

        let state = AttachCompanionState::default();
        let client_endpoint = endpoint.clone();
        let (connected_tx, connected_rx) = mpsc::channel();
        state.set_desktop_client_for_test(false, move |stop, holder| {
            std::thread::spawn(move || {
                let mut connected_tx = Some(connected_tx);
                serve_desktop_client_at(
                    &client_endpoint,
                    "0.0.1",
                    Duration::from_secs(1),
                    Duration::from_millis(10),
                    stop,
                    holder,
                    move |connected| {
                        if connected {
                            connected_tx.take().unwrap().send(()).unwrap();
                        }
                    },
                )
            })
        });
        connected_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        state.record_connected(true);
        let app = tauri::test::mock_app();
        app.manage(state);

        let DesktopClientSession::Connected(client) =
            app.state::<AttachCompanionState>().desktop_client_session()
        else {
            panic!("desktop client must be connected");
        };
        let slow_call = std::thread::spawn(move || client.list_companions());
        request_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let list_busy = attach_companions(app.state()).unwrap_err();
        assert_eq!(list_busy.code(), ErrorCode::DesktopBusy);
        assert_eq!(list_busy.message(), ErrorMessage::DesktopBusy);
        let revoke_busy = attach_revoke_companion(app.state(), "companion-1".into()).unwrap_err();
        assert_eq!(revoke_busy.code(), ErrorCode::DesktopBusy);
        assert_eq!(revoke_busy.message(), ErrorMessage::DesktopBusy);
        release_tx.send(()).unwrap();
        assert!(slow_call.join().unwrap().is_ok());

        attach_revoke_companion(app.state(), "companion-1".into()).unwrap();
        assert_eq!(
            attach_companions(app.state()).unwrap_err().code(),
            ErrorCode::PersistenceFailed
        );
        server.join().unwrap();
        app.state::<AttachCompanionState>()
            .stop_desktop_client_for_test();
        let _ = std::fs::remove_file(endpoint);

        let disconnected = AttachCompanionState::default();
        disconnected.set_desktop_client_for_test(false, |_, _| std::thread::spawn(|| {}));
        let app = tauri::test::mock_app();
        app.manage(disconnected);
        let list_unreachable = attach_companions(app.state()).unwrap_err();
        assert_eq!(list_unreachable.code(), ErrorCode::PersistenceFailed);
        assert_eq!(list_unreachable.message(), ErrorMessage::PersistenceFailed);
        let revoke_unreachable =
            attach_revoke_companion(app.state(), "companion-1".into()).unwrap_err();
        assert_eq!(revoke_unreachable.code(), ErrorCode::PersistenceFailed);
        assert_eq!(
            revoke_unreachable.message(),
            ErrorMessage::PersistenceFailed
        );
        app.state::<AttachCompanionState>()
            .stop_desktop_client_for_test();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn runtime_version_hold_covers_older_and_invalid_versions() {
        assert!(runtime_version_upgrade_pending(Some("0.0.0")));
        assert!(runtime_version_upgrade_pending(Some("invalid")));
        assert!(runtime_version_upgrade_pending(None));
        assert!(!runtime_version_upgrade_pending(Some(
            MINIMUM_COMPATIBLE_RUNTIME_VERSION
        )));
        assert!(!runtime_version_upgrade_pending(Some("0.0.2")));
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
                    presenting: false,
                    supervisor_running: false,
                    connected: false,
                    chat_events_connected: false,
                    runtime_upgrade_pending: false,
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
                presenting: false,
                supervisor_running: false,
                connected: false,
                chat_events_connected: false,
                runtime_upgrade_pending: false,
            }
        );

        state.record_presenting(true);
        assert!(state.listener_status().presenting);
        state.record_presenting(false);

        let pending = AttachCompanionState::default().listener_status();
        assert_eq!(
            pending,
            AttachListenerStatus {
                started: false,
                failure: None,
                pending: true,
                stopped: false,
                presenting: false,
                supervisor_running: false,
                connected: false,
                chat_events_connected: false,
                runtime_upgrade_pending: false,
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
                presenting: false,
                supervisor_running: false,
                connected: false,
                chat_events_connected: false,
                runtime_upgrade_pending: false,
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
        state.approval.record("workspace-a".into());
        let approval = state.approval.clone();
        let approvals = AttachApprovalState::default();
        approvals.register_presenter(move |_| {
            approval.clear();
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
        state.approval.record("workspace-a".into());
        let approval = state.approval.clone();
        let approvals = AttachApprovalState::default();
        approvals.register_presenter(move |_| {
            approval.record("workspace-b".into());
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
                presenting: false,
                supervisor_running: false,
                connected: false,
                chat_events_connected: false,
                runtime_upgrade_pending: false,
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
                presenting: false,
                supervisor_running: false,
                connected: false,
                chat_events_connected: false,
                runtime_upgrade_pending: false,
            }
        );
        assert_state_works(&app);
        std::fs::remove_file(credential_path).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_home_uses_recorded_home() {
        let root = std::env::temp_dir().join(format!("muniment-attach-home-{}", Uuid::now_v7()));
        let config = root.join("config");
        let recorded_home = root.join("recorded-home");
        muniment_core::home::confirm_home(&config, &recorded_home).unwrap();

        assert_eq!(
            resolve_attach_home(
                &config,
                Some(root.join("documents")),
                Some(root.join("user-home")),
            )
            .resolve()
            .unwrap(),
            recorded_home
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_home_uses_default_when_home_is_not_recorded() {
        let root = std::env::temp_dir().join(format!("muniment-attach-home-{}", Uuid::now_v7()));
        let config = root.join("config");
        std::fs::create_dir_all(&config).unwrap();

        assert_eq!(
            resolve_attach_home(&config, None, Some(root.clone()))
                .resolve()
                .unwrap(),
            root.join("Muniment")
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_home_rejects_unreadable_recorded_home() {
        let root = std::env::temp_dir().join(format!("muniment-attach-home-{}", Uuid::now_v7()));
        let config = root.join("config");
        std::fs::create_dir_all(config.join("home.json")).unwrap();

        assert_eq!(
            resolve_attach_home(&config, Some(root.join("documents")), Some(root.clone()))
                .resolve(),
            Err(ProtocolError::persistence_failed())
        );

        std::fs::remove_dir_all(root).unwrap();
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
                    home: std::env::temp_dir().into(),
                    workspace_contexts: Arc::new(Mutex::new(WorkspaceContextMap::default())),
                    client_credentials: credentials,
                    credential_path: None,
                    client_identity: None,
                    drain_state: muniment_core::attach::DrainState::new(),
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
                        let approval = SignedWorkspaceApproval::default();
                        approval.record("workspace-a".into());
                        Some(ApprovalDecision::Approve(approval.approval().unwrap()))
                    },
                    &registry,
                    None,
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
                    home: service_root.join("home").into(),
                    workspace_contexts: contexts,
                    client_credentials: credentials,
                    credential_path: None,
                    client_identity: None,
                    drain_state: muniment_core::attach::DrainState::new(),
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
