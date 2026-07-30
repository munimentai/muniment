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
fn session_requests_fail_without_an_authorized_attach() {
    let responses = exchange(&[
        initialize_request(),
        request(2, "session/load", json!({})),
        request(3, "session/prompt", json!({})),
    ]);

    assert_eq!(responses[0]["result"]["protocolVersion"], 1);
    for (response, id) in responses[1..].iter().zip(2..=3) {
        assert_eq!(response["id"], id);
        assert_eq!(response["error"]["code"], -32000);
        assert_eq!(
            response["error"]["message"],
            "no authorized Muniment runtime attach exists"
        );
    }
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
