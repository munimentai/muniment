#![cfg(target_os = "linux")]

use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use muniment_attach::handshake_stream;
use muniment_core::attach::linux::{
    AttachFilesystem, LiveConnectionRegistry, ThreadListPage, ThreadListRequest, ThreadListService,
};
use muniment_core::attach::{
    ApprovalCoordinator, CompanionRegistry, ProtocolError, SignedWorkspaceApproval,
};
use muniment_runtime::run_attach_listener;

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

fn handshake(approvals: ApprovalCoordinator) -> bool {
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
                &registry,
                approval,
                approvals,
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

#[test]
fn handshake_gets_no_grant_without_a_presenter() {
    assert!(!handshake(ApprovalCoordinator::default()));
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

    assert!(handshake(approvals));
}
