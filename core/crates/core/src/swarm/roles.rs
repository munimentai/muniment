//! Roles: what each member may hold, pass on and write, by the role the
//! signed roster gives it. Every exchange passes through here, once as it
//! leaves and once as it arrives, for every record class, thread, message
//! and secret.
//!
//! - **Full** members hold and relay everything.
//! - A **worker** holds the content of the agents it runs: their directory
//!   fields, threads, messages, approvals and run notices, and the leases
//!   sealed to it. It holds the ordinary threads it owns the same way: their
//!   registration, summary, runs and posts, so a full member shows them and
//!   steers them with a post. Of the rest it holds the fleet's metadata
//!   alone: the members, the roster and each agent's directory entry. A
//!   record class [`visibility`] does not name stays with full members, so
//!   a class a later stage adds never reaches a worker by default.
//! - A worker is a **leaf**. It sends only what it wrote, and a full member
//!   takes from it only what it wrote about the agents it runs and the
//!   ordinary threads it owns.
//! - A peer that has not proved a place in the roster over the channel it
//!   uses gets the roster alone, and nothing it sends is taken in.

use std::collections::BTreeSet;

use rusqlite::OptionalExtension;

use super::fleet::{self, Role};
use super::store::sql;
use super::{
    audit, directory, messaging, sync, BlobWant, Exchange, Inner, RecordRow, SealedSecret,
};
use crate::journal::EventPayload;

/// What one peer may hold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Access {
    /// A trusted full member over a proven channel, or any peer while this
    /// runtime holds no fleet keys.
    Full,
    /// A peer the roster does not name that predates fleet keys, or the
    /// inviter over this runtime's pinned link before its vouch arrives:
    /// everything but sealed secrets.
    Legacy,
    /// A worker over a proven channel.
    Worker(Grant),
    /// No proven place in the roster: the roster alone.
    Nothing,
}

/// What a worker may hold and write.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Grant {
    pub(super) runtime_id: String,
    /// The agents whose primary runtime it is.
    pub(super) agents: BTreeSet<String>,
    /// The threads of those agents, and the ordinary threads it owns.
    pub(super) threads: BTreeSet<String>,
}

/// Who may hold one record row.
enum Visibility {
    /// Every member: an agent's directory entry or its tombstone.
    Members,
    /// The members that run the agent, and full members.
    Agent(String),
    /// The members that run the thread's agent, and full members.
    Thread {
        thread_id: String,
        agent_id: Option<String>,
    },
    /// Full members alone.
    Full,
}

fn visibility(row: &RecordRow) -> Visibility {
    match row.class.as_str() {
        directory::CLASS if row.field.is_empty() || row.field == directory::ENTRY => {
            Visibility::Members
        }
        directory::CLASS => Visibility::Agent(row.key.clone()),
        sync::CLASS => Visibility::Thread {
            thread_id: row.key.clone(),
            agent_id: row
                .value
                .as_ref()
                .filter(|value| value["kind"] == "agent")
                .and_then(|value| value["agent_id"].as_str())
                .map(str::to_owned),
        },
        _ => Visibility::Full,
    }
}

impl Grant {
    /// The grant of the worker `runtime_id`: the agents the directory names
    /// it primary of, their threads, and the ordinary threads it owns.
    pub(super) fn of(inner: &Inner, runtime_id: &str) -> Result<Self, String> {
        let agents: BTreeSet<String> = directory::all(inner)?
            .into_iter()
            .filter(|entry| !entry.deleted && entry.primary_runtime == runtime_id)
            .map(|entry| entry.agent_id)
            .collect();
        let threads = sync::registrations(inner)?
            .into_iter()
            .filter(|registration| match registration.kind.as_str() {
                "agent" => registration
                    .agent_id
                    .as_ref()
                    .is_some_and(|agent| agents.contains(agent)),
                sync::PLAIN => registration.owner_runtime.as_deref() == Some(runtime_id),
                _ => false,
            })
            .map(|registration| registration.thread_id)
            .collect();
        Ok(Self {
            runtime_id: runtime_id.to_owned(),
            agents,
            threads,
        })
    }

    fn holds_row(&self, row: &RecordRow) -> bool {
        match visibility(row) {
            Visibility::Members => true,
            Visibility::Agent(agent) => self.agents.contains(&agent),
            Visibility::Thread {
                thread_id,
                agent_id,
            } => {
                self.threads.contains(&thread_id)
                    || agent_id.is_some_and(|agent| self.agents.contains(&agent))
            }
            Visibility::Full => false,
        }
    }

