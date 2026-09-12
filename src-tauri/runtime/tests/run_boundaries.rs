#![cfg(target_os = "linux")]

use std::collections::BTreeMap;
use std::io::Write;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use muniment_core::attach::linux::{CompanionProvenance, RunSubmitRequest, ThreadListService};
use muniment_core::attach::{Id, RuntimeActivityRegistry, SignedWorkspaceApproval};
use muniment_core::auth::{
    EntitlementSnapshotTracker, KeyringNativeCredentialStore, NativeCredentialStore,
};
use muniment_core::chat_view::SelectedFile;
use muniment_core::journal::thread_mutation::create_thread_now;
use muniment_core::journal::Provenance;
use muniment_core::memory_runtime::ApplicationMemoryRuntime;
use muniment_core::pi_execution::PiRuntime;
use muniment_core::run_start::{
    prepare_desktop_run, RunStartBoundaries, RunStartError, RunStartRequest,
};
use muniment_core::session_thread::SessionThread;
use muniment_runtime::{open_profile_storage, RuntimeAttachBoundaries, RuntimeAttachState};

mod common;
use common::{credentials, stage_pi_stub, TemporaryProfile};

static ENVIRONMENT: Mutex<()> = Mutex::new(());

struct TestEnvironment {
    name: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl TestEnvironment {
    fn set(name: &'static str, value: &std::path::Path) -> Self {
        let previous = std::env::var_os(name);
        std::env::set_var(name, value);
        Self { name, previous }
    }
}

impl Drop for TestEnvironment {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.name, value),
            None => std::env::remove_var(self.name),
        }
    }
}

