//! The absence pause: when no client used any member for 14 days, each
//! member pauses the routines of the agents it runs with `paused_reason:
//! "absence"`. `swarm.resume` on any member clears the pause on every member.

use serde_json::{json, Value};

use super::{directory, lock, Inner};
use crate::agents;

/// How long the swarm waits without a client before it pauses routines.
pub const ABSENCE_DAYS: i64 = 14;
/// The pause reason the absence pause writes.
pub const ABSENCE_REASON: &str = "absence";
const CLIENT_NOTE_SECONDS: i64 = 30;

pub(super) fn initialize(inner: &Inner) -> Result<(), String> {
    if inner.store.meta("last_client_at")?.is_none() {
        inner
            .store
            .set_meta("last_client_at", Some(&inner.now_text()))?;
    }
    Ok(())
}

/// Records that a client used this runtime, at most twice a minute.
pub(super) fn note_client(inner: &Inner) -> Result<(), String> {
    let now = inner.now();
    let recent = inner
        .store
        .meta("last_client_at")?
        .as_deref()
        .and_then(super::parse_time)
        .is_some_and(|seen| (now - seen).num_seconds() < CLIENT_NOTE_SECONDS);
    if !recent {
        inner
            .store
            .set_meta("last_client_at", Some(&super::timestamp(now)))?;
    }
    Ok(())
}

/// The latest time a client used any member this runtime heard of.
pub(super) fn swarm_last_client(inner: &Inner) -> Result<Option<String>, String> {
    let mut latest = inner.store.meta("last_client_at")?;
    let gossiped = inner.store.meta("swarm_client_at")?;
    if gossiped > latest {
        latest = gossiped;
    }
    for member in inner
        .store
        .records::<super::MemberRecord>("members", "record")?
    {
        if member.last_client_at > latest {
            latest = member.last_client_at;
        }
    }
    Ok(latest)
}

/// Pauses the routines of this runtime's agents once the swarm saw no
/// client for [`ABSENCE_DAYS`].
pub(super) fn check(inner: &Inner) -> Result<(), String> {
    if inner.store.meta("absence_paused_at")?.is_some() {
        return Ok(());
    }
    let Some(last) = swarm_last_client(inner)?
        .as_deref()
        .and_then(super::parse_time)
    else {
        return Ok(());
    };
    if (inner.now() - last).num_days() < ABSENCE_DAYS {
        return Ok(());
    }
    // The pause is on record before any agent pauses, so a reader that sees
    // a paused agent also sees the pause.
    inner
        .store
        .set_meta("absence_paused_at", Some(&inner.now_text()))?;
    let mut paused = Vec::new();
    for mut agent in directory::local_agents(inner) {
        if !directory::runs_here(inner, &agent)
            || agent.paused_reason.is_some()
            || !agent.routines.iter().any(|routine| routine.enabled)
        {
            continue;
        }
        agent.paused_reason = Some(ABSENCE_REASON.into());
        agent.schedule = None;
        let id = agent.id.clone();
        match agents::save(&inner.profile, agent) {
            Ok(_) => paused.push(id),
            Err(error) => crate::runtime_eprintln!(
                "muniment-runtime: absence pause failed agent_id={id}: {error}"
            ),
        }
    }
    crate::runtime_eprintln!(
        "muniment-runtime: routines paused for absence agents={}",
        paused.len()
    );
    inner.emit(
        muniment_attach::EventName::SwarmChanged,
        json!({"kind": "absence", "paused": true, "agents": paused}),
    );
    Ok(())
}

/// Clears this runtime's absence pauses and answers the agents it resumed.
fn clear(inner: &Inner) -> Result<Vec<String>, String> {
    let mut resumed = Vec::new();
    for mut agent in directory::local_agents(inner) {
        if agent.paused_reason.as_deref() != Some(ABSENCE_REASON) {
            continue;
        }
        agent.paused_reason = None;
        agent.schedule = None;
        let id = agent.id.clone();
        agents::save(&inner.profile, agent)?;
        resumed.push(id);
    }
    inner.store.set_meta("absence_paused_at", None)?;
    inner.emit(
        muniment_attach::EventName::SwarmChanged,
        json!({"kind": "absence", "paused": false, "agents": resumed}),
    );
    Ok(resumed)
}

/// `swarm.resume`.
pub(super) fn resume(inner: &Inner) -> Result<Value, String> {
    let _merge = lock(&inner.merge);
    let now = inner.now_text();
    inner.store.set_meta("resumed_at", Some(&now))?;
    inner.store.set_meta("last_client_at", Some(&now))?;
    let resumed = clear(inner)?;
    inner.nudge();
    Ok(json!({"resumed": resumed}))
}

/// Takes in the latest client time and resume another member knows. A
/// resume after this runtime's pause clears it.
pub(super) fn merge(
    inner: &Inner,
    client_seen_at: Option<String>,
    resumed_at: Option<String>,
) -> Result<(), String> {
    if client_seen_at.is_some() && client_seen_at > inner.store.meta("swarm_client_at")? {
        inner
            .store
            .set_meta("swarm_client_at", client_seen_at.as_deref())?;
    }
    if resumed_at.is_some() && resumed_at > inner.store.meta("resumed_at")? {
        inner.store.set_meta("resumed_at", resumed_at.as_deref())?;
        if let Some(paused_at) = inner.store.meta("absence_paused_at")? {
            if resumed_at.as_deref() >= Some(paused_at.as_str()) {
                clear(inner)?;
            }
        }
    }
    Ok(())
}

pub(super) fn view(inner: &Inner) -> Result<Value, String> {
    let paused_at = inner.store.meta("absence_paused_at")?;
    Ok(json!({
        "paused": paused_at.is_some(),
        "paused_at": paused_at,
        "last_client_at": swarm_last_client(inner)?,
    }))
}
