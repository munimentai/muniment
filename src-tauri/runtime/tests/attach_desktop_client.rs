#![cfg(target_os = "linux")]

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use muniment_attach::{connect_desktop_client_at, Operation};
use muniment_core::attach::linux::{
    AttachFilesystem, LiveConnectionRegistry, ThreadListPage, ThreadListRequest, ThreadListService,
};
use muniment_core::attach::{
    ApprovalCoordinator, CompanionRegistry, ProtocolError, SignedWorkspaceApproval,
    CAPABILITY_IDLE_LIFETIME,
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
