//! The values members trade in `swarm.exchange` and answer in the swarm
//! operations. Every field a later version adds reads with a default, so a
//! newer member can link to an older one. A member that names `store/1`
//! trades records and thread clock deltas; an older one gets the full-state
//! `directory`, `groups`, `threads` and `clocks`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One member as it describes itself. The member rewrites its own record on
/// every exchange, so `heartbeat_at` says when it was last heard of.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberRecord {
    pub runtime_id: String,
    pub runtime_name: String,
    #[serde(default)]
    pub form_factor: Option<String>,
    pub heartbeat_at: String,
    /// When a client last used the member.
    #[serde(default)]
    pub last_client_at: Option<String>,
    /// The members it links to directly.
    #[serde(default)]
    pub links: Vec<String>,
    pub updated_at: String,
    /// What the member's exchanges speak beyond the full-state fields, such
    /// as `store/1` for records and thread clock deltas.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    /// How many days of ordinary threads the member keeps offline, `0` for
    /// all of them. Absent means the default, [`super::OFFLINE_DAYS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offline_days: Option<u32>,
    /// The ordinary threads the member asked to hold in full whatever their
    /// age, newest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pins: Vec<String>,
    /// The member's record version vector, so every member knows which
    /// writes all others hold before it drops a tombstone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_vv: Option<VersionVector>,
    /// The member's fleet key, signed with its identity key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fleet: Option<FleetCard>,
    /// The `host:port` endpoints the member's listener advertises.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub endpoints: Vec<String>,
}

/// A member's keys as it states them: its P-256 identity key, the SHA-256 of
/// the TLS certificate that key serves, and its X25519 fleet key, signed with
/// the identity key. Members seal secrets to the fleet key the roster names.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetCard {
    /// The uncompressed P-256 public key, base64url.
    pub identity_pub: String,
    /// Lowercase hex.
    pub cert_sha256: String,
    /// The X25519 public key, base64url.
    pub fleet_pub: String,
    /// The identity key's signature of the other fields, base64url.
    pub sig: String,
}

/// One signed entry of the fleet roster. An `add` names a member, its card
/// and its role, signed by the member that vouches for it; a `remove` ends a
/// member. An entry counts when a member this runtime trusts signed it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RosterEntry {
    /// The SHA-256 of the signed statement, lowercase hex.
    pub entry_id: String,
    /// `add` or `remove`.
    pub kind: String,
    pub runtime_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<FleetCard>,
    /// `full` or `worker`, on an `add`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub signer: String,
    pub clock: super::hlc::Hlc,
    /// The signer's identity signature of the statement, base64url.
    pub sig: String,
}

/// One version of a secret, sealed to each recipient's fleet key. Relays
/// carry it without opening it. The owner signs it with its identity key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SealedSecret {
    pub secret_id: String,
    pub version: u64,
    pub owner: String,
    /// `models`, `mcp`, `vault` or `handover`.
    pub class: String,
    /// The X25519 public key the owner drew for this version, base64url.
    pub ephemeral: String,
    /// Each recipient runtime's copy.
    #[serde(default)]
    pub recipients: BTreeMap<String, SealedCopy>,
    pub sealed_at: String,
    /// The owner's identity signature of the other fields, base64url.
    pub sig: String,
}

/// One recipient's AES-256-GCM copy of a sealed secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SealedCopy {
    pub nonce: String,
    pub ciphertext: String,
}

/// One agent in the swarm's directory. The entry with the highest epoch,
/// then the latest `updated_at`, wins.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DirectoryEntry {
    pub agent_id: String,
    pub name: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub avatar: Option<Value>,
    pub primary_runtime: String,
    #[serde(default)]
    pub epoch: u64,
    pub updated_at: String,
    #[serde(default)]
    pub deleted: bool,
    /// The agent's primary thread, which every member replicates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_thread: Option<String>,
    /// Whether the primary has been away long enough that this member offers
    /// to take the agent. Client reads only.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub move_suggested: bool,
    /// The agent's definition, which a forced move installs when the old
    /// primary is offline. Exchanges carry it; client reads leave it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<Value>,
    /// What the agent does now, as its primary runtime last saw it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<crate::agent_runs::Activity>,
}

