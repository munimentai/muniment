//! Replicated records: the one store every member keeps of the classes the
//! swarm shares, such as the agent directory and groups.
//!
//! - **Rows.** A record is a `(class, key)` with named fields. Each field is
//!   a last-writer-wins register ordered by `(rank, clock, origin)`: the
//!   rank a class gives the write (an agent's epoch, else 0), then the
//!   hybrid logical clock, then the writing member's id.
//! - **Tombstones.** Deleting a record writes a tombstone row (field `""`)
//!   that wins over every older field write; removing a field writes a
//!   deleted row for it. Tombstones last [`TOMBSTONE_DAYS`]; a member away
//!   longer must take a fresh snapshot.
//! - **Sets.** A set field holds elements, each under the tag of the write
//!   that added it. A removal deletes only the tags it saw, so an add made
//!   at the same time on another member keeps the element.
//! - **Replication.** Each member numbers its own writes. A row carries its
//!   origin's number, and the version vector says the highest number held
//!   from each origin, so an exchange carries only the rows a peer lacks.
//!   The table holds only each field's winning write, so a delta from an
//!   empty version vector is a snapshot of the live rows and tombstones.
//! - **Collection.** A deleted row older than [`TOMBSTONE_DAYS`] that every
//!   member still in touch holds is dropped. The newest clock dropped is the
//!   horizon: a row at or before it for a field this runtime no longer
//!   holds may be a write such a tombstone beat, so it is refused.

use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::hlc::{Hlc, HlcClock, SKEW_WARNING_MILLIS};
use super::store::{sql, Store};
use super::{lock, RecordRow, VersionVector};

/// The capability a member names when it trades records and thread clock
/// deltas.
pub const STORE_CAPABILITY: &str = "store/1";
/// How long a tombstone lasts. A member away longer takes a fresh snapshot.
pub const TOMBSTONE_DAYS: i64 = 90;
/// The largest value one field holds.
pub const MAX_VALUE_BYTES: usize = 192 * 1024;
const MAX_KEY_BYTES: usize = 256;
const MAX_CLASS_BYTES: usize = 64;
/// Separates a set's name from an element's tag in a field name.
const SET_SEPARATOR: char = '\u{1f}';
/// The meta key of the newest clock of a deleted row this runtime dropped.
const HORIZON: &str = "gc_horizon";

/// One change to a record, as subscribers see it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordChange {
    pub class: String,
    pub key: String,
    /// The field that changed, `""` for the whole record.
    pub field: String,
    /// The change removed the field or the record.
    pub deleted: bool,
    /// The member that wrote it.
    pub origin: String,
    /// This runtime wrote it.
    pub local: bool,
}

/// One field's live value and the write that set it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub value: Value,
    pub clock: Hlc,
    pub origin: String,
    pub rank: u64,
}

/// The write that deleted a record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tombstone {
    pub clock: Hlc,
    pub origin: String,
    pub rank: u64,
}

/// A record as this runtime holds it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub class: String,
    pub key: String,
    pub fields: BTreeMap<String, Field>,
    /// Each set's elements and their values.
    pub sets: BTreeMap<String, BTreeMap<String, Value>>,
    pub tombstone: Option<Tombstone>,
    /// The newest clock of any write to the record.
    pub clock: Hlc,
}

impl Record {
    /// Whether any field or set element is live.
    pub fn is_live(&self) -> bool {
        !self.fields.is_empty() || !self.sets.is_empty()
    }

    /// A field's value, if it is live.
    pub fn value(&self, field: &str) -> Option<&Value> {
        self.fields.get(field).map(|field| &field.value)
    }

    /// The newest clock of any live field.
    pub fn fields_clock(&self) -> Option<Hlc> {
        self.fields.values().map(|field| field.clock).max()
    }
}

/// How many rows and tombstones a class holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassCount {
    pub rows: u64,
    pub tombstones: u64,
}

type Version = (u64, Hlc, String);

fn version(row: &RecordRow) -> Version {
    (row.rank, row.clock, row.origin.clone())
}

/// The records of one profile in `swarm.sqlite3`.
pub(super) struct RecordStore {
    store: Store,
    clock: Mutex<HlcClock>,
    subscribers: Mutex<Vec<Sender<RecordChange>>>,
}

impl RecordStore {
    pub(super) fn open(store: Store) -> Result<Self, String> {
        let connection = store.connect()?;
        let saved: Option<String> = connection
            .query_row("SELECT value FROM meta WHERE key='hlc'", [], |row| {
                row.get(0)
            })
            .optional()
            .map_err(sql)?;
        let newest: Option<i64> = connection
            .query_row("SELECT MAX(clock) FROM records", [], |row| row.get(0))
            .map_err(sql)?;
        let saved = saved
            .and_then(|saved| saved.parse::<Hlc>().ok())
            .unwrap_or_default();
        let last = saved.max(newest.map(Hlc::from_packed).unwrap_or_default());
        Ok(Self {
            store,
            clock: Mutex::new(HlcClock::new(last)),
            subscribers: Mutex::new(Vec::new()),
        })
    }

