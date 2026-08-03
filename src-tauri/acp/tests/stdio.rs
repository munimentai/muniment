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
    assert_eq!(capabilities["loadSession"], true);
    assert!(capabilities.get("modes").is_none());
    assert!(capabilities.get("configOptions").is_none());
    assert!(capabilities.get("fs").is_none());
    assert!(capabilities.get("terminal").is_none());
}

#[test]
fn load_rejects_an_unknown_session_without_an_attach() {
    let responses = exchange(&[
        initialize_request(),
        request(
            2,
            "session/load",
            json!({
                "sessionId": "01900000-0000-7000-8000-000000000099",
                "cwd": "/workspace",
                "mcpServers": []
            }),
        ),
    ]);

    assert_eq!(responses[0]["result"]["protocolVersion"], 1);
    assert_eq!(responses[1]["id"], 2);
    assert_eq!(responses[1]["error"]["code"], -32002);
    assert_eq!(responses[1]["error"]["message"], "Resource not found");
    assert!(responses
        .iter()
        .all(|response| response.get("method").is_none()));
}

#[cfg(target_os = "linux")]
#[test]
fn load_rejects_relative_cwd_without_updates() {
    let root = std::env::temp_dir().join(format!(
        "muniment-acp-invalid-load-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let config = root.join("config");
    let session_id = "01900000-0000-7000-8000-000000000012";
    let records = config.join("muniment/acp-sessions");
    std::fs::create_dir_all(&records).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_muniment-acp"));
    command.env("XDG_CONFIG_HOME", &config);
    let responses = exchange_with_command(
        command,
        &[request(
            1,
            "session/load",
            json!({"sessionId": session_id, "cwd": "relative", "mcpServers": []}),
        )],
    );

    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["error"]["code"], -32602);
    assert_eq!(
        responses[0]["error"]["message"],
        "session/load cwd must be absolute"
    );
    assert!(responses
        .iter()
        .all(|response| response.get("method").is_none()));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(target_os = "linux")]
fn rejected_record_load(record_contents: &[u8]) -> Vec<Value> {
    use std::os::unix::fs::PermissionsExt;

    let root = std::env::temp_dir().join(format!(
        "muniment-acp-invalid-record-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let config = root.join("config");
    let session_id = "01900000-0000-7000-8000-000000000012";
    let records = config.join("muniment/acp-sessions");
    std::fs::create_dir_all(&records).unwrap();
    let record = records.join(format!("{session_id}.json"));
    std::fs::write(&record, record_contents).unwrap();
    std::fs::set_permissions(&record, std::fs::Permissions::from_mode(0o600)).unwrap();

    let mut command = Command::new(env!("CARGO_BIN_EXE_muniment-acp"));
    command.env("XDG_CONFIG_HOME", &config);
    let responses = exchange_with_command(
        command,
        &[request(
            1,
            "session/load",
            json!({"sessionId": session_id, "cwd": "/workspace", "mcpServers": []}),
        )],
    );

    std::fs::remove_dir_all(root).unwrap();
    responses
}

#[cfg(target_os = "linux")]
fn assert_record_load_not_found_without_updates(responses: &[Value]) {
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["error"]["code"], -32002);
    assert_eq!(responses[0]["error"]["message"], "Resource not found");
    assert!(responses
        .iter()
        .all(|response| response.get("method").is_none()));
}

#[cfg(target_os = "linux")]
#[test]
fn load_rejects_a_malformed_record_without_updates() {
    assert_record_load_not_found_without_updates(&rejected_record_load(b"not JSON"));
}

#[cfg(target_os = "linux")]
#[test]
fn load_rejects_an_invalid_record_without_updates() {
    assert_record_load_not_found_without_updates(&rejected_record_load(
        br#"{
            "version": 2,
            "session_id": "01900000-0000-7000-8000-000000000012",
            "workspace_root": "/workspace",
            "thread_id": "01900000-0000-7000-8000-000000000013",
            "profile_id": "profile-id"
        }"#,
    ));
}

#[cfg(target_os = "linux")]
fn rejected_scripted_load(case: &str) -> Vec<Value> {
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

    let root = std::env::temp_dir().join(format!(
        "muniment-acp-rejected-load-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let socket_directory = root.join("muniment");
    let config = root.join("config");
    let workspace = root.join("workspace");
    std::fs::create_dir_all(&socket_directory).unwrap();
    std::fs::create_dir(&workspace).unwrap();
    let listener = UnixListener::bind(socket_directory.join("attach-v1.sock")).unwrap();
    let session_id = "01900000-0000-7000-8000-000000000020";
    let thread_id = "01900000-0000-7000-8000-000000000021";
    let record_directory = config.join("muniment/acp-sessions");
    std::fs::create_dir_all(&record_directory).unwrap();
    let record_path = record_directory.join(format!("{session_id}.json"));
    std::fs::write(
        &record_path,
        serde_json::to_vec(&json!({
            "version": 1,
            "session_id": session_id,
            "workspace_root": workspace,
            "thread_id": thread_id,
            "profile_id": "profile-id"
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::set_permissions(&record_path, std::fs::Permissions::from_mode(0o600)).unwrap();

    let expected_workspace = workspace.to_string_lossy().into_owned();
    let case = case.to_owned();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _hello = read_frame(&mut stream);
        stream
            .write_all(
                &encode_frame(&welcome(1, "0.0.1", "11".repeat(16), "22".repeat(16))).unwrap(),
            )
            .unwrap();
        let profile_id = if case == "profile" {
            "other-profile"
        } else {
            "profile-id"
        };
        stream
            .write_all(
                &encode_frame(&authorized_with_client_credential(
                    profile_id,
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
        let opened_directory = if case == "workspace" {
            format!("{expected_workspace}-other")
        } else {
            expected_workspace.clone()
        };
        respond(
            &mut stream,
            &onboard,
            json!({
                "opened_directory": opened_directory,
                "memory_location": expected_workspace,
                "instructions": null
            }),
        );
        if case == "workspace" || case == "profile" {
            return;
        }
        let open = read_frame(&mut stream);
        if case == "page" {
            return;
        }
        respond(
            &mut stream,
            &open,
            json!({
                "thread_id": thread_id,
                "entries": [{
                    "run_seq": 1,
                    "kind": if case == "effect" { "tool_running:not-a-uuid" } else { "tool_call" },
                    "text": "hidden"
                }]
            }),
        );
    });

    let mut command = Command::new(env!("CARGO_BIN_EXE_muniment-acp"));
    command
        .env("XDG_RUNTIME_DIR", &root)
        .env("XDG_CONFIG_HOME", &config);
    let responses = exchange_with_command(
        command,
        &[request(
            1,
            "session/load",
            json!({"sessionId": session_id, "cwd": workspace, "mcpServers": []}),
        )],
    );
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
    responses
}

#[cfg(target_os = "linux")]
fn assert_load_failed_without_updates(responses: &[Value], message: &str) {
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["error"]["code"], -32000);
    assert_eq!(responses[0]["error"]["message"], message);
    assert!(responses
        .iter()
        .all(|response| response.get("method").is_none()));
}

#[cfg(target_os = "linux")]
#[test]
fn load_rejects_a_workspace_mismatch_without_updates() {
    assert_load_failed_without_updates(
        &rejected_scripted_load("workspace"),
        "Muniment runtime sent an invalid pairing message",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn load_rejects_a_profile_mismatch_without_updates() {
    assert_load_failed_without_updates(
        &rejected_scripted_load("profile"),
        "Muniment runtime sent an invalid pairing message",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn load_rejects_an_unknown_entry_kind_without_updates() {
    assert_load_failed_without_updates(
        &rejected_scripted_load("entry"),
        "Muniment runtime sent an invalid pairing message",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn load_rejects_an_invalid_tool_effect_id_without_updates() {
    assert_load_failed_without_updates(
        &rejected_scripted_load("effect"),
        "Muniment runtime sent an invalid pairing message",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn load_rejects_a_page_failure_without_updates() {
    assert_load_failed_without_updates(
        &rejected_scripted_load("page"),
        "Muniment runtime pairing was denied or closed",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn load_replays_all_entry_kinds_across_pages_before_responding() {
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

    let runtime = std::env::temp_dir().join(format!(
        "muniment-acp-load-{}-{}",
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
    let session_id = "01900000-0000-7000-8000-000000000010";
    let thread_id = "01900000-0000-7000-8000-000000000011";
    let record_directory = config.join("muniment/acp-sessions");
    std::fs::create_dir_all(&record_directory).unwrap();
    let record_path = record_directory.join(format!("{session_id}.json"));
    std::fs::write(
        &record_path,
        serde_json::to_vec(&json!({
            "version": 1,
            "session_id": session_id,
            "workspace_root": workspace,
            "thread_id": thread_id,
            "profile_id": "profile-id"
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::set_permissions(&record_path, std::fs::Permissions::from_mode(0o600)).unwrap();

    let expected_workspace = workspace.to_string_lossy().into_owned();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let hello = read_frame(&mut stream);
        assert_eq!(hello["client"]["kind"], "acp-adapter");
        stream
            .write_all(
                &encode_frame(&welcome(1, "0.0.1", "11".repeat(16), "22".repeat(16))).unwrap(),
            )
            .unwrap();
        stream
            .write_all(
                &encode_frame(&authorized_with_client_credential(
                    "profile-id",
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
        respond(
            &mut stream,
            &onboard,
            json!({
                "opened_directory": expected_workspace,
                "memory_location": expected_workspace,
                "instructions": null
            }),
        );
        let first = read_frame(&mut stream);
        assert_eq!(first["operation"], "thread.open");
        assert!(first["body"].get("cursor").is_none());
        respond(
            &mut stream,
            &first,
            json!({
                "thread_id": thread_id,
                "entries": [
                    {"run_seq": 1, "kind": "user_message", "text": "Question"},
                    {"run_seq": 2, "kind": "assistant_message", "text": "Answer"},
                    {"run_seq": 3, "kind": "tool_running:01900000-0000-7000-8000-000000000040", "text": "Read file"},
                    {"run_seq": 4, "kind": "tool_completed", "text": "Ran command"}
                ],
                "next_cursor": "page-2"
            }),
        );
        let second = read_frame(&mut stream);
        assert_eq!(second["body"]["cursor"], "page-2");
        respond(
            &mut stream,
            &second,
            json!({
                "thread_id": thread_id,
                "entries": [
                    {"run_seq": 5, "kind": "attachment", "text": "report.txt"},
                    {"run_seq": 6, "kind": "tool_failed"},
                    {"run_seq": 7, "kind": "permission_pending", "text": "Allow write"},
                    {"run_seq": 8, "kind": "assistant_message"}
                ]
            }),
        );
    });

    let mut command = Command::new(env!("CARGO_BIN_EXE_muniment-acp"));
    command
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_CONFIG_HOME", &config);
    let responses = exchange_with_command(
        command,
        &[
            initialize_request(),
            request(
                2,
                "session/load",
                json!({"sessionId": session_id, "cwd": workspace, "mcpServers": []}),
            ),
        ],
    );
    server.join().unwrap();

    assert_eq!(responses.len(), 10);
    assert_eq!(
        responses[0]["result"]["agentCapabilities"]["loadSession"],
        true
    );
    assert_eq!(
        responses[1]["params"]["update"],
        json!({
            "sessionUpdate": "user_message_chunk",
            "content": {"type": "text", "text": "Question"}
        })
    );
    assert_eq!(
        responses[2]["params"]["update"],
        json!({
            "sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "Answer"}
        })
    );
    assert_eq!(
        responses[3]["params"]["update"],
        json!({
            "sessionUpdate": "tool_call",
            "toolCallId": format!("{session_id}:2"),
            "title": "Read file"
        })
    );
    assert_eq!(
        responses[4]["params"]["update"],
        json!({
            "sessionUpdate": "tool_call",
            "toolCallId": format!("{session_id}:3"),
            "status": "completed",
            "title": "Ran command"
        })
    );
    assert_eq!(
        responses[5]["params"]["update"],
        json!({
            "sessionUpdate": "user_message_chunk",
            "content": {"type": "text", "text": "report.txt"}
        })
    );
    assert_eq!(
        responses[6]["params"]["update"],
        json!({
            "sessionUpdate": "tool_call",
            "toolCallId": format!("{session_id}:5"),
            "status": "failed",
            "title": ""
        })
    );
    assert_eq!(
        responses[7]["params"]["update"],
        json!({
            "sessionUpdate": "tool_call",
            "toolCallId": format!("{session_id}:6"),
            "title": "Allow write"
        })
    );
    assert_eq!(
        responses[8]["params"]["update"],
        json!({
            "sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": ""}
        })
    );
    assert_eq!(responses[9]["result"], json!({}));
    assert!(responses[1..9]
        .iter()
        .all(|response| response["method"] == "session/update"));
    assert_eq!(
        [3, 4, 6, 7].map(|index| responses[index]["params"]["update"]["status"]
            .as_str()
            .unwrap_or("pending")),
        ["pending", "completed", "failed", "pending"]
    );
    assert!(responses
        .iter()
        .all(|response| response["method"] != "session/request_permission"));
    std::fs::remove_dir_all(runtime).unwrap();
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
fn scripted_prompt_ending(event_type: &str, stream_resumable: Option<bool>) -> Vec<Value> {
    use muniment_attach::{
        authorized_with_client_credential, encode_frame, welcome, Event, EventName, Id, Protocol,
        Response, Success,
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

    let root = std::env::temp_dir().join(format!(
        "muniment-acp-ending-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let runtime = root.join("runtime");
    let config = root.join("config");
    let socket_directory = runtime.join("muniment");
    let records = config.join("muniment/acp-sessions");
    std::fs::create_dir_all(&socket_directory).unwrap();
    std::fs::create_dir_all(&records).unwrap();
    let session_id = "01900000-0000-7000-8000-000000000021";
    let thread_id = "01900000-0000-7000-8000-000000000022";
    let record = records.join(format!("{session_id}.json"));
    std::fs::write(
        &record,
        serde_json::to_vec(&json!({
            "version": 1,
            "session_id": session_id,
            "workspace_root": "/workspace",
            "thread_id": thread_id,
            "profile_id": "profile-id"
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::set_permissions(&record, std::fs::Permissions::from_mode(0o600)).unwrap();
    let listener = UnixListener::bind(socket_directory.join("attach-v1.sock")).unwrap();
    let event_type = event_type.to_owned();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _hello = read_frame(&mut stream);
        stream
            .write_all(
                &encode_frame(&welcome(1, "0.0.1", "11".repeat(16), "22".repeat(16))).unwrap(),
            )
            .unwrap();
        stream
            .write_all(
                &encode_frame(&authorized_with_client_credential(
                    "profile-id",
                    "33".repeat(32),
                    3600,
                    900,
                    BTreeMap::new(),
                    "44".repeat(32),
                ))
                .unwrap(),
            )
            .unwrap();
        let start = read_frame(&mut stream);
        let run_id = "01900000-0000-7000-8000-000000000023";
        respond(
            &mut stream,
            &start,
            json!({
                "run_id": run_id, "thread_id": thread_id, "committed_seq": 1,
                "accepted_at": "2026-08-03T00:00:00Z"
            }),
        );
        let subscribe = read_frame(&mut stream);
        let first_subscription = "01900000-0000-7000-8000-000000000024";
        respond(
            &mut stream,
            &subscribe,
            json!({
                "subscription_id": first_subscription, "run_id": run_id,
                "first_available_run_seq": 2, "current_run_seq": 2,
                "window": {"max_events": 16, "max_bytes": 1048576, "max_text_bytes": 262144}
            }),
        );

        if let Some(resumable) = stream_resumable {
            stream
                .write_all(
                    &encode_frame(&Event {
                        protocol: Protocol,
                        subscription_id: Id::new(first_subscription).unwrap(),
                        event: EventName::StreamClosed,
                        run_id: Some(Id::new(run_id).unwrap()),
                        run_seq: Some(1),
                        body: json!({"code": "closed", "resumable": resumable}),
                    })
                    .unwrap(),
                )
                .unwrap();
            if resumable {
                let resumed = read_frame(&mut stream);
                assert_eq!(resumed["operation"], "run.stream");
                assert_eq!(resumed["body"]["after_run_seq"], 1);
                let resumed_subscription = "01900000-0000-7000-8000-000000000025";
                respond(
                    &mut stream,
                    &resumed,
                    json!({
                        "subscription_id": resumed_subscription, "run_id": run_id,
                        "first_available_run_seq": 2, "current_run_seq": 2,
                        "window": {"max_events": 16, "max_bytes": 1048576, "max_text_bytes": 262144}
                    }),
                );
                stream
                    .write_all(
                        &encode_frame(&Event {
                            protocol: Protocol,
                            subscription_id: Id::new(resumed_subscription).unwrap(),
                            event: EventName::RunEvent,
                            run_id: Some(Id::new(run_id).unwrap()),
                            run_seq: Some(2),
                            body: json!({
                                "event_type": "run.completed", "event_version": 1,
                                "recorded_at": "2026-08-03T00:00:01Z",
                                "payload": {"withheld": true}
                            }),
                        })
                        .unwrap(),
                    )
                    .unwrap();
                let acknowledgement = read_frame(&mut stream);
                assert_eq!(acknowledgement["operation"], "run.cursor_ack");
                respond(
                    &mut stream,
                    &acknowledgement,
                    json!({"subscription_id": resumed_subscription, "through_run_seq": 2}),
                );
            }
        } else {
            stream
                .write_all(
                    &encode_frame(&Event {
                        protocol: Protocol,
                        subscription_id: Id::new(first_subscription).unwrap(),
                        event: EventName::RunEvent,
                        run_id: Some(Id::new(run_id).unwrap()),
                        run_seq: Some(2),
                        body: json!({
                            "event_type": event_type, "event_version": 1,
                            "recorded_at": "2026-08-03T00:00:01Z",
                            "payload": {"withheld": true}
                        }),
                    })
                    .unwrap(),
                )
                .unwrap();
            let acknowledgement = read_frame(&mut stream);
            assert_eq!(acknowledgement["operation"], "run.cursor_ack");
            respond(
                &mut stream,
                &acknowledgement,
                json!({"subscription_id": first_subscription, "through_run_seq": 2}),
            );
        }
    });

    let mut command = Command::new(env!("CARGO_BIN_EXE_muniment-acp"));
    command
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_CONFIG_HOME", &config);
    let responses = exchange_with_command(
        command,
        &[request(
            1,
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": [{"type": "text", "text": "Continue."}]
            }),
        )],
    );
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
    responses
}

#[cfg(target_os = "linux")]
#[test]
fn prompt_ends_when_a_run_needs_attention() {
    let responses = scripted_prompt_ending("run.needs_attention", None);
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["error"]["code"], -32603);
    assert_eq!(
        responses[0]["error"]["message"],
        "Muniment run needs attention"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn prompt_ends_when_a_run_stream_cannot_resume() {
    let responses = scripted_prompt_ending("", Some(false));
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["error"]["code"], -32000);
    assert_eq!(
        responses[0]["error"]["message"],
        "Muniment run stream closed"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn prompt_resubscribes_when_a_run_stream_can_resume() {
    let responses = scripted_prompt_ending("", Some(true));
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["result"]["stopReason"], "end_turn");
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
                        "profile-id",
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
            let create = read_frame(&mut stream);
            assert_eq!(create["operation"], "thread.create");
            assert!(create["body"].as_object().unwrap().is_empty());
            stream
                .write_all(
                    &encode_frame(&Response {
                        protocol: Protocol,
                        request_id: Id::new(create["request_id"].as_str().unwrap()).unwrap(),
                        ok: Success,
                        body: json!({
                            "thread_id": "01900000-0000-7000-8000-000000000000"
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
    let record_path = client_files
        .join("acp-sessions")
        .join(format!("{session_id}.json"));
    let metadata = std::fs::metadata(&record_path).unwrap();
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    let record: Value = serde_json::from_slice(&std::fs::read(record_path).unwrap()).unwrap();
    assert_eq!(record.as_object().unwrap().len(), 5);
    assert_eq!(record["version"], 1);
    assert_eq!(record["session_id"], session_id);
    assert_eq!(
        record["workspace_root"],
        workspace.to_string_lossy().as_ref()
    );
    assert_eq!(record["thread_id"], "01900000-0000-7000-8000-000000000000");
    assert_eq!(record["profile_id"], "profile-id");
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
                    "profile-id",
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
        let create = read_frame(&mut stream);
        assert_eq!(create["operation"], "thread.create");
        let thread_id = "01900000-0000-7000-8000-000000000003";
        respond(&mut stream, &create, json!({"thread_id": thread_id}));

        let (mut stream, _) = listener.accept().unwrap();
        pair(&mut stream);
        let start = read_frame(&mut stream);
        assert_eq!(start["operation"], "run.start");
        assert_eq!(start["body"]["workspace"], expected_workspace);
        assert_eq!(start["body"]["text"], "Tell me more.");
        assert_eq!(start["body"]["thread_id"], thread_id);
        let run_id = "01900000-0000-7000-8000-000000000001";
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
                "current_run_seq": 8,
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
                EventName::RunEvent,
                3,
                json!({
                    "event_type": "tool.effect.started", "event_version": 1,
                    "recorded_at": "2026-08-03T00:00:02Z",
                    "payload": {"withheld": false, "effect_id": "tool-1", "display_name": "Search"}
                }),
            ),
            (
                EventName::RunEvent,
                4,
                json!({
                    "event_type": "tool.effect.completed", "event_version": 1,
                    "recorded_at": "2026-08-03T00:00:03Z",
                    "payload": {"withheld": false, "effect_id": "tool-1"}
                }),
            ),
            (
                EventName::PermissionPending,
                5,
                json!({
                    "gate_id": "gate-1", "kind": "confirm", "title": "Allow access?",
                    "message": "The command needs access."
                }),
            ),
            (
                EventName::RunEvent,
                6,
                json!({
                    "event_type": "model.stream.delta", "event_version": 1,
                    "recorded_at": "2026-08-03T00:00:02Z", "payload": {"withheld": true}
                }),
            ),
            (
                EventName::RunEvent,
                7,
                json!({
                    "event_type": "model.stream.delta", "event_version": 1,
                    "recorded_at": "2026-08-03T00:00:03Z", "payload": {"withheld": false, "text": "reply"}
                }),
            ),
            (
                EventName::RunEvent,
                8,
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

        for sequence in 2..=8 {
            if sequence == 5 {
                let cancel = read_frame(&mut stream);
                assert_eq!(cancel["operation"], "run.cancel");
                assert_eq!(cancel["body"]["run_id"], run_id);
                respond(
                    &mut stream,
                    &cancel,
                    json!({
                        "run_id": run_id, "accepted_at": "2026-08-03T00:00:03Z"
                    }),
                );
                let answer = read_frame(&mut stream);
                assert_eq!(answer["operation"], "permission.answer");
                assert_eq!(answer["body"]["decision"], "allow");
                respond(
                    &mut stream,
                    &answer,
                    json!({
                        "run_id": run_id, "gate_id": "gate-1", "decision": "allow",
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
        let run_id = "01900000-0000-7000-8000-000000000006";
        let subscription_id = "01900000-0000-7000-8000-000000000007";
        respond(
            &mut stream,
            &start,
            json!({
                "run_id": run_id,
                "thread_id": replacement_thread_id,
                "committed_seq": 1,
                "accepted_at": "2026-08-03T00:02:00Z"
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
        stream
            .write_all(
                &encode_frame(&Event {
                    protocol: Protocol,
                    subscription_id: Id::new(subscription_id).unwrap(),
                    event: EventName::PermissionPending,
                    run_id: Some(Id::new(run_id).unwrap()),
                    run_seq: Some(2),
                    body: json!({
                        "gate_id": "gate-2", "kind": "confirm", "title": "Allow again?"
                    }),
                })
                .unwrap(),
            )
            .unwrap();
        let answer = read_frame(&mut stream);
        assert_eq!(answer["operation"], "permission.answer");
        assert_eq!(answer["body"]["decision"], "deny");
        stream
            .write_all(
                &encode_frame(&ErrorEnvelope {
                    protocol: Protocol,
                    request_id: Some(Id::new(answer["request_id"].as_str().unwrap()).unwrap()),
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
    for _ in 0..4 {
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        responses.push(serde_json::from_str(&line).unwrap());
    }
    let permission = &responses[3];
    let permission_id = permission["id"].clone();
    assert_eq!(permission["method"], "session/request_permission");
    assert_eq!(permission["params"]["sessionId"], session_id);
    assert_eq!(permission["params"]["toolCall"]["toolCallId"], "gate-1");
    assert_eq!(permission["params"]["toolCall"]["title"], "Allow access?");
    assert_eq!(
        permission["params"]["toolCall"]["content"],
        json!([{"type": "content", "content": {"type": "text", "text": "The command needs access."}}])
    );
    assert_eq!(
        permission["params"]["options"],
        json!([
            {"optionId": "allow_once", "name": "Allow once", "kind": "allow_once"},
            {"optionId": "reject_once", "name": "Reject once", "kind": "reject_once"}
        ])
    );
    serde_json::to_writer(
        &mut input,
        &request(
            permission_id.as_u64().unwrap(),
            "colliding/editor/request",
            json!({}),
        ),
    )
    .unwrap();
    input.write_all(b"\n").unwrap();
    serde_json::to_writer(
        &mut input,
        &json!({
            "jsonrpc": "2.0",
            "method": "session/cancel",
            "params": {"sessionId": session_id}
        }),
    )
    .unwrap();
    input.write_all(b"\n").unwrap();
    serde_json::to_writer(
        &mut input,
        &json!({
            "jsonrpc": "2.0",
            "id": permission_id,
            "result": {"outcome": {"outcome": "selected", "optionId": "allow_once"}}
        }),
    )
    .unwrap();
    input.write_all(b"\n").unwrap();
    input.flush().unwrap();
    for _ in 0..2 {
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
    input.flush().unwrap();
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    let failed_permission: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(failed_permission["method"], "session/request_permission");
    responses.push(failed_permission.clone());
    serde_json::to_writer(
        &mut input,
        &json!({
            "jsonrpc": "2.0",
            "id": failed_permission["id"],
            "result": {"outcome": {"outcome": "selected", "optionId": "reject_once"}}
        }),
    )
    .unwrap();
    input.write_all(b"\n").unwrap();
    drop(input);
    responses.extend(
        output
            .lines()
            .map(|line| serde_json::from_str(&line.unwrap()).unwrap()),
    );
    assert_eq!(responses.len(), 10);
    assert_eq!(responses[0]["method"], "session/update");
    assert_eq!(
        responses[0]["params"]["update"]["content"]["text"],
        "First "
    );
    assert_eq!(
        responses[1]["params"]["update"],
        json!({
            "sessionUpdate": "tool_call", "toolCallId": "tool-1", "title": "Search"
        })
    );
    assert_eq!(
        responses[2]["params"]["update"],
        json!({
            "sessionUpdate": "tool_call_update", "toolCallId": "tool-1", "status": "completed"
        })
    );
    assert_eq!(responses[4]["params"]["update"]["content"]["text"], "reply");
    assert_eq!(responses[5]["id"], 2);
    assert_eq!(responses[5]["result"]["stopReason"], "end_turn");
    assert_eq!(responses[6]["id"], permission_id);
    assert_eq!(responses[6]["error"]["code"], -32601);
    assert_eq!(responses[7]["id"], 3);
    assert_eq!(responses[7]["result"]["stopReason"], "end_turn");
    assert_eq!(responses[8]["method"], "session/request_permission");
    assert_eq!(responses[9]["id"], 4);
    assert_eq!(responses[9]["error"]["code"], -32000);
    assert!(child.wait().unwrap().success());
    server.join().unwrap();
    let record: Value = serde_json::from_slice(
        &std::fs::read(
            config
                .join("muniment/acp-sessions")
                .join(format!("{session_id}.json")),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(record["thread_id"], "0190a100-0000-7000-8000-000000000002");
    std::fs::remove_dir_all(runtime).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn cancel_during_a_prompt_targets_the_bound_run_and_drains_to_cancelled() {
    use muniment_attach::{
        authorized_with_client_credential, encode_frame, welcome, ErrorEnvelope, Event, EventName,
        Failure, Id, Protocol, ProtocolError, Response, Success,
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
                    "profile-id",
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
        let create = read_frame(&mut stream);
        assert_eq!(create["operation"], "thread.create");
        respond(
            &mut stream,
            &create,
            json!({"thread_id": "01900000-0000-7000-8000-000000000013"}),
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
                "current_run_seq": 3,
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
                        "event_type": "model.stream.delta",
                        "event_version": 1,
                        "recorded_at": "2026-08-03T00:00:01Z",
                        "payload": {"withheld": false, "text": "Stopping."}
                    }),
                })
                .unwrap(),
            )
            .unwrap();
        let acknowledgement = read_frame(&mut stream);
        assert_eq!(acknowledgement["operation"], "run.cursor_ack");
        respond(
            &mut stream,
            &acknowledgement,
            json!({"subscription_id": subscription_id, "through_run_seq": 2}),
        );
        let failed_cancel = read_frame(&mut stream);
        assert_eq!(failed_cancel["operation"], "run.cancel");
        assert_eq!(failed_cancel["body"]["run_id"], run_id);
        stream
            .write_all(
                &encode_frame(&ErrorEnvelope {
                    protocol: Protocol,
                    request_id: Some(
                        Id::new(failed_cancel["request_id"].as_str().unwrap()).unwrap(),
                    ),
                    ok: Failure,
                    error: ProtocolError::invalid_request(),
                })
                .unwrap(),
            )
            .unwrap();
        stream
            .write_all(
                &encode_frame(&Event {
                    protocol: Protocol,
                    subscription_id: Id::new(subscription_id).unwrap(),
                    event: EventName::RunEvent,
                    run_id: Some(Id::new(run_id).unwrap()),
                    run_seq: Some(3),
                    body: json!({
                        "event_type": "run.cancelled",
                        "event_version": 1,
                        "recorded_at": "2026-08-03T00:00:03Z",
                        "payload": {"withheld": true}
                    }),
                })
                .unwrap(),
            )
            .unwrap();
        let acknowledgement = read_frame(&mut stream);
        assert_eq!(acknowledgement["operation"], "run.cursor_ack");
        respond(
            &mut stream,
            &acknowledgement,
            json!({"subscription_id": subscription_id, "through_run_seq": 3}),
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
    serde_json::to_writer(
        &mut input,
        &json!({
            "jsonrpc": "2.0",
            "method": "session/cancel",
            "params": {"sessionId": "another-session"}
        }),
    )
    .unwrap();
    input.write_all(b"\n").unwrap();
    serde_json::to_writer(
        &mut input,
        &json!({
            "jsonrpc": "2.0",
            "method": "session/cancel",
            "params": {"sessionId": created["result"]["sessionId"]}
        }),
    )
    .unwrap();
    input.write_all(b"\n").unwrap();
    serde_json::to_writer(&mut input, &initialize_request()).unwrap();
    input.write_all(b"\n").unwrap();
    input.flush().unwrap();
    prompt_started_rx.recv().unwrap();
    finish_prompt_tx.send(()).unwrap();
    drop(input);

    let responses: Vec<Value> = output
        .lines()
        .map(|line| serde_json::from_str(&line.unwrap()).unwrap())
        .collect();
    assert_eq!(responses.len(), 3);
    assert_eq!(responses[0]["method"], "session/update");
    assert_eq!(
        responses[0]["params"]["update"]["content"]["text"],
        "Stopping."
    );
    assert_eq!(responses[1]["id"], 2);
    assert_eq!(
        responses[1]["result"]["stopReason"], "cancelled",
        "{responses:?}"
    );
    assert_eq!(responses[2]["id"], 1);
    assert_eq!(responses[2]["result"]["protocolVersion"], 1);
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
