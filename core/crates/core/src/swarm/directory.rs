//! The agent directory the members gossip. Each primary runtime describes
//! the agents it runs; the entry with the highest epoch, then the latest
//! `updated_at`, wins. A runtime that sees a higher epoch for an agent it
//! runs steps down.

use muniment_attach::EventName;
use serde_json::{json, Value};

use super::{DirectoryEntry, Inner};
use crate::agents::{self, Agent};

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

fn same_content(a: &DirectoryEntry, b: &DirectoryEntry) -> bool {
    let strip = |entry: &DirectoryEntry| DirectoryEntry {
        updated_at: String::new(),
        ..entry.clone()
    };
    strip(a) == strip(b)
}

/// A time after `earlier`, so a changed entry always wins over the one before.
fn later_than(now: String, earlier: Option<&str>) -> String {
    match earlier.and_then(super::parse_time) {
        Some(earlier) if now.as_str() <= super::timestamp(earlier).as_str() => {
            super::timestamp(earlier + chrono::Duration::seconds(1))
        }
        _ => now,
    }
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
        let existing: Option<DirectoryEntry> =
            inner
                .store
                .record("directory", "agent_id", "entry", &agent.id)?;
        let mut entry = DirectoryEntry {
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
            definition: definition(agent),
            activity: super::lock(&inner.activities).get(&agent.id).cloned(),
        };
        if let Some(existing) = &existing {
            // A move to another runtime with a later epoch wins; the merge
            // steps this runtime down.
            if existing.epoch > agent.epoch && existing.primary_runtime != self_id {
                continue;
            }
            entry.updated_at = existing.updated_at.clone();
            if same_content(existing, &entry) {
                continue;
            }
        }
        entry.updated_at = later_than(
            inner.now_text(),
            existing.as_ref().map(|entry| entry.updated_at.as_str()),
        );
        inner
            .store
            .put_record("directory", "agent_id", "entry", &agent.id, &entry)?;
        changed = true;
    }
    for mut entry in inner
        .store
        .records::<DirectoryEntry>("directory", "entry")?
    {
        if entry.deleted
            || entry.primary_runtime != self_id
            || local.iter().any(|agent| agent.id == entry.agent_id)
        {
            continue;
        }
        entry.deleted = true;
        entry.definition = None;
        entry.updated_at = later_than(inner.now_text(), Some(&entry.updated_at));
        inner
            .store
            .put_record("directory", "agent_id", "entry", &entry.agent_id, &entry)?;
        changed = true;
    }
    if changed {
        inner.changed("agents");
    }
    Ok(false)
}

/// Keeps each entry that wins over the stored one, and steps this runtime
/// down from agents another runtime now runs.
pub(super) fn merge(inner: &Inner, entries: Vec<DirectoryEntry>) -> Result<bool, String> {
    let mut changed = false;
    for mut entry in entries {
        if uuid::Uuid::parse_str(&entry.agent_id).is_err() {
            continue;
        }
        let existing: Option<DirectoryEntry> =
            inner
                .store
                .record("directory", "agent_id", "entry", &entry.agent_id)?;
        let wins = existing
            .as_ref()
            .is_none_or(|existing| entry.version() > existing.version());
        if !wins {
            continue;
        }
        let activity_changed = existing
            .as_ref()
            .is_none_or(|existing| existing.activity != entry.activity);
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
        if entry.definition.is_none() && !entry.deleted {
            entry.definition = existing.and_then(|existing| existing.definition);
        }
        inner
            .store
            .put_record("directory", "agent_id", "entry", &entry.agent_id, &entry)?;
        changed = true;
        step_down(inner, &entry);
    }
    Ok(changed)
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

/// The directory, newest first. With definitions, as an exchange carries it,
/// up to its byte budget.
pub(super) fn entries(
    inner: &Inner,
    with_definitions: bool,
) -> Result<Vec<DirectoryEntry>, String> {
    let mut entries = inner
        .store
        .records::<DirectoryEntry>("directory", "entry")?;
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
        .store
        .record("directory", "agent_id", "entry", agent_id)
        .ok()
        .flatten()
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
