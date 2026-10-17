use super::*;

#[cfg(target_os = "linux")]
pub(crate) fn start_runtime_clients<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if app.try_state::<AttachCompanionState>().is_none() {
        app.manage(AttachCompanionState::default());
    }
    register_approval_event_presenter(app);
    start_approval_presenter(app);
    start_desktop_client(app);
}

#[cfg(target_os = "linux")]
pub(crate) fn runtime_clients_ready<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    let (desktop, chat_events) = app
        .state::<AttachCompanionState>()
        .runtime_client_connections();
    desktop && chat_events
}

#[cfg(target_os = "linux")]
fn start_approval_presenter<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let Some(endpoint) = runtime_profile_endpoint() else {
        eprintln!("approval presenter profile lookup failed");
        return;
    };
    let presenter_app = app.clone();
    app.state::<AttachCompanionState>()
        .start_approval_presenter(move |stop| {
            std::thread::spawn(move || {
                let approvals = presenter_app.state::<AttachApprovalState>().inner().clone();
                serve_approval_presenter_at(
                    &endpoint,
                    env!("CARGO_PKG_VERSION"),
                    Duration::from_secs(5),
                    Duration::from_millis(250),
                    stop,
                    |_| {},
                    move |request| answer_presented_approval(&approvals, request),
                );
            });
        });
}

#[cfg(target_os = "windows")]
pub(crate) fn start_approval_presenter<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    register_approval_event_presenter(app);
    let presenter_app = app.clone();
    app.state::<AttachCompanionState>()
        .start_approval_presenter(move |stop| {
            std::thread::spawn(move || {
                let approvals = presenter_app.state::<AttachApprovalState>().inner().clone();
                serve_windows_approval_presenter(
                    env!("CARGO_PKG_VERSION"),
                    Duration::from_secs(5),
                    Duration::from_millis(250),
                    stop,
                    |_| {},
                    move |request| answer_presented_approval(&approvals, request),
                );
            });
        });
}

#[cfg(target_os = "linux")]
pub(crate) fn runtime_profile_endpoint() -> Option<PathBuf> {
    let Ok(filesystem) = AttachFilesystem::from_environment() else {
        eprintln!("desktop client filesystem lookup failed");
        return None;
    };
    Some(filesystem.endpoint_path().to_owned())
}

#[cfg(target_os = "macos")]
pub(crate) fn runtime_profile_endpoint() -> Option<PathBuf> {
    muniment_runtime::profile_directory()
        .ok()
        .map(|profile| profile.join("muniment/attach-v1.sock"))
}

#[cfg(all(test, target_os = "macos"))]
pub(super) static TEST_PRESENTER_STARTS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(all(test, target_os = "macos"))]
pub(super) static TEST_PRESENTER_WORKERS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(target_os = "windows")]
pub(crate) fn start_desktop_client<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let client_app = app.clone();
    let event_app = app.clone();
    let event_status_app = app.clone();
    app.state::<AttachCompanionState>()
        .start_desktop_supervisors(
            move |stop, holder| {
                std::thread::spawn(move || {
                    let observer_app = client_app.clone();
                    serve_windows_desktop_client(
                        env!("CARGO_PKG_VERSION"),
                        Duration::from_secs(5),
                        Duration::from_millis(250),
                        stop,
                        holder,
                        move |connected| {
                            observe_desktop_client_connection(&observer_app, connected);
                        },
                    );
                })
            },
            move |stop| {
                std::thread::spawn(move || {
                    serve_windows_chat_events(
                        env!("CARGO_PKG_VERSION"),
                        Duration::from_secs(5),
                        Duration::from_millis(250),
                        stop,
                        move |connected| {
                            observe_chat_event_subscription(&event_status_app, connected);
                        },
                        move |event| deliver_local_event(&event_app, event),
                    );
                })
            },
        );
}