#[test]
fn local_mode_prepares_a_journaled_run_without_native_auth_or_a_cloud_grant() {
    let _environment = ENVIRONMENT.lock().unwrap();
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let store = KeyringNativeCredentialStore::new();
    store.clear_session().unwrap();
    store.save_credentials(&credentials()).unwrap();
    let server = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    server.set_nonblocking(true).unwrap();
    std::env::set_var(
        "MUNIMENT_API_BASE_URL",
        format!("http://{}", server.local_addr().unwrap()),
    );
    let temporary_profile = TemporaryProfile::new("local-run-boundaries", true);
    std::fs::write(
        temporary_profile
            .config
            .join(muniment_core::local_mode::LOCAL_MODE_MARKER),
        "1",
    )
    .unwrap();
    let storage = open_profile_storage(&temporary_profile.profile).unwrap();
    let boundaries = RuntimeAttachBoundaries::new(
        Arc::clone(&storage),
        Arc::new(Mutex::new(None)),
        temporary_profile.profile.clone(),
        temporary_profile.config.clone(),
        Arc::new(Mutex::new(None::<PiRuntime>)),
        Arc::new(ApplicationMemoryRuntime::new(
            temporary_profile.config.clone(),
            temporary_profile.profile.join("memory"),
        )),
        RuntimeActivityRegistry::new(),
        Arc::new(EntitlementSnapshotTracker::new()),
        SignedWorkspaceApproval::default(),
        Arc::new(SessionThread::default()),
        muniment_runtime::open_companion_registry(&temporary_profile.profile).unwrap(),
    );

    let (result, launch) = prepare_desktop_run(
        &boundaries,
        RunStartRequest {
            prompt: "hello".into(),
            files: Vec::new(),
            workspace: None,
            provenance: None,
            thread_id: None,
        },
    )
    .unwrap();

    assert!(launch.grant.is_local());
    assert!(launch.tokens.access_token.is_empty());
    let events = storage
        .lock()
        .unwrap()
        .journal
        .events(&result.run_id)
        .unwrap();
    assert!(!events.is_empty());
    assert!(events.iter().all(|event| event.envelope_version == 1));
    assert!(events
        .iter()
        .all(|event| event.provenance.source == "muniment-runtime"));
    assert_eq!(
        server.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    std::env::remove_var("MUNIMENT_API_BASE_URL");
    store.clear_session().unwrap();
}

#[test]
fn local_mode_runs_pi_and_journals_the_signed_in_event_shapes() {
    local_mode_run(true);
}

#[test]
fn local_mode_submit_without_a_configured_home_creates_the_default_home_and_replies() {
    local_mode_run(false);
}

fn local_mode_run(configured: bool) {
    let _environment = ENVIRONMENT.lock().unwrap();
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let profile = TemporaryProfile::new("local-pi-run", configured);
    std::fs::write(
        profile
            .config
            .join(muniment_core::local_mode::LOCAL_MODE_MARKER),
        "1",
    )
    .unwrap();
    let descriptor = stage_pi_stub(&profile.profile);
    let documents = profile.root.join("Documents");
    std::fs::create_dir_all(&documents).unwrap();
    let _home = TestEnvironment::set("HOME", &profile.root);
    let _xdg_config = TestEnvironment::set("XDG_CONFIG_HOME", &profile.config);
    let expected_home = if configured {
        profile.root.join("home")
    } else {
        assert!(!profile.config.join("home.json").exists());
        assert!(!documents.join("Muniment").exists());
        muniment_core::home::choose_default_home(Some(documents), Some(profile.root.clone()))
            .unwrap()
    };
    std::env::remove_var("MUNIMENT_PI_ROOT");
    let prompt_capture = profile.root.join("prompt.txt");
    std::env::set_var("PI_RESUME_STUB_PROMPTS", &prompt_capture);
    std::env::set_var("PI_RESUME_STUB_TOOL_EVENTS", "1");
    let state = RuntimeAttachState::open(&profile.profile, &profile.config).unwrap();
    let launch_boundaries = state.boundaries().with_pi_artifact(descriptor);
    let boundaries = state.boundaries();
    let mut service = muniment_runtime::compose_attach_service(
        launch_boundaries,
        state.companion_registry(),
        &profile.profile,
        &profile.config,
    )
    .unwrap();

    let chat_events = service.subscribe_chat_events().unwrap();
    assert!(state.signed_workspace_approval().approval().is_none());
    let submitted = service
        .submit_run(
            "",
            RunSubmitRequest {
                text: "local prompt".into(),
                files: Vec::new(),
                thread_id: None,
            },
            &Id::new("018f0000-0000-7000-8000-000000000021").unwrap(),
            &Id::new("018f0000-0000-7000-8000-000000000022").unwrap(),
            CompanionProvenance {
                profile: "default".into(),
                companion_kind: "desktop".into(),
                companion_version: "test".into(),
                peer_uid: 1000,
                peer_pid: 42,
            },
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while boundaries.active_run_exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    std::env::remove_var("PI_RESUME_STUB_PROMPTS");
    std::env::remove_var("PI_RESUME_STUB_TOOL_EVENTS");
    assert!(!boundaries.active_run_exists());
    assert!(!submitted.run_id.is_empty());
    assert_eq!(
        muniment_core::home::configured_home(&profile.config).unwrap(),
        Some(expected_home.clone())
    );
    for folder in ["memory", "agents", "projects", "sessions"] {
        assert!(expected_home.join(folder).join("README.md").is_file());
    }
    assert_eq!(
        std::fs::read_to_string(prompt_capture).unwrap().trim(),
        "local prompt"
    );

    let delivered = std::iter::from_fn(|| chat_events.try_recv().ok()).collect::<Vec<_>>();
    assert!(delivered
        .iter()
        .any(|event| event.phase == "streaming" && event.text == " resumed"));
    let completed = delivered.last().unwrap();
    assert_eq!(completed.phase, "complete");
    assert_eq!(completed.text, " resumed");
    assert!(completed.receipt.is_some());
    assert!(delivered
        .iter()
        .all(|event| event.run_id == submitted.run_id
            && event.thread_id.as_deref() == Some(submitted.thread_id.as_str())));
    assert!(state.signed_workspace_approval().approval().is_none());

    let storage = open_profile_storage(&profile.profile).unwrap();
    let events = storage
        .lock()
        .unwrap()
        .journal
        .events(&submitted.run_id)
        .unwrap();
    let event_types = events
        .iter()
        .map(|event| event.event_type.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        event_types,
        [
            "run.started",
            "runtime.pi_session.bound",
            "model.prompt.accepted",
            "model.stream.delta",
            "tool.effect.started",
            "tool.effect.completed",
            "run.completed",
        ]
    );
    assert!(events.iter().all(|event| event.envelope_version == 1));
    assert!(events
        .iter()
        .all(|event| event.provenance.source == "muniment-runtime"));
    let payloads = events
        .iter()
        .map(|event| match &event.payload {
            muniment_core::journal::EventPayload::Inline { payload_json } => payload_json,
            _ => panic!("the local Pi stub emits only inline events"),
        })
        .collect::<Vec<_>>();
    assert!(payloads[2].as_object().unwrap().is_empty());
    assert_eq!(payloads[3]["text"].as_str(), Some(" resumed"));
    assert_eq!(payloads[4]["effect_id"].as_str(), Some("tool-1"));
    assert_eq!(payloads[4]["display_name"].as_str(), Some("read"));
    assert_eq!(payloads[5]["effect_id"].as_str(), Some("tool-1"));
    assert_eq!(payloads[5].as_object().unwrap().len(), 1);
    let receipt = payloads[6]["receipt"].as_object().unwrap();
    assert_eq!(receipt.len(), 1);
    let elapsed = receipt["time"].as_str().unwrap();
    assert!(elapsed.ends_with('s'));
    assert!(elapsed.trim_end_matches('s').parse::<f64>().is_ok());

    let cause = "Reply delivery failed. The desktop missed the five-second chat.event frame deadline with 17 bytes pending.";
    let connected = service.subscribe_chat_events().unwrap();
    service.record_chat_delivery_failure(&submitted.run_id, cause);
    assert_eq!(
        connected.try_recv().unwrap().failure_reason.as_deref(),
        Some(cause)
    );
    // A failed subscription response drops the receiver before the desktop reads its cause.
    drop(service.subscribe_chat_events().unwrap());
    let reconnected = service.subscribe_chat_events().unwrap();
    let concurrent = service.subscribe_chat_events().unwrap();
    let failure = reconnected.try_recv().unwrap();
    assert_eq!(failure.run_id, submitted.run_id);
    assert_eq!(
        failure.thread_id.as_deref(),
        Some(submitted.thread_id.as_str())
    );
    assert_eq!(failure.phase, "delivery-failed");
    assert_eq!(failure.failure_reason.as_deref(), Some(cause));
    assert!(reconnected.try_recv().is_err());
    let assert_failure = |event: muniment_core::run_events::ChatEvent| {
        assert_eq!(event.run_id, failure.run_id);
        assert_eq!(event.thread_id, failure.thread_id);
        assert_eq!(event.phase, failure.phase);
        assert_eq!(event.failure_reason, failure.failure_reason);
    };
    assert_failure(concurrent.try_recv().unwrap());
    drop(reconnected);
    assert_failure(service.subscribe_chat_events().unwrap().try_recv().unwrap());
    let restored = storage
        .lock()
        .unwrap()
        .journal
        .events(&submitted.run_id)
        .unwrap();
    assert_eq!(restored, events);

    service.record_chat_delivery_failure("missing-run", cause);
    assert_failure(service.subscribe_chat_events().unwrap().try_recv().unwrap());
    service.record_chat_delivery_failure(&submitted.run_id, cause);
    std::fs::remove_file(
        profile
            .config
            .join(muniment_core::local_mode::LOCAL_MODE_MARKER),
    )
    .unwrap();
    assert!(service.subscribe_chat_events().unwrap().try_recv().is_err());
    std::fs::write(
        profile
            .config
            .join(muniment_core::local_mode::LOCAL_MODE_MARKER),
        "1",
    )
    .unwrap();
    assert_failure(service.subscribe_chat_events().unwrap().try_recv().unwrap());
}

#[test]
fn workspace_less_submit_records_the_resolved_grant_workspace() {
    let _environment = ENVIRONMENT.lock().unwrap();
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let store = KeyringNativeCredentialStore::new();
    store.clear_session().unwrap();
    store.save_credentials(&credentials()).unwrap();

    let server = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let base_url = format!("http://{}", server.local_addr().unwrap());
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    let responses = std::thread::spawn(move || {
        serve_grants(server, 2);
    });

    let profile = TemporaryProfile::new("attach-submit", true);
    let descriptor = stage_pi_stub(&profile.root);
    std::env::set_var("PI_RESUME_STUB_PROMPT_DELAY_MS", "500");
    let state = RuntimeAttachState::open(&profile.profile, &profile.config).unwrap();
    let mut service = muniment_runtime::compose_attach_service(
        state.boundaries().with_pi_artifact(descriptor),
        state.companion_registry(),
        &profile.profile,
        &profile.config,
    )
    .unwrap();
    let first = service
        .submit_run(
            "",
            RunSubmitRequest {
                text: "hello".into(),
                files: Vec::new(),
                thread_id: None,
            },
            &Id::new("018f0000-0000-7000-8000-000000000011").unwrap(),
            &Id::new("018f0000-0000-7000-8000-000000000012").unwrap(),
            CompanionProvenance {
                profile: "default".into(),
                companion_kind: "desktop".into(),
                companion_version: "test".into(),
                peer_uid: 1000,
                peer_pid: 42,
            },
        )
        .unwrap();
    assert!(!first.run_id.is_empty());
    assert!(!first.thread_id.is_empty());
    assert!(first.committed_seq > 0);
    assert_eq!(
        state
            .signed_workspace_approval()
            .approval()
            .unwrap()
            .workspace,
        "local"
    );

    let boundaries = state.boundaries();
    assert!(boundaries.active_run_exists());
    let deadline = Instant::now() + Duration::from_secs(10);
    while boundaries.active_run_exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!boundaries.active_run_exists());

    let second = service
        .submit_run(
            "local",
            RunSubmitRequest {
                text: "hello again".into(),
                files: Vec::new(),
                thread_id: None,
            },
            &Id::new("018f0000-0000-7000-8000-000000000013").unwrap(),
            &Id::new("018f0000-0000-7000-8000-000000000014").unwrap(),
            CompanionProvenance {
                profile: "default".into(),
                companion_kind: "desktop".into(),
                companion_version: "test".into(),
                peer_uid: 1000,
                peer_pid: 42,
            },
        )
        .unwrap();
    assert_ne!(first.thread_id, second.thread_id);
    let deadline = Instant::now() + Duration::from_secs(10);
    while boundaries.active_run_exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!boundaries.active_run_exists());
    let storage = open_profile_storage(&profile.profile).unwrap();
    assert_eq!(
        storage
            .lock()
            .unwrap()
            .journal
            .run_thread_id(&first.run_id)
            .unwrap()
            .unwrap(),
        first.thread_id
    );
    assert_eq!(
        storage
            .lock()
            .unwrap()
            .journal
            .run_thread_id(&second.run_id)
            .unwrap()
            .unwrap(),
        second.thread_id
    );
    responses.join().unwrap();
    store.clear_session().unwrap();
    std::env::remove_var("MUNIMENT_API_BASE_URL");
    std::env::remove_var("MUNIMENT_PI_ROOT");
    std::env::remove_var("PI_RESUME_STUB_PROMPT_DELAY_MS");
}

