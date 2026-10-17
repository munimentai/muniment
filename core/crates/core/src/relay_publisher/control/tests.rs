use std::sync::Mutex;

use super::*;
use crate::active_run::ChatDelivery;
use crate::permission_gate::ChatPermissionAnswer;
use crate::run_events::ChatEventSubscription;

/// A runtime that answers attach operations from a table and records them.
#[derive(Default)]
struct Answers {
    calls: Mutex<Vec<(String, Value, String)>>,
    answers: Mutex<Vec<(String, Result<Value, RequestError>)>>,
    summaries: Mutex<Vec<Value>>,
}

impl Answers {
    fn with(self, operation: &str, answer: Result<Value, RequestError>) -> Self {
        self.answers
            .lock()
            .unwrap()
            .push((operation.to_owned(), answer));
        self
    }

    fn calls(&self) -> Vec<(String, Value)> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|(operation, body, _)| (operation.clone(), body.clone()))
            .collect()
    }
}

impl RelayRuntime for Answers {
    fn subscribe(&self) -> Option<ChatEventSubscription> {
        None
    }
    fn stop(&self, _run_id: &str) -> Result<(), String> {
        unreachable!()
    }
    fn queue(&self, _run_id: &str, _delivery: ChatDelivery, _text: &str) -> Result<(), String> {
        unreachable!()
    }
    fn answer(
        &self,
        _run_id: &str,
        _gate_id: &str,
        _answer: ChatPermissionAnswer,
    ) -> Result<(), String> {
        unreachable!()
    }

    fn request(
        &self,
        operation: Operation,
        body: Value,
        request_id: &str,
    ) -> Result<Value, RequestError> {
        let name = serde_json::to_value(operation).unwrap();
        let name = name.as_str().unwrap().to_owned();
        self.calls
            .lock()
            .unwrap()
            .push((name.clone(), body.clone(), request_id.to_owned()));
        if name == "thread.summaries" {
            // Pages of two, newest first.
            let summaries = self.summaries.lock().unwrap().clone();
            let start: usize = body["cursor"].as_str().map_or(0, |c| c.parse().unwrap());
            let end = (start + 2).min(summaries.len());
            return Ok(json!({
                "summaries": summaries[start..end],
                "nextCursor": (end < summaries.len()).then(|| end.to_string()),
            }));
        }
        self.answers
            .lock()
            .unwrap()
            .iter()
            .find(|(listed, _)| *listed == name)
            .map(|(_, answer)| answer.clone())
            .unwrap_or_else(|| Err(RequestError::new("failed", format!("{name} has no answer"))))
    }
}

fn summary(id: &str, updated_at: &str, owner: &str) -> Value {
    json!({"threadId": id, "title": id, "updatedAt": updated_at, "owner_runtime": owner})
}

fn fleet() -> Answers {
    let runtime = Answers::default()
        .with(
            "swarm.members",
            Ok(json!({"self": "rt_studio", "members": [
                {"runtime_id": "rt_studio", "runtime_name": "Studio", "self": true, "online": true},
                {"runtime_id": "rt_laptop", "runtime_name": "Laptop", "self": false, "online": false},
            ], "absence": null, "sync": {}})),
        )
        .with(
            "swarm.agents",
            Ok(json!({"agents": [
                {"agent_id": "agent_scout", "name": "Scout", "primary_runtime": "rt_laptop",
                 "avatar": {"style": "muniment", "seed": "s"}},
            ]})),
        )
        .with(
            "agent.approvals",
            Ok(json!({"approvals": [{"approval_id": "run_1:gate_1", "state": "pending"}]})),
        )
        .with(
            "group.list",
            Err(RequestError::new("failed", "Groups are unavailable.")),
        )
        .with("fleet.sync_status", Ok(json!({"state": "synced"})));
    *runtime.summaries.lock().unwrap() = vec![
        summary("thread_c", "2026-10-17T12:00:03.000Z", "rt_studio"),
        summary("thread_b", "2026-10-17T12:00:02.000Z", "rt_laptop"),
        summary("thread_a", "2026-10-17T12:00:01.000Z", "rt_laptop"),
    ];
    runtime
}

