//! The agent directory the members gossip, kept as records of class
//! `agent`. Each primary runtime describes the agents it runs in three
//! fields: `entry` (name, label, avatar, primary runtime, primary thread),
//! `definition` and `activity`. The agent's epoch ranks every write, so the
//! directory orders by (epoch, clock). A runtime that sees a higher epoch
//! for an agent it runs steps down.

use muniment_attach::EventName;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::hlc::Hlc;
use super::records::{Record, RecordChange};
use super::{DirectoryEntry, Inner};
use crate::agents::{self, Agent};

/// The record class of the directory.
pub(super) const CLASS: &str = "agent";
pub(super) const ENTRY: &str = "entry";
const DEFINITION: &str = "definition";
const ACTIVITY: &str = "activity";

/// The bytes of agent definitions one exchange carries at most.
const DEFINITION_BUDGET: usize = 160 * 1024;

/// One agent a message or move names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Target {
    pub(super) agent_id: String,
    pub(super) name: String,
    pub(super) primary_runtime: Option<String>,
}

/// This runtime's agents. A profile without a Home folder has none.
pub(super) fn local_agents(inner: &Inner) -> Vec<Agent> {
    agents::list(&inner.profile)
        .map(|listing| listing.agents)
        .unwrap_or_default()
}

/// Whether this runtime runs `agent`.
pub(super) fn runs_here(inner: &Inner, agent: &Agent) -> bool {
    match (&agent.primary_runtime, inner.runtime_id()) {
        (None, _) => true,
        (Some(primary), Some(own)) => *primary == own,
        (Some(_), None) => false,
    }
}

fn definition(agent: &Agent) -> Option<Value> {
    let mut value = serde_json::to_value(agent).ok()?;
    if let Some(routines) = value.get_mut("routines").and_then(Value::as_array_mut) {
        for routine in routines {
            if let Some(routine) = routine.as_object_mut() {
                routine.remove("last_fired_at");
                routine.remove("last_result");
                routine.remove("next_fire_at");
            }
        }
    }
    Some(value)
}

/// The `entry` field of an agent's record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct EntryField {
    name: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    avatar: Option<Value>,
    primary_runtime: String,
    #[serde(default)]
    primary_thread: Option<String>,
}

/// The directory entry an agent's record holds. A deleted agent reads as a
/// deleted entry from its tombstone.
fn from_record(record: &Record) -> Option<DirectoryEntry> {
    let updated = |clock: Hlc| super::timestamp(clock.time());
    if !record.is_live() {
        let tombstone = record.tombstone.as_ref()?;
        return Some(DirectoryEntry {
            agent_id: record.key.clone(),
            name: String::new(),
            label: String::new(),
            avatar: None,
            primary_runtime: tombstone.origin.clone(),
            epoch: tombstone.rank,
            updated_at: updated(tombstone.clock),
            deleted: true,
            primary_thread: None,
            move_suggested: false,
            definition: None,
            activity: None,
        });
    }
    let entry = record.fields.get(ENTRY)?;
    let body: EntryField = serde_json::from_value(entry.value.clone()).ok()?;
    Some(DirectoryEntry {
        agent_id: record.key.clone(),
        name: body.name,
        label: body.label,
        avatar: body.avatar,
        primary_runtime: body.primary_runtime,
        epoch: entry.rank,
        updated_at: updated(record.fields_clock().unwrap_or(entry.clock)),
        deleted: false,
        primary_thread: body.primary_thread,
        move_suggested: false,
        definition: record
            .value(DEFINITION)
            .filter(|value| !value.is_null())
            .cloned(),
        activity: record
            .value(ACTIVITY)
            .and_then(|value| serde_json::from_value(value.clone()).ok()),
    })
}

/// The `(epoch, clock)` an entry holds, which a full-state entry from an
/// older member must beat.
fn held_version(inner: &Inner, agent_id: &str) -> Result<Option<(u64, Hlc)>, String> {
    Ok(inner.records.get(CLASS, agent_id)?.and_then(|record| {
        match (&record.tombstone, record.fields.get(ENTRY)) {
            (_, Some(entry)) if record.is_live() => Some((entry.rank, record.clock)),
            (Some(tombstone), _) => Some((tombstone.rank, tombstone.clock)),
            _ => None,
        }
    }))
}

