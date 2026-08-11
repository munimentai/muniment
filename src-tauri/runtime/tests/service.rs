use muniment_core::journal::{EventEnvelope, EventPayload, Provenance};
use muniment_runtime::open_profile_storage;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct ProfileDirectory(PathBuf);

impl ProfileDirectory {
    fn new() -> Self {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "muniment-runtime-service-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for ProfileDirectory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn started_event(run_id: &str) -> EventEnvelope {
    EventEnvelope {
        event_id: "018f0000-0000-7000-8000-000000000001".into(),
        run_id: run_id.into(),
        run_seq: 1,
        event_type: "run.started".into(),
        event_version: 1,
        envelope_version: 1,
        recorded_at: "2026-08-11T00:00:00Z".into(),
        occurred_at: None,
        correlation_id: None,
        causation_id: None,
        payload: EventPayload::Inline {
            payload_json: "{}".parse().unwrap(),
        },
        provenance: Provenance {
            source: "service-test".into(),
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
fn opens_fresh_profile_storage() {
    let profile = ProfileDirectory::new();

    let storage = open_profile_storage(&profile.0).unwrap();
    let mut storage = storage.lock().unwrap();

    assert!(profile.0.join("runs.sqlite3").is_file());
    assert!(profile.0.join("cas").is_dir());
    assert!(storage.journal.run_event_types().unwrap().is_empty());
}

#[test]
fn reconciles_an_interrupted_run_when_reopened() {
    let profile = ProfileDirectory::new();
    let run_id = "018f0000-0000-7000-8000-000000000002";
    {
        let storage = open_profile_storage(&profile.0).unwrap();
        storage
            .lock()
            .unwrap()
            .journal
            .append(0, &started_event(run_id))
            .unwrap();
    }

    let storage = open_profile_storage(&profile.0).unwrap();
    let mut storage = storage.lock().unwrap();
    let events = storage.journal.events(run_id).unwrap();

    assert_eq!(events.len(), 2);
    assert_eq!(events[1].event_type, "run.needs_attention");
    assert_eq!(events[1].provenance.source, "muniment-runtime");
    assert_eq!(
        events[1].provenance.source_version,
        env!("CARGO_PKG_VERSION")
    );
}
