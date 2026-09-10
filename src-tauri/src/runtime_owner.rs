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
    }

    fn observe(&mut self, connected: bool, now: Instant) {
        if self.snapshot.busy {
            return;
        }
        if self.awaiting_disconnect {
            if connected {
                return;
            }
            self.awaiting_disconnect = false;
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
            self.snapshot.visible =
                self.snapshot.visible || first || now.duration_since(since) >= DWELL;
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
        self.snapshot.busy = false;
        self.outage = None;
        self.awaiting_disconnect = event == RuntimeEvent::Stopped;
        let visible = match event {
            RuntimeEvent::Connected => false,
            RuntimeEvent::Starting => self.snapshot.visible,
            _ => true,
        };
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
    let event = match app.path().app_data_dir() {
        Ok(directory) => {
            crate::windows_runtime_service::register_runtime_task_at_startup(&directory);
            crate::windows_runtime_service::start_runtime_task_at_startup(&directory)
        }
        Err(_) => RuntimeEvent::StartFailed,
    };
    #[cfg(target_os = "linux")]
    let event = match crate::linux_runtime_service::start_runtime(app) {
        Ok(()) => RuntimeEvent::Connected,
        Err(_) => RuntimeEvent::StartFailed,
    };
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
        let result = {
            let mut child = owner.child.lock().unwrap();
            match child.as_mut() {
                Some(process) => {
                    let result = process.kill().and_then(|()| process.wait().map(|_| ()));
                    if result.is_ok() {
                        *child = None;
                    }
                    result.map_err(|_| ())
                }
                None => crate::linux_runtime_service::stop_runtime(&app),
            }
        };
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
                        state.snapshot.last_event = RuntimeEvent::Exited;
                        state.observe(false, Instant::now());
                    });
                }
            }
        }
        let connected = app
            .state::<crate::attach_service::AttachCompanionState>()
            .runtime_connected();
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
            state.observe(connected, Instant::now());
        });
        std::thread::sleep(Duration::from_millis(100));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_restart_and_shared_dwell() {
        let now = Instant::now();
        let mut state = Lifecycle::default();
        state.observe(true, now);
        state.observe(false, now);
        assert!(!state.snapshot.visible);
        state.observe(false, now + DWELL - Duration::from_millis(1));
        assert!(!state.snapshot.visible);
        state.observe(false, now + DWELL);
        assert!(state.snapshot.visible);
        assert_eq!(state.snapshot.last_event, RuntimeEvent::Disconnected);
        assert!(state.start());
        assert!(!state.start());
        state.started(RuntimeEvent::Starting);
        state.observe(true, now + DWELL);
        assert!(!state.snapshot.visible);
        state.change(RuntimeEvent::Exited, true);
        assert!(state.snapshot.visible);
        assert!(state.start());
        state.started(RuntimeEvent::StartFailed);
        assert_eq!(state.snapshot.last_event, RuntimeEvent::StartFailed);
        assert!(state.snapshot.visible);
    }

    #[test]
    fn repeated_outages_do_not_extend_the_dwell() {
        let now = Instant::now();
        let mut state = Lifecycle::default();
        state.observe(true, now);
        state.observe(false, now);
        state.observe(false, now + Duration::from_secs(1));
        assert!(!state.snapshot.visible);
        state.observe(false, now + DWELL);
        assert!(state.snapshot.visible);
        state.observe(true, now + DWELL);
        state.observe(false, now + DWELL);
        assert!(!state.snapshot.visible);
        state.observe(false, now + DWELL + DWELL);
        assert!(state.snapshot.visible);
    }

    #[test]
    fn start_timeout_and_stop_preserve_the_last_action() {
        let now = Instant::now();
        let mut state = Lifecycle::default();
        assert!(state.start());
        state.observe(false, now);
        assert!(!state.snapshot.visible);
        state.started(RuntimeEvent::Starting);
        state.observe(false, now);
        state.observe(false, now + Duration::from_millis(9999));
        assert_eq!(state.snapshot.last_event, RuntimeEvent::Starting);
        state.observe(false, now + Duration::from_secs(10));
        assert_eq!(state.snapshot.last_event, RuntimeEvent::StartFailed);
        assert!(state.start());
        state.started(RuntimeEvent::Connected);
        assert!(!state.snapshot.visible);
        assert!(state.start());
        state.started(RuntimeEvent::Stopped);
        state.observe(true, now);
        assert_eq!(state.snapshot.last_event, RuntimeEvent::Stopped);
        state.observe(false, now);
        assert_eq!(state.snapshot.last_event, RuntimeEvent::Stopped);
        assert!(state.start());
        state.started(RuntimeEvent::Starting);
        state.observe(true, now);
        assert_eq!(state.snapshot.last_event, RuntimeEvent::Connected);
        assert!(!state.snapshot.visible);
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
        state.observe(false, now);
        assert!(state.snapshot.visible);
        state.observe(false, now + Duration::from_millis(100));
        assert!(state.snapshot.visible);
        state.observe(true, now);
        state.observe(false, now);
        state.observe(true, now + Duration::from_millis(250));
        state.observe(true, now + DWELL);
        assert!(!state.snapshot.visible);
    }
}
