//! Thread replication on the run journal.
//!
//! Each runtime indexes the journal events it wrote into a replicated thread
//! as `(thread, origin = itself, seq)` in journal order, and the events it
//! copied from another member under their own origin. An exchange carries
//! the events a peer's clocks lack, oldest first; the receiver appends them to
//! its journal under the same thread, run and event ids. A posted user
//! message carries the clock its poster had seen, and the thread's primary
//! consumes it once it holds every event that clock names.
//!
//! - **Ordinary threads.** A thread this runtime created registers as
//!   `plain` with this runtime as its owner once it has a run. Every member
//!   learns of it through its record, which also holds a summary (title,
//!   owner, newest post). A member holds the history of the ordinary threads
//!   active within its offline window, and of the ones it pinned; other
//!   threads show as their summary until a client fetches them.
//! - **Incremental work.** The scan reads only the threads the journal
//!   changed since its last pass, with a full pass every 10 minutes, and an
//!   export to a peer looks only at threads whose clock changed since its
//!   last look and at those the peer still lacked.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::PathBuf;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

use super::records::RecordChange;
use super::store::sql;
use super::{
    directory, groups, BlobChunk, BlobWant, Clocks, Inner, LastPost, ReplicaScope, ReplicatedEvent,
    ThreadRegistration, ThreadSummaryRecord,
};
use crate::agent_runs::{NewRun, RunStore, RunTrigger, TriggerKind};
use crate::journal::replica::{MAX_POSTED_TEXT_BYTES, THREAD_MESSAGE_POSTED};
use crate::journal::{EventEnvelope, EventPayload, Provenance, ThreadEventEnvelope};
use crate::run_events::SharedStorage;

/// The thread event the primary appends once it consumed a posted message.
pub const THREAD_MESSAGE_CONSUMED: &str = "thread.message.consumed";
/// The thread events a member replicates. Creation and permission policies
/// stay with the runtime that wrote them; the permission mode and a rewind
/// replicate from the thread's owner.
const REPLICATED_THREAD_EVENTS: [&str; 6] = [
    THREAD_MESSAGE_POSTED,
    THREAD_MESSAGE_CONSUMED,
    "thread.title.renamed",
    "thread.deleted",
    crate::journal::THREAD_PERMISSION_MODE_SET,
    crate::journal::THREAD_REWOUND,
];
/// The registration kind of an ordinary thread.
pub(super) const PLAIN: &str = "plain";
/// The provenance source of the threads and events the swarm writes.
const SWARM_SOURCE: &str = "muniment-swarm";
/// How often the scan reads every thread instead of the changed ones.
const FULL_SCAN_SECONDS: i64 = 10 * 60;
/// An owner rewrites a summary whose title and newest post are unchanged
/// only once its newest event is this much later.
const SUMMARY_REFRESH_SECONDS: i64 = 60 * 60;
/// The longest text a summary keeps of the newest post.
const LAST_POST_CHARS: usize = 280;
/// The provenance field that names the member an imported event came from.
pub const SWARM_ORIGIN: &str = "swarm_origin";
const MAX_ROWS_PER_ORIGIN: i64 = 400;
/// The most bytes of one blob slice. Its base64 text stays within the
/// longest string an attach frame carries, or the link refuses the frame.
const MAX_BLOB_CHUNK: usize = muniment_attach::MAX_TEXT_LENGTH / 4 * 3;

/// One index row: its sequence, kind, event id, run id and run sequence.
type LogRow = (u64, String, String, Option<String>, Option<u64>);

fn journal_error(error: impl std::fmt::Display) -> String {
    format!("The journal failed: {error}")
}

pub(super) fn storage_lock(
    storage: &SharedStorage,
) -> Result<std::sync::MutexGuard<'_, crate::run_events::ChatStorage>, String> {
    storage
        .lock()
        .map_err(|_| "The journal is unavailable.".to_string())
}

pub(super) fn registrations(inner: &Inner) -> Result<Vec<ThreadRegistration>, String> {
    inner.store.records("threads", "record")
}

pub(super) fn registration(inner: &Inner, thread_id: &str) -> Option<ThreadRegistration> {
    inner
        .store
        .record("threads", "thread_id", "record", thread_id)
        .ok()
        .flatten()
}

/// Sends `group.posted` to this runtime's clients when `thread_id` is a
/// group's thread: the post as `thread.history` lists it.
fn announce_post(inner: &Inner, thread_id: &str, payload: &Value, posted_at: &str) {
    let Some(group_id) = registration(inner, thread_id)
        .filter(|registration| registration.kind == "group")
        .and_then(|registration| registration.group_id)
    else {
        return;
    };
    inner.emit(
        muniment_attach::EventName::GroupPosted,
        json!({
            "group_id": group_id,
            "thread_id": thread_id,
            "post": {
                "message_id": payload["message_id"],
                "author": payload["author"],
                "text": payload["text"],
                "origin_runtime": payload["origin_runtime"],
                "posted_at": posted_at,
                "consumed": false,
                "run_id": null,
            },
        }),
    );
}

/// The record class of thread registrations, kept in the `threads` table
/// that the replication reads.
pub(super) const CLASS: &str = "thread";
pub(super) const REGISTRATION: &str = "registration";
/// The record field of an ordinary thread's summary.
const SUMMARY: &str = "summary";

/// Registers a thread this runtime runs or learned of from an older member,
/// and writes its record so `store/1` members learn of it too.
pub(super) fn ensure_registration(
    inner: &Inner,
    registration: &ThreadRegistration,
) -> Result<bool, String> {
    if !keep_registration(inner, registration)? {
        return Ok(false);
    }
    put_registration(inner, registration)?;
    Ok(true)
}

/// Keeps a registration in the `threads` table unless one is held.
fn keep_registration(inner: &Inner, registration: &ThreadRegistration) -> Result<bool, String> {
    if registration_valid(registration)
        && self::registration(inner, &registration.thread_id).is_none()
    {
        inner.store.put_record(
            "threads",
            "thread_id",
            "record",
            &registration.thread_id,
            registration,
        )?;
        return Ok(true);
    }
    Ok(false)
}

fn put_registration(inner: &Inner, registration: &ThreadRegistration) -> Result<(), String> {
    let Ok(origin) = inner.origin() else {
        return Ok(());
    };
    let value = serde_json::to_value(registration).map_err(|error| error.to_string())?;
    inner.records.put(
        &origin,
        inner.now(),
        CLASS,
        &registration.thread_id,
        REGISTRATION,
        value,
        0,
    )?;
    Ok(())
}

/// Registers the threads another member's records name, shows the summary
/// of each ordinary thread this runtime does not hold, and forgets a thread
/// its owner deleted. Answers whether the thread list changed.
pub(super) fn on_changes(inner: &Inner, changes: &[RecordChange]) -> Result<bool, String> {
    let mut changed = false;
    let mut keys: Vec<&str> = Vec::new();
    for change in changes
        .iter()
        .filter(|change| change.class == CLASS && !change.local)
    {
        if change.deleted && change.field.is_empty() {
            changed |= forget_thread(inner, &change.key)?;
        } else if !change.deleted && !keys.contains(&change.key.as_str()) {
            keys.push(&change.key);
        }
    }
    if keys.is_empty() {
        return Ok(changed);
    }
    // One transaction registers a page of threads at once.
    let mut plain = Vec::new();
    {
        let mut connection = inner.store.connect()?;
        let transaction = connection.transaction().map_err(sql)?;
        for key in keys {
            let registration = super::records::get_with(&transaction, CLASS, key)?
                .and_then(|record| record.value(REGISTRATION).cloned())
                .and_then(|value| serde_json::from_value::<ThreadRegistration>(value).ok())
                .filter(|registration| registration.thread_id == key);
            let Some(registration) = registration else {
                continue;
            };
            if registration_valid(&registration) {
                let added = transaction
                    .execute(
                        "INSERT OR IGNORE INTO threads(thread_id, record) VALUES(?1, ?2)",
                        params![
                            registration.thread_id,
                            serde_json::to_string(&registration)
                                .map_err(|error| error.to_string())?
                        ],
                    )
                    .map_err(sql)?;
                changed |= added > 0;
            }
            if registration.kind == PLAIN {
                plain.push(registration.thread_id);
            }
        }
        transaction.commit().map_err(sql)?;
    }
    if inner.host.replica_scope().is_some() {
        for thread_id in plain {
            changed |= show_summary(inner, &thread_id)?;
        }
    }
    Ok(changed)
}

/// Writes a record for each registration a store held before records.
pub(super) fn migrate(inner: &Inner) -> Result<(), String> {
    for registration in registrations(inner)? {
        if inner.records.get(CLASS, &registration.thread_id)?.is_none() {
            put_registration(inner, &registration)?;
        }
    }
    Ok(())
}

