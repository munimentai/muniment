#![cfg(target_os = "linux")]

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::os::unix::{fs::PermissionsExt, net::UnixStream};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use muniment_attach::{
    connect_desktop_client_at, handshake_stream, serve_approval_presenter_at,
    ApprovalDecision as PresenterDecision, ApprovalPresenterStopHandle, Operation,
};
use muniment_core::attach::linux::{
    AttachFilesystem, LiveConnectionRegistry, ThreadListPage, ThreadListRequest, ThreadListService,
};
use muniment_core::attach::{
    ApprovalCoordinator, ApprovalRequest, CompanionRegistry, ProtocolError,
    SignedWorkspaceApproval, CAPABILITY_IDLE_LIFETIME,
};
use muniment_runtime::{run_attach_listener, AttachListenerInputs};

mod common;
use common::TemporaryProfile;

#[derive(Default)]
struct TestService;

impl ThreadListService for TestService {
    fn list_threads(
        &mut self,
        workspace: &str,
        _request: ThreadListRequest,
    ) -> Result<ThreadListPage, ProtocolError> {
        assert_eq!(workspace, "workspace-a");
        Ok(ThreadListPage {
            threads: Vec::new(),
            next_cursor: None,
        })
    }
}

fn thread_list_body<T: FromIterator<(String, T)> + From<u64>>() -> T {
    [("limit".to_owned(), T::from(20))].into_iter().collect()
}

#[test]
fn shipped_desktop_client_completes_the_session() {
    let profile = TemporaryProfile::new("attach-desktop-client", false);
    fs::set_permissions(&profile.root, fs::Permissions::from_mode(0o700)).unwrap();
    let filesystem = AttachFilesystem::from_runtime_directory(&profile.root).unwrap();
    let endpoint = filesystem.endpoint_path().to_owned();
    drop(filesystem);
    let approval = SignedWorkspaceApproval::default();
    approval.record("workspace-a".into());
    let registry = CompanionRegistry::new(
        Arc::new(Mutex::new(HashMap::new())),
        profile.profile.join("companions.json"),
        LiveConnectionRegistry::default(),
    );
    let (stop_tx, stop_rx) = mpsc::channel();

    let (profile_id, workspace_scopes, summary, response) = thread::scope(|scope| {
        let listener = scope.spawn(|| {
            run_attach_listener(
                &profile.root,
                AttachListenerInputs {
                    companion_registry: &registry,
                    approval,
                    approvals: ApprovalCoordinator::default(),
                    expected_desktop_executable: Some(std::env::current_exe().unwrap()),
                },
                None,
                || Ok::<_, ()>(TestService),
                stop_rx,
            )
            .unwrap()
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut client = loop {
            match connect_desktop_client_at(&endpoint, "1.0.0", Duration::from_secs(1)) {
                Ok(client) => break client,
                Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                Err(error) => panic!("attach listener did not accept the desktop client: {error}"),
            }
        };

        let profile_id = client.profile_id().to_owned();
        let workspace_scopes = client.workspace_scopes().clone();
        let summary = client.authorization_summary();
        let response = client.request(Operation::ThreadList, None, thread_list_body());

        drop(client);
        stop_tx.send(()).unwrap();
        listener.join().unwrap();
        (profile_id, workspace_scopes, summary, response)
    });

    assert_eq!(profile_id, "desktop-owner");
    assert_eq!(workspace_scopes.len(), 1);
    assert_eq!(
        workspace_scopes["workspace-a"],
        BTreeSet::from(["run.write".to_owned(), "thread.read".to_owned()])
    );
    assert_eq!(summary.expires_in_seconds, 60 * 60);
    assert_eq!(
        summary.idle_timeout_seconds,
        CAPABILITY_IDLE_LIFETIME.as_secs()
    );
    assert!(response.unwrap().body["threads"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn presenter_and_desktop_client_serve_concurrent_sessions() {
    let profile = TemporaryProfile::new("attach-desktop-concurrent", false);
    fs::set_permissions(&profile.root, fs::Permissions::from_mode(0o700)).unwrap();
    let filesystem = AttachFilesystem::from_runtime_directory(&profile.root).unwrap();
    let endpoint = filesystem.endpoint_path().to_owned();
    drop(filesystem);
    let approval = SignedWorkspaceApproval::default();
    approval.record("workspace-a".into());
    let registry = CompanionRegistry::new(
        Arc::new(Mutex::new(HashMap::new())),
        profile.profile.join("companions.json"),
        LiveConnectionRegistry::default(),
    );
    let approvals = ApprovalCoordinator::default();
    let requests = approvals.clone();
    let (stop_tx, stop_rx) = mpsc::channel();
    let presenter_stop = ApprovalPresenterStopHandle::new();
    let worker_stop = presenter_stop.clone();
    let (presented_tx, presented_rx) = mpsc::channel();

    thread::scope(|scope| {
        let listener = scope.spawn(|| {
            run_attach_listener(
                &profile.root,
                AttachListenerInputs {
                    companion_registry: &registry,
                    approval,
                    approvals,
                    expected_desktop_executable: Some(std::env::current_exe().unwrap()),
                },
                None,
                || Ok::<_, ()>(TestService),
                stop_rx,
            )
            .unwrap()
        });
        let presenter = scope.spawn(|| {
            serve_approval_presenter_at(
                &endpoint,
                "1.0.0",
                Duration::from_secs(1),
                Duration::from_millis(10),
                worker_stop,
                |_| {},
                |request| {
                    presented_tx.send(request.challenge.clone()).unwrap();
                    PresenterDecision::Approve
                },
            )
        });

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut attempt = 0;
        loop {
            attempt += 1;
            if requests.request(
                approval_request(format!("presenter-ready-{attempt}")),
                Duration::from_millis(100),
            ) {
                break;
            }
            assert!(Instant::now() < deadline, "presenter did not connect");
            thread::sleep(Duration::from_millis(10));
        }
        presented_rx.recv_timeout(Duration::from_secs(1)).unwrap();

        let mut client = connect_desktop_client_at(&endpoint, "1.0.0", Duration::from_secs(1))
            .expect("desktop client did not connect");
        assert!(client
            .request(Operation::ThreadList, None, thread_list_body())
            .unwrap()
            .body["threads"]
            .as_array()
            .unwrap()
            .is_empty());

        let companion = handshake_stream(
            UnixStream::connect(&endpoint).unwrap(),
            "1.0.0",
            Duration::from_secs(1),
            Duration::from_secs(1),
            || {},
        )
        .expect("companion did not pair through the presenter");
        let paired_challenge = presented_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(!paired_challenge.is_empty());
        drop(companion);

        client
            .request(Operation::ThreadList, None, thread_list_body())
            .expect("desktop client stopped after companion pairing");
        drop(client);

        let final_challenge = "presenter-after-desktop-client";
        assert!(requests.request(
            approval_request(final_challenge.into()),
            Duration::from_secs(1),
        ));
        assert_eq!(
            presented_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            final_challenge
        );

        presenter_stop.stop();
        presenter.join().unwrap();
        stop_tx.send(()).unwrap();
        listener.join().unwrap();
    });
}

fn approval_request(challenge: String) -> ApprovalRequest {
    ApprovalRequest {
        challenge,
        claimed_kind: "test".into(),
        claimed_version: "1.0.0".into(),
        workspace: "workspace-a".into(),
        scopes: Default::default(),
    }
}