    /// The last clock reading this runtime issued or saw.
    pub(super) fn clock(&self) -> Hlc {
        lock(&self.clock).last()
    }

    pub(super) fn subscribe(&self) -> Receiver<RecordChange> {
        let (sender, receiver) = mpsc::channel();
        lock(&self.subscribers).push(sender);
        receiver
    }

    fn notify(&self, changes: &[RecordChange]) {
        if changes.is_empty() {
            return;
        }
        lock(&self.subscribers).retain(|subscriber| {
            changes
                .iter()
                .all(|change| subscriber.send(change.clone()).is_ok())
        });
    }

    /// Sets a field. A write that an existing one outranks is refused with
    /// `None`, as is one that changes nothing.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn put(
        &self,
        origin: &str,
        now: DateTime<Utc>,
        class: &str,
        key: &str,
        field: &str,
        value: Value,
        rank: u64,
    ) -> Result<Option<RecordChange>, String> {
        check_field(field)?;
        self.write_local(
            origin,
            now,
            class,
            key,
            field.to_owned(),
            Some(value),
            rank,
            None,
        )
    }

    /// Sets a field with a clock read from an older member's full state
    /// instead of this runtime's clock.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn put_at(
        &self,
        origin: &str,
        now: DateTime<Utc>,
        class: &str,
        key: &str,
        field: &str,
        value: Value,
        rank: u64,
        clock: Hlc,
    ) -> Result<Option<RecordChange>, String> {
        check_field(field)?;
        self.write_local(
            origin,
            now,
            class,
            key,
            field.to_owned(),
            Some(value),
            rank,
            Some(clock),
        )
    }

    /// Removes a field.
    pub(super) fn remove_field(
        &self,
        origin: &str,
        now: DateTime<Utc>,
        class: &str,
        key: &str,
        field: &str,
        rank: u64,
    ) -> Result<Option<RecordChange>, String> {
        check_field(field)?;
        self.write_local(origin, now, class, key, field.to_owned(), None, rank, None)
    }

    /// Deletes a record: its tombstone wins over every older field write.
    pub(super) fn delete(
        &self,
        origin: &str,
        now: DateTime<Utc>,
        class: &str,
        key: &str,
        rank: u64,
        clock: Option<Hlc>,
    ) -> Result<Option<RecordChange>, String> {
        self.write_local(origin, now, class, key, String::new(), None, rank, clock)
    }

    /// Adds `element` to the set `set` with `value`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn set_add(
        &self,
        origin: &str,
        now: DateTime<Utc>,
        class: &str,
        key: &str,
        set: &str,
        element: &str,
        value: Value,
    ) -> Result<Option<RecordChange>, String> {
        check_field(set)?;
        if element.is_empty() || element.len() > MAX_KEY_BYTES {
            return Err("The set element is invalid.".into());
        }
        if self
            .get(class, key)?
            .and_then(|record| record.sets.get(set)?.get(element).cloned())
            == Some(value.clone())
        {
            return Ok(None);
        }
        let mut connection = self.store.connect()?;
        let transaction = connection.transaction().map_err(sql)?;
        let seq = next_seq(&transaction, origin)?;
        let row = RecordRow {
            class: class.to_owned(),
            key: key.to_owned(),
            field: format!("{set}{SET_SEPARATOR}{origin}.{seq}"),
            value: Some(serde_json::json!({"element": element, "value": value})),
            clock: lock(&self.clock).tick(now.timestamp_millis()),
            origin: origin.to_owned(),
            seq,
            rank: 0,
            deleted: false,
        };
        self.commit_local(transaction, row)
    }

    /// Removes `element` from the set `set`: every tag this runtime holds
    /// for it.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn set_remove(
        &self,
        origin: &str,
        now: DateTime<Utc>,
        class: &str,
        key: &str,
        set: &str,
        element: &str,
    ) -> Result<Option<RecordChange>, String> {
        check_field(set)?;
        let prefix = format!("{set}{SET_SEPARATOR}");
        let mut connection = self.store.connect()?;
        let transaction = connection.transaction().map_err(sql)?;
        let tags: Vec<String> = {
            let mut statement = transaction
                .prepare(
                    "SELECT field, value FROM records WHERE class=?1 AND key=?2 AND deleted=0 \
                     AND substr(field, 1, length(?3))=?3",
                )
                .map_err(sql)?;
            let rows = statement
                .query_map(params![class, key, prefix], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                })
                .map_err(sql)?;
            let mut tags = Vec::new();
            for row in rows {
                let (field, value) = row.map_err(sql)?;
                let held = value
                    .and_then(|value| serde_json::from_str::<Value>(&value).ok())
                    .is_some_and(|value| value["element"] == element);
                if held {
                    tags.push(field);
                }
            }
            tags
        };
        if tags.is_empty() {
            return Ok(None);
        }
        let mut change = None;
        for field in tags {
            let seq = next_seq(&transaction, origin)?;
            let row = RecordRow {
                class: class.to_owned(),
                key: key.to_owned(),
                field,
                value: None,
                clock: lock(&self.clock).tick(now.timestamp_millis()),
                origin: origin.to_owned(),
                seq,
                rank: 0,
                deleted: true,
            };
            change = apply(&transaction, &row, true)?.or(change);
        }
        save_clock(&transaction, lock(&self.clock).last())?;
        transaction.commit().map_err(sql)?;
        let change = change.map(|mut change| {
            change.field = set.to_owned();
            change
        });
        self.notify(change.as_slice());
        Ok(change)
    }

    #[allow(clippy::too_many_arguments)]
    fn write_local(
        &self,
        origin: &str,
        now: DateTime<Utc>,
        class: &str,
        key: &str,
        field: String,
        value: Option<Value>,
        rank: u64,
        at: Option<Hlc>,
    ) -> Result<Option<RecordChange>, String> {
        check_name(class, key)?;
        if let Some(value) = &value {
            if value.to_string().len() > MAX_VALUE_BYTES {
                return Err("The record value is too large.".into());
            }
        }
        let mut connection = self.store.connect()?;
        let transaction = connection.transaction().map_err(sql)?;
        // A write an existing one outranks would lose, and a write that
        // repeats the live value changes nothing.
        let current = current_row(&transaction, class, key, &field)?;
        let tombstone = if field.is_empty() {
            None
        } else {
            current_row(&transaction, class, key, "")?
        };
        for existing in current.iter().chain(&tombstone) {
            if existing.rank > rank {
                return Ok(None);
            }
        }
        let unchanged = if field.is_empty() {
            !has_live_fields(&transaction, class, key)?
        } else {
            match &current {
                Some(current) => current.rank == rank && current.value == value,
                None => value.is_none(),
            }
        };
        if unchanged {
            return Ok(None);
        }
        let seq = next_seq(&transaction, origin)?;
        let row = RecordRow {
            class: class.to_owned(),
            key: key.to_owned(),
            deleted: value.is_none(),
            field,
            value,
            clock: match at {
                Some(at) => {
                    lock(&self.clock).observe(at, now.timestamp_millis());
                    at
                }
                None => lock(&self.clock).tick(now.timestamp_millis()),
            },
            origin: origin.to_owned(),
            seq,
            rank,
        };
        self.commit_local(transaction, row)
    }

    fn commit_local(
        &self,
        transaction: Transaction<'_>,
        row: RecordRow,
    ) -> Result<Option<RecordChange>, String> {
        let change = apply(&transaction, &row, true)?;
        save_clock(&transaction, lock(&self.clock).last())?;
        transaction.commit().map_err(sql)?;
        let change = change.map(|mut change| {
            if let Some((set, _)) = change.field.split_once(SET_SEPARATOR) {
                change.field = set.to_owned();
            }
            change
        });
        self.notify(change.as_slice());
        Ok(change)
    }

    /// Takes in rows another member sent and answers what changed.
    pub(super) fn merge(
        &self,
        now: DateTime<Utc>,
        rows: Vec<RecordRow>,
    ) -> Result<Vec<RecordChange>, String> {
        if rows.is_empty() {
            return Ok(Vec::new());
        }
        let now_millis = now.timestamp_millis();
        let horizon = self.horizon()?;
        let mut connection = self.store.connect()?;
        let transaction = connection.transaction().map_err(sql)?;
        let mut changes = Vec::new();
        let mut skew = 0;
        for mut row in rows {
            // A field set to JSON null crosses the wire without its value.
            if !row.deleted && row.value.is_none() {
                row.value = Some(Value::Null);
            }
            if check_name(&row.class, &row.key).is_err()
                || row.field.len() > MAX_KEY_BYTES
                || row.origin.is_empty()
                || row.origin.len() > MAX_CLASS_BYTES * 2
                || row.seq == 0
                || row.deleted != row.value.is_none()
                || row
                    .value
                    .as_ref()
                    .is_some_and(|value| value.to_string().len() > MAX_VALUE_BYTES)
            {
                continue;
            }
            skew = skew.max(lock(&self.clock).observe(row.clock, now_millis));
            transaction
                .execute(
                    "INSERT INTO record_vv(origin, seq) VALUES(?1, ?2) \
                     ON CONFLICT(origin) DO UPDATE SET seq=MAX(seq, excluded.seq)",
                    params![row.origin, row.seq as i64],
                )
                .map_err(sql)?;
            if horizon.is_some_and(|horizon| row.clock <= horizon)
                && current_row(&transaction, &row.class, &row.key, &row.field)?.is_none()
            {
                continue;
            }
            if let Some(mut change) = apply(&transaction, &row, false)? {
                if let Some((set, _)) = change.field.split_once(SET_SEPARATOR) {
                    change.field = set.to_owned();
                }
                if !changes.contains(&change) {
                    changes.push(change);
                }
            }
        }
        save_clock(&transaction, lock(&self.clock).last())?;
        transaction.commit().map_err(sql)?;
        if skew > SKEW_WARNING_MILLIS {
            crate::runtime_eprintln!(
                "muniment-runtime: swarm record clock ahead of this machine by {} s",
                skew / 1000
            );
        }
        self.notify(&changes);
        Ok(changes)
    }

    /// The highest number this runtime holds from each origin.
    pub(super) fn version_vector(&self) -> Result<VersionVector, String> {
        let connection = self.store.connect()?;
        let mut statement = connection
            .prepare("SELECT origin, seq FROM record_vv")
            .map_err(sql)?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
            })
            .map_err(sql)?;
        rows.collect::<Result<_, _>>().map_err(sql)
    }

    /// The rows a peer with version vector `peer` lacks, in number order per
    /// origin, within `budget` bytes. Answers whether rows were left out.
    pub(super) fn delta(
        &self,
        peer: &VersionVector,
        budget: &mut usize,
    ) -> Result<(Vec<RecordRow>, bool), String> {
        let connection = self.store.connect()?;
        let mut rows = Vec::new();
        for (origin, held) in self.version_vector()? {
            let known = peer.get(&origin).copied().unwrap_or(0);
            if held <= known {
                continue;
            }
            let mut statement = connection
                .prepare(
                    "SELECT class, key, field, value, clock, origin, seq, deleted, rank FROM records \
                     WHERE origin=?1 AND seq>?2 ORDER BY seq",
                )
                .map_err(sql)?;
            let found = statement
                .query_map(params![origin, known as i64], read_row)
                .map_err(sql)?;
            for row in found {
                let row = row.map_err(sql)?;
                let size = serde_json::to_vec(&row).map_or(usize::MAX, |bytes| bytes.len());
                if size > *budget {
                    return Ok((rows, true));
                }
                *budget -= size;
                rows.push(row);
            }
        }
        Ok((rows, false))
    }

    /// The record `(class, key)`, if this runtime holds any write to it.
    pub(super) fn get(&self, class: &str, key: &str) -> Result<Option<Record>, String> {
        get_with(&*self.store.connect()?, class, key)
    }

    /// The records of a class in key order after `after`, at most `limit`.
    pub(super) fn scan(
        &self,
        class: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Record>, String> {
        let connection = self.store.connect()?;
        let mut statement = connection
            .prepare(
                "SELECT class, key, field, value, clock, origin, seq, deleted, rank FROM records \
                 WHERE class=?1 AND key IN (SELECT DISTINCT key FROM records WHERE class=?1 AND key>?2 \
                 ORDER BY key LIMIT ?3) ORDER BY key",
            )
            .map_err(sql)?;
        let rows = statement
            .query_map(
                params![
                    class,
                    after.unwrap_or(""),
                    limit.min(i64::MAX as usize) as i64
                ],
                read_row,
            )
            .map_err(sql)?;
        let mut records = Vec::new();
        let mut current: Vec<RecordRow> = Vec::new();
        for row in rows {
            let row = row.map_err(sql)?;
            if current.first().is_some_and(|first| first.key != row.key) {
                let key = current[0].key.clone();
                records.extend(assemble(class, &key, std::mem::take(&mut current)));
            }
            current.push(row);
        }
        if let Some(first) = current.first() {
            let key = first.key.clone();
            records.extend(assemble(class, &key, current));
        }
        Ok(records)
    }

    /// The newest clock of a deleted row this runtime dropped.
    pub(super) fn horizon(&self) -> Result<Option<Hlc>, String> {
        Ok(self
            .store
            .meta(HORIZON)?
            .and_then(|horizon| horizon.parse::<Hlc>().ok()))
    }

    /// Drops the deleted rows written before `before` that `covered` says
    /// every member still in touch holds, and moves the horizon past them.
    /// Answers how many it dropped.
    pub(super) fn collect(
        &self,
        before: Hlc,
        covered: impl Fn(&str, u64) -> bool,
    ) -> Result<usize, String> {
        let mut connection = self.store.connect()?;
        let transaction = connection.transaction().map_err(sql)?;
        let candidates: Vec<RecordRow> = {
            let mut statement = transaction
                .prepare(
                    "SELECT class, key, field, value, clock, origin, seq, deleted, rank FROM records \
                     WHERE deleted=1 AND clock<?1",
                )
                .map_err(sql)?;
            let rows = statement
                .query_map([before.packed()], read_row)
                .map_err(sql)?;
            rows.collect::<Result<_, _>>().map_err(sql)?
        };
        let mut dropped = 0;
        let mut newest = self.horizon()?.unwrap_or_default();
        for row in candidates {
            if !covered(&row.origin, row.seq) {
                continue;
            }
            transaction
                .execute(
                    "DELETE FROM records WHERE class=?1 AND key=?2 AND field=?3",
                    params![row.class, row.key, row.field],
                )
                .map_err(sql)?;
            newest = newest.max(row.clock);
            dropped += 1;
        }
        if dropped > 0 {
            transaction
                .execute(
                    "INSERT INTO meta(key, value) VALUES(?1, ?2) \
                     ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    params![HORIZON, newest.to_string()],
                )
                .map_err(sql)?;
        }
        transaction.commit().map_err(sql)?;
        Ok(dropped)
    }

    /// Forgets every row but the ones `own` numbered after `delivered`, and
    /// what it held of other members, so a member that was away past
    /// [`TOMBSTONE_DAYS`] takes a fresh snapshot. Its own write numbers stay,
    /// so it never reuses one.
    pub(super) fn forget(&self, own: &str, delivered: u64) -> Result<(), String> {
        let mut connection = self.store.connect()?;
        let transaction = connection.transaction().map_err(sql)?;
        transaction
            .execute(
                "DELETE FROM records WHERE NOT (origin=?1 AND seq>?2)",
                params![own, delivered as i64],
            )
            .map_err(sql)?;
        transaction
            .execute("DELETE FROM record_vv WHERE origin<>?1", [own])
            .map_err(sql)?;
        transaction
            .execute("DELETE FROM meta WHERE key=?1", [HORIZON])
            .map_err(sql)?;
        transaction.commit().map_err(sql)
    }

    /// The rows and tombstones of each class.
    pub(super) fn counts(&self) -> Result<BTreeMap<String, ClassCount>, String> {
        let connection = self.store.connect()?;
        let mut statement = connection
            .prepare("SELECT class, COUNT(*), SUM(deleted) FROM records GROUP BY class")
            .map_err(sql)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    ClassCount {
                        rows: row.get::<_, i64>(1)? as u64,
                        tombstones: row.get::<_, Option<i64>>(2)?.unwrap_or(0) as u64,
                    },
                ))
            })
            .map_err(sql)?;
        rows.collect::<Result<_, _>>().map_err(sql)
    }
}