fn registration_valid(registration: &ThreadRegistration) -> bool {
    uuid::Uuid::parse_str(&registration.thread_id).is_ok()
        && match registration.kind.as_str() {
            "agent" => registration.agent_id.is_some(),
            "group" => registration.group_id.is_some(),
            PLAIN => registration.owner_runtime.is_some(),
            _ => false,
        }
}

/// Registers the threads of the agents and groups this runtime runs.
pub(super) fn register_local(inner: &Inner) -> Result<(), String> {
    let Some(self_id) = inner.runtime_id() else {
        return Ok(());
    };
    let state = crate::agents::state(&inner.profile).unwrap_or_default();
    let local = directory::local_agents(inner);
    let mut added = false;
    for (thread, agent_id) in &state.threads {
        if local
            .iter()
            .any(|agent| &agent.id == agent_id && directory::runs_here(inner, agent))
        {
            added |= ensure_registration(
                inner,
                &ThreadRegistration {
                    thread_id: thread.clone(),
                    kind: "agent".into(),
                    agent_id: Some(agent_id.clone()),
                    group_id: None,
                    updated_at: inner.now_text(),
                    owner_runtime: None,
                },
            )?;
        }
    }
    for group in groups::all(inner)? {
        if group.owner_runtime == self_id && !group.deleted {
            added |= ensure_registration(
                inner,
                &ThreadRegistration {
                    thread_id: group.thread_id.clone(),
                    kind: "group".into(),
                    agent_id: None,
                    group_id: Some(group.group_id.clone()),
                    updated_at: inner.now_text(),
                    owner_runtime: None,
                },
            )?;
        }
    }
    if added {
        inner.changed("threads");
    }
    Ok(())
}

pub(super) fn register_agent_thread(
    inner: &Inner,
    thread_id: &str,
    agent_id: &str,
) -> Result<(), String> {
    ensure_registration(
        inner,
        &ThreadRegistration {
            thread_id: thread_id.to_owned(),
            kind: "agent".into(),
            agent_id: Some(agent_id.to_owned()),
            group_id: None,
            updated_at: inner.now_text(),
            owner_runtime: None,
        },
    )
    .map(drop)
}

pub(super) fn merge_registrations(
    inner: &Inner,
    registrations: &[ThreadRegistration],
) -> Result<(), String> {
    for registration in registrations {
        ensure_registration(inner, registration)?;
    }
    Ok(())
}

/// Registers an ordinary thread this runtime created, once it has a run.
/// A thread the swarm created, an agent's thread and a group's are not
/// ordinary threads.
fn register_plain(inner: &Inner, self_id: &str, thread_id: &str) -> Result<bool, String> {
    let ordinary = {
        let mut storage = storage_lock(&inner.storage)?;
        let journal = &mut storage.journal;
        if !journal.thread_exists(thread_id).map_err(journal_error)?
            || journal
                .thread_run_marks(thread_id)
                .map_err(journal_error)?
                .is_empty()
        {
            return Ok(false);
        }
        journal
            .thread_events_after(thread_id, 0)
            .map_err(journal_error)?
            .first()
            .is_some_and(|created| {
                created.provenance.source != SWARM_SOURCE
                    && !created.provenance.extra.contains_key(SWARM_ORIGIN)
            })
    };
    if !ordinary {
        return Ok(false);
    }
    let state = crate::agents::state(&inner.profile).unwrap_or_default();
    if state.threads.contains_key(thread_id)
        || groups::all(inner)?
            .iter()
            .any(|group| group.thread_id == thread_id)
    {
        return Ok(false);
    }
    ensure_registration(
        inner,
        &ThreadRegistration {
            thread_id: thread_id.to_owned(),
            kind: PLAIN.into(),
            agent_id: None,
            group_id: None,
            updated_at: inner.now_text(),
            owner_runtime: Some(self_id.to_owned()),
        },
    )
}

/// The runtime that writes run output into a registered thread.
pub(super) fn thread_primary(inner: &Inner, registration: &ThreadRegistration) -> Option<String> {
    match registration.kind.as_str() {
        "agent" => directory::primary_of(inner, registration.agent_id.as_deref()?),
        "group" => {
            groups::get(inner, registration.group_id.as_deref()?).map(|group| group.owner_runtime)
        }
        PLAIN => registration.owner_runtime.clone(),
        _ => None,
    }
}

/// The primary of a replicated thread that another member runs.
pub(super) fn replica_primary(inner: &Inner, thread_id: &str) -> Option<String> {
    let registration = registration(inner, thread_id)?;
    let primary = thread_primary(inner, &registration)?;
    (inner.runtime_id().as_deref() != Some(primary.as_str())).then_some(primary)
}

/// The summary an ordinary thread's owner wrote, if any.
pub(super) fn summary(inner: &Inner, thread_id: &str) -> Option<ThreadSummaryRecord> {
    inner
        .records
        .get(CLASS, thread_id)
        .ok()
        .flatten()
        .and_then(|record| record.value(SUMMARY).cloned())
        .and_then(|value| serde_json::from_value(value).ok())
}

/// Every ordinary thread's summary, by thread.
pub(super) fn summaries(inner: &Inner) -> Result<BTreeMap<String, ThreadSummaryRecord>, String> {
    Ok(inner
        .records
        .scan(CLASS, None, usize::MAX)?
        .into_iter()
        .filter_map(|record| {
            let summary = serde_json::from_value(record.value(SUMMARY)?.clone()).ok()?;
            Some((record.key, summary))
        })
        .collect())
}

/// Shows an ordinary thread another member owns in this runtime's thread
/// list: a thread under its id, created at its newest event's time, that
/// takes the summary's title. Its history arrives when it is active within
/// the offline window or a client fetches it.
fn show_summary(inner: &Inner, thread_id: &str) -> Result<bool, String> {
    let Some(summary) = summary(inner, thread_id) else {
        return Ok(false);
    };
    if inner.runtime_id().as_deref() == Some(summary.owner_runtime.as_str()) {
        return Ok(false);
    }
    let Some(scope) = inner.host.replica_scope() else {
        return Ok(false);
    };
    let mut storage = storage_lock(&inner.storage)?;
    let journal = &mut storage.journal;
    let created = journal
        .create_thread_with_id(
            thread_id,
            &scope.workspace,
            &summary.last_at,
            imported_provenance(&scope, &summary.owner_runtime),
        )
        .map_err(journal_error)?;
    if !journal.thread_exists(thread_id).map_err(journal_error)? {
        return Ok(false);
    }
    let title = journal
        .summaries_of_threads(&[thread_id.to_owned()])
        .map_err(journal_error)?
        .into_iter()
        .next()
        .map(|held| held.title);
    let wanted = summary.title.trim();
    if !wanted.is_empty() && title.as_deref() != Some(wanted) {
        journal
            .append_thread_event_with_id(ThreadEventEnvelope::new(
                &uuid::Uuid::now_v7().to_string(),
                thread_id,
                "thread.title.renamed",
                &summary.last_at,
                json!({"title": wanted}),
                imported_provenance(&scope, &summary.owner_runtime),
            ))
            .map_err(journal_error)?;
        return Ok(true);
    }
    Ok(created)
}

/// The provenance of an event this runtime writes on another member's
/// behalf, which the scan never indexes as its own.
fn imported_provenance(scope: &ReplicaScope, origin: &str) -> Provenance {
    let mut provenance = local_provenance(Some(scope), &scope.attach_profile);
    provenance
        .extra
        .insert(SWARM_ORIGIN.into(), Value::from(origin));
    provenance
}

/// Forgets a thread whose owner deleted it: its registration goes, and its
/// copy here is deleted. Answers whether this runtime held it.
fn forget_thread(inner: &Inner, thread_id: &str) -> Result<bool, String> {
    let Some(registration) = registration(inner, thread_id) else {
        return Ok(false);
    };
    inner
        .store
        .delete_record("threads", "thread_id", thread_id)?;
    let owner = thread_primary(inner, &registration).unwrap_or_default();
    if inner.runtime_id().as_deref() == Some(owner.as_str()) {
        return Ok(true);
    }
    delete_copy(inner, thread_id, &owner)?;
    Ok(true)
}

/// Deletes this runtime's copy of a thread `owner` runs.
/// Drops this runtime's copy of a thread `owner` runs: its registration
/// and index here, and the thread in the journal.
pub(super) fn drop_replica(inner: &Inner, thread_id: &str, owner: &str) -> Result<(), String> {
    let connection = inner.store.connect()?;
    for table in ["threads", "log", "thread_marks", "thread_versions", "blobs"] {
        connection
            .execute(
                &format!("DELETE FROM {table} WHERE thread_id=?1"),
                [thread_id],
            )
            .map_err(sql)?;
    }
    delete_copy(inner, thread_id, owner)
}