    /// Widens the grant by what the worker writes in `rows`: an agent it
    /// creates, which no member held before, the threads of its agents, and
    /// an ordinary thread it registers as its own that no other member owns.
    fn with_writes(mut self, inner: &Inner, rows: &[RecordRow]) -> Self {
        for row in rows {
            let creates = row.class == directory::CLASS
                && row.field == directory::ENTRY
                && row.origin == self.runtime_id
                && row
                    .value
                    .as_ref()
                    .is_some_and(|value| value["primary_runtime"] == self.runtime_id.as_str())
                && inner
                    .records
                    .get(directory::CLASS, &row.key)
                    .ok()
                    .flatten()
                    .is_none();
            if creates {
                self.agents.insert(row.key.clone());
            }
        }
        for row in rows {
            if let Visibility::Thread {
                thread_id,
                agent_id: Some(agent),
            } = visibility(row)
            {
                if row.origin == self.runtime_id && self.agents.contains(&agent) {
                    self.threads.insert(thread_id);
                }
            }
        }
        for row in rows {
            let registers = row.class == sync::CLASS
                && row.field == sync::REGISTRATION
                && row.origin == self.runtime_id
                && self.owns_plain(row)
                && row
                    .value
                    .as_ref()
                    .is_some_and(|value| value["thread_id"] == row.key.as_str())
                && sync::registration(inner, &row.key).is_none_or(|held| {
                    held.kind == sync::PLAIN
                        && held.owner_runtime.as_deref() == Some(self.runtime_id.as_str())
                });
            if registers {
                self.threads.insert(row.key.clone());
            }
        }
        self
    }

    /// Whether a thread row names an ordinary thread this worker owns.
    fn owns_plain(&self, row: &RecordRow) -> bool {
        row.value.as_ref().is_some_and(|value| {
            value["owner_runtime"] == self.runtime_id.as_str()
                && (row.field != sync::REGISTRATION || value["kind"] == sync::PLAIN)
        })
    }

    /// Whether the worker may write a row a full member takes in: its own
    /// audit records, the fields and thread registrations of the agents it
    /// runs, as their primary, and the registration and summary of each
    /// ordinary thread it owns. It may neither delete one of those agents
    /// nor name another primary for it, nor name another owner of a thread.
    fn writes_row(&self, inner: &Inner, row: &RecordRow) -> bool {
        if row.origin != self.runtime_id {
            return false;
        }
        match row.class.as_str() {
            audit::CLASS => audit::written_by(row, &self.runtime_id),
            directory::CLASS => {
                if !self.agents.contains(&row.key) || row.field.is_empty() {
                    return false;
                }
                let held_rank = inner
                    .records
                    .get(directory::CLASS, &row.key)
                    .ok()
                    .flatten()
                    .and_then(|record| record.fields.get(directory::ENTRY).map(|entry| entry.rank));
                held_rank.is_none_or(|held| row.rank <= held)
                    && (row.field != directory::ENTRY
                        || row.value.as_ref().is_some_and(|value| {
                            value["primary_runtime"] == self.runtime_id.as_str()
                        }))
            }
            sync::CLASS => {
                let agent_thread = matches!(
                    visibility(row),
                    Visibility::Thread {
                        agent_id: Some(_),
                        ..
                    }
                );
                !row.field.is_empty()
                    && self.holds_row(row)
                    && (agent_thread || self.owns_plain(row))
            }
            _ => false,
        }
    }
}

impl Access {
    /// Whether the peer may hold a record row.
    pub(super) fn holds_row(&self, row: &RecordRow) -> bool {
        match self {
            Self::Full | Self::Legacy => true,
            Self::Worker(grant) => grant.holds_row(row),
            Self::Nothing => false,
        }
    }

    /// Whether the peer may hold a thread's events and clock.
    pub(super) fn holds_thread(&self, thread_id: &str) -> bool {
        match self {
            Self::Full | Self::Legacy => true,
            Self::Worker(grant) => grant.threads.contains(thread_id),
            Self::Nothing => false,
        }
    }

