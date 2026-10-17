//! How far this runtime's copy of the fleet stands, and the work that keeps
//! the copy small.
//!
//! - **Offline window.** `<profile>/fleet-settings.json` says how many days
//!   of ordinary threads this machine keeps offline (30 by default, 0 for
//!   all) and whether it fetches attachments as a client opens a thread or
//!   at once. Members learn the window from this runtime's member record.
//! - **Sync status.** `fleet.sync_status {}` answers, per record class and
//!   for thread history, how much this runtime holds of what its peers hold.
//!   `fleet.sync_changed` carries the same body while a bootstrap runs and
//!   once it ends.
//! - **Collection.** Once an hour a member drops the tombstones older than
//!   [`TOMBSTONE_DAYS`] that every member heard from within that time holds.
//! - **Fresh snapshot.** A member that comes back after more than
//!   [`TOMBSTONE_DAYS`] forgets the records it held, keeps its own writes
//!   no other member held, and takes the records again from its peers.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use muniment_attach::EventName;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::hlc::Hlc;
use super::{lock, parse_time, sync, Inner, MemberRecord, SnapshotMark, TOMBSTONE_DAYS};

/// The days of ordinary threads a member keeps offline by default.
pub const OFFLINE_DAYS: u32 = 30;
/// Attachments arrive as a client opens their thread.
pub(super) const ATTACHMENTS_OPEN: &str = "open";
/// Attachments arrive as soon as their event does.
pub(super) const ATTACHMENTS_ALL: &str = "all";
const SETTINGS_FILE: &str = "fleet-settings.json";
/// How often a member drops covered tombstones.
const COLLECT_SECONDS: i64 = 60 * 60;
/// The least time between two `fleet.sync_changed` events.
const STATUS_MILLIS: u128 = 1000;
/// The class name of thread history in the sync status.
const HISTORY: &str = "history";

/// What this machine keeps offline.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfflineSettings {
    /// Days of ordinary threads, `0` for all of them.
    pub thread_days: u32,
    /// `open` or `all`.
    pub attachments: String,
}

impl Default for OfflineSettings {
    fn default() -> Self {
        Self {
            thread_days: OFFLINE_DAYS,
            attachments: ATTACHMENTS_OPEN.into(),
        }
    }
}

#[derive(Default, Deserialize, Serialize)]
struct SettingsFile {
    #[serde(default)]
    offline: Option<OfflineSettings>,
}

fn settings_path(profile: &Path) -> PathBuf {
    profile.join(SETTINGS_FILE)
}

/// The offline settings saved in the profile, or the defaults.
pub(super) fn load_offline(profile: &Path) -> OfflineSettings {
    std::fs::read(settings_path(profile))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<SettingsFile>(&bytes).ok())
        .and_then(|file| file.offline)
        .unwrap_or_default()
}

/// This runtime's offline settings.
pub(super) fn offline(inner: &Inner) -> OfflineSettings {
    lock(&inner.offline).clone()
}

/// `fleet.offline_set {thread_days?, attachments?}`: saves what this
/// machine keeps offline and answers the settings.
pub(super) fn offline_set(inner: &Inner, body: Value) -> Result<Value, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Body {
        #[serde(default)]
        thread_days: Option<u32>,
        #[serde(default)]
        attachments: Option<String>,
    }
    let body: Body = serde_json::from_value(body).map_err(|_| "The request is invalid.")?;
    let mut settings = offline(inner);
    if let Some(days) = body.thread_days {
        if days > 3650 {
            return Err("Keep at most 3650 days offline, or 0 for every thread.".into());
        }
        settings.thread_days = days;
    }
    if let Some(attachments) = body.attachments {
        if attachments != ATTACHMENTS_OPEN && attachments != ATTACHMENTS_ALL {
            return Err("Attachments arrive on open or all at once.".into());
        }
        settings.attachments = attachments;
    }
    let file = SettingsFile {
        offline: Some(settings.clone()),
    };
    let bytes = serde_json::to_vec_pretty(&file).map_err(|error| error.to_string())?;
    let destination = settings_path(&inner.profile);
    let temporary = destination.with_extension(format!("tmp-{}", uuid::Uuid::now_v7()));
    std::fs::write(&temporary, &bytes)
        .and_then(|()| crate::atomic_file::replace(&temporary, &destination))
        .map_err(|error| {
            let _ = std::fs::remove_file(&temporary);
            format!("The fleet settings could not be saved: {error}")
        })?;
    if settings.attachments == ATTACHMENTS_ALL {
        inner
            .store
            .connect()?
            .execute("UPDATE blobs SET requested=1 WHERE done=0", [])
            .map_err(super::store::sql)?;
    }
    *lock(&inner.offline) = settings.clone();
    inner.nudge();
    Ok(json!({"offline": settings}))
}

