#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use muniment_attach::{connect_desktop_client_at, ClientError, Operation};
use muniment_core::attach::{
    linux::AttachFilesystem, RuntimeActivityRegistry, SignedWorkspaceApproval,
};
use muniment_core::auth::EntitlementSnapshotTracker;
use muniment_core::memory_runtime::ApplicationMemoryRuntime;
use muniment_core::session_thread::SessionThread;
use muniment_runtime::{
    compose_attach_service, open_companion_registry, open_profile_storage, run_attach_listener,
    AttachListenerInputs, RuntimeAttachBoundaries,
};
use serde_json::json;

mod common;
use common::TemporaryProfile;

const THREAD: &str = "01900000-0000-7000-8000-000000000001";

#[test]
fn history_failures_reach_the_desktop_wire_and_runtime_log() {
    const CHILD: &str = "MUNIMENT_HISTORY_DIAGNOSTICS_CHILD";
    if std::env::var_os(CHILD).is_none() {
        // Capture the runtime stderr sink without changing the process-wide logging path.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "history_failures_reach_the_desktop_wire_and_runtime_log",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        let log = String::from_utf8(output.stderr).unwrap();
        assert!(
            output.status.success(),
            "{log}\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
        for operation in [
            "thread.summaries",
            "thread.select",
            "thread.history",
            "thread.create",
            "thread.rename",
            "thread.delete",
            "run.resume",
            "retention.recheck",
        ] {
            for cause in ["poisoned", "no such table:"] {
                assert!(
                    log.lines().any(|line| {
                        line.contains(&format!(
                            "desktop request rejected operation=\"{operation}\""
                        )) && line.contains(cause)
                    }),
                    "missing {operation} cause {cause}: {log}"
                );
            }
        }
        return;
    }
    for poisoned in [false, true] {
        check_history_failures(poisoned);
    }
}

fn check_history_failures(poisoned: bool) {
    let profile = TemporaryProfile::new("history-diagnostics", true);
    fs::set_permissions(&profile.root, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(
        profile
            .config
            .join(muniment_core::local_mode::LOCAL_MODE_MARKER),
        b"",
    )
    .unwrap();
    muniment_core::retention_record::write_retention_choice(
        &profile.config,
        muniment_core::retention_record::RetentionChoice::DeleteAfter30Days,
    )
    .unwrap();
    let storage = open_profile_storage(&profile.profile).unwrap();
    if poisoned {
        let storage = Arc::clone(&storage);
        assert!(thread::spawn(move || {
            let _guard = storage.lock().unwrap();
            panic!("poison the history lock");
        })
        .join()
        .is_err());
    } else {
        rusqlite::Connection::open(profile.profile.join("runs.sqlite3"))
            .unwrap()
            .execute_batch("DROP TABLE thread_events; DROP TABLE run_threads; DROP TABLE run_workspaces; DROP TABLE events;")
            .unwrap();
    }
    let registry = open_companion_registry(&profile.profile).unwrap();
    let approval = SignedWorkspaceApproval::default();
    approval.record("local".into());
    let filesystem = AttachFilesystem::from_runtime_directory(&profile.root).unwrap();
    let endpoint = filesystem.endpoint_path().to_owned();
    drop(filesystem);
    let (stop_tx, stop_rx) = mpsc::channel();
    thread::scope(|scope| {
        let listener = scope.spawn(|| {
            let profile_directory = profile.profile.clone();
            let config_directory = profile.config.clone();
            let service_registry = registry.clone();
            let service_approval = approval.clone();
            run_attach_listener(
                &profile.root,
                AttachListenerInputs {
                    companion_registry: &registry,
                    approval: approval.clone(),
                    approvals: Default::default(),
                    expected_desktop_executable: Some(std::env::current_exe().unwrap()),
                },
                None,
                move || {
                    let boundaries = RuntimeAttachBoundaries::new(
                        Arc::clone(&storage),
                        Arc::new(Mutex::new(None)),
                        profile_directory.clone(),
                        config_directory.clone(),
                        Arc::new(Mutex::new(None)),
                        Arc::new(ApplicationMemoryRuntime::new(
                            config_directory.clone(),
                            profile_directory.join("memory"),
                        )),
                        RuntimeActivityRegistry::new(),
                        Arc::new(EntitlementSnapshotTracker::new()),
                        service_approval.clone(),
                        Arc::new(SessionThread::default()),
                        service_registry.clone(),
                    );
                    compose_attach_service(
                        boundaries,
                        &service_registry,
                        &profile_directory,
                        &config_directory,
                    )
                },
                stop_rx,
            )
            .unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut client = loop {
            match connect_desktop_client_at(&endpoint, "1.0.0", Duration::from_secs(1)) {
                Ok(client) => break client,
                Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                Err(error) => {
                    stop_tx.send(()).unwrap();
                    panic!("the desktop connection failed: {error}");
                }
            }
        };
        let cases = [
            (Operation::ThreadSummaries, json!({"limit": 10})),
            (Operation::ThreadSelect, json!({"thread_id": THREAD})),
            (
                Operation::ThreadHistory,
                json!({"thread_id": THREAD, "limit": 10}),
            ),
            (Operation::ThreadCreate, json!({})),
            (
                Operation::ThreadRename,
                json!({"thread_id": THREAD, "title": "A title"}),
            ),
            (Operation::ThreadDelete, json!({"thread_id": THREAD})),
            (Operation::RunResume, json!({"run_id": THREAD})),
            (Operation::RetentionRecheck, json!({})),
        ];
        let results = cases
            .into_iter()
            .enumerate()
            .map(|(index, (operation, body))| {
                let key = format!("01900000-0000-7000-8000-{index:012x}");
                let key = matches!(
                    operation,
                    Operation::ThreadCreate
                        | Operation::ThreadRename
                        | Operation::ThreadDelete
                        | Operation::RunResume
                )
                .then(|| muniment_attach::Id::new(key).unwrap());
                let result = client.request(operation, key, body);
                let reason = client.last_request_error().map(ToString::to_string);
                (operation, result, reason)
            })
            .collect::<Vec<_>>();
        drop(client);
        stop_tx.send(()).unwrap();
        listener.join().unwrap();
        for (operation, result, reason) in results {
            assert!(
                matches!(result, Err(ClientError::DesktopFailed)),
                "{operation:?}: {result:?}"
            );
            let reason = reason.unwrap();
            let cause = if poisoned {
                "poisoned"
            } else {
                "no such table:"
            };
            assert!(
                reason.contains("Conversation history"),
                "{operation:?}: {reason}"
            );
            assert!(reason.contains(cause), "{operation:?}: {reason}");
        }
    });
}
