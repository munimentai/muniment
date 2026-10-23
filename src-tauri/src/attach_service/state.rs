use super::*;

#[cfg(unix)]
type PlatformChatEventStopHandle = ChatEventStopHandle;
#[cfg(target_os = "windows")]
type PlatformChatEventStopHandle = WindowsChatEventStopHandle;

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct AuthorizedCompanion {
    pub(crate) identity: String,
    pub(crate) claimed_kind: String,
    pub(crate) claimed_version: String,
    pub(crate) approved_at: Option<String>,
}

impl AttachCompanionState {
    #[cfg(target_os = "macos")]
    pub(crate) fn runtime_admission_failure_since(
        &self,
        since: std::time::Instant,
    ) -> Option<serde_json::Value> {
        self.desktop_client_holder.admission_failure_since(since)
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn runtime_connected(&self) -> bool {
        let status = self.listener_status();
        status.connected && status.chat_events_connected
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(crate) fn runtime_client_connections(&self) -> (bool, bool) {
        let desktop = self
            .connected
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let chat_events = self
            .chat_events_connected
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (*desktop, *chat_events)
    }
}

pub struct AttachCompanionState {
    #[cfg(any(unix, target_os = "windows"))]
    pub(super) approval_presenter: Mutex<Option<ApprovalPresenterStopHandle>>,
    #[cfg(any(unix, target_os = "windows"))]
    pub(super) desktop_supervisor_lifecycle: Mutex<()>,
    #[cfg(any(unix, target_os = "windows"))]
    pub(super) desktop_client: Mutex<Option<DesktopClientSupervisor>>,
    #[cfg(any(unix, target_os = "windows"))]
    pub(super) desktop_client_holder: DesktopClientHolder,
    #[cfg(any(unix, target_os = "windows"))]
    pub(super) chat_events: Mutex<Option<ChatEventSupervisor>>,
    #[cfg(any(unix, target_os = "windows"))]
    pub(super) connected: Mutex<bool>,
    #[cfg(any(unix, target_os = "windows"))]
    pub(super) connected_changed: Condvar,
    #[cfg(any(unix, target_os = "windows"))]
    pub(super) chat_events_connected: Mutex<bool>,
    /// The connections of every paired runtime, by runtime id. Each one
    /// stays connected while the app runs.
    pub(super) remotes: Mutex<std::collections::HashMap<String, crate::runtimes::RemoteSupervisor>>,
    /// The paired runtime requests go to: the one holding the open thread, or
    /// the one the new-thread picker chose. `None` is this computer.
    pub(super) active: Mutex<Option<String>>,
    /// The runtime each listed thread lives on, from the merged thread list.
    pub(super) thread_runtimes: Mutex<std::collections::HashMap<String, String>>,
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
    #[cfg(all(test, target_os = "linux"))]
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

    #[cfg(any(unix, target_os = "windows"))]
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

    #[cfg(test)]
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

    #[cfg(test)]
    pub(super) fn stop_desktop_client(&self) {
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

    #[cfg(all(test, unix))]
    pub(super) fn stop_chat_events(&self) {
        let _lifecycle = self
            .desktop_supervisor_lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.stop_chat_events_locked();
    }

    #[cfg(test)]
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

    /// The session of the selected runtime: a paired runtime's when one is
    /// selected, else this computer's.
    #[cfg(any(unix, target_os = "windows"))]
    pub(crate) fn desktop_client_session(&self) -> DesktopClientSession {
        self.remote_session()
            .unwrap_or_else(|| self.local_client_session())
    }

    fn remote_session(&self) -> Option<DesktopClientSession> {
        let id = self.remote_runtime_id()?;
        Some(self.paired_session(&id))
    }

    /// The session of the paired runtime `id`, whichever runtime is active.
    #[cfg(any(unix, target_os = "windows"))]
    pub(crate) fn paired_session(&self, id: &str) -> DesktopClientSession {
        match lock(&self.remotes).get(id) {
            Some(remote) if remote.connected() => {
                DesktopClientSession::Connected(remote.holder.clone())
            }
            _ => DesktopClientSession::Disconnected,
        }
    }

    /// The session of this computer's runtime, whichever runtime is
    /// selected. The cloud account and same-machine companions live there.
    #[cfg(any(unix, target_os = "windows"))]
    pub(crate) fn local_client_session(&self) -> DesktopClientSession {
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

    /// The session once the client connects, or after `wait` with no connection.
    /// A history read at window open lands before the first connection, and
    /// this wait keeps that read from reporting the service unreachable.
    #[cfg(any(unix, target_os = "windows"))]
    pub(crate) fn desktop_client_session_within(
        &self,
        wait: std::time::Duration,
    ) -> DesktopClientSession {
        let deadline = std::time::Instant::now() + wait;
        if self.remote_active() {
            loop {
                match self.desktop_client_session() {
                    DesktopClientSession::Disconnected if std::time::Instant::now() < deadline => {
                        std::thread::sleep(std::time::Duration::from_millis(25));
                    }
                    session => return session,
                }
            }
        }
        self.local_client_session_within(wait)
    }

    /// This computer's session once its client connects, or after `wait`
    /// with no connection, whichever runtime is active.
    #[cfg(any(unix, target_os = "windows"))]
    pub(crate) fn local_client_session_within(
        &self,
        wait: std::time::Duration,
    ) -> DesktopClientSession {
        let deadline = std::time::Instant::now() + wait;
        loop {
            match self.local_client_session() {
                DesktopClientSession::Disconnected => {
                    let now = std::time::Instant::now();
                    if now >= deadline {
                        return DesktopClientSession::Disconnected;
                    }
                    let connected = self
                        .connected
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if *connected {
                        continue;
                    }
                    let _ = self
                        .connected_changed
                        .wait_timeout(connected, deadline - now)
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                }
                session => return session,
            }
        }
    }

    pub(super) fn record_connected(&self, connected: bool) {
        *self
            .connected
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = connected;
        self.connected_changed.notify_all();
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
}

#[cfg(any(unix, target_os = "windows"))]
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

#[cfg(any(unix, target_os = "windows"))]
impl Default for AttachCompanionState {
    fn default() -> Self {
        Self {
            approval_presenter: Mutex::new(None),
            desktop_supervisor_lifecycle: Mutex::new(()),
            desktop_client: Mutex::new(None),
            desktop_client_holder: DesktopClientHolder::new(),
            chat_events: Mutex::new(None),
            connected: Mutex::new(false),
            connected_changed: Condvar::new(),
            chat_events_connected: Mutex::new(false),
            remotes: Mutex::new(std::collections::HashMap::new()),
            active: Mutex::new(None),
            thread_runtimes: Mutex::new(std::collections::HashMap::new()),
        }
    }
}

#[cfg(not(any(unix, target_os = "windows")))]
impl Default for AttachCompanionState {
    fn default() -> Self {
        Self {
            remotes: Mutex::new(std::collections::HashMap::new()),
            active: Mutex::new(None),
            thread_runtimes: Mutex::new(std::collections::HashMap::new()),
        }
    }
}

impl AttachCompanionState {
    pub(crate) fn clear_workspace(&self) {}

    /// Whether a paired runtime is active.
    pub(crate) fn remote_active(&self) -> bool {
        self.remote_runtime_id().is_some()
    }

    /// The active paired runtime's id, or `None` for this computer.
    pub(crate) fn remote_runtime_id(&self) -> Option<String> {
        lock(&self.active).clone()
    }

    /// Makes `id` the runtime requests go to, `None` for this computer, and
    /// answers whether that changed.
    pub(crate) fn set_active(&self, id: Option<String>) -> bool {
        let mut active = lock(&self.active);
        let changed = *active != id;
        *active = id;
        changed
    }

    /// Opens connections of their own to the active paired runtime.
    pub(crate) fn remote_connector(&self) -> Option<crate::runtimes::Connector> {
        self.paired_connector(&self.remote_runtime_id()?)
    }

    /// Opens connections of their own to the paired runtime `id`, active or not.
    pub(crate) fn paired_connector(&self, id: &str) -> Option<crate::runtimes::Connector> {
        lock(&self.remotes)
            .get(id)
            .map(crate::runtimes::RemoteSupervisor::connector)
    }

    /// Whether the paired runtime `id` has connections.
    pub(crate) fn has_remote(&self, id: &str) -> bool {
        lock(&self.remotes).contains_key(id)
    }

    /// The ids of the paired runtimes with connections.
    pub(crate) fn remote_ids(&self) -> Vec<String> {
        lock(&self.remotes).keys().cloned().collect()
    }

    /// Keeps the connections `start` opens to the paired runtime `id` unless
    /// it has some, and answers whether `start` ran. Callers that race for
    /// one runtime start one set of connections between them.
    pub(crate) fn insert_remote_once(
        &self,
        id: &str,
        start: impl FnOnce() -> crate::runtimes::RemoteSupervisor,
    ) -> bool {
        let mut remotes = lock(&self.remotes);
        if remotes.contains_key(id) {
            return false;
        }
        remotes.insert(id.to_owned(), start());
        true
    }

    /// Takes the connections of the paired runtime `id`. An active runtime
    /// that loses its connections hands requests back to this computer.
    pub(crate) fn remove_remote(&self, id: &str) -> Option<crate::runtimes::RemoteSupervisor> {
        let removed = lock(&self.remotes).remove(id);
        let mut active = lock(&self.active);
        if active.as_deref() == Some(id) {
            *active = None;
        }
        removed
    }

    /// Takes every paired runtime's connections.
    pub(crate) fn take_remotes(&self) -> Vec<crate::runtimes::RemoteSupervisor> {
        *lock(&self.active) = None;
        lock(&self.remotes)
            .drain()
            .map(|(_, remote)| remote)
            .collect()
    }

    /// Records which runtime each listed thread lives on.
    pub(crate) fn remember_thread_runtimes(
        &self,
        threads: impl IntoIterator<Item = (String, String)>,
    ) {
        lock(&self.thread_runtimes).extend(threads);
    }

    /// The runtime a listed thread lives on, when the merged list named it.
    pub(crate) fn thread_runtime(&self, thread_id: &str) -> Option<String> {
        lock(&self.thread_runtimes).get(thread_id).cloned()
    }

    /// The version this computer's runtime named in its welcome, while the
    /// app holds a connection to it.
    #[cfg(any(unix, target_os = "windows"))]
    pub(crate) fn local_runtime_version(&self) -> Option<String> {
        self.desktop_client_holder.runtime_version()
    }

    #[cfg(not(any(unix, target_os = "windows")))]
    pub(crate) fn local_runtime_version(&self) -> Option<String> {
        None
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