#[test]
fn a_snapshot_reads_the_whole_fleet_and_names_the_parts_that_failed() {
    let runtime = fleet();
    let snapshot = answer(&runtime, "fleet.snapshot", json!({}), "req_snapshot_01").unwrap();
    assert_eq!(snapshot["full"], true);
    assert_eq!(snapshot["self"], "rt_studio");
    assert_eq!(snapshot["members"].as_array().unwrap().len(), 2);
    assert_eq!(snapshot["members"][1]["online"], false);
    assert_eq!(snapshot["agents"][0]["primary_runtime"], "rt_laptop");
    assert_eq!(snapshot["approvals"][0]["approval_id"], "run_1:gate_1");
    assert_eq!(snapshot["groups"], json!([]));
    assert_eq!(snapshot["errors"]["group.list"], "Groups are unavailable.");
    assert_eq!(snapshot["sync"]["state"], "synced");
    // A full snapshot carries the first page and the cursor of the next.
    let threads: Vec<_> = snapshot["threads"]
        .as_array()
        .unwrap()
        .iter()
        .map(|thread| thread["threadId"].as_str().unwrap())
        .collect();
    assert_eq!(threads, ["thread_c", "thread_b"]);
    assert_eq!(snapshot["threads_cursor"], "2");
    assert_eq!(snapshot["version"], "2026-10-17T12:00:03.000Z");
    assert!(runtime
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|(_, _, request_id)| request_id == "req_snapshot_01"));
}

#[test]
fn a_snapshot_since_a_version_lists_only_the_threads_that_changed() {
    let runtime = fleet();
    let snapshot = answer(
        &runtime,
        "fleet.snapshot",
        json!({"since": "2026-10-17T12:00:01.000Z"}),
        "req_snapshot_02",
    )
    .unwrap();
    assert_eq!(snapshot["full"], false);
    let threads: Vec<_> = snapshot["threads"]
        .as_array()
        .unwrap()
        .iter()
        .map(|thread| thread["threadId"].as_str().unwrap())
        .collect();
    assert_eq!(threads, ["thread_c", "thread_b"]);
    assert_eq!(snapshot["threads_cursor"], Value::Null);
    // Nothing newer keeps the version the phone sent.
    let unchanged = answer(
        &runtime,
        "fleet.snapshot",
        json!({"since": "2026-10-17T12:00:03.000Z"}),
        "req_snapshot_03",
    )
    .unwrap();
    assert_eq!(unchanged["threads"], json!([]));
    assert_eq!(unchanged["version"], "2026-10-17T12:00:03.000Z");
    for body in [
        json!({"since": 5}),
        json!({"since": ""}),
        json!({"other": 1}),
    ] {
        assert_eq!(
            answer(&runtime, "fleet.snapshot", body, "req_snapshot_04")
                .unwrap_err()
                .code,
            "invalid_request"
        );
    }
}

#[test]
fn a_runtime_outside_a_swarm_is_a_fleet_of_one() {
    let runtime = Answers::default();
    let snapshot = answer(&runtime, "fleet.snapshot", json!({}), "req_snapshot_05").unwrap();
    assert_eq!(snapshot["members"].as_array().unwrap().len(), 1);
    assert_eq!(snapshot["members"][0]["self"], true);
    assert_eq!(snapshot["agents"], json!([]));
    assert_eq!(snapshot["threads"], json!([]));
    assert_eq!(snapshot["version"], "0");
    assert!(snapshot["errors"]["swarm.members"].is_string());
}

#[test]
fn only_the_phone_operations_reach_the_runtime() {
    let runtime = Answers::default().with("thread.post", Ok(json!({"forwarded": true})));
    assert_eq!(
        answer(
            &runtime,
            "thread.post",
            json!({"thread_id": "thread_a", "text": "hi"}),
            "req_post_0001"
        )
        .unwrap(),
        json!({"forwarded": true})
    );
    for operation in [
        "agent.save",
        "swarm.invite",
        "runtime.info",
        "fleet.offline_set",
    ] {
        assert_eq!(
            answer(&runtime, operation, json!({}), "req_other_0001")
                .unwrap_err()
                .code,
            "unsupported_operation"
        );
    }
    assert_eq!(
        answer(&runtime, "thread.post", json!([]), "req_post_0002")
            .unwrap_err()
            .code,
        "invalid_request"
    );
    assert_eq!(
        runtime.calls(),
        vec![(
            "thread.post".to_owned(),
            json!({"thread_id": "thread_a", "text": "hi"})
        )]
    );
}

