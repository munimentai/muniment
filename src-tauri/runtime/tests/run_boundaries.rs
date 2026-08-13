#![cfg(target_os = "linux")]

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use muniment_core::attach::{RuntimeActivityRegistry, SignedWorkspaceApproval};
use muniment_core::auth::{KeyringNativeCredentialStore, NativeCredentialStore};
use muniment_core::chat_view::SelectedFile;
use muniment_core::journal::thread_mutation::create_thread_now;
use muniment_core::journal::Provenance;
use muniment_core::memory_runtime::ApplicationMemoryRuntime;
use muniment_core::pi_execution::PiRuntime;
use muniment_core::run_start::{
    prepare_desktop_run, RunStartBoundaries, RunStartError, RunStartRequest,
};
use muniment_core::session_thread::SessionThread;
use muniment_runtime::{open_profile_storage, RuntimeAttachBoundaries};

mod common;
use common::{credentials, TemporaryProfile};

#[test]
fn runtime_boundaries_prepare_a_desktop_run() {
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
        SignedWorkspaceApproval::default(),
        Arc::new(SessionThread::default()),
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