/// Where the `fleet.sync_changed` events stand.
#[derive(Default)]
pub(super) struct StatusState {
    last: Option<Value>,
    at: Option<Instant>,
    /// A change waits for the next tick, as the last event was too recent.
    pending: bool,
}

/// One class's progress: what this runtime holds of what its peers hold.
fn progress(class: &str, held: u64, total: u64) -> Value {
    let total = total.max(held);
    let percent = (held * 100).checked_div(total).unwrap_or(100);
    json!({"class": class, "held": held, "total": total, "percent": percent})
}

/// `fleet.sync_status {}`: this runtime's progress per record class and for
/// thread history, its state and its offline settings.
pub(super) fn status(inner: &Inner) -> Result<Value, String> {
    let counts = inner.records.counts()?;
    let (snapshots, peer_clocks, peers) = {
        let peers = lock(&inner.peers);
        let mut totals: BTreeMap<String, u64> = BTreeMap::new();
        let mut clocks: super::Clocks = BTreeMap::new();
        for sync in peers.values() {
            if let Some(snapshot) = &sync.snapshot {
                for (class, rows) in &snapshot.classes {
                    let total = totals.entry(class.clone()).or_default();
                    *total = (*total).max(*rows);
                }
            }
            for (thread, origins) in &sync.clocks {
                let held = clocks.entry(thread.clone()).or_default();
                for (origin, seq) in origins {
                    let entry = held.entry(origin.clone()).or_default();
                    *entry = (*entry).max(*seq);
                }
            }
        }
        (totals, clocks, peers.len())
    };
    let mut classes = Vec::new();
    let mut complete = true;
    let names: std::collections::BTreeSet<&String> =
        counts.keys().chain(snapshots.keys()).collect();
    for class in names {
        let held = counts.get(class).map_or(0, |count| count.rows);
        let total = snapshots.get(class).copied().unwrap_or(0);
        complete &= held >= total;
        classes.push(progress(class, held, total));
    }
    let (held, total) = history_progress(inner, &peer_clocks)?;
    complete &= held >= total;
    classes.push(progress(HISTORY, held, total));
    let members = inner
        .store
        .records::<MemberRecord>("members", "record")?
        .into_iter()
        .filter(|member| Some(&member.runtime_id) != inner.runtime_id().as_ref())
        .collect::<Vec<_>>();
    let online = members
        .iter()
        .any(|member| super::link::online(inner, &member.runtime_id));
    let bootstrapping = inner.store.meta(BOOTSTRAPPING)?.is_some();
    let state = if members.is_empty() && peers == 0 {
        "alone"
    } else if bootstrapping || !complete {
        "bootstrapping"
    } else if !online {
        "offline"
    } else {
        "synced"
    };
    Ok(json!({
        "state": state,
        "classes": classes,
        "last_sync_at": inner.store.meta("last_sync_at")?,
        "snapshot_needed_at": inner.store.meta("snapshot_needed_at")?,
        "bootstrapped_at": inner.store.meta("bootstrapped_at")?,
        "offline": offline(inner),
    }))
}

/// The threads this runtime keeps the history of and holds every event its
/// peers named, out of those it keeps: every agent and group thread, and
/// the ordinary threads another member owns that were active within this
/// machine's window or that it pinned.
fn history_progress(inner: &Inner, peer_clocks: &super::Clocks) -> Result<(u64, u64), String> {
    let self_id = inner.runtime_id();
    let settings = offline(inner);
    let since = (settings.thread_days > 0).then(|| {
        super::timestamp(inner.now() - chrono::Duration::days(i64::from(settings.thread_days)))
    });
    let pins: std::collections::BTreeSet<String> = sync::own_pins(inner)?.into_iter().collect();
    let summaries = sync::summaries(inner)?;
    let mine = sync::clocks(inner)?;
    let (mut held, mut total) = (0, 0);
    for registration in sync::registrations(inner)? {
        if registration.kind == sync::PLAIN {
            if registration.owner_runtime == self_id {
                continue;
            }
            let active = match (&since, summaries.get(&registration.thread_id)) {
                (None, _) => true,
                (Some(since), Some(summary)) => summary.last_at.as_str() >= since.as_str(),
                (Some(_), None) => false,
            };
            if !active && !pins.contains(&registration.thread_id) {
                continue;
            }
        }
        total += 1;
        let have = mine.get(&registration.thread_id);
        let covered = peer_clocks
            .get(&registration.thread_id)
            .is_none_or(|clock| {
                clock.iter().all(|(origin, seq)| {
                    have.and_then(|have| have.get(origin)).copied().unwrap_or(0) >= *seq
                })
            });
        if covered {
            held += 1;
        }
    }
    Ok((held, total))
}

