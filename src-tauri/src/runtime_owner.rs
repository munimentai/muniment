use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};

#[cfg(target_os = "linux")]
use crate::linux_runtime_service::RuntimeChild;
#[cfg(target_os = "macos")]
use std::process::Child as RuntimeChild;

const DWELL: Duration = Duration::from_secs(2);
/// The wait before the first automatic start after a failed one. Each
/// further failure doubles it up to [`RETRY_LIMIT`].
const RETRY_FIRST: Duration = Duration::from_secs(2);
const RETRY_LIMIT: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum RuntimeEvent {
    #[default]
    Starting,
    Connected,
    Disconnected,
    #[cfg(any(test, target_os = "linux", target_os = "macos"))]
    Exited,
    StartFailed,
    #[cfg(any(test, all(target_os = "macos", not(feature = "local-runtime"))))]
    RequiresApproval,
    #[cfg(all(target_os = "macos", any(test, not(feature = "local-runtime"))))]
    NotFound,
    #[cfg(any(test, target_os = "macos"))]
    RegistrationFailed,
    #[cfg(any(test, all(target_os = "macos", not(feature = "local-runtime"))))]
    ServiceInactive,
    #[cfg(any(test, target_os = "macos"))]
    ChildStarted,
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
    /// The background service could not stay running, so the runtime runs as
    /// the desktop's child and stops when the desktop quits.
    foreground_only: bool,
    /// The service that ran the runtime stopped because Login Items turned it
    /// off, and the desktop's child takes over. The workspace stays on screen
    /// until the child connects or its start fails.
    reconnecting: bool,
}

#[derive(Default)]
pub(crate) struct RuntimeOwner {
    state: Mutex<Lifecycle>,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    child: Mutex<Option<RuntimeChild>>,
}

#[derive(Default)]
struct Lifecycle {
    snapshot: Snapshot,
    read_status: bool,
    outage: Option<Instant>,
    #[cfg(any(test, target_os = "macos"))]
    startup_attempt: Option<Instant>,
    awaiting_disconnect: bool,
    /// Automatic starts since the runtime last connected.
    retries: u32,
    /// When the owner starts the runtime again after a failed start.
    retry_at: Option<Instant>,
    /// Set while an automatic start runs, so its request keeps the failure on screen.
    retrying: bool,
    /// The child runs because Login Items keeps the service off, so the
    /// service takes the runtime over once the user turns it on.
    #[cfg(any(test, target_os = "macos"))]
    child_until_approval: bool,
    /// The last status read found the service off in Login Items.
    #[cfg(any(test, target_os = "macos"))]
    service_off: bool,
}

/// Why a start ran the runtime as the desktop's child.
#[cfg(any(test, target_os = "macos"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Child {
    /// The service runs the runtime, or nothing does.
    None,
    /// The service never registers or never stays running.
    #[cfg(any(test, all(target_os = "macos", not(feature = "local-runtime"))))]
    ServiceUnavailable,
    /// The user turned the service off in Login Items.
    LoginItemsOff,
}

/// What the approval watch does after it reads the service status.
#[cfg(any(test, target_os = "macos"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ApprovalStep {
    Wait,
    /// Stop the child and start the service the user turned on.
    HandOver,
    /// Start the runtime again: the service that ran it stopped because the
    /// user turned it off in Login Items, so the start runs a child.
    Restart,
}

/// How long the owner waits before its automatic start after `retries` earlier ones.
fn retry_delay(retries: u32) -> Duration {
    RETRY_FIRST
        .saturating_mul(2u32.saturating_pow(retries))
        .min(RETRY_LIMIT)
}

/// Whether a start reports only that its request went out, and the clients
/// still have to connect.
fn start_requested(event: RuntimeEvent) -> bool {
    #[cfg(any(test, target_os = "macos"))]
    if event == RuntimeEvent::ChildStarted {
        return true;
    }
    event == RuntimeEvent::Starting
}

/// Starts the runtime through the background service, and runs it as the
/// desktop's child when the service never registers, never stays running, or
/// is turned off in Login Items. The second value names why that child
/// started, since the runtime then stops with the desktop.
#[cfg(any(test, all(target_os = "macos", not(feature = "local-runtime"))))]
fn start_with_fallback(
    service: impl FnOnce() -> RuntimeEvent,
    child: impl FnOnce() -> RuntimeEvent,
) -> (RuntimeEvent, Child) {
    let reason = match service() {
        RuntimeEvent::RequiresApproval => Child::LoginItemsOff,
        RuntimeEvent::RegistrationFailed | RuntimeEvent::ServiceInactive => {
            Child::ServiceUnavailable
        }
        event => return (event, Child::None),
    };
    match child() {
        RuntimeEvent::ChildStarted => (RuntimeEvent::ChildStarted, reason),
        event => (event, Child::None),
    }
}

impl Lifecycle {
    /// Records a start and whether its runtime lives only as long as the desktop.
    #[cfg(any(test, target_os = "macos"))]
    fn service_started(&mut self, event: RuntimeEvent, child: Child) {
        self.snapshot.foreground_only = child != Child::None;
        self.child_until_approval = child == Child::LoginItemsOff;
        self.started(event);
    }

    /// Claims the handover from a child that runs because Login Items keeps
    /// the service off, once the user turns the service on and no agent run is
    /// in progress. The claim marks the owner busy until the service start ends.
    #[cfg(any(test, target_os = "macos"))]
    fn claim_handover(&mut self, run_in_progress: bool) -> bool {
        if !self.child_until_approval || run_in_progress || !self.start_attempt() {
            return false;
        }
        self.child_until_approval = false;
        true
    }

    /// Claims a start once the service that ran the runtime stopped and the
    /// service is off, as when the user turns it off in Login Items while the
    /// desktop is open. Only a lost connection claims it, so a stop the user
    /// asked for, a child that runs, or a failed child start never repeats it.
    /// An agent run the service had in progress ends with it, and the child
    /// marks it interrupted when its lease expires.
    #[cfg(any(test, target_os = "macos"))]
    fn claim_restart(&mut self) -> bool {
        if self.snapshot.foreground_only
            || !matches!(
                self.snapshot.last_event,
                RuntimeEvent::Disconnected | RuntimeEvent::Exited
            )
            || !self.start_attempt()
        {
            return false;
        }
        // The user turned the service off. The child start that follows is a
        // reconnect behind the Login Items notice, never a blocking screen.
        self.snapshot.visible = false;
        self.snapshot.reconnecting = true;
        self.snapshot.foreground_only = true;
        true
    }

    /// Restores the child's state after a handover that could not stop it.
    #[cfg(any(test, target_os = "macos"))]
    fn handover_failed(&mut self) {
        self.child_until_approval = true;
        self.snapshot.busy = false;
    }

