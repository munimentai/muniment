#![cfg(unix)]
//! Event triggers: webhook deliveries over the remote-access listener and
//! MCP polls, each queuing an event-triggered agent run.

mod common;
use common::CI_WAIT;

use std::io::{Read, Write};
use std::sync::{mpsc, Arc, Mutex};

use muniment_core::agent_hooks::HookEndpoints;
use muniment_core::agent_runs::{RunStore, TriggerKind};
use muniment_core::agents::{self, Agent};
use muniment_core::attach::{
    InstallKind, Operation, RemoteAccessOptions, RemoteListener, RemoteListenerConfig,
};
use muniment_core::run_start::RunAttachBoundaries;
use muniment_core::serde_json::{self, json, Value};
use muniment_runtime_core::host::FileCredentialStore;
use muniment_runtime_core::{RuntimeAttachState, RuntimeHost};

/// Sends one HTTP request over the pinned TLS connection and answers the
/// status and the JSON body.
fn post(
    address: &str,
    fingerprint: muniment_core::attach::CertificateFingerprint,
    request: &[u8],
) -> (u16, Value) {
    let mut stream =
        muniment_core::attach::connect_pinned(&[address.to_owned()], fingerprint, CI_WAIT).unwrap();
    stream.write_all(request).unwrap();
    let mut answer = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => answer.extend_from_slice(&buffer[..read]),
        }
    }
    let text = String::from_utf8(answer).unwrap();
    let status = text[9..12].parse().unwrap();
    let body = text.split("\r\n\r\n").nth(1).unwrap_or("{}");
    (status, serde_json::from_str(body).unwrap_or(Value::Null))
}

