#![cfg(target_os = "linux")]

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;

use muniment_core::active_run::ChatDelivery;
use muniment_core::attach::linux::{
    CompanionProvenance, ThreadListRequest, ThreadListService, ThreadOpenRequest,
};
use muniment_core::attach::{
    ErrorCode, Id, ProtocolError, RuntimeActivityRegistry, SignedWorkspaceApproval,
};
use muniment_core::auth::{
    BrowserOpenError, EntitlementSnapshotTracker, KeyringNativeCredentialStore,
    NativeCredentialStore,
};
use muniment_core::journal::{CasReference, EventEnvelope, EventPayload, Provenance};
use muniment_core::memory_runtime::ApplicationMemoryRuntime;
use muniment_core::permission_gate::ChatPermissionAnswer;
use muniment_core::pi_execution::PiRuntime;
use muniment_core::run_preparation::{
    prepare_new_run_in_thread_after_validation, prepare_new_run_with_session_thread,
    SessionThreadStart,
};
use muniment_core::run_start::{ActiveRun, RunAttachBoundaries, RunStartBoundaries};
use muniment_core::session_thread::SessionThread;
use muniment_runtime::{
    open_companion_registry, open_profile_storage, RuntimeAttachBoundaries, RuntimeAttachState,
};

mod common;
#[path = "../../src/linux_runtime_service/activation.rs"]
mod desktop_activation;
use common::{
    credentials, credentials_with_expiry, spawn_server, spawn_server_sequence, TemporaryProfile,
};

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn provenance() -> Provenance {
    let mut provenance = Provenance {
        source: "muniment-attach".into(),
        source_version: "test".into(),
        actor_id: None,
        device_id: None,
        rpc_request_id: None,
        capability_versions: None,
        extra: BTreeMap::new(),
    };
    provenance
        .extra
        .insert("attach_profile".into(), "profile-a".into());
    provenance
}

fn artifact_event(
    event_id: &str,
    run_id: &str,
    run_seq: u64,
    payload: EventPayload,
) -> EventEnvelope {
    EventEnvelope {
        event_id: event_id.into(),
        run_id: run_id.into(),
        run_seq,
        event_type: "test.artifact".into(),
        event_version: 1,
        envelope_version: 1,
        recorded_at: "2026-08-20T00:00:00Z".into(),
        occurred_at: None,
        correlation_id: None,
        causation_id: None,
        payload,
        provenance: provenance(),
        extra: BTreeMap::new(),
    }
}

fn assert_invalid_artifact(
    result: Result<muniment_core::attach::linux::ArtifactFetchResult, ProtocolError>,
) {
    let error = result.unwrap_err();
    assert_eq!(error.code(), ErrorCode::InvalidRequest);
    assert!(!error.retryable());
}

#[derive(Clone)]
struct AuthorizationAttempt {
    redirect_uri: String,
    state: String,
}

fn spawn_sign_in_server() -> (
    String,
    Arc<Mutex<Option<AuthorizationAttempt>>>,
    thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let attempt = Arc::new(Mutex::new(None));
    let captured_attempt = Arc::clone(&attempt);
    let server = thread::spawn(move || {
        for index in 0..3 {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request_body(&mut stream);
            let body: String = match index {
                0 => r#"{"device_id":"10000000-0000-4000-8000-000000000001","registration_token":"AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI","device_challenge":"AwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwM","expires_in":600}"#.into(),
                1 => {
                    let request_body = request.split_once("\r\n\r\n").unwrap().1;
                    *captured_attempt.lock().unwrap() = Some(AuthorizationAttempt {
                        redirect_uri: json_field(request_body, "redirect_uri"),
                        state: json_field(request_body, "state"),
                    });
                    r#"{"authorization_url":"https://login.muniment.test/continue","device_challenge":"BAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQ"}"#.into()
                }
                _ => r#"{"access_token":"access-secret","token_type":"Bearer","expires_in":900,"refresh_token":"refresh-secret","refresh_expires_in":86400,"session":{"org_id":"20000000-0000-4000-8000-000000000002","user_id":"30000000-0000-4000-8000-000000000003","role":"user","device_id":"10000000-0000-4000-8000-000000000001","client_role":"desktop"},"entitlement_snapshot":{"payload":{"version":1},"signature":"snapshot-secret","algorithm":"hmac-sha256"},"device_challenge":"BQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQU"}"#.into(),
            };
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
    });
    (base_url, attempt, server)
}

fn json_field(body: &str, field: &str) -> String {
    body.split_once(&format!(r#""{field}":""#))
        .unwrap()
        .1
        .split_once('"')
        .unwrap()
        .0
        .into()
}

fn read_request_body(stream: &mut TcpStream) -> String {
    let mut request = Vec::new();
    while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
        let mut buffer = [0; 1024];
        let count = stream.read(&mut buffer).unwrap();
        request.extend_from_slice(&buffer[..count]);
    }
    let headers = String::from_utf8(request).unwrap();
    let content_length = headers
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length: ")
                .and_then(|value| value.parse::<usize>().ok())
        })
        .unwrap_or(0);
    let present = headers.split_once("\r\n\r\n").unwrap().1.len();
    let mut request = headers;
    let mut body = vec![0; content_length.saturating_sub(present)];
    stream.read_exact(&mut body).unwrap();
    request.push_str(std::str::from_utf8(&body).unwrap());
    request
}

