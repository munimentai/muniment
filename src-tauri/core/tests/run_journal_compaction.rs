use muniment_core::journal::compaction::{CompactionError, CompactionFault};
use muniment_core::journal::{CasReference, EventEnvelope, EventPayload, Provenance, RunJournal};
use rusqlite::Connection;
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DB: AtomicU64 = AtomicU64::new(0);

struct TestDb(PathBuf);
impl TestDb {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "muniment-compaction-{}-{}.sqlite3",
            std::process::id(),
            NEXT_DB.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl AsRef<Path> for TestDb {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}
impl Drop for TestDb {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = fs::remove_file(format!("{}{}", self.0.display(), suffix));
        }
        if let Some(parent) = self.0.parent() {
            if let Some(name) = self.0.file_name() {
                let prefix = format!(".{}.compact-", name.to_string_lossy());
                if let Ok(entries) = fs::read_dir(parent) {
                    for entry in entries.flatten() {
                        if entry.file_name().to_string_lossy().starts_with(&prefix) {
                            let _ = fs::remove_file(entry.path());
                        }
                    }
                }
            }
        }
    }
}

const RUN: &str = "0190a100-0000-7000-8000-000000000001";
const DELETED_RUN: &str = "0190a100-0000-7000-8000-000000000002";
const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn event(run_id: &str, seq: u64, padding: usize) -> EventEnvelope {
    EventEnvelope {
        event_id: format!(
            "0190a100-0000-7000-8000-{:012}",
            seq + if run_id == RUN { 0 } else { 10_000 }
        ),
        run_id: run_id.into(),
        run_seq: seq,
        event_type: "future.event".into(),
        event_version: 1,
        envelope_version: 1,
        recorded_at: "2026-07-10T12:00:00Z".into(),
        occurred_at: None,
        correlation_id: None,
        causation_id: None,
        payload: if run_id == RUN && seq == 1 {
            EventPayload::Cas {
                payload_cas: CasReference {
                    sha256: HASH.into(),
                    media_type: "application/octet-stream".into(),
                    byte_length: 1,
                },
            }
        } else {
            EventPayload::Inline {
                payload_json: json!({"padding": "x".repeat(padding), "seq": seq}),
            }
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
        extra: BTreeMap::from([("unknown".into(), json!({"preserved": true}))]),
    }
}

fn raw_envelopes(path: &Path) -> Vec<String> {
    let connection = Connection::open(path).unwrap();
    let mut statement = connection
        .prepare("SELECT envelope_json FROM events ORDER BY run_id,run_seq")
        .unwrap();
    statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn compaction_reduces_churn_and_preserves_the_complete_authoritative_history() {
    let db = TestDb::new();
    let mut journal = RunJournal::open(db.as_ref()).unwrap();
    journal
        .append_batch(
            0,
            &(1..=3).map(|seq| event(RUN, seq, 32)).collect::<Vec<_>>(),
        )
        .unwrap();
    for seq in 1..=160 {
        journal
            .append(seq - 1, &event(DELETED_RUN, seq, 8_000))
            .unwrap();
    }
    journal.delete_run(DELETED_RUN).unwrap();

    let before_events = journal.events(RUN).unwrap();
    let before_hashes = journal.referenced_hashes().unwrap();
    let before_runs = journal.run_ids().unwrap();
    let before_raw = raw_envelopes(db.as_ref());
    let before_version: i64 = Connection::open(db.as_ref())
        .unwrap()
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    let before_size = fs::metadata(db.as_ref()).unwrap().len();

    journal.compact().unwrap();
    let after_size = fs::metadata(db.as_ref()).unwrap().len();
    assert!(
        after_size < before_size,
        "{after_size} was not less than {before_size}"
    );
    assert_eq!(journal.events(RUN).unwrap(), before_events);
    assert_eq!(journal.referenced_hashes().unwrap(), before_hashes);
    assert_eq!(journal.run_ids().unwrap(), before_runs);
    assert_eq!(raw_envelopes(db.as_ref()), before_raw);
    let after_version: i64 = Connection::open(db.as_ref())
        .unwrap()
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(after_version, before_version);

    drop(journal);
    assert_eq!(
        RunJournal::open(db.as_ref()).unwrap().events(RUN).unwrap(),
        before_events
    );
}

#[test]
fn committed_wal_events_are_included() {
    let db = TestDb::new();
    let mut journal = RunJournal::open(db.as_ref()).unwrap();
    journal.append(0, &event(RUN, 1, 32)).unwrap();
    assert!(PathBuf::from(format!("{}-wal", db.0.display())).exists());
    journal.compact().unwrap();
    drop(journal);
    assert_eq!(
        RunJournal::open(db.as_ref())
            .unwrap()
            .events(RUN)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn injected_failures_leave_the_original_reopenable_and_remove_temporary_files() {
    for fault in [
        CompactionFault::BeforeSnapshotCompletion,
        CompactionFault::BeforeReplacement,
        CompactionFault::DuringReplacement,
    ] {
        let db = TestDb::new();
        let mut journal = RunJournal::open(db.as_ref()).unwrap();
        journal
            .append_batch(0, &[event(RUN, 1, 32), event(RUN, 2, 32)])
            .unwrap();
        assert!(
            matches!(journal.compact_with_fault(Some(fault)), Err(CompactionError::Injected(found)) if found == fault)
        );
        drop(journal);
        assert_eq!(
            RunJournal::open(db.as_ref())
                .unwrap()
                .events(RUN)
                .unwrap()
                .len(),
            2
        );
        let prefix = format!(".{}.compact-", db.0.file_name().unwrap().to_string_lossy());
        assert!(!fs::read_dir(db.0.parent().unwrap())
            .unwrap()
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().starts_with(&prefix)));
    }
}

#[test]
fn in_memory_compaction_is_typed_and_non_destructive() {
    let mut journal = RunJournal::open(":memory:").unwrap();
    journal.append(0, &event(RUN, 1, 0)).unwrap();
    assert!(matches!(
        journal.compact(),
        Err(CompactionError::UnsupportedInMemory)
    ));
    assert_eq!(journal.events(RUN).unwrap().len(), 1);
}
