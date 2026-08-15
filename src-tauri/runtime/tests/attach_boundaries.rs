#![cfg(target_os = "linux")]

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use muniment_core::attach::linux::{
    CompanionProvenance, ThreadListRequest, ThreadListService, ThreadOpenRequest,
};
use muniment_core::attach::{Id, ProtocolError, RuntimeActivityRegistry, SignedWorkspaceApproval};
use muniment_core::auth::{
    EntitlementSnapshotTracker, KeyringNativeCredentialStore, NativeCredentialStore,
};
use muniment_core::journal::Provenance;
use muniment_core::memory_runtime::ApplicationMemoryRuntime;
use muniment_core::permission_gate::ChatPermissionAnswer;
use muniment_core::pi_execution::PiRuntime;
use muniment_core::run_preparation::{prepare_new_run_with_session_thread, SessionThreadStart};
use muniment_core::run_start::{ActiveRun, RunAttachBoundaries, RunStartBoundaries};
use muniment_core::session_thread::SessionThread;
use muniment_runtime::{open_profile_storage, RuntimeAttachBoundaries, RuntimeAttachState};

mod common;
use common::{
    credentials, credentials_with_expiry, spawn_server, spawn_server_sequence, TemporaryProfile,
};

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

#[test]
fn runtime_state_answers_entitlement_and_replays_sign_out() {
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let temporary_profile = TemporaryProfile::new("attach-session", false);
    let state =
        RuntimeAttachState::open(&temporary_profile.profile, &temporary_profile.config).unwrap();
    let credential_store = KeyringNativeCredentialStore::new();
    credential_store.clear_session().unwrap();
    credential_store.save_credentials(&credentials()).unwrap();

    let session_body = |version| {
        format!(
            r#"{{"session":{{"org_id":"20000000-0000-4000-8000-000000000002","user_id":"30000000-0000-4000-8000-000000000003","role":"owner","device_id":"10000000-0000-4000-8000-000000000001","client_role":"desktop"}},"entitlement_snapshot":{{"payload":{{"version":{version},"user_display_name":"User","organization_display_name":"Muniment","groups":[]}},"signature":"signature-secret","algorithm":"hmac-sha256"}}}}"#
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

    let run_id = "01900000-0000-7000-8000-000000000020";
    let (current_run_seq, _) = prepare_new_run_with_session_thread(
        &storage,
        SessionThreadStart {
            tracker: &SessionThread::default(),
            continue_existing: false,
        },
        run_id,
        "workspace-a",
        None,
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

    let session_body = r#"{"session":{"org_id":"20000000-0000-4000-8000-000000000002","user_id":"30000000-0000-4000-8000-000000000003","role":"owner","device_id":"10000000-0000-4000-8000-000000000001","client_role":"desktop"},"entitlement_snapshot":{"payload":{"version":7,"user_display_name":"User","organization_display_name":"Muniment","groups":[]},"signature":"signature-secret","algorithm":"hmac-sha256"}}"#;
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

    let _commit = boundaries
        .queue_attach_permission_answer(
            "workspace-a",
            "run-1",
            "gate-1",
            ChatPermissionAnswer::Confirm(true),
        )
        .unwrap();
    let queued = permission_answers.lock().unwrap().pop_front().unwrap();
    assert_eq!(queued.gate_id, "gate-1");

    drop(boundaries);
    drop(storage);
}

#[test]
fn runtime_boundaries_share_owner_approval_and_session_thread() {
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