#[test]
fn attach_dispatch_submits_to_an_explicit_thread() {
    let _environment = ENVIRONMENT.lock().unwrap();
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let store = KeyringNativeCredentialStore::new();
    store.clear_session().unwrap();
    store.save_credentials(&credentials()).unwrap();

    let server = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let base_url = format!("http://{}", server.local_addr().unwrap());
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    let responses = std::thread::spawn(move || {
        serve_grants(server, 2);
    });

    let profile = TemporaryProfile::new("attach-explicit-thread", true);
    let descriptor = stage_pi_stub(&profile.root);
    let state = RuntimeAttachState::open(&profile.profile, &profile.config).unwrap();
    let mut service = muniment_runtime::compose_attach_service(
        state.boundaries().with_pi_artifact(descriptor),
        state.companion_registry(),
        &profile.profile,
        &profile.config,
    )
    .unwrap();
    let first = service
        .submit_run(
            "local",
            RunSubmitRequest {
                text: "first".into(),
                files: Vec::new(),
                thread_id: None,
            },
            &Id::new("018f0000-0000-7000-8000-000000000015").unwrap(),
            &Id::new("018f0000-0000-7000-8000-000000000016").unwrap(),
            CompanionProvenance {
                profile: "default".into(),
                companion_kind: "desktop".into(),
                companion_version: "test".into(),
                peer_uid: 1000,
                peer_pid: 42,
            },
        )
        .unwrap();
    let boundaries = state.boundaries();
    let deadline = Instant::now() + Duration::from_secs(10);
    while boundaries.active_run_exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!boundaries.active_run_exists());

    let accepted = service
        .submit_run(
            "local",
            RunSubmitRequest {
                text: "hello".into(),
                files: Vec::new(),
                thread_id: Some(first.thread_id.clone()),
            },
            &Id::new("018f0000-0000-7000-8000-000000000017").unwrap(),
            &Id::new("018f0000-0000-7000-8000-000000000018").unwrap(),
            CompanionProvenance {
                profile: "default".into(),
                companion_kind: "desktop".into(),
                companion_version: "test".into(),
                peer_uid: 1000,
                peer_pid: 42,
            },
        )
        .unwrap();
    assert_eq!(accepted.thread_id, first.thread_id);

    let deadline = Instant::now() + Duration::from_secs(10);
    while boundaries.active_run_exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!boundaries.active_run_exists());
    let storage = open_profile_storage(&profile.profile).unwrap();
    assert_eq!(
        storage
            .lock()
            .unwrap()
            .journal
            .run_thread_id(&accepted.run_id)
            .unwrap()
            .unwrap(),
        accepted.thread_id
    );
    responses.join().unwrap();
    store.clear_session().unwrap();
    std::env::remove_var("MUNIMENT_API_BASE_URL");
    std::env::remove_var("MUNIMENT_PI_ROOT");
}