fn send_callback(attempt: AuthorizationAttempt) {
    let (authority, path) = attempt
        .redirect_uri
        .strip_prefix("http://")
        .unwrap()
        .split_once('/')
        .unwrap();
    let mut stream = TcpStream::connect(authority).unwrap();
    write!(stream, "GET /{path}?code=CQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQk&state={} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n", attempt.state).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"));
}

struct StopRuntime(std::sync::mpsc::Sender<()>);

impl Drop for StopRuntime {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

#[test]
fn desktop_starts_the_deb_runtime_and_reconnects_after_a_stale_socket() {
    desktop_starts_runtime_in_layout("usr/lib/muniment", "usr/bin", true);
}

#[test]
fn desktop_starts_the_appimage_runtime_without_a_deb_alias() {
    desktop_starts_runtime_in_layout("usr/lib/muniment", "usr/bin", false);
}

#[test]
fn desktop_starts_the_development_runtime_and_reconnects_after_a_stale_socket() {
    desktop_starts_runtime_in_layout("target/debug", "target/debug", false);
}

fn desktop_starts_runtime_in_layout(resources: &str, bin: &str, deb_alias: bool) {
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::time::Duration;

    use muniment_attach::connect_desktop_client_at;
    use muniment_core::attach::linux::AttachFilesystem;

    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    struct Environment(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for Environment {
        fn drop(&mut self) {
            for (key, value) in &self.0 {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    let _guard = TEST_LOCK.lock().unwrap();
    let profile = TemporaryProfile::new("packaged-desktop-start", false);
    fs::set_permissions(&profile.root, fs::Permissions::from_mode(0o700)).unwrap();
    let filesystem = AttachFilesystem::from_runtime_directory(&profile.root).unwrap();
    let resources = profile.root.join(resources);
    let bin = profile.root.join(bin);
    fs::create_dir_all(&resources).unwrap();
    fs::create_dir_all(&bin).unwrap();
    let desktop = bin.join("muniment-desktop");
    symlink(std::env::current_exe().unwrap(), &desktop).unwrap();
    if deb_alias {
        symlink("muniment-desktop", bin.join("muniment")).unwrap();
    } else {
        assert!(!bin.join("muniment").exists());
    }
    let executable = resources.join("muniment-runtime");
    fs::copy(env!("CARGO_BIN_EXE_muniment-runtime"), &executable).unwrap();
    symlink(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../third-party/sherpa-onnx-v1.13.2/linux-x86_64"),
        resources.join("asr-runtime"),
    )
    .unwrap();
    let _environment = Environment(
        ["XDG_RUNTIME_DIR", "XDG_DATA_HOME", "XDG_CONFIG_HOME"]
            .into_iter()
            .map(|key| (key, std::env::var_os(key)))
            .collect(),
    );
    std::env::set_var("XDG_RUNTIME_DIR", &profile.root);
    std::env::set_var("XDG_DATA_HOME", &profile.profile);
    std::env::set_var("XDG_CONFIG_HOME", &profile.config);
    let config = profile.config.join("ai.muniment.desktop");
    fs::create_dir_all(&config).unwrap();
    fs::write(
        config.join(muniment_core::local_mode::LOCAL_MODE_MARKER),
        "1",
    )
    .unwrap();
    assert!(!filesystem.endpoint_path().exists());
    for _ in 0..2 {
        let mut child = None;
        let mut client = None;
        let mut events = None;
        desktop_activation::activate_runtime(
            &filesystem,
            || {
                child = Some(Child(desktop_activation::spawn_runtime(&executable)?));
                Ok(())
            },
            || {
                client = connect_desktop_client_at(
                    filesystem.endpoint_path(),
                    "1.0.0",
                    Duration::from_secs(1),
                )
                .ok();
                events = connect_desktop_client_at(
                    filesystem.endpoint_path(),
                    "1.0.0",
                    Duration::from_secs(1),
                )
                .ok()
                .and_then(|mut client| {
                    client.subscribe_chat_events().ok()?;
                    Some(client)
                });
                client.is_some() && events.is_some()
            },
            Duration::from_secs(5),
        )
        .unwrap();
        let mut client = client.unwrap();
        let events = events.unwrap();
        assert_eq!(client.runtime_version(), env!("CARGO_PKG_VERSION"));
        assert!(client.thread_summaries(20, None).unwrap()["summaries"]
            .as_array()
            .unwrap()
            .is_empty());
        client.recheck_retention().unwrap();
        desktop_activation::activate_runtime(
            &filesystem,
            || panic!("A connected runtime must not start twice."),
            || true,
            Duration::from_secs(1),
        )
        .unwrap();
        // Both connections use the same handshake. Reject missing and mismatched executables.
        for replacement in [None, Some("/bin/true")] {
            fs::remove_file(&desktop).unwrap();
            if let Some(replacement) = replacement {
                symlink(replacement, &desktop).unwrap();
            }
            assert!(connect_desktop_client_at(
                filesystem.endpoint_path(),
                "1.0.0",
                Duration::from_secs(1),
            )
            .is_err());
            if replacement.is_some() {
                fs::remove_file(&desktop).unwrap();
            }
            symlink(std::env::current_exe().unwrap(), &desktop).unwrap();
        }
        drop(events);
        drop(client);
        drop(child);
        assert!(filesystem.endpoint_path().exists());
    }
}

#[test]
fn cold_desktop_start_serves_sign_in_local_chat_history_and_retention() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    use muniment_attach::connect_desktop_client_at;
    use muniment_core::attach::linux::AttachFilesystem;
    use muniment_core::retention_record::{write_retention_choice, RetentionChoice};

    let _guard = TEST_LOCK.lock().unwrap();
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let store = KeyringNativeCredentialStore::new();
    store.clear_session().unwrap();
    let profile = TemporaryProfile::new("cold-desktop", true);
    fs::set_permissions(&profile.root, fs::Permissions::from_mode(0o700)).unwrap();
    let filesystem = AttachFilesystem::from_runtime_directory(&profile.root).unwrap();
    let endpoint = filesystem.endpoint_path();
    assert!(!endpoint.exists());
    let artifact = common::stage_pi_stub(&profile.profile);
    std::env::remove_var("MUNIMENT_PI_ROOT");
    let (base_url, attempt, server) = spawn_sign_in_server();
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    let opener = Arc::new(move |_: &str| -> Result<(), BrowserOpenError> {
        let callback = attempt.lock().unwrap().clone().unwrap();
        thread::spawn(move || send_callback(callback));
        Ok(())
    });
    let (stop_tx, stop_rx) = std::sync::mpsc::channel();
    // Open the observer before the runtime starts so it cannot reconcile a live run.
    let storage = open_profile_storage(&profile.profile).unwrap();
    let mut client = None;
    thread::scope(|scope| {
        let stop = StopRuntime(stop_tx);
        let mut listener = None;
        desktop_activation::activate_runtime(
            &filesystem,
            || {
                let root = profile.root.clone();
                let config = profile.config.clone();
                let data = profile.profile.clone();
                listener = Some(scope.spawn(move || {
                    let state = Arc::new(RuntimeAttachState::open(&data, &config).unwrap());
                    let service_state = state.clone();
                    let mut inputs = state.attach_listener_inputs();
                    inputs.expected_desktop_executable = Some(std::env::current_exe().unwrap());
                    muniment_runtime::run_attach_listener(
                        &root,
                        inputs,
                        None,
                        move || {
                            muniment_runtime::compose_attach_service(
                                service_state
                                    .boundaries()
                                    .with_browser_opener(opener.clone())
                                    .with_pi_artifact(artifact),
                                service_state.companion_registry(),
                                &data,
                                &config,
                            )
                        },
                        stop_rx,
                    )
                    .unwrap();
                }));
                Ok(())
            },
            || {
                client = connect_desktop_client_at(endpoint, "1.0.0", Duration::from_secs(1)).ok();
                client.is_some()
            },
            Duration::from_secs(5),
        )
        .unwrap();
        let mut client = client.take().unwrap();
        {
            assert_eq!(client.sign_in().unwrap()["status"]["signed_in"], true);
            fs::write(
                profile
                    .config
                    .join(muniment_core::local_mode::LOCAL_MODE_MARKER),
                "1",
            )
            .unwrap();
            assert!(client.thread_summaries(20, None).unwrap()["summaries"]
                .as_array()
                .unwrap()
                .is_empty());
            let accepted = client.run_submit("cold local prompt", &[], None).unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let events = storage
                    .lock()
                    .unwrap()
                    .journal
                    .events(&accepted.run_id)
                    .unwrap();
                assert!(!events.iter().any(|event| event.event_type == "run.failed"));
                if events
                    .iter()
                    .any(|event| event.event_type == "run.completed")
                {
                    break;
                }
                assert!(Instant::now() < deadline, "The local run did not complete.");
                thread::sleep(Duration::from_millis(10));
            }
            let summaries = client.thread_summaries(20, None).unwrap();
            let thread_id = summaries["summaries"][0]["threadId"].as_str().unwrap();
            let history = client.thread_history(thread_id, 20, None).unwrap();
            assert_eq!(history["entries"][0]["runId"], accepted.run_id);
            assert_eq!(history["entries"][0]["prompt"], "cold local prompt");
            let expired = "01900000-0000-7000-8000-000000000031";
            for (seq, event_type) in [(1, "run.started"), (2, "run.completed")] {
                let mut event = artifact_event(
                    &format!("01900000-0000-7000-8000-00000000004{seq}"),
                    expired,
                    seq,
                    EventPayload::Inline {
                        payload_json: "{}".parse().unwrap(),
                    },
                );
                event.event_type = event_type.into();
                event.recorded_at = "2000-01-01T00:00:00Z".into();
                storage
                    .lock()
                    .unwrap()
                    .journal
                    .append_batch(seq - 1, &[event])
                    .unwrap();
            }
            write_retention_choice(&profile.config, RetentionChoice::DeleteAfter30Days).unwrap();
            client.recheck_retention().unwrap();
            assert!(storage
                .lock()
                .unwrap()
                .journal
                .events(expired)
                .unwrap()
                .is_empty());
            // The retention check preserves this new run.
            assert!(!storage
                .lock()
                .unwrap()
                .journal
                .events(&accepted.run_id)
                .unwrap()
                .is_empty());
        }
        drop(client);
        drop(stop);
        listener.unwrap().join().unwrap();
    });
    server.join().unwrap();
    store.clear_session().unwrap();
    std::env::remove_var("MUNIMENT_API_BASE_URL");
}

#[test]
fn sign_in_cannot_override_a_return_to_local_mode() {
    let profile = TemporaryProfile::new("sign-in-marker", false);
    let state = RuntimeAttachState::open(&profile.profile, &profile.config).unwrap();
    let marker = profile
        .config
        .join(muniment_core::local_mode::LOCAL_MODE_MARKER);
    fs::write(&marker, "1").unwrap();
    let error = state
        .boundaries()
        .with_browser_opener(Arc::new(|_: &str| panic!("The browser must not open.")))
        .sign_in(provenance())
        .unwrap_err();
    assert_eq!(error.code(), ErrorCode::AuthorizationFailed);
    let diagnostic = error.to_string();
    assert!(diagnostic.contains("Cancelled"));
    assert!(marker.exists());
    assert!(!diagnostic.contains(profile.config.to_str().unwrap()));
}

#[test]
fn runtime_boundaries_serve_sign_in_and_refuse_a_concurrent_attempt() {
    let _guard = TEST_LOCK.lock().unwrap();
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let temporary_profile = TemporaryProfile::new("attach-sign-in", false);
    let state = Arc::new(
        RuntimeAttachState::open(&temporary_profile.profile, &temporary_profile.config).unwrap(),
    );
    let (base_url, attempt, server) = spawn_sign_in_server();
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    let (opened_tx, opened_rx) = std::sync::mpsc::sync_channel(0);
    let (continue_tx, continue_rx) = std::sync::mpsc::sync_channel(0);
    let continue_rx = Mutex::new(continue_rx);
    let first_state = Arc::clone(&state);
    let first = thread::spawn(move || {
        let opener = Arc::new(move |url: &str| -> Result<(), BrowserOpenError> {
            assert_eq!(url, "https://login.muniment.test/continue");
            opened_tx.send(()).unwrap();
            continue_rx.lock().unwrap().recv().unwrap();
            let callback = attempt.lock().unwrap().clone().unwrap();
            thread::spawn(move || send_callback(callback));
            Ok(())
        });
        first_state
            .boundaries()
            .with_browser_opener(opener)
            .sign_in(provenance())
    });
    opened_rx.recv().unwrap();

    let browser_count = Arc::new(AtomicBool::new(false));
    let called = Arc::clone(&browser_count);
    let second = state
        .boundaries()
        .with_browser_opener(Arc::new(move |_: &str| {
            called.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }))
        .sign_in(provenance())
        .unwrap_err();
    assert_eq!(second.code(), ErrorCode::InvalidRequest);
    assert!(!browser_count.load(std::sync::atomic::Ordering::SeqCst));

    continue_tx.send(()).unwrap();
    let status = first.join().unwrap().unwrap();
    assert!(status.signed_in);
    server.join().unwrap();
    std::env::remove_var("MUNIMENT_API_BASE_URL");
}

#[test]
fn runtime_state_answers_entitlement_and_replays_sign_out() {
    let _guard = TEST_LOCK.lock().unwrap();
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let temporary_profile = TemporaryProfile::new("attach-session", false);
    let state =
        RuntimeAttachState::open(&temporary_profile.profile, &temporary_profile.config).unwrap();
    let credential_store = KeyringNativeCredentialStore::new();
    credential_store.clear_session().unwrap();
    credential_store.save_credentials(&credentials()).unwrap();

    let session_body = |version| {
        format!(
            r#"{{"session":{{"org_id":"20000000-0000-4000-8000-000000000002","user_id":"30000000-0000-4000-8000-000000000003","role":"owner","device_id":"10000000-0000-4000-8000-000000000001","client_role":"desktop","expires_at":"2099-01-01T00:00:00Z"}},"user":{{"id":"30000000-0000-4000-8000-000000000003","email":"user@example.com","status":"active","role":"owner","entitlement_version":{version}}},"org":{{"id":"20000000-0000-4000-8000-000000000002","display_name":"Muniment"}},"entitlement_snapshot":{{"payload":{{"org_id":"20000000-0000-4000-8000-000000000002","user_id":"30000000-0000-4000-8000-000000000003","entitlement_version":{version},"issued_at":"2026-08-01T00:00:00Z","capabilities":[],"grants":[]}},"signature":"signature-secret","algorithm":"hmac-sha256"}}}}"#
        )
    };
    let (base_url, server) = spawn_server(200, session_body(7));
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    let first = state.boundaries().entitlement_snapshot().unwrap();
    assert_eq!(first.snapshot.snapshot_version, 7);
    assert_eq!(first.changed_snapshot_version, None);
    server.join().unwrap();

    let (base_url, server) = spawn_server(200, session_body(8));
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    let second = state.boundaries().entitlement_snapshot().unwrap();
    assert_eq!(second.snapshot.snapshot_version, 8);
    assert_eq!(second.changed_snapshot_version, Some(8));
    server.join().unwrap();

    let (base_url, server) = spawn_server(200, r#"{"ok":true}"#.into());
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    let mut service = state.attach_service().unwrap();
    let idempotency_key = Id::new("018f0000-0000-7000-8000-000000000030").unwrap();
    let companion = CompanionProvenance {
        profile: "default".into(),
        companion_kind: "cli".into(),
        companion_version: "1.2.3".into(),
        peer_uid: 1000,
        peer_pid: 42,
    };
    for request_id in [
        "018f0000-0000-7000-8000-000000000031",
        "018f0000-0000-7000-8000-000000000032",
    ] {
        let status = service
            .sign_out(
                &Id::new(request_id).unwrap(),
                &idempotency_key,
                companion.clone(),
            )
            .unwrap();
        assert!(!status.signed_in);
    }
    let request = server.join().unwrap().to_ascii_lowercase();
    assert!(request.starts_with("post /v1/auth/native/revoke http/1.1\r\n"));
    assert!(credential_store.load_credentials().unwrap().is_none());
    std::env::remove_var("MUNIMENT_API_BASE_URL");
}

#[test]
fn runtime_boundaries_answer_all_attach_reads() {
    let _guard = TEST_LOCK.lock().unwrap();
    let temporary_profile = TemporaryProfile::new("attach-boundaries", false);
    let profile = temporary_profile.profile.clone();
    let storage = open_profile_storage(&profile).unwrap();
    let runtime_activity = RuntimeActivityRegistry::new();
    let permission_answers = Arc::new(Mutex::new(VecDeque::new()));
    let active = Arc::new(Mutex::new(Some(ActiveRun {
        id: "run-1".into(),
        workspace: "workspace-a".into(),
        cancelled: Arc::new(AtomicBool::new(false)),
        transport: Arc::new(Mutex::new(None)),
        adapter: Arc::new(Mutex::new(None)),
        permission_answers: Arc::clone(&permission_answers),
        _activity: runtime_activity.mark_active_run(),
    })));
    let config = temporary_profile.config.clone();
    let boundaries = RuntimeAttachBoundaries::new(
        Arc::clone(&storage),
        active,
        profile.clone(),
        config.clone(),
        Arc::new(Mutex::new(None::<PiRuntime>)),
        Arc::new(ApplicationMemoryRuntime::new(
            config,
            profile.join("memory"),
        )),
        runtime_activity,
        Arc::new(EntitlementSnapshotTracker::new()),
        SignedWorkspaceApproval::default(),
        Arc::new(SessionThread::default()),
        open_companion_registry(&profile).unwrap(),
    );

    let created_thread = boundaries
        .create_thread("workspace-a", provenance())
        .unwrap();
    let created_events = storage
        .lock()
        .unwrap()
        .journal
        .thread_events(&created_thread)
        .unwrap();
    assert_eq!(created_events[0].provenance.source, "muniment-runtime");
    assert_eq!(
        created_events[0].provenance.source_version,
        env!("CARGO_PKG_VERSION")
    );
    prepare_new_run_in_thread_after_validation(
        &storage,
        "01900000-0000-7000-8000-000000000019",
        "workspace-a",
        Some("user"),
        Vec::new(),
        Some(provenance()),
        &created_thread,
        "test",
        "1",
        || Ok(()),
    )
    .unwrap();

    let run_id = "01900000-0000-7000-8000-000000000020";
    let (current_run_seq, _) = prepare_new_run_with_session_thread(
        &storage,
        SessionThreadStart {
            tracker: &SessionThread::default(),
            continue_existing: false,
        },
        run_id,
        "workspace-a",
        Some("user"),
        Vec::new(),
        None,
        "test",
        "1",
        || Ok(()),
    )
    .unwrap();
    let run_thread = storage
        .lock()
        .unwrap()
        .journal
        .run_thread_id(run_id)
        .unwrap()
        .unwrap();
    let other_run = "01900000-0000-7000-8000-000000000021";
    prepare_new_run_with_session_thread(
        &storage,
        SessionThreadStart {
            tracker: &SessionThread::default(),
            continue_existing: false,
        },
        other_run,
        "workspace-a",
        Some("other"),
        Vec::new(),
        None,
        "test",
        "1",
        || Ok(()),
    )
    .unwrap();
    let other_thread = storage
        .lock()
        .unwrap()
        .journal
        .run_thread_id(other_run)
        .unwrap()
        .unwrap();

    let threads = boundaries
        .list_threads(
            "workspace-a",
            ThreadListRequest {
                limit: 10,
                cursor: None,
            },
        )
        .unwrap();
    assert!(threads
        .threads
        .iter()
        .any(|thread| thread.thread_id == run_thread));

    let opened = boundaries
        .open_thread(
            "workspace-a",
            ThreadOpenRequest {
                thread_id: run_thread.clone(),
                limit: 10,
                cursor: None,
            },
        )
        .unwrap();
    assert_eq!(opened.thread_id, run_thread);

    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let credential_store = KeyringNativeCredentialStore::new();
    credential_store.clear_session().unwrap();
    credential_store.save_credentials(&credentials()).unwrap();

    let status = boundaries.session_status().unwrap();
    assert!(status.signed_in);
    assert_eq!(status.subject.as_deref(), Some("user"));

    // Local mode answers the status read without opening the keyring, so a
    // keyring that refuses every read never reaches the desktop.
    let local_mode_marker = temporary_profile
        .config
        .join(muniment_core::local_mode::LOCAL_MODE_MARKER);
    fs::write(&local_mode_marker, "1").unwrap();
    muniment_core::chat_prompt::use_refused_keyring_for_tests(
        -25293,
        "refused",
        true,
        Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    );
    let local_status = boundaries.session_status().unwrap();
    assert!(!local_status.signed_in);
    assert_eq!(local_status.subject, None);
    assert!(boundaries.entitlement_snapshot().is_err());
    fs::remove_file(&local_mode_marker).unwrap();
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    credential_store.clear_session().unwrap();
    credential_store.save_credentials(&credentials()).unwrap();
    assert!(boundaries.session_status().unwrap().signed_in);

    let session_body = r#"{"session":{"org_id":"20000000-0000-4000-8000-000000000002","user_id":"30000000-0000-4000-8000-000000000003","role":"owner","device_id":"10000000-0000-4000-8000-000000000001","client_role":"desktop","expires_at":"2099-01-01T00:00:00Z"},"user":{"id":"30000000-0000-4000-8000-000000000003","email":"user@example.com","status":"active","role":"owner","entitlement_version":7},"org":{"id":"20000000-0000-4000-8000-000000000002","display_name":"Muniment"},"entitlement_snapshot":{"payload":{"org_id":"20000000-0000-4000-8000-000000000002","user_id":"30000000-0000-4000-8000-000000000003","entitlement_version":7,"issued_at":"2026-08-01T00:00:00Z","capabilities":[],"grants":[]},"signature":"signature-secret","algorithm":"hmac-sha256"}}"#;
    let (base_url, summaries_server) = spawn_server(200, session_body.into());
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    let summaries = boundaries
        .thread_summaries(ThreadListRequest {
            limit: 10,
            cursor: None,
        })
        .unwrap();
    summaries_server.join().unwrap();
    let summaries = summaries.summaries;
    assert!(summaries
        .iter()
        .any(|summary| summary.thread_id == run_thread));
    assert!(!summaries
        .iter()
        .any(|summary| summary.thread_id == other_thread));

    let (base_url, history_server) = spawn_server(200, session_body.into());
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    let history = boundaries
        .thread_history(ThreadOpenRequest {
            thread_id: run_thread.clone(),
            limit: 10,
            cursor: None,
        })
        .unwrap();
    history_server.join().unwrap();
    assert_eq!(history.entries.len(), 1);
    assert_eq!(history.entries[0].run_id, run_id);

    let (base_url, other_history_server) = spawn_server(200, session_body.into());
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    assert_eq!(
        boundaries
            .thread_history(ThreadOpenRequest {
                thread_id: other_thread,
                limit: 10,
                cursor: None,
            })
            .err()
            .unwrap(),
        ProtocolError::persistence_failed_with_reason(
            "Conversation history journal operation failed: ThreadNotOwned"
        )
    );
    other_history_server.join().unwrap();

    let devices_body = r#"{"devices":[{"device_id":"10000000-0000-4000-8000-000000000001","client_id":"muniment-desktop","client_role":"desktop","platform":"desktop","created_at":"2026-08-01T10:00:00Z","revoked_at":null,"last_active_at":"2026-08-12T12:00:00Z","current":true}]}"#;
    let (base_url, devices_server) =
        spawn_server_sequence(vec![(200, session_body.into()), (200, devices_body.into())]);
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    let devices = boundaries.list_devices().unwrap();
    assert_eq!(devices.devices.len(), 1);
    assert!(devices.devices[0].current);
    let device_requests = devices_server.join().unwrap();
    assert_eq!(device_requests.len(), 2);
    let devices_request = device_requests[1].to_ascii_lowercase();
    assert!(devices_request.contains("authorization: bearer access-secret\r\n"));

    let (base_url, rename_server) = spawn_server(200, session_body.into());
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    boundaries
        .rename_thread(&run_thread, "New title", provenance())
        .unwrap();
    rename_server.join().unwrap();
    let (base_url, delete_server) = spawn_server(200, session_body.into());
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    boundaries.delete_thread(&run_thread, provenance()).unwrap();
    delete_server.join().unwrap();
    std::env::remove_var("MUNIMENT_API_BASE_URL");
    credential_store.clear_session().unwrap();
    assert_eq!(
        boundaries.list_devices().unwrap_err(),
        ProtocolError::unauthorized()
    );
    assert_eq!(
        boundaries
            .thread_summaries(ThreadListRequest {
                limit: 10,
                cursor: None,
            })
            .unwrap_err(),
        ProtocolError::unauthorized()
    );
    assert_eq!(
        boundaries
            .thread_history(ThreadOpenRequest {
                thread_id: run_thread.clone(),
                limit: 10,
                cursor: None,
            })
            .err()
            .unwrap(),
        ProtocolError::unauthorized()
    );
    credential_store
        .save_credentials(&credentials_with_expiry(0))
        .unwrap();
    assert_eq!(
        boundaries.list_devices().unwrap_err(),
        ProtocolError::unauthorized()
    );
    credential_store.clear_session().unwrap();

    let mutation_events = storage
        .lock()
        .unwrap()
        .journal
        .thread_events(&run_thread)
        .unwrap();
    assert_eq!(mutation_events[1].event_type, "thread.title.renamed");
    assert_eq!(mutation_events[1].provenance.source, "muniment-runtime");
    assert_eq!(mutation_events[2].event_type, "thread.deleted");
    assert_eq!(mutation_events[2].provenance.source, "muniment-runtime");

    let stream = boundaries.stream_run("workspace-a", run_id, 0).unwrap();
    assert_eq!(stream.current_run_seq, current_run_seq);

    let subscription = boundaries.subscribe_run_commits(run_id).unwrap();
    assert_eq!(subscription.committed_high_water, current_run_seq);
    assert_eq!(
        boundaries.subscribe_run_commits("unknown-run").unwrap_err(),
        ProtocolError::thread_not_found()
    );

    thread::scope(|scope| {
        let answer = scope.spawn(|| {
            boundaries.queue_attach_permission_answer(
                "workspace-a",
                "run-1",
                "gate-1",
                ChatPermissionAnswer::Confirm(true),
            )
        });
        let queued = loop {
            if let Some(queued) = permission_answers.lock().unwrap().pop_front() {
                break queued;
            }
            thread::yield_now();
        };
        assert_eq!(queued.gate_id, "gate-1");
        queued.resolved.unwrap().send(Some(7)).unwrap();
        assert_eq!(answer.join().unwrap().unwrap().recv().unwrap(), Some(7));
    });
    for delivery in [ChatDelivery::Steer, ChatDelivery::FollowUp] {
        assert!(matches!(
            boundaries.queue_attach_message("workspace-a", "missing-run", delivery, "message"),
            Err(muniment_core::run_start::RunStartError::InvalidRequest(_))
        ));
    }
    assert!(matches!(
        boundaries.queue_attach_permission_answer(
            "workspace-a",
            "missing-run",
            "gate-1",
            ChatPermissionAnswer::Confirm(true),
        ),
        Err(muniment_core::run_start::RunStartError::InvalidRequest(_))
    ));

    drop(boundaries);
    drop(storage);
}

#[test]
fn runtime_boundaries_fetch_only_readable_workspace_artifacts() {
    let temporary_profile = TemporaryProfile::new("attach-artifacts", false);
    let profile = temporary_profile.profile.clone();
    let storage = open_profile_storage(&profile).unwrap();
    let config = temporary_profile.config.clone();
    let boundaries = RuntimeAttachBoundaries::new(
        Arc::clone(&storage),
        Arc::new(Mutex::new(None)),
        profile.clone(),
        config.clone(),
        Arc::new(Mutex::new(None::<PiRuntime>)),
        Arc::new(ApplicationMemoryRuntime::new(
            config,
            profile.join("memory"),
        )),
        RuntimeActivityRegistry::new(),
        Arc::new(EntitlementSnapshotTracker::new()),
        SignedWorkspaceApproval::default(),
        Arc::new(SessionThread::default()),
        open_companion_registry(&profile).unwrap(),
    );
    let cases = [
        (
            "01900000-0000-7000-8000-000000000101",
            "01900000-0000-7000-8000-000000000201",
            "workspace-a",
            b"success".as_slice(),
        ),
        (
            "01900000-0000-7000-8000-000000000102",
            "01900000-0000-7000-8000-000000000202",
            "workspace-b",
            b"foreign".as_slice(),
        ),
        (
            "01900000-0000-7000-8000-000000000104",
            "01900000-0000-7000-8000-000000000204",
            "workspace-a",
            b"missing".as_slice(),
        ),
        (
            "01900000-0000-7000-8000-000000000105",
            "01900000-0000-7000-8000-000000000205",
            "workspace-a",
            b"corrupt".as_slice(),
        ),
        (
            "01900000-0000-7000-8000-000000000106",
            "01900000-0000-7000-8000-000000000206",
            "workspace-a",
            b"unreadable".as_slice(),
        ),
    ];
    let mut hashes = BTreeMap::new();
    {
        let mut storage = storage.lock().unwrap();
        for (event_id, run_id, workspace, bytes) in cases {
            let hash = storage.cas.put(bytes).unwrap();
            storage
                .journal
                .append(
                    0,
                    &artifact_event(
                        event_id,
                        run_id,
                        1,
                        EventPayload::Cas {
                            payload_cas: CasReference {
                                sha256: hash.to_string(),
                                media_type: "application/octet-stream".into(),
                                byte_length: bytes.len() as u64,
                            },
                        },
                    ),
                )
                .unwrap();
            storage
                .journal
                .bind_run_workspace(run_id, workspace)
                .unwrap();
            hashes.insert(event_id, hash);
        }
        storage
            .journal
            .append(
                0,
                &artifact_event(
                    "01900000-0000-7000-8000-000000000103",
                    "01900000-0000-7000-8000-000000000203",
                    1,
                    EventPayload::Inline {
                        payload_json: "not an artifact".into(),
                    },
                ),
            )
            .unwrap();
        storage
            .journal
            .bind_run_workspace("01900000-0000-7000-8000-000000000203", "workspace-a")
            .unwrap();
    }

    let success_id = Id::new("01900000-0000-7000-8000-000000000101").unwrap();
    let success = boundaries
        .fetch_artifact("workspace-a", &success_id)
        .unwrap();
    assert_eq!(success.total_bytes, 7);
    assert_eq!(success.sha256, hashes[success_id.as_str()].to_string());
    assert_eq!(
        RunAttachBoundaries::read_artifact_range(&boundaries, "workspace-a", &success_id, 2, 3)
            .unwrap(),
        b"cce"
    );

    for id in [
        "01900000-0000-7000-8000-000000000199",
        "01900000-0000-7000-8000-000000000102",
        "01900000-0000-7000-8000-000000000103",
    ] {
        assert_invalid_artifact(boundaries.fetch_artifact("workspace-a", &Id::new(id).unwrap()));
    }

    let object_path = |id: &str| {
        let hash = hashes[id].as_str();
        profile
            .join("cas/objects")
            .join(&hash[..2])
            .join(&hash[2..])
    };
    let missing_id = "01900000-0000-7000-8000-000000000104";
    fs::remove_file(object_path(missing_id)).unwrap();
    assert_invalid_artifact(
        boundaries.fetch_artifact("workspace-a", &Id::new(missing_id).unwrap()),
    );

    let corrupt_id = "01900000-0000-7000-8000-000000000105";
    fs::write(object_path(corrupt_id), b"CORRUPT").unwrap();
    assert_invalid_artifact(
        boundaries.fetch_artifact("workspace-a", &Id::new(corrupt_id).unwrap()),
    );

    let unreadable_id = "01900000-0000-7000-8000-000000000106";
    let unreadable_path = object_path(unreadable_id);
    fs::remove_file(&unreadable_path).unwrap();
    std::os::unix::fs::symlink(unreadable_path.file_name().unwrap(), &unreadable_path).unwrap();
    assert_invalid_artifact(
        boundaries.fetch_artifact("workspace-a", &Id::new(unreadable_id).unwrap()),
    );
}

#[test]
fn runtime_boundaries_share_owner_approval_and_session_thread() {
    let _guard = TEST_LOCK.lock().unwrap();
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let temporary_profile = TemporaryProfile::new("attach-approval", false);
    let profile = temporary_profile.profile.clone();
    let config = temporary_profile.config.clone();
    let storage = open_profile_storage(&profile).unwrap();
    let approval = SignedWorkspaceApproval::default();
    let session_thread = Arc::new(SessionThread::default());
    let boundaries = RuntimeAttachBoundaries::new(
        Arc::clone(&storage),
        Arc::new(Mutex::new(None)),
        profile.clone(),
        config.clone(),
        Arc::new(Mutex::new(None::<PiRuntime>)),
        Arc::new(ApplicationMemoryRuntime::new(
            config.clone(),
            profile.join("memory"),
        )),
        RuntimeActivityRegistry::new(),
        Arc::new(EntitlementSnapshotTracker::new()),
        approval.clone(),
        Arc::clone(&session_thread),
        open_companion_registry(&profile).unwrap(),
    );
    let sibling = RuntimeAttachBoundaries::new(
        Arc::clone(&storage),
        Arc::new(Mutex::new(None)),
        profile.clone(),
        config.clone(),
        Arc::new(Mutex::new(None::<PiRuntime>)),
        Arc::new(ApplicationMemoryRuntime::new(
            config,
            profile.join("memory"),
        )),
        RuntimeActivityRegistry::new(),
        Arc::new(EntitlementSnapshotTracker::new()),
        approval,
        Arc::clone(&session_thread),
        open_companion_registry(&profile).unwrap(),
    );

    boundaries
        .signed_workspace_approval()
        .record("workspace-a".into());

    let approval = sibling.attach_approval().unwrap();
    assert_eq!(approval.profile, "desktop-owner");
    assert_eq!(approval.workspace, "workspace-a");

    sibling.clear_workspace();
    assert!(boundaries.attach_approval().is_none());

    let initial_run_id = "01900000-0000-7000-8000-000000000020";
    prepare_new_run_with_session_thread(
        &storage,
        SessionThreadStart {
            tracker: &SessionThread::default(),
            continue_existing: false,
        },
        initial_run_id,
        "workspace-a",
        Some("user"),
        Vec::new(),
        None,
        "test",
        "1",
        || Ok(()),
    )
    .unwrap();
    let thread_id = storage
        .lock()
        .unwrap()
        .journal
        .run_thread_id(initial_run_id)
        .unwrap()
        .unwrap();
    session_thread.record(thread_id.clone(), "workspace-a", Some("user"));
    let tokens = common::credentials().tokens;
    for (boundary, run_id) in [
        (&boundaries, "01900000-0000-7000-8000-000000000021"),
        (&sibling, "01900000-0000-7000-8000-000000000022"),
    ] {
        boundary
            .prepare_run(
                run_id,
                "prompt",
                &common::fixture_grant(),
                &tokens,
                Vec::new(),
                None,
                None,
            )
            .unwrap();
        assert_eq!(
            storage
                .lock()
                .unwrap()
                .journal
                .run_thread_id(run_id)
                .unwrap()
                .as_deref(),
            Some(thread_id.as_str())
        );
    }
}
