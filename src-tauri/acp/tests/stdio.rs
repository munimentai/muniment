use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

fn exchange(messages: &[Value]) -> Vec<Value> {
    exchange_with_command(Command::new(env!("CARGO_BIN_EXE_muniment-acp")), messages)
}

fn exchange_with_command(mut command: Command, messages: &[Value]) -> Vec<Value> {
    let mut child = command
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
    assert_eq!(capabilities["loadSession"], false);
    assert!(capabilities.get("modes").is_none());
    assert!(capabilities.get("configOptions").is_none());
    assert!(capabilities.get("fs").is_none());
    assert!(capabilities.get("terminal").is_none());
}

#[test]
fn session_requests_fail_without_an_authorized_attach() {
    let responses = exchange(&[initialize_request(), request(2, "session/load", json!({}))]);

    assert_eq!(responses[0]["result"]["protocolVersion"], 1);
    assert_eq!(responses[1]["id"], 2);
    assert_eq!(responses[1]["error"]["code"], -32000);
    assert_eq!(
        responses[1]["error"]["message"],
        "no authorized Muniment runtime attach exists"
    );
}

#[test]
fn prompt_rejects_an_unknown_session_without_an_attach() {
    let responses = exchange(&[request(
        1,
        "session/prompt",
        json!({
            "sessionId": "unknown",
            "prompt": [{"type": "text", "text": "Do not run this."}]
        }),
    )]);

    assert_eq!(responses[0]["error"]["code"], -32002);
    assert_eq!(responses[0]["error"]["message"], "Resource not found");
}

