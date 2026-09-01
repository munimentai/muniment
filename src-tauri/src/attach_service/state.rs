use super::*;

#[cfg(target_os = "linux")]
pub(super) type WorkspaceContexts = Arc<Mutex<WorkspaceContextMap>>;

#[cfg(unix)]
type PlatformChatEventStopHandle = ChatEventStopHandle;
#[cfg(target_os = "windows")]
type PlatformChatEventStopHandle = WindowsChatEventStopHandle;

#[cfg(target_os = "linux")]
pub(crate) struct AttachListenerState {
    pub(super) workspace_contexts: WorkspaceContexts,
    pub(super) client_credentials: Arc<Mutex<HashMap<String, ClientCredential>>>,
    pub(super) companion_registry: CompanionRegistry,
    pub(super) approval: SignedWorkspaceApproval,
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
    pub(super) listener: Mutex<Option<Arc<AttachListenerState>>>,
    #[cfg(target_os = "linux")]
    pub(super) approval: SignedWorkspaceApproval,
    #[cfg(target_os = "linux")]
    pub(super) listener_lifecycle: Mutex<AttachListenerLifecycle>,
    #[cfg(target_os = "linux")]
    pub(super) listener_stop: Mutex<AttachListenerStopState>,
    #[cfg(target_os = "linux")]
    pub(super) listener_stopped: Condvar,
    #[cfg(unix)]
    pub(super) approval_presenter: Mutex<Option<ApprovalPresenterStopHandle>>,
    #[cfg(target_os = "linux")]
    pub(super) presenting: Mutex<bool>,
    #[cfg(any(unix, target_os = "windows"))]
    pub(super) desktop_supervisor_lifecycle: Mutex<()>,
    #[cfg(all(test, target_os = "linux"))]
    pub(super) desktop_stop_started: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    #[cfg(any(unix, target_os = "windows"))]
    pub(super) desktop_client: Mutex<Option<DesktopClientSupervisor>>,
    #[cfg(any(unix, target_os = "windows"))]
    pub(super) desktop_client_holder: DesktopClientHolder,
    #[cfg(any(unix, target_os = "windows"))]
    pub(super) chat_events: Mutex<Option<ChatEventSupervisor>>,
    #[cfg(any(unix, target_os = "windows"))]
    pub(super) connected: Mutex<bool>,
    #[cfg(any(unix, target_os = "windows"))]
    pub(super) chat_events_connected: Mutex<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct AttachListenerStatus {
    pub(super) started: bool,
    pub(super) failure: Option<&'static str>,
    pub(super) pending: bool,
    pub(super) stopped: bool,
    pub(super) presenting: bool,
    pub(super) supervisor_running: bool,
    pub(super) connected: bool,
    pub(super) chat_events_connected: bool,
    pub(super) runtime_upgrade_pending: bool,
}

#[cfg(target_os = "linux")]
pub(super) enum AttachListenerStopState {
    Pending { stop_requested: bool },
    Listening(AttachStopHandle),
    Stopped,
}

#[cfg(any(unix, target_os = "windows"))]
pub(super) struct DesktopClientSupervisor {
    pub(super) stop: DesktopClientStopHandle,
    pub(super) worker: std::thread::JoinHandle<()>,
}

#[cfg(any(unix, target_os = "windows"))]
pub(super) struct ChatEventSupervisor {
    #[cfg(unix)]
    pub(super) stop: ChatEventStopHandle,
    #[cfg(target_os = "windows")]
    pub(super) stop: WindowsChatEventStopHandle,
    pub(super) worker: std::thread::JoinHandle<()>,
}

#[cfg(unix)]
#[derive(Clone, Default)]
pub(super) struct ChatEventStopHandle {
    pub(super) inner: Arc<(Mutex<ChatEventStopState>, Condvar)>,
}

#[cfg(unix)]
#[derive(Default)]
pub(super) struct ChatEventStopState {
    pub(super) stopped: bool,
    pub(super) stream: Option<UnixStream>,
    #[cfg(test)]
    pub(super) connect_started: Option<std::sync::mpsc::Sender<()>>,
}

#[cfg(unix)]
impl InterruptibleConnectState for ChatEventStopState {
    fn stopped(&self) -> bool {
        self.stopped
    }