    /// Whether the peer may hold a sealed secret: a full member any, a
    /// worker those sealed to it.
    pub(super) fn holds_sealed(&self, secret: &SealedSecret) -> bool {
        match self {
            Self::Full => true,
            Self::Worker(grant) => secret.recipients.contains_key(&grant.runtime_id),
            Self::Legacy | Self::Nothing => false,
        }
    }

    /// The runtime a worker's messages must name as their target.
    pub(super) fn message_target(&self) -> Option<&str> {
        match self {
            Self::Worker(grant) => Some(&grant.runtime_id),
            _ => None,
        }
    }
}

/// What `peer` may hold, by its role in this runtime's roster and how the
/// current exchange reaches it.
pub(super) fn peer_access(inner: &Inner, peer: &str) -> Result<Access, String> {
    if fleet::keys(inner).is_err() {
        return Ok(Access::Full);
    }
    let view = fleet::view(inner)?;
    if view.removed.contains(peer) {
        return Ok(Access::Nothing);
    }
    Ok(match view.role(peer) {
        Some(_) if !fleet::proven(inner, peer)? => Access::Nothing,
        Some(Role::Full) => Access::Full,
        Some(Role::Worker) => Access::Worker(Grant::of(inner, peer)?),
        None if fleet::legacy_channel(inner, peer)? => Access::Legacy,
        None => Access::Nothing,
    })
}

/// This runtime's grant when the roster makes it a worker, which makes it
/// a leaf.
pub(super) fn leaf(inner: &Inner) -> Result<Option<Grant>, String> {
    if fleet::keys(inner).is_err() {
        return Ok(None);
    }
    let view = fleet::view(inner)?;
    if view.role(&view.self_id) != Some(Role::Worker) {
        return Ok(None);
    }
    Grant::of(inner, &view.self_id).map(Some)
}

/// Whether a member that is a leaf sends a record row: only its own.
pub(super) fn leaf_sends_row(leaf: Option<&Grant>, row: &RecordRow) -> bool {
    leaf.is_none_or(|grant| row.origin == grant.runtime_id)
}

/// Whether a member that is a leaf sends a thread event: only its own.
pub(super) fn leaf_sends_event(leaf: Option<&Grant>, origin: &str) -> bool {
    leaf.is_none_or(|grant| origin == grant.runtime_id)
}

/// The content-store objects `access` asked for that it may hold: for a
/// worker, those its own threads' run events name.
pub(super) fn servable(
    inner: &Inner,
    access: &Access,
    wants: Vec<BlobWant>,
) -> Result<Vec<BlobWant>, String> {
    let grant = match access {
        Access::Full | Access::Legacy => return Ok(wants),
        Access::Nothing => return Ok(Vec::new()),
        Access::Worker(grant) => grant,
    };
    if wants.is_empty() {
        return Ok(wants);
    }
    let wanted: BTreeSet<&str> = wants.iter().map(|want| want.hash.as_str()).collect();
    let runs: Vec<String> = {
        let connection = inner.store.connect()?;
        let mut statement = connection
            .prepare("SELECT DISTINCT run_id FROM log WHERE thread_id=?1 AND run_id IS NOT NULL")
            .map_err(sql)?;
        let mut runs = Vec::new();
        for thread in &grant.threads {
            let rows = statement
                .query_map([thread], |row| row.get::<_, String>(0))
                .map_err(sql)?;
            for run in rows {
                runs.push(run.map_err(sql)?);
            }
        }
        runs
    };
    let mut found: BTreeSet<String> = BTreeSet::new();
    let mut storage = inner
        .storage
        .lock()
        .map_err(|_| "The journal is unavailable.".to_string())?;
    for run in runs {
        let events = storage
            .journal
            .events(&run)
            .map_err(|error| format!("The journal failed: {error}"))?;
        for event in events {
            let hash = match &event.payload {
                EventPayload::Cas { payload_cas } => payload_cas.sha256.clone(),
                EventPayload::Attachment { attachment } => attachment.sha256().as_str().to_owned(),
                EventPayload::Inline { .. } => continue,
            };
            if wanted.contains(hash.as_str()) {
                found.insert(hash);
            }
        }
        if found.len() == wanted.len() {
            break;
        }
    }
    Ok(wants
        .into_iter()
        .filter(|want| found.contains(&want.hash))
        .collect())
}