#[cfg(target_os = "linux")]
#[test]
fn new_session_reuses_authorization_and_rejects_invalid_parameters() {
    use muniment_attach::{
        authorized_with_client_credential, encode_frame, welcome, Id, Protocol, Response, Success,
    };
    use std::collections::BTreeMap;
    use std::io::Read;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::thread;

    fn read_frame(stream: &mut UnixStream) -> Value {
        let mut prefix = [0; 4];
        stream.read_exact(&mut prefix).unwrap();
        let mut payload = vec![0; u32::from_be_bytes(prefix) as usize];
        stream.read_exact(&mut payload).unwrap();
        serde_json::from_slice(&payload).unwrap()
    }

    let runtime = std::env::temp_dir().join(format!(
        "muniment-acp-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let socket_directory = runtime.join("muniment");
    let config = runtime.join("config");
    std::fs::create_dir_all(&socket_directory).unwrap();
    let listener = UnixListener::bind(socket_directory.join("attach-v1.sock")).unwrap();
    let workspace = runtime.join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let expected_workspace = workspace.to_string_lossy().into_owned();
    let server = thread::spawn(move || {
        let mut client_id = None;
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let hello = read_frame(&mut stream);
            assert_eq!(hello["client"]["kind"], "acp-adapter");
            if let Some(client_id) = &client_id {
                assert_eq!(hello["authorized_client_id"], *client_id);
                assert_eq!(hello["authorized_client_credential"], "44".repeat(32));
            } else {
                client_id = Some(hello["authorized_client_id"].clone());
                assert!(hello.get("authorized_client_credential").is_none());
            }
            stream
                .write_all(
                    &encode_frame(&welcome(1, "0.0.1", "11".repeat(16), "22".repeat(16))).unwrap(),
                )
                .unwrap();
            stream
                .write_all(
                    &encode_frame(&authorized_with_client_credential(
                        "33".repeat(32),
                        3600,
                        900,
                        BTreeMap::new(),
                        "44".repeat(32),
                    ))
                    .unwrap(),
                )
                .unwrap();
            let onboard = read_frame(&mut stream);
            assert_eq!(onboard["operation"], "workspace.onboard");
            assert_eq!(onboard["body"]["opened_directory"], expected_workspace);
            assert_eq!(onboard["body"]["memory_location"], expected_workspace);
            stream
                .write_all(
                    &encode_frame(&Response {
                        protocol: Protocol,
                        request_id: Id::new(onboard["request_id"].as_str().unwrap()).unwrap(),
                        ok: Success,
                        body: json!({
                            "opened_directory": expected_workspace,
                            "memory_location": expected_workspace,
                            "instructions": null
                        }),
                    })
                    .unwrap(),
                )
                .unwrap();
        }
    });

    let messages = [
        request(
            1,
            "session/new",
            json!({"cwd": workspace, "mcpServers": []}),
        ),
        request(
            2,
            "session/new",
            json!({"cwd": "relative/workspace", "mcpServers": []}),
        ),
        request(
            3,
            "session/new",
            json!({
                "cwd": "/workspace",
                "mcpServers": [{"name": "tools", "command": "secret-command"}]
            }),
        ),
    ];
    let mut first = Command::new(env!("CARGO_BIN_EXE_muniment-acp"));
    first
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_CONFIG_HOME", &config);
    let responses = exchange_with_command(first, &messages);
    let mut second = Command::new(env!("CARGO_BIN_EXE_muniment-acp"));
    second
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_CONFIG_HOME", &config);
    let second_responses = exchange_with_command(second, &messages[..1]);
    server.join().unwrap();

    let client_files = config.join("muniment");
    for name in ["acp-client-id", "acp-client-credential"] {
        let metadata = std::fs::metadata(client_files.join(name)).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    }
    assert!(!client_files.join("cli-client-id").exists());
    assert!(!client_files.join("cli-client-credential").exists());
    let session_id = responses[0]["result"]["sessionId"].as_str().unwrap();
    assert_eq!(
        uuid::Uuid::parse_str(session_id).unwrap().get_version_num(),
        7
    );
    assert!(second_responses[0]["result"]["sessionId"].is_string());
    assert_eq!(
        responses[1]["error"]["message"],
        "session/new cwd must be absolute"
    );
    assert_eq!(
        responses[2]["error"]["message"],
        "session/new does not accept MCP servers"
    );
    let failures = format!("{} {}", responses[1], responses[2]);
    assert!(!failures.contains(runtime.to_string_lossy().as_ref()));
    assert!(!failures.contains("secret-command"));
    std::fs::remove_dir_all(runtime).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn consecutive_prompts_continue_one_thread() {
    use muniment_attach::{
        authorized_with_client_credential, encode_frame, welcome, ErrorEnvelope, Event, EventName,
        Failure, Id, Protocol, ProtocolError, Response, Success,
    };
    use std::collections::BTreeMap;
    use std::io::{BufRead, BufReader, Read};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::thread;

    fn read_frame(stream: &mut UnixStream) -> Value {
        let mut prefix = [0; 4];
        stream.read_exact(&mut prefix).unwrap();
        let mut payload = vec![0; u32::from_be_bytes(prefix) as usize];
        stream.read_exact(&mut payload).unwrap();
        serde_json::from_slice(&payload).unwrap()
    }

    fn respond(stream: &mut UnixStream, request: &Value, body: Value) {
        stream
            .write_all(
                &encode_frame(&Response {
                    protocol: Protocol,
                    request_id: Id::new(request["request_id"].as_str().unwrap()).unwrap(),
                    ok: Success,
                    body,
                })
                .unwrap(),
            )
            .unwrap();
    }

    fn pair(stream: &mut UnixStream) {
        let hello = read_frame(stream);
        assert_eq!(hello["client"]["kind"], "acp-adapter");
        stream
            .write_all(
                &encode_frame(&welcome(1, "0.0.1", "11".repeat(16), "22".repeat(16))).unwrap(),
            )
            .unwrap();
        stream
            .write_all(
                &encode_frame(&authorized_with_client_credential(
                    "33".repeat(32),
                    3600,
                    900,
                    BTreeMap::new(),
                    "44".repeat(32),
                ))
                .unwrap(),
            )
            .unwrap();
    }

    let runtime = std::env::temp_dir().join(format!(
        "muniment-acp-prompt-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let socket_directory = runtime.join("muniment");
    let config = runtime.join("config");
    let workspace = runtime.join("workspace");
    std::fs::create_dir_all(&socket_directory).unwrap();
    std::fs::create_dir(&workspace).unwrap();
    let listener = UnixListener::bind(socket_directory.join("attach-v1.sock")).unwrap();
    let expected_workspace = workspace.to_string_lossy().into_owned();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        pair(&mut stream);
        let onboard = read_frame(&mut stream);
        assert_eq!(onboard["operation"], "workspace.onboard");
        respond(
            &mut stream,
            &onboard,
            json!({
                "opened_directory": expected_workspace,
                "memory_location": expected_workspace,
                "instructions": null
            }),
        );

        let (mut stream, _) = listener.accept().unwrap();
        pair(&mut stream);
        let start = read_frame(&mut stream);
        assert_eq!(start["operation"], "run.start");
        assert_eq!(start["body"]["workspace"], expected_workspace);
        assert_eq!(start["body"]["text"], "Tell me more.");
        assert!(start["body"].get("thread_id").is_none());
        let run_id = "01900000-0000-7000-8000-000000000001";
        let thread_id = "01900000-0000-7000-8000-000000000003";
        let subscription_id = "01900000-0000-7000-8000-000000000002";
        respond(
            &mut stream,
            &start,
            json!({
                "run_id": run_id,
                "thread_id": thread_id,
                "committed_seq": 1,
                "accepted_at": "2026-08-03T00:00:00Z"
            }),
        );
        let subscribe = read_frame(&mut stream);
        assert_eq!(subscribe["operation"], "run.stream");
        assert_eq!(subscribe["body"]["after_run_seq"], 1);
        respond(
            &mut stream,
            &subscribe,
            json!({
                "subscription_id": subscription_id,
                "run_id": run_id,
                "first_available_run_seq": 2,
                "current_run_seq": 6,
                "window": {"max_events": 16, "max_bytes": 1048576, "max_text_bytes": 262144}
            }),
        );

        let events = [
            (
                EventName::RunEvent,
                2,
                json!({
                    "event_type": "model.stream.delta", "event_version": 1,
                    "recorded_at": "2026-08-03T00:00:01Z",
                    "payload": {"withheld": false, "text": "First "}
                }),
            ),
            (
                EventName::PermissionPending,
                3,
                json!({
                    "gate_id": "gate-1", "kind": "confirm", "title": "Allow access?"
                }),
            ),
            (
                EventName::RunEvent,
                4,
                json!({
                    "event_type": "model.stream.delta", "event_version": 1,
                    "recorded_at": "2026-08-03T00:00:02Z", "payload": {"withheld": true}
                }),
            ),
            (
                EventName::RunEvent,
                5,
                json!({
                    "event_type": "model.stream.delta", "event_version": 1,
                    "recorded_at": "2026-08-03T00:00:03Z", "payload": {"withheld": false, "text": "reply"}
                }),
            ),
            (
                EventName::RunEvent,
                6,
                json!({
                    "event_type": "run.completed", "event_version": 1,
                    "recorded_at": "2026-08-03T00:00:04Z", "payload": {"withheld": true}
                }),
            ),
        ];
        for (kind, sequence, body) in events {
            stream
                .write_all(
                    &encode_frame(&Event {
                        protocol: Protocol,
                        subscription_id: Id::new(subscription_id).unwrap(),
                        event: kind,
                        run_id: Some(Id::new(run_id).unwrap()),
                        run_seq: Some(sequence),
                        body,
                    })
                    .unwrap(),
                )
                .unwrap();
        }

        for sequence in 2..=6 {
            if sequence == 3 {
                let answer = read_frame(&mut stream);
                assert_eq!(answer["operation"], "permission.answer");
                assert_eq!(answer["body"]["decision"], "deny");
                respond(
                    &mut stream,
                    &answer,
                    json!({
                        "run_id": run_id, "gate_id": "gate-1", "decision": "deny",
                        "committed_seq": 6, "accepted_at": "2026-08-03T00:00:04Z"
                    }),
                );
            }
            let acknowledgement = read_frame(&mut stream);
            assert_eq!(acknowledgement["operation"], "run.cursor_ack");
            assert_eq!(acknowledgement["body"]["through_run_seq"], sequence);
            respond(
                &mut stream,
                &acknowledgement,
                json!({"subscription_id": subscription_id, "through_run_seq": sequence}),
            );
        }

        let (mut stream, _) = listener.accept().unwrap();
        pair(&mut stream);
        let start = read_frame(&mut stream);
        assert_eq!(start["operation"], "run.start");
        assert_eq!(start["body"]["workspace"], expected_workspace);
        assert_eq!(start["body"]["text"], "Continue.");
        assert_eq!(start["body"]["thread_id"], thread_id);
        stream
            .write_all(
                &encode_frame(&ErrorEnvelope {
                    protocol: Protocol,
                    request_id: Some(Id::new(start["request_id"].as_str().unwrap()).unwrap()),
                    ok: Failure,
                    error: ProtocolError::thread_not_found(),
                })
                .unwrap(),
            )
            .unwrap();
        let retry = read_frame(&mut stream);
        assert_eq!(retry["operation"], "run.start");
        assert_ne!(retry["request_id"], start["request_id"]);
        assert_ne!(retry["idempotency_key"], start["idempotency_key"]);
        assert!(retry["body"].get("thread_id").is_none());
        let run_id = "01900000-0000-7000-8000-000000000004";
        let replacement_thread_id = "0190a100-0000-7000-8000-000000000002";
        let subscription_id = "01900000-0000-7000-8000-000000000005";
        respond(
            &mut stream,
            &retry,
            json!({
                "run_id": run_id,
                "thread_id": replacement_thread_id,
                "committed_seq": 1,
                "accepted_at": "2026-08-03T00:01:00Z"
            }),
        );
        let subscribe = read_frame(&mut stream);
        assert_eq!(subscribe["operation"], "run.stream");
        respond(
            &mut stream,
            &subscribe,
            json!({
                "subscription_id": subscription_id,
                "run_id": run_id,
                "first_available_run_seq": 2,
                "current_run_seq": 2,
                "window": {"max_events": 16, "max_bytes": 1048576, "max_text_bytes": 262144}
            }),
        );
        stream
            .write_all(
                &encode_frame(&Event {
                    protocol: Protocol,
                    subscription_id: Id::new(subscription_id).unwrap(),
                    event: EventName::RunEvent,
                    run_id: Some(Id::new(run_id).unwrap()),
                    run_seq: Some(2),
                    body: json!({
                        "event_type": "run.completed", "event_version": 1,
                        "recorded_at": "2026-08-03T00:01:01Z", "payload": {"withheld": true}
                    }),
                })
                .unwrap(),
            )
            .unwrap();
        let acknowledgement = read_frame(&mut stream);
        assert_eq!(acknowledgement["operation"], "run.cursor_ack");
        assert_eq!(acknowledgement["body"]["through_run_seq"], 2);
        respond(
            &mut stream,
            &acknowledgement,
            json!({"subscription_id": subscription_id, "through_run_seq": 2}),
        );

        let (mut stream, _) = listener.accept().unwrap();
        pair(&mut stream);
        let start = read_frame(&mut stream);
        assert_eq!(start["operation"], "run.start");
        assert_eq!(start["body"]["thread_id"], replacement_thread_id);
        stream
            .write_all(
                &encode_frame(&ErrorEnvelope {
                    protocol: Protocol,
                    request_id: Some(Id::new(start["request_id"].as_str().unwrap()).unwrap()),
                    ok: Failure,
                    error: ProtocolError::invalid_request(),
                })
                .unwrap(),
            )
            .unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_millis(250)))
            .unwrap();
        let mut prefix = [0; 4];
        assert!(stream.read_exact(&mut prefix).is_err());
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_muniment-acp"))
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_CONFIG_HOME", &config)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    serde_json::to_writer(
        &mut input,
        &request(
            1,
            "session/new",
            json!({"cwd": workspace, "mcpServers": []}),
        ),
    )
    .unwrap();
    input.write_all(b"\n").unwrap();
    input.flush().unwrap();
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    let created: Value = serde_json::from_str(&line).unwrap();
    let session_id = created["result"]["sessionId"].as_str().unwrap();
    serde_json::to_writer(
        &mut input,
        &request(
            2,
            "session/prompt",
            json!({"sessionId": session_id, "prompt": [{"type": "text", "text": "Tell me more."}]}),
        ),
    )
    .unwrap();
    input.write_all(b"\n").unwrap();
    input.flush().unwrap();
    let mut responses: Vec<Value> = Vec::new();
    for _ in 0..3 {
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        responses.push(serde_json::from_str(&line).unwrap());
    }
    serde_json::to_writer(
        &mut input,
        &request(
            3,
            "session/prompt",
            json!({"sessionId": session_id, "prompt": [{"type": "text", "text": "Continue."}]}),
        ),
    )
    .unwrap();
    input.write_all(b"\n").unwrap();
    input.flush().unwrap();
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    responses.push(serde_json::from_str(&line).unwrap());
    serde_json::to_writer(
        &mut input,
        &request(
            4,
            "session/prompt",
            json!({"sessionId": session_id, "prompt": [{"type": "text", "text": "Try again."}]}),
        ),
    )
    .unwrap();
    input.write_all(b"\n").unwrap();
    drop(input);
    responses.extend(
        output
            .lines()
            .map(|line| serde_json::from_str(&line.unwrap()).unwrap()),
    );
    assert_eq!(responses.len(), 5);
    assert_eq!(responses[0]["method"], "session/update");
    assert_eq!(
        responses[0]["params"]["update"]["content"]["text"],
        "First "
    );
    assert_eq!(responses[1]["params"]["update"]["content"]["text"], "reply");
    assert_eq!(responses[2]["id"], 2);
    assert_eq!(responses[2]["result"]["stopReason"], "end_turn");
    assert_eq!(responses[3]["id"], 3);
    assert_eq!(responses[3]["result"]["stopReason"], "end_turn");
    assert_eq!(responses[4]["id"], 4);
    assert_eq!(responses[4]["error"]["code"], -32000);
    assert!(child.wait().unwrap().success());
    server.join().unwrap();
    std::fs::remove_dir_all(runtime).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn serves_a_line_received_during_a_prompt_after_the_prompt_returns() {
    use muniment_attach::{
        authorized_with_client_credential, encode_frame, welcome, Event, EventName, Id, Protocol,
        Response, Success,
    };
    use std::collections::BTreeMap;
    use std::io::{BufRead, BufReader, Read};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::sync::mpsc;
    use std::thread;

    fn read_frame(stream: &mut UnixStream) -> Value {
        let mut prefix = [0; 4];
        stream.read_exact(&mut prefix).unwrap();
        let mut payload = vec![0; u32::from_be_bytes(prefix) as usize];
        stream.read_exact(&mut payload).unwrap();
        serde_json::from_slice(&payload).unwrap()
    }

    fn respond(stream: &mut UnixStream, request: &Value, body: Value) {
        stream
            .write_all(
                &encode_frame(&Response {
                    protocol: Protocol,
                    request_id: Id::new(request["request_id"].as_str().unwrap()).unwrap(),
                    ok: Success,
                    body,
                })
                .unwrap(),
            )
            .unwrap();
    }

    fn pair(stream: &mut UnixStream) {
        read_frame(stream);
        stream
            .write_all(
                &encode_frame(&welcome(1, "0.0.1", "11".repeat(16), "22".repeat(16))).unwrap(),
            )
            .unwrap();
        stream
            .write_all(
                &encode_frame(&authorized_with_client_credential(
                    "33".repeat(32),
                    3600,
                    900,
                    BTreeMap::new(),
                    "44".repeat(32),
                ))
                .unwrap(),
            )
            .unwrap();
    }

    let runtime = std::env::temp_dir().join(format!(
        "muniment-acp-mid-prompt-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let socket_directory = runtime.join("muniment");
    let config = runtime.join("config");
    let workspace = runtime.join("workspace");
    std::fs::create_dir_all(&socket_directory).unwrap();
    std::fs::create_dir(&workspace).unwrap();
    let listener = UnixListener::bind(socket_directory.join("attach-v1.sock")).unwrap();
    let expected_workspace = workspace.to_string_lossy().into_owned();
    let (prompt_started_tx, prompt_started_rx) = mpsc::channel();
    let (finish_prompt_tx, finish_prompt_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        pair(&mut stream);
        let onboard = read_frame(&mut stream);
        respond(
            &mut stream,
            &onboard,
            json!({
                "opened_directory": expected_workspace,
                "memory_location": expected_workspace,
                "instructions": null
            }),
        );

        let (mut stream, _) = listener.accept().unwrap();
        pair(&mut stream);
        let start = read_frame(&mut stream);
        let run_id = "01900000-0000-7000-8000-000000000011";
        let subscription_id = "01900000-0000-7000-8000-000000000012";
        respond(
            &mut stream,
            &start,
            json!({
                "run_id": run_id,
                "thread_id": "01900000-0000-7000-8000-000000000013",
                "committed_seq": 1,
                "accepted_at": "2026-08-03T00:00:00Z"
            }),
        );
        let subscribe = read_frame(&mut stream);
        respond(
            &mut stream,
            &subscribe,
            json!({
                "subscription_id": subscription_id,
                "run_id": run_id,
                "first_available_run_seq": 2,
                "current_run_seq": 2,
                "window": {"max_events": 16, "max_bytes": 1048576, "max_text_bytes": 262144}
            }),
        );
        prompt_started_tx.send(()).unwrap();
        finish_prompt_rx.recv().unwrap();
        stream
            .write_all(
                &encode_frame(&Event {
                    protocol: Protocol,
                    subscription_id: Id::new(subscription_id).unwrap(),
                    event: EventName::RunEvent,
                    run_id: Some(Id::new(run_id).unwrap()),
                    run_seq: Some(2),
                    body: json!({
                        "event_type": "run.completed",
                        "event_version": 1,
                        "recorded_at": "2026-08-03T00:00:01Z",
                        "payload": {"withheld": true}
                    }),
                })
                .unwrap(),
            )
            .unwrap();
        let acknowledgement = read_frame(&mut stream);
        respond(
            &mut stream,
            &acknowledgement,
            json!({"subscription_id": subscription_id, "through_run_seq": 2}),
        );
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_muniment-acp"))
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_CONFIG_HOME", &config)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    serde_json::to_writer(
        &mut input,
        &request(
            1,
            "session/new",
            json!({"cwd": workspace, "mcpServers": []}),
        ),
    )
    .unwrap();
    input.write_all(b"\n").unwrap();
    input.flush().unwrap();
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    let created: Value = serde_json::from_str(&line).unwrap();
    serde_json::to_writer(
        &mut input,
        &request(
            2,
            "session/prompt",
            json!({
                "sessionId": created["result"]["sessionId"],
                "prompt": [{"type": "text", "text": "Wait for another line."}]
            }),
        ),
    )
    .unwrap();
    input.write_all(b"\n").unwrap();
    input.flush().unwrap();
    prompt_started_rx.recv().unwrap();
    serde_json::to_writer(&mut input, &initialize_request()).unwrap();
    input.write_all(b"\n").unwrap();
    input.flush().unwrap();
    finish_prompt_tx.send(()).unwrap();
    drop(input);

    let responses: Vec<Value> = output
        .lines()
        .map(|line| serde_json::from_str(&line.unwrap()).unwrap())
        .collect();
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["id"], 2);
    assert_eq!(responses[0]["result"]["stopReason"], "end_turn");
    assert_eq!(responses[1]["id"], 1);
    assert_eq!(responses[1]["result"]["protocolVersion"], 1);
    assert!(child.wait().unwrap().success());
    server.join().unwrap();
    std::fs::remove_dir_all(runtime).unwrap();
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
