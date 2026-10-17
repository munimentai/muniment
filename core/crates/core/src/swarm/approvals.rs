//! Agent approvals and run notices across the swarm.
//!
//! The member that runs an agent run records each gate the run waits at as
//! an approval and gossips it, so every client of every member hears
//! `agent.approval_pending`. An answer from any member travels to that
//! member, which applies the first answer and records who gave it; a later
//! answer reads "already answered". Run changes that warrant a notification
//! reach the other members' clients as `agent.run_updated`.

use std::time::{Duration, Instant};

use muniment_attach::EventName;
use serde_json::{json, Value};

use super::store::sql;
use super::{lock, messaging, Approval, Inner, Notice, GOSSIP_TTL_SECONDS};
use crate::agent_runs::{RunStatus, RunStore};
use crate::journal::reducer::ChatProjector;
use crate::permission_gate::ChatPermissionAnswer;

/// How long an answer given on another member waits to hear which answer won.
const REMOTE_ANSWER_WAIT: Duration = Duration::from_secs(8);

fn self_id(inner: &Inner) -> String {
    inner.runtime_id().unwrap_or_else(|| "local".into())
}

fn get(inner: &Inner, approval_id: &str) -> Result<Option<Approval>, String> {
    inner
        .store
        .record("approvals", "approval_id", "record", approval_id)
}

fn put(inner: &Inner, approval: &Approval) -> Result<(), String> {
    inner.store.put_record(
        "approvals",
        "approval_id",
        "record",
        &approval.approval_id,
        approval,
    )
}

fn body(approval: &Approval) -> Value {
    serde_json::to_value(approval).unwrap_or(Value::Null)
}

/// The gate a run waits at now, read from the journal when the run grew.
fn current_gate(inner: &Inner, run_id: &str) -> Result<Option<Value>, String> {
    let mut storage = inner
        .storage
        .lock()
        .map_err(|_| "The journal is unavailable.".to_string())?;
    let last = storage
        .journal
        .last_run_seq(run_id)
        .map_err(|error| error.to_string())?;
    if let Some((seen, gate)) = lock(&inner.gates).get(run_id) {
        if *seen == last {
            return Ok(gate.clone());
        }
    }
    let events = storage
        .journal
        .events(run_id)
        .map_err(|error| error.to_string())?;
    drop(storage);
    let mut projector = ChatProjector::new();
    for event in &events {
        projector.apply(event).map_err(|error| error.to_string())?;
    }
    let gate = projector
        .projection()
        .map_err(|error| error.to_string())?
        .pending_permission
        .and_then(|gate| serde_json::to_value(gate).ok());
    lock(&inner.gates).insert(run_id.to_owned(), (last, gate.clone()));
    Ok(gate)
}

/// What an approval shows beside its gate: the exact policy request, the
/// summary, the reason it asks and the plan of a plan gate, all from the
/// gate's policy, and the answer ids of its options.
struct GateFacts {
    rule: Option<crate::permission_gate::PolicyRequest>,
    summary: Option<String>,
    reason: Option<String>,
    plan: Option<String>,
    answers: Vec<String>,
}

