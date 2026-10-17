//! The agent operations' canonical fixtures are the core's own wire shapes:
//! each body decodes into the core type and encodes back unchanged.

use std::path::PathBuf;

use muniment_core::agent_runs::{Activity, ActivityState, Run, RunStatus, TriggerKind};
use muniment_core::agents::{
    Agent, Condition, ConditionOp, MessagingAllow, PermissionMode, Routine, Trigger, WebhookPreset,
};
use serde_json::Value;

fn fixture(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocol-fixtures/muniment.attach/1")
        .join(name);
    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap()
}

fn round_trip<T: serde::de::DeserializeOwned + serde::Serialize>(value: &Value) -> T {
    let decoded: T = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(&serde_json::to_value(&decoded).unwrap(), value);
    decoded
}

#[test]
fn the_agent_fixture_is_an_agent_v2() {
    let body = fixture("response-agent-read.json")["body"].clone();
    let agent: Agent = round_trip(&body["agent"]);
    assert_eq!(
        agent.model.as_ref().unwrap().thinking.as_deref(),
        Some("medium")
    );
    assert_eq!(
        agent.permissions.as_ref().unwrap().mode,
        PermissionMode::Ask
    );
    assert_eq!(agent.messaging.as_ref().unwrap().allow, MessagingAllow::Any);
    assert!(agent.primary_runtime.is_none() && agent.paused_reason.is_none());
    assert_eq!(agent.routines.len(), 2);
    assert!(matches!(
        &agent.routines[1].trigger,
        Trigger::Webhook(hook) if hook.preset == WebhookPreset::Forgejo
    ));
    // The request to save carries the same agent.
    let request = fixture("request-agent-save.json");
    assert_eq!(request["operation"], "agent.save");
    round_trip::<Agent>(&request["body"]["agent"]);
    let mut listing = fixture("response-agent-list.json")["body"].clone();
    // `agent.list` adds each agent's activity to the agent.
    let activity = listing["agents"][0]
        .as_object_mut()
        .unwrap()
        .remove("activity")
        .unwrap();
    round_trip::<Activity>(&activity);
    round_trip::<Agent>(&listing["agents"][0]);
    round_trip::<Run>(&listing["latest_runs"]["018f0000-0000-4000-8000-0000000000a1"]);
}

#[test]
fn the_routine_fixture_is_an_mcp_routine_with_a_condition() {
    let body = fixture("response-routine-save.json")["body"].clone();
    let routine: Routine = round_trip(&body["routine"]);
    muniment_core::agent_routines::validate_routine(&routine).unwrap();
    let Trigger::Mcp(mcp) = &routine.trigger else {
        panic!("an mcp trigger");
    };
    assert_eq!(mcp.every.tz, "America/New_York");
    let Condition::All(all) = &mcp.condition else {
        panic!("an all group");
    };
    assert!(matches!(&all.all[0], Condition::Leaf(leaf) if leaf.op == ConditionOp::Gt));
    assert!(matches!(&all.all[1], Condition::Any(any) if any.any.len() == 2));
    for routine in fixture("response-routine-list.json")["body"]["routines"]
        .as_array()
        .unwrap()
    {
        round_trip::<Routine>(routine);
    }
}

#[test]
fn the_run_fixtures_are_runs() {
    let run: Run = round_trip(&fixture("response-agent-run-now.json")["body"]["run"]);
    assert_eq!(run.trigger.kind, TriggerKind::Cron);
    assert_eq!(run.status, RunStatus::Completed);
    let page = fixture("response-agent-runs.json")["body"].clone();
    round_trip::<Run>(&page["runs"][0]);
    assert!(page["next_cursor"].is_string());
    let mut event = fixture("event-agent-run-updated.json");
    assert_eq!(event["event"], "agent.run_updated");
    // The event adds whether the change warrants a notification to the run.
    let notify = event["body"]
        .as_object_mut()
        .unwrap()
        .remove("notify")
        .unwrap();
    assert_eq!(notify, true);
    let waiting: Run = round_trip(&event["body"]);
    assert_eq!(waiting.status, RunStatus::WaitingInput);
    assert!(waiting.waiting_since.is_some());
    assert_eq!(
        fixture("request-run-chat-events-agent-runs.json")["body"]["agent_runs"],
        true
    );
}

#[test]
fn the_polish_fixtures_are_core_shapes() {
    use muniment_core::swarm::{Approval, DirectoryEntry};
    // An approval carries the exact rule an agent's `always_allow` takes.
    let pending: Approval = round_trip(&fixture("event-agent-approval-pending.json")["body"]);
    let rule = pending.rule.clone().unwrap();
    let agent: Agent =
        serde_json::from_value(fixture("response-agent-read.json")["body"]["agent"].clone())
            .unwrap();
    let mut permissions = agent.permissions.unwrap();
    permissions.always_allow.push(rule);
    let permissions = serde_json::to_value(&permissions).unwrap();
    assert_eq!(
        permissions["always_allow"][1],
        fixture("event-agent-approval-pending.json")["body"]["rule"]
    );
    assert!(pending
        .answers
        .iter()
        .any(|answer| answer == "always_allow_agent"));
    let gate: muniment_core::journal::reducer::PermissionGate =
        serde_json::from_value(pending.request.clone()).unwrap();
    assert_eq!(gate.policy.unwrap().summary, pending.summary.unwrap());
    assert_eq!(
        fixture("request-agent-approval-answer-always.json")["body"]["answer"],
        "always_allow_agent"
    );
    // The directory and the activity event carry the same activity.
    let entry: DirectoryEntry =
        round_trip(&fixture("response-swarm-agents.json")["body"]["agents"][0]);
    let event = fixture("event-agent-activity.json");
    let activity: Activity = round_trip(&event["body"]["activity"]);
    assert_eq!(activity.state, ActivityState::Waiting);
    assert_eq!(entry.activity, Some(activity));
    assert_eq!(
        fixture("request-run-chat-events-swarm-live.json")["body"]["swarm_live"],
        true
    );
    // A replay is the saved inputs plus a listing's preview fields.
    let mut listed = fixture("response-agent-replays.json")["body"]["replays"][0].clone();
    let object = listed.as_object_mut().unwrap();
    let preview = object.remove("prompt_preview").unwrap();
    assert_eq!(
        object.remove("prompt_bytes").unwrap().as_u64().unwrap() as usize,
        preview.as_str().unwrap().len()
    );
    assert_eq!(object.remove("definition_changed").unwrap(), true);
    object.insert("prompt".into(), preview);
    let replay: muniment_core::agent_replays::Replay = round_trip(&listed);
    assert_eq!(replay.trigger.kind, TriggerKind::Webhook);
    assert!(replay.prompt.contains("<webhook_payload"));
    let replayed = fixture("response-agent-replay.json")["body"].clone();
    let run: Run = round_trip(&replayed["run"]);
    assert_eq!(
        run.origin_run_id.as_deref(),
        Some(replay.replay_id.as_str())
    );
    assert_eq!(replayed["replay"]["replayed_by"][0], run.id.as_str());
    // A group post names its meeting round and phase.
    let posted = fixture("event-group-posted.json")["body"].clone();
    assert_eq!(posted["post"]["author"]["phase"], "debate");
    assert_eq!(posted["post"]["author"]["round"], 2);
}