/// Writes an entry's fields at its epoch, or its tombstone when deleted.
/// A definition left out keeps the one held. `clock` replaces this
/// runtime's clock for an entry an older member sent. Answers whether the
/// directory changed.
pub(super) fn put_entry(
    inner: &Inner,
    entry: &DirectoryEntry,
    clock: Option<Hlc>,
) -> Result<bool, String> {
    let origin = inner.origin()?;
    let now = inner.now();
    let rank = entry.epoch;
    if entry.deleted {
        return Ok(inner
            .records
            .delete(&origin, now, CLASS, &entry.agent_id, rank, clock)?
            .is_some());
    }
    let body = serde_json::to_value(EntryField {
        name: entry.name.clone(),
        label: entry.label.clone(),
        avatar: entry.avatar.clone(),
        primary_runtime: entry.primary_runtime.clone(),
        primary_thread: entry.primary_thread.clone(),
    })
    .map_err(|error| error.to_string())?;
    let activity = entry
        .activity
        .as_ref()
        .map_or(Ok(Value::Null), serde_json::to_value)
        .map_err(|error| error.to_string())?;
    let mut fields = vec![(ENTRY, body), (ACTIVITY, activity)];
    if let Some(definition) = &entry.definition {
        fields.push((DEFINITION, definition.clone()));
    }
    let mut changed = false;
    for (field, value) in fields {
        let written = match clock {
            Some(clock) => inner.records.put_at(
                &origin,
                now,
                CLASS,
                &entry.agent_id,
                field,
                value,
                rank,
                clock,
            )?,
            None => inner
                .records
                .put(&origin, now, CLASS, &entry.agent_id, field, value, rank)?,
        };
        changed |= written.is_some();
    }
    Ok(changed)
}

/// Derives the activity of each agent this runtime runs and sends
/// `agent.activity` to this runtime's clients for each one that changed.
pub(super) fn refresh_activity(inner: &Inner) -> Result<bool, String> {
    let ids: Vec<String> = local_agents(inner)
        .into_iter()
        .filter(|agent| runs_here(inner, agent))
        .map(|agent| agent.id)
        .collect();
    let activities = crate::agent_runs::activities(&inner.profile, &ids, &|run_id| {
        super::approvals::pending_summary(inner, run_id)
    })?;
    let runtime_id = inner.runtime_id();
    let mut known = super::lock(&inner.activities);
    known.retain(|agent_id, _| activities.contains_key(agent_id));
    for (agent_id, activity) in activities {
        if known.get(&agent_id) == Some(&activity) {
            continue;
        }
        inner.emit(
            EventName::AgentActivity,
            json!({"agent_id": agent_id, "runtime_id": runtime_id, "activity": activity}),
        );
        known.insert(agent_id, activity);
    }
    Ok(false)
}

/// Describes this runtime's agents in the directory and marks the ones it
/// no longer has as deleted.
pub(super) fn refresh_local(inner: &Inner) -> Result<bool, String> {
    let Some(self_id) = inner.runtime_id() else {
        return Ok(false);
    };
    let state = agents::state(&inner.profile).unwrap_or_default();
    let local = local_agents(inner);
    let mut changed = false;
    for agent in local.iter().filter(|agent| runs_here(inner, agent)) {
        // A move to another runtime with a later epoch wins; the merge
        // steps this runtime down.
        if entry(inner, &agent.id).is_some_and(|existing| {
            existing.epoch > agent.epoch && existing.primary_runtime != self_id
        }) {
            continue;
        }
        let desired = DirectoryEntry {
            agent_id: agent.id.clone(),
            name: agent.name.clone(),
            label: agent.label.clone(),
            avatar: agent
                .avatar
                .as_ref()
                .and_then(|avatar| serde_json::to_value(avatar).ok()),
            primary_runtime: self_id.clone(),
            epoch: agent.epoch,
            updated_at: String::new(),
            deleted: false,
            primary_thread: state.primary_threads.get(&agent.id).cloned(),
            move_suggested: false,
            definition: definition(agent),
            activity: super::lock(&inner.activities).get(&agent.id).cloned(),
        };
        changed |= put_entry(inner, &desired, None)?;
    }
    for mut entry in all(inner)? {
        if entry.deleted
            || entry.primary_runtime != self_id
            || local.iter().any(|agent| agent.id == entry.agent_id)
        {
            continue;
        }
        entry.deleted = true;
        changed |= put_entry(inner, &entry, None)?;
    }
    if changed {
        inner.changed("agents");
    }
    Ok(false)
}