/// Takes out of an exchange this runtime sends what the peer's role may
/// not hold, and when this runtime is a leaf, what it did not write.
pub(super) fn restrict_outgoing(
    inner: &Inner,
    access: &Access,
    leaf: Option<&Grant>,
    exchange: &mut Exchange,
) -> Result<(), String> {
    match access {
        Access::Full | Access::Legacy => {}
        Access::Nothing => {
            let roster = std::mem::take(&mut exchange.roster);
            let roster_digest = exchange.roster_digest.take();
            let admission = exchange.admission.take();
            *exchange = Exchange {
                from: exchange.from.clone(),
                roster,
                roster_digest,
                admission,
                ..Exchange::default()
            };
            return Ok(());
        }
        Access::Worker(grant) => {
            for entry in &mut exchange.directory {
                if !grant.agents.contains(&entry.agent_id) {
                    entry.definition = None;
                    entry.activity = None;
                }
            }
            exchange.groups.clear();
            exchange
                .threads
                .retain(|registration| grant.threads.contains(&registration.thread_id));
            exchange
                .clocks
                .retain(|thread, _| grant.threads.contains(thread));
            if let Some(watermarks) = &mut exchange.watermarks {
                // A digest covers threads a worker never holds.
                watermarks.digest = None;
            }
            exchange
                .events
                .retain(|event| grant.threads.contains(&event.thread_id));
            exchange.records.retain(|row| grant.holds_row(row));
            exchange
                .messages
                .retain(|message| message.to_runtime.as_deref() == Some(&grant.runtime_id));
            let mut acks = Vec::new();
            for ack in std::mem::take(&mut exchange.acks) {
                if acknowledges_worker(inner, &grant.runtime_id, &ack)? {
                    acks.push(ack);
                }
            }
            exchange.acks = acks;
            exchange.notices.retain(|notice| {
                notice.body["agent_id"]
                    .as_str()
                    .is_some_and(|agent| grant.agents.contains(agent))
            });
            exchange
                .approvals
                .retain(|approval| grant.agents.contains(&approval.agent_id));
            exchange.sealed.retain(|secret| access.holds_sealed(secret));
        }
    }
    let Some(own) = leaf else {
        return Ok(());
    };
    let own_id = own.runtime_id.as_str();
    exchange.members.clear();
    exchange
        .directory
        .retain(|entry| entry.primary_runtime == own_id);
    exchange.groups.clear();
    exchange
        .threads
        .retain(|registration| own.threads.contains(&registration.thread_id));
    exchange.records.retain(|row| row.origin == own_id);
    exchange.events.retain(|event| event.origin == own_id);
    exchange
        .messages
        .retain(|message| message.from_runtime == own_id);
    let mut acks = Vec::new();
    for ack in std::mem::take(&mut exchange.acks) {
        if delivered_here(inner, &ack.message_id)? {
            acks.push(ack);
        }
    }
    exchange.acks = acks;
    exchange.notices.retain(|notice| notice.origin == own_id);
    exchange
        .approvals
        .retain(|approval| approval.runtime_id == own_id);
    exchange.roster.retain(|entry| entry.signer == own_id);
    exchange.sealed.clear();
    Ok(())
}

/// Keeps from the fleet fields a worker sent what it wrote itself: the
/// roster entries it signed, and no sealed secret. A leaf keeps only the
/// secrets sealed to it.
pub(super) fn restrict_fleet_incoming(
    inner: &Inner,
    peer: &str,
    roster: &mut Vec<super::RosterEntry>,
    sealed: &mut Vec<SealedSecret>,
) -> Result<(), String> {
    if fleet::keys(inner).is_err() {
        return Ok(());
    }
    let view = fleet::view(inner)?;
    if view.role(peer) == Some(Role::Worker) {
        roster.retain(|entry| entry.signer == peer);
        sealed.clear();
    }
    // A leaf keeps only the secrets sealed to it, whatever a peer that has
    // not learned its role yet sends.
    if view.role(&view.self_id) == Some(Role::Worker) {
        sealed.retain(|secret| secret.recipients.contains_key(&view.self_id));
    }
    Ok(())
}

