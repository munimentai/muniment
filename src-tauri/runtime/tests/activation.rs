#![cfg(target_os = "linux")]

use muniment_attach::{connect_desktop_client_at, Operation};
use muniment_core::attach::linux::AttachFilesystem;
use muniment_core::journal::{EventEnvelope, EventPayload, Provenance};
use muniment_core::retention_record::{write_retention_choice, RetentionChoice};
use muniment_runtime::{
    open_profile_storage, run_runtime_activation_with_desktop_executable,
    run_runtime_activation_with_retention_trigger,
};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

mod common;
use common::TemporaryProfile;

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
                Instant::now() + Duration::from_secs(2),
                Some(std::env::current_exe().unwrap()),
                stop_rx,
            )
            .unwrap()
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut client = loop {
            match connect_desktop_client_at(&endpoint, "1.0.0", Duration::from_secs(1)) {
                Ok(client) => break client,
                Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                Err(error) => panic!("runtime activation did not accept the client: {error}"),
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
        activation.join().unwrap();
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
    let (stop_tx, stop_rx) = mpsc::channel();

    thread::scope(|scope| {
        let activation = scope.spawn(|| {
            run_runtime_activation_with_retention_trigger(
                &profile.root,
                &profile.profile,
                &profile.config,
                Instant::now() + Duration::from_secs(2),
                Some(std::env::current_exe().unwrap()),
                Some(trigger_rx),
                stop_rx,
            )
            .unwrap();
        });
        write_retention_choice(&profile.config, RetentionChoice::DeleteAfter30Days).unwrap();
        trigger_tx.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let mut storage = storage.lock().unwrap();
            if storage.journal.events(expired).unwrap().is_empty() {
                assert_eq!(storage.journal.events(recent).unwrap().len(), 2);
                break;
            }
            drop(storage);
            assert!(Instant::now() < deadline, "retention trigger did not run");
            thread::sleep(Duration::from_millis(10));
        }
        stop_tx.send(()).unwrap();
        activation.join().unwrap();
    });
}