fn request(hook: &str, headers: &[(&str, String)], body: &[u8]) -> Vec<u8> {
    let mut head = format!("POST /hooks/{hook} HTTP/1.1\r\nHost: runtime\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

fn state(temporary: &common::TemporaryProfile) -> Arc<RuntimeAttachState> {
    muniment_core::home::confirm_home(&temporary.profile, &temporary.root.join("muniment"))
        .unwrap();
    let host = RuntimeHost {
        credentials: Arc::new(FileCredentialStore::new(temporary.root.join("credentials"))),
        ..RuntimeHost::default()
    };
    Arc::new(RuntimeAttachState::open(host, &temporary.profile, &temporary.config).unwrap())
}

#[test]
fn a_signed_delivery_over_the_remote_listener_queues_a_webhook_run() {
    let temporary = common::TemporaryProfile::new("webhook-listener", false);
    let state = state(&temporary);
    let access = state
        .open_remote_access(RemoteAccessOptions {
            runtime_name: Some("Hooks".into()),
            runtime_version: state.runtime_version().into(),
            install_kind: InstallKind::Server,
        })
        .unwrap();
    let listener =
        RemoteListener::bind(RemoteListenerConfig::new("127.0.0.1:0".parse().unwrap())).unwrap();
    let address = listener.local_addr().unwrap().to_string();
    access
        .set_advertised_endpoints(vec![address.clone()])
        .unwrap();
    let (stop, stopped) = mpsc::channel();
    let served = Arc::clone(&state);
    let served_access = access.clone();
    let server =
        std::thread::spawn(move || served.serve_remote_listener(listener, served_access, stopped));

    let agent = agents::save(
        &temporary.profile,
        Agent {
            name: "Builder".into(),
            instructions: "Watch the builds.".into(),
            ..Agent::default()
        },
    )
    .unwrap();
    let created = state
        .boundaries()
        .hook_operation(
            Operation::RoutineWebhookCreate,
            json!({"agent_id": agent.id, "routine": {"name": "Build finished",
                "trigger": {"kind": "webhook", "preset": "github"}, "prompt": "Summarize the build."}}),
            HookEndpoints {
                endpoints: access.advertised_endpoints(),
                certificate_sha256: Some(access.certificate_fingerprint().to_hex()),
                listening: true,
            },
        )
        .unwrap();
    let hook = created["hook"]["hook_id"].as_str().unwrap().to_owned();
    let secret = created["secret"].as_str().unwrap().to_owned();
    assert_eq!(
        created["hook"]["urls"][0],
        format!("https://{address}/hooks/{hook}")
    );
    assert_eq!(
        created["hook"]["certificate_sha256"],
        access.certificate_fingerprint().to_hex()
    );
    // The credential store holds the secret; the agent file holds its hash.
    let file = std::fs::read_to_string(
        agents::folder(&temporary.profile, &agent.id)
            .unwrap()
            .join("agent.md"),
    )
    .unwrap();
    assert!(!file.contains(&secret));

    let body = br#"{"action":"completed","workflow_run":{"conclusion":"failure"}}"#;
    let signed = |delivery: &str, body: &[u8]| {
        vec![
            (
                "X-Hub-Signature-256",
                format!(
                    "sha256={}",
                    muniment_core::agent_hooks::signature(&secret, body)
                ),
            ),
            ("X-GitHub-Delivery", delivery.to_owned()),
            ("X-GitHub-Event", "workflow_run".to_owned()),
        ]
    };
    let fingerprint = access.certificate_fingerprint();
    let (status, answer) = post(
        &address,
        fingerprint,
        &request(&hook, &signed("one", body), body),
    );
    assert_eq!(status, 202, "{answer}");
    let run = RunStore::open(&temporary.profile)
        .unwrap()
        .get(answer["run_id"].as_str().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(run.trigger.kind, TriggerKind::Webhook);
    assert_eq!(run.agent_id, agent.id);
    assert!(run.prompt.contains("<webhook_payload preset=\"github\""));

    let (status, _) = post(
        &address,
        fingerprint,
        &request(&hook, &signed("one", body), body),
    );
    assert_eq!(status, 409, "a replayed delivery");
    let mut bad = signed("two", body);
    bad[0].1 = format!("sha256={}", "00".repeat(32));
    let (status, _) = post(&address, fingerprint, &request(&hook, &bad, body));
    assert_eq!(status, 401, "a bad signature");
    let oversize = format!(
        "POST /hooks/{hook} HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
        muniment_core::agent_hooks::MAX_PAYLOAD_BYTES + 1
    );
    let (status, _) = post(&address, fingerprint, oversize.as_bytes());
    assert_eq!(status, 413, "an oversize payload");
    let (status, _) = post(
        &address,
        fingerprint,
        &request("unknown-hook", &signed("three", body), body),
    );
    assert_eq!(status, 404);
    // An attach client still connects to the same listener.
    assert!(muniment_core::attach::connect_pinned(
        std::slice::from_ref(&address),
        fingerprint,
        CI_WAIT
    )
    .is_ok());
    drop(stop);
    server.join().unwrap().unwrap();
}

#[test]
fn a_due_mcp_routine_polls_its_tool_and_queues_a_run_when_its_condition_holds() {
    let temporary = common::TemporaryProfile::new("mcp-poll", false);
    let state = state(&temporary);
    let agent = agents::save(
        &temporary.profile,
        serde_json::from_value(json!({
            "name": "Poller", "instructions": "Watch leads.",
            "routines": [{"id": "leads", "name": "Leads", "prompt": "Summarize the leads.",
                "trigger": {"kind": "mcp", "server": "crm", "tool": "search_leads", "args": {},
                    "every": {"expr": "*/5 * * * *", "tz": "UTC"},
                    "condition": {"path": "/total", "op": "gt", "value": 0}}}]
        }))
        .unwrap(),
    )
    .unwrap();
    let totals = Arc::new(Mutex::new(vec![0, 3]));
    let call = {
        let totals = Arc::clone(&totals);
        move |_: &muniment_core::agents::McpTrigger| -> Result<Value, String> {
            let total = totals.lock().unwrap().remove(0);
            Ok(json!({"isError": false,
                "content": [{"type": "text", "text": json!({"total": total}).to_string()}]}))
        }
    };
    let due_now = || {
        agents::with_state(&temporary.profile, |saved| {
            saved
                .routines
                .entry(agent.id.clone())
                .or_default()
                .entry("leads".into())
                .or_default()
                .next_fire_at = Some(0);
            Ok(())
        })
        .unwrap();
    };
    // The first pass schedules the routine.
    assert!(state.poll_mcp_routines_with(&call).unwrap().is_empty());
    due_now();
    assert!(
        state.poll_mcp_routines_with(&call).unwrap().is_empty(),
        "total 0"
    );
    due_now();
    let runs = state.poll_mcp_routines_with(&call).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].trigger.kind, TriggerKind::Mcp);
    assert!(runs[0].trigger.kind.event_triggered());
    assert!(runs[0].prompt.contains("\"total\": 3"));
    let routine = &agents::read(&temporary.profile, &agent.id)
        .unwrap()
        .routines[0];
    assert!(routine.last_poll.as_ref().unwrap().fired);
}
