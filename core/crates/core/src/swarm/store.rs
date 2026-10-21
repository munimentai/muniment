//! The swarm's own records in `<profile>/swarm.sqlite3`: links, members, the
//! replicated records (the directory, groups and thread registrations among
//! them), the replication index and thread versions, the outbox and inbox,
//! forwarded actions, approvals, meetings and pending moves, the time of
//! each thread's newest event, and the ordinary threads this runtime asked
//! to hold in full.
//!
//! Thread content stays in the run journal. The replication index maps each
//! `(thread, origin, seq)` to the journal event that holds it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

pub(super) const STORE_FILE: &str = "swarm.sqlite3";

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS peers(
  runtime_id TEXT NOT NULL, direction TEXT NOT NULL, device_id TEXT, runtime_name TEXT,
  state TEXT NOT NULL, last_seen_at TEXT, last_error TEXT, created_at TEXT NOT NULL,
  PRIMARY KEY(runtime_id, direction));
CREATE TABLE IF NOT EXISTS members(runtime_id TEXT PRIMARY KEY, record TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS directory(agent_id TEXT PRIMARY KEY, entry TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS groups(group_id TEXT PRIMARY KEY, record TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS threads(thread_id TEXT PRIMARY KEY, record TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS log(
  thread_id TEXT NOT NULL, origin TEXT NOT NULL, seq INTEGER NOT NULL, kind TEXT NOT NULL,
  event_id TEXT NOT NULL, run_id TEXT, run_seq INTEGER, event_type TEXT,
  PRIMARY KEY(thread_id, origin, seq));
CREATE UNIQUE INDEX IF NOT EXISTS log_event ON log(event_id);
CREATE INDEX IF NOT EXISTS log_type ON log(event_type, thread_id);
CREATE TABLE IF NOT EXISTS runs(
  run_id TEXT PRIMARY KEY, thread_id TEXT NOT NULL, origin TEXT NOT NULL, indexed_seq INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS thread_marks(thread_id TEXT PRIMARY KEY, scanned_seq INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS consumed(event_id TEXT PRIMARY KEY, thread_id TEXT NOT NULL, at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS outbox(
  message_id TEXT PRIMARY KEY, message TEXT NOT NULL, to_runtime TEXT, created_at TEXT NOT NULL,
  relay INTEGER NOT NULL, acked INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS acks(message_id TEXT PRIMARY KEY, ack TEXT NOT NULL, at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS inbox(message_id TEXT PRIMARY KEY, ack TEXT NOT NULL, run_id TEXT);
CREATE TABLE IF NOT EXISTS routes(run_id TEXT PRIMARY KEY, message TEXT NOT NULL, done INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS notices(notice_id TEXT PRIMARY KEY, notice TEXT NOT NULL, created_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS approvals(approval_id TEXT PRIMARY KEY, record TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS meetings(group_id TEXT PRIMARY KEY, state TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS moves(agent_id TEXT PRIMARY KEY, bundle TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS blobs(
  hash TEXT PRIMARY KEY, thread_id TEXT, total INTEGER, requested INTEGER NOT NULL DEFAULT 0,
  done INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS records(
  class TEXT NOT NULL, key TEXT NOT NULL, field TEXT NOT NULL, value TEXT,
  clock INTEGER NOT NULL, origin TEXT NOT NULL, seq INTEGER NOT NULL,
  deleted INTEGER NOT NULL DEFAULT 0, rank INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY(class, key, field));
CREATE INDEX IF NOT EXISTS records_origin ON records(origin, seq);
CREATE TABLE IF NOT EXISTS record_vv(origin TEXT PRIMARY KEY, seq INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS ops(
  op_id TEXT PRIMARY KEY, record TEXT NOT NULL, actor_device TEXT NOT NULL,
  idempotency_key TEXT NOT NULL, operation TEXT NOT NULL, open INTEGER NOT NULL,
  handed INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL);
CREATE UNIQUE INDEX IF NOT EXISTS ops_key ON ops(actor_device, idempotency_key, operation);
CREATE INDEX IF NOT EXISTS ops_open ON ops(open, created_at);
CREATE TABLE IF NOT EXISTS op_inbox(
  op_id TEXT PRIMARY KEY, message TEXT NOT NULL, received_at TEXT NOT NULL,
  done INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS op_results(
  key TEXT PRIMARY KEY, op_id TEXT NOT NULL, state TEXT NOT NULL, result TEXT, note TEXT,
  at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS thread_versions(thread_id TEXT PRIMARY KEY, version INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS thread_versions_version ON thread_versions(version);
CREATE TABLE IF NOT EXISTS thread_activity(thread_id TEXT PRIMARY KEY, at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS pins(thread_id TEXT PRIMARY KEY, at TEXT NOT NULL);
-- The version trigger upserts: an OR clause in a trigger gives way to the
-- inserting statement's own, and the log inserts with OR IGNORE.
DROP TRIGGER IF EXISTS log_thread_version;
CREATE TRIGGER IF NOT EXISTS log_thread_version_upsert AFTER INSERT ON log BEGIN
  INSERT INTO thread_versions(thread_id, version)
  VALUES(NEW.thread_id, (SELECT COALESCE(MAX(version), 0) + 1 FROM thread_versions))
  ON CONFLICT(thread_id) DO UPDATE SET version=excluded.version;
END;
";

/// Numbers the threads a store indexed before thread versions existed, so
/// clock deltas reach them.
const BACKFILL_THREAD_VERSIONS: &str = "
INSERT OR IGNORE INTO thread_versions(thread_id, version)
SELECT thread_id, (SELECT COALESCE(MAX(version), 0) FROM thread_versions)
  + ROW_NUMBER() OVER (ORDER BY thread_id)
FROM (SELECT DISTINCT thread_id FROM log
  WHERE thread_id NOT IN (SELECT thread_id FROM thread_versions));
";

/// The most idle connections a store keeps for reuse.
const POOLED_CONNECTIONS: usize = 4;

/// The swarm store of one profile. It keeps a few idle connections, so the
/// many small reads of a pass do not each open the file.
#[derive(Clone)]
pub(super) struct Store {
    path: PathBuf,
    idle: Arc<Mutex<Vec<Connection>>>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Store")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

/// A connection to the store that goes back to its pool when dropped.
pub(super) struct Pooled {
    connection: Option<Connection>,
    idle: Arc<Mutex<Vec<Connection>>>,
}

impl std::ops::Deref for Pooled {
    type Target = Connection;

    fn deref(&self) -> &Connection {
        self.connection
            .as_ref()
            .expect("a pooled connection is present until it drops")
    }
}

impl std::ops::DerefMut for Pooled {
    fn deref_mut(&mut self) -> &mut Connection {
        self.connection
            .as_mut()
            .expect("a pooled connection is present until it drops")
    }
}

impl Drop for Pooled {
    fn drop(&mut self) {
        let Some(connection) = self.connection.take() else {
            return;
        };
        // A connection left inside a transaction is not reused.
        if !connection.is_autocommit() {
            return;
        }
        let mut idle = self
            .idle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if idle.len() < POOLED_CONNECTIONS {
            idle.push(connection);
        }
    }
}

impl Store {
    pub(super) fn open(profile: &Path) -> Result<Self, String> {
        let store = Self {
            path: profile.join(STORE_FILE),
            idle: Arc::new(Mutex::new(Vec::new())),
        };
        let connection = Connection::open(&store.path)
            .map_err(|error| format!("The swarm store could not be opened: {error}"))?;
        // One transaction keeps the schema and the backfill to a single commit.
        connection
            .execute_batch("BEGIN")
            .and_then(|()| connection.execute_batch(SCHEMA))
            .and_then(|()| connection.execute_batch(BACKFILL_THREAD_VERSIONS))
            .and_then(|()| connection.execute_batch("COMMIT"))
            .map_err(|error| {
                let _ = connection.execute_batch("ROLLBACK");
                format!("The swarm store could not be opened: {error}")
            })?;
        Ok(store)
    }

    /// Opens the store, or reuses an idle connection. A store removed since
    /// `open` stays removed.
    pub(super) fn connect(&self) -> Result<Pooled, String> {
        if !self.path.exists() {
            self.idle
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clear();
            return Err("The swarm store is unavailable: it was removed.".into());
        }
        let reused = self
            .idle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop();
        let connection = match reused {
            Some(connection) => connection,
            None => self.open_connection()?,
        };
        Ok(Pooled {
            connection: Some(connection),
            idle: Arc::clone(&self.idle),
        })
    }

    fn open_connection(&self) -> Result<Connection, String> {
        let connection = Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_URI
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|error| format!("The swarm store is unavailable: {error}"))?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|error| format!("The swarm store is unavailable: {error}"))?;
        let _ = connection.pragma_update(None, "journal_mode", "WAL");
        // Tests skip fsync so the large store scenarios finish in time.
        #[cfg(test)]
        let _ = connection.pragma_update(None, "synchronous", "OFF");
        Ok(connection)
    }

    pub(super) fn meta(&self, key: &str) -> Result<Option<String>, String> {
        self.connect()?
            .query_row("SELECT value FROM meta WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()
            .map_err(sql)
    }

    pub(super) fn set_meta(&self, key: &str, value: Option<&str>) -> Result<(), String> {
        let connection = self.connect()?;
        match value {
            Some(value) => connection.execute(
                "INSERT INTO meta(key, value) VALUES(?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![key, value],
            ),
            None => connection.execute("DELETE FROM meta WHERE key=?1", [key]),
        }
        .map(drop)
        .map_err(sql)
    }

    /// Reads every JSON record of a two-column table, such as `groups`.
    pub(super) fn records<T: serde::de::DeserializeOwned>(
        &self,
        table: &str,
        column: &str,
    ) -> Result<Vec<T>, String> {
        let connection = self.connect()?;
        let mut statement = connection
            .prepare(&format!("SELECT {column} FROM {table}"))
            .map_err(sql)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(sql)?;
        let mut records = Vec::new();
        for row in rows {
            if let Ok(record) = serde_json::from_str(&row.map_err(sql)?) {
                records.push(record);
            }
        }
        Ok(records)
    }

    /// Reads one JSON record by its key.
    pub(super) fn record<T: serde::de::DeserializeOwned>(
        &self,
        table: &str,
        key_column: &str,
        column: &str,
        key: &str,
    ) -> Result<Option<T>, String> {
        let raw: Option<String> = self
            .connect()?
            .query_row(
                &format!("SELECT {column} FROM {table} WHERE {key_column}=?1"),
                [key],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql)?;
        Ok(raw.and_then(|raw| serde_json::from_str(&raw).ok()))
    }

    /// Writes one JSON record by its key.
    pub(super) fn put_record<T: serde::Serialize>(
        &self,
        table: &str,
        key_column: &str,
        column: &str,
        key: &str,
        record: &T,
    ) -> Result<(), String> {
        let json = serde_json::to_string(record).map_err(|error| error.to_string())?;
        self.connect()?
            .execute(
                &format!(
                    "INSERT INTO {table}({key_column}, {column}) VALUES(?1, ?2) \
                     ON CONFLICT({key_column}) DO UPDATE SET {column}=excluded.{column}"
                ),
                params![key, json],
            )
            .map(drop)
            .map_err(sql)
    }

    pub(super) fn delete_record(
        &self,
        table: &str,
        key_column: &str,
        key: &str,
    ) -> Result<(), String> {
        self.connect()?
            .execute(&format!("DELETE FROM {table} WHERE {key_column}=?1"), [key])
            .map(drop)
            .map_err(sql)
    }
}

pub(super) fn sql(error: rusqlite::Error) -> String {
    format!("The swarm store failed: {error}")
}
