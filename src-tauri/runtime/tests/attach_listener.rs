#![cfg(target_os = "linux")]

use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use muniment_attach::{
    handshake_stream, serve_approval_presenter_at, ApprovalDecision as PresenterDecision,
    ApprovalPresenterStopHandle,
};
use muniment_core::attach::linux::{
    AttachFilesystem, LiveConnectionRegistry, ThreadListPage, ThreadListRequest, ThreadListService,
};
use muniment_core::attach::{
    probe_handoff, ApprovalCoordinator, ApprovalRequest, CompanionRegistry, HandoffProbeError,
    ProtocolError, SignedWorkspaceApproval,
};
use muniment_runtime::{run_attach_listener, AttachListenerInputs};

mod common;
use common::TemporaryProfile;

#[derive(Default)]
struct TestService;

impl ThreadListService for TestService {
    fn list_threads(
        &mut self,
        _workspace: &str,
        _request: ThreadListRequest,
    ) -> Result<ThreadListPage, ProtocolError> {
        unreachable!("the handshake does not list threads")
    }
}

fn handshake(approvals: ApprovalCoordinator, handoff_nonce: Option<String>) -> bool {
    let profile = TemporaryProfile::new("attach-listener", false);
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

    thread::scope(|scope| {
        let listener = scope.spawn(|| {
            run_attach_listener(
                &profile.root,
                AttachListenerInputs {
                    companion_registry: &registry,
                    approval,
                    approvals,
                    expected_desktop_executable: std::env::current_exe().ok(),
                },
                handoff_nonce,
                || Ok::<_, ()>(TestService),
                stop_rx,
            )
            .unwrap()
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        let stream = loop {
            match UnixStream::connect(&endpoint) {
                Ok(stream) => break stream,
                Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                Err(error) => panic!("attach listener did not start: {error}"),
            }
        };
        let result = handshake_stream(
            stream,
            "1.0.0",
            Duration::from_secs(1),
            Duration::from_secs(1),
            || {},
        )
        .is_ok();
        stop_tx.send(()).unwrap();
        listener.join().unwrap();
        result
    })
}

fn probe(handoff_nonce: Option<String>) -> Result<(), HandoffProbeError> {
    let profile = TemporaryProfile::new("attach-listener-probe", false);
    fs::set_permissions(&profile.root, fs::Permissions::from_mode(0o700)).unwrap();
    let filesystem = AttachFilesystem::from_runtime_directory(&profile.root).unwrap();
    let endpoint = filesystem.endpoint_path().to_owned();
    drop(filesystem);
    let registry = CompanionRegistry::new(
        Arc::new(Mutex::new(HashMap::new())),
        profile.profile.join("companions.json"),
        LiveConnectionRegistry::default(),
    );
    let (stop_tx, stop_rx) = mpsc::channel();

    thread::scope(|scope| {
        let listener = scope.spawn(|| {
            run_attach_listener(
                &profile.root,
                AttachListenerInputs {
                    companion_registry: &registry,
                    approval: SignedWorkspaceApproval::default(),
                    approvals: ApprovalCoordinator::default(),
                    expected_desktop_executable: std::env::current_exe().ok(),
                },
                handoff_nonce,
                || Ok::<_, ()>(TestService),
                stop_rx,
            )
            .unwrap()
        });
        let result = probe_handoff(
            &endpoint,
            "prepared-nonce",
            Instant::now() + Duration::from_secs(2),
        )
        .map(|_| ());
        stop_tx.send(()).unwrap();
        listener.join().unwrap();
        result
    })
}

#[test]
fn probe_confirms_the_prepared_handoff_nonce() {
    assert_eq!(probe(Some("prepared-nonce".into())), Ok(()));
}

#[test]
fn probe_rejects_a_welcome_without_a_handoff_nonce() {
    assert_eq!(probe(None), Err(HandoffProbeError::MissingNonce));
}

#[test]
fn handshake_gets_no_grant_without_a_presenter() {
    assert!(!handshake(ApprovalCoordinator::default(), None));
}

#[test]
fn handshake_gets_a_grant_when_the_presenter_approves() {
    let approvals = ApprovalCoordinator::default();
    let decisions = approvals.clone();
    approvals.register_presenter(move |request| {
        let decisions = decisions.clone();
        let challenge = request.challenge.clone();
        thread::spawn(move || {
            decisions.decide(&challenge, true);
        });
        true
    });

    assert!(handshake(approvals, Some("prepared-nonce".into())));
}

#[test]
fn desktop_presenter_answers_an_approval_request() {
    let profile = TemporaryProfile::new("attach-listener-presenter", false);
    fs::set_permissions(&profile.root, fs::Permissions::from_mode(0o700)).unwrap();
    let filesystem = AttachFilesystem::from_runtime_directory(&profile.root).unwrap();
    let endpoint = filesystem.endpoint_path().to_owned();
    drop(filesystem);
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
                    approval: SignedWorkspaceApproval::default(),
                    approvals,
                    expected_desktop_executable: std::env::current_exe().ok(),
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
                |request| {
                    presented_tx.send(request.challenge.clone()).unwrap();
                    PresenterDecision::Approve
                },
            )
        });

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut attempt = 0;
        let challenge = loop {
            attempt += 1;
            let challenge = format!("runtime-presenter-{attempt}");
            if requests.request(
                ApprovalRequest {
                    challenge: challenge.clone(),
                    claimed_kind: "editor-extension".into(),
                    claimed_version: "1.0.0".into(),
                    workspace: "workspace-a".into(),
                    scopes: Default::default(),
                },
                Duration::from_millis(100),
            ) {
                break challenge;
            }
            assert!(Instant::now() < deadline, "presenter did not connect");
            thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(
            presented_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            challenge
        );

        presenter_stop.stop();
        presenter.join().unwrap();
        stop_tx.send(()).unwrap();
        listener.join().unwrap();
    });
}