/// The record `(class, key)` read over `connection`, so a caller that reads
/// many records opens the store once.
pub(super) fn get_with(
    connection: &rusqlite::Connection,
    class: &str,
    key: &str,
) -> Result<Option<Record>, String> {
    let mut statement = connection
        .prepare_cached(
            "SELECT class, key, field, value, clock, origin, seq, deleted, rank FROM records \
             WHERE class=?1 AND key=?2",
        )
        .map_err(sql)?;
    let rows = statement
        .query_map(params![class, key], read_row)
        .map_err(sql)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql)?;
    Ok(assemble(class, key, rows))
}

fn check_name(class: &str, key: &str) -> Result<(), String> {
    let class_valid = !class.is_empty()
        && class.len() <= MAX_CLASS_BYTES
        && class.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
        });
    if !class_valid
        || key.is_empty()
        || key.len() > MAX_KEY_BYTES
        || key.chars().any(char::is_control)
    {
        return Err("The record class or key is invalid.".into());
    }
    Ok(())
}

fn check_field(field: &str) -> Result<(), String> {
    if field.is_empty()
        || field.len() > MAX_KEY_BYTES / 2
        || field.contains(SET_SEPARATOR)
        || field.chars().any(char::is_control)
    {
        return Err("The record field is invalid.".into());
    }
    Ok(())
}

fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecordRow> {
    let value: Option<String> = row.get(3)?;
    Ok(RecordRow {
        class: row.get(0)?,
        key: row.get(1)?,
        field: row.get(2)?,
        value: value.and_then(|value| serde_json::from_str(&value).ok()),
        clock: Hlc::from_packed(row.get(4)?),
        origin: row.get(5)?,
        seq: row.get::<_, i64>(6)? as u64,
        deleted: row.get::<_, i64>(7)? != 0,
        rank: row.get::<_, i64>(8)? as u64,
    })
}

fn current_row(
    transaction: &Transaction<'_>,
    class: &str,
    key: &str,
    field: &str,
) -> Result<Option<RecordRow>, String> {
    transaction
        .query_row(
            "SELECT class, key, field, value, clock, origin, seq, deleted, rank FROM records \
             WHERE class=?1 AND key=?2 AND field=?3",
            params![class, key, field],
            read_row,
        )
        .optional()
        .map_err(sql)
}

fn has_live_fields(transaction: &Transaction<'_>, class: &str, key: &str) -> Result<bool, String> {
    transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM records WHERE class=?1 AND key=?2 AND field<>'' AND deleted=0)",
            params![class, key],
            |row| row.get(0),
        )
        .map_err(sql)
}

fn next_seq(transaction: &Transaction<'_>, origin: &str) -> Result<u64, String> {
    let seq: i64 = transaction
        .query_row(
            "SELECT COALESCE((SELECT seq FROM record_vv WHERE origin=?1), 0) + 1",
            [origin],
            |row| row.get(0),
        )
        .map_err(sql)?;
    transaction
        .execute(
            "INSERT INTO record_vv(origin, seq) VALUES(?1, ?2) \
             ON CONFLICT(origin) DO UPDATE SET seq=excluded.seq",
            params![origin, seq],
        )
        .map_err(sql)?;
    Ok(seq as u64)
}