/// Takes out of an exchange `peer` sent what its role may not write or
/// pass on. Answers whether to take in its member records.
pub(super) fn restrict_incoming(
    inner: &Inner,
    peer: &str,
    access: &Access,
    exchange: &mut Exchange,
) -> Result<bool, String> {
    match access {
        Access::Full | Access::Legacy => Ok(true),
        Access::Nothing => {
            *exchange = Exchange {
                from: exchange.from.clone(),
                ..Exchange::default()
            };
            Ok(false)
        }
        Access::Worker(grant) => {
            let grant = grant.clone().with_writes(inner, &exchange.records);
            exchange.members.clear();
            // A worker speaks `store/1`; the full-state fields of an older
            // member carry nothing it may write.
            exchange.directory.clear();
            exchange.groups.clear();
            exchange.threads.clear();
            exchange.records.retain(|row| grant.writes_row(inner, row));
            exchange
                .clocks
                .retain(|thread, _| grant.threads.contains(thread));
            exchange
                .events
                .retain(|event| event.origin == peer && grant.threads.contains(&event.thread_id));
            exchange
                .messages
                .retain(|message| message.from_runtime == peer);
            let mut acks = Vec::new();
            for ack in std::mem::take(&mut exchange.acks) {
                let addressed_to_worker = messaging::outbox_message(inner, &ack.message_id)?
                    .is_some_and(|message| message.to_runtime.as_deref() == Some(peer));
                if addressed_to_worker {
                    acks.push(ack);
                }
            }
            exchange.acks = acks;
            exchange.notices.retain(|notice| notice.origin == peer);
            exchange.approvals.retain(|approval| {
                approval.runtime_id == peer && grant.agents.contains(&approval.agent_id)
            });
            exchange.sealed.clear();
            exchange.record_floor = None;
            Ok(true)
        }
    }
}

/// Keeps from an exchange that reaches a leaf only what it may hold, as a
/// peer that has not learned of its new role yet may send more.
pub(super) fn keep_for_leaf(leaf: Option<&Grant>, exchange: &mut Exchange) {
    let Some(own) = leaf else {
        return;
    };
    exchange
        .records
        .retain(|row| row.origin == own.runtime_id || own.holds_row(row));
    exchange
        .events
        .retain(|event| own.threads.contains(&event.thread_id));
    exchange
        .clocks
        .retain(|thread, _| own.threads.contains(thread));
    exchange
        .approvals
        .retain(|approval| own.agents.contains(&approval.agent_id));
}

/// Whether an acknowledgement answers a message the worker `worker` sent:
/// one this runtime relays or ran for it. An agent message this runtime
/// took names no sender here, so its acknowledgement goes when it carries
/// no answer of an action.
fn acknowledges_worker(inner: &Inner, worker: &str, ack: &super::Ack) -> Result<bool, String> {
    if let Some(message) = messaging::outbox_message(inner, &ack.message_id)? {
        return Ok(message.from_runtime == worker);
    }
    let forwarded: Option<super::Message> =
        inner
            .store
            .record("op_inbox", "op_id", "message", &ack.message_id)?;
    if let Some(message) = forwarded {
        return Ok(message.from_runtime == worker);
    }
    Ok(ack.result.is_none() && delivered_here(inner, &ack.message_id)?)
}

/// Whether this runtime acknowledged `message_id` itself, as its target.
fn delivered_here(inner: &Inner, message_id: &str) -> Result<bool, String> {
    Ok(inner
        .store
        .connect()?
        .query_row(
            "SELECT 1 FROM inbox WHERE message_id=?1",
            [message_id],
            |_| Ok(()),
        )
        .optional()
        .map_err(sql)?
        .is_some())
}

/// Brings this runtime in line with a new role of its own. Every peer's
/// sync starts over, and peers send again every record the role holds. A
/// worker drops the records and thread copies it may no longer hold.
pub(super) fn change_own_role(inner: &Inner, role: Role) -> Result<(), String> {
    let own = inner.origin()?;
    inner.peers_reset();
    if role == Role::Worker {
        let grant = Grant::of(inner, &own)?;
        inner
            .records
            .drop_where(&own, &|row| grant.holds_row(row))?;
        for registration in sync::registrations(inner)? {
            let owner = sync::thread_primary(inner, &registration).unwrap_or_default();
            if owner != own && !grant.threads.contains(&registration.thread_id) {
                sync::drop_replica(inner, &registration.thread_id, &owner)?;
            }
        }
    }
    inner.records.forget_numbers(&own)?;
    inner.changed("members");
    inner.nudge();
    Ok(())
}
