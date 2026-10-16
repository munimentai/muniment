//! The values members trade in `swarm.exchange` and answer in the swarm
//! operations. Every field a later version adds reads with a default, so a
//! newer member can link to an older one.

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
    /// The agent's definition, which a forced move installs when the old
    /// primary is offline. Exchanges carry it; client reads leave it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<Value>,
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

/// A thread every member replicates: an agent's thread or a group's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadRegistration {
    pub thread_id: String,
    /// `agent` or `group`.
    pub kind: String,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub group_id: Option<String>,
    pub updated_at: String,
}

/// Per thread, the highest sequence held from each origin runtime.
pub type Clocks = BTreeMap<String, BTreeMap<String, u64>>;

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
    /// `group_reply`, `approval_answer`, `handover` or `move_request`.
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
}
