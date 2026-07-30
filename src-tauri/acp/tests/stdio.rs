use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

fn exchange(messages: &[Value]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_muniment-acp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("adapter starts");
    {
        let mut input = child.stdin.take().expect("adapter stdin");
        for message in messages {
            serde_json::to_writer(&mut input, message).expect("request writes");
            input.write_all(b"\n").expect("newline writes");
        }
    }
    let output = child.wait_with_output().expect("adapter exits");
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .expect("UTF-8 output")
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON response"))
        .collect()
}

fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

fn initialize_request() -> Value {
    request(
        1,
        "initialize",
        json!({"protocolVersion": 1, "clientCapabilities": {}}),
    )
}

#[test]
fn initializes_with_only_the_supported_capabilities() {
    let responses = exchange(&[request(
        1,
        "initialize",
        json!({
            "protocolVersion": 1,
            "clientCapabilities": {
                "fs": {"readTextFile": true, "writeTextFile": true},
                "terminal": true
            }
        }),
    )]);

    assert_eq!(responses.len(), 1);
    let response = &responses[0];
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["protocolVersion"], 1);
    let capabilities = &response["result"]["agentCapabilities"];
    assert_eq!(capabilities["loadSession"], true);
    assert!(capabilities.get("modes").is_none());
    assert!(capabilities.get("configOptions").is_none());
    assert!(capabilities.get("fs").is_none());
    assert!(capabilities.get("terminal").is_none());
}

#[test]
fn session_requests_fail_without_an_authorized_attach() {
    let responses = exchange(&[
        initialize_request(),
        request(2, "session/new", json!({})),
        request(3, "session/load", json!({})),
        request(4, "session/prompt", json!({})),
    ]);

    assert_eq!(responses[0]["result"]["protocolVersion"], 1);
    for (response, id) in responses[1..].iter().zip(2..=4) {
        assert_eq!(response["id"], id);
        assert_eq!(response["error"]["code"], -32000);
        assert_eq!(
            response["error"]["message"],
            "no authorized Muniment runtime attach exists"
        );
    }
}

#[test]
fn cancel_and_unknown_notifications_have_no_effect() {
    let responses = exchange(&[
        initialize_request(),
        json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {}}),
        json!({"jsonrpc": "2.0", "method": "unknown", "params": {}}),
    ]);

    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["result"]["protocolVersion"], 1);
}

#[test]
fn unknown_request_returns_method_not_found() {
    let responses = exchange(&[initialize_request(), request(7, "unknown", json!({}))]);

    assert_eq!(
        responses[1],
        json!({
            "jsonrpc": "2.0",
            "id": 7,
            "error": {"code": -32601, "message": "Method not found"}
        })
    );
}