#[test]
fn runtime_boundaries_prepare_a_desktop_run() {
    let _environment = ENVIRONMENT.lock().unwrap();
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let store = KeyringNativeCredentialStore::new();
    store.clear_session().unwrap();
    store.save_credentials(&credentials()).unwrap();

    let server = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let base_url = format!("http://{}", server.local_addr().unwrap());
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    let responses = std::thread::spawn(move || {
        serve_grants(server, 1);
    });

    let temporary_profile = TemporaryProfile::new("run-boundaries", true);
    let root = temporary_profile.root.clone();
    let profile = temporary_profile.profile.clone();
    let config = temporary_profile.config.clone();
    let storage = open_profile_storage(&profile).unwrap();
    let thread_id = create_thread_now(
        &mut storage.lock().unwrap().journal,
        "local",
        Provenance {
            source: "test".into(),
            source_version: "1".into(),
            actor_id: None,
            device_id: None,
            rpc_request_id: None,
            capability_versions: None,
            extra: BTreeMap::from([("attach_profile".into(), "profile-a".into())]),
        },
    )
    .unwrap();
    let activity = RuntimeActivityRegistry::new();
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
        activity,
        Arc::new(EntitlementSnapshotTracker::new()),
        SignedWorkspaceApproval::default(),
        Arc::new(SessionThread::default()),
        muniment_runtime::open_companion_registry(&profile).unwrap(),
    );
    assert!(matches!(
        boundaries.prepare_run(
            "01900000-0000-7000-8000-000000000001",
            "hello",
            &common::fixture_grant(),
            &credentials().tokens,
            vec![SelectedFile { path: root.clone() }],
            None,
            None,
        ),
        Err(RunStartError::Persistence(_))
    ));

    let (result, launch) = prepare_desktop_run(
        &boundaries,
        RunStartRequest {
            prompt: "hello".into(),
            files: Vec::new(),
            workspace: Some("local".into()),
            provenance: Some(Provenance {
                source: "test".into(),
                source_version: "1".into(),
                actor_id: Some("user".into()),
                device_id: None,
                rpc_request_id: None,
                capability_versions: None,
                extra: BTreeMap::from([("attach_profile".into(), "profile-a".into())]),
            }),
            thread_id: Some(thread_id.clone()),
        },
    )
    .unwrap();

    assert_eq!(result.run_id, launch.run_id);
    assert_eq!(result.committed_seq, launch.prepared.0);
    assert_eq!(
        storage
            .lock()
            .unwrap()
            .journal
            .run_thread_id(&result.run_id)
            .unwrap()
            .unwrap(),
        thread_id
    );

    responses.join().unwrap();
    store.clear_session().unwrap();
    std::env::remove_var("MUNIMENT_API_BASE_URL");
}

