mod cases {
    use super::super::*;
    #[cfg(unix)]
    use crate::test_support::socket_temp_path;
    use muniment_core::attach::{
        decode_frame, encode_frame, ErrorCode, ErrorMessage, Id, Protocol, Response, Success,
    };

    #[cfg(any(unix, target_os = "windows"))]
    #[test]
    fn a_history_read_waits_a_bounded_time_for_the_desktop_client_connection() {
        use std::time::{Duration, Instant};

        let state = AttachCompanionState::default();
        assert!(matches!(
            state.desktop_client_session_within(Duration::from_millis(50)),
            DesktopClientSession::NoSupervisor
        ));
        state.set_desktop_client_for_test(false, |_, _| std::thread::spawn(|| {}));
        let started = Instant::now();
        assert!(matches!(
            state.desktop_client_session_within(Duration::from_millis(100)),
            DesktopClientSession::Disconnected
        ));
        assert!(started.elapsed() >= Duration::from_millis(100));
        std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(100));
                state.record_connected(true);
            });
            let started = Instant::now();
            assert!(matches!(
                state.desktop_client_session_within(Duration::from_secs(5)),
                DesktopClientSession::Connected(_)
            ));
            assert!(started.elapsed() < Duration::from_secs(5));
        });
        assert!(matches!(
            state.desktop_client_session_within(Duration::from_secs(5)),
            DesktopClientSession::Connected(_)
        ));
        state.stop_desktop_client_for_test();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn runtime_start_waits_for_commands_and_chat_events() {
        let app = tauri::test::mock_app();
        app.manage(AttachCompanionState::default());
        let state = app.state::<AttachCompanionState>();
        assert!(!runtime_clients_ready(app.handle()));
        state.record_connected(true);
        assert!(!runtime_clients_ready(app.handle()));
        state.record_chat_events_connected(true);
        assert!(runtime_clients_ready(app.handle()));
        state.record_connected(false);
        assert!(!runtime_clients_ready(app.handle()));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_status_uses_live_desktop_client_state_and_start_is_idempotent() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let app = tauri::test::mock_app();
        app.manage(AttachCompanionState::default());
        let starts = Arc::new(AtomicUsize::new(0));
        for _ in 0..2 {
            let starts = starts.clone();
            app.state::<AttachCompanionState>()
                .start_desktop_client(move |_, _| {
                    starts.fetch_add(1, Ordering::SeqCst);
                    std::thread::spawn(|| {})
                });
        }
        app.state::<AttachCompanionState>().record_connected(true);

        let status = attach_listener_status(app.state::<AttachCompanionState>());
        assert!(status.started);
        assert!(status.supervisor_running);
        assert!(status.connected);
        assert!(!status.chat_events_connected);
        assert!(status.runtime_upgrade_pending);
        assert_eq!(starts.load(Ordering::SeqCst), 1);
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    #[test]
    fn desktop_companion_commands_fail_without_a_connected_client() {
        fn assert_commands_fail(app: &tauri::App<tauri::test::MockRuntime>) {
            assert_eq!(
                attach_companions(app.state()).unwrap_err().code(),
                ErrorCode::PersistenceFailed
            );
            assert_eq!(
                attach_revoke_companion(app.state(), "companion-1".into())
                    .unwrap_err()
                    .code(),
                ErrorCode::PersistenceFailed
            );
        }

        let app = tauri::test::mock_app();
        app.manage(AttachCompanionState::default());
        assert_commands_fail(&app);

        let state = AttachCompanionState::default();
        state.set_desktop_client_for_test(false, |_, _| std::thread::spawn(|| {}));
        let app = tauri::test::mock_app();
        app.manage(state);
        assert_commands_fail(&app);
        app.state::<AttachCompanionState>()
            .stop_desktop_client_for_test();
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    #[test]
    fn frontend_decision_resolves_a_presented_request() {
        use muniment_attach::ApprovalDecision;
        use muniment_core::attach::ApprovalPresentRequest;
        use std::sync::mpsc;
        use tauri::Listener;

        fn resolve(approve: bool) {
            let app = tauri::test::mock_app();
            let approvals = AttachApprovalState::default();
            app.manage(approvals.clone());
            register_approval_event_presenter(app.handle());
            let (event_tx, event_rx) = mpsc::channel();
            app.handle()
                .listen("attach-pairing-requested", move |event| {
                    event_tx.send(event.payload().to_owned()).unwrap();
                });

            let request = ApprovalPresentRequest {
                challenge: format!("challenge-{approve}"),
                claimed_kind: "cli".into(),
                claimed_version: "1.0.0".into(),
                workspace: "workspace-a".into(),
                scopes: vec!["thread.read".into()],
                deadline_ms: 1_000,
            };
            let worker =
                std::thread::spawn(move || answer_presented_approval(&approvals, &request));

            let payload = event_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            let event: serde_json::Value = serde_json::from_str(&payload).unwrap();
            assert_eq!(event["challenge"], format!("challenge-{approve}"));
            assert_eq!(event["claimed_kind"], "cli");
            assert_eq!(event["claimed_version"], "1.0.0");
            assert_eq!(event["workspace"], "workspace-a");
            assert_eq!(event["scopes"], serde_json::json!(["thread.read"]));

            attach_pairing_decide(
                app.state::<AttachApprovalState>(),
                format!("challenge-{approve}"),
                approve,
            );
            assert_eq!(
                worker.join().unwrap(),
                if approve {
                    ApprovalDecision::Approve
                } else {
                    ApprovalDecision::Deny
                }
            );
        }

        resolve(true);
        resolve(false);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_observers_publish_each_connection_transition() {
        use std::sync::mpsc;
        use tauri::Listener;

        let app = tauri::test::mock_app();
        app.manage(AttachCompanionState::default());
        let (status_tx, status_rx) = mpsc::channel();
        app.handle()
            .listen("desktop-client-status-changed", move |event| {
                let status: serde_json::Value = serde_json::from_str(event.payload()).unwrap();
                status_tx.send(status).unwrap();
            });

        observe_desktop_client_connection(app.handle(), true);
        let connected = status_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(connected["connected"], true);
        assert_eq!(connected["chat_events_connected"], false);

        observe_desktop_client_connection(app.handle(), false);
        let disconnected = status_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(disconnected["connected"], false);
        assert_eq!(disconnected["chat_events_connected"], false);

        observe_chat_event_subscription(app.handle(), true);
        let subscribed = status_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(subscribed["connected"], false);
        assert_eq!(subscribed["chat_events_connected"], true);

        observe_chat_event_subscription(app.handle(), false);
        let unsubscribed = status_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(unsubscribed["connected"], false);
        assert_eq!(unsubscribed["chat_events_connected"], false);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_starts_and_stops_both_desktop_supervisors() {
        use muniment_core::attach::{ChatEventSupervisorStop, DesktopClientSupervisorStop};
        use std::sync::mpsc;

        let state = AttachCompanionState::default();
        let (client_finished, client_finished_rx) = mpsc::channel();
        let (events_finished, events_finished_rx) = mpsc::channel();
        state.start_desktop_supervisors(
            move |stop, _| {
                std::thread::spawn(move || {
                    while !stop.stopped() {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    client_finished.send(()).unwrap();
                })
            },
            move |stop| {
                std::thread::spawn(move || {
                    while !stop.stopped() {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    events_finished.send(()).unwrap();
                })
            },
        );

        assert!(state.desktop_client.lock().unwrap().is_some());
        assert!(state.chat_events.lock().unwrap().is_some());
        state.stop_desktop_client();
        client_finished_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        events_finished_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        assert!(state.desktop_client.lock().unwrap().is_none());
        assert!(state.chat_events.lock().unwrap().is_none());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_drop_stops_and_joins_both_desktop_supervisors() {
        use muniment_core::attach::{ChatEventSupervisorStop, DesktopClientSupervisorStop};
        use std::sync::mpsc;

        let (client_finished, client_finished_rx) = mpsc::channel();
        let (events_finished, events_finished_rx) = mpsc::channel();
        {
            let state = AttachCompanionState::default();
            state.start_desktop_supervisors(
                move |stop, _| {
                    std::thread::spawn(move || {
                        while !stop.stopped() {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        client_finished.send(()).unwrap();
                    })
                },
                move |stop| {
                    std::thread::spawn(move || {
                        while !stop.stopped() {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        events_finished.send(()).unwrap();
                    })
                },
            );
        }

        client_finished_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        events_finished_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_observers_publish_each_connection_transition() {
        use std::sync::mpsc;
        use tauri::Listener;

        let app = tauri::test::mock_app();
        app.manage(AttachCompanionState::default());
        let (status_tx, status_rx) = mpsc::channel();
        app.handle()
            .listen("desktop-client-status-changed", move |event| {
                let status: Value = serde_json::from_str(event.payload()).unwrap();
                status_tx.send(status).unwrap();
            });

        observe_desktop_client_connection(app.handle(), true);
        let connected = status_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(connected["connected"], true);
        assert_eq!(connected["chat_events_connected"], false);

        observe_desktop_client_connection(app.handle(), false);
        let disconnected = status_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(disconnected["connected"], false);
        assert_eq!(disconnected["chat_events_connected"], false);

        observe_chat_event_subscription(app.handle(), true);
        let subscribed = status_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(subscribed["connected"], false);
        assert_eq!(subscribed["chat_events_connected"], true);

        observe_chat_event_subscription(app.handle(), false);
        let unsubscribed = status_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(unsubscribed["connected"], false);
        assert_eq!(unsubscribed["chat_events_connected"], false);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_starts_both_desktop_supervisors_without_a_listener() {
        let state = AttachCompanionState::default();
        let endpoint = socket_temp_path();
        let _ = std::fs::remove_file(&endpoint);
        assert!(!endpoint.exists());
        let client_endpoint = endpoint.clone();
        state.start_desktop_supervisors(
            move |stop, holder| {
                std::thread::spawn(move || {
                    serve_desktop_client_at(
                        &client_endpoint,
                        "0.0.1",
                        Duration::from_millis(10),
                        Duration::from_secs(30),
                        stop,
                        holder,
                        |_| {},
                    );
                })
            },
            move |stop| {
                std::thread::spawn(move || {
                    serve_chat_events_at(
                        &endpoint,
                        "0.0.1",
                        Duration::from_millis(10),
                        Duration::from_secs(30),
                        stop,
                        |_| {},
                        |_| {},
                    );
                })
            },
        );

        assert!(matches!(
            state.desktop_client_session(),
            DesktopClientSession::Disconnected
        ));
        assert!(state.desktop_client.lock().unwrap().is_some());
        assert!(state.chat_events.lock().unwrap().is_some());
        state.stop_desktop_client();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn approval_presenter_starts_once_and_stops() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let state = AttachCompanionState::default();
        let starts = Arc::new(AtomicUsize::new(0));
        let worker = Arc::new(Mutex::new(None));
        let endpoint = socket_temp_path();

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
        state.stop_approval_presenter();
        worker.lock().unwrap().take().unwrap().join().unwrap();
        assert!(state.approval_presenter.lock().unwrap().is_none());
    }

    // The presenter start counters are process-wide, so the tests that start
    // the desktop client take turns.
    #[cfg(target_os = "macos")]
    static DESKTOP_CLIENT_START: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_desktop_client_starts_without_a_managed_companion_state() {
        let _turn = DESKTOP_CLIENT_START
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let app = tauri::test::mock_app();
        app.manage(AttachApprovalState::default());

        start_desktop_client(app.handle());

        let state = app
            .try_state::<AttachCompanionState>()
            .expect("start_desktop_client manages the companion state it reads");
        state.stop_approval_presenter();
        state.stop_desktop_client();
    }

    // The runtime's presenter connection hands a pairing request to the
    // desktop's coordinator. Without the card registered as its presenter the
    // request is denied before the shell ever hears of it.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_desktop_client_registers_the_pairing_card_as_presenter() {
        use muniment_attach::ApprovalDecision;
        use muniment_core::attach::ApprovalPresentRequest;
        use std::sync::mpsc;
        use tauri::Listener;

        let _turn = DESKTOP_CLIENT_START
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let app = tauri::test::mock_app();
        app.manage(AttachApprovalState::default());
        start_desktop_client(app.handle());
        let (event_tx, event_rx) = mpsc::channel();
        app.handle()
            .listen("attach-pairing-requested", move |event| {
                event_tx.send(event.payload().to_owned()).unwrap();
            });

        let approvals = app.state::<AttachApprovalState>().inner().clone();
        let worker = std::thread::spawn(move || {
            answer_presented_approval(
                &approvals,
                &ApprovalPresentRequest {
                    challenge: "challenge-card".into(),
                    claimed_kind: "cli".into(),
                    claimed_version: "1.0.0".into(),
                    workspace: "workspace-a".into(),
                    scopes: vec!["thread.read".into()],
                    deadline_ms: 1_000,
                },
            )
        });

        let payload = event_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let event: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(event["challenge"], "challenge-card");
        attach_pairing_decide(
            app.state::<AttachApprovalState>(),
            "challenge-card".into(),
            true,
        );
        assert_eq!(worker.join().unwrap(), ApprovalDecision::Approve);

        let state = app.state::<AttachCompanionState>();
        state.stop_approval_presenter();
        state.stop_desktop_client();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_desktop_supervisor_restart_does_not_duplicate_presenter() {
        use std::sync::atomic::Ordering;

        let _turn = DESKTOP_CLIENT_START
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        TEST_PRESENTER_STARTS.store(0, Ordering::SeqCst);
        TEST_PRESENTER_WORKERS.store(0, Ordering::SeqCst);
        let app = tauri::test::mock_app();
        app.manage(AttachCompanionState::default());
        app.manage(AttachApprovalState::default());

        start_desktop_client(app.handle());
        start_desktop_client(app.handle());

        assert_eq!(TEST_PRESENTER_STARTS.load(Ordering::SeqCst), 1);
        for _ in 0..100 {
            if TEST_PRESENTER_WORKERS.load(Ordering::SeqCst) == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(TEST_PRESENTER_WORKERS.load(Ordering::SeqCst), 1);
        let state = app.state::<AttachCompanionState>();
        state.stop_approval_presenter();
        state.stop_desktop_client();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn desktop_client_starts_once_and_stops() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let state = AttachCompanionState::default();
        let starts = Arc::new(AtomicUsize::new(0));
        let endpoint = socket_temp_path();

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
        state.stop_desktop_client();
        assert!(state.desktop_client.lock().unwrap().is_none());
        assert!(!state.listener_status().supervisor_running);
        assert!(!state.listener_status().connected);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn chat_event_supervisor_recovers_a_late_subscription_and_reports_disconnect() {
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

        let endpoint = socket_temp_path();
        let listener = UnixListener::bind(&endpoint).unwrap();
        let (pending, pending_rx) = mpsc::channel();
        let (subscribe, subscribe_rx) = mpsc::channel();
        let (close, close_rx) = mpsc::channel();
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
            pending.send(()).unwrap();
            subscribe_rx.recv_timeout(Duration::from_secs(5)).unwrap();
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
            close_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        });

        let state = Arc::new(AttachCompanionState::default());
        state.record_connected(true);
        let observer_state = state.clone();
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
                    move |connected| {
                        observer_state.record_chat_events_connected(connected);
                        observed.send(connected).unwrap();
                    },
                    move |event| delivered.send(event).unwrap(),
                )
            })
        });
        pending_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(state.runtime_client_connections(), (true, false));
        subscribe.send(()).unwrap();
        assert_eq!(
            received.recv_timeout(Duration::from_secs(1)).unwrap(),
            SubscriptionEvent::Chat(json!({"phase": "running", "text": "forwarded"}))
        );
        assert!(observations.recv_timeout(Duration::from_secs(1)).unwrap());
        assert_eq!(state.runtime_client_connections(), (true, true));
        close.send(()).unwrap();
        let disconnected = observations.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(!disconnected);
        assert_eq!(state.runtime_client_connections(), (true, false));
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

        let endpoint = socket_temp_path();
        let listener = UnixListener::bind(&endpoint).unwrap();
        // SAFETY: `listener` owns a valid Unix socket descriptor.
        assert_eq!(unsafe { listen(listener.as_raw_fd(), 0) }, 0);
        let queued_stream = UnixStream::connect(&endpoint).unwrap();
        let (stop, connect_started) = ChatEventStopHandle::for_test();
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

        connect_started.recv().unwrap();
        stop.stop();
        finished_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        worker.join().unwrap();
        drop(queued_stream);
        drop(listener);
        std::fs::remove_file(endpoint).unwrap();
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
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

        let endpoint = socket_temp_path();
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
    fn attach_listener_status_reports_the_runtime_upgrade_hold_while_connected() {
        let state = AttachCompanionState::default();

        state.record_connected(true);
        let held = state.listener_status();
        assert!(held.connected);
        assert!(held.runtime_upgrade_pending);

        state.record_connected(false);
        let released = state.listener_status();
        assert!(!released.connected);
        assert!(!released.runtime_upgrade_pending);
    }

}