impl DirectoryEntry {
    /// The order the directory keeps: a later move wins, then a later edit.
    pub fn version(&self) -> (u64, &str, &str) {
        (self.epoch, &self.updated_at, &self.primary_runtime)
    }
}

/// A group chat of two to six agents, owned by the runtime that created it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    pub group_id: String,
    pub name: String,
    pub members: Vec<String>,
    pub lead: String,
    #[serde(default)]
    pub meeting_mode: bool,
    #[serde(default = "default_debate_rounds")]
    pub debate_rounds: u32,
    pub owner_runtime: String,
    pub thread_id: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub deleted: bool,
}

pub(super) fn default_debate_rounds() -> u32 {
    super::groups::MAX_DEBATE_ROUNDS
}

/// A thread every member replicates: an agent's thread, a group's, or an
/// ordinary thread its owner created.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadRegistration {
    pub thread_id: String,
    /// `agent`, `group` or `plain`.
    pub kind: String,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub group_id: Option<String>,
    pub updated_at: String,
    /// The runtime that created a `plain` thread and runs it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_runtime: Option<String>,
}

/// What every member knows of an ordinary thread before it holds the
/// thread's history: its owner writes it as the thread changes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadSummaryRecord {
    pub title: String,
    pub owner_runtime: String,
    /// The time of the thread's newest event.
    pub last_at: String,
    /// The newest prompt or posted message, clipped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_post: Option<LastPost>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastPost {
    pub text: String,
    /// `user`, `agent` or `system`.
    pub author: String,
    pub at: String,
}

/// What the sender of a record delta holds while the receiver catches up:
/// its rows per class, so the receiver can show its progress.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotMark {
    /// Rows, tombstones among them, per record class.
    pub classes: BTreeMap<String, u64>,
    /// The delta left nothing out.
    #[serde(default)]
    pub complete: bool,
}

/// Per thread, the highest sequence held from each origin runtime.
pub type Clocks = BTreeMap<String, BTreeMap<String, u64>>;

/// The highest record number held from each origin runtime.
pub type VersionVector = BTreeMap<String, u64>;

/// One replicated record row: a field's value, or its removal, and the write
/// that set it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecordRow {
    pub class: String,
    pub key: String,
    /// The field, `""` for the record's tombstone.
    pub field: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    pub clock: super::hlc::Hlc,
    pub origin: String,
    /// The origin's own number for the write.
    pub seq: u64,
    /// What the class ranks above the clock, such as an agent's epoch.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rank: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deleted: bool,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

/// Where the thread clocks of one exchange stand. The sender numbers each
/// change to a thread's clock; `clocks` holds the threads changed after
/// `base`, up to `mark`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Watermarks {
    /// The sender's version the receiver held, as the receiver last said.
    pub base: u64,
    /// The sender's version these clocks reach.
    pub mark: u64,
    /// No thread changed after `mark`.
    #[serde(default)]
    pub complete: bool,
    /// The receiver's version the sender holds.
    pub ack: u64,
    /// A hash of all the sender's clocks, sent every 10 minutes so a
    /// receiver whose copy drifted asks for them again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

/// One journal event of a replicated thread with its `(origin, seq)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReplicatedEvent {
    pub thread_id: String,
    pub origin: String,
    pub seq: u64,
    /// `run` for a run event, `thread` for a thread event.
    pub kind: String,
    pub envelope: Value,
    /// The prompt of a run's first event, which the journal keeps outside
    /// the event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

/// One message a member delivers to another through the swarm, retried until
/// the target acknowledges it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub message_id: String,
    /// `agent_message`, `handoff`, `user_message`, `group_turn`,
    /// `group_reply`, `approval_answer`, `handover`, `move_request` or `op`.
    pub kind: String,
    pub from_runtime: String,
    #[serde(default)]
    pub to_runtime: Option<String>,
    #[serde(default)]
    pub from_agent: Option<String>,
    #[serde(default)]
    pub to_agent: Option<String>,
    #[serde(default)]
    pub hop: u32,
    pub created_at: String,
    pub body: Value,
}