    fn change(&mut self, event: RuntimeEvent, visible: bool) {
        if event == RuntimeEvent::Connected {
            self.retries = 0;
            self.retry_at = None;
        }
        self.snapshot.reconnecting &= event == RuntimeEvent::ChildStarted;
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

    #[cfg(any(test, target_os = "macos"))]
    fn observe_endpoint_clients(
        &mut self,
        clients: (bool, bool),
        now: Instant,
        endpoint: Option<&std::path::Path>,
        failure: Option<serde_json::Value>,
        admissions: impl FnOnce(Instant) -> Vec<serde_json::Value>,
    ) {
        if self.awaiting_disconnect && (clients.0 || clients.1) {
            return;
        }
        let starting = start_requested(self.snapshot.last_event);
        self.observe_endpoint_with_failure(clients.0 && clients.1, now, endpoint, failure);
        if starting && self.snapshot.last_event == RuntimeEvent::StartFailed {
            if let Some(cause) = &mut self.snapshot.cause {
                cause.push_str(&format!(
                    " Desktop client connected: {}. Chat events connected: {}. Admissions: {}.",
                    clients.0,
                    clients.1,
                    serde_json::Value::Array(admissions(self.startup_attempt.unwrap_or(now)))
                ));
            }
        }
    }

    #[cfg(any(test, target_os = "macos"))]
    fn observe_endpoint_with_failure(
        &mut self,
        connected: bool,
        now: Instant,
        endpoint: Option<&std::path::Path>,
        failure: Option<serde_json::Value>,
    ) {
        let starting = start_requested(self.snapshot.last_event);
        self.observe_endpoint(connected, now, endpoint);
        if starting && self.snapshot.last_event == RuntimeEvent::StartFailed {
            if let (Some(cause), Some(failure)) = (&mut self.snapshot.cause, failure) {
                cause.push_str(&format!(" Attach failure: {failure}."));
            }
        }
    }

    #[cfg(any(test, target_os = "macos"))]
    fn observe_endpoint(
        &mut self,
        connected: bool,
        now: Instant,
        endpoint: Option<&std::path::Path>,
    ) {
        let starting = start_requested(self.snapshot.last_event);
        let elapsed = now.saturating_duration_since(self.outage.unwrap_or(now));
        self.observe(connected, !connected, now);
        if starting && self.snapshot.last_event == RuntimeEvent::StartFailed {
            let endpoint = match endpoint {
                Some(path) => format!(
                    "The desktop waited for runtime endpoint {}.",
                    path.display()
                ),
                None => "The desktop could not resolve the runtime endpoint.".to_owned(),
            };
            self.snapshot.cause = Some(format!(
                "The runtime clients did not connect within 10 seconds. {endpoint} Startup elapsed: {} ms. Startup bound: 10000 ms.",
                elapsed.as_millis()
            ));
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
                // The service stops once the user turns it off in Login
                // Items, and the approval watch then starts the child.
                #[cfg(any(test, target_os = "macos"))]
                {
                    self.snapshot.reconnecting =
                        self.service_off && !self.snapshot.foreground_only;
                }
            } else if start_requested(self.snapshot.last_event)
                && now.duration_since(since) >= Duration::from_secs(10)
            {
                self.snapshot.last_event = RuntimeEvent::StartFailed;
                self.snapshot.reconnecting = false;
            }
            self.snapshot.visible = !self.snapshot.reconnecting
                && !start_requested(self.snapshot.last_event)
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

    fn start_attempt(&mut self) -> bool {
        if !self.start() {
            return false;
        }
        self.retry_at = None;
        #[cfg(any(test, target_os = "macos"))]
        {
            self.startup_attempt = Some(Instant::now());
        }
        true
    }

    /// Claims an automatic start once a failed start has waited out its
    /// backoff. The runtime can fail to answer for a while, for example while
    /// a macOS folder prompt holds it, and it must not stay down until the
    /// user starts it by hand.
    fn retry_due(&mut self, now: Instant) -> bool {
        if self.snapshot.last_event != RuntimeEvent::StartFailed || self.snapshot.busy {
            return false;
        }
        let at = *self
            .retry_at
            .get_or_insert_with(|| now + retry_delay(self.retries));
        if now < at || !self.start_attempt() {
            return false;
        }
        self.retries = self.retries.saturating_add(1);
        self.retrying = true;
        true
    }

    fn started(&mut self, event: RuntimeEvent) {
        if std::mem::take(&mut self.retrying) && start_requested(event) {
            // The failure stays on screen until the clients connect, and the
            // next automatic start waits out a longer backoff.
            self.snapshot.busy = false;
            return;
        }
        self.awaiting_disconnect = event == RuntimeEvent::Stopped;
        self.snapshot.busy = self.awaiting_disconnect;
        self.outage = None;
        // A start that only sent its request waits for the clients, and the
        // observer shows a failure if they never connect.
        let visible = event != RuntimeEvent::Connected && !start_requested(event);
        self.change(event, visible);
    }
}

impl RuntimeOwner {
    /// Reads whether the service is off, and claims the start that follows a
    /// change of the Login Items switch while the desktop is open: the handover
    /// to the service the user turned on while the desktop's child runs, or a
    /// child once the service the user turned off has stopped. A service that
    /// is off is a notice over a child, never a blocking screen.
    #[cfg(any(test, target_os = "macos"))]
    fn poll_approval<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        query: impl FnOnce() -> Option<bool>,
        run_in_progress: impl FnOnce() -> bool,
    ) -> ApprovalStep {
        let waiting = self.state.lock().unwrap().child_until_approval;
        let service_off = query();
        if let Some(off) = service_off {
            self.state.lock().unwrap().service_off = off;
        }
        let mut claimed = false;
        match service_off {
            // launchd starts the service as soon as the user turns it on. That
            // runtime waits while the child serves, and binds once it stops.
            Some(false) if waiting => {
                let busy = run_in_progress();
                self.update(app, |state| claimed = state.claim_handover(busy));
                if claimed {
                    return ApprovalStep::HandOver;
                }
            }
            Some(true) => {
                self.update(app, |state| claimed = state.claim_restart());
                if claimed {
                    return ApprovalStep::Restart;
                }
            }
            // The service is on again before the desktop claimed a child, so
            // the outage waits for launchd and shows as any other outage.
            Some(false) => self.update(app, |state| {
                if state.snapshot.last_event == RuntimeEvent::Disconnected {
                    state.snapshot.reconnecting = false;
                }
            }),
            None => {}
        }
        ApprovalStep::Wait
    }

    fn update<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        update: impl FnOnce(&mut Lifecycle),
    ) {
        let mut state = self.state.lock().unwrap();
        let before = state.snapshot.clone();
        update(&mut state);
        if state.snapshot != before {
            #[cfg(windows)]
            if state.snapshot.last_event != before.last_event {
                crate::subscription_probe::runtime_admission(
                    state.snapshot.last_event == RuntimeEvent::Connected,
                    state.snapshot.last_event == RuntimeEvent::StartFailed,
                );
            }
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

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(crate) fn keep_child(&self, child: RuntimeChild) {
        *self.child.lock().unwrap() = Some(child);
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn child_running(&self) -> bool {
        self.child
            .lock()
            .unwrap()
            .as_mut()
            .is_some_and(|process| matches!(process.try_wait(), Ok(None)))
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(crate) fn stop_child(&self) -> Result<bool, ()> {
        let mut child = self.child.lock().unwrap();
        let Some(process) = child.as_mut() else {
            return Ok(false);
        };
        // The runtime removes its socket on a termination signal. Kill is the fallback.
        #[cfg(target_os = "macos")]
        terminate(process);
        process
            .kill()
            .and_then(|()| process.wait())
            .map_err(|_| ())?;
        *child = None;
        Ok(true)
    }
}

#[cfg(target_os = "macos")]
fn terminate(process: &mut std::process::Child) {
    let Ok(pid) = libc::pid_t::try_from(process.id()) else {
        return;
    };
    // SAFETY: kill sends one signal to the child the desktop spawned and still holds.
    if unsafe { libc::kill(pid, libc::SIGTERM) } != 0 {
        return;
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if !matches!(process.try_wait(), Ok(None)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
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
    owner.update(app, |state| claimed = state.start_attempt());
    if claimed {
        finish_start(app);
    }
}

fn finish_start<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let owner = app.state::<RuntimeOwner>();
    #[cfg(all(target_os = "macos", feature = "local-runtime"))]
    let event = crate::macos_runtime_service::start_child(&owner);
    #[cfg(all(target_os = "macos", feature = "local-runtime"))]
    let event = (event, Child::None);
    #[cfg(all(target_os = "macos", not(feature = "local-runtime")))]
    let event = start_with_fallback(
        || crate::macos_runtime_service::start(&owner),
        || crate::macos_runtime_service::start_child(&owner),
    );
    #[cfg(target_os = "windows")]
    {
        // The runner owns the disposable runtime, including recovery after MSI file replacement.
        // Task Scheduler does not inherit the probe profile or its process job.
        let result = if crate::subscription_probe::runtime_managed_by_runner() {
            crate::subscription_probe::runtime_admission(false, false);
            Ok(())
        } else {
            muniment_runtime::profile_directory()
                .map_err(|error| format!("Runtime state lookup failed: {error}"))
                .and_then(|directory| {
                    crate::windows_runtime_service::register_runtime_task_at_startup(&directory)?;
                    crate::windows_runtime_service::start_runtime_task_at_startup(&directory)
                })
        };
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
    owner.update(app, |state| state.service_started(event.0, event.1));
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
        let result = match owner.stop_child() {
            Ok(true) => Ok(()),
            Ok(false) => crate::macos_runtime_service::stop(),
            Err(()) => Err(()),
        };
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
    // Capture the attempt before the supervisors can complete an admission.
    app.state::<RuntimeOwner>().update(app, |state| {
        state.start_attempt();
    });
    #[cfg(target_os = "linux")]
    crate::attach_service::start_runtime_clients(app);
    #[cfg(target_os = "windows")]
    {
        crate::attach_service::start_approval_presenter(app);
        crate::attach_service::start_desktop_client(app);
    }
    #[cfg(target_os = "macos")]
    crate::attach_service::start_desktop_client(app);
    crate::runtimes::setup(app);
    let start_app = app.clone();
    std::thread::spawn(move || finish_start(&start_app));
    let app = app.clone();
    #[cfg(target_os = "macos")]
    let endpoint = muniment_runtime::profile_directory()
        .ok()
        .map(|profile| profile.join("muniment/attach-v1.sock"));
    #[cfg(target_os = "macos")]
    {
        let approval_app = app.clone();
        // Native service queries can block. They must not delay client observations.
        std::thread::spawn(move || loop {
            let owner = approval_app.state::<RuntimeOwner>();
            match owner.poll_approval(
                &approval_app,
                crate::macos_runtime_service::service_off,
                run_in_progress,
            ) {
                ApprovalStep::HandOver => hand_over_to_service(&approval_app),
                ApprovalStep::Restart => finish_start(&approval_app),
                ApprovalStep::Wait => {}
            }
            std::thread::sleep(Duration::from_secs(1));
        });
    }
    std::thread::spawn(move || loop {
        let owner = app.state::<RuntimeOwner>();
        let revision = owner.state.lock().unwrap().snapshot.revision;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
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
        #[cfg(target_os = "windows")]
        let connected = companion.runtime_connected();
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let (desktop, chat_events) = companion.runtime_client_connections();
        #[cfg(target_os = "windows")]
        let disconnected = !connected;
        owner.update(&app, |state| {
            if state.snapshot.revision != revision {
                return;
            }
            #[cfg(target_os = "linux")]
            state.observe_clients(desktop, chat_events, Instant::now());
            #[cfg(target_os = "macos")]
            state.observe_endpoint_clients(
                (desktop, chat_events),
                Instant::now(),
                endpoint.as_deref(),
                companion.runtime_admission_failure_since(
                    state.startup_attempt.unwrap_or_else(Instant::now),
                ),
                |since| {
                    endpoint.as_deref().map_or_else(Vec::new, |endpoint| {
                        muniment_core::attach::macos_admissions_since(endpoint, since)
                    })
                },
            );
            #[cfg(target_os = "windows")]
            state.observe(connected, disconnected, Instant::now());
        });
        let mut retry = false;
        owner.update(&app, |state| retry = state.retry_due(Instant::now()));
        if retry {
            let retry_app = app.clone();
            std::thread::spawn(move || finish_start(&retry_app));
        }
        std::thread::sleep(Duration::from_millis(100));
    });
}

/// Whether an agent run is in progress, from core's agent state. A state that
/// cannot be read counts as a run, so the handover waits.
#[cfg(target_os = "macos")]
fn run_in_progress() -> bool {
    muniment_runtime::profile_directory()
        .ok()
        .and_then(|profile| muniment_core::agents::state(&profile).ok())
        .is_none_or(|state| runs_active(&state))
}

/// Whether an agent run is running or queued.
#[cfg(any(test, target_os = "macos"))]
fn runs_active(state: &muniment_core::agents::State) -> bool {
    state
        .runs
        .values()
        .any(|run| matches!(run.status.as_str(), "running" | "queued"))
}

/// Stops the desktop's child and starts the runtime through the service the
/// user turned on, after the owner claimed the handover. The child removes its
/// endpoint as it stops, and the service instance that launchd started binds it.
#[cfg(target_os = "macos")]
fn hand_over_to_service<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let owner = app.state::<RuntimeOwner>();
    if owner.stop_child().is_err() {
        owner.update(app, Lifecycle::handover_failed);
        return;
    }
    if let Ok(home) = muniment_core::user_diagnostics::effective_user_home() {
        crate::macos_runtime_service::record_handover(&home.join("Library/Logs/Muniment"));
    }
    finish_start(app);
}

/// How far ahead a routine counts as due when the desktop quits, in seconds.
#[cfg(any(test, target_os = "macos"))]
const QUIT_ROUTINE_WINDOW: i64 = 60 * 60;

/// The menu item that quits through [`quit`] in place of the system's Quit.
#[cfg(target_os = "macos")]
pub(crate) const QUIT_MENU_ID: &str = "muniment-quit";

/// The line a quit confirmation shows when quitting stops agent work: a run in
/// progress, or a routine due within the hour, from core's agent state.
#[cfg(any(test, target_os = "macos"))]
fn quit_warning(state: &muniment_core::agents::State, now: i64) -> Option<&'static str> {
    if runs_active(state) {
        return Some("An agent is running. Quitting Muniment stops it.");
    }
    state
        .routines
        .values()
        .flat_map(|routines| routines.values())
        .filter_map(|routine| routine.next_fire_at)
        .any(|at| (now..=now.saturating_add(QUIT_ROUTINE_WINDOW)).contains(&at))
        .then_some("A scheduled agent is due within the hour. It runs only while Muniment is open.")
}

/// Puts [`QUIT_MENU_ID`] in place of the app menu's system Quit, which ends
/// the app without a request the desktop could answer.
#[cfg(target_os = "macos")]
pub(crate) fn replace_quit_menu_item<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> tauri::Result<()> {
    use tauri::menu::{MenuItem, MenuItemKind};
    let Some(menu) = app.menu() else {
        return Ok(());
    };
    let Some(MenuItemKind::Submenu(app_menu)) = menu.items()?.into_iter().next() else {
        return Ok(());
    };
    let Some(MenuItemKind::Predefined(quit)) = app_menu.items()?.into_iter().last() else {
        return Ok(());
    };
    let text = quit.text()?;
    if !text.starts_with("Quit") {
        return Ok(());
    }
    app_menu.remove(&quit)?;
    app_menu.append(&MenuItem::with_id(
        app,
        QUIT_MENU_ID,
        text,
        true,
        Some("CmdOrCtrl+Q"),
    )?)
}

/// Quits the app, and first asks when the runtime is the desktop's child and
/// quitting would stop a run or a routine due soon.
#[cfg(target_os = "macos")]
pub(crate) fn quit<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    let warning = app
        .state::<RuntimeOwner>()
        .child_running()
        .then(muniment_runtime::profile_directory)
        .and_then(Result::ok)
        .and_then(|profile| muniment_core::agents::state(&profile).ok())
        .and_then(|state| quit_warning(&state, muniment_core::agents::now()));
    let Some(warning) = warning else {
        app.exit(0);
        return;
    };
    let quitting = app.clone();
    // The first button is the default, so Enter keeps the app open.
    app.dialog()
        .message(warning)
        .title("Quit Muniment?")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Keep open".into(),
            "Quit".into(),
        ))
        .show(move |keep_open| {
            if !keep_open {
                quitting.exit(0);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Listener;

    #[test]
    fn endpoint_timeout_names_the_path_and_clears_on_recovery_or_retry() {
        let now = Instant::now();
        let endpoint = std::path::Path::new(
            "/Users/test home/.local/share/ai.muniment.desktop/muniment/attach-v1.sock",
        );
        for endpoint in [Some(endpoint), None] {
            let app = tauri::test::mock_app();
            app.manage(RuntimeOwner::default());
            let owner = app.state::<RuntimeOwner>();
            for elapsed in [0, 9999, 10000, 11000] {
                owner.update(app.handle(), |state| {
                    state.observe_endpoint(false, now + Duration::from_millis(elapsed), endpoint);
                });
                let snapshot = runtime_state(app.state());
                if elapsed < 10000 {
                    assert_eq!(snapshot.last_event, RuntimeEvent::Starting);
                    assert!(!snapshot.visible);
                    assert_eq!(snapshot.cause, None);
                } else {
                    assert_eq!(snapshot.last_event, RuntimeEvent::StartFailed);
                    assert!(snapshot.visible);
                    let detail = match endpoint {
                        Some(path) => format!(
                            "The desktop waited for runtime endpoint {}.",
                            path.display()
                        ),
                        None => "The desktop could not resolve the runtime endpoint.".to_owned(),
                    };
                    assert_eq!(
                        snapshot.cause,
                        Some(format!(
                            "The runtime clients did not connect within 10 seconds. {detail} Startup elapsed: 10000 ms. Startup bound: 10000 ms."
                        ))
                    );
                    assert_eq!(snapshot.revision, 1);
                }
            }
            owner.update(app.handle(), |state| {
                state.observe_endpoint(true, now + Duration::from_secs(12), endpoint);
            });
            assert_eq!(runtime_state(app.state()).cause, None);
            assert!(!runtime_state(app.state()).visible);
            owner.update(app.handle(), |state| {
                assert!(state.start());
                state.started(RuntimeEvent::Starting);
                state.observe_endpoint(false, now, endpoint);
                state.observe_endpoint(false, now + Duration::from_secs(10), endpoint);
                assert!(state.snapshot.cause.is_some());
                assert!(state.start());
                state.observe_endpoint(false, now + Duration::from_secs(11), endpoint);
                assert!(state.snapshot.cause.is_some());
                state.started(RuntimeEvent::Starting);
            });
            assert_eq!(runtime_state(app.state()).cause, None);
        }
    }

    #[test]
    fn endpoint_timeout_keeps_the_connect_failure_and_clears_it_on_recovery() {
        let now = Instant::now();
        let endpoint_path =
            std::path::PathBuf::from(format!("/tmp/m1876-start-{}.sock", std::process::id()));
        let endpoint = endpoint_path.as_path();
        #[cfg(target_os = "macos")]
        let failure = {
            use muniment_attach::{
                serve_desktop_client_at, DesktopClientHolder, DesktopClientStopHandle,
            };
            let stop = DesktopClientStopHandle::new();
            let holder = DesktopClientHolder::new();
            let worker_stop = stop.clone();
            let worker_holder = holder.clone();
            let worker_endpoint = endpoint_path.clone();
            let worker = std::thread::spawn(move || {
                serve_desktop_client_at(
                    &worker_endpoint,
                    "1.0.0",
                    Duration::from_millis(50),
                    Duration::from_millis(1),
                    worker_stop,
                    worker_holder,
                    |_| panic!("the unavailable endpoint cannot connect"),
                )
            });
            let deadline = Instant::now() + Duration::from_secs(2);
            let failure = loop {
                if let Some(failure) = holder.admission_failure_since(now) {
                    break Some(failure);
                }
                if Instant::now() >= deadline {
                    break None;
                }
                std::thread::sleep(Duration::from_millis(1));
            };
            stop.stop();
            worker.join().unwrap();
            let failure = failure.expect("the supervisor must report its connect failure");
            assert_eq!(failure["error"]["kind"], "NotFound");
            assert_eq!(failure["code_check"], "not_run");
            failure
        };
        #[cfg(not(target_os = "macos"))]
        let failure = serde_json::json!({
            "event": "macos_attach_admission", "phase": "connect",
            "endpoint": endpoint, "requested_route": "desktop-client",
            "error": {"kind": "NotFound", "os_error": 2},
            "code_check": "not_run", "closed_by": "not_connected",
            "connect_elapsed_ms": 1, "connect_bound": "supervisor_stop",
        });
        let mut state = Lifecycle::default();
        state.observe_endpoint_with_failure(false, now, Some(endpoint), None);
        state.observe_endpoint_with_failure(
            false,
            now + Duration::from_secs(10),
            Some(endpoint),
            Some(failure.clone()),
        );
        let cause = state.snapshot.cause.clone().unwrap();
        assert_eq!(state.snapshot.last_event, RuntimeEvent::StartFailed);
        assert!(cause.contains("Startup elapsed: 10000 ms."));
        assert!(cause.contains("Startup bound: 10000 ms."));
        assert!(cause.contains(&format!("Attach failure: {failure}.")));
        state.observe_endpoint_with_failure(
            false,
            now + Duration::from_secs(11),
            Some(endpoint),
            None,
        );
        assert_eq!(state.snapshot.cause.as_deref(), Some(cause.as_str()));
        state.observe_endpoint_with_failure(
            true,
            now + Duration::from_secs(12),
            Some(endpoint),
            Some(failure.clone()),
        );
        assert_eq!(state.snapshot.cause, None);
        state.started(RuntimeEvent::Starting);
        state.observe_endpoint_with_failure(
            false,
            now + Duration::from_secs(13),
            Some(endpoint),
            None,
        );
        state.observe_endpoint_with_failure(
            true,
            now + Duration::from_secs(23),
            Some(endpoint),
            Some(failure),
        );
        assert_eq!(state.snapshot.last_event, RuntimeEvent::Connected);
        assert_eq!(state.snapshot.cause, None);
    }

    #[test]
    fn a_service_turned_off_in_login_items_runs_a_child_behind_a_non_blocking_notice() {
        let app = tauri::test::mock_app();
        app.manage(RuntimeOwner::default());
        let owner = app.state::<RuntimeOwner>();
        let (event, child) = start_with_fallback(
            || RuntimeEvent::RequiresApproval,
            || RuntimeEvent::ChildStarted,
        );
        assert_eq!((event, child), (RuntimeEvent::ChildStarted, Child::LoginItemsOff));
        owner.update(app.handle(), |state| state.service_started(event, child));
        let now = Instant::now();
        // Approval queries keep reporting the switch as off. Neither the
        // starting child nor the connected one turns into the approval screen.
        assert_eq!(
            owner.poll_approval(app.handle(), || Some(true), || false),
            ApprovalStep::Wait
        );
        assert_eq!(
            runtime_state(app.state()).last_event,
            RuntimeEvent::ChildStarted
        );
        owner.update(app.handle(), |state| {
            state.observe_endpoint_clients((true, true), now, None, None, |_| vec![]);
        });
        let connected = runtime_state(app.state());
        assert_eq!(connected.last_event, RuntimeEvent::Connected);
        assert!(!connected.visible);
        assert!(connected.foreground_only);
        assert_eq!(
            owner.poll_approval(app.handle(), || Some(true), || false),
            ApprovalStep::Wait
        );
        assert_eq!(runtime_state(app.state()), connected);
        // A child that exits shows the outage, not the approval screen.
        owner.update(app.handle(), |state| {
            state.observe_endpoint_clients((false, false), now, None, None, |_| vec![]);
        });
        assert_eq!(
            owner.poll_approval(app.handle(), || Some(true), || false),
            ApprovalStep::Wait
        );
        let outage = runtime_state(app.state());
        assert_eq!(outage.last_event, RuntimeEvent::Disconnected);
        assert!(outage.foreground_only);
        assert_eq!(
            serde_json::to_value(&outage).unwrap()["foregroundOnly"],
            true
        );
    }

    #[test]
    fn turning_the_service_on_hands_the_child_over_once_no_run_is_in_progress() {
        let app = tauri::test::mock_app();
        app.manage(RuntimeOwner::default());
        let owner = app.state::<RuntimeOwner>();
        let (event, child) = start_with_fallback(
            || RuntimeEvent::RequiresApproval,
            || RuntimeEvent::ChildStarted,
        );
        owner.update(app.handle(), |state| state.service_started(event, child));
        owner.update(app.handle(), |state| {
            state.observe_endpoint_clients((true, true), Instant::now(), None, None, |_| vec![]);
        });
        let connected = runtime_state(app.state());
        // An unreadable status or a switch still off keeps the child.
        let unasked = std::cell::Cell::new(true);
        for service_off in [None, Some(true)] {
            assert_eq!(
                owner.poll_approval(
                    app.handle(),
                    || service_off,
                    || {
                        unasked.set(false);
                        false
                    }
                ),
                ApprovalStep::Wait
            );
        }
        assert!(unasked.get());
        // A run in progress keeps the child until the run ends.
        assert_eq!(
            owner.poll_approval(app.handle(), || Some(false), || true),
            ApprovalStep::Wait
        );
        assert_eq!(runtime_state(app.state()), connected);
        // With no run, the owner claims the start, and only once.
        assert_eq!(
            owner.poll_approval(app.handle(), || Some(false), || false),
            ApprovalStep::HandOver
        );
        assert!(runtime_state(app.state()).busy);
        assert_eq!(
            owner.poll_approval(app.handle(), || Some(false), || false),
            ApprovalStep::Wait
        );
        // A child that could not stop stays, and the next poll claims again.
        owner.update(app.handle(), Lifecycle::handover_failed);
        assert!(!runtime_state(app.state()).busy);
        assert_eq!(
            owner.poll_approval(app.handle(), || Some(false), || false),
            ApprovalStep::HandOver
        );
        // The service takes the start, and the runtime no longer stops with the desktop.
        let (event, child) = start_with_fallback(
            || RuntimeEvent::Starting,
            || panic!("A service the user turned on must not start a child."),
        );
        owner.update(app.handle(), |state| state.service_started(event, child));
        let handed_over = runtime_state(app.state());
        assert_eq!(handed_over.last_event, RuntimeEvent::Starting);
        assert!(!handed_over.busy);
        assert!(!handed_over.foreground_only);
        assert_eq!(
            owner.poll_approval(app.handle(), || Some(false), || false),
            ApprovalStep::Wait
        );
    }

    #[test]
    fn a_child_for_an_unavailable_service_is_never_handed_over() {
        let app = tauri::test::mock_app();
        app.manage(RuntimeOwner::default());
        let owner = app.state::<RuntimeOwner>();
        for service in [RuntimeEvent::ServiceInactive, RuntimeEvent::RegistrationFailed] {
            let (event, child) = start_with_fallback(|| service, || RuntimeEvent::ChildStarted);
            assert_eq!(child, Child::ServiceUnavailable);
            owner.update(app.handle(), |state| state.service_started(event, child));
            // A job launchd cannot keep running may still read as enabled.
            assert_eq!(
            owner.poll_approval(app.handle(), || Some(false), || false),
            ApprovalStep::Wait
        );
            assert!(runtime_state(app.state()).foreground_only);
        }
    }

    #[test]
    fn approval_queries_do_not_replace_a_connected_runtime() {
        let app = tauri::test::mock_app();
        app.manage(RuntimeOwner::default());
        let owner = app.state::<RuntimeOwner>();
        let now = Instant::now();
        owner.update(app.handle(), |state| {
            state.observe_endpoint_clients((true, true), now, None, None, |_| vec![]);
        });
        let connected = runtime_state(app.state());
        for service_off in [Some(true), Some(true), None, Some(false), Some(true), Some(false)] {
            assert_eq!(
                owner.poll_approval(app.handle(), || service_off, || false),
                ApprovalStep::Wait
            );
            assert_eq!(runtime_state(app.state()), connected);
            owner.update(app.handle(), |state| {
                state.observe_endpoint_clients((true, true), now, None, None, |_| vec![]);
            });
            assert_eq!(runtime_state(app.state()), connected);
        }

        // An outage with an enabled service or an unread status waits for
        // launchd and the owner's own retries, and never shows a blocking screen.
        owner.update(app.handle(), |state| {
            state.observe_endpoint_clients((false, false), now, None, None, |_| vec![]);
        });
        let outage = runtime_state(app.state());
        for service_off in [None, Some(false)] {
            assert_eq!(
                owner.poll_approval(app.handle(), || service_off, || false),
                ApprovalStep::Wait
            );
            assert_eq!(runtime_state(app.state()), outage);
        }
        // A start in progress keeps its claim.
        owner.update(app.handle(), |state| {
            assert!(state.start_attempt());
        });
        let busy = runtime_state(app.state());
        assert_eq!(
            owner.poll_approval(app.handle(), || Some(true), || false),
            ApprovalStep::Wait
        );
        assert_eq!(runtime_state(app.state()), busy);
    }

    #[test]
    fn turning_the_service_off_while_open_starts_a_child_behind_the_notice() {
        let app = tauri::test::mock_app();
        app.manage(RuntimeOwner::default());
        let owner = app.state::<RuntimeOwner>();
        let now = Instant::now();
        let (event, child) = start_with_fallback(
            || RuntimeEvent::Starting,
            || panic!("A service that starts must not start a child."),
        );
        owner.update(app.handle(), |state| state.service_started(event, child));
        owner.update(app.handle(), |state| {
            state.observe_endpoint_clients((true, true), now, None, None, |_| vec![]);
        });
        // The switch reads off while the service still serves: nothing changes yet.
        assert_eq!(
            owner.poll_approval(app.handle(), || Some(true), || false),
            ApprovalStep::Wait
        );
        // launchd stops the service. The first status read after the lost
        // connection claims a start, even with an agent run in progress: that
        // run ended with the service.
        owner.update(app.handle(), |state| {
            state.observe_endpoint_clients((false, false), now, None, None, |_| vec![]);
        });
        assert_eq!(
            owner.poll_approval(
                app.handle(),
                || Some(true),
                || panic!("A restart does not wait for the run the service ended.")
            ),
            ApprovalStep::Restart
        );
        assert!(runtime_state(app.state()).busy);
        assert_eq!(
            owner.poll_approval(app.handle(), || Some(true), || false),
            ApprovalStep::Wait
        );
        // The start finds the service off, as the launch does, and runs a child.
        let (event, child) = start_with_fallback(
            || RuntimeEvent::RequiresApproval,
            || RuntimeEvent::ChildStarted,
        );
        owner.update(app.handle(), |state| state.service_started(event, child));
        let started = runtime_state(app.state());
        assert_eq!(started.last_event, RuntimeEvent::ChildStarted);
        assert!(!started.busy);
        assert!(started.foreground_only);
        assert_eq!(
            serde_json::to_value(&started).unwrap()["foregroundOnly"],
            true
        );
        owner.update(app.handle(), |state| {
            state.observe_endpoint_clients((true, true), now, None, None, |_| vec![]);
        });
        let connected = runtime_state(app.state());
        assert_eq!(connected.last_event, RuntimeEvent::Connected);
        assert!(!connected.visible);
        assert!(connected.foreground_only);
        // The child runs until the user turns the service on again.
        assert_eq!(
            owner.poll_approval(app.handle(), || Some(true), || false),
            ApprovalStep::Wait
        );
        assert_eq!(runtime_state(app.state()), connected);
        assert_eq!(
            owner.poll_approval(app.handle(), || Some(false), || false),
            ApprovalStep::HandOver
        );
    }

    #[test]
    fn the_requires_approval_restart_never_shows_a_blocking_screen() {
        let app = tauri::test::mock_app();
        app.manage(RuntimeOwner::default());
        let owner = app.state::<RuntimeOwner>();
        let now = Instant::now();
        let seen = std::sync::Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
        let record = seen.clone();
        app.listen_any("runtime-state-changed", move |event| {
            record
                .lock()
                .unwrap()
                .push(serde_json::from_str(event.payload()).unwrap());
        });
        owner.update(app.handle(), |state| {
            state.service_started(RuntimeEvent::Starting, Child::None);
            state.observe_endpoint_clients((true, true), now, None, None, |_| vec![]);
        });
        seen.lock().unwrap().clear();
        // The switch reads off while the service still serves, then launchd stops it.
        assert_eq!(
            owner.poll_approval(app.handle(), || Some(true), || false),
            ApprovalStep::Wait
        );
        for elapsed in [0, 500, 2400, 2800] {
            owner.update(app.handle(), |state| {
                state.observe_endpoint_clients(
                    (false, false),
                    now + Duration::from_millis(elapsed),
                    None,
                    None,
                    |_| vec![],
                );
            });
        }
        let lost = runtime_state(app.state());
        assert_eq!(lost.last_event, RuntimeEvent::Disconnected);
        assert!(!lost.visible && lost.reconnecting);
        assert_eq!(
            owner.poll_approval(app.handle(), || Some(true), || false),
            ApprovalStep::Restart
        );
        let claimed = runtime_state(app.state());
        assert!(claimed.reconnecting && claimed.foreground_only && !claimed.visible);
        let (event, child) = start_with_fallback(
            || RuntimeEvent::RequiresApproval,
            || RuntimeEvent::ChildStarted,
        );
        owner.update(app.handle(), |state| state.service_started(event, child));
        // The child takes a few seconds to bind. The workspace stays with the
        // Login Items notice and the reconnect state, under the start bound.
        let child_at = now + Duration::from_secs(3);
        for elapsed in [0, 400, 2000, 9000] {
            owner.update(app.handle(), |state| {
                state.observe_endpoint_clients(
                    (false, false),
                    child_at + Duration::from_millis(elapsed),
                    None,
                    None,
                    |_| vec![],
                );
            });
        }
        let waiting = runtime_state(app.state());
        assert_eq!(waiting.last_event, RuntimeEvent::ChildStarted);
        assert!(waiting.reconnecting && waiting.foreground_only && !waiting.visible);
        owner.update(app.handle(), |state| {
            state.observe_endpoint_clients((true, true), child_at, None, None, |_| vec![]);
        });
        let connected = runtime_state(app.state());
        assert_eq!(connected.last_event, RuntimeEvent::Connected);
        assert!(!connected.reconnecting && connected.foreground_only);
        let seen = seen.lock().unwrap();
        assert!(seen.len() >= 4);
        assert!(seen.iter().all(|snapshot| snapshot["visible"] == false));
        assert!(seen
            .iter()
            .all(|snapshot| snapshot["lastEvent"] == "connected" || snapshot["reconnecting"] == true));
    }

    #[test]
    fn a_reconnecting_child_that_never_binds_shows_the_failure_and_an_enabled_service_the_outage() {
        let now = Instant::now();
        let mut state = Lifecycle::default();
        state.observe(true, false, now);
        state.service_off = true;
        state.observe(false, true, now);
        assert!(state.claim_restart());
        state.service_started(RuntimeEvent::ChildStarted, Child::LoginItemsOff);
        state.observe(false, true, now);
        state.observe(false, true, now + Duration::from_secs(9));
        assert!(!state.snapshot.visible);
        state.observe(false, true, now + Duration::from_secs(10));
        assert_eq!(state.snapshot.last_event, RuntimeEvent::StartFailed);
        assert!(state.snapshot.visible && !state.snapshot.reconnecting);

        // The switch turns on again before the claim: the outage shows after its dwell.
        let app = tauri::test::mock_app();
        app.manage(RuntimeOwner::default());
        let owner = app.state::<RuntimeOwner>();
        owner.update(app.handle(), |state| state.observe(true, false, now));
        owner.poll_approval(app.handle(), || Some(true), || false);
        owner.update(app.handle(), |state| state.observe(false, true, now));
        assert!(runtime_state(app.state()).reconnecting);
        assert_eq!(
            owner.poll_approval(app.handle(), || Some(false), || false),
            ApprovalStep::Wait
        );
        owner.update(app.handle(), |state| state.observe(false, true, now + DWELL));
        let outage = runtime_state(app.state());
        assert!(outage.visible && !outage.reconnecting);
    }

    #[test]
    fn a_stop_or_a_failed_child_start_is_not_restarted_by_the_approval_watch() {
        let app = tauri::test::mock_app();
        app.manage(RuntimeOwner::default());
        let owner = app.state::<RuntimeOwner>();
        let now = Instant::now();
        owner.update(app.handle(), |state| {
            state.observe_endpoint_clients((true, true), now, None, None, |_| vec![]);
            assert!(state.start());
            state.started(RuntimeEvent::Stopped);
            state.observe_endpoint_clients((false, false), now, None, None, |_| vec![]);
        });
        // The stop unregisters the service, which then reads as off.
        assert_eq!(runtime_state(app.state()).last_event, RuntimeEvent::Stopped);
        assert_eq!(
            owner.poll_approval(app.handle(), || Some(true), || false),
            ApprovalStep::Wait
        );
        owner.update(app.handle(), |state| {
            assert!(state.start_attempt());
            state.service_started(RuntimeEvent::Starting, Child::None);
            state.observe_endpoint_clients((true, true), now, None, None, |_| vec![]);
            state.observe_endpoint_clients((false, false), now, None, None, |_| vec![]);
        });
        assert_eq!(
            owner.poll_approval(app.handle(), || Some(true), || false),
            ApprovalStep::Restart
        );
        let (event, child) = start_with_fallback(
            || RuntimeEvent::RequiresApproval,
            || RuntimeEvent::RegistrationFailed,
        );
        owner.update(app.handle(), |state| state.service_started(event, child));
        assert_eq!(
            runtime_state(app.state()).last_event,
            RuntimeEvent::RegistrationFailed
        );
        assert_eq!(
            owner.poll_approval(app.handle(), || Some(true), || false),
            ApprovalStep::Wait
        );
    }

    #[test]
    fn subscription_timeout_keeps_admissions_from_busy_startup_but_not_a_previous_attempt() {
        let mut state = Lifecycle::default();
        assert!(state.start_attempt());
        let attempt = state.startup_attempt.unwrap();
        let admitted = Instant::now();
        let history = [
            (
                attempt - Duration::from_secs(1),
                serde_json::json!({"elapsed_ms": 999}),
            ),
            (
                admitted,
                serde_json::json!({"requested_route": "desktop-client", "elapsed_ms": 1641}),
            ),
        ];
        state.observe_endpoint_clients((true, false), admitted, None, None, |_| {
            panic!("A busy startup cannot time out.")
        });
        assert!(state.snapshot.busy);
        assert_eq!(state.outage, None);
        assert!(!state.start_attempt());
        assert_eq!(state.startup_attempt, Some(attempt));
        state.started(RuntimeEvent::Starting);
        let observing = admitted + Duration::from_secs(2);
        state.observe_endpoint_clients((true, false), observing, None, None, |_| vec![]);
        state.observe_endpoint_clients(
            (true, false),
            observing + Duration::from_secs(10),
            None,
            None,
            |since| {
                assert_eq!(since, attempt);
                history
                    .iter()
                    .filter(|(at, _)| *at >= since)
                    .map(|(_, value)| value.clone())
                    .collect()
            },
        );
        assert_eq!(state.snapshot.last_event, RuntimeEvent::StartFailed);
        let cause = state.snapshot.cause.as_deref().unwrap();
        assert!(cause.contains("Desktop client connected: true. Chat events connected: false."));
        assert!(cause.contains("\"elapsed_ms\":1641"));
        assert!(!cause.contains("999"));
        assert!(cause.contains("Startup elapsed: 10000 ms. Startup bound: 10000 ms."));

        assert!(state.start_attempt());
        let retry = state.startup_attempt.unwrap();
        assert!(retry > admitted);
        state.started(RuntimeEvent::Starting);
        state.observe_endpoint_clients((true, false), retry, None, None, |_| vec![]);
        state.observe_endpoint_clients(
            (true, false),
            retry + Duration::from_secs(10),
            None,
            None,
            |since| {
                assert_eq!(since, retry);
                history
                    .iter()
                    .filter(|(at, _)| *at >= since)
                    .map(|(_, value)| value.clone())
                    .collect()
            },
        );
        assert_eq!(state.snapshot.last_event, RuntimeEvent::StartFailed);
        assert!(state
            .snapshot
            .cause
            .as_deref()
            .unwrap()
            .contains("Admissions: []."));
    }

    #[test]
    fn late_subscription_recovers_while_a_native_status_query_is_pending() {
        use std::sync::mpsc;

        let app = tauri::test::mock_app();
        app.manage(RuntimeOwner::default());
        let owner = app.state::<RuntimeOwner>();
        let now = Instant::now();
        owner.update(app.handle(), |state| {
            state.observe_endpoint_clients((true, false), now, None, None, |_| vec![]);
        });
        let (entered, entered_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let query_app = app.handle().clone();
        let query = std::thread::spawn(move || {
            query_app
                .state::<RuntimeOwner>()
                .poll_approval(
                &query_app,
                || {
                    entered.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                    Some(true)
                },
                || false,
            );
        });
        entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        owner.update(app.handle(), |state| {
            state.observe_endpoint_clients(
                (true, false),
                now + Duration::from_secs(10),
                None,
                None,
                |_| vec![serde_json::json!({"requested_route": "desktop-client", "elapsed_ms": 1641})],
            );
        });
        let failed = runtime_state(app.state());
        assert_eq!(failed.last_event, RuntimeEvent::StartFailed);
        let cause = failed.cause.unwrap();
        assert!(cause.contains("Desktop client connected: true. Chat events connected: false."));
        assert!(cause.contains("\"elapsed_ms\":1641"));
        owner.update(app.handle(), |state| {
            state.observe_endpoint_clients(
                (true, true),
                now + Duration::from_secs(11),
                None,
                None,
                |_| vec![],
            );
        });
        assert_eq!(
            runtime_state(app.state()).last_event,
            RuntimeEvent::Connected
        );
        assert_eq!(runtime_state(app.state()).cause, None);
        assert!(!runtime_state(app.state()).visible);
        release.send(()).unwrap();
        query.join().unwrap();
        assert_eq!(
            runtime_state(app.state()).last_event,
            RuntimeEvent::Connected
        );
        owner.update(app.handle(), |state| {
            state.observe_endpoint_clients(
                (true, false),
                now + Duration::from_secs(12),
                None,
                None,
                |_| vec![],
            );
            state.observe_endpoint_clients(
                (false, false),
                now + Duration::from_secs(14),
                None,
                None,
                |_| vec![],
            );
        });
        // The status read found the service off, so the lost connection is
        // the reconnect to a child, not a blocking outage.
        let disconnected = runtime_state(app.state());
        assert_eq!(disconnected.last_event, RuntimeEvent::Disconnected);
        assert!(!disconnected.visible && disconnected.reconnecting);
    }

    #[test]
    fn endpoint_clients_require_both_connections_and_keep_the_startup_bound() {
        let now = Instant::now();
        for missing in [(true, false), (false, true), (false, false)] {
            let mut state = Lifecycle::default();
            state.observe_endpoint_clients(missing, now, None, None, |_| vec![]);
            state.observe_endpoint_clients(
                missing,
                now + Duration::from_millis(9999),
                None,
                None,
                |_| vec![],
            );
            assert_eq!(state.snapshot.last_event, RuntimeEvent::Starting);
            state.observe_endpoint_clients(
                missing,
                now + Duration::from_secs(10),
                None,
                None,
                |_| vec![],
            );
            assert_eq!(state.snapshot.last_event, RuntimeEvent::StartFailed);
            state.observe_endpoint_clients(
                missing,
                now + Duration::from_secs(20),
                None,
                None,
                |_| vec![],
            );
            assert_eq!(state.snapshot.last_event, RuntimeEvent::StartFailed);
            state.observe_endpoint_clients(
                (true, true),
                now + Duration::from_secs(21),
                None,
                None,
                |_| vec![],
            );
            assert_eq!(state.snapshot.last_event, RuntimeEvent::Connected);
        }
        let mut state = Lifecycle::default();
        state.observe_endpoint_clients((true, false), now, None, None, |_| vec![]);
        state.observe_endpoint_clients(
            (true, true),
            now + Duration::from_secs(10),
            None,
            None,
            |_| vec![],
        );
        assert_eq!(state.snapshot.last_event, RuntimeEvent::Connected);
        state.started(RuntimeEvent::Stopped);
        for partial in [(true, false), (false, true)] {
            state.observe_endpoint_clients(partial, now, None, None, |_| vec![]);
            assert!(state.snapshot.busy);
        }
        state.observe_endpoint_clients((false, false), now, None, None, |_| vec![]);
        assert!(!state.snapshot.busy);
        assert_eq!(state.snapshot.last_event, RuntimeEvent::Stopped);
    }

    #[test]
    fn endpoint_timeout_reports_the_elapsed_time_after_a_delayed_observation() {
        let now = Instant::now();
        let mut state = Lifecycle::default();
        state.observe_endpoint(false, now, None);
        state.observe_endpoint(false, now + Duration::from_secs(13), None);
        let cause = state.snapshot.cause.unwrap();
        assert!(cause.contains("Startup elapsed: 13000 ms."));
        assert!(cause.contains("Startup bound: 10000 ms."));
    }

    #[test]
    fn endpoint_connection_at_the_timeout_boundary_has_no_failure() {
        let now = Instant::now();
        let mut state = Lifecycle::default();
        state.observe_endpoint(false, now, None);
        state.observe_endpoint(true, now + Duration::from_secs(10), None);
        assert_eq!(state.snapshot.last_event, RuntimeEvent::Connected);
        assert_eq!(state.snapshot.cause, None);
        assert!(!state.snapshot.visible);
    }

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
    fn a_failed_start_retries_with_backoff_until_the_clients_connect() {
        let now = Instant::now();
        let cause = "The runtime clients did not connect within 10 seconds.";
        let mut state = Lifecycle::default();
        assert!(state.start());
        state.windows_start_finished(Err(cause.into()));
        let failure = state.snapshot.clone();
        assert_eq!(failure.last_event, RuntimeEvent::StartFailed);
        let mut at = now;
        for seconds in [2, 4, 8, 16, 32, 60, 60] {
            let delay = Duration::from_secs(seconds);
            assert!(!state.retry_due(at));
            assert!(!state.retry_due(at + delay - Duration::from_millis(1)));
            assert!(state.retry_due(at + delay));
            assert!(state.snapshot.busy);
            assert!(!state.retry_due(at + delay + RETRY_LIMIT));
            // The injected start sends its request, and the clients stay away.
            state.windows_start_finished(Ok(()));
            assert!(!state.snapshot.busy);
            assert_eq!(state.snapshot.last_event, RuntimeEvent::StartFailed);
            assert!(state.snapshot.visible);
            assert_eq!(state.snapshot.cause, failure.cause);
            at += delay;
            state.observe(false, true, at);
            assert_eq!(state.snapshot.cause, failure.cause);
        }
        // An injected start that fails outright waits out the next backoff too.
        assert!(!state.retry_due(at));
        assert!(state.retry_due(at + RETRY_LIMIT));
        state.windows_start_finished(Err(cause.into()));
        assert_eq!(state.snapshot.last_event, RuntimeEvent::StartFailed);
        assert!(!state.snapshot.busy);
        at += RETRY_LIMIT;
        assert!(!state.retry_due(at));
        assert!(!state.retry_due(at + RETRY_LIMIT - Duration::from_millis(1)));
        assert!(state.retry_due(at + RETRY_LIMIT));
        state.windows_start_finished(Ok(()));

        state.observe(true, false, at + Duration::from_secs(1));
        assert_eq!(state.snapshot.last_event, RuntimeEvent::Connected);
        assert!(!state.snapshot.visible);
        assert!(!state.retry_due(at + Duration::from_secs(600)));

        // A later failure starts again from the first delay.
        assert!(state.start());
        state.windows_start_finished(Err(cause.into()));
        let later = at + Duration::from_secs(700);
        assert!(!state.retry_due(later));
        assert!(state.retry_due(later + RETRY_FIRST));
    }

    #[test]
    fn a_retry_that_reports_a_state_for_the_user_stops_retrying() {
        let now = Instant::now();
        let mut state = Lifecycle::default();
        state.observe(false, true, now);
        state.observe(false, true, now + Duration::from_secs(10));
        assert_eq!(state.snapshot.last_event, RuntimeEvent::StartFailed);
        let due = now + Duration::from_secs(10);
        assert!(!state.retry_due(due));
        assert!(state.retry_due(due + RETRY_FIRST));
        state.started(RuntimeEvent::RequiresApproval);
        assert_eq!(state.snapshot.last_event, RuntimeEvent::RequiresApproval);
        assert!(state.snapshot.visible);
        assert!(!state.retry_due(due + RETRY_LIMIT + RETRY_LIMIT));
    }

    #[test]
    fn retry_delays_double_up_to_the_limit() {
        assert_eq!(retry_delay(0), RETRY_FIRST);
        assert_eq!(retry_delay(1), RETRY_FIRST * 2);
        assert_eq!(retry_delay(4), Duration::from_secs(32));
        assert_eq!(retry_delay(5), RETRY_LIMIT);
        assert_eq!(retry_delay(u32::MAX), RETRY_LIMIT);
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

    #[test]
    fn an_inactive_service_starts_the_child_and_flags_a_foreground_only_runtime() {
        let child_calls = std::cell::Cell::new(0);
        for service in [
            RuntimeEvent::ServiceInactive,
            RuntimeEvent::RegistrationFailed,
            RuntimeEvent::RequiresApproval,
        ] {
            let started = start_with_fallback(
                || service,
                || {
                    child_calls.set(child_calls.get() + 1);
                    RuntimeEvent::ChildStarted
                },
            );
            assert_eq!(started.0, RuntimeEvent::ChildStarted);
            assert_ne!(started.1, Child::None);
        }
        assert_eq!(child_calls.get(), 3);
        let mut state = Lifecycle::default();
        state.service_started(RuntimeEvent::ChildStarted, Child::ServiceUnavailable);
        state.observe(true, false, Instant::now());
        // The connected workspace stays on screen and the flag outlives the connection.
        assert_eq!(state.snapshot.last_event, RuntimeEvent::Connected);
        assert!(!state.snapshot.visible);
        assert_eq!(
            serde_json::to_value(&state.snapshot).unwrap()["foregroundOnly"],
            true
        );
        // A later start that the service takes clears the flag.
        let (event, child) = start_with_fallback(
            || RuntimeEvent::Starting,
            || panic!("A running service must not start a child."),
        );
        state.service_started(event, child);
        assert!(!state.snapshot.foreground_only);
        // A child that does not start leaves the failure, not the flag.
        assert_eq!(
            start_with_fallback(
                || RuntimeEvent::ServiceInactive,
                || RuntimeEvent::RegistrationFailed
            ),
            (RuntimeEvent::RegistrationFailed, Child::None)
        );
    }

    #[test]
    fn quit_warns_for_a_run_in_progress_or_a_routine_due_within_the_hour() {
        use muniment_core::agents::{RoutineState, RunState, State};
        let now = 1_900_000_000;
        let mut state = State::default();
        assert_eq!(quit_warning(&state, now), None);
        let routine = |at| RoutineState {
            next_fire_at: Some(at),
            ..RoutineState::default()
        };
        state
            .routines
            .entry("agent".into())
            .or_default()
            .insert("later".into(), routine(now + QUIT_ROUTINE_WINDOW + 1));
        // A fire time already past is stale state, not a routine about to run.
        state
            .routines
            .entry("agent".into())
            .or_default()
            .insert("past".into(), routine(now - 1));
        assert_eq!(quit_warning(&state, now), None);
        state
            .routines
            .entry("agent".into())
            .or_default()
            .insert("soon".into(), routine(now + QUIT_ROUTINE_WINDOW));
        assert_eq!(
            quit_warning(&state, now),
            Some("A scheduled agent is due within the hour. It runs only while Muniment is open.")
        );
        state.routines.clear();
        for status in ["completed", "failed", "waiting"] {
            state.runs.insert(
                "agent".into(),
                RunState {
                    status: status.into(),
                    ..RunState::default()
                },
            );
            assert_eq!(quit_warning(&state, now), None);
        }
        for status in ["running", "queued"] {
            state.runs.insert(
                "agent".into(),
                RunState {
                    status: status.into(),
                    ..RunState::default()
                },
            );
            assert_eq!(
                quit_warning(&state, now),
                Some("An agent is running. Quitting Muniment stops it.")
            );
        }
    }
}
