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
    let status = app.state::<AttachCompanionState>().listener_status();
    status.connected && status.chat_events_connected
}

#[cfg(target_os = "linux")]
pub(super) fn should_retry_attach_accept(error: AttachAcceptError) -> bool {
    matches!(
        error,
        AttachAcceptError::Accept
            | AttachAcceptError::PeerCredentials
            | AttachAcceptError::WrongUid(_)
    )
}

#[cfg(target_os = "linux")]
pub(super) fn initialize_attach_listener<R, F>(
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

#[cfg(unix)]
pub(super) fn start_approval_presenter<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let Some(endpoint) = runtime_profile_endpoint() else {
        eprintln!("approval presenter profile lookup failed");
        return;
    };
    let presenter_app = app.clone();
    app.state::<AttachCompanionState>()
        .start_approval_presenter(move |stop| {
            std::thread::spawn(move || {
                let approvals = presenter_app.state::<AttachApprovalState>().inner().clone();
                #[cfg(target_os = "linux")]
                let observer_app = presenter_app.clone();
                serve_approval_presenter_at(
                    &endpoint,
                    env!("CARGO_PKG_VERSION"),
                    Duration::from_secs(5),
                    Duration::from_millis(250),
                    stop,
                    move |presenting| {
                        #[cfg(target_os = "linux")]
                        observer_app
                            .state::<AttachCompanionState>()
                            .record_presenting(presenting);
                        #[cfg(target_os = "macos")]
                        let _ = presenting;
                    },
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
pub(super) fn runtime_profile_endpoint() -> Option<PathBuf> {
    let Ok(filesystem) = AttachFilesystem::from_environment() else {
        eprintln!("desktop client filesystem lookup failed");
        return None;
    };
    Some(filesystem.endpoint_path().to_owned())
}

#[cfg(target_os = "macos")]
pub(super) fn runtime_profile_endpoint() -> Option<PathBuf> {
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
                        move |event| {
                            let _ = event_app.emit("chat-event", event);
                        },
                    );
                })
            },
        );
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
                        move |event| {
                            let _ = event_app.emit("chat-event", event);
                        },
                    );
                })
            },
        );
    };
    #[cfg(target_os = "macos")]
    {
        state.start_approval_presenter(move |stop| {
            #[cfg(test)]
            TEST_PRESENTER_STARTS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::thread::spawn(move || {
                #[cfg(test)]
                TEST_PRESENTER_WORKERS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let approvals = presenter_app.state::<AttachApprovalState>().inner().clone();
                serve_approval_presenter_at(
                    &presenter_endpoint,
                    env!("CARGO_PKG_VERSION"),
                    Duration::from_secs(5),
                    Duration::from_millis(250),
                    stop,
                    |_| {},
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

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(super) fn register_approval_event_presenter<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
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
    register_approval_event_presenter(&app);
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
pub(super) fn run_attach_listener<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: Arc<AttachListenerState>,
    filesystem: AttachFilesystem,
) {
    run_attach_listener_with_hooks(app, state, filesystem, || {}, || {});
}

#[cfg(target_os = "linux")]
pub(super) fn run_attach_listener_with_hooks<R: tauri::Runtime>(
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