/// Sends `fleet.sync_changed` when the status changed while a bootstrap
/// runs, at most once a second, and once more when it ends. `news` says an
/// exchange carried records, clocks or events; `catching_up` says a peer
/// sent a snapshot mark, as it does while this runtime lacks many records.
/// Once synced, only a snapshot mark costs a status.
pub(super) fn note_progress(inner: &Inner, news: bool, catching_up: bool) {
    let due = {
        let mut state = lock(&inner.status);
        let synced = state
            .last
            .as_ref()
            .is_some_and(|last| last["state"] != "bootstrapping");
        let wanted = state.pending || catching_up || (news && !synced);
        if !wanted {
            return;
        }
        let recent = state
            .at
            .is_some_and(|at| at.elapsed().as_millis() < STATUS_MILLIS);
        if recent {
            state.pending = true;
            false
        } else {
            state.pending = false;
            state.at = Some(Instant::now());
            true
        }
    };
    if !due {
        return;
    }
    let status = match status(inner) {
        Ok(status) => status,
        Err(error) => {
            crate::runtime_eprintln!("muniment-runtime: swarm sync status failed: {error}");
            return;
        }
    };
    let changed = {
        let mut state = lock(&inner.status);
        let changed = state.last.as_ref() != Some(&status);
        state.last = Some(status.clone());
        changed
    };
    if changed {
        inner.emit(EventName::FleetSyncChanged, status);
    }
}

/// Sends a status change that waited for the throttle.
pub(super) fn flush_progress(inner: &Inner) {
    if lock(&inner.status).pending {
        note_progress(inner, false, false);
    }
}

/// The record counts a peer that catches up on records gets with its delta.
pub(super) fn snapshot_mark(inner: &Inner, complete: bool) -> Result<SnapshotMark, String> {
    Ok(SnapshotMark {
        classes: inner
            .records
            .counts()?
            .into_iter()
            .map(|(class, count)| (class, count.rows))
            .collect(),
        complete,
    })
}

/// The meta key set while this runtime takes a fresh snapshot after a long
/// absence.
pub(super) const BOOTSTRAPPING: &str = "bootstrapping";
/// The meta key of the threads other members ran when the fresh snapshot
/// began, with their owners.
const FORGOTTEN_THREADS: &str = "forgotten_threads";

/// Forgets the records this runtime held when it was away longer than a
/// tombstone lasts, keeping its own writes no member held when last heard
/// from, and takes the records again from its peers.
pub(super) fn rebootstrap(inner: &Inner) -> Result<(), String> {
    let origin = inner.origin()?;
    let delivered = inner
        .store
        .records::<MemberRecord>("members", "record")?
        .iter()
        .filter_map(|member| member.record_vv.as_ref()?.get(&origin).copied())
        .max()
        .unwrap_or(0);
    // The threads other members run are checked against the snapshot once
    // it arrives; one whose record never comes back was deleted meanwhile.
    let held: Vec<(String, String)> = sync::registrations(inner)?
        .into_iter()
        .filter_map(|registration| {
            let owner = sync::thread_primary(inner, &registration)?;
            (owner != origin).then_some((registration.thread_id, owner))
        })
        .collect();
    inner.records.forget(&origin, delivered)?;
    inner
        .store
        .connect()?
        .execute("DELETE FROM threads", [])
        .map_err(super::store::sql)?;
    inner.store.set_meta(
        FORGOTTEN_THREADS,
        Some(&serde_json::to_string(&held).map_err(|error| error.to_string())?),
    )?;
    inner.store.set_meta(BOOTSTRAPPING, Some("1"))?;
    lock(&inner.peers).clear();
    sync::rescan(inner);
    inner.nudge();
    crate::runtime_eprintln!(
        "muniment-runtime: swarm member was away past {TOMBSTONE_DAYS} days; it forgot its records and takes a fresh snapshot"
    );
    Ok(())
}