fn save_clock(transaction: &Transaction<'_>, clock: Hlc) -> Result<(), String> {
    transaction
        .execute(
            "INSERT INTO meta(key, value) VALUES('hlc', ?1) \
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [clock.to_string()],
        )
        .map(drop)
        .map_err(sql)
}

/// Keeps `row` when it wins over what the store holds and answers the
/// visible change.
fn apply(
    transaction: &Transaction<'_>,
    row: &RecordRow,
    local: bool,
) -> Result<Option<RecordChange>, String> {
    let incoming = version(row);
    let current = current_row(transaction, &row.class, &row.key, &row.field)?;
    if current
        .as_ref()
        .is_some_and(|current| incoming <= version(current))
    {
        return Ok(None);
    }
    if !row.field.is_empty() {
        if let Some(tombstone) = current_row(transaction, &row.class, &row.key, "")? {
            if incoming <= version(&tombstone) {
                return Ok(None);
            }
        }
    }
    let was_live = if row.field.is_empty() {
        has_live_fields(transaction, &row.class, &row.key)?
    } else {
        current.as_ref().is_some_and(|current| !current.deleted)
    };
    let value = row
        .value
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO records(class, key, field, value, clock, origin, seq, deleted, rank) \
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) ON CONFLICT(class, key, field) DO UPDATE SET \
             value=excluded.value, clock=excluded.clock, origin=excluded.origin, seq=excluded.seq, \
             deleted=excluded.deleted, rank=excluded.rank",
            params![
                row.class,
                row.key,
                row.field,
                value,
                row.clock.packed(),
                row.origin,
                row.seq as i64,
                row.deleted,
                row.rank as i64
            ],
        )
        .map_err(sql)?;
    if row.field.is_empty() {
        // The tombstone wins over every older field write of the record.
        transaction
            .execute(
                "DELETE FROM records WHERE class=?1 AND key=?2 AND field<>'' AND \
                 (rank<?3 OR (rank=?3 AND (clock<?4 OR (clock=?4 AND origin<?5))))",
                params![
                    row.class,
                    row.key,
                    row.rank as i64,
                    row.clock.packed(),
                    row.origin
                ],
            )
            .map_err(sql)?;
        let live = has_live_fields(transaction, &row.class, &row.key)?;
        if was_live == live {
            return Ok(None);
        }
    } else {
        let changed = match &current {
            Some(current) if !current.deleted && !row.deleted => current.value != row.value,
            _ => was_live != !row.deleted,
        };
        if !changed {
            return Ok(None);
        }
    }
    Ok(Some(RecordChange {
        class: row.class.clone(),
        key: row.key.clone(),
        field: row.field.clone(),
        deleted: row.deleted,
        origin: row.origin.clone(),
        local,
    }))
}

