use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};

const DWELL: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum RuntimeEvent {
    #[default]
    Starting,
    Connected,
    Disconnected,
    #[cfg(any(test, target_os = "linux"))]
    Exited,
    StartFailed,
    #[cfg(target_os = "macos")]
    RequiresApproval,
    #[cfg(target_os = "macos")]
    Approved,
    #[cfg(target_os = "macos")]
    NotFound,
    #[cfg(target_os = "macos")]
    RegistrationFailed,
    Stopped,
    StopFailed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    revision: u64,
    last_event: RuntimeEvent,
    visible: bool,
    busy: bool,
    cause: Option<String>,
}

#[derive(Default)]
pub(crate) struct RuntimeOwner {
    state: Mutex<Lifecycle>,
    #[cfg(target_os = "linux")]
    child: Mutex<Option<std::process::Child>>,
}

#[derive(Default)]
struct Lifecycle {
    snapshot: Snapshot,
    read_status: bool,
    outage: Option<Instant>,
    awaiting_disconnect: bool,
}

impl Lifecycle {
    fn change(&mut self, event: RuntimeEvent, visible: bool) {
        self.snapshot.last_event = event;
        self.snapshot.visible = visible;
        self.snapshot.cause = None;
    }

    #[cfg(target_os = "linux")]
    fn start_failed(&mut self, error: &str, desktop: bool, chat_events: bool) {
        self.started(RuntimeEvent::StartFailed);
        self.snapshot.cause = Some(format!(
            "{error} Desktop client connected: {desktop}. Chat events connected: {chat_events}."
        ));
    }

