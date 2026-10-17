//! Journal reads and appends that swarm replication builds on.
//!
//! A swarm member replicates agent and group threads by copying journal
//! events between runtimes. A replica thread keeps the primary's thread id,
//! a replicated run keeps its run id and run sequence, and a replicated
//! thread event keeps its event id while it takes the next local thread
//! sequence. Posted messages are thread events of type
//! `thread.message.posted`.

use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde_json::Value;

use super::{
    append_thread_created_with_provenance, canonical_thread_envelope, validate_thread_envelope,
    EventEnvelope, JournalError, Provenance, RunJournal, ThreadEventEnvelope,
};

/// The thread event a user, an agent or the runtime posts to a thread
/// without a run.
pub const THREAD_MESSAGE_POSTED: &str = "thread.message.posted";
/// The longest posted message, in bytes.
pub const MAX_POSTED_TEXT_BYTES: usize = 64 * 1024;

/// The workspace and attach profile a thread was created with.
pub type ThreadScope = (Option<String>, Option<String>);

impl ThreadEventEnvelope {
    pub fn event_id(&self) -> &str {
        &self.event_id
    }

    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    pub fn thread_seq(&self) -> u64 {
        self.thread_seq
    }

    pub fn recorded_at(&self) -> &str {
        &self.recorded_at
    }

    pub fn payload(&self) -> &Value {
        &self.payload_json
    }

    /// A thread event for `thread_id` that a caller appends with
    /// [`RunJournal::append_thread_event_with_id`]. Its sequence is set at
    /// the append.
    pub fn new(
        event_id: &str,
        thread_id: &str,
        event_type: &str,
        recorded_at: &str,
        payload_json: Value,
        provenance: Provenance,
    ) -> Self {
        Self {
            event_id: event_id.to_owned(),
            thread_id: thread_id.to_owned(),
            thread_seq: 1,
            event_type: event_type.to_owned(),
            event_version: 1,
            envelope_version: 1,
            recorded_at: recorded_at.to_owned(),
            payload_json,
            provenance,
        }
    }
}

/// Checks a `thread.message.posted` payload: its message id, author, text
/// and the vector clock the poster had seen.
pub(super) fn valid_posted_payload(payload: &Value) -> bool {
    let Some(object) = payload.as_object() else {
        return false;
    };
    let message_id = object
        .get("message_id")
        .and_then(Value::as_str)
        .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok());
    let author = object
        .get("author")
        .and_then(Value::as_object)
        .and_then(|author| author.get("kind"))
        .and_then(Value::as_str)
        .is_some_and(|kind| matches!(kind, "user" | "agent" | "system"));
    let text = object
        .get("text")
        .and_then(Value::as_str)
        .is_some_and(|text| !text.trim().is_empty() && text.len() <= MAX_POSTED_TEXT_BYTES);
    let deps = object.get("deps").is_none_or(|deps| {
        deps.as_object()
            .is_some_and(|deps| deps.values().all(Value::is_u64))
    });
    message_id && author && text && deps
}