/// Ends a fresh snapshot once a peer sent every record it holds.
pub(super) fn snapshot_received(inner: &Inner) -> Result<(), String> {
    if inner.store.meta(BOOTSTRAPPING)?.is_none() {
        return Ok(());
    }
    let forgotten: Vec<(String, String)> = inner
        .store
        .meta(FORGOTTEN_THREADS)?
        .and_then(|held| serde_json::from_str(&held).ok())
        .unwrap_or_default();
    for (thread_id, owner) in forgotten {
        if sync::registration(inner, &thread_id).is_none() {
            sync::delete_copy(inner, &thread_id, &owner)?;
        }
    }
    inner.store.set_meta(FORGOTTEN_THREADS, None)?;
    inner.store.set_meta(BOOTSTRAPPING, None)?;
    inner.store.set_meta("snapshot_needed_at", None)?;
    inner
        .store
        .set_meta("bootstrapped_at", Some(&inner.now_text()))?;
    inner.changed("records");
    Ok(())
}

/// Drops the tombstones older than [`TOMBSTONE_DAYS`] that every member
/// heard from within that time holds. A member whose record names no
/// version vector, as an older member's does not, keeps every tombstone.
/// Runs once an hour unless `now` says otherwise. Answers how many rows it
/// dropped.
pub(super) fn collect(inner: &Inner, force: bool) -> Result<usize, String> {
    let now = inner.now();
    if !force {
        let last = inner.store.meta("gc_at")?.as_deref().and_then(parse_time);
        if last.is_some_and(|last| (now - last).num_seconds() < COLLECT_SECONDS) {
            return Ok(0);
        }
        inner
            .store
            .set_meta("gc_at", Some(&super::timestamp(now)))?;
    }
    let Some(self_id) = inner.runtime_id() else {
        return Ok(0);
    };
    let mut vectors = Vec::new();
    for member in inner.store.records::<MemberRecord>("members", "record")? {
        if member.runtime_id == self_id {
            continue;
        }
        let fresh = parse_time(&member.heartbeat_at)
            .is_some_and(|seen| (now - seen).num_days() < TOMBSTONE_DAYS);
        if !fresh {
            continue;
        }
        match member.record_vv {
            Some(vector) => vectors.push(vector),
            None => return Ok(0),
        }
    }
    let before = Hlc::at((now - chrono::Duration::days(TOMBSTONE_DAYS)).timestamp_millis());
    let dropped = inner.records.collect(before, |origin, seq| {
        vectors
            .iter()
            .all(|vector| vector.get(origin).copied().unwrap_or(0) >= seq)
    })?;
    if dropped > 0 {
        crate::runtime_eprintln!(
            "muniment-runtime: swarm dropped {dropped} tombstones older than {TOMBSTONE_DAYS} days"
        );
    }
    Ok(dropped)
}

/// `thread.history_fetch {thread_id, before?}`: pins an ordinary thread so
/// its history arrives from its owner or any member that holds it. Answers
/// `held` once this runtime holds every event its peers named, else
/// `fetching`; the runtime adds the `thread.history` page before `before`.
pub(super) fn history_fetch(inner: &Inner, body: Value) -> Result<Value, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Body {
        thread_id: String,
        #[serde(default)]
        before: Option<String>,
    }
    let body: Body = serde_json::from_value(body).map_err(|_| "The request is invalid.")?;
    let registration = sync::registration(inner, &body.thread_id)
        .ok_or("The swarm does not share this thread.")?;
    let owner = sync::thread_primary(inner, &registration);
    let own = owner.is_some() && owner == inner.runtime_id();
    if !own && registration.kind == sync::PLAIN && sync::pin(inner, &body.thread_id)? {
        inner.refresh_exports();
        inner.nudge();
    }
    let mine = sync::thread_clock(inner, &body.thread_id)?;
    let named: BTreeMap<String, u64> = {
        let peers = lock(&inner.peers);
        let mut named = BTreeMap::new();
        for sync in peers.values() {
            for (origin, seq) in sync.clocks.get(&body.thread_id).into_iter().flatten() {
                let entry: &mut u64 = named.entry(origin.clone()).or_default();
                *entry = (*entry).max(*seq);
            }
        }
        named
    };
    let held_events: u64 = mine.values().sum();
    let named_events: u64 = named
        .iter()
        .map(|(origin, seq)| (*seq).max(mine.get(origin).copied().unwrap_or(0)))
        .sum();
    let held = own
        || (!named.is_empty()
            && named
                .iter()
                .all(|(origin, seq)| mine.get(origin).copied().unwrap_or(0) >= *seq));
    Ok(json!({
        "thread_id": body.thread_id,
        "owner_runtime": owner,
        "status": if held { "held" } else { "fetching" },
        "held_events": held_events,
        "total_events": named_events.max(held_events),
        "before": body.before,
    }))
}
