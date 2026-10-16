//! The agent operations' canonical fixtures are the core's own wire shapes:
//! each body decodes into the core type and encodes back unchanged.

use std::path::PathBuf;

use muniment_core::agent_runs::{Run, RunStatus, TriggerKind};
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
    let listing = fixture("response-agent-list.json")["body"].clone();
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