    #[cfg(any(test, target_os = "windows"))]
    fn windows_start_finished(&mut self, result: Result<(), String>) {
        match result {
            Ok(()) => self.started(RuntimeEvent::Starting),
            Err(cause) => {
                self.started(RuntimeEvent::StartFailed);
                self.snapshot.cause = Some(cause);
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn activation_finished(&mut self, result: Result<(), String>, clients: (bool, bool)) {
        match result {
            Ok(()) => self.started(RuntimeEvent::Connected),
            Err(error) => self.start_failed(&error, clients.0, clients.1),
        }
    }

    #[cfg(target_os = "linux")]
    fn observe_clients(&mut self, desktop: bool, chat_events: bool, now: Instant) {
        let starting = self.snapshot.last_event == RuntimeEvent::Starting;
        self.observe(desktop && chat_events, !desktop && !chat_events, now);
        if starting && self.snapshot.last_event == RuntimeEvent::StartFailed {
            self.start_failed(
                "The runtime clients did not connect within 10 seconds.",
                desktop,
                chat_events,
            );
        }
    }

    fn observe(&mut self, connected: bool, disconnected: bool, now: Instant) {
        if self.awaiting_disconnect {
            if connected || !disconnected {
                return;
            }
            self.awaiting_disconnect = false;
            self.snapshot.busy = false;
        }
        if self.snapshot.busy {
            return;
        }
        if connected {
            self.outage = None;
            self.change(RuntimeEvent::Connected, false);
        } else {
            let first = !self.read_status;
            let since = *self.outage.get_or_insert(now);
            if self.snapshot.last_event == RuntimeEvent::Connected {
                self.snapshot.last_event = RuntimeEvent::Disconnected;
            } else if self.snapshot.last_event == RuntimeEvent::Starting
                && now.duration_since(since) >= Duration::from_secs(10)
            {
                self.snapshot.last_event = RuntimeEvent::StartFailed;
            }
            self.snapshot.visible = self.snapshot.last_event != RuntimeEvent::Starting
                && (self.snapshot.visible || first || now.duration_since(since) >= DWELL);
        }
        self.read_status = true;
    }

    fn start(&mut self) -> bool {
        if self.snapshot.busy {
            return false;
        }
        self.snapshot.busy = true;
        // Keep the failure on screen until the start finishes.
        true
    }

    fn started(&mut self, event: RuntimeEvent) {
        self.awaiting_disconnect = event == RuntimeEvent::Stopped;
        self.snapshot.busy = self.awaiting_disconnect;
        self.outage = None;
        let visible = !matches!(event, RuntimeEvent::Connected | RuntimeEvent::Starting);
        self.change(event, visible);
    }
}

impl RuntimeOwner {
    fn update<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        update: impl FnOnce(&mut Lifecycle),
    ) {
        let mut state = self.state.lock().unwrap();
        let before = state.snapshot.clone();
        update(&mut state);
        if state.snapshot != before {
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            if state.snapshot.cause != before.cause || (before.busy && !state.snapshot.busy) {
                if let Some(cause) = &state.snapshot.cause {
                    eprintln!("The runtime start failed. {cause}");
                }
            }
            state.snapshot.revision += 1;
            let snapshot = state.snapshot.clone();
            drop(state);
            // Revisions let windows discard delayed events without a lock around callbacks.
            let _ = app.emit("runtime-state-changed", snapshot);
        }
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn keep_child(&self, child: std::process::Child) {
        *self.child.lock().unwrap() = Some(child);
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn stop_child(&self) -> Result<bool, ()> {
        let mut child = self.child.lock().unwrap();
        let Some(process) = child.as_mut() else {
            return Ok(false);
        };
        process
            .kill()
            .and_then(|()| process.wait())
            .map_err(|_| ())?;
        *child = None;
        Ok(true)
    }
}

#[tauri::command]
pub(crate) fn runtime_state(owner: tauri::State<'_, RuntimeOwner>) -> Snapshot {
    owner.state.lock().unwrap().snapshot.clone()
}

#[tauri::command]
pub(crate) async fn runtime_start(app: tauri::AppHandle) -> Result<(), &'static str> {
    tauri::async_runtime::spawn_blocking(move || start(&app))
        .await
        .map_err(|_| "The runtime start failed.")?;
    Ok(())
}

fn start<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let owner = app.state::<RuntimeOwner>();
    let mut claimed = false;
    owner.update(app, |state| claimed = state.start());
    if !claimed {
        return;
    }
    #[cfg(target_os = "macos")]
    let event = crate::macos_runtime_service::start();
    #[cfg(target_os = "windows")]
    {
        let result = app
            .path()
            .app_data_dir()
            .map_err(|error| format!("Runtime state lookup failed: {error}"))
            .and_then(|directory| {
                crate::windows_runtime_service::register_runtime_task_at_startup(&directory)?;
                crate::windows_runtime_service::start_runtime_task_at_startup(&directory)
            });
        owner.update(app, |state| state.windows_start_finished(result));
    }
    #[cfg(target_os = "linux")]
    {
        let result = crate::linux_runtime_service::start_runtime(app);
        let clients = app
            .state::<crate::attach_service::AttachCompanionState>()
            .runtime_client_connections();
        owner.update(app, |state| state.activation_finished(result, clients));
    }
    #[cfg(target_os = "macos")]
    owner.update(app, |state| state.started(event));
}

// Closing a window does not stop the runtime. Only an explicit stop does.
#[tauri::command]
pub(crate) async fn runtime_stop(app: tauri::AppHandle) -> Result<(), &'static str> {
    tauri::async_runtime::spawn_blocking(move || {
        let owner = app.state::<RuntimeOwner>();
        let mut claimed = false;
        owner.update(&app, |state| claimed = state.start());
        if !claimed {
            return Err("The runtime has a pending action.");
        }
        #[cfg(target_os = "macos")]
        let result = crate::macos_runtime_service::stop();
        #[cfg(target_os = "windows")]
        let result = crate::windows_runtime_service::stop_runtime();
        #[cfg(target_os = "linux")]
        let result = crate::linux_runtime_service::stop_runtime(&app);
        owner.update(&app, |state| {
            state.started(if result.is_ok() {
                RuntimeEvent::Stopped
            } else {
                RuntimeEvent::StopFailed
            });
        });
        result.map_err(|()| "The runtime stop failed.")
    })
    .await
    .map_err(|_| "The runtime stop failed.")?
}

pub(crate) fn setup<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    app.manage(RuntimeOwner::default());
    app.manage(crate::attach_service::AttachCompanionState::default());
    #[cfg(target_os = "linux")]
    crate::attach_service::start_runtime_clients(app);
    #[cfg(target_os = "windows")]
    {
        crate::attach_service::start_approval_presenter(app);
        crate::attach_service::start_desktop_client(app);
    }
    #[cfg(target_os = "macos")]
    crate::attach_service::start_desktop_client(app);
    let start_app = app.clone();
    std::thread::spawn(move || start(&start_app));
    let app = app.clone();
    std::thread::spawn(move || loop {
        let owner = app.state::<RuntimeOwner>();
        let revision = owner.state.lock().unwrap().snapshot.revision;
        #[cfg(target_os = "linux")]
        {
            let mut child = owner.child.lock().unwrap();
            if let Some(process) = child.as_mut() {
                if matches!(process.try_wait(), Ok(Some(_))) {
                    *child = None;
                    owner.update(&app, |state| {
                        state.change(RuntimeEvent::Exited, state.snapshot.visible);
                        state.observe(false, false, Instant::now());
                    });
                }
            }
        }
        let companion = app.state::<crate::attach_service::AttachCompanionState>();
        #[cfg(not(target_os = "linux"))]
        let connected = companion.runtime_connected();
        #[cfg(target_os = "linux")]
        let (desktop, chat_events) = companion.runtime_client_connections();
        #[cfg(not(target_os = "linux"))]
        let disconnected = !connected;
        #[cfg(target_os = "macos")]
        let requires_approval = crate::macos_runtime_service::requires_approval();
        owner.update(&app, |state| {
            if state.snapshot.revision != revision {
                return;
            }
            #[cfg(target_os = "macos")]
            if requires_approval == Some(true) && !state.snapshot.busy {
                state.change(RuntimeEvent::RequiresApproval, true);
                return;
            }
            #[cfg(target_os = "macos")]
            if requires_approval == Some(false)
                && state.snapshot.last_event == RuntimeEvent::RequiresApproval
            {
                state.change(RuntimeEvent::Approved, true);
            }
            #[cfg(target_os = "linux")]
            state.observe_clients(desktop, chat_events, Instant::now());
            #[cfg(not(target_os = "linux"))]
            state.observe(connected, disconnected, Instant::now());
        });
        std::thread::sleep(Duration::from_millis(100));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_queued_cause_reaches_the_snapshot_and_clears_on_recovery() {
        let app = tauri::test::mock_app();
        app.manage(RuntimeOwner::default());
        let owner = app.state::<RuntimeOwner>();
        let cause = "Task Scheduler kept the runtime task in state Queued for session id 7.";
        owner.update(app.handle(), |state| {
            assert!(state.start());
            state.windows_start_finished(Err(cause.to_owned()));
        });
        owner.update(app.handle(), |state| {
            state.observe(false, true, Instant::now());
        });
        let snapshot = runtime_state(app.state());
        assert_eq!(snapshot.last_event, RuntimeEvent::StartFailed);
        assert_eq!(snapshot.cause.as_deref(), Some(cause));
        assert!(snapshot.visible);
        assert!(!snapshot.busy);
        owner.update(app.handle(), |state| {
            assert!(state.start());
            assert!(!state.start());
            state.windows_start_finished(Ok(()));
        });
        assert_eq!(runtime_state(app.state()).cause, None);
        owner.update(app.handle(), |state| {
            state.observe(true, false, Instant::now());
        });
        assert!(!runtime_state(app.state()).visible);
    }

    #[test]
    fn windows_start_preserves_the_cause_until_a_connection_or_successful_retry() {
        let mut state = Lifecycle::default();
        let cause = "Task registration failed: RegisterTaskDefinition HRESULT(0x80070005)";
        assert!(state.start());
        state.windows_start_finished(Err(cause.into()));
        state.observe(false, true, Instant::now());
        assert_eq!(state.snapshot.last_event, RuntimeEvent::StartFailed);
        assert_eq!(state.snapshot.cause.as_deref(), Some(cause));
        assert!(state.snapshot.visible);
        assert!(!state.snapshot.busy);
        assert!(state.start());
        assert_eq!(state.snapshot.cause.as_deref(), Some(cause));
        state.windows_start_finished(Ok(()));
        assert_eq!(state.snapshot.cause, None);
        assert_eq!(state.snapshot.last_event, RuntimeEvent::Starting);
        state.windows_start_finished(Err(cause.into()));
        state.observe(true, false, Instant::now());
        assert_eq!(state.snapshot.cause, None);
        assert!(!state.snapshot.visible);
    }

    #[test]
    fn exit_restart_and_shared_dwell() {
        let now = Instant::now();
        let mut state = Lifecycle::default();
        state.observe(true, false, now);
        state.observe(false, true, now);
        assert!(!state.snapshot.visible);
        state.observe(false, true, now + DWELL - Duration::from_millis(1));
        assert!(!state.snapshot.visible);
        state.observe(false, true, now + DWELL);
        assert!(state.snapshot.visible);
        assert_eq!(state.snapshot.last_event, RuntimeEvent::Disconnected);
        assert!(state.start());
        assert!(!state.start());
        state.started(RuntimeEvent::Starting);
        state.observe(true, false, now + DWELL);
        assert!(!state.snapshot.visible);
        state.change(RuntimeEvent::Exited, true);
        assert!(state.snapshot.visible);
        assert!(state.start());
        state.started(RuntimeEvent::StartFailed);
        assert_eq!(state.snapshot.last_event, RuntimeEvent::StartFailed);
        assert!(state.snapshot.visible);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn ten_second_timeout_records_each_missing_client_and_clears_on_recovery() {
        let now = Instant::now();
        for (desktop, chat_events) in [(false, false), (true, false), (false, true)] {
            let app = tauri::test::mock_app();
            app.manage(RuntimeOwner::default());
            let owner = app.state::<RuntimeOwner>();
            for elapsed in [Duration::ZERO, DWELL, Duration::from_millis(9999)] {
                owner.update(app.handle(), |state| {
                    state.observe_clients(desktop, chat_events, now + elapsed);
                });
                let snapshot = runtime_state(app.state());
                assert_eq!(snapshot.last_event, RuntimeEvent::Starting);
                assert!(!snapshot.visible);
                assert_eq!(snapshot.cause, None);
            }
            owner.update(app.handle(), |state| {
                state.observe_clients(desktop, chat_events, now + Duration::from_secs(10));
            });
            let snapshot = runtime_state(app.state());
            assert_eq!(snapshot.last_event, RuntimeEvent::StartFailed);
            assert!(snapshot.visible);
            assert_eq!(snapshot.cause, Some(format!(
                "The runtime clients did not connect within 10 seconds. Desktop client connected: {desktop}. Chat events connected: {chat_events}."
            )));
            let serialized = serde_json::to_value(&snapshot).unwrap();
            assert_eq!(serialized["cause"], snapshot.cause.as_deref().unwrap());
            owner.update(app.handle(), |state| {
                state.observe_clients(desktop, chat_events, now + Duration::from_secs(11));
            });
            assert_eq!(runtime_state(app.state()), snapshot);
            owner.update(app.handle(), |state| {
                state.observe_clients(true, true, now + Duration::from_secs(12));
            });
            let recovered = runtime_state(app.state());
            assert_eq!(recovered.last_event, RuntimeEvent::Connected);
            assert!(!recovered.visible);
            assert_eq!(recovered.cause, None);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn activation_failure_publishes_the_error_and_both_client_states() {
        let app = tauri::test::mock_app();
        app.manage(RuntimeOwner::default());
        let owner = app.state::<RuntimeOwner>();
        owner.update(app.handle(), |state| assert!(state.start()));
        owner.update(app.handle(), |state| {
            state.activation_finished(
                Err(
                    "The runtime start timed out while waiting for the running runtime clients."
                        .into(),
                ),
                (true, false),
            );
        });
        let snapshot = runtime_state(app.state());
        assert_eq!(snapshot.last_event, RuntimeEvent::StartFailed);
        assert!(snapshot.visible);
        assert!(!snapshot.busy);
        assert_eq!(snapshot.cause.as_deref(), Some(
            "The runtime start timed out while waiting for the running runtime clients. Desktop client connected: true. Chat events connected: false."
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn activation_error_survives_busy_observations_and_retry() {
        let now = Instant::now();
        let mut state = Lifecycle::default();
        assert!(state.start());
        state.observe_clients(false, true, now + Duration::from_secs(10));
        assert_eq!(state.snapshot.last_event, RuntimeEvent::Starting);
        assert!(!state.snapshot.visible);
        state.activation_finished(Err("The startup lock denied access.".into()), (false, true));
        let failure = state.snapshot.clone();
        assert_eq!(failure.cause.as_deref(), Some(
            "The startup lock denied access. Desktop client connected: false. Chat events connected: true."
        ));
        assert!(failure.visible);
        assert!(!failure.busy);
        assert!(state.start());
        assert!(!state.start());
        state.observe_clients(true, true, now + Duration::from_secs(11));
        assert_eq!(state.snapshot.cause, failure.cause);
        state.activation_finished(Ok(()), (true, true));
        assert_eq!(state.snapshot.last_event, RuntimeEvent::Connected);
        assert_eq!(state.snapshot.cause, None);
        assert!(!state.snapshot.visible);
    }

    #[test]
    fn a_starting_owner_connects_at_the_timeout_boundary_without_a_notice() {
        let now = Instant::now();
        let mut state = Lifecycle::default();
        state.observe(false, true, now);
        assert!(!state.snapshot.visible);
        state.observe(true, false, now + Duration::from_secs(10));
        assert_eq!(state.snapshot.last_event, RuntimeEvent::Connected);
        assert!(!state.snapshot.visible);
        assert_eq!(state.snapshot.cause, None);
    }

    #[test]
    fn repeated_outages_do_not_extend_the_dwell() {
        let now = Instant::now();
        let mut state = Lifecycle::default();
        state.observe(true, false, now);
        state.observe(false, true, now);
        state.observe(false, true, now + Duration::from_secs(1));
        assert!(!state.snapshot.visible);
        state.observe(false, true, now + DWELL);
        assert!(state.snapshot.visible);
        state.observe(true, false, now + DWELL);
        state.observe(false, true, now + DWELL);
        assert!(!state.snapshot.visible);
        state.observe(false, true, now + DWELL + DWELL);
        assert!(state.snapshot.visible);
    }

    #[test]
    fn start_timeout_and_stop_preserve_the_last_action() {
        let now = Instant::now();
        let mut state = Lifecycle::default();
        assert!(state.start());
        state.observe(false, true, now);
        assert!(!state.snapshot.visible);
        state.started(RuntimeEvent::Starting);
        state.observe(false, true, now);
        state.observe(false, true, now + Duration::from_millis(9999));
        assert_eq!(state.snapshot.last_event, RuntimeEvent::Starting);
        assert!(!state.snapshot.visible);
        state.observe(false, true, now + Duration::from_secs(10));
        assert_eq!(state.snapshot.last_event, RuntimeEvent::StartFailed);
        assert!(state.start());
        state.started(RuntimeEvent::Connected);
        assert!(!state.snapshot.visible);
        assert!(state.start());
        state.started(RuntimeEvent::Stopped);
        state.observe(true, false, now);
        assert_eq!(state.snapshot.last_event, RuntimeEvent::Stopped);
        assert!(!state.start());
        state.observe(false, false, now);
        assert!(state.snapshot.busy);
        assert!(!state.start());
        state.observe(false, true, now);
        assert_eq!(state.snapshot.last_event, RuntimeEvent::Stopped);
        assert!(!state.snapshot.busy);
        assert!(state.start());
        state.started(RuntimeEvent::Starting);
        state.observe(true, false, now);
        assert_eq!(state.snapshot.last_event, RuntimeEvent::Connected);
        assert!(!state.snapshot.visible);
    }

    #[test]
    fn stop_blocks_restart_until_both_delayed_connections_close() {
        let now = Instant::now();
        for chat_closes_first in [true, false] {
            let mut state = Lifecycle::default();
            state.observe(true, false, now);
            assert!(state.start());
            state.started(RuntimeEvent::Stopped);
            for (attach, chat) in [
                (true, true),
                (chat_closes_first, !chat_closes_first),
                (chat_closes_first, !chat_closes_first),
            ] {
                state.observe(attach && chat, !attach && !chat, now + DWELL);
                assert!(state.snapshot.busy);
                assert_eq!(state.snapshot.last_event, RuntimeEvent::Stopped);
                assert!(!state.start());
            }
            // Reject contradictory samples if the callbacks run between status reads.
            state.observe(true, true, now + DWELL);
            assert!(!state.start());
            state.observe(false, true, now + DWELL);
            assert!(!state.snapshot.busy);
            assert_eq!(state.snapshot.last_event, RuntimeEvent::Stopped);
            assert!(state.start());
            assert!(!state.start());
            state.started(RuntimeEvent::Starting);
            state.observe(false, true, now + DWELL);
            assert_eq!(state.snapshot.last_event, RuntimeEvent::Starting);
            state.observe(true, false, now + DWELL);
            assert_eq!(state.snapshot.last_event, RuntimeEvent::Connected);
            assert!(!state.snapshot.visible);
        }
    }

    #[test]
    fn windows_share_one_action_and_one_revision() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let app = tauri::test::mock_app();
        app.manage(RuntimeOwner::default());
        let owner = app.state::<RuntimeOwner>();
        let claims = AtomicUsize::new(0);
        let handle = app.handle();
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    owner.update(handle, |state| {
                        if state.start() {
                            claims.fetch_add(1, Ordering::SeqCst);
                        }
                    });
                });
            }
        });
        assert_eq!(claims.load(Ordering::SeqCst), 1);
        let first_window = runtime_state(app.state());
        let second_window = runtime_state(app.state());
        assert_eq!(first_window, second_window);
        assert_eq!(first_window.revision, 1);
        assert!(first_window.busy);
        owner.update(app.handle(), |state| {
            state.started(RuntimeEvent::StartFailed)
        });
        let snapshot = runtime_state(app.state());
        assert_eq!(snapshot.revision, 2);
        assert_eq!(snapshot.last_event, RuntimeEvent::StartFailed);
        assert!(snapshot.visible);
    }

    #[test]
    fn first_outage_shows_at_once_and_short_drop_keeps_workspace() {
        let now = Instant::now();
        let mut state = Lifecycle::default();
        state.change(RuntimeEvent::Disconnected, false);
        state.observe(false, true, now);
        assert!(state.snapshot.visible);
        state.observe(false, true, now + Duration::from_millis(100));
        assert!(state.snapshot.visible);
        state.observe(true, false, now);
        state.observe(false, true, now);
        state.observe(true, false, now + Duration::from_millis(250));
        state.observe(true, false, now + DWELL);
        assert!(!state.snapshot.visible);
    }
}