/// Hands one event of the chat-event subscription to the window: a chat event
/// as `chat-event`, and a provider sign-in step as `local-mode-login`.
#[cfg(any(unix, target_os = "windows"))]
pub(crate) fn deliver_subscription_event<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    event: SubscriptionEvent,
) {
    match event {
        SubscriptionEvent::Chat(event) => {
            let _ = app.emit("chat-event", event);
        }
        SubscriptionEvent::ModelsLogin(step) => crate::account_login::deliver_step(app, step),
        // Only this computer's runtime sends an update request, on its owner
        // route. The runtimes list takes each runtime's update status.
        SubscriptionEvent::RuntimeUpdateRequested(_)
        | SubscriptionEvent::RuntimeUpdateStatus(_) => {}
        // Each runtime's agent and swarm events reach the window whichever
        // runtime is selected, through `deliver_agent_event`.
        SubscriptionEvent::AgentRunUpdated(_)
        | SubscriptionEvent::SwarmChanged(_)
        | SubscriptionEvent::ApprovalPending(_)
        | SubscriptionEvent::ApprovalResolved(_)
        | SubscriptionEvent::AgentActivity(_)
        | SubscriptionEvent::GroupPosted(_)
        | SubscriptionEvent::FleetSyncChanged(_)
        | SubscriptionEvent::FleetOpUpdated(_) => {}
    }
}

/// Hands one runtime's agent or swarm event to the window whichever runtime
/// is selected: `agent-run-updated` `{runtimeId, run}`, `swarm-changed`
/// `{runtimeId, change}`, `agent-approval-pending` or
/// `agent-approval-resolved` `{runtimeId, approval}`, `agent-activity`
/// `{runtimeId, update}`, `group-posted` `{runtimeId, posted}`,
/// `fleet-sync-changed` `{runtimeId, status}` and `fleet-op-updated`
/// `{runtimeId, op}`. Any other event comes back to the caller.
#[cfg(any(unix, target_os = "windows"))]
pub(crate) fn deliver_agent_event<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    runtime_id: &str,
    event: SubscriptionEvent,
) -> Option<SubscriptionEvent> {
    let (name, field, body) = match event {
        SubscriptionEvent::AgentRunUpdated(run) => ("agent-run-updated", "run", run),
        SubscriptionEvent::SwarmChanged(change) => ("swarm-changed", "change", change),
        SubscriptionEvent::ApprovalPending(approval) => {
            ("agent-approval-pending", "approval", approval)
        }
        SubscriptionEvent::ApprovalResolved(approval) => {
            ("agent-approval-resolved", "approval", approval)
        }
        SubscriptionEvent::AgentActivity(update) => ("agent-activity", "update", update),
        SubscriptionEvent::GroupPosted(posted) => ("group-posted", "posted", posted),
        SubscriptionEvent::FleetSyncChanged(status) => ("fleet-sync-changed", "status", status),
        SubscriptionEvent::FleetOpUpdated(op) => ("fleet-op-updated", "op", op),
        event => return Some(event),
    };
    let _ = app.emit(name, serde_json::json!({"runtimeId": runtime_id, field: body}));
    None
}

/// Hands a local runtime event to the window only while this computer's
/// runtime is selected, since the window shows one runtime's threads at a
/// time. An update request for this computer applies whichever is selected.
#[cfg(any(unix, target_os = "windows"))]
fn deliver_local_event<R: tauri::Runtime>(app: &tauri::AppHandle<R>, event: SubscriptionEvent) {
    if let SubscriptionEvent::RuntimeUpdateRequested(request) = event {
        crate::runtimes::update_requested(app, request);
    } else if let SubscriptionEvent::RuntimeUpdateStatus(status) = event {
        crate::runtimes::local_update_status(app, status);
    } else if let Some(event) = deliver_agent_event(app, "local", event) {
        if !app.state::<AttachCompanionState>().remote_active() {
            deliver_subscription_event(app, event);
        }
    }
}