pub(super) fn delete_copy(inner: &Inner, thread_id: &str, owner: &str) -> Result<(), String> {
    let scope = inner.host.replica_scope();
    let mut storage = storage_lock(&inner.storage)?;
    let journal = &mut storage.journal;
    if let Some(scope) = scope {
        if journal.thread_exists(thread_id).map_err(journal_error)? {
            journal
                .append_thread_event_with_id(ThreadEventEnvelope::new(
                    &uuid::Uuid::now_v7().to_string(),
                    thread_id,
                    "thread.deleted",
                    &inner.now_text(),
                    json!({}),
                    imported_provenance(&scope, owner),
                ))
                .map_err(journal_error)?;
        }
    }
    Ok(())
}

/// Deletes the record of an ordinary thread this runtime owns once its
/// journal deleted the thread, so every member forgets it.
fn retire_plain(inner: &Inner, thread_id: &str) -> Result<(), String> {
    inner
        .store
        .delete_record("threads", "thread_id", thread_id)?;
    let origin = inner.origin()?;
    inner
        .records
        .delete(&origin, inner.now(), CLASS, thread_id, 0, None)?;
    inner.changed("threads");
    Ok(())
}

/// Rewrites the summary of an ordinary thread this runtime owns when its
/// title or newest post changed, or its newest event moved on by an hour.
fn refresh_summary(inner: &Inner, self_id: &str, thread_id: &str) -> Result<(), String> {
    let held = storage_lock(&inner.storage)?
        .journal
        .summaries_of_threads(&[thread_id.to_owned()])
        .map_err(journal_error)?
        .into_iter()
        .next();
    let Some(held) = held else {
        return Ok(());
    };
    let next = ThreadSummaryRecord {
        title: held.title,
        owner_runtime: self_id.to_owned(),
        last_at: held.updated_at,
        last_post: last_post(inner, thread_id)?,
    };
    if let Some(current) = summary(inner, thread_id) {
        let moved = match (
            super::parse_time(&current.last_at),
            super::parse_time(&next.last_at),
        ) {
            (Some(was), Some(now)) => (now - was).num_seconds() >= SUMMARY_REFRESH_SECONDS,
            _ => current.last_at != next.last_at,
        };
        if current.title == next.title && current.last_post == next.last_post && !moved {
            return Ok(());
        }
    }
    let value = serde_json::to_value(&next).map_err(|error| error.to_string())?;
    inner
        .records
        .put(self_id, inner.now(), CLASS, thread_id, SUMMARY, value, 0)?;
    Ok(())
}

fn clip_text(text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= LAST_POST_CHARS {
        return text;
    }
    text.chars()
        .take(LAST_POST_CHARS - 1)
        .chain(std::iter::once('…'))
        .collect()
}

/// The newer of a thread's newest posted message and its newest run's
/// prompt.
fn last_post(inner: &Inner, thread_id: &str) -> Result<Option<LastPost>, String> {
    let posted: Option<String> = inner
        .store
        .connect()?
        .query_row(
            "SELECT event_id FROM log WHERE thread_id=?1 AND event_type=?2 ORDER BY rowid DESC LIMIT 1",
            params![thread_id, THREAD_MESSAGE_POSTED],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql)?;
    let mut storage = storage_lock(&inner.storage)?;
    let journal = &mut storage.journal;
    let post = match posted {
        Some(event_id) => journal
            .thread_event_by_id(&event_id)
            .map_err(journal_error)?
            .map(|event| LastPost {
                text: clip_text(event.payload()["text"].as_str().unwrap_or_default()),
                author: event.payload()["author"]["kind"]
                    .as_str()
                    .unwrap_or("user")
                    .to_owned(),
                at: event.recorded_at().to_owned(),
            }),
        None => None,
    };
    let last_run = journal
        .thread_run_marks(thread_id)
        .map_err(journal_error)?
        .pop()
        .map(|(run_id, _)| run_id);
    let prompt = match last_run {
        Some(run_id) => journal
            .run_events_after(&run_id, 0, 4)
            .map_err(journal_error)?
            .into_iter()
            .find(|event| event.event_type == "user.prompt.submitted")
            .and_then(|event| match &event.payload {
                EventPayload::Inline { payload_json } => payload_json["prompt"]
                    .as_str()
                    .filter(|prompt| !prompt.trim().is_empty())
                    .map(|prompt| LastPost {
                        text: clip_text(prompt),
                        author: "user".into(),
                        at: event.recorded_at.clone(),
                    }),
                _ => None,
            }),
        None => None,
    };
    Ok(match (post, prompt) {
        (Some(post), Some(prompt)) => Some(if prompt.at > post.at { prompt } else { post }),
        (post, prompt) => post.or(prompt),
    })
}

/// The owner of a registered thread, or this runtime for a thread the swarm
/// does not share.
pub(super) fn thread_owner(inner: &Inner, thread_id: &str) -> Option<String> {
    match registration(inner, thread_id) {
        Some(registration) => thread_primary(inner, &registration),
        None => inner.runtime_id(),
    }
}

pub(super) fn clocks(inner: &Inner) -> Result<Clocks, String> {
    let connection = inner.store.connect()?;
    let mut statement = connection
        .prepare("SELECT thread_id, origin, MAX(seq) FROM log GROUP BY thread_id, origin")
        .map_err(sql)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u64>(2)?,
            ))
        })
        .map_err(sql)?;
    let mut clocks = Clocks::new();
    for row in rows {
        let (thread, origin, seq) = row.map_err(sql)?;
        clocks.entry(thread).or_default().insert(origin, seq);
    }
    Ok(clocks)
}

pub(super) fn thread_clock(
    inner: &Inner,
    thread_id: &str,
) -> Result<BTreeMap<String, u64>, String> {
    clock_of(&*inner.store.connect()?, thread_id)
}