/// Takes in the full-state directory of a member without `store/1`: each
/// entry whose (epoch, time) beats the one held is written with its own
/// time.
pub(super) fn merge_full(inner: &Inner, entries: Vec<DirectoryEntry>) -> Result<bool, String> {
    let mut changed = Vec::new();
    for entry in entries {
        if uuid::Uuid::parse_str(&entry.agent_id).is_err() {
            continue;
        }
        let Some(time) = super::parse_time(&entry.updated_at) else {
            continue;
        };
        let clock = Hlc::at(time.timestamp_millis());
        if held_version(inner, &entry.agent_id)?.is_some_and(|held| (entry.epoch, clock) <= held) {
            continue;
        }
        let before = self::entry(inner, &entry.agent_id);
        if put_entry(inner, &entry, Some(clock))? {
            changed.push((before, entry.agent_id.clone()));
        }
    }
    for (before, agent_id) in &changed {
        if let Some(after) = self::entry(inner, agent_id) {
            announce(inner, before.as_ref(), &after);
            step_down(inner, &after);
        }
    }
    Ok(!changed.is_empty())
}

/// Reacts to directory records another member wrote: activity events and
/// step-downs. Answers whether the directory changed.
pub(super) fn on_changes(inner: &Inner, changes: &[RecordChange]) -> bool {
    let mut keys: Vec<&str> = changes
        .iter()
        .filter(|change| change.class == CLASS && !change.local)
        .map(|change| change.key.as_str())
        .collect();
    keys.sort_unstable();
    keys.dedup();
    for key in &keys {
        let Some(after) = entry(inner, key) else {
            continue;
        };
        let activity_changed = changes
            .iter()
            .any(|change| change.key == *key && change.field == ACTIVITY);
        if activity_changed {
            announce(inner, None, &after);
        }
        step_down(inner, &after);
    }
    !keys.is_empty()
}

/// Sends `agent.activity` for an entry another member runs whose activity
/// changed.
fn announce(inner: &Inner, before: Option<&DirectoryEntry>, entry: &DirectoryEntry) {
    let activity_changed = before.is_none_or(|before| before.activity != entry.activity);
    if activity_changed
        && entry.activity.is_some()
        && Some(&entry.primary_runtime) != inner.runtime_id().as_ref()
    {
        inner.emit(
            EventName::AgentActivity,
            json!({
                "agent_id": entry.agent_id,
                "runtime_id": entry.primary_runtime,
                "activity": entry.activity,
            }),
        );
    }
}

/// Hands an agent this runtime runs to the runtime a later epoch names.
fn step_down(inner: &Inner, entry: &DirectoryEntry) {
    let Some(self_id) = inner.runtime_id() else {
        return;
    };
    if entry.primary_runtime == self_id || entry.deleted {
        return;
    }
    let Ok(mut agent) = agents::get(&inner.profile, &entry.agent_id) else {
        return;
    };
    if !runs_here(inner, &agent) || entry.epoch <= agent.epoch {
        return;
    }
    agent.primary_runtime = Some(entry.primary_runtime.clone());
    agent.epoch = entry.epoch;
    match agents::save(&inner.profile, agent) {
        Ok(_) => crate::runtime_eprintln!(
            "muniment-runtime: swarm stepped down agent_id={} primary={} epoch={}",
            entry.agent_id,
            entry.primary_runtime,
            entry.epoch
        ),
        Err(error) => crate::runtime_eprintln!(
            "muniment-runtime: swarm step-down failed agent_id={}: {error}",
            entry.agent_id
        ),
    }
}

