//! Actions forwarded to the member that owns their item.
//!
//! A client that acts on an agent, thread, run or approval another member
//! owns gets `{forwarded: true, op_id}` at once. The action travels as an
//! `op` message through the durable outbox, retried and relayed until the
//! owner acknowledges it. The owner checks its allowlist and the actor's
//! scopes, runs the same operation its own clients reach, and puts the
//! answer in the acknowledgement. The forwarding member reports each change
//! as `fleet.op_updated`: `queued`, `sent`, `applied` or `failed`.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use muniment_attach::{EventName, Operation, SCOPE_SWARM_PEER};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::store::sql;
use super::{approvals, directory, link, lock, messaging, parse_time, sync, Ack, Inner, Message};
use crate::attach::RemoteRule;

/// The message kind of a forwarded action.
pub(super) const KIND: &str = "op";
/// The capability a member names when it runs forwarded actions.
pub const OPS_CAPABILITY: &str = "ops/1";
/// How long a forwarded action waits for its owner.
pub const OP_DAYS: i64 = 7;
/// How long an agent's primary is away before other members offer to move
/// the agent to them.
pub const MOVE_SUGGEST_HOURS: i64 = 24;
/// The most bytes of an owner's answer an acknowledgement carries, since
/// acknowledgements travel to every member.
const MAX_RESULT_BYTES: usize = 2 * 1024;
/// The fields an answer too large to carry keeps.
const RESULT_KEYS: [&str; 9] = [
    "id",
    "approval_id",
    "run_id",
    "thread_id",
    "message_id",
    "status",
    "state",
    "name",
    "answered_by",
];

/// The operations a member forwards to the owner of their item, and the
/// only ones an owner runs for another member. The phone's actions are all
/// here: posts, approval answers, agent runs, stop and steer. A
/// `thread.create` that names another member's `runtime_id` starts the
/// thread there.
pub const FORWARDABLE: [Operation; 14] = [
    Operation::ThreadCreate,
    Operation::ThreadPost,
    Operation::RunSteer,
    Operation::RunFollowUp,
    Operation::RunCancel,
    Operation::ThreadPermissionModeSet,
    Operation::ThreadPlanAnswer,
    Operation::AgentApprovalAnswer,
    Operation::AgentRunNow,
    Operation::AgentSave,
    Operation::RoutineSave,
    Operation::RoutineDelete,
    Operation::AgentRoutineProposalAccept,
    Operation::AgentRoutineProposalDismiss,
];

pub fn is_forwardable(operation: Operation) -> bool {
    FORWARDABLE.contains(&operation)
}

/// Who asked for an action: the device and the scopes its pairing grants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FleetActor {
    pub device: String,
    pub scopes: BTreeSet<String>,
}

impl FleetActor {
    pub fn new(device: impl Into<String>, scopes: BTreeSet<String>) -> Self {
        Self {
            device: device.into(),
            scopes,
        }
    }

    /// The runtime's owner on its own machine, who holds every scope.
    pub fn owner() -> Self {
        Self::new("local", full_scopes())
    }

    /// The paired phone, which reads and acts on runs.
    pub fn phone() -> Self {
        Self::new(
            "phone",
            [
                muniment_attach::SCOPE_THREAD_READ,
                muniment_attach::SCOPE_RUN_WRITE,
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        )
    }
}

/// Every scope a client can hold. A linked runtime's own scope is not one.
fn full_scopes() -> BTreeSet<String> {
    muniment_attach::REMOTE_SCOPES
        .iter()
        .filter(|scope| **scope != SCOPE_SWARM_PEER)
        .map(|scope| (*scope).to_owned())
        .collect()
}

/// The scopes the member `runtime_id` may pass on for its clients, by its
/// role in the roster: a full member every scope, a worker every scope but
/// `agents.manage` and `runtime.admin`, and a member the roster does not
/// name or removed none. Without fleet keys every member holds the full set.
fn member_scopes(inner: &Inner, runtime_id: &str) -> BTreeSet<String> {
    if super::fleet::keys(inner).is_err() {
        return full_scopes();
    }
    let Ok(view) = super::fleet::view(inner) else {
        return BTreeSet::new();
    };
    if view.removed.contains(runtime_id) {
        return BTreeSet::new();
    }
    match view.role(runtime_id) {
        Some(super::fleet::Role::Full) => full_scopes(),
        Some(super::fleet::Role::Worker) => full_scopes()
            .into_iter()
            .filter(|scope| {
                scope != muniment_attach::SCOPE_AGENTS_MANAGE
                    && scope != muniment_attach::SCOPE_RUNTIME_ADMIN
            })
            .collect(),
        None => BTreeSet::new(),
    }
}

/// Why this runtime may not send an action at all: a worker originates no
/// action that takes a scope its role lacks.
fn own_refusal(inner: &Inner, own: &str, operation: Operation, body: &Value) -> Option<String> {
    let scopes = member_scopes(inner, own);
    match crate::attach::remote_rule(operation, body) {
        RemoteRule::Scope(scope) if !muniment_attach::scopes_grant(&scopes, scope) => Some(format!(
            "This machine is a worker in the fleet, so it cannot send actions that need the {scope} scope."
        )),
        _ => None,
    }
}

/// What an actor may do through a member: the actor's scopes that the
/// member's role also holds.
pub(super) fn effective_scopes(
    actor: &BTreeSet<String>,
    member: &BTreeSet<String>,
) -> BTreeSet<String> {
    actor.intersection(member).cloned().collect()
}

/// The item an action acts on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpItem {
    /// `thread`, `agent`, `run` or `approval`.
    pub kind: String,
    pub id: String,
}

/// Why an action still waits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpReason {
    /// `owner_offline`, `waiting` or `expired`.
    pub code: String,
    pub text: String,
    #[serde(default)]
    pub last_seen_at: Option<String>,
}

/// One forwarded action as `fleet.op_status` and `fleet.op_updated` report
/// it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Op {
    pub op_id: String,
    pub operation: String,
    pub item: OpItem,
    pub owner_runtime: String,
    #[serde(default)]
    pub owner_name: Option<String>,
    /// `queued`, `sent`, `applied`, `failed` or `cancelled`.
    pub state: String,
    #[serde(default)]
    pub reason: Option<OpReason>,
    /// The owner's answer, once applied.
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub expires_at: String,
}

