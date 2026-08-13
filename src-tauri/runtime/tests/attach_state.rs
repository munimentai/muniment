#![cfg(target_os = "linux")]

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