#[test]
fn runtime_service_broadcasts_a_driven_prompts_chat_events() {
    let _environment = ENVIRONMENT.lock().unwrap();
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let store = KeyringNativeCredentialStore::new();
    store.clear_session().unwrap();
    store.save_credentials(&credentials()).unwrap();

    let server = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let base_url = format!("http://{}", server.local_addr().unwrap());
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    let responses = std::thread::spawn(move || {
        serve_grants(server.try_clone().unwrap(), 1);
        let (mut stream, _) = server.accept().unwrap();
        let request = read_request(&mut stream);
        assert!(request.starts_with("POST /v1/chat/receipts "), "{request}");
        let body = r#"{"route":"test","model":"muniment-stub-chat"}"#;
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    });

    let profile = TemporaryProfile::new("run-chat-events", true);
    let descriptor = stage_pi_stub(&profile.root);
    let state = RuntimeAttachState::open(&profile.profile, &profile.config).unwrap();
    let boundaries = state.boundaries().with_pi_artifact(descriptor);
    let mut subscription_service = state.attach_service().unwrap();
    let events = subscription_service.subscribe_chat_events().unwrap();
    let mut second_subscription_service = state.attach_service().unwrap();
    let second_events = second_subscription_service.subscribe_chat_events().unwrap();
    let (result, launch) = prepare_desktop_run(
        &boundaries,
        RunStartRequest {
            prompt: "hello".into(),
            files: Vec::new(),
            workspace: Some("local".into()),
            provenance: None,
            thread_id: None,
        },
    )
    .unwrap();

    boundaries.launch(launch);

    let event = events.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(event.run_id, result.run_id);
    let thread_id = boundaries.run_thread_id(&result.run_id).unwrap();
    assert_eq!(event.thread_id.as_deref(), Some(thread_id.as_str()));
    let second_event = second_events.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(second_event.run_id, result.run_id);
    assert_eq!(second_event.thread_id.as_deref(), Some(thread_id.as_str()));
    let deadline = Instant::now() + Duration::from_secs(10);
    while boundaries.active_run_exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!boundaries.active_run_exists());
    assert!(std::iter::from_fn(|| events.try_recv().ok()).any(|event| event.phase == "complete"));

    responses.join().unwrap();
    store.clear_session().unwrap();
    std::env::remove_var("MUNIMENT_API_BASE_URL");
    std::env::remove_var("MUNIMENT_PI_ROOT");
}