impl Op {
    fn open(&self) -> bool {
        matches!(self.state.as_str(), "queued" | "sent")
    }
}

fn text(body: &Value, key: &str) -> Option<String> {
    body.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn item(kind: &str, id: Option<String>) -> Option<OpItem> {
    id.map(|id| OpItem {
        kind: kind.into(),
        id,
    })
}

/// The item a forwardable operation names in its body.
fn item_of(operation: Operation, body: &Value) -> Option<OpItem> {
    match operation {
        Operation::ThreadCreate => item("runtime", text(body, "runtime_id")),
        Operation::ThreadPost => item("thread", text(body, "thread_id"))
            .or_else(|| item("agent", text(body, "agent_id"))),
        Operation::RunSteer | Operation::RunFollowUp | Operation::RunCancel => {
            item("run", text(body, "run_id"))
        }
        Operation::ThreadPermissionModeSet | Operation::ThreadPlanAnswer => {
            item("thread", text(body, "thread_id"))
        }
        Operation::AgentApprovalAnswer => item("approval", text(body, "approval_id")),
        Operation::AgentRunNow => item("agent", text(body, "id")),
        Operation::AgentSave => item(
            "agent",
            body.get("agent").and_then(|agent| text(agent, "id")),
        ),
        Operation::RoutineSave
        | Operation::RoutineDelete
        | Operation::AgentRoutineProposalAccept
        | Operation::AgentRoutineProposalDismiss => item("agent", text(body, "agent_id")),
        _ => None,
    }
}

/// The member that owns an item, when this runtime knows it.
fn owner_of(inner: &Inner, item: &OpItem) -> Option<String> {
    match item.kind.as_str() {
        "thread" => {
            let registration = sync::registration(inner, &item.id)?;
            sync::thread_primary(inner, &registration)
        }
        "agent" => directory::primary_of(inner, &item.id),
        "run" => {
            let (thread, origin): (String, String) = inner
                .store
                .connect()
                .ok()?
                .query_row(
                    "SELECT thread_id, origin FROM runs WHERE run_id=?1",
                    [&item.id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .ok()??;
            Some(origin)
                .filter(|origin| !origin.is_empty())
                .or_else(|| {
                    let registration = sync::registration(inner, &thread)?;
                    sync::thread_primary(inner, &registration)
                })
        }
        "approval" => approvals::get(inner, &item.id)
            .ok()
            .flatten()
            .map(|approval| approval.runtime_id),
        "runtime" => Some(item.id.clone()),
        _ => None,
    }
}

fn member(inner: &Inner, runtime_id: &str) -> Option<super::MemberRecord> {
    inner
        .store
        .record("members", "runtime_id", "record", runtime_id)
        .ok()
        .flatten()
}

/// Whether a member runs forwarded actions. A member this runtime never
/// heard from is taken to.
fn speaks_ops(inner: &Inner, runtime_id: &str) -> bool {
    member(inner, runtime_id).is_none_or(|record| {
        record
            .capabilities
            .iter()
            .any(|capability| capability == OPS_CAPABILITY)
    })
}

fn member_name(inner: &Inner, runtime_id: &str) -> Option<String> {
    member(inner, runtime_id)
        .map(|record| record.runtime_name)
        .filter(|name| !name.is_empty())
}

/// When a member was last heard from.
pub(super) fn last_seen(inner: &Inner, runtime_id: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    member(inner, runtime_id).and_then(|record| parse_time(&record.heartbeat_at))
}

/// Whether an agent's primary is another member that has been away past
/// [`MOVE_SUGGEST_HOURS`], so this member offers to take the agent.
pub(super) fn move_suggested(inner: &Inner, primary_runtime: &str) -> bool {
    if inner.runtime_id().as_deref() == Some(primary_runtime) {
        return false;
    }
    last_seen(inner, primary_runtime)
        .is_some_and(|seen| (inner.now() - seen).num_hours() >= MOVE_SUGGEST_HOURS)
}

/// Where a client's call goes. `None` runs it here; otherwise the action
/// was forwarded to the member that owns its item and the answer says so.
pub(super) fn route(
    inner: &Inner,
    operation: Operation,
    body: &Value,
    actor: &FleetActor,
    idempotency_key: Option<&str>,
) -> Result<Option<Value>, String> {
    if !is_forwardable(operation) {
        return Ok(None);
    }
    let Some(item) = item_of(operation, body) else {
        return Ok(None);
    };
    let Some(own) = inner.runtime_id() else {
        // A thread meant for a named machine never starts here in its place.
        if item.kind == "runtime" {
            return Err(
                "This machine is not in a fleet yet, so it cannot start a thread on another machine."
                    .into(),
            );
        }
        return Ok(None);
    };
    let Some(owner) = owner_of(inner, &item) else {
        return Ok(None);
    };
    if owner == own {
        return Ok(None);
    }
    if item.kind == "runtime" && !link::known_member(inner, &owner) {
        return Err("The fleet has no such machine.".into());
    }
    if let Some(refusal) = own_refusal(inner, &own, operation, body) {
        return Err(refusal);
    }
    // A new thread on another member takes its id here, so the answer
    // names it at once.
    let mut body = body.clone();
    if operation == Operation::ThreadCreate && text(&body, "thread_id").is_none() {
        body["thread_id"] = Value::String(uuid::Uuid::now_v7().to_string());
    }
    let mut body = &body;
    if !speaks_ops(inner, &owner) {
        // An older owner takes posts and approval answers the way it always
        // did, through replicated posts and answer messages.
        if matches!(
            operation,
            Operation::ThreadPost | Operation::AgentApprovalAnswer
        ) {
            return Ok(None);
        }
        let name = member_name(inner, &owner).unwrap_or_else(|| "The machine that runs it".into());
        return Err(format!(
            "{name} runs an older Muniment. Update it to act on its items from here."
        ));
    }
    let op = forward(inner, operation, body, actor, idempotency_key, item, &owner)?;
    // A repeat of the key answers the op it queued first, with that body.
    let queued = messaging::outbox_message(inner, &op.op_id)?.map(|message| message.body);
    if let Some(queued) = queued.as_ref().and_then(|queued| queued.get("body")) {
        body = queued;
    }
    if operation == Operation::AgentApprovalAnswer {
        return answer_after_wait(inner, op).map(Some);
    }
    Ok(Some(placeholder(inner, &op, operation, body)))
}

/// The answer a client gets at once for a forwarded action: the keys its
/// operation's answer always has, and `forwarded` with the op's id.
fn placeholder(inner: &Inner, op: &Op, operation: Operation, body: &Value) -> Value {
    let mut answer = match operation {
        Operation::ThreadCreate => json!({
            "thread_id": body["thread_id"],
            "run_id": null,
        }),
        Operation::ThreadPost => json!({
            "message_id": op.op_id,
            "thread_id": body.get("thread_id").cloned().unwrap_or(Value::Null),
            "origin_runtime": inner.runtime_id(),
            "seq": null,
        }),
        Operation::RunSteer | Operation::RunFollowUp | Operation::RunCancel => json!({
            "run_id": body["run_id"],
            "accepted_at": inner.now_text(),
        }),
        Operation::AgentRunNow => json!({"run": null}),
        Operation::AgentSave => json!({"agent": null}),
        Operation::RoutineSave | Operation::AgentRoutineProposalAccept => {
            json!({"routine": null})
        }
        Operation::ThreadPermissionModeSet => json!({
            "thread_id": body["thread_id"],
            "mode": body["mode"],
            "source": null,
        }),
        Operation::ThreadPlanAnswer => json!({"status": "sent", "answer": body["answer"]}),
        _ => json!({}),
    };
    answer["forwarded"] = Value::Bool(true);
    answer["op_id"] = Value::String(op.op_id.clone());
    answer["state"] = Value::String(op.state.clone());
    answer
}

/// Waits briefly for an approval answer's owner, so the client learns which
/// answer won, as an answer given on the owner does.
fn answer_after_wait(inner: &Inner, op: Op) -> Result<Value, String> {
    let deadline = Instant::now() + approvals::REMOTE_ANSWER_WAIT;
    let mut current = op;
    while current.open() && Instant::now() < deadline {
        inner.nudge();
        std::thread::sleep(Duration::from_millis(50));
        current = get(inner, &current.op_id)?.unwrap_or(current);
    }
    let approval_id = current.item.id.clone();
    let mut answer = match current.state.as_str() {
        "applied" => {
            let mut answer = current.result.clone().unwrap_or_else(|| json!({}));
            // The owner's record replicates in full; the acknowledgement may
            // carry only part of it.
            let replicated = wait_for_approval(inner, &approval_id, deadline)?;
            if let Some(approval) = replicated {
                answer["approval"] = serde_json::to_value(approval).unwrap_or(Value::Null);
            }
            answer
        }
        "failed" => {
            return Err(current
                .error
                .clone()
                .unwrap_or_else(|| "The answer was refused.".into()))
        }
        _ => json!({
            "status": "sent",
            "approval": approvals::get(inner, &approval_id)?,
        }),
    };
    answer["forwarded"] = Value::Bool(true);
    answer["op_id"] = Value::String(current.op_id.clone());
    answer["state"] = Value::String(current.state.clone());
    Ok(answer)
}

/// The replicated approval once it is no longer pending, or as it stands at
/// `deadline`.
fn wait_for_approval(
    inner: &Inner,
    approval_id: &str,
    deadline: Instant,
) -> Result<Option<super::Approval>, String> {
    loop {
        let approval = approvals::get(inner, approval_id)?;
        if approval
            .as_ref()
            .is_none_or(|approval| approval.state != "pending")
            || Instant::now() >= deadline
        {
            return Ok(approval);
        }
        inner.nudge();
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn expires_at(created_at: &str) -> String {
    parse_time(created_at)
        .map(|created| super::timestamp(created + chrono::Duration::days(OP_DAYS)))
        .unwrap_or_default()
}

/// Queues an action for its owner. A repeat of the same actor's
/// idempotency key answers the op it already queued.
fn forward(
    inner: &Inner,
    operation: Operation,
    body: &Value,
    actor: &FleetActor,
    idempotency_key: Option<&str>,
    item: OpItem,
    owner: &str,
) -> Result<Op, String> {
    let own = inner
        .runtime_id()
        .ok_or("Turn on remote access to act on another machine's items.")?;
    let key = idempotency_key
        .map(str::to_owned)
        .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
    let op = {
        let _merge = lock(&inner.merge);
        let existing: Option<String> = inner
            .store
            .connect()?
            .query_row(
                "SELECT op_id FROM ops WHERE actor_device=?1 AND idempotency_key=?2 AND operation=?3",
                params![actor.device, key, operation.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql)?;
        if let Some(op) = existing
            .map(|op_id| get(inner, &op_id))
            .transpose()?
            .flatten()
        {
            return Ok(op);
        }
        let op_id = uuid::Uuid::now_v7().to_string();
        let now = inner.now_text();
        let message = Message {
            message_id: op_id.clone(),
            kind: KIND.into(),
            from_runtime: own,
            to_runtime: Some(owner.to_owned()),
            from_agent: None,
            to_agent: None,
            hop: 0,
            created_at: now.clone(),
            body: json!({
                "operation": operation.as_str(),
                "body": body,
                "actor_device": actor.device,
                "actor_scopes": actor.scopes,
                "idempotency_key": key,
            }),
        };
        let op = Op {
            op_id: op_id.clone(),
            operation: operation.as_str().into(),
            item,
            owner_runtime: owner.to_owned(),
            owner_name: member_name(inner, owner),
            state: "queued".into(),
            reason: None,
            result: None,
            error: None,
            expires_at: expires_at(&now),
            created_at: now.clone(),
            updated_at: now,
        };
        inner
            .store
            .connect()?
            .execute(
                "INSERT INTO ops(op_id, record, actor_device, idempotency_key, operation, open, handed, created_at) \
                 VALUES(?1, ?2, ?3, ?4, ?5, 1, 0, ?6)",
                params![
                    op.op_id,
                    serde_json::to_string(&op).map_err(|error| error.to_string())?,
                    actor.device,
                    key,
                    op.operation,
                    op.created_at,
                ],
            )
            .map_err(sql)?;
        messaging::enqueue(inner, &message, false)?;
        op
    };
    let op = settle(inner, op, false, true)?;
    inner.nudge();
    Ok(op)
}

pub(super) fn get(inner: &Inner, op_id: &str) -> Result<Option<Op>, String> {
    inner.store.record("ops", "op_id", "record", op_id)
}

/// The state an open op is in now: queued while its owner is offline or no
/// peer took it yet, then sent.
fn evaluate(inner: &Inner, op: &Op, handed: bool) -> (String, Option<OpReason>) {
    let name = op
        .owner_name
        .clone()
        .or_else(|| member_name(inner, &op.owner_runtime))
        .unwrap_or_else(|| "The machine that runs it".into());
    if !link::online(inner, &op.owner_runtime) {
        let seen = last_seen(inner, &op.owner_runtime).map(super::timestamp);
        let text = match &seen {
            Some(seen) => format!("{name} is offline. It was last seen at {seen}."),
            None => format!("{name} is offline."),
        };
        return (
            "queued".into(),
            Some(OpReason {
                code: "owner_offline".into(),
                text,
                last_seen_at: seen,
            }),
        );
    }
    if handed {
        return ("sent".into(), None);
    }
    (
        "queued".into(),
        Some(OpReason {
            code: "waiting".into(),
            text: format!("Waiting to reach {name}."),
            last_seen_at: None,
        }),
    )
}

/// Saves an op's new state and tells this runtime's clients when it changed.
fn store(inner: &Inner, op: &Op) -> Result<(), String> {
    inner
        .store
        .connect()?
        .execute(
            "UPDATE ops SET record=?2, open=?3 WHERE op_id=?1",
            params![
                op.op_id,
                serde_json::to_string(op).map_err(|error| error.to_string())?,
                op.open(),
            ],
        )
        .map(drop)
        .map_err(sql)
}

/// Re-evaluates an open op and reports a change. `force` reports it anyway.
fn settle(inner: &Inner, mut op: Op, handed: bool, force: bool) -> Result<Op, String> {
    if !op.open() {
        return Ok(op);
    }
    let (state, reason) = evaluate(inner, &op, handed);
    if !force && state == op.state && reason == op.reason {
        return Ok(op);
    }
    op.state = state;
    op.reason = reason;
    op.updated_at = inner.now_text();
    store(inner, &op)?;
    emit(inner, &op);
    Ok(op)
}

fn emit(inner: &Inner, op: &Op) {
    inner.emit(
        EventName::FleetOpUpdated,
        serde_json::to_value(op).unwrap_or(Value::Null),
    );
}

/// Marks the ops this runtime sent that an exchange carried to a peer.
pub(super) fn note_handed(inner: &Inner, messages: &[Message]) {
    let own = inner.runtime_id();
    for message in messages {
        if message.kind != KIND || Some(&message.from_runtime) != own.as_ref() {
            continue;
        }
        let changed = inner.store.connect().and_then(|connection| {
            connection
                .execute(
                    "UPDATE ops SET handed=1 WHERE op_id=?1 AND handed=0",
                    [&message.message_id],
                )
                .map_err(sql)
        });
        if changed.is_ok_and(|changed| changed > 0) {
            let settled = get(inner, &message.message_id)
                .and_then(|op| op.map(|op| settle(inner, op, true, false)).transpose());
            if let Err(error) = settled {
                crate::runtime_eprintln!("muniment-runtime: fleet op state: {error}");
            }
        }
    }
}

/// Takes in the owner's acknowledgement of an op this runtime sent.
pub(super) fn on_ack(inner: &Inner, ack: &Ack) -> Result<(), String> {
    let Some(mut op) = get(inner, &ack.message_id)? else {
        return Ok(());
    };
    // A cancelled op whose relayed copy still reached its owner reports
    // what the owner did with it.
    if !op.open() && op.state != CANCELLED {
        return Ok(());
    }
    if ack.status == "refused" {
        op.state = "failed".into();
        op.error = Some(
            ack.note
                .clone()
                .unwrap_or_else(|| "The machine that runs it refused the action.".into()),
        );
    } else {
        op.state = "applied".into();
        op.result = ack.result.clone();
    }
    op.reason = None;
    op.updated_at = inner.now_text();
    store(inner, &op)?;
    emit(inner, &op);
    Ok(())
}

/// Re-evaluates every open op, and fails those that waited past
/// [`OP_DAYS`], which stop travelling.
pub(super) fn refresh(inner: &Inner) -> Result<(), String> {
    let open: Vec<(String, bool)> = {
        let connection = inner.store.connect()?;
        let mut statement = connection
            .prepare("SELECT op_id, handed FROM ops WHERE open=1 ORDER BY created_at")
            .map_err(sql)?;
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(sql)?;
        rows.collect::<Result<_, _>>().map_err(sql)?
    };
    let now = inner.now();
    for (op_id, handed) in open {
        let Some(mut op) = get(inner, &op_id)? else {
            continue;
        };
        let expired =
            parse_time(&op.created_at).is_some_and(|created| (now - created).num_days() >= OP_DAYS);
        if !expired {
            settle(inner, op, handed, false)?;
            continue;
        }
        op.state = "failed".into();
        op.reason = Some(OpReason {
            code: "expired".into(),
            text: format!("The action waited {OP_DAYS} days for its machine, so it was dropped."),
            last_seen_at: last_seen(inner, &op.owner_runtime).map(super::timestamp),
        });
        op.error = Some(
            op.reason
                .as_ref()
                .map(|reason| reason.text.clone())
                .unwrap_or_default(),
        );
        op.updated_at = inner.now_text();
        store(inner, &op)?;
        inner
            .store
            .connect()?
            .execute("DELETE FROM outbox WHERE message_id=?1", [&op.op_id])
            .map_err(sql)?;
        emit(inner, &op);
    }
    Ok(())
}

/// `fleet.op_status`: one forwarded action as it stands.
pub(super) fn status(inner: &Inner, body: Value) -> Result<Value, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Body {
        op_id: String,
    }
    let body: Body = serde_json::from_value(body).map_err(|_| "The request is invalid.")?;
    let op = get(inner, &body.op_id)?.ok_or("The action was not found.")?;
    Ok(json!({"op": op}))
}

/// The state of an op its sender cancelled before it left.
const CANCELLED: &str = "cancelled";
/// The most ops `fleet.ops` lists.
const MAX_LISTED: usize = 200;

/// `fleet.op_cancel`: drops an action that waits for its owner. An action
/// the owner already received cannot be called back. A copy another member
/// relays may still reach the owner, and its acknowledgement then reports
/// what the owner did.
pub(super) fn cancel(inner: &Inner, body: Value) -> Result<Value, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Body {
        op_id: String,
    }
    let body: Body = serde_json::from_value(body).map_err(|_| "The request is invalid.")?;
    let op = {
        let _merge = lock(&inner.merge);
        let mut op = get(inner, &body.op_id)?.ok_or("The action was not found.")?;
        if op.state == CANCELLED {
            return Ok(json!({"op": op}));
        }
        if op.state != "queued" {
            let name = op
                .owner_name
                .clone()
                .unwrap_or_else(|| "The machine that runs it".into());
            return Err(match op.state.as_str() {
                "sent" => format!("{name} already received the action."),
                _ => "The action already finished.".into(),
            });
        }
        op.state = CANCELLED.into();
        op.reason = None;
        op.updated_at = inner.now_text();
        store(inner, &op)?;
        inner
            .store
            .connect()?
            .execute("DELETE FROM outbox WHERE message_id=?1", [&op.op_id])
            .map_err(sql)?;
        op
    };
    emit(inner, &op);
    Ok(json!({"op": op}))
}

/// `fleet.ops`: the actions this member forwarded, newest first. `state`
/// names one state, or `open` for those still `queued` or `sent`.
pub(super) fn list(inner: &Inner, body: Value) -> Result<Value, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Body {
        #[serde(default)]
        state: Option<String>,
    }
    let body: Body = serde_json::from_value(body).map_err(|_| "The request is invalid.")?;
    let state = body.state.filter(|state| !state.is_empty());
    if let Some(state) = &state {
        if !["open", "queued", "sent", "applied", "failed", CANCELLED].contains(&state.as_str()) {
            return Err("The state is not one an action takes.".into());
        }
    }
    // Open ops come from their index. A closed state reads every op, since
    // the state lives in the record.
    let (query, limit) = match state.as_deref() {
        Some("open" | "queued" | "sent") => (
            "SELECT record FROM ops WHERE open=1 ORDER BY created_at DESC LIMIT ?1",
            MAX_LISTED as i64,
        ),
        None => (
            "SELECT record FROM ops ORDER BY created_at DESC LIMIT ?1",
            MAX_LISTED as i64,
        ),
        Some(_) => (
            "SELECT record FROM ops ORDER BY created_at DESC LIMIT ?1",
            -1,
        ),
    };
    let rows: Vec<String> = {
        let connection = inner.store.connect()?;
        let mut statement = connection.prepare(query).map_err(sql)?;
        let rows = statement
            .query_map([limit], |row| row.get(0))
            .map_err(sql)?;
        rows.collect::<Result<_, _>>().map_err(sql)?
    };
    let ops: Vec<Op> = rows
        .iter()
        .filter_map(|raw| serde_json::from_str::<Op>(raw).ok())
        .filter(|op| match state.as_deref() {
            None | Some("open") => true,
            Some(state) => op.state == state,
        })
        .take(MAX_LISTED)
        .collect();
    Ok(json!({"ops": ops}))
}

/// Keeps an op addressed to this runtime until the worker runs it. Copies
/// that arrive again while it waits change nothing.
pub(super) fn accept(inner: &Inner, message: &Message) -> Result<(), String> {
    inner
        .store
        .connect()?
        .execute(
            "INSERT OR IGNORE INTO op_inbox(op_id, message, received_at) VALUES(?1, ?2, ?3)",
            params![
                message.message_id,
                serde_json::to_string(message).map_err(|error| error.to_string())?,
                inner.now_text(),
            ],
        )
        .map(drop)
        .map_err(sql)?;
    inner.nudge();
    Ok(())
}

/// What a forwarded action carries besides the operation's own body.
#[derive(Deserialize)]
struct Envelope {
    operation: Operation,
    body: Value,
    #[serde(default)]
    actor_device: String,
    #[serde(default)]
    actor_scopes: BTreeSet<String>,
    idempotency_key: String,
}

/// One action another member forwarded, as the owner runs it.
#[derive(Clone, Debug, PartialEq)]
pub struct Forwarded {
    pub operation: Operation,
    pub body: Value,
    /// A UUID made from the forwarding member, device and key, so a repeat
    /// of the action takes the same key on this runtime.
    pub idempotency_key: String,
    /// The member the actor used.
    pub from_runtime: String,
    pub actor_device: String,
}

/// Why the owner refuses an action before it runs, if it does.
fn refusal(inner: &Inner, message: &Message, envelope: &Envelope) -> Option<String> {
    let expired = parse_time(&message.created_at)
        .is_some_and(|created| (inner.now() - created).num_days() >= OP_DAYS);
    if expired {
        return Some(format!(
            "The action waited more than {OP_DAYS} days, so it was dropped."
        ));
    }
    if !is_forwardable(envelope.operation) {
        return Some(format!(
            "{} cannot run from another machine.",
            envelope.operation.as_str()
        ));
    }
    let scopes = effective_scopes(
        &envelope.actor_scopes,
        &member_scopes(inner, &message.from_runtime),
    );
    match crate::attach::remote_rule(envelope.operation, &envelope.body) {
        RemoteRule::Scope(scope) if muniment_attach::scopes_grant(&scopes, scope) => None,
        RemoteRule::AnyScope => None,
        RemoteRule::Scope(scope) => Some(format!(
            "The device that asked lacks the {scope} scope, so this machine refused the action."
        )),
        _ => Some(format!(
            "{} cannot run from another machine.",
            envelope.operation.as_str()
        )),
    }
}

/// A UUID for a forwarded action's key, the same for every repeat of it.
fn forwarded_key(from_runtime: &str, actor_device: &str, key: &str) -> String {
    let digest = Sha256::digest(format!("{from_runtime}\u{1f}{actor_device}\u{1f}{key}"));
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    uuid::Builder::from_random_bytes(bytes)
        .into_uuid()
        .to_string()
}

/// An answer small enough for an acknowledgement.
fn compact(result: Value) -> Value {
    let fits = serde_json::to_vec(&result).is_ok_and(|bytes| bytes.len() <= MAX_RESULT_BYTES);
    if fits {
        return result;
    }
    let Value::Object(object) = result else {
        return json!({"truncated": true});
    };
    let mut kept = serde_json::Map::new();
    for (key, value) in object {
        match value {
            Value::Object(inner) => {
                let ids: serde_json::Map<String, Value> = inner
                    .into_iter()
                    .filter(|(key, value)| {
                        RESULT_KEYS.contains(&key.as_str())
                            && serde_json::to_vec(value).is_ok_and(|bytes| bytes.len() <= 256)
                    })
                    .collect();
                kept.insert(key, Value::Object(ids));
            }
            Value::Array(_) => {}
            value => {
                if serde_json::to_vec(&value).is_ok_and(|bytes| bytes.len() <= 256) {
                    kept.insert(key, value);
                }
            }
        }
    }
    kept.insert("truncated".into(), Value::Bool(true));
    Value::Object(kept)
}

/// Runs the ops other members forwarded to this runtime, oldest first, and
/// acknowledges each with its answer. A key this runtime already ran answers
/// the first result again. An action that was running when the runtime
/// stopped is not run twice.
pub(super) fn execute_pending(inner: &Inner) -> Result<(), String> {
    let _running = lock(&inner.ops_running);
    let Some(own) = inner.runtime_id() else {
        return Ok(());
    };
    let pending: Vec<String> = {
        let connection = inner.store.connect()?;
        let mut statement = connection
            .prepare("SELECT message FROM op_inbox WHERE done=0 ORDER BY received_at, op_id")
            .map_err(sql)?;
        let rows = statement.query_map([], |row| row.get(0)).map_err(sql)?;
        rows.collect::<Result<_, _>>().map_err(sql)?
    };
    for raw in pending {
        let Ok(message) = serde_json::from_str::<Message>(&raw) else {
            continue;
        };
        let outcome = match serde_json::from_value::<Envelope>(message.body.clone()) {
            Err(_) => Err("The action is not one this machine knows.".to_owned()),
            Ok(envelope) => match refusal(inner, &message, &envelope) {
                Some(note) => Err(note),
                None => {
                    let moved = item_of(envelope.operation, &envelope.body)
                        .and_then(|item| owner_of(inner, &item))
                        .filter(|owner| *owner != own);
                    if let Some(owner) = moved {
                        // The item moved on: the action follows it.
                        let _merge = lock(&inner.merge);
                        messaging::readdress(inner, &message, &owner)?;
                        inner
                            .store
                            .connect()?
                            .execute("DELETE FROM op_inbox WHERE op_id=?1", [&message.message_id])
                            .map_err(sql)?;
                        inner.nudge();
                        continue;
                    }
                    run_once(inner, &message, envelope)
                }
            },
        };
        let ack = match outcome {
            Ok(result) => Ack {
                message_id: message.message_id.clone(),
                status: "delivered".into(),
                note: None,
                at: inner.now_text(),
                result: Some(compact(result)),
            },
            Err(note) => Ack {
                message_id: message.message_id.clone(),
                status: "refused".into(),
                note: Some(note),
                at: inner.now_text(),
                result: None,
            },
        };
        let actor_device = message
            .body
            .get("actor_device")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let _merge = lock(&inner.merge);
        let logged = super::audit::append(
            inner,
            "op",
            Some(&message.from_runtime),
            json!({
                "op_id": message.message_id,
                "operation": message.body.get("operation"),
                "actor_device": actor_device,
                "origin": message.from_runtime,
                "result": ack.status,
                "note": ack.note,
            }),
        );
        if let Err(error) = logged {
            crate::runtime_eprintln!("muniment-runtime: fleet audit of an action failed: {error}");
        }
        messaging::finish(inner, &message.message_id, &ack, None)?;
        inner
            .store
            .connect()?
            .execute(
                "UPDATE op_inbox SET done=1 WHERE op_id=?1",
                [&message.message_id],
            )
            .map_err(sql)?;
        inner.nudge();
    }
    Ok(())
}

/// Runs one action unless its key ran before, and records its outcome under
/// the key.
fn run_once(inner: &Inner, message: &Message, envelope: Envelope) -> Result<Value, String> {
    let key = forwarded_key(
        &message.from_runtime,
        &envelope.actor_device,
        &envelope.idempotency_key,
    );
    let earlier: Option<(String, Option<String>, Option<String>)> = inner
        .store
        .connect()?
        .query_row(
            "SELECT state, result, note FROM op_results WHERE key=?1",
            [&key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(sql)?;
    match earlier {
        Some((state, result, _)) if state == "applied" => {
            return Ok(result
                .and_then(|result| serde_json::from_str(&result).ok())
                .unwrap_or(Value::Null))
        }
        Some((state, _, note)) if state == "failed" => {
            return Err(note.unwrap_or_else(|| "The action failed.".into()))
        }
        Some(_) => return Err(
            "This machine stopped while it ran the action. Check its result before you try again."
                .into(),
        ),
        None => {}
    }
    inner
        .store
        .connect()?
        .execute(
            "INSERT INTO op_results(key, op_id, state, at) VALUES(?1, ?2, 'running', ?3)",
            params![key, message.message_id, inner.now_text()],
        )
        .map_err(sql)?;
    let outcome = inner.host.run_forwarded(&Forwarded {
        operation: envelope.operation,
        body: envelope.body,
        idempotency_key: key.clone(),
        from_runtime: message.from_runtime.clone(),
        actor_device: envelope.actor_device,
    });
    let (state, result, note) = match &outcome {
        Ok(result) => ("applied", Some(result.to_string()), None),
        Err(note) => ("failed", None, Some(note.clone())),
    };
    inner
        .store
        .connect()?
        .execute(
            "UPDATE op_results SET state=?2, result=?3, note=?4, at=?5 WHERE key=?1",
            params![key, state, result, note, inner.now_text()],
        )
        .map_err(sql)?;
    outcome
}

/// Drops the owner's record of keys past the time a repeat can arrive.
pub(super) fn forget_old_results(inner: &Inner) -> Result<(), String> {
    let cutoff = super::timestamp(inner.now() - chrono::Duration::days(OP_DAYS + 1));
    let connection = inner.store.connect()?;
    connection
        .execute(
            "DELETE FROM op_results WHERE at < ?1 AND state<>'running'",
            [&cutoff],
        )
        .map_err(sql)?;
    connection
        .execute(
            "DELETE FROM op_inbox WHERE done=1 AND received_at < ?1",
            [&cutoff],
        )
        .map_err(sql)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::super::{Identity, ReplicaScope, Swarm, SwarmHost};
    use super::*;
    use crate::permission_gate::ChatPermissionAnswer;

    /// A host that counts the forwarded actions it runs.
    #[derive(Default)]
    struct Counting(AtomicUsize);

    impl SwarmHost for Counting {
        fn replica_scope(&self) -> Option<ReplicaScope> {
            None
        }

        fn steer(&self, _thread_id: &str, _text: &str) -> Option<String> {
            None
        }

        fn answer_permission(
            &self,
            _run_id: &str,
            _gate_id: &str,
            _answer: ChatPermissionAnswer,
        ) -> Result<(), String> {
            Err("No runs here.".into())
        }

        fn kick_runs(&self) {}

        fn run_forwarded(&self, action: &Forwarded) -> Result<Value, String> {
            let count = self.0.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(json!({"operation": action.operation.as_str(), "count": count}))
        }
    }

    struct Owner {
        swarm: Swarm,
        host: Arc<Counting>,
        id: String,
        root: PathBuf,
    }

    impl Drop for Owner {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    impl Owner {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("swarm-ops-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            let storage = Arc::new(Mutex::new(crate::run_events::ChatStorage {
                journal: crate::journal::RunJournal::open(root.join("runs.sqlite3")).unwrap(),
                cas: crate::cas::LocalCas::open(&root.join("cas")).unwrap(),
            }));
            let host = Arc::new(Counting::default());
            let swarm = Swarm::open(&root, storage, host.clone()).unwrap();
            let id = uuid::Uuid::now_v7().to_string();
            *lock(&swarm.inner.identity) = Some(Identity {
                access: None,
                runtime_id: id.clone(),
                runtime_name: "Owner".into(),
                runtime_version: "1.0.0".into(),
                form_factor: None,
            });
            Self {
                swarm,
                host,
                id,
                root,
            }
        }

        fn inner(&self) -> &Inner {
            &self.swarm.inner
        }

        /// Takes in messages as a peer's exchange brings them, then runs the
        /// forwarded actions.
        fn receive(&self, messages: Vec<Message>) {
            {
                let _merge = lock(&self.inner().merge);
                messaging::receive(self.inner(), messages).unwrap();
            }
            execute_pending(self.inner()).unwrap();
        }

        fn ack(&self, message_id: &str) -> Option<Ack> {
            self.inner()
                .store
                .record("acks", "message_id", "ack", message_id)
                .unwrap()
        }

        fn runs(&self) -> usize {
            self.host.0.load(Ordering::SeqCst)
        }

        fn op(&self, operation: &str, scopes: &[&str], key: &str) -> Message {
            Message {
                message_id: uuid::Uuid::now_v7().to_string(),
                kind: KIND.into(),
                from_runtime: "member-c".into(),
                to_runtime: Some(self.id.clone()),
                from_agent: None,
                to_agent: None,
                hop: 0,
                created_at: self.inner().now_text(),
                body: json!({
                    "operation": operation,
                    "body": {"thread_id": "thread-1", "text": "Check the west region."},
                    "actor_device": "phone",
                    "actor_scopes": scopes,
                    "idempotency_key": key,
                }),
            }
        }
    }

    const PHONE: [&str; 2] = ["thread.read", "run.write"];

    #[test]
    fn an_owner_runs_a_repeated_action_once() {
        let owner = Owner::new();
        let first = owner.op("thread.post", &PHONE, "key-1");
        // The outbox sends a message again until its acknowledgement returns.
        owner.receive(vec![first.clone(), first.clone()]);
        owner.receive(vec![first.clone()]);
        assert_eq!(owner.runs(), 1);
        let ack = owner.ack(&first.message_id).unwrap();
        assert_eq!(ack.status, "delivered");
        assert_eq!(ack.result.as_ref().unwrap()["count"], 1);
        // The same actor's key under a new op answers the first result.
        let again = owner.op("thread.post", &PHONE, "key-1");
        owner.receive(vec![again.clone()]);
        assert_eq!(owner.runs(), 1);
        assert_eq!(owner.ack(&again.message_id).unwrap().result, ack.result);
        // Another key runs again.
        let other = owner.op("thread.post", &PHONE, "key-2");
        owner.receive(vec![other.clone()]);
        assert_eq!(owner.runs(), 2);
    }

    #[test]
    fn an_owner_refuses_actions_off_its_allowlist_past_their_scopes_or_too_old() {
        let owner = Owner::new();
        let delete = owner.op("agent.delete", &PHONE, "key-1");
        let unscoped = owner.op("thread.post", &["thread.read"], "key-2");
        let mut old = owner.op("thread.post", &PHONE, "key-3");
        old.created_at =
            super::super::timestamp(owner.inner().now() - chrono::Duration::days(OP_DAYS + 1));
        owner.receive(vec![delete.clone(), unscoped.clone(), old.clone()]);
        assert_eq!(owner.runs(), 0);
        let refused = |message: &Message| {
            let ack = owner.ack(&message.message_id).unwrap();
            assert_eq!(ack.status, "refused");
            ack.note.unwrap()
        };
        assert!(refused(&delete).contains("cannot run from another machine"));
        assert!(refused(&unscoped).contains("run.write"));
        assert!(refused(&old).contains("7 days"));
    }

    fn set(scopes: &[&str]) -> BTreeSet<String> {
        scopes.iter().map(|scope| (*scope).to_owned()).collect()
    }

    #[test]
    fn the_phone_actions_are_all_forwardable() {
        for operation in [
            Operation::ThreadPost,
            Operation::AgentApprovalAnswer,
            Operation::AgentRunNow,
            Operation::RunCancel,
            Operation::RunSteer,
        ] {
            assert!(is_forwardable(operation), "{}", operation.as_str());
        }
        for operation in [
            Operation::AgentDelete,
            Operation::AgentMove,
            Operation::SwarmLeave,
            Operation::PermissionSettingsSave,
            Operation::RemotePairingCreate,
        ] {
            assert!(!is_forwardable(operation), "{}", operation.as_str());
        }
    }

    #[test]
    fn scopes_are_what_both_the_device_and_the_member_hold() {
        let phone = set(&["thread.read", "run.write"]);
        let member = set(&["thread.read", "run.write", "agents.manage"]);
        assert_eq!(effective_scopes(&phone, &member), phone);
        let worker = set(&["thread.read"]);
        assert_eq!(effective_scopes(&phone, &worker), set(&["thread.read"]));
        assert!(!full_scopes().contains(SCOPE_SWARM_PEER));
        assert!(full_scopes().contains("agents.manage"));
    }

    #[test]
    fn items_come_from_each_operations_body() {
        let thread = item_of(
            Operation::ThreadPost,
            &json!({"thread_id": "t", "text": "x"}),
        );
        assert_eq!(thread.unwrap().kind, "thread");
        let agent = item_of(
            Operation::ThreadPost,
            &json!({"agent_id": "a", "text": "x"}),
        );
        assert_eq!(agent.unwrap().kind, "agent");
        let saved = item_of(Operation::AgentSave, &json!({"agent": {"id": "a"}})).unwrap();
        assert_eq!((saved.kind.as_str(), saved.id.as_str()), ("agent", "a"));
        assert!(item_of(Operation::AgentSave, &json!({"agent": {"name": "New"}})).is_none());
        let run = item_of(Operation::RunCancel, &json!({"run_id": "r"})).unwrap();
        assert_eq!(run.kind, "run");
        assert!(item_of(Operation::AgentDelete, &json!({"id": "a"})).is_none());
    }

    #[test]
    fn a_large_answer_keeps_its_ids() {
        let long = "x".repeat(4096);
        let answer = compact(json!({
            "run": {"id": "r-1", "status": "queued", "prompt": long},
            "note": "short",
        }));
        assert_eq!(answer["run"], json!({"id": "r-1", "status": "queued"}));
        assert_eq!(answer["note"], "short");
        assert_eq!(answer["truncated"], true);
        let small = json!({"status": "answered"});
        assert_eq!(compact(small.clone()), small);
    }

    #[test]
    fn a_repeat_takes_the_same_key() {
        let first = forwarded_key("c", "phone", "k");
        assert_eq!(first, forwarded_key("c", "phone", "k"));
        assert_ne!(first, forwarded_key("b", "phone", "k"));
        assert!(muniment_attach::Id::new(first).is_ok());
    }

    #[test]
    fn a_queued_action_cancels_and_leaves_the_outbox() {
        let sender = Owner::new();
        let inner = sender.inner();
        let queue = |key: &str| {
            forward(
                inner,
                Operation::ThreadPost,
                &json!({"thread_id": "thread-1", "text": "Check the west region."}),
                &FleetActor::owner(),
                Some(key),
                OpItem {
                    kind: "thread".into(),
                    id: "thread-1".into(),
                },
                "member-b",
            )
            .unwrap()
        };
        let first = queue("key-1");
        let second = queue("key-2");
        assert_eq!(first.state, "queued");
        let outbox = |op_id: &str| messaging::outbox_message(inner, op_id).unwrap().is_some();
        assert!(outbox(&first.op_id));
        let open = list(inner, json!({"state": "open"})).unwrap();
        assert_eq!(open["ops"].as_array().unwrap().len(), 2);
        // Newest first.
        assert_eq!(open["ops"][0]["op_id"], second.op_id);

        let cancelled = cancel(inner, json!({"op_id": first.op_id})).unwrap();
        assert_eq!(cancelled["op"]["state"], "cancelled");
        assert!(!outbox(&first.op_id));
        // A second cancel answers the same op.
        assert_eq!(
            cancel(inner, json!({"op_id": first.op_id})).unwrap()["op"]["state"],
            "cancelled"
        );
        let open = list(inner, json!({"state": "open"})).unwrap();
        assert_eq!(open["ops"].as_array().unwrap().len(), 1);
        let gone = list(inner, json!({"state": "cancelled"})).unwrap();
        assert_eq!(gone["ops"][0]["op_id"], first.op_id);
        assert_eq!(
            list(inner, json!({})).unwrap()["ops"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert!(list(inner, json!({"state": "lost"})).is_err());

        // A relayed copy that still reached the owner reports its outcome.
        on_ack(
            inner,
            &Ack {
                message_id: first.op_id.clone(),
                status: "delivered".into(),
                note: None,
                at: inner.now_text(),
                result: Some(json!({"message_id": "m-1"})),
            },
        )
        .unwrap();
        let applied = get(inner, &first.op_id).unwrap().unwrap();
        assert_eq!(applied.state, "applied");
        assert_eq!(
            cancel(inner, json!({"op_id": first.op_id})).unwrap_err(),
            "The action already finished."
        );
        assert_eq!(
            cancel(inner, json!({"op_id": "missing"})).unwrap_err(),
            "The action was not found."
        );
    }
}