#[cfg(unix)]
pub(crate) fn start_desktop_client<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let Some(endpoint) = runtime_profile_endpoint() else {
        eprintln!("desktop client profile lookup failed");
        return;
    };
    let client_endpoint = endpoint.clone();
    let client_app = app.clone();
    let event_app = app.clone();
    let event_status_app = app.clone();
    #[cfg(target_os = "macos")]
    let presenter_endpoint = endpoint.clone();
    #[cfg(target_os = "macos")]
    let presenter_app = app.clone();
    if app.try_state::<AttachCompanionState>().is_none() {
        app.manage(AttachCompanionState::default());
    }
    let state = app.state::<AttachCompanionState>();
    let start_supervisors = || {
        state.start_desktop_supervisors(
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
                            observe_desktop_client_connection(&observer_app, connected);
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
                            observe_chat_event_subscription(&event_status_app, connected);
                        },
                        move |event| deliver_local_event(&event_app, event),
                    );
                })
            },
        );
    };
    #[cfg(target_os = "macos")]
    {
        register_approval_event_presenter(app);
        state.start_approval_presenter(move |stop| {
            #[cfg(test)]
            TEST_PRESENTER_STARTS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::thread::spawn(move || {
                #[cfg(test)]
                TEST_PRESENTER_WORKERS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let approvals = presenter_app.state::<AttachApprovalState>().inner().clone();
                eprintln!("desktop approval presenter connected=false");
                serve_approval_presenter_at(
                    &presenter_endpoint,
                    env!("CARGO_PKG_VERSION"),
                    Duration::from_secs(5),
                    Duration::from_millis(250),
                    stop,
                    |connected| eprintln!("desktop approval presenter connected={connected}"),
                    move |request| answer_presented_approval(&approvals, request),
                );
            });
        });
        start_supervisors();
    }
    #[cfg(target_os = "linux")]
    start_supervisors();
    #[cfg(target_os = "linux")]
    {
        let status = app.state::<AttachCompanionState>().listener_status();
        let _ = app.emit("desktop-client-status-changed", status);
    }
}

#[cfg(unix)]
pub(super) fn serve_chat_events_at(
    endpoint: &Path,
    client_version: &str,
    io_timeout: Duration,
    retry_interval: Duration,
    stop: ChatEventStopHandle,
    mut observe: impl FnMut(bool),
    mut deliver: impl FnMut(SubscriptionEvent),
) {
    #[cfg(target_os = "linux")]
    let mut diagnostic = muniment_core::attach::LinuxConnectDiagnostic::default();
    #[cfg(target_os = "macos")]
    let mut diagnostic = muniment_core::attach::MacosConnectDiagnostic::default();
    // Each reconnect resumes after the newest chat event already delivered.
    let mut cursor = None;
    loop {
        #[cfg(target_os = "linux")]
        {
            if let Some(mut client) =
                diagnostic.connect(endpoint, "chat-events", &stop.inner, |stream| {
                    let mut client =
                        handshake_desktop_client_stream(stream, client_version, io_timeout)?;
                    client.subscribe_chat_events_with_fleet(cursor)?;
                    Ok(client)
                })
            {
                observe(true);
                loop {
                    match client.read_subscription_event() {
                        Ok(event) => deliver(event),
                        Err(error) => {
                            eprintln!("muniment-desktop: session closed endpoint={endpoint:?} route=chat-events reason={error:?}");
                            break;
                        }
                    }
                }
                cursor = client.chat_cursor().or(cursor);
                observe(false);
            }
            stop.inner
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .stream = None;
        }
        #[cfg(target_os = "macos")]
        {
            if let Some(mut client) =
                diagnostic.connect(endpoint, "chat-events", &stop.inner, io_timeout, |stream| {
                    let mut client =
                        handshake_desktop_client_stream(stream, client_version, io_timeout)?;
                    client.subscribe_chat_events_with_fleet(cursor)?;
                    Ok(client)
                })
            {
                observe(true);
                while let Ok(event) = client.read_subscription_event() {
                    deliver(event);
                }
                cursor = client.chat_cursor().or(cursor);
                observe(false);
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

// The card in the shell is the presenter of last resort on every platform:
// the runtime's presenter connection hands each pairing request to this
// coordinator, and this registration turns it into the `attach-pairing-requested` event.
pub(super) fn register_approval_event_presenter<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let approval_app = app.clone();
    app.state::<AttachApprovalState>()
        .register_presenter_until(move |request, deadline| {
            let request = AttachPairingRequest {
                challenge: request.challenge.clone(),
                claimed_kind: request.claimed_kind.clone(),
                claimed_version: request.claimed_version.clone(),
                workspace: request.workspace.clone(),
                scopes: request.scopes.clone(),
                deadline_ms: u64::try_from(
                    deadline
                        .saturating_duration_since(std::time::Instant::now())
                        .as_millis(),
                )
                .unwrap_or(u64::MAX),
            };
            approval_app
                .emit("attach-pairing-requested", &request)
                .is_ok()
        });
}