#[test]
fn attach_submit_reaches_a_chat_event_subscriber() {
    let _environment = ENVIRONMENT.lock().unwrap();
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let store = KeyringNativeCredentialStore::new();
    store.clear_session().unwrap();
    store.save_credentials(&credentials()).unwrap();

    let server = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let base_url = format!("http://{}", server.local_addr().unwrap());
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    let responses = std::thread::spawn(move || {
        serve_grants(server, 1);
    });

    let profile = TemporaryProfile::new("attach-submit-chat-events", true);
    let descriptor = stage_pi_stub(&profile.root);
    let state = RuntimeAttachState::open(&profile.profile, &profile.config).unwrap();
    let boundaries = state.boundaries();
    let mut subscription_service = state.attach_service().unwrap();
    let events = subscription_service.subscribe_chat_events().unwrap();
    let mut service = muniment_runtime::compose_attach_service(
        state.boundaries().with_pi_artifact(descriptor),
        state.companion_registry(),
        &profile.profile,
        &profile.config,
    )
    .unwrap();
    let accepted = service
        .submit_run(
            "local",
            RunSubmitRequest {
                text: "hello".into(),
                files: Vec::new(),
                thread_id: None,
            },
            &Id::new("018f0000-0000-7000-8000-000000000019").unwrap(),
            &Id::new("018f0000-0000-7000-8000-000000000020").unwrap(),
            CompanionProvenance {
                profile: "default".into(),
                companion_kind: "desktop".into(),
                companion_version: "test".into(),
                peer_uid: 1000,
                peer_pid: 42,
            },
        )
        .unwrap();

    let event = events.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(event.run_id, accepted.run_id);
    assert_eq!(
        event.thread_id.as_deref(),
        Some(accepted.thread_id.as_str())
    );

    let deadline = Instant::now() + Duration::from_secs(10);
    while boundaries.active_run_exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!boundaries.active_run_exists());

    responses.join().unwrap();
    store.clear_session().unwrap();
    std::env::remove_var("MUNIMENT_API_BASE_URL");
    std::env::remove_var("MUNIMENT_PI_ROOT");
}