fn gate_facts(gate: &Value) -> GateFacts {
    let answers = gate["options"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(crate::permission_gate::gate_answer)
        .map(str::to_owned)
        .collect();
    let Ok(policy) =
        serde_json::from_value::<crate::journal::reducer::GatePolicy>(gate["policy"].clone())
    else {
        return GateFacts {
            rule: None,
            summary: None,
            reason: None,
            plan: None,
            answers,
        };
    };
    // A plan gate names no request a rule could allow.
    let rule = if policy.plan.is_none() {
        Some(crate::permission_gate::PolicyRequest {
            request_kind: policy.request_kind,
            resource: policy.resource,
        })
    } else {
        None
    };
    GateFacts {
        rule,
        summary: Some(policy.summary),
        reason: policy.reason,
        plan: policy.plan,
        answers,
    }
}

/// The gate answer a client's answer names: an answer id the approval
/// offers, such as `always_allow_agent`, or a gate answer itself.
fn resolve_answer(approval: &Approval, answer: &Value) -> Result<ChatPermissionAnswer, String> {
    if let Some(id) = answer.as_str() {
        if !approval.answers.iter().any(|offered| offered == id) {
            return Err("This approval does not offer that answer.".into());
        }
        let option = crate::permission_gate::gate_option(id).ok_or("The answer is invalid.")?;
        return Ok(ChatPermissionAnswer::Select(option.to_owned()));
    }
    serde_json::from_value(answer.clone()).map_err(|_| "The answer is invalid.".into())
}

/// The summary of the approval a run waits at, if any.
pub(super) fn pending_summary(inner: &Inner, run_id: &str) -> Option<String> {
    inner
        .store
        .records::<Approval>("approvals", "record")
        .ok()?
        .into_iter()
        .find(|approval| approval.run_id == run_id && approval.state == "pending")
        .map(|approval| {
            approval.summary.unwrap_or_else(|| {
                approval.request["title"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned()
            })
        })
}

/// Records each gate this runtime's agent runs wait at, and closes the
/// approvals whose gate went away.
pub(super) fn scan(inner: &Inner) -> Result<(), String> {
    let own = self_id(inner);
    let store = RunStore::open(&inner.profile)?;
    let active = store.active()?;
    let mut waiting = Vec::new();
    for run in active.iter().filter(|run| {
        matches!(run.status, RunStatus::Running | RunStatus::WaitingInput)
            && run.thread_id.is_some()
    }) {
        let Some(gate) = current_gate(inner, &run.id)? else {
            continue;
        };
        let gate_id = gate["gate_id"].as_str().unwrap_or_default().to_owned();
        let approval_id = format!("{}:{gate_id}", run.id);
        waiting.push(approval_id.clone());
        if get(inner, &approval_id)?.is_some() {
            continue;
        }
        let agent_name = crate::agents::get(&inner.profile, &run.agent_id)
            .map(|agent| agent.name)
            .unwrap_or_default();
        let GateFacts {
            rule,
            summary,
            reason,
            plan,
            answers,
        } = gate_facts(&gate);
        let approval = Approval {
            approval_id,
            runtime_id: own.clone(),
            run_id: run.id.clone(),
            gate_id,
            agent_id: run.agent_id.clone(),
            agent_name,
            thread_id: run.thread_id.clone(),
            request: gate,
            rule,
            summary,
            reason,
            plan,
            answers,
            state: "pending".into(),
            created_at: inner.now_text(),
            answered_at: None,
            answered_by: None,
            answer_id: None,
        };
        put(inner, &approval)?;
        inner.emit(EventName::AgentApprovalPending, body(&approval));
        inner.nudge();
    }
    let live: Vec<String> = active.iter().map(|run| run.id.clone()).collect();
    lock(&inner.gates).retain(|run_id, _| live.contains(run_id));
    for mut approval in inner.store.records::<Approval>("approvals", "record")? {
        if approval.runtime_id != own
            || approval.state != "pending"
            || waiting.contains(&approval.approval_id)
        {
            continue;
        }
        approval.state = if live.contains(&approval.run_id) {
            "answered"
        } else {
            "closed"
        }
        .into();
        approval.answered_at = Some(inner.now_text());
        put(inner, &approval)?;
        inner.emit(EventName::AgentApprovalResolved, body(&approval));
        inner.nudge();
    }
    Ok(())
}

/// Takes in the approvals a peer gossiped. The member that runs a gate
/// alone resolves it, so a resolved record wins over a pending one.
pub(super) fn merge(inner: &Inner, approvals: Vec<Approval>) -> Result<(), String> {
    let own = self_id(inner);
    for approval in approvals {
        if approval.runtime_id == own {
            continue;
        }
        let existing = get(inner, &approval.approval_id)?;
        match existing {
            None => {
                put(inner, &approval)?;
                if approval.state == "pending" {
                    inner.emit(EventName::AgentApprovalPending, body(&approval));
                }
            }
            Some(existing) if existing.state == "pending" && approval.state != "pending" => {
                put(inner, &approval)?;
                inner.emit(EventName::AgentApprovalResolved, body(&approval));
            }
            Some(_) => {}
        }
    }
    Ok(())
}

pub(super) fn recent(inner: &Inner) -> Result<Vec<Approval>, String> {
    let cutoff = super::timestamp(inner.now() - chrono::Duration::seconds(GOSSIP_TTL_SECONDS));
    let mut approvals: Vec<Approval> = inner
        .store
        .records::<Approval>("approvals", "record")?
        .into_iter()
        .filter(|approval| approval.created_at >= cutoff || approval.state == "pending")
        .collect();
    approvals.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    approvals.truncate(100);
    Ok(approvals)
}

/// `agent.approvals`: the approvals that still wait, oldest first.
pub(super) fn list(inner: &Inner) -> Result<Value, String> {
    let mut pending: Vec<Approval> = inner
        .store
        .records::<Approval>("approvals", "record")?
        .into_iter()
        .filter(|approval| approval.state == "pending")
        .collect();
    pending.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(json!({"approvals": pending}))
}

/// Resolves a pending approval of this runtime for `answer_id` and answers
/// it, or `None` when an earlier answer won.
fn claim(
    inner: &Inner,
    approval_id: &str,
    answer_id: &str,
    by: &str,
) -> Result<Option<Approval>, String> {
    let Some(mut approval) = get(inner, approval_id)? else {
        return Err("The approval was not found.".into());
    };
    if approval.state != "pending" {
        return Ok(None);
    }
    approval.state = "answered".into();
    approval.answer_id = Some(answer_id.to_owned());
    approval.answered_by = Some(by.to_owned());
    approval.answered_at = Some(inner.now_text());
    put(inner, &approval)?;
    Ok(Some(approval))
}

/// Applies a claimed answer to the run's gate. A gate another client closed
/// first leaves that answer standing.
fn apply(inner: &Inner, approval: &Approval, answer: ChatPermissionAnswer) -> bool {
    let applied = inner
        .host
        .answer_permission(&approval.run_id, &approval.gate_id, answer)
        .is_ok();
    inner.emit(EventName::AgentApprovalResolved, body(approval));
    inner.nudge();
    applied
}

/// Applies an answer another member sent for one of this runtime's gates.
pub(super) fn on_answer(inner: &Inner, message: &super::Message) -> Result<(), String> {
    let approval_id = message.body["approval_id"].as_str().unwrap_or_default();
    let answer_id = message.body["answer_id"].as_str().unwrap_or_default();
    let answer: ChatPermissionAnswer = serde_json::from_value(message.body["answer"].clone())
        .map_err(|_| "The answer is invalid.".to_string())?;
    if let Some(approval) = claim(inner, approval_id, answer_id, &message.from_runtime)? {
        apply(inner, &approval, answer);
    }
    Ok(())
}

/// `agent.approval_answer`: the first answer wins on the member that runs
/// the gate.
pub(super) fn answer_operation(
    inner: &Inner,
    body: Value,
    client: Option<&str>,
) -> Result<Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Body {
        approval_id: String,
        answer: Value,
    }
    let body: Body = serde_json::from_value(body).map_err(|_| "The request is invalid.")?;
    let answer_id = uuid::Uuid::now_v7().to_string();
    let own = self_id(inner);
    let answered = |approval: Approval, ours: bool| {
        json!({
            "status": if ours { "answered" } else { "already_answered" },
            "approval": approval,
        })
    };
    let approval = {
        let _merge = lock(&inner.merge);
        get(inner, &body.approval_id)?.ok_or("The approval was not found.")?
    };
    if approval.state != "pending" {
        return Ok(answered(approval, false));
    }
    let answer = resolve_answer(&approval, &body.answer)?;
    if approval.runtime_id == own {
        let claimed = {
            let _merge = lock(&inner.merge);
            claim(inner, &body.approval_id, &answer_id, client.unwrap_or(&own))?
        };
        return Ok(match claimed {
            Some(approval) => {
                let applied = apply(inner, &approval, answer);
                answered(approval, applied)
            }
            None => answered(
                get(inner, &body.approval_id)?.ok_or("The approval was not found.")?,
                false,
            ),
        });
    }
    {
        let _merge = lock(&inner.merge);
        messaging::send(
            inner,
            messaging::new_message(
                inner,
                "approval_answer",
                &approval.runtime_id,
                None,
                None,
                0,
                json!({
                    "approval_id": body.approval_id,
                    "answer": answer.decision(),
                    "answer_id": answer_id,
                }),
            ),
        )?;
    }
    let deadline = Instant::now() + REMOTE_ANSWER_WAIT;
    while Instant::now() < deadline {
        inner.nudge();
        if let Some(current) = get(inner, &body.approval_id)? {
            if current.state != "pending" {
                let ours = current.answer_id.as_deref() == Some(answer_id.as_str());
                return Ok(answered(current, ours));
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(json!({"status": "sent", "approval": approval}))
}

/// Turns this runtime's run changes that warrant a notification into notices
/// for the other members, and drops notices past the gossip window.
pub(super) fn forward_run_changes(inner: &Inner) -> Result<(), String> {
    let mut changes = Vec::new();
    if let Some(receiver) = lock(&inner.run_changes).as_ref() {
        while let Ok(change) = receiver.try_recv() {
            changes.push(change);
        }
    }
    let Some(own) = inner.runtime_id() else {
        return Ok(());
    };
    for mut change in changes {
        if change["notify"] != true || change.get("runtime_id").is_some() {
            continue;
        }
        change["runtime_id"] = Value::from(own.as_str());
        let notice = Notice {
            notice_id: uuid::Uuid::now_v7().to_string(),
            kind: "run_updated".into(),
            origin: own.clone(),
            body: change,
            created_at: inner.now_text(),
        };
        store_notice(inner, &notice)?;
        inner.nudge();
    }
    let cutoff = super::timestamp(inner.now() - chrono::Duration::seconds(GOSSIP_TTL_SECONDS));
    inner
        .store
        .connect()?
        .execute("DELETE FROM notices WHERE created_at < ?1", [&cutoff])
        .map_err(sql)?;
    Ok(())
}

fn store_notice(inner: &Inner, notice: &Notice) -> Result<bool, String> {
    let changed = inner
        .store
        .connect()?
        .execute(
            "INSERT OR IGNORE INTO notices(notice_id, notice, created_at) VALUES(?1, ?2, ?3)",
            rusqlite::params![
                notice.notice_id,
                serde_json::to_string(notice).map_err(|error| error.to_string())?,
                notice.created_at
            ],
        )
        .map_err(sql)?;
    Ok(changed > 0)
}

/// Takes in notices and shows the run changes of other members to this
/// runtime's clients.
pub(super) fn merge_notices(inner: &Inner, notices: Vec<Notice>) -> Result<(), String> {
    let own = self_id(inner);
    let cutoff = super::timestamp(inner.now() - chrono::Duration::seconds(GOSSIP_TTL_SECONDS));
    for notice in notices {
        if notice.origin == own || notice.created_at < cutoff {
            continue;
        }
        if store_notice(inner, &notice)? && notice.kind == "run_updated" {
            crate::agent_runs::publish_remote(&inner.profile, notice.body);
        }
    }
    Ok(())
}

pub(super) fn recent_notices(inner: &Inner) -> Result<Vec<Notice>, String> {
    let connection = inner.store.connect()?;
    let mut statement = connection
        .prepare("SELECT notice FROM notices ORDER BY created_at DESC LIMIT 100")
        .map_err(sql)?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(sql)?;
    let mut notices = Vec::new();
    for row in rows {
        if let Ok(notice) = serde_json::from_str(&row.map_err(sql)?) {
            notices.push(notice);
        }
    }
    Ok(notices)
}

/// The chat event the phone relay shows for an approval another member
/// holds: a waiting gate while it is pending, a finished run once answered.
pub(super) fn phone_event(approval: &Approval) -> Option<crate::run_events::ChatEvent> {
    let pending = approval.state == "pending";
    let request: Option<crate::journal::reducer::PermissionGate> =
        serde_json::from_value(approval.request.clone()).ok();
    let pending_permission = match (pending, request) {
        (true, Some(gate)) => Some(crate::chat_view::ChatPendingPermission {
            gate_id: gate.gate_id,
            request: gate.request,
            diff: None,
        }),
        (true, None) => return None,
        (false, _) => None,
    };
    Some(crate::run_events::ChatEvent {
        run_id: approval.run_id.clone(),
        thread_id: approval.thread_id.clone(),
        phase: if pending {
            "pending-permission"
        } else {
            "complete"
        }
        .into(),
        text: if pending {
            format!("{} waits for your approval.", approval.agent_name)
        } else {
            format!("{}'s approval was answered.", approval.agent_name)
        },
        prompt_accepted: true,
        turn_started: true,
        routing_stage: None,
        prompt_storage_notice: None,
        resume_notice: None,
        failure_reason: None,
        receipt: None,
        tool_activity: Vec::new(),
        attachments: Vec::new(),
        recalls: Vec::new(),
        applied_diffs: Vec::new(),
        pending_permission,
        delta: None,
        seq: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approval(state: &str) -> Approval {
        Approval {
            approval_id: "run-1:gate-1".into(),
            runtime_id: "runtime-a".into(),
            run_id: "run-1".into(),
            gate_id: "gate-1".into(),
            agent_id: "agent-1".into(),
            agent_name: "Deployer".into(),
            thread_id: Some("thread-1".into()),
            request: json!({"gate_id": "gate-1", "kind": "confirm", "title": "Allow bash?", "message": "git push"}),
            rule: None,
            summary: None,
            reason: None,
            plan: None,
            answers: Vec::new(),
            state: state.into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            answered_at: None,
            answered_by: None,
            answer_id: None,
        }
    }

    #[test]
    fn the_phone_sees_a_remote_gate_wait_and_then_close() {
        let waiting = phone_event(&approval("pending")).unwrap();
        assert_eq!(waiting.phase, "pending-permission");
        assert_eq!(waiting.run_id, "run-1");
        let gate = waiting.pending_permission.unwrap();
        assert_eq!(gate.gate_id, "gate-1");
        assert!(waiting.text.contains("Deployer"));
        let closed = phone_event(&approval("answered")).unwrap();
        assert_eq!(closed.phase, "complete");
        assert!(closed.pending_permission.is_none());
        let mut broken = approval("pending");
        broken.request = json!({"kind": "unknown"});
        assert!(phone_event(&broken).is_none());
    }
}