/// The target's answer to one message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ack {
    pub message_id: String,
    /// `delivered` or `refused`.
    pub status: String,
    #[serde(default)]
    pub note: Option<String>,
    pub at: String,
    /// The owner's answer to a forwarded action it ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
}

/// A change every member shows its clients, such as a run that ended.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Notice {
    pub notice_id: String,
    pub kind: String,
    pub origin: String,
    pub body: Value,
    pub created_at: String,
}

/// One approval an agent run waits at, on the member that runs it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Approval {
    pub approval_id: String,
    pub runtime_id: String,
    pub run_id: String,
    pub gate_id: String,
    pub agent_id: String,
    pub agent_name: String,
    #[serde(default)]
    pub thread_id: Option<String>,
    pub request: Value,
    /// The exact `{request_kind, resource}` an agent gate asks about, as
    /// the agent profile's `always_allow` rules name it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<crate::permission_gate::PolicyRequest>,
    /// A short human summary of the operation and its inputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Why the gate asks, such as the auto reviewer's reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The plan a plan gate asks to approve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    /// The answer ids `agent.approval_answer` takes for this gate, such as
    /// `allow_once` and `always_allow_agent`, or `plan_edit_auto` for a plan.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub answers: Vec<String>,
    /// `pending`, `answered` or `closed`.
    pub state: String,
    pub created_at: String,
    #[serde(default)]
    pub answered_at: Option<String>,
    #[serde(default)]
    pub answered_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer_id: Option<String>,
}

/// A request for the bytes of one content-store object from `offset`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobWant {
    pub hash: String,
    #[serde(default)]
    pub offset: u64,
}

/// One slice of a content-store object, base64 in `data`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobChunk {
    pub hash: String,
    pub offset: u64,
    pub total: u64,
    pub data: String,
}

/// The body of `swarm.exchange`, which a request and its response share.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Exchange {
    pub from: MemberRecord,
    #[serde(default)]
    pub members: Vec<MemberRecord>,
    #[serde(default)]
    pub directory: Vec<DirectoryEntry>,
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(default)]
    pub threads: Vec<ThreadRegistration>,
    #[serde(default)]
    pub clocks: Clocks,
    #[serde(default)]
    pub events: Vec<ReplicatedEvent>,
    #[serde(default)]
    pub messages: Vec<Message>,
    #[serde(default)]
    pub acks: Vec<Ack>,
    #[serde(default)]
    pub notices: Vec<Notice>,
    #[serde(default)]
    pub approvals: Vec<Approval>,
    #[serde(default)]
    pub wants: Vec<BlobWant>,
    #[serde(default)]
    pub blobs: Vec<BlobChunk>,
    #[serde(default)]
    pub client_seen_at: Option<String>,
    #[serde(default)]
    pub resumed_at: Option<String>,
    /// The sender left items out to keep the frame bounded.
    #[serde(default)]
    pub more: bool,
    /// The record rows the receiver lacks (`store/1`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub records: Vec<RecordRow>,
    /// The sender's record version vector (`store/1`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_vv: Option<VersionVector>,
    /// Present when `clocks` holds only the threads changed since the
    /// receiver's watermark (`store/1`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watermarks: Option<Watermarks>,
    /// Present while the receiver catches up on records (`store/1`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<SnapshotMark>,
    /// The roster entries, when the receiver's roster digest differs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roster: Vec<RosterEntry>,
    /// A hash of the sender's roster entry ids.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roster_digest: Option<String>,
    /// The sealed secrets the receiver lacks, sent only to a roster member.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sealed: Vec<SealedSecret>,
    /// The version of each sealed secret the sender holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sealed_have: Option<BTreeMap<String, u64>>,
    /// The hash of the invitation secret a joining member redeemed, which
    /// the inviter answers with the joiner's roster entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admission: Option<String>,
}