fn session_body() -> String {
    r#"{"session":{"org_id":"20000000-0000-4000-8000-000000000002","user_id":"30000000-0000-4000-8000-000000000003","role":"owner","device_id":"10000000-0000-4000-8000-000000000001","client_role":"desktop","expires_at":"2099-01-01T00:00:00Z"},"user":{"id":"30000000-0000-4000-8000-000000000003","email":"user@example.com","status":"active","role":"owner","entitlement_version":7},"org":{"id":"20000000-0000-4000-8000-000000000002","display_name":"Muniment"},"entitlement_snapshot":{"payload":{"org_id":"20000000-0000-4000-8000-000000000002","user_id":"30000000-0000-4000-8000-000000000003","entitlement_version":7,"issued_at":"2026-08-01T00:00:00Z","capabilities":[],"grants":[]},"signature":"signature-secret","algorithm":"hmac-sha256"}}"#.into()
}

fn serve_grants(server: TcpListener, runs: usize) {
    for _ in 0..runs {
        for (status, body, path) in [
            (200, session_body(), "GET /v1/auth/native/session "),
            (201, common::grant_body(), "POST /v1/chat/grants "),
        ] {
            loop {
                let (mut stream, _) = server.accept().unwrap();
                let request = read_request(&mut stream);
                let receipt = request.starts_with("POST /v1/chat/receipts ");
                if !receipt {
                    assert!(request.starts_with(path), "{request}");
                }
                let (status, body) = if receipt {
                    (200, "{\"route\":\"test\",\"model\":\"muniment-stub-chat\"}")
                } else {
                    (status, body.as_str())
                };
                write!(stream, "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                if !receipt {
                    break;
                }
            }
        }
    }
}

fn read_request(stream: &mut std::net::TcpStream) -> String {
    common::read_request(stream)
}