impl RunJournal {
    /// Whether a thread was created and not deleted.
    pub fn thread_exists(&mut self, thread_id: &str) -> Result<bool, JournalError> {
        let coordination = self.coordination.clone();
        let _operation = coordination
            .as_ref()
            .map(|state| state.operation.lock().unwrap());
        self.refresh_after_compaction()?;
        self.connection
            .as_ref()
            .expect("journal connection is always present outside compaction")
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM thread_events WHERE thread_id=?1 AND thread_seq=1) \
                 AND NOT EXISTS(SELECT 1 FROM thread_events WHERE thread_id=?1 \
                 AND event_type='thread.deleted')",
                [thread_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    /// Creates a thread under a given id, the id its primary runtime chose,
    /// and answers whether it was created. An existing thread stays as it is.
    pub fn create_thread_with_id(
        &mut self,
        thread_id: &str,
        workspace: &str,
        recorded_at: &str,
        provenance: Provenance,
    ) -> Result<bool, JournalError> {
        if workspace.is_empty()
            || !provenance
                .extra
                .get("attach_profile")
                .and_then(Value::as_str)
                .is_some_and(|profile| !profile.is_empty())
        {
            return Err(JournalError::InvalidEnvelope(
                "thread workspace and attach profile must be valid".into(),
            ));
        }
        let coordination = self.coordination.clone();
        let _operation = coordination
            .as_ref()
            .map(|state| state.operation.lock().unwrap());
        self.refresh_after_compaction()?;
        let tx = self
            .connection
            .as_mut()
            .expect("journal connection is always present outside compaction")
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM thread_events WHERE thread_id=?1)",
            [thread_id],
            |row| row.get(0),
        )?;
        if exists {
            return Ok(false);
        }
        append_thread_created_with_provenance(
            &tx,
            thread_id,
            Some(workspace),
            recorded_at,
            false,
            provenance,
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// Each run of a thread, in thread order, with its last run sequence.
    pub fn thread_run_marks(
        &mut self,
        thread_id: &str,
    ) -> Result<Vec<(String, u64)>, JournalError> {
        let coordination = self.coordination.clone();
        let _operation = coordination
            .as_ref()
            .map(|state| state.operation.lock().unwrap());
        self.refresh_after_compaction()?;
        let connection = self
            .connection
            .as_ref()
            .expect("journal connection is always present outside compaction");
        let mut statement = connection.prepare(
            "SELECT rt.run_id, COALESCE((SELECT MAX(run_seq) FROM events e WHERE e.run_id=rt.run_id), 0) \
             FROM run_threads rt WHERE rt.thread_id=?1 ORDER BY rt.thread_run_ordinal",
        )?;
        let rows = statement.query_map([thread_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// The last run sequence of a run, or 0 when it has none.
    pub fn last_run_seq(&mut self, run_id: &str) -> Result<u64, JournalError> {
        let coordination = self.coordination.clone();
        let _operation = coordination
            .as_ref()
            .map(|state| state.operation.lock().unwrap());
        self.refresh_after_compaction()?;
        self.connection
            .as_ref()
            .expect("journal connection is always present outside compaction")
            .query_row(
                "SELECT COALESCE(MAX(run_seq), 0) FROM events WHERE run_id=?1",
                [run_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    /// Up to `limit` events of a run after `after_seq`, in order.
    pub fn run_events_after(
        &mut self,
        run_id: &str,
        after_seq: u64,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, JournalError> {
        let coordination = self.coordination.clone();
        let _operation = coordination
            .as_ref()
            .map(|state| state.operation.lock().unwrap());
        self.refresh_after_compaction()?;
        let connection = self
            .connection
            .as_ref()
            .expect("journal connection is always present outside compaction");
        let mut statement = connection.prepare(
            "SELECT envelope_json FROM events WHERE run_id=?1 AND run_seq>?2 ORDER BY run_seq LIMIT ?3",
        )?;
        let rows = statement.query_map(params![run_id, after_seq, limit as i64], |row| {
            row.get::<_, String>(0)
        })?;
        rows.map(|row| {
            serde_json::from_str(&row?)
                .map_err(|e| JournalError::Corrupt(format!("invalid stored envelope JSON: {e}")))
        })
        .collect()
    }

    /// One event of a run by its sequence.
    pub fn run_event(
        &mut self,
        run_id: &str,
        run_seq: u64,
    ) -> Result<Option<EventEnvelope>, JournalError> {
        Ok(self
            .run_events_after(run_id, run_seq.saturating_sub(1), 1)?
            .into_iter()
            .find(|event| event.run_seq == run_seq))
    }

    /// The thread events after `after_seq`, in thread order.
    pub fn thread_events_after(
        &mut self,
        thread_id: &str,
        after_seq: u64,
    ) -> Result<Vec<ThreadEventEnvelope>, JournalError> {
        let coordination = self.coordination.clone();
        let _operation = coordination
            .as_ref()
            .map(|state| state.operation.lock().unwrap());
        self.refresh_after_compaction()?;
        let connection = self
            .connection
            .as_ref()
            .expect("journal connection is always present outside compaction");
        let mut statement = connection.prepare(
            "SELECT envelope_json FROM thread_events WHERE thread_id=?1 AND thread_seq>?2 ORDER BY thread_seq",
        )?;
        let rows =
            statement.query_map(params![thread_id, after_seq], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            serde_json::from_str(&row?).map_err(|error| {
                JournalError::Corrupt(format!("invalid stored thread envelope JSON: {error}"))
            })
        })
        .collect()
    }

    /// One thread event by its event id.
    pub fn thread_event_by_id(
        &mut self,
        event_id: &str,
    ) -> Result<Option<ThreadEventEnvelope>, JournalError> {
        let coordination = self.coordination.clone();
        let _operation = coordination
            .as_ref()
            .map(|state| state.operation.lock().unwrap());
        self.refresh_after_compaction()?;
        let raw: Option<String> = self
            .connection
            .as_ref()
            .expect("journal connection is always present outside compaction")
            .query_row(
                "SELECT envelope_json FROM thread_events WHERE event_id=?1",
                [event_id],
                |row| row.get(0),
            )
            .optional()?;
        raw.map(|raw| {
            serde_json::from_str(&raw).map_err(|error| {
                JournalError::Corrupt(format!("invalid stored thread envelope JSON: {error}"))
            })
        })
        .transpose()
    }

    /// Appends a thread event under its own event id at the thread's next
    /// sequence and answers the stored event. An event id already stored
    /// answers the stored event and appends nothing, so a replicated event
    /// that arrives twice is kept once.
    pub fn append_thread_event_with_id(
        &mut self,
        mut event: ThreadEventEnvelope,
    ) -> Result<ThreadEventEnvelope, JournalError> {
        let coordination = self.coordination.clone();
        let _operation = coordination
            .as_ref()
            .map(|state| state.operation.lock().unwrap());
        self.refresh_after_compaction()?;
        let tx = self
            .connection
            .as_mut()
            .expect("journal connection is always present outside compaction")
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let stored: Option<String> = tx
            .query_row(
                "SELECT envelope_json FROM thread_events WHERE event_id=?1",
                [&event.event_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(stored) = stored {
            return serde_json::from_str(&stored).map_err(|error| {
                JournalError::Corrupt(format!("invalid stored thread envelope JSON: {error}"))
            });
        }
        let actual: u64 = tx.query_row(
            "SELECT COALESCE(MAX(thread_seq), 0) FROM thread_events WHERE thread_id=?1",
            [&event.thread_id],
            |row| row.get(0),
        )?;
        if actual == 0 {
            return Err(JournalError::InvalidEnvelope(
                "thread does not exist".into(),
            ));
        }
        let deleted: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM thread_events \
             WHERE thread_id=?1 AND event_type='thread.deleted')",
            [&event.thread_id],
            |row| row.get(0),
        )?;
        if deleted {
            return Err(JournalError::InvalidEnvelope("thread is deleted".into()));
        }
        // A thread's permission mode replicates from its owner; its
        // policies stay with the runtime that decided them.
        if event.event_type == "thread.created"
            || (event.event_type.starts_with("thread.permission.")
                && event.event_type != super::THREAD_PERMISSION_MODE_SET)
        {
            return Err(JournalError::InvalidEnvelope(
                "this thread event cannot be replicated".into(),
            ));
        }
        event.thread_seq = actual + 1;
        validate_thread_envelope(&event)?;
        let canonical = canonical_thread_envelope(&event)?;
        tx.execute(
            "INSERT INTO thread_events(event_id,thread_id,thread_seq,event_type,event_version,\
             envelope_version,recorded_at,envelope_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                event.event_id,
                event.thread_id,
                event.thread_seq,
                event.event_type,
                event.event_version,
                event.envelope_version,
                event.recorded_at,
                canonical
            ],
        )?;
        tx.commit()?;
        Ok(event)
    }

    /// Whether the journal holds a run event with this event id.
    pub fn run_event_exists(&mut self, event_id: &str) -> Result<bool, JournalError> {
        let coordination = self.coordination.clone();
        let _operation = coordination
            .as_ref()
            .map(|state| state.operation.lock().unwrap());
        self.refresh_after_compaction()?;
        self.connection
            .as_ref()
            .expect("journal connection is always present outside compaction")
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM events WHERE event_id=?1)",
                [event_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    /// The threads whose run events or thread events were stored after the
    /// row marks `run_mark` and `thread_mark`, with the newest marks. A mark
    /// past the newest row, as after a compaction renumbers rows, reads as
    /// zero, so the caller rescans.
    pub fn threads_changed_after(
        &mut self,
        run_mark: i64,
        thread_mark: i64,
    ) -> Result<(Vec<String>, i64, i64), JournalError> {
        let coordination = self.coordination.clone();
        let _operation = coordination
            .as_ref()
            .map(|state| state.operation.lock().unwrap());
        self.refresh_after_compaction()?;
        let connection = self
            .connection
            .as_ref()
            .expect("journal connection is always present outside compaction");
        let (run_max, thread_max): (i64, i64) = connection.query_row(
            "SELECT (SELECT COALESCE(MAX(rowid), 0) FROM events), \
             (SELECT COALESCE(MAX(rowid), 0) FROM thread_events)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let run_mark = if run_mark > run_max { 0 } else { run_mark };
        let thread_mark = if thread_mark > thread_max {
            0
        } else {
            thread_mark
        };
        let mut statement = connection.prepare(
            "SELECT DISTINCT rt.thread_id FROM events e JOIN run_threads rt ON rt.run_id=e.run_id \
             WHERE e.rowid>?1 AND e.rowid<=?2 \
             UNION SELECT DISTINCT thread_id FROM thread_events WHERE rowid>?3 AND rowid<=?4",
        )?;
        let rows = statement
            .query_map(params![run_mark, run_max, thread_mark, thread_max], |row| {
                row.get::<_, String>(0)
            })?;
        let threads = rows.collect::<Result<Vec<_>, _>>()?;
        Ok((threads, run_max, thread_max))
    }

    /// The workspace and attach profile a thread was created with.
    pub fn thread_scope(&mut self, thread_id: &str) -> Result<Option<ThreadScope>, JournalError> {
        let coordination = self.coordination.clone();
        let _operation = coordination
            .as_ref()
            .map(|state| state.operation.lock().unwrap());
        self.refresh_after_compaction()?;
        self.connection
            .as_ref()
            .expect("journal connection is always present outside compaction")
            .query_row(
                "SELECT json_extract(envelope_json,'$.payload_json.workspace'), \
                 json_extract(envelope_json,'$.provenance.attach_profile') \
                 FROM thread_events WHERE thread_id=?1 AND thread_seq=1",
                [thread_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn provenance() -> Provenance {
        let mut extra = BTreeMap::new();
        extra.insert("attach_profile".into(), Value::from("desktop-owner"));
        Provenance {
            source: "test".into(),
            source_version: "1".into(),
            actor_id: None,
            device_id: None,
            rpc_request_id: None,
            capability_versions: None,
            extra,
        }
    }

    #[test]
    fn a_replica_thread_keeps_its_id_and_a_posted_message_appends_once() {
        let mut journal = RunJournal::open(":memory:").unwrap();
        let thread = uuid::Uuid::now_v7().to_string();
        assert!(journal
            .create_thread_with_id(&thread, "local", "2026-01-01T00:00:00Z", provenance())
            .unwrap());
        assert!(!journal
            .create_thread_with_id(&thread, "local", "2026-01-01T00:00:00Z", provenance())
            .unwrap());
        assert!(journal.thread_exists(&thread).unwrap());
        let message_id = uuid::Uuid::now_v7().to_string();
        let event = ThreadEventEnvelope::new(
            &message_id,
            &thread,
            THREAD_MESSAGE_POSTED,
            "2026-01-01T00:00:01Z",
            serde_json::json!({
                "message_id": message_id, "author": {"kind": "user"}, "text": "Hello", "deps": {}
            }),
            provenance(),
        );
        let stored = journal.append_thread_event_with_id(event.clone()).unwrap();
        assert_eq!(stored.thread_seq(), 2);
        let again = journal.append_thread_event_with_id(event).unwrap();
        assert_eq!(again.thread_seq(), 2);
        assert_eq!(journal.thread_events_after(&thread, 1).unwrap().len(), 1);
        assert_eq!(
            journal
                .thread_event_by_id(&message_id)
                .unwrap()
                .unwrap()
                .payload()["text"],
            "Hello"
        );
        let bad = ThreadEventEnvelope::new(
            &uuid::Uuid::now_v7().to_string(),
            &thread,
            THREAD_MESSAGE_POSTED,
            "2026-01-01T00:00:01Z",
            serde_json::json!({"message_id": "x", "author": {"kind": "user"}, "text": ""}),
            provenance(),
        );
        assert!(journal.append_thread_event_with_id(bad).is_err());
        assert_eq!(
            journal.thread_scope(&thread).unwrap(),
            Some((Some("local".into()), Some("desktop-owner".into())))
        );
    }
}