/// Every entry the directory holds, deleted ones included.
pub(super) fn all(inner: &Inner) -> Result<Vec<DirectoryEntry>, String> {
    Ok(inner
        .records
        .scan(CLASS, None, usize::MAX)?
        .iter()
        .filter_map(from_record)
        .collect())
}

/// The directory, newest first. With definitions, as an exchange carries it,
/// up to its byte budget.
pub(super) fn entries(
    inner: &Inner,
    with_definitions: bool,
) -> Result<Vec<DirectoryEntry>, String> {
    let mut entries = all(inner)?;
    entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    let mut budget = DEFINITION_BUDGET;
    for entry in &mut entries {
        let size = entry
            .definition
            .as_ref()
            .and_then(|definition| serde_json::to_vec(definition).ok())
            .map_or(0, |bytes| bytes.len());
        if !with_definitions || size > budget {
            entry.definition = None;
        } else {
            budget -= size;
        }
    }
    if !with_definitions {
        entries.retain(|entry| !entry.deleted);
        entries.sort_by_key(|entry| entry.name.to_lowercase());
    }
    Ok(entries)
}

pub(super) fn entry(inner: &Inner, agent_id: &str) -> Option<DirectoryEntry> {
    inner
        .records
        .get(CLASS, agent_id)
        .ok()
        .flatten()
        .as_ref()
        .and_then(from_record)
}

/// Moves the directory a store held before records into records, once.
pub(super) fn migrate(inner: &Inner) -> Result<(), String> {
    let old: Vec<DirectoryEntry> = inner.store.records("directory", "entry")?;
    for entry in &old {
        if let Some(time) = super::parse_time(&entry.updated_at) {
            put_entry(inner, entry, Some(Hlc::at(time.timestamp_millis())))?;
        }
    }
    if !old.is_empty() {
        inner
            .store
            .connect()?
            .execute("DELETE FROM directory", [])
            .map_err(super::store::sql)?;
    }
    Ok(())
}

/// The runtime that runs an agent: this runtime's own record of it first,
/// else the directory.
pub(super) fn primary_of(inner: &Inner, agent_id: &str) -> Option<String> {
    match agents::get(&inner.profile, agent_id) {
        Ok(agent) => agent.primary_runtime.or_else(|| inner.runtime_id()),
        Err(_) => entry(inner, agent_id)
            .filter(|entry| !entry.deleted)
            .map(|entry| entry.primary_runtime),
    }
}

/// Finds an agent by id, or by its name in any case.
pub(super) fn resolve(inner: &Inner, to: &str) -> Result<Target, String> {
    let to = to.trim();
    let mut candidates: Vec<Target> = Vec::new();
    for entry in entries(inner, false)? {
        candidates.push(Target {
            agent_id: entry.agent_id,
            name: entry.name,
            primary_runtime: Some(entry.primary_runtime),
        });
    }
    for agent in local_agents(inner) {
        if runs_here(inner, &agent) && !candidates.iter().any(|known| known.agent_id == agent.id) {
            candidates.push(Target {
                primary_runtime: agent.primary_runtime.clone().or_else(|| inner.runtime_id()),
                agent_id: agent.id,
                name: agent.name,
            });
        }
    }
    for candidate in &candidates {
        crate::tool_presentation::remember_agent(&candidate.agent_id, &candidate.name);
    }
    if let Some(found) = candidates.iter().find(|candidate| candidate.agent_id == to) {
        return Ok(found.clone());
    }
    let named: Vec<&Target> = candidates
        .iter()
        .filter(|candidate| candidate.name.eq_ignore_ascii_case(to))
        .collect();
    match named.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(format!("No agent is named {to}.")),
        _ => Err(format!(
            "Several agents are named {to}. Name one by its id."
        )),
    }
}