fn assemble(class: &str, key: &str, rows: Vec<RecordRow>) -> Option<Record> {
    if rows.is_empty() {
        return None;
    }
    let mut record = Record {
        class: class.to_owned(),
        key: key.to_owned(),
        ..Record::default()
    };
    for row in rows {
        record.clock = record.clock.max(row.clock);
        if row.field.is_empty() {
            record.tombstone = Some(Tombstone {
                clock: row.clock,
                origin: row.origin,
                rank: row.rank,
            });
            continue;
        }
        if row.deleted {
            continue;
        }
        let value = row.value.unwrap_or(Value::Null);
        match row.field.split_once(SET_SEPARATOR) {
            Some((set, _)) => {
                if let Some(element) = value["element"].as_str() {
                    record
                        .sets
                        .entry(set.to_owned())
                        .or_default()
                        .insert(element.to_owned(), value["value"].clone());
                }
            }
            None => {
                record.fields.insert(
                    row.field,
                    Field {
                        value,
                        clock: row.clock,
                        origin: row.origin,
                        rank: row.rank,
                    },
                );
            }
        }
    }
    Some(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Member {
        id: &'static str,
        records: RecordStore,
        _root: tempfile_dir::Dir,
    }

    mod tempfile_dir {
        pub struct Dir(pub std::path::PathBuf);
        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    fn member(id: &'static str) -> Member {
        let root =
            std::env::temp_dir().join(format!("swarm-records-{id}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store = Store::open(&root).unwrap();
        Member {
            id,
            records: RecordStore::open(store).unwrap(),
            _root: tempfile_dir::Dir(root),
        }
    }

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_790_000_000 + seconds, 0).unwrap()
    }

    fn all_rows(member: &Member) -> Vec<RecordRow> {
        member
            .records
            .delta(&VersionVector::new(), &mut usize::MAX.to_owned())
            .unwrap()
            .0
    }

    fn put(member: &Member, seconds: i64, key: &str, field: &str, value: Value) {
        member
            .records
            .put(member.id, at(seconds), "note", key, field, value, 0)
            .unwrap();
    }

    fn snapshot(member: &Member) -> Vec<Record> {
        member.records.scan("note", None, 100).unwrap()
    }

    #[test]
    fn concurrent_field_writes_on_three_members_converge_in_any_order() {
        let orders: [[usize; 3]; 6] = [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ];
        let mut results = Vec::new();
        for order in orders {
            let writers = [member("a"), member("b"), member("c")];
            // Each writes the same field at once, and its own field.
            put(&writers[0], 10, "k", "title", json!("from a"));
            put(&writers[1], 10, "k", "title", json!("from b"));
            put(&writers[2], 9, "k", "title", json!("from c"));
            put(&writers[0], 11, "k", "color", json!("red"));
            put(&writers[1], 12, "k", "size", json!(3));
            put(&writers[2], 13, "k", "color", json!("blue"));
            let batches: Vec<Vec<RecordRow>> = writers.iter().map(all_rows).collect();
            let reader = member("r");
            for index in order {
                // Deliver one row at a time in reverse, too, to vary order
                // within a batch.
                for row in batches[index].iter().rev() {
                    reader.records.merge(at(20), vec![row.clone()]).unwrap();
                }
            }
            for writer in &writers {
                for index in order {
                    writer
                        .records
                        .merge(at(20), batches[index].clone())
                        .unwrap();
                }
            }
            let state = snapshot(&reader);
            for writer in &writers {
                assert_eq!(
                    snapshot(writer)[0].fields.keys().collect::<Vec<_>>(),
                    state[0].fields.keys().collect::<Vec<_>>()
                );
                let values = |record: &Record| {
                    record
                        .fields
                        .iter()
                        .map(|(name, field)| (name.clone(), field.value.clone()))
                        .collect::<Vec<_>>()
                };
                assert_eq!(values(&snapshot(writer)[0]), values(&state[0]));
            }
            results.push(state[0].clone());
        }
        let first = &results[0];
        assert!(results
            .iter()
            .all(|result| result.fields.len() == first.fields.len()));
        // Equal clocks fall to the higher origin; later clocks win.
        assert_eq!(first.value("title"), Some(&json!("from b")));
        assert_eq!(first.value("color"), Some(&json!("blue")));
        assert_eq!(first.value("size"), Some(&json!(3)));
        for result in &results {
            assert_eq!(result.value("title"), first.value("title"));
            assert_eq!(result.value("color"), first.value("color"));
        }
    }

    #[test]
    fn a_tombstone_wins_over_older_writes_and_loses_to_newer_ones() {
        let a = member("a");
        let b = member("b");
        put(&a, 10, "k", "title", json!("old"));
        put(&b, 12, "k", "color", json!("red"));
        let older = all_rows(&b);
        // A deletes after both writes, then receives B's older field.
        a.records.delete("a", at(20), "note", "k", 0, None).unwrap();
        a.records.merge(at(21), older).unwrap();
        let record = a.records.get("note", "k").unwrap().unwrap();
        assert!(!record.is_live());
        assert!(record.tombstone.is_some());
        // B gets the tombstone after its write: the record is gone there too.
        b.records.merge(at(21), all_rows(&a)).unwrap();
        assert!(!b.records.get("note", "k").unwrap().unwrap().is_live());
        // A write after the tombstone lives.
        put(&b, 30, "k", "title", json!("new"));
        a.records.merge(at(31), all_rows(&b)).unwrap();
        let record = a.records.get("note", "k").unwrap().unwrap();
        assert_eq!(record.value("title"), Some(&json!("new")));
        assert!(record.value("color").is_none());
        // A removed field reads as absent.
        a.records
            .remove_field("a", at(40), "note", "k", "title", 0)
            .unwrap();
        assert!(!a.records.get("note", "k").unwrap().unwrap().is_live());
    }

    #[test]
    fn a_higher_rank_outranks_a_later_clock() {
        let a = member("a");
        let b = member("b");
        a.records
            .put("a", at(10), "agent", "x", "entry", json!("moved"), 2)
            .unwrap();
        // B still holds epoch 1 and writes later in time.
        b.records
            .put("b", at(50), "agent", "x", "entry", json!("stale"), 1)
            .unwrap();
        a.records.merge(at(51), all_rows(&b)).unwrap();
        b.records.merge(at(51), all_rows(&a)).unwrap();
        for member in [&a, &b] {
            let record = member.records.get("agent", "x").unwrap().unwrap();
            assert_eq!(record.value("entry"), Some(&json!("moved")));
        }
        // A local write below the held rank is refused.
        assert!(b
            .records
            .put("b", at(60), "agent", "x", "entry", json!("again"), 1)
            .unwrap()
            .is_none());
    }

    #[test]
    fn a_set_keeps_an_element_added_while_another_member_removed_it() {
        let a = member("a");
        let b = member("b");
        let add = |member: &Member, seconds, element: &str| {
            member
                .records
                .set_add(
                    member.id,
                    at(seconds),
                    "rules",
                    "deny",
                    "items",
                    element,
                    json!({"tool": element}),
                )
                .unwrap()
        };
        let set = |member: &Member| {
            member
                .records
                .get("rules", "deny")
                .unwrap()
                .map(|record| {
                    record
                        .sets
                        .get("items")
                        .map(|items| items.keys().cloned().collect::<Vec<_>>())
                        .unwrap_or_default()
                })
                .unwrap_or_default()
        };
        add(&a, 10, "bash");
        add(&a, 11, "write");
        b.records.merge(at(12), all_rows(&a)).unwrap();
        assert_eq!(set(&b), vec!["bash", "write"]);
        // A removes bash while B adds it again; B also removes write.
        a.records
            .set_remove("a", at(20), "rules", "deny", "items", "bash")
            .unwrap();
        b.records
            .set_remove("b", at(19), "rules", "deny", "items", "bash")
            .unwrap();
        add(&b, 21, "bash");
        b.records
            .set_remove("b", at(22), "rules", "deny", "items", "write")
            .unwrap();
        a.records.merge(at(30), all_rows(&b)).unwrap();
        b.records.merge(at(30), all_rows(&a)).unwrap();
        assert_eq!(set(&a), vec!["bash"]);
        assert_eq!(set(&b), vec!["bash"]);
        // Adding a held element with the same value changes nothing.
        assert!(add(&a, 40, "bash").is_none());
    }

    #[test]
    fn the_delta_carries_only_what_a_peer_lacks_and_relays_converge() {
        let a = member("a");
        let b = member("b");
        let c = member("c");
        for index in 0..20 {
            put(&a, index, &format!("k{index}"), "n", json!(index));
        }
        let (rows, more) = a
            .records
            .delta(
                &b.records.version_vector().unwrap(),
                &mut usize::MAX.to_owned(),
            )
            .unwrap();
        assert_eq!((rows.len(), more), (20, false));
        // A small budget sends a prefix and says more is left.
        let mut budget = 600;
        let (prefix, more) = a
            .records
            .delta(&b.records.version_vector().unwrap(), &mut budget)
            .unwrap();
        assert!(more && !prefix.is_empty() && prefix.len() < 20);
        b.records.merge(at(30), prefix).unwrap();
        let (rest, _) = a
            .records
            .delta(
                &b.records.version_vector().unwrap(),
                &mut usize::MAX.to_owned(),
            )
            .unwrap();
        assert_eq!(
            rest.len(),
            20 - b.records.scan("note", None, 100).unwrap().len()
        );
        b.records.merge(at(31), rest).unwrap();
        assert!(a
            .records
            .delta(
                &b.records.version_vector().unwrap(),
                &mut usize::MAX.to_owned()
            )
            .unwrap()
            .0
            .is_empty());
        // C hears A's writes only through B.
        let (relayed, _) = b
            .records
            .delta(
                &c.records.version_vector().unwrap(),
                &mut usize::MAX.to_owned(),
            )
            .unwrap();
        c.records.merge(at(32), relayed).unwrap();
        assert_eq!(snapshot(&c), snapshot(&a));
        let counts = c.records.counts().unwrap();
        assert_eq!(counts["note"].rows, 20);
    }

    #[test]
    fn subscribers_see_remote_and_local_changes_once() {
        let a = member("a");
        let b = member("b");
        let changes = b.records.subscribe();
        put(&a, 10, "k", "title", json!("x"));
        b.records.merge(at(11), all_rows(&a)).unwrap();
        b.records.merge(at(12), all_rows(&a)).unwrap();
        put(&b, 13, "k", "title", json!("y"));
        let seen: Vec<RecordChange> = changes.try_iter().collect();
        assert_eq!(seen.len(), 2);
        assert!(!seen[0].local && seen[0].origin == "a");
        assert!(seen[1].local && seen[1].field == "title");
    }

    #[test]
    fn the_clock_survives_reopening_the_store() {
        let a = member("a");
        put(&a, 10_000, "k", "title", json!("x"));
        let before = a.records.clock();
        let reopened = RecordStore::open(Store::open(&a._root.0).unwrap()).unwrap();
        assert_eq!(reopened.clock(), before);
        reopened
            .put("a", at(0), "note", "k", "title", json!("y"), 0)
            .unwrap();
        let record = reopened.get("note", "k").unwrap().unwrap();
        assert!(record.fields["title"].clock > before);
        assert_eq!(record.value("title"), Some(&json!("y")));
    }
}
