#![cfg(target_os = "linux")]

use muniment_attach::{connect_desktop_client_at, Operation};
use muniment_core::attach::linux::AttachFilesystem;
use muniment_core::journal::{EventEnvelope, EventPayload, Provenance};
use muniment_core::retention_record::{write_retention_choice, RetentionChoice};
use muniment_runtime::{
    run_runtime_activation_with_desktop_executable, run_runtime_activation_with_retention_trigger,
    run_runtime_activation_with_upgrade_watch, RetentionScheduleTestControl,
    RuntimeActivationError, RuntimeActivationExit, UpgradeWatchTestControl,
};
use muniment_runtime_core::open_profile_storage;
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

mod common;
use common::TemporaryProfile;
use common::CI_WAIT;

#[test]
fn executable_replacement_requests_an_upgrade_refresh_exit() {
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let profile = TemporaryProfile::new("activation-upgrade-watch", false);
    fs::set_permissions(&profile.root, fs::Permissions::from_mode(0o700)).unwrap();
    let executable = profile.root.join("runtime-executable");
    fs::write(&executable, "old").unwrap();
    let filesystem = AttachFilesystem::from_runtime_directory(&profile.root).unwrap();
    let endpoint = filesystem.endpoint_path().to_owned();
    drop(filesystem);
    let (stop_tx, stop_rx) = mpsc::channel();

    thread::scope(|scope| {
        let watched_executable = executable.clone();
        let activation = scope.spawn(|| {
            run_runtime_activation_with_upgrade_watch(
                &profile.root,
                &profile.profile,
                &profile.config,
                Instant::now() + CI_WAIT,
                Some(UpgradeWatchTestControl {
                    path: watched_executable,
                    poll_interval: Duration::from_millis(5),
                }),
                None,
                stop_rx,
            )
            .unwrap()
        });
        let deadline = Instant::now() + CI_WAIT;
        while !endpoint.exists() {
            if Instant::now() >= deadline {
                // Stop the activation so the scope can join it before the failure.
                let _ = stop_tx.send(());
            }
            assert!(Instant::now() < deadline, "runtime endpoint did not open");
            thread::sleep(Duration::from_millis(5));
        }
        let replacement = profile.root.join("replacement-executable");
        fs::write(&replacement, "new").unwrap();
        fs::rename(replacement, executable).unwrap();
        assert_eq!(
            activation.join().unwrap(),
            RuntimeActivationExit::UpgradeRefresh
        );
    });
}

