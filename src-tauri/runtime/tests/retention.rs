use muniment_core::journal::{EventEnvelope, EventPayload, Provenance};
use muniment_core::run_events::SharedStorage;
use muniment_core::run_preparation::{prepare_new_run_with_session_thread, SessionThreadStart};
use muniment_core::session_thread::SessionThread;
use muniment_runtime::{apply_retention, open_profile_storage};
use std::collections::BTreeMap;
use std::fs;
use std::sync::Arc;

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

fn prepare_recent_terminal_run(storage: &SharedStorage, run_id: &str) {
    prepare_new_run_with_session_thread(
        storage,
        SessionThreadStart {
            tracker: &SessionThread::default(),
            continue_existing: false,
        },
        run_id,
        "workspace-a",
        None,
        Vec::new(),
        None,
        "test",
        "1",
        || Ok(()),
    )
    .unwrap();
    let mut storage = storage.lock().unwrap();
    let started = storage.journal.events(run_id).unwrap().remove(0);
    storage
        .journal
        .append(
            1,
            &event(
                run_id,
                "01900000-0000-7000-8000-000000000004",
                2,
                "run.completed",
                &started.recorded_at,
            ),
        )
        .unwrap();
}

#[test]
fn deletes_an_expired_terminal_run_and_keeps_a_recent_run() {
    let temporary_root =
        std::env::temp_dir().join(format!("muniment-runtime-retention-{}", std::process::id()));
    fs::create_dir_all(&temporary_root).unwrap();
    let storage = open_profile_storage(&temporary_root).unwrap();
    let expired_run = "01900000-0000-7000-8000-000000000001";
    let recent_run = "01900000-0000-7000-8000-000000000002";
    {
        let mut storage = storage.lock().unwrap();
        storage
            .journal
            .append_batch(
                0,
                &[
                    event(
                        expired_run,
                        "01900000-0000-7000-8000-000000000001",
                        1,
                        "run.started",
                        "2000-01-01T00:00:00Z",
                    ),
                    event(
                        expired_run,
                        "01900000-0000-7000-8000-000000000002",
                        2,
                        "run.completed",
                        "2000-01-01T00:00:00Z",
                    ),
                ],
            )
            .unwrap();
    }
    prepare_recent_terminal_run(&storage, recent_run);

    let outcome = apply_retention(Arc::clone(&storage), 30 * 24 * 60 * 60).unwrap();

    assert_eq!(outcome.deleted_run_ids, [expired_run]);
    {
        let mut locked = storage.lock().unwrap();
        assert!(locked.journal.events(expired_run).unwrap().is_empty());
        assert_eq!(locked.journal.events(recent_run).unwrap().len(), 2);
    }
    drop(storage);
    fs::remove_dir_all(temporary_root).unwrap();
}