#[test]
fn reading_an_agent_another_member_runs_answers_its_directory_entry() {
    let runtime = fleet().with(
        "agent.read",
        Err(RequestError::new("failed", "The agent was not found.")),
    );
    let read = answer(
        &runtime,
        "agent.read",
        json!({"id": "agent_scout"}),
        "req_read_0001",
    )
    .unwrap();
    assert_eq!(read["remote"], true);
    assert_eq!(read["agent"]["name"], "Scout");
    let missing = answer(
        &runtime,
        "agent.read",
        json!({"id": "agent_ghost"}),
        "req_read_0002",
    )
    .unwrap_err();
    assert_eq!(missing.message, "The agent was not found.");
}

/// The phone's side: joins the pages and parses the envelope.
fn joined(pages: &[ControlContent]) -> Value {
    let mut text = String::new();
    for (index, page) in pages.iter().enumerate() {
        let ControlContent::Response {
            page: number,
            pages: total,
            body,
            ..
        } = page
        else {
            panic!("a response page")
        };
        assert_eq!(*number as usize, index);
        assert_eq!(*total as usize, pages.len());
        text.push_str(body);
    }
    serde_json::from_str(&text).unwrap()
}

#[test]
fn a_large_answer_splits_into_pages_that_each_seal_and_join_again() {
    // Quotes, backslashes, control characters and wide characters all grow
    // when the page body is escaped.
    let text = "\"\\\u{1}é漢🙂 ".repeat(40_000);
    let pages = pages("req_large_0001", Ok(json!({"text": text})));
    assert!(pages.len() > 1);
    let keys = crate::relay_encryption::RelayKeys {
        pair_id: "00000000-0000-4000-8000-000000000000".into(),
        desktop_to_phone: crate::relay_encryption::DirectionKey::from_bytes([7; 32]),
        phone_to_desktop: crate::relay_encryption::DirectionKey::from_bytes([8; 32]),
    };
    let header = crate::relay_encryption::ControlHeader::new(
        "ctl_00000000000000004000800000000000",
        1,
        false,
        "2026-10-17T12:00:00.000Z",
    );
    for (counter, page) in pages.iter().enumerate() {
        crate::relay_encryption::seal_control(
            &keys.desktop_to_phone,
            &header,
            counter as u64 + 1,
            page,
        )
        .unwrap();
    }
    assert_eq!(joined(&pages)["result"]["text"], text.as_str());
    let failed = pages_of_error();
    assert_eq!(failed["ok"], false);
    assert_eq!(failed["error"]["code"], "unauthorized");
}

fn pages_of_error() -> Value {
    joined(&pages(
        "req_error_0001",
        Err(RequestError::new("unauthorized", "Pair it again.")),
    ))
}

#[test]
fn an_answer_past_the_response_limit_becomes_too_large() {
    let text = "x".repeat(MAX_RESPONSE_BYTES + 1);
    let pages = pages("req_large_0002", Ok(json!({"text": text})));
    assert_eq!(pages.len(), 1);
    assert_eq!(joined(&pages)["error"]["code"], "too_large");
}

#[test]
fn the_control_session_and_the_forwarded_events() {
    assert_eq!(
        control_session_id("0199A1B2-c3d4-7e5f-8a9b-0c1d2e3f4a5b"),
        "ctl_0199a1b2c3d47e5f8a9b0c1d2e3f4a5b"
    );
    assert_eq!(
        forwarded(&EventName::AgentApprovalPending),
        Some(("agent.approval_pending".to_owned(), true))
    );
    assert_eq!(
        forwarded(&EventName::FleetOpUpdated),
        Some(("fleet.op_updated".to_owned(), false))
    );
    assert_eq!(forwarded(&EventName::ModelsLogin), None);
    let ControlContent::Event { body, .. } = event("swarm.changed", json!({"kind": "agents"}))
    else {
        panic!("an event")
    };
    assert_eq!(body, Some(json!({"kind": "agents"})));
    let ControlContent::Event { body, .. } = event(
        "group.posted",
        json!({"post": "x".repeat(MAX_PAGE_BODY_BYTES)}),
    ) else {
        panic!("an event")
    };
    assert_eq!(body, None, "a body past one page goes as null");
}