    fn set_stream(&mut self, stream: Option<UnixStream>) {
        #[cfg(test)]
        if stream.is_some() {
            if let Some(started) = self.connect_started.take() {
                let _ = started.send(());
            }
        }
        self.stream = stream;
    }
}

#[cfg(unix)]
impl ChatEventStopHandle {
    #[cfg(test)]
    pub(super) fn for_test() -> (Self, std::sync::mpsc::Receiver<()>) {
        let (connect_started, started) = std::sync::mpsc::channel();
        let stop = Self {
            inner: Arc::new((
                Mutex::new(ChatEventStopState {
                    connect_started: Some(connect_started),
                    ..ChatEventStopState::default()
                }),
                Condvar::new(),
            )),
        };
        (stop, started)
    }

    pub(super) fn stop(&self) {
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

#[cfg(any(unix, target_os = "windows"))]
pub(crate) enum DesktopClientSession {
    NoSupervisor,
    Connected(DesktopClientHolder),
    Disconnected,
}

#[cfg(any(unix, target_os = "windows"))]
impl AttachCompanionState {
    #[cfg(all(target_os = "linux", test))]
    pub(super) fn new(listener: Arc<AttachListenerState>) -> Self {
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
            #[cfg(all(test, target_os = "linux"))]
            desktop_stop_started: Mutex::new(None),
            desktop_client: Mutex::new(None),
            desktop_client_holder: DesktopClientHolder::new(),
            chat_events: Mutex::new(None),
            connected: Mutex::new(false),
            chat_events_connected: Mutex::new(false),
        }
    }

    #[cfg(target_os = "linux")]
    pub(super) fn set_listener(&self, listener: Arc<AttachListenerState>) {
        *self
            .listener
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(listener);
    }

    #[cfg(target_os = "linux")]
    pub(super) fn listener(&self) -> Result<Arc<AttachListenerState>, ProtocolError> {
        self.listener
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?
            .clone()
            .ok_or_else(ProtocolError::persistence_failed)
    }

    #[cfg(target_os = "linux")]
    pub(super) fn record_listener_started(&self) {
        self.stop_approval_presenter();
        self.stop_desktop_client();
        self.listener_lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .record_listening();
    }

    #[cfg(target_os = "linux")]
    pub(super) fn record_listener_pending(&self) {
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

    #[cfg(target_os = "linux")]
    pub(super) fn record_listener_start_failure(&self, failure: AttachListenerStartFailure) {
        self.listener_lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .record_failure(failure);
        self.record_listener_finished();
    }

    #[cfg(target_os = "linux")]
    pub(super) fn publish_listener_stop(&self, stop: AttachStopHandle) {
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

    #[cfg(target_os = "linux")]
    pub(super) fn record_listener_stopped(&self) {
        self.listener_lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .record_stopped();
        self.record_listener_finished();
    }

    #[cfg(target_os = "linux")]
    pub(super) fn record_listener_finished(&self) {
        *self
            .listener_stop
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = AttachListenerStopState::Stopped;
        self.listener_stopped.notify_all();
    }

    #[cfg(target_os = "linux")]
    pub(super) fn stop_listener(&self) {
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

    #[cfg(target_os = "linux")]
    pub(super) fn listener_status(&self) -> AttachListenerStatus {
        let lifecycle = *self
            .listener_lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let failure = match lifecycle {
            AttachListenerLifecycle::Failed(failure) => Some(failure),
            _ => None,
        };
        let connected = *self
            .connected
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
            connected,
            chat_events_connected: *self
                .chat_events_connected
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            runtime_upgrade_pending: connected
                && runtime_upgrade_pending(&self.desktop_client_holder),
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(super) fn listener_status(&self) -> AttachListenerStatus {
        let connected = *self
            .connected
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        AttachListenerStatus {
            started: true,
            failure: None,
            pending: false,
            stopped: false,
            presenting: false,
            supervisor_running: self
                .desktop_client
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_some(),
            connected,
            chat_events_connected: *self
                .chat_events_connected
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            runtime_upgrade_pending: connected
                && runtime_upgrade_pending(&self.desktop_client_holder),
        }
    }

    #[cfg(unix)]
    pub(super) fn start_approval_presenter(&self, start: impl FnOnce(ApprovalPresenterStopHandle)) {
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

    #[cfg(unix)]
    pub(super) fn stop_approval_presenter(&self) {
        if let Some(stop) = self
            .approval_presenter
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            stop.stop();
        }
    }

    #[cfg(any(test, target_os = "windows"))]
    pub(super) fn start_desktop_client(
        &self,
        start: impl FnOnce(DesktopClientStopHandle, DesktopClientHolder) -> std::thread::JoinHandle<()>,
    ) {
        let _lifecycle = self
            .desktop_supervisor_lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.start_desktop_client_locked(start);
    }

    pub(super) fn start_desktop_client_locked(
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

    #[cfg(all(test, unix))]
    pub(super) fn start_chat_events(
        &self,
        start: impl FnOnce(ChatEventStopHandle) -> std::thread::JoinHandle<()>,
    ) {
        let _lifecycle = self
            .desktop_supervisor_lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.start_chat_events_locked(start);
    }

    #[cfg(unix)]
    pub(super) fn start_chat_events_locked(
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

    #[cfg(target_os = "windows")]
    pub(super) fn start_chat_events_locked(
        &self,
        start: impl FnOnce(WindowsChatEventStopHandle) -> std::thread::JoinHandle<()>,
    ) {
        let mut supervisor = self
            .chat_events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if supervisor.is_some() {
            return;
        }
        let Ok(stop) = WindowsChatEventStopHandle::new() else {
            return;
        };
        let worker = start(stop.clone());
        *supervisor = Some(ChatEventSupervisor { stop, worker });
    }

    #[cfg(any(unix, target_os = "windows"))]
    pub(super) fn start_desktop_supervisors(
        &self,
        start_client: impl FnOnce(
            DesktopClientStopHandle,
            DesktopClientHolder,
        ) -> std::thread::JoinHandle<()>,
        start_chat_events: impl FnOnce(PlatformChatEventStopHandle) -> std::thread::JoinHandle<()>,
    ) {
        let _lifecycle = self
            .desktop_supervisor_lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.start_desktop_client_locked(start_client);
        self.start_chat_events_locked(start_chat_events);
    }

    pub(super) fn stop_desktop_client(&self) {
        let _lifecycle = self
            .desktop_supervisor_lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        #[cfg(all(test, target_os = "linux"))]
        if let Some(started) = self
            .desktop_stop_started
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            let _ = started.send(());
        }
        #[cfg(any(unix, target_os = "windows"))]
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

    #[cfg(all(test, unix))]
    pub(super) fn stop_chat_events(&self) {
        let _lifecycle = self
            .desktop_supervisor_lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.stop_chat_events_locked();
    }

    #[cfg(any(unix, target_os = "windows"))]
    pub(super) fn stop_chat_events_locked(&self) {
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

    #[cfg(any(unix, target_os = "windows"))]
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

    pub(super) fn record_connected(&self, connected: bool) {
        *self
            .connected
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = connected;
    }

    pub(super) fn record_chat_events_connected(&self, connected: bool) {
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

    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn observe_desktop_stop_start(&self) -> std::sync::mpsc::Receiver<()> {
        let (started, receiver) = std::sync::mpsc::channel();
        *self
            .desktop_stop_started
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(started);
        receiver
    }

    #[cfg(target_os = "linux")]
    pub(super) fn record_presenting(&self, presenting: bool) {
        *self
            .presenting
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = presenting;
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn record_workspace(&self, workspace: String) {
        self.approval.record(workspace);
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn clear_workspace(&self) {
        self.approval.clear();
    }

    #[cfg(target_os = "linux")]
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
            #[cfg(all(test, target_os = "linux"))]
            desktop_stop_started: Mutex::new(None),
            desktop_client: Mutex::new(None),
            desktop_client_holder: DesktopClientHolder::new(),
            chat_events: Mutex::new(None),
            connected: Mutex::new(false),
            chat_events_connected: Mutex::new(false),
        }
    }
}

#[cfg(any(unix, target_os = "windows"))]
impl Drop for AttachCompanionState {
    fn drop(&mut self) {
        #[cfg(unix)]
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
        #[cfg(any(unix, target_os = "windows"))]
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

#[cfg(target_os = "macos")]
impl Default for AttachCompanionState {
    fn default() -> Self {
        Self {
            approval_presenter: Mutex::new(None),
            desktop_supervisor_lifecycle: Mutex::new(()),
            desktop_client: Mutex::new(None),
            desktop_client_holder: DesktopClientHolder::new(),
            chat_events: Mutex::new(None),
            connected: Mutex::new(false),
            chat_events_connected: Mutex::new(false),
        }
    }
}

#[cfg(target_os = "windows")]
impl Default for AttachCompanionState {
    fn default() -> Self {
        Self {
            desktop_supervisor_lifecycle: Mutex::new(()),
            desktop_client: Mutex::new(None),
            desktop_client_holder: DesktopClientHolder::new(),
            chat_events: Mutex::new(None),
            connected: Mutex::new(false),
            chat_events_connected: Mutex::new(false),
        }
    }
}

#[cfg(not(any(unix, target_os = "windows")))]
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