/// The highest sequence held from each origin in one thread.
fn clock_of(
    connection: &rusqlite::Connection,
    thread_id: &str,
) -> Result<BTreeMap<String, u64>, String> {
    let mut statement = connection
        .prepare_cached("SELECT origin, MAX(seq) FROM log WHERE thread_id=?1 GROUP BY origin")
        .map_err(sql)?;
    let rows = statement
        .query_map([thread_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(sql)?;
    rows.collect::<Result<_, _>>().map_err(sql)
}

/// The clocks of the threads whose version is past `base`, in version
/// order, within `budget` bytes: the clocks, the version they reach and
/// whether no later version is left.
pub(super) fn clock_delta(
    inner: &Inner,
    base: u64,
    budget: &mut usize,
) -> Result<(Clocks, u64, bool), String> {
    let connection = inner.store.connect()?;
    let mut statement = connection
        .prepare("SELECT thread_id, version FROM thread_versions WHERE version>?1 ORDER BY version")
        .map_err(sql)?;
    let mut rows = statement.query([base as i64]).map_err(sql)?;
    let mut origins = connection
        .prepare("SELECT origin, MAX(seq) FROM log WHERE thread_id=?1 GROUP BY origin")
        .map_err(sql)?;
    let mut clocks = Clocks::new();
    let mut mark = base;
    while let Some(row) = rows.next().map_err(sql)? {
        let thread: String = row.get(0).map_err(sql)?;
        let version = row.get::<_, i64>(1).map_err(sql)? as u64;
        let clock: BTreeMap<String, u64> = origins
            .query_map([&thread], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(sql)?
            .collect::<Result<_, _>>()
            .map_err(sql)?;
        let size = super::item_cost(
            thread.len() + 8 + clock.keys().map(|origin| origin.len() + 16).sum::<usize>(),
            1 + clock.len(),
        );
        if size > *budget {
            return Ok((clocks, mark, false));
        }
        *budget -= size;
        clocks.insert(thread, clock);
        mark = version;
    }
    Ok((clocks, mark, true))
}

/// The highest thread version this runtime numbered.
pub(super) fn clock_version(inner: &Inner) -> Result<u64, String> {
    inner
        .store
        .connect()?
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM thread_versions",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|version| version as u64)
        .map_err(sql)
}

/// A hash of every thread clock, which a peer compares with its copy.
pub(super) fn clock_digest(clocks: &Clocks) -> String {
    use sha2::Digest;
    let bytes = serde_json::to_vec(clocks).unwrap_or_default();
    hex::encode(sha2::Sha256::digest(bytes))
}

fn next_seq(
    connection: &rusqlite::Connection,
    thread_id: &str,
    origin: &str,
) -> Result<u64, String> {
    connection
        .query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM log WHERE thread_id=?1 AND origin=?2",
            params![thread_id, origin],
            |row| row.get(0),
        )
        .map_err(sql)
}

/// What the scan of the journal last read: its row marks and when it last
/// read every thread.
#[derive(Default)]
pub(super) struct ScanMarks {
    run: i64,
    thread: i64,
    full_at: Option<chrono::DateTime<chrono::Utc>>,
    /// The log row up to which every posted message was consumed or is
    /// another member's to consume.
    posts: i64,
}

/// Indexes the events this runtime wrote into each registered thread the
/// journal changed since the last pass, registers the ordinary threads it
/// created, and keeps the summaries of those it owns. Every 10 minutes it
/// reads every thread instead.
pub(super) fn scan_local(inner: &Inner) -> Result<(), String> {
    let Some(self_id) = inner.runtime_id() else {
        return Ok(());
    };
    let now = inner.now();
    let (run_mark, thread_mark, full) = {
        let marks = super::lock(&inner.scan);
        let full = marks
            .full_at
            .is_none_or(|at| (now - at).num_seconds() >= FULL_SCAN_SECONDS);
        if full {
            (0, 0, true)
        } else {
            (marks.run, marks.thread, false)
        }
    };
    let (changed, run_max, thread_max) = storage_lock(&inner.storage)?
        .journal
        .threads_changed_after(run_mark, thread_mark)
        .map_err(journal_error)?;
    let mut added = false;
    for thread_id in &changed {
        let registration = match registration(inner, thread_id) {
            Some(registration) => registration,
            None if register_plain(inner, &self_id, thread_id)? => {
                added = true;
                match registration(inner, thread_id) {
                    Some(registration) => registration,
                    None => continue,
                }
            }
            None => continue,
        };
        let exists = scan_thread(inner, &self_id, thread_id)?;
        if registration.kind == PLAIN
            && registration.owner_runtime.as_deref() == Some(self_id.as_str())
        {
            if exists {
                refresh_summary(inner, &self_id, thread_id)?;
            } else {
                retire_plain(inner, thread_id)?;
            }
        }
    }
    let mut marks = super::lock(&inner.scan);
    marks.run = run_max;
    marks.thread = thread_max;
    if full {
        marks.full_at = Some(now);
        marks.posts = 0;
    }
    drop(marks);
    if added {
        inner.changed("threads");
    }
    Ok(())
}

/// Makes the next scan read every thread, as after a fresh snapshot.
pub(super) fn rescan(inner: &Inner) {
    *super::lock(&inner.scan) = ScanMarks::default();
}

/// Keeps the time of a thread's newest event, which decides whether an
/// ordinary thread is within a member's offline window.
fn note_activity(
    connection: &rusqlite::Connection,
    thread_id: &str,
    at: &str,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO thread_activity(thread_id, at) VALUES(?1, ?2) \
             ON CONFLICT(thread_id) DO UPDATE SET at=MAX(at, excluded.at)",
            params![thread_id, at],
        )
        .map(drop)
        .map_err(sql)
}

fn origin_of(event: &EventEnvelope) -> Option<String> {
    event
        .provenance
        .extra
        .get(SWARM_ORIGIN)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Indexes the events this runtime wrote into one thread. Answers whether
/// the journal holds the thread.
pub(super) fn scan_thread(inner: &Inner, self_id: &str, thread_id: &str) -> Result<bool, String> {
    let mut connection = inner.store.connect()?;
    let mark: u64 = connection
        .query_row(
            "SELECT scanned_seq FROM thread_marks WHERE thread_id=?1",
            [thread_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql)?
        .unwrap_or(1);
    let known_runs: BTreeMap<String, (String, u64)> = {
        let mut statement = connection
            .prepare("SELECT run_id, origin, indexed_seq FROM runs WHERE thread_id=?1")
            .map_err(sql)?;
        let rows = statement
            .query_map([thread_id], |row| {
                Ok((row.get::<_, String>(0)?, (row.get(1)?, row.get(2)?)))
            })
            .map_err(sql)?;
        rows.collect::<Result<_, _>>().map_err(sql)?
    };
    let mut new_runs = Vec::new();
    let mut run_events: Vec<(String, Vec<EventEnvelope>)> = Vec::new();
    let thread_events;
    {
        let mut storage = storage_lock(&inner.storage)?;
        let journal = &mut storage.journal;
        if !journal.thread_exists(thread_id).map_err(journal_error)? {
            return Ok(false);
        }
        for (run_id, last) in journal.thread_run_marks(thread_id).map_err(journal_error)? {
            let (origin, indexed) = match known_runs.get(&run_id) {
                Some(known) => known.clone(),
                None => {
                    let origin = journal
                        .run_event(&run_id, 1)
                        .map_err(journal_error)?
                        .as_ref()
                        .and_then(origin_of)
                        .unwrap_or_else(|| self_id.to_owned());
                    let indexed = if origin == self_id { 0 } else { last };
                    new_runs.push((run_id.clone(), origin.clone(), indexed));
                    (origin, indexed)
                }
            };
            if origin == self_id && last > indexed {
                run_events.push((
                    run_id.clone(),
                    journal
                        .run_events_after(&run_id, indexed, 1000)
                        .map_err(journal_error)?,
                ));
            }
        }
        thread_events = journal
            .thread_events_after(thread_id, mark)
            .map_err(journal_error)?;
    }
    let transaction = connection.transaction().map_err(sql)?;
    for (run_id, origin, indexed) in new_runs {
        transaction
            .execute(
                "INSERT OR IGNORE INTO runs(run_id, thread_id, origin, indexed_seq) VALUES(?1, ?2, ?3, ?4)",
                params![run_id, thread_id, origin, indexed],
            )
            .map_err(sql)?;
    }
    let mut newest: Option<String> = None;
    for (run_id, events) in run_events {
        let mut last = None;
        for event in events {
            if newest
                .as_deref()
                .is_none_or(|at| event.recorded_at.as_str() > at)
            {
                newest = Some(event.recorded_at.clone());
            }
            let seq = next_seq(&transaction, thread_id, self_id)?;
            transaction
                .execute(
                    "INSERT OR IGNORE INTO log(thread_id, origin, seq, kind, event_id, run_id, run_seq, event_type) \
                     VALUES(?1, ?2, ?3, 'run', ?4, ?5, ?6, ?7)",
                    params![thread_id, self_id, seq, event.event_id, run_id, event.run_seq, event.event_type],
                )
                .map_err(sql)?;
            last = Some(event.run_seq);
        }
        if let Some(last) = last {
            transaction
                .execute(
                    "UPDATE runs SET indexed_seq=?2 WHERE run_id=?1",
                    params![run_id, last],
                )
                .map_err(sql)?;
        }
    }
    let mut scanned = mark;
    for event in thread_events {
        scanned = scanned.max(event.thread_seq());
        let imported = event.provenance.extra.contains_key(SWARM_ORIGIN);
        if imported || !REPLICATED_THREAD_EVENTS.contains(&event.event_type.as_str()) {
            continue;
        }
        let seq = next_seq(&transaction, thread_id, self_id)?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO log(thread_id, origin, seq, kind, event_id, run_id, run_seq, event_type) \
                 VALUES(?1, ?2, ?3, 'thread', ?4, NULL, NULL, ?5)",
                params![thread_id, self_id, seq, event.event_id(), event.event_type],
            )
            .map_err(sql)?;
        if newest.as_deref().is_none_or(|at| event.recorded_at() > at) {
            newest = Some(event.recorded_at().to_owned());
        }
    }
    transaction
        .execute(
            "INSERT INTO thread_marks(thread_id, scanned_seq) VALUES(?1, ?2) \
             ON CONFLICT(thread_id) DO UPDATE SET scanned_seq=excluded.scanned_seq",
            params![thread_id, scanned],
        )
        .map_err(sql)?;
    if let Some(newest) = newest {
        note_activity(&transaction, thread_id, &newest)?;
    }
    transaction.commit().map_err(sql)?;
    Ok(true)
}

/// Where an export to one peer stands: the thread version up to which it
/// looked, and the threads the peer still lacked events of.
#[derive(Clone, Debug, Default)]
pub(super) struct ExportState {
    mark: u64,
    lagging: BTreeSet<String>,
}

/// Which ordinary threads a peer takes the history of.
struct Window {
    peer: String,
    /// The oldest newest-event time within the peer's window, `None` for
    /// every thread.
    since: Option<String>,
    pins: BTreeSet<String>,
}

impl Window {
    fn for_peer(inner: &Inner, peer: &str) -> Result<Self, String> {
        let members = inner
            .store
            .records::<super::MemberRecord>("members", "record")?;
        let days = members
            .iter()
            .find(|member| member.runtime_id == peer)
            .and_then(|member| member.offline_days)
            .unwrap_or(super::OFFLINE_DAYS);
        let mut pins: BTreeSet<String> =
            members.into_iter().flat_map(|member| member.pins).collect();
        pins.extend(own_pins(inner)?);
        Ok(Self {
            peer: peer.to_owned(),
            since: (days > 0)
                .then(|| super::timestamp(inner.now() - chrono::Duration::days(i64::from(days)))),
            pins,
        })
    }

    /// Whether the peer takes the history of `thread_id`: every agent and
    /// group thread, and an ordinary thread the peer owns, someone pinned,
    /// or that was active within the peer's window. A thread with no
    /// registration, as one its owner deleted, goes to no peer: a member
    /// takes in the events of registered threads alone.
    fn wants(
        &self,
        inner: &Inner,
        connection: &rusqlite::Connection,
        thread_id: &str,
    ) -> Result<bool, String> {
        let Some(registration) = registration(inner, thread_id) else {
            return Ok(false);
        };
        let Some(since) = &self.since else {
            return Ok(true);
        };
        if registration.kind != PLAIN
            || registration.owner_runtime.as_deref() == Some(self.peer.as_str())
            || self.pins.contains(thread_id)
        {
            return Ok(true);
        }
        let at: Option<String> = connection
            .query_row(
                "SELECT at FROM thread_activity WHERE thread_id=?1",
                [thread_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql)?;
        Ok(at.is_none_or(|at| at.as_str() >= since.as_str()))
    }
}

/// The ordinary threads this runtime asked to hold in full, newest first.
pub(super) fn own_pins(inner: &Inner) -> Result<Vec<String>, String> {
    let connection = inner.store.connect()?;
    let mut statement = connection
        .prepare("SELECT thread_id FROM pins ORDER BY at DESC LIMIT ?1")
        .map_err(sql)?;
    let rows = statement
        .query_map([MAX_PINS as i64], |row| row.get(0))
        .map_err(sql)?;
    rows.collect::<Result<_, _>>().map_err(sql)
}

/// The most ordinary threads one member pins.
pub(super) const MAX_PINS: usize = 64;

/// Pins a thread so its history arrives whatever its age. Answers whether
/// it was new.
pub(super) fn pin(inner: &Inner, thread_id: &str) -> Result<bool, String> {
    let connection = inner.store.connect()?;
    let added = connection
        .execute(
            "INSERT INTO pins(thread_id, at) VALUES(?1, ?2) \
             ON CONFLICT(thread_id) DO UPDATE SET at=excluded.at",
            params![thread_id, inner.now_text()],
        )
        .map_err(sql)?;
    connection
        .execute(
            "DELETE FROM pins WHERE thread_id NOT IN \
             (SELECT thread_id FROM pins ORDER BY at DESC LIMIT ?1)",
            [MAX_PINS as i64],
        )
        .map_err(sql)?;
    Ok(added > 0)
}

/// The events this runtime holds that the peer's clocks lack, of the
/// threads and origins `allow` passes, oldest first per origin, within
/// `budget` bytes. `peer_clock` answers the peer's clock
/// of one thread. It looks at the threads whose clock changed since `state`
/// last looked and at those the peer still lacked, so an idle exchange
/// reads no clocks. `state` is `None` for an older member, whose full
/// clocks arrive each time.
pub(super) fn export(
    inner: &Inner,
    peer: &str,
    peer_clock: &dyn Fn(&str) -> Option<BTreeMap<String, u64>>,
    state: Option<&mut ExportState>,
    budget: &mut usize,
    allow: &dyn Fn(&str, &str) -> bool,
) -> Result<(Vec<ReplicatedEvent>, bool), String> {
    let mut fresh = ExportState::default();
    let state = state.unwrap_or(&mut fresh);
    let connection = inner.store.connect()?;
    let mut candidates: Vec<String> = state.lagging.iter().cloned().collect();
    {
        let mut statement = connection
            .prepare_cached(
                "SELECT thread_id, version FROM thread_versions WHERE version>?1 ORDER BY version",
            )
            .map_err(sql)?;
        let mut rows = statement.query([state.mark as i64]).map_err(sql)?;
        while let Some(row) = rows.next().map_err(sql)? {
            let thread: String = row.get(0).map_err(sql)?;
            state.mark = state.mark.max(row.get::<_, i64>(1).map_err(sql)? as u64);
            if !state.lagging.contains(&thread) {
                candidates.push(thread);
            }
        }
    }
    let window = Window::for_peer(inner, peer)?;
    let subject = inner.host.replica_scope().and_then(|scope| scope.subject);
    let mut events = Vec::new();
    let mut lagging = BTreeSet::new();
    let mut more = false;
    for thread in candidates {
        if more {
            lagging.insert(thread);
            continue;
        }
        let mine: BTreeMap<String, u64> = clock_of(&connection, &thread)?
            .into_iter()
            .filter(|(origin, _)| allow(&thread, origin))
            .collect();
        let theirs = peer_clock(&thread).unwrap_or_default();
        let known = |origin: &str| theirs.get(origin).copied().unwrap_or(0);
        if mine.iter().all(|(origin, held)| *held <= known(origin))
            || !window.wants(inner, &connection, &thread)?
        {
            continue;
        }
        lagging.insert(thread.clone());
        for (origin, held) in &mine {
            let known = known(origin);
            if *held <= known {
                continue;
            }
            if !export_rows(
                inner,
                &connection,
                &thread,
                origin,
                known,
                subject.as_deref(),
                &mut events,
                budget,
            )? {
                more = true;
                break;
            }
        }
    }
    state.lagging = lagging;
    Ok((events, more))
}

/// Adds the events of one thread and origin after `known` to `events`.
/// Answers false when the budget ran out.
#[allow(clippy::too_many_arguments)]
fn export_rows(
    inner: &Inner,
    connection: &rusqlite::Connection,
    thread: &str,
    origin: &str,
    known: u64,
    subject: Option<&str>,
    events: &mut Vec<ReplicatedEvent>,
    budget: &mut usize,
) -> Result<bool, String> {
    let rows: Vec<LogRow> = {
        let mut statement = connection
            .prepare_cached(
                "SELECT seq, kind, event_id, run_id, run_seq FROM log \
                 WHERE thread_id=?1 AND origin=?2 AND seq>?3 ORDER BY seq LIMIT ?4",
            )
            .map_err(sql)?;
        let rows = statement
            .query_map(params![thread, origin, known, MAX_ROWS_PER_ORIGIN], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .map_err(sql)?;
        rows.collect::<Result<_, _>>().map_err(sql)?
    };
    for (seq, kind, event_id, run_id, run_seq) in rows {
        let (envelope, prompt) = {
            let mut storage = storage_lock(&inner.storage)?;
            let journal = &mut storage.journal;
            match (kind.as_str(), run_id, run_seq) {
                ("run", Some(run_id), Some(run_seq)) => {
                    let Some(event) = journal.run_event(&run_id, run_seq).map_err(journal_error)?
                    else {
                        break;
                    };
                    let prompt = (run_seq == 1)
                        .then(|| {
                            crate::chat_prompt::load_prompt(&run_id, subject)
                                .ok()
                                .flatten()
                        })
                        .flatten();
                    (
                        serde_json::to_value(event).map_err(|e| e.to_string())?,
                        prompt,
                    )
                }
                _ => {
                    let Some(event) = journal
                        .thread_event_by_id(&event_id)
                        .map_err(journal_error)?
                    else {
                        break;
                    };
                    (
                        serde_json::to_value(event).map_err(|e| e.to_string())?,
                        None,
                    )
                }
            }
        };
        let event = ReplicatedEvent {
            thread_id: thread.to_owned(),
            origin: origin.to_owned(),
            seq,
            kind,
            envelope,
            prompt,
        };
        let size = super::budget_cost(&event);
        if size > *budget {
            return Ok(false);
        }
        *budget -= size;
        events.push(event);
    }
    Ok(true)
}

fn local_provenance(scope: Option<&ReplicaScope>, attach_profile: &str) -> Provenance {
    let mut extra = BTreeMap::new();
    extra.insert("attach_profile".into(), Value::from(attach_profile));
    Provenance {
        source: "muniment-swarm".into(),
        source_version: env!("CARGO_PKG_VERSION").into(),
        actor_id: scope.and_then(|scope| scope.subject.clone()),
        device_id: None,
        rpc_request_id: None,
        capability_versions: None,
        extra,
    }
}

/// Creates the replica of a registered thread under its own id.
fn ensure_thread(
    inner: &Inner,
    journal: &mut crate::journal::RunJournal,
    thread_id: &str,
    scope: Option<&ReplicaScope>,
) -> Result<bool, String> {
    if journal.thread_exists(thread_id).map_err(journal_error)? {
        return Ok(true);
    }
    let Some(scope) = scope else {
        return Ok(false);
    };
    journal
        .create_thread_with_id(
            thread_id,
            &scope.workspace,
            &inner.now_text(),
            local_provenance(Some(scope), &scope.attach_profile),
        )
        .map_err(journal_error)?;
    Ok(true)
}

fn note_blob(inner: &Inner, event: &EventEnvelope, thread_id: &str) -> Result<(), String> {
    let (hash, total) = match &event.payload {
        EventPayload::Cas { payload_cas } => (payload_cas.sha256.clone(), payload_cas.byte_length),
        EventPayload::Attachment { attachment } => (
            attachment.sha256().as_str().to_owned(),
            attachment.byte_length(),
        ),
        EventPayload::Inline { .. } => return Ok(()),
    };
    let present = hash
        .parse::<crate::cas::ContentHash>()
        .ok()
        .and_then(|parsed| {
            storage_lock(&inner.storage)
                .ok()
                .and_then(|storage| storage.cas.has(&parsed).ok())
        })
        .unwrap_or(false);
    if !present {
        // A member that keeps every attachment offline asks at once; others
        // ask as a client opens the thread.
        let requested =
            super::bootstrap::offline(inner).attachments == super::bootstrap::ATTACHMENTS_ALL;
        inner
            .store
            .connect()?
            .execute(
                "INSERT OR IGNORE INTO blobs(hash, thread_id, total, requested) VALUES(?1, ?2, ?3, ?4)",
                params![hash, thread_id, total, requested],
            )
            .map_err(sql)?;
    }
    Ok(())
}

/// Appends the events a peer sent, in `(origin, seq)` order. An event whose
/// predecessor is missing waits for a later exchange.
pub(super) fn import(inner: &Inner, mut events: Vec<ReplicatedEvent>) -> Result<(), String> {
    let Some(self_id) = inner.runtime_id() else {
        return Ok(());
    };
    if events.is_empty() {
        return Ok(());
    }
    events.sort_by(|a, b| (&a.thread_id, &a.origin, a.seq).cmp(&(&b.thread_id, &b.origin, b.seq)));
    let scope = inner.host.replica_scope();
    let connection = inner.store.connect()?;
    let mut blocked: Vec<(String, String)> = Vec::new();
    let mut imported = 0usize;
    let mut posts: Vec<(String, Value, String)> = Vec::new();
    for event in events {
        let key = (event.thread_id.clone(), event.origin.clone());
        if event.origin == self_id
            || blocked.contains(&key)
            || registration(inner, &event.thread_id).is_none()
        {
            continue;
        }
        let expected = next_seq(&connection, &event.thread_id, &event.origin)?;
        if event.seq < expected {
            continue;
        }
        if event.seq > expected {
            blocked.push(key);
            continue;
        }
        let logged = match import_one(inner, &event, scope.as_ref()) {
            Ok(Some(logged)) => logged,
            Ok(None) => {
                blocked.push(key);
                continue;
            }
            Err(error) => {
                crate::runtime_eprintln!(
                    "muniment-runtime: swarm import failed thread_id={} origin={} seq={}: {error}",
                    event.thread_id,
                    event.origin,
                    event.seq
                );
                blocked.push(key);
                continue;
            }
        };
        connection
            .execute(
                "INSERT OR IGNORE INTO log(thread_id, origin, seq, kind, event_id, run_id, run_seq, event_type) \
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    event.thread_id,
                    event.origin,
                    event.seq,
                    event.kind,
                    logged.event_id,
                    logged.run_id,
                    logged.run_seq,
                    logged.event_type
                ],
            )
            .map_err(sql)?;
        if let Some(at) = event.envelope["recorded_at"].as_str() {
            note_activity(&connection, &event.thread_id, at)?;
        }
        imported += 1;
        if logged.event_type == THREAD_MESSAGE_POSTED {
            if let Ok(envelope) = serde_json::from_value::<ThreadEventEnvelope>(event.envelope) {
                posts.push((
                    event.thread_id,
                    envelope.payload().clone(),
                    envelope.recorded_at().to_owned(),
                ));
            }
        }
    }
    if imported > 0 {
        inner.changed("threads");
    }
    for (thread_id, payload, posted_at) in posts {
        announce_post(inner, &thread_id, &payload, &posted_at);
    }
    Ok(())
}

struct Logged {
    event_id: String,
    run_id: Option<String>,
    run_seq: Option<u64>,
    event_type: String,
}

/// Appends one event and answers what the index records, or `None` when it
/// must wait.
fn import_one(
    inner: &Inner,
    event: &ReplicatedEvent,
    scope: Option<&ReplicaScope>,
) -> Result<Option<Logged>, String> {
    let mut storage = storage_lock(&inner.storage)?;
    let journal = &mut storage.journal;
    if !ensure_thread(inner, journal, &event.thread_id, scope)? {
        return Ok(None);
    }
    let (_, thread_profile) = journal
        .thread_scope(&event.thread_id)
        .map_err(journal_error)?
        .unwrap_or((None, None));
    let thread_profile = thread_profile.unwrap_or_default();
    match event.kind.as_str() {
        "run" => {
            let mut envelope: EventEnvelope = serde_json::from_value(event.envelope.clone())
                .map_err(|error| format!("The run event is invalid: {error}"))?;
            let logged = Logged {
                event_id: envelope.event_id.clone(),
                run_id: Some(envelope.run_id.clone()),
                run_seq: Some(envelope.run_seq),
                event_type: envelope.event_type.clone(),
            };
            if journal
                .run_event_exists(&envelope.event_id)
                .map_err(journal_error)?
            {
                return Ok(Some(logged));
            }
            let run_id = envelope.run_id.clone();
            // The copy belongs to this runtime's owner and names its origin.
            envelope.provenance.actor_id = scope.and_then(|scope| scope.subject.clone());
            envelope
                .provenance
                .extra
                .insert(SWARM_ORIGIN.into(), Value::from(event.origin.as_str()));
            if envelope.run_seq == 1 {
                let (workspace, _) = journal
                    .thread_scope(&event.thread_id)
                    .map_err(journal_error)?
                    .unwrap_or((None, None));
                let workspace = workspace.ok_or("The replica thread has no workspace.")?;
                envelope.provenance.extra.insert(
                    "attach_profile".into(),
                    Value::from(thread_profile.as_str()),
                );
                journal
                    .append_new_run_in_thread(&workspace, &event.thread_id, &envelope)
                    .map_err(journal_error)?;
                drop(storage);
                if let Some(prompt) = &event.prompt {
                    let subject = scope.and_then(|scope| scope.subject.clone());
                    if let Err(error) =
                        crate::chat_prompt::store_prompt(&run_id, prompt, subject.as_deref())
                    {
                        crate::runtime_eprintln!(
                            "muniment-runtime: swarm prompt not stored run_id={run_id}: {error:?}"
                        );
                    }
                }
                inner
                    .store
                    .connect()?
                    .execute(
                        "INSERT INTO runs(run_id, thread_id, origin, indexed_seq) VALUES(?1, ?2, ?3, 1) \
                         ON CONFLICT(run_id) DO UPDATE SET indexed_seq=1",
                        params![run_id, event.thread_id, event.origin],
                    )
                    .map_err(sql)?;
            } else {
                let last = journal.last_run_seq(&run_id).map_err(journal_error)?;
                if last + 1 != envelope.run_seq {
                    return Ok(None);
                }
                journal
                    .append_batch(last, std::slice::from_ref(&envelope))
                    .map_err(journal_error)?;
                drop(storage);
                inner
                    .store
                    .connect()?
                    .execute(
                        "UPDATE runs SET indexed_seq=?2 WHERE run_id=?1",
                        params![run_id, envelope.run_seq],
                    )
                    .map_err(sql)?;
            }
            note_blob(inner, &envelope, &event.thread_id)?;
            Ok(Some(logged))
        }
        "thread" => {
            let mut envelope: ThreadEventEnvelope = serde_json::from_value(event.envelope.clone())
                .map_err(|error| format!("The thread event is invalid: {error}"))?;
            if envelope.thread_id() != event.thread_id {
                return Err("The thread event names another thread.".into());
            }
            envelope.provenance.extra.insert(
                "attach_profile".into(),
                Value::from(thread_profile.as_str()),
            );
            envelope
                .provenance
                .extra
                .insert(SWARM_ORIGIN.into(), Value::from(event.origin.as_str()));
            let stored = journal
                .append_thread_event_with_id(envelope)
                .map_err(journal_error)?;
            Ok(Some(Logged {
                event_id: stored.event_id().to_owned(),
                run_id: None,
                run_seq: None,
                event_type: stored.event_type.clone(),
            }))
        }
        _ => Err("The replicated event kind is unknown.".into()),
    }
}

/// Appends a message to a registered thread with this runtime as its origin
/// and answers its message id and sequence.
pub(super) fn post(
    inner: &Inner,
    thread_id: &str,
    author: Value,
    text: &str,
) -> Result<(String, u64), String> {
    let self_id = inner
        .runtime_id()
        .ok_or("Turn on remote access to post to shared threads.")?;
    if registration(inner, thread_id).is_none() {
        return Err("This thread is not shared with the swarm.".into());
    }
    let text = text.trim();
    if text.is_empty() || text.len() > MAX_POSTED_TEXT_BYTES {
        return Err("Write a message under 64 KB.".into());
    }
    let deps = thread_clock(inner, thread_id)?;
    let message_id = uuid::Uuid::now_v7().to_string();
    let scope = inner.host.replica_scope();
    {
        let mut storage = storage_lock(&inner.storage)?;
        let journal = &mut storage.journal;
        if !ensure_thread(inner, journal, thread_id, scope.as_ref())? {
            return Err("Open Muniment and connect a model before posting.".into());
        }
        let profile = journal
            .thread_scope(thread_id)
            .map_err(journal_error)?
            .and_then(|(_, profile)| profile)
            .unwrap_or_default();
        let envelope = ThreadEventEnvelope::new(
            &message_id,
            thread_id,
            THREAD_MESSAGE_POSTED,
            &inner.now_text(),
            json!({
                "message_id": message_id,
                "author": author,
                "text": text,
                "deps": deps,
                "origin_runtime": self_id,
            }),
            local_provenance(scope.as_ref(), &profile),
        );
        let stored = journal
            .append_thread_event_with_id(envelope)
            .map_err(journal_error)?;
        announce_post(inner, thread_id, stored.payload(), stored.recorded_at());
    }
    scan_thread(inner, &self_id, thread_id)?;
    let seq: u64 = inner
        .store
        .connect()?
        .query_row(
            "SELECT seq FROM log WHERE event_id=?1",
            [&message_id],
            |row| row.get(0),
        )
        .map_err(sql)?;
    inner.nudge();
    Ok((message_id, seq))
}

/// Appends a thread event this runtime writes as the thread's primary and
/// answers its recorded time.
fn append_local(
    inner: &Inner,
    thread_id: &str,
    event_type: &str,
    payload: Value,
) -> Result<String, String> {
    let scope = inner.host.replica_scope();
    let mut storage = storage_lock(&inner.storage)?;
    let journal = &mut storage.journal;
    if !ensure_thread(inner, journal, thread_id, scope.as_ref())? {
        return Err("The thread is not on this runtime.".into());
    }
    let profile = journal
        .thread_scope(thread_id)
        .map_err(journal_error)?
        .and_then(|(_, profile)| profile)
        .unwrap_or_default();
    let event_id = uuid::Uuid::now_v7().to_string();
    let stored = journal
        .append_thread_event_with_id(ThreadEventEnvelope::new(
            &event_id,
            thread_id,
            event_type,
            &inner.now_text(),
            payload,
            local_provenance(scope.as_ref(), &profile),
        ))
        .map_err(journal_error)?;
    Ok(stored.recorded_at().to_owned())
}

/// Posts a message an agent or this runtime writes, such as an agent's
/// reply in a group or a delivery note.
pub(super) fn post_as(
    inner: &Inner,
    thread_id: &str,
    author: Value,
    text: &str,
) -> Result<(), String> {
    let message_id = uuid::Uuid::now_v7().to_string();
    let deps = thread_clock(inner, thread_id)?;
    let mut text = text.trim().to_owned();
    if text.is_empty() {
        text = "(no reply)".into();
    }
    if text.len() > MAX_POSTED_TEXT_BYTES {
        let mut end = MAX_POSTED_TEXT_BYTES - 3;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push('…');
    }
    let payload = json!({
        "message_id": message_id,
        "author": author,
        "text": text,
        "deps": deps,
        "origin_runtime": inner.runtime_id(),
    });
    let posted_at = append_local(inner, thread_id, THREAD_MESSAGE_POSTED, payload.clone())?;
    announce_post(inner, thread_id, &payload, &posted_at);
    if let Some(self_id) = inner.runtime_id() {
        scan_thread(inner, &self_id, thread_id)?;
    }
    inner.nudge();
    Ok(())
}

/// `thread.post`: posts to an agent, group or ordinary thread on any member,
/// or to an agent that has no thread yet through its primary runtime.
pub(super) fn post_operation(inner: &Inner, body: Value) -> Result<Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Body {
        #[serde(default)]
        thread_id: Option<String>,
        #[serde(default)]
        agent_id: Option<String>,
        text: String,
    }
    let body: Body = serde_json::from_value(body).map_err(|_| "The request is invalid.")?;
    let _merge = super::lock(&inner.merge);
    let origin = inner.runtime_id();
    let thread = match (&body.thread_id, &body.agent_id) {
        (Some(thread), _) => Some(thread.clone()),
        (None, Some(agent)) => directory::entry(inner, agent)
            .and_then(|entry| entry.primary_thread)
            .filter(|thread| registration(inner, thread).is_some()),
        (None, None) => return Err("Name the thread or the agent to post to.".into()),
    };
    match thread {
        Some(thread) => {
            let (message_id, seq) = post(inner, &thread, json!({"kind": "user"}), &body.text)?;
            let kick = consume_posts(inner)?;
            drop(_merge);
            if kick {
                inner.host.kick_runs();
            }
            Ok(json!({
                "message_id": message_id,
                "thread_id": thread,
                "origin_runtime": origin,
                "seq": seq,
            }))
        }
        None => {
            let agent = body.agent_id.unwrap_or_default();
            let message_id = super::messaging::send_user_message(inner, &agent, &body.text)?;
            drop(_merge);
            inner.host.kick_runs();
            Ok(json!({
                "message_id": message_id,
                "thread_id": null,
                "origin_runtime": origin,
                "seq": null,
            }))
        }
    }
}

struct Pending {
    origin: String,
    seq: u64,
    event_id: String,
    recorded_at: String,
    payload: Value,
}

/// Consumes the posted user messages of the threads this runtime is primary
/// of, in causal order: a message waits until this runtime holds every event
/// its poster had seen. Answers whether it queued runs.
pub(super) fn consume_posts(inner: &Inner) -> Result<bool, String> {
    let Some(self_id) = inner.runtime_id() else {
        return Ok(false);
    };
    let mut kick = false;
    // Only threads with a posted message not yet consumed have work. The
    // pass reads the posts after its mark, and a thread another member runs
    // never has work here.
    let mark = super::lock(&inner.scan).posts;
    let posted: Vec<(i64, String, bool)> = {
        let connection = inner.store.connect()?;
        let mut statement = connection
            .prepare_cached(
                "SELECT l.rowid, l.thread_id, EXISTS(SELECT 1 FROM consumed c WHERE c.event_id=l.event_id) \
                 FROM log l WHERE l.rowid>?1 AND l.event_type=?2 AND l.kind='thread' ORDER BY l.rowid",
            )
            .map_err(sql)?;
        let rows = statement
            .query_map(params![mark, THREAD_MESSAGE_POSTED], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(sql)?;
        rows.collect::<Result<_, _>>().map_err(sql)?
    };
    let mut ours: BTreeMap<String, bool> = BTreeMap::new();
    let mut waiting: Vec<String> = Vec::new();
    let mut next_mark = posted.last().map_or(mark, |(rowid, _, _)| *rowid);
    for (rowid, thread, consumed) in posted {
        if consumed {
            continue;
        }
        let primary = *ours.entry(thread.clone()).or_insert_with(|| {
            registration(inner, &thread)
                .and_then(|registration| thread_primary(inner, &registration))
                .as_deref()
                == Some(self_id.as_str())
        });
        if primary {
            next_mark = next_mark.min(rowid - 1);
            if !waiting.contains(&thread) {
                waiting.push(thread);
            }
        }
    }
    super::lock(&inner.scan).posts = next_mark;
    for registration in waiting
        .iter()
        .filter_map(|thread| registration(inner, thread))
    {
        if thread_primary(inner, &registration).as_deref() != Some(self_id.as_str()) {
            continue;
        }
        let rows: Vec<(String, u64, String)> = {
            let connection = inner.store.connect()?;
            let mut statement = connection
                .prepare(
                    "SELECT origin, seq, event_id FROM log l WHERE thread_id=?1 AND kind='thread' \
                     AND event_type=?2 AND NOT EXISTS(SELECT 1 FROM consumed c WHERE c.event_id=l.event_id) \
                     ORDER BY seq",
                )
                .map_err(sql)?;
            let rows = statement
                .query_map(
                    params![registration.thread_id, THREAD_MESSAGE_POSTED],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(sql)?;
            rows.collect::<Result<_, _>>().map_err(sql)?
        };
        if rows.is_empty() {
            continue;
        }
        let mut pending = Vec::new();
        {
            let mut storage = storage_lock(&inner.storage)?;
            for (origin, seq, event_id) in rows {
                if let Some(event) = storage
                    .journal
                    .thread_event_by_id(&event_id)
                    .map_err(journal_error)?
                {
                    pending.push(Pending {
                        origin,
                        seq,
                        event_id,
                        recorded_at: event.recorded_at().to_owned(),
                        payload: event.payload().clone(),
                    });
                }
            }
        }
        let clock = thread_clock(inner, &registration.thread_id)?;
        loop {
            let ready = pending
                .iter()
                .enumerate()
                .filter(|(_, message)| {
                    message.payload["deps"].as_object().is_none_or(|deps| {
                        deps.iter().all(|(origin, seq)| {
                            seq.as_u64().unwrap_or(0) <= clock.get(origin).copied().unwrap_or(0)
                        })
                    })
                })
                .min_by(|(_, a), (_, b)| {
                    (&a.recorded_at, &a.origin, a.seq).cmp(&(&b.recorded_at, &b.origin, b.seq))
                })
                .map(|(index, _)| index);
            let Some(index) = ready else {
                break;
            };
            let user = pending[index].payload["author"]["kind"] == "user";
            let mut run_id = None;
            let mut error = None;
            if user && registration.kind == PLAIN {
                // The owner runs a post in an ordinary thread as the next
                // prompt, or steers the run live there; a busy thread keeps
                // the post for the next pass.
                let text = pending[index].payload["text"].as_str().unwrap_or_default();
                match inner.host.steer(&registration.thread_id, text) {
                    Some(steered) => run_id = Some(steered),
                    None => match inner.host.submit(&registration.thread_id, text) {
                        Ok(started) => run_id = Some(started),
                        Err(reason) if reason == crate::active_run::THREAD_BUSY => break,
                        Err(reason) => error = Some(reason),
                    },
                }
            }
            let message = pending.remove(index);
            if user {
                match registration.kind.as_str() {
                    "agent" => {
                        let agent_id = registration.agent_id.clone().unwrap_or_default();
                        let text = message.payload["text"].as_str().unwrap_or_default();
                        match inner.host.steer(&registration.thread_id, text) {
                            Some(steered) => run_id = Some(steered),
                            None => {
                                let run = RunStore::open(&inner.profile)?.enqueue(NewRun {
                                    agent_id,
                                    thread_id: Some(registration.thread_id.clone()),
                                    trigger: RunTrigger {
                                        kind: TriggerKind::Manual,
                                        routine_id: None,
                                        source: Some(format!("swarm:{}", message.origin)),
                                    },
                                    prompt: text.to_owned(),
                                    hop: 0,
                                    origin_run_id: None,
                                })?;
                                crate::agents::mirror_run(&inner.profile, &run)?;
                                run_id = Some(run.id);
                                kick = true;
                            }
                        }
                    }
                    "group" => {
                        kick |= groups::route(
                            inner,
                            registration.group_id.as_deref().unwrap_or_default(),
                            &message.payload,
                        )?;
                    }
                    _ => {}
                }
            }
            inner
                .store
                .connect()?
                .execute(
                    "INSERT OR IGNORE INTO consumed(event_id, thread_id, at) VALUES(?1, ?2, ?3)",
                    params![message.event_id, registration.thread_id, inner.now_text()],
                )
                .map_err(sql)?;
            if user {
                let mut payload = json!({
                    "message_id": message.payload["message_id"],
                    "run_id": run_id,
                });
                if let Some(error) = error {
                    payload["error"] = Value::from(error);
                }
                append_local(
                    inner,
                    &registration.thread_id,
                    THREAD_MESSAGE_CONSUMED,
                    payload,
                )?;
                scan_thread(inner, &self_id, &registration.thread_id)?;
            }
        }
    }
    Ok(kick)
}

/// The messages posted to a thread, oldest first, each with whether its
/// primary consumed it, the run that answered it, and why it started none.
pub fn posted_messages(
    storage: &SharedStorage,
    thread_id: &str,
    limit: usize,
) -> Result<Vec<Value>, String> {
    let events = storage_lock(storage)?
        .journal
        .thread_events_after(thread_id, 0)
        .map_err(journal_error)?;
    let mut posts: Vec<Value> = Vec::new();
    let mut consumed: BTreeMap<String, (Value, Option<Value>)> = BTreeMap::new();
    for event in events {
        let payload = event.payload();
        match event.event_type.as_str() {
            THREAD_MESSAGE_POSTED => posts.push(json!({
                "message_id": payload["message_id"],
                "author": payload["author"],
                "text": payload["text"],
                "origin_runtime": payload["origin_runtime"],
                "posted_at": event.recorded_at(),
                "consumed": false,
                "run_id": null,
            })),
            THREAD_MESSAGE_CONSUMED => {
                if let Some(id) = payload["message_id"].as_str() {
                    consumed.insert(
                        id.to_owned(),
                        (payload["run_id"].clone(), payload.get("error").cloned()),
                    );
                }
            }
            _ => {}
        }
    }
    for post in &mut posts {
        if let Some((run_id, error)) = post["message_id"].as_str().and_then(|id| consumed.get(id)) {
            post["consumed"] = Value::Bool(true);
            post["run_id"] = run_id.clone();
            if let Some(error) = error {
                post["error"] = error.clone();
            }
        }
    }
    let excess = posts.len().saturating_sub(limit);
    posts.drain(..excess);
    Ok(posts)
}

fn blob_part(inner: &Inner, hash: &str) -> PathBuf {
    inner
        .profile
        .join("swarm")
        .join("blobs")
        .join(format!("{hash}.part"))
}

/// Asks for the missing objects of one thread.
pub(super) fn want_thread_blobs(inner: &Inner, thread_id: &str) -> Result<(), String> {
    let changed = inner
        .store
        .connect()?
        .execute(
            "UPDATE blobs SET requested=1 WHERE thread_id=?1 AND done=0 AND requested=0",
            [thread_id],
        )
        .map_err(sql)?;
    if changed > 0 {
        inner.nudge();
    }
    Ok(())
}

/// The object slices a peer asked for that this runtime holds, and this
/// runtime's own requests.
pub(super) fn blob_traffic(
    inner: &Inner,
    serve: &[BlobWant],
    budget: &mut usize,
) -> Result<(Vec<BlobChunk>, Vec<BlobWant>), String> {
    let mut chunks = Vec::new();
    for want in serve {
        if *budget * 3 / 4 == 0 {
            break;
        }
        let Ok(hash) = want.hash.parse::<crate::cas::ContentHash>() else {
            continue;
        };
        let bytes = storage_lock(&inner.storage)?
            .cas
            .get(&hash)
            .map_err(|error| error.to_string())?;
        let Some(bytes) = bytes else {
            continue;
        };
        // Consecutive slices of one blob share an exchange while the
        // budget lasts; the receiver appends them in order.
        let mut start = usize::try_from(want.offset).unwrap_or(usize::MAX);
        while start < bytes.len() {
            let room = (*budget * 3 / 4).min(MAX_BLOB_CHUNK);
            if room == 0 {
                break;
            }
            let end = (start + room).min(bytes.len());
            let data = STANDARD.encode(&bytes[start..end]);
            *budget = budget.saturating_sub(super::item_cost(data.len() + 256, 6));
            chunks.push(BlobChunk {
                hash: want.hash.clone(),
                offset: start as u64,
                total: bytes.len() as u64,
                data,
            });
            start = end;
        }
    }
    let wanted: Vec<String> = {
        let connection = inner.store.connect()?;
        let mut statement = connection
            .prepare("SELECT hash FROM blobs WHERE requested=1 AND done=0 LIMIT 16")
            .map_err(sql)?;
        let rows = statement.query_map([], |row| row.get(0)).map_err(sql)?;
        rows.collect::<Result<_, _>>().map_err(sql)?
    };
    let wants = wanted
        .into_iter()
        .map(|hash| BlobWant {
            offset: std::fs::metadata(blob_part(inner, &hash)).map_or(0, |metadata| metadata.len()),
            hash,
        })
        .collect();
    Ok((chunks, wants))
}

/// Appends received object slices and stores each complete, verified object.
pub(super) fn receive_blobs(inner: &Inner, chunks: Vec<BlobChunk>) -> Result<(), String> {
    for chunk in chunks {
        let wanted: Option<i64> = inner
            .store
            .connect()?
            .query_row(
                "SELECT done FROM blobs WHERE hash=?1",
                [&chunk.hash],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql)?;
        if wanted != Some(0) {
            continue;
        }
        let path = blob_part(inner, &chunk.hash);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let held = std::fs::metadata(&path).map_or(0, |metadata| metadata.len());
        if held != chunk.offset {
            continue;
        }
        let data = STANDARD
            .decode(chunk.data.as_bytes())
            .map_err(|_| "A blob slice is invalid.".to_string())?;
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut file| file.write_all(&data))
            .map_err(|error| error.to_string())?;
        if held + data.len() as u64 >= chunk.total {
            let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
            let stored = storage_lock(&inner.storage)?
                .cas
                .put(&bytes)
                .map_err(|error| error.to_string())?;
            let _ = std::fs::remove_file(&path);
            if stored.as_str() == chunk.hash {
                inner
                    .store
                    .connect()?
                    .execute("UPDATE blobs SET done=1 WHERE hash=?1", [&chunk.hash])
                    .map_err(sql)?;
            }
        }
    }
    Ok(())
}
