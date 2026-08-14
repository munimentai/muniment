#![cfg(target_os = "linux")]

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use muniment_core::attach::ApprovalRequest;
use muniment_core::run_start::{RunAttachBoundaries, RunStartBoundaries};
use muniment_runtime::RuntimeAttachState;

mod common;
use common::TemporaryProfile;

#[test]
fn boundaries_share_workspace_approval_and_journal() {
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let profile = TemporaryProfile::new("attach-state", true);
    let state = RuntimeAttachState::open(&profile.profile, &profile.config).unwrap();
    let first = state.boundaries();
    let second = state.boundaries();
    let credentials = common::credentials();

    first
        .signed_workspace_approval()
        .record("workspace-a".into());
    assert_eq!(second.attach_approval().unwrap().workspace, "workspace-a");

    let run_id = "01900000-0000-7000-8000-000000000001";
    let (run_seq, _) = first
        .prepare_run(
            run_id,
            "hello",
            &common::fixture_grant(),
            &credentials.tokens,
            Vec::new(),
            None,
            None,
        )
        .unwrap();
    let first_stream = first.stream_run("workspace-a", run_id, 0).unwrap();
    let second_stream = second.stream_run("workspace-a", run_id, 0).unwrap();
    assert_eq!(first_stream.current_run_seq, run_seq);
    assert_eq!(second_stream.current_run_seq, run_seq);

    assert_eq!(
        state
            .signed_workspace_approval()
            .approval()
            .unwrap()
            .workspace,
        "workspace-a"
    );
    let _registry = state.companion_registry();
    state.attach_service().unwrap();
}

#[test]
fn listener_inputs_share_the_approval_coordinator() {
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let profile = TemporaryProfile::new("attach-state-approvals", true);
    let state = RuntimeAttachState::open(&profile.profile, &profile.config).unwrap();
    let claimed = state.attach_listener_inputs().approvals;
    let requested = state.attach_listener_inputs().approvals;
    let decisions = requested.clone();
    let (presented_tx, presented_rx) = mpsc::channel();
    let guard = claimed
        .claim_presenter(move |request| presented_tx.send(request.challenge.clone()).is_ok())
        .unwrap();

    assert!(claimed.claim_presenter(|_| false).is_none());

    let request = thread::spawn(move || {
        requested.request(
            ApprovalRequest {
                challenge: "challenge-a".into(),
                claimed_kind: "cli".into(),
                claimed_version: "1.0.0".into(),
                workspace: "workspace-a".into(),
                scopes: Default::default(),
            },
            Duration::from_secs(1),
        )
    });
    assert_eq!(
        presented_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        "challenge-a"
    );
    assert!(decisions.decide("challenge-a", true));
    assert!(request.join().unwrap());

    drop(guard);
}
