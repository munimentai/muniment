#![cfg(target_os = "linux")]

use std::collections::BTreeMap;
use std::io::{Read, Write};
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
        for body in [session_body(), grant_body(), session_body(), grant_body()] {
            let (mut stream, _) = server.accept().unwrap();
            read_request(&mut stream);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
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
        "workspace-a"
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
            "workspace-a",
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
        for body in [session_body(), grant_body(), session_body(), grant_body()] {
            let (mut stream, _) = server.accept().unwrap();
            read_request(&mut stream);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
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
            "workspace-a",
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
            "workspace-a",
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
        for body in [session_body(), grant_body()] {
            let (mut stream, _) = server.accept().unwrap();
            read_request(&mut stream);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });

    let temporary_profile = TemporaryProfile::new("run-boundaries", true);
    let root = temporary_profile.root.clone();
    let profile = temporary_profile.profile.clone();
    let config = temporary_profile.config.clone();
    let storage = open_profile_storage(&profile).unwrap();
    let thread_id = create_thread_now(
        &mut storage.lock().unwrap().journal,
        "workspace-a",
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
            workspace: Some("workspace-a".into()),
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
        for body in [session_body(), grant_body()] {
            let (mut stream, _) = server.accept().unwrap();
            read_request(&mut stream);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });

    let profile = TemporaryProfile::new("run-chat-events", true);
    stage_pi_stub(&profile.root);
    let state = RuntimeAttachState::open(&profile.profile, &profile.config).unwrap();
    let boundaries = state.boundaries();
    let mut subscription_service = state.attach_service().unwrap();
    let events = subscription_service.subscribe_chat_events().unwrap();
    let mut second_subscription_service = state.attach_service().unwrap();
    let second_events = second_subscription_service.subscribe_chat_events().unwrap();
    let (result, launch) = prepare_desktop_run(
        &boundaries,
        RunStartRequest {
            prompt: "hello".into(),
            files: Vec::new(),
            workspace: Some("workspace-a".into()),
            provenance: None,
            thread_id: None,
        },
    )
    .unwrap();

    boundaries.launch(launch);

    let event = events.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(event.run_id, result.run_id);
    let second_event = second_events.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(second_event.run_id, result.run_id);
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
        for body in [session_body(), grant_body()] {
            let (mut stream, _) = server.accept().unwrap();
            read_request(&mut stream);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
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
            "workspace-a",
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
    r#"{"session":{"org_id":"20000000-0000-4000-8000-000000000002","user_id":"30000000-0000-4000-8000-000000000003","role":"owner","device_id":"10000000-0000-4000-8000-000000000001","client_role":"desktop"},"entitlement_snapshot":{"payload":{"version":7,"user_display_name":"User","organization_display_name":"Muniment","groups":[]},"signature":"signature-secret","algorithm":"hmac-sha256"}}"#.into()
}

fn grant_body() -> String {
    r#"{"workspace":"workspace-a","gatewayUrl":"https://gateway.example.com","virtualKey":"key","minimumCacheablePrefixCharacters":8192,"receiptUrl":"https://receipts.example.com"}"#.into()
}

fn read_request(stream: &mut std::net::TcpStream) {
    let mut bytes = Vec::new();
    while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
        let mut buffer = [0; 1024];
        let read = stream.read(&mut buffer).unwrap();
        bytes.extend_from_slice(&buffer[..read]);
    }
}