fn event(run_id: &str, event_id: &str, run_seq: u64, event_type: &str, at: &str) -> EventEnvelope {
    EventEnvelope {
        event_id: event_id.into(),
        run_id: run_id.into(),
        run_seq,
        event_type: event_type.into(),
        event_version: 1,
        envelope_version: 1,
        recorded_at: at.into(),
        occurred_at: None,
        correlation_id: None,
        causation_id: None,
        payload: EventPayload::Inline {
            payload_json: "{}".parse().unwrap(),
        },
        provenance: Provenance {
            source: "test".into(),
            source_version: "1".into(),
            actor_id: None,
            device_id: None,
            rpc_request_id: None,
            capability_versions: None,
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    }
}

#[test]
fn fresh_profile_serves_session_status_and_releases_the_endpoint() {
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let profile = TemporaryProfile::new("activation", false);
    fs::set_permissions(&profile.root, fs::Permissions::from_mode(0o700)).unwrap();
    let filesystem = AttachFilesystem::from_runtime_directory(&profile.root).unwrap();
    let endpoint = filesystem.endpoint_path().to_owned();
    drop(filesystem);
    let (stop_tx, stop_rx) = mpsc::channel();

    thread::scope(|scope| {
        let activation = scope.spawn(|| {
            run_runtime_activation_with_desktop_executable(
                &profile.root,
                &profile.profile,
                &profile.config,
                Instant::now() + CI_WAIT,
                Some(std::env::current_exe().unwrap()),
                stop_rx,
            )
            .unwrap()
        });
        let deadline = Instant::now() + CI_WAIT;
        let mut client = loop {
            match connect_desktop_client_at(&endpoint, "1.0.0", Duration::from_secs(1)) {
                Ok(client) => break client,
                Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                Err(error) => {
                    let _ = stop_tx.send(());
                    panic!("runtime activation did not accept the client: {error}");
                }
            }
        };
        let response = client
            .request(
                Operation::SessionStatus,
                None,
                std::iter::empty::<(String, u64)>().collect(),
            )
            .unwrap();
        assert_eq!(response.body["signed_in"], false);

        drop(client);
        stop_tx.send(()).unwrap();
        assert_eq!(
            activation.join().unwrap(),
            RuntimeActivationExit::ManagerStop
        );
    });

    let filesystem = AttachFilesystem::from_runtime_directory(&profile.root).unwrap();
    let lock = filesystem.acquire_instance_lock().unwrap();
    drop(lock);
    assert!(std::os::unix::net::UnixStream::connect(endpoint).is_err());
}

#[test]
fn retention_trigger_deletes_only_expired_terminal_runs() {
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let profile = TemporaryProfile::new("activation-retention", false);
    fs::set_permissions(&profile.root, fs::Permissions::from_mode(0o700)).unwrap();
    let storage = open_profile_storage(&profile.profile).unwrap();
    let expired = "01900000-0000-7000-8000-000000000011";
    let recent = "01900000-0000-7000-8000-000000000012";
    {
        let mut storage = storage.lock().unwrap();
        for (run_id, started_id, terminal_id, at) in [
            (
                expired,
                "01900000-0000-7000-8000-000000000021",
                "01900000-0000-7000-8000-000000000023",
                "2000-01-01T00:00:00Z",
            ),
            (
                recent,
                "01900000-0000-7000-8000-000000000022",
                "01900000-0000-7000-8000-000000000024",
                "2999-01-01T00:00:00Z",
            ),
        ] {
            storage
                .journal
                .append_batch(
                    0,
                    &[
                        event(run_id, started_id, 1, "run.started", at),
                        event(run_id, terminal_id, 2, "run.completed", at),
                    ],
                )
                .unwrap();
        }
    }
    let (trigger_tx, trigger_rx) = mpsc::channel();
    let (checked_tx, checked_rx) = mpsc::channel();
    let (stop_tx, stop_rx) = mpsc::channel();

    thread::scope(|scope| {
        let activation = scope.spawn(|| {
            run_runtime_activation_with_retention_trigger(
                &profile.root,
                &profile.profile,
                &profile.config,
                Instant::now() + CI_WAIT,
                Some(std::env::current_exe().unwrap()),
                Some(RetentionScheduleTestControl {
                    trigger: trigger_rx,
                    checked: checked_tx,
                }),
                stop_rx,
            )
            .unwrap();
        });
        // Stop the activation before any wait or assert below can panic, so
        // the scope joins instead of hanging.
        let checked = checked_rx.recv_timeout(CI_WAIT);
        if checked.is_err() {
            let _ = stop_tx.send(());
        }
        checked.unwrap();
        write_retention_choice(&profile.config, RetentionChoice::DeleteAfter30Days).unwrap();
        trigger_tx.send(()).unwrap();
        let checked = checked_rx.recv_timeout(CI_WAIT);
        if checked.is_err() {
            let _ = stop_tx.send(());
        }
        checked.unwrap();
        let deadline = Instant::now() + CI_WAIT;
        loop {
            let mut storage = storage.lock().unwrap();
            if storage.journal.events(expired).unwrap().is_empty() {
                let recent_count = storage.journal.events(recent).unwrap().len();
                drop(storage);
                let _ = stop_tx.send(());
                assert_eq!(recent_count, 2);
                break;
            }
            drop(storage);
            if Instant::now() >= deadline {
                let _ = stop_tx.send(());
            }
            assert!(Instant::now() < deadline, "retention trigger did not run");
            thread::sleep(Duration::from_millis(10));
        }
        stop_tx.send(()).unwrap();
        activation.join().unwrap();

        let stopped = "01900000-0000-7000-8000-000000000013";
        storage
            .lock()
            .unwrap()
            .journal
            .append_batch(
                0,
                &[
                    event(
                        stopped,
                        "01900000-0000-7000-8000-000000000025",
                        1,
                        "run.started",
                        "2000-01-01T00:00:00Z",
                    ),
                    event(
                        stopped,
                        "01900000-0000-7000-8000-000000000026",
                        2,
                        "run.completed",
                        "2000-01-01T00:00:00Z",
                    ),
                ],
            )
            .unwrap();
        let _ = trigger_tx.send(());
        assert!(checked_rx.recv_timeout(Duration::from_millis(100)).is_err());
        assert_eq!(
            storage
                .lock()
                .unwrap()
                .journal
                .events(stopped)
                .unwrap()
                .len(),
            2
        );
    });
}

#[test]
fn blocked_ownership_does_not_start_retention() {
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let profile = TemporaryProfile::new("activation-retention-lock", false);
    fs::set_permissions(&profile.root, fs::Permissions::from_mode(0o700)).unwrap();
    let storage = open_profile_storage(&profile.profile).unwrap();
    let expired = "01900000-0000-7000-8000-000000000031";
    storage
        .lock()
        .unwrap()
        .journal
        .append_batch(
            0,
            &[
                event(
                    expired,
                    "01900000-0000-7000-8000-000000000032",
                    1,
                    "run.started",
                    "2000-01-01T00:00:00Z",
                ),
                event(
                    expired,
                    "01900000-0000-7000-8000-000000000033",
                    2,
                    "run.completed",
                    "2000-01-01T00:00:00Z",
                ),
            ],
        )
        .unwrap();
    write_retention_choice(&profile.config, RetentionChoice::DeleteAfter30Days).unwrap();
    let filesystem = AttachFilesystem::from_runtime_directory(&profile.root).unwrap();
    let _lock = filesystem.acquire_instance_lock().unwrap();
    let (_trigger_tx, trigger_rx) = mpsc::channel();
    let (checked_tx, checked_rx) = mpsc::channel();
    let (_stop_tx, stop_rx) = mpsc::channel();

    assert!(run_runtime_activation_with_retention_trigger(
        &profile.root,
        &profile.profile,
        &profile.config,
        Instant::now() + Duration::from_secs(2),
        Some(std::env::current_exe().unwrap()),
        Some(RetentionScheduleTestControl {
            trigger: trigger_rx,
            checked: checked_tx,
        }),
        stop_rx,
    )
    .is_err());
    assert!(checked_rx.try_recv().is_err());
    assert_eq!(
        storage
            .lock()
            .unwrap()
            .journal
            .events(expired)
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn listener_failure_stops_the_coordinator_and_returns_the_error() {
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let profile = TemporaryProfile::new("activation-listener-failure", false);
    fs::set_permissions(&profile.root, fs::Permissions::from_mode(0o700)).unwrap();
    let filesystem = AttachFilesystem::from_runtime_directory(&profile.root).unwrap();
    let _lock = filesystem.acquire_instance_lock().unwrap();
    let (_stop_tx, stop_rx) = mpsc::channel();

    let result = run_runtime_activation_with_desktop_executable(
        &profile.root,
        &profile.profile,
        &profile.config,
        Instant::now(),
        Some(std::env::current_exe().unwrap()),
        stop_rx,
    );

    assert!(matches!(result, Err(RuntimeActivationError::Migration(_))));
}
