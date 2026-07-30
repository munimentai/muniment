use agent_client_protocol::schema::{
    v1::{
        AgentCapabilities, InitializeRequest, InitializeResponse, NewSessionRequest,
        NewSessionResponse,
    },
    ProtocolVersion,
};
use muniment_attach::{handshake, ClientError};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

const NO_ATTACH: &str = "no authorized Muniment runtime attach exists";
const CLIENT_KIND: &str = "acp-adapter";

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    serve(stdin.lock(), stdout.lock())
}

fn serve(input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    for line in input.lines() {
        let line = line?;
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            write_message(
                &mut output,
                json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "Parse error"}}),
            )?;
            continue;
        };
        if let Some(response) = response(message) {
            write_message(&mut output, response)?;
        }
    }
    Ok(())
}

fn response(message: Value) -> Option<Value> {
    let object = message.as_object()?;
    let method = object.get("method")?.as_str()?;
    let id = object.get("id").cloned();
    let id = id?;

    match method {
        "initialize" => Some(initialize(id, object.get("params"))),
        "session/new" => Some(new_session(id, object.get("params"))),
        "session/load" | "session/prompt" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32000, "message": NO_ATTACH}
        })),
        _ => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32601, "message": "Method not found"}
        })),
    }
}

fn new_session(id: Value, params: Option<&Value>) -> Value {
    let Some(params) = params else {
        return invalid_params(id, "session/new requires an absolute cwd");
    };
    if params
        .get("mcpServers")
        .and_then(Value::as_array)
        .is_some_and(|servers| !servers.is_empty())
    {
        return invalid_params(id, "session/new does not accept MCP servers");
    }
    let Ok(request) = serde_json::from_value::<NewSessionRequest>(params.clone()) else {
        return invalid_params(id, "session/new parameters are invalid");
    };
    if !request.cwd.is_absolute() {
        return invalid_params(id, "session/new cwd must be absolute");
    }

    let workspace = request.cwd.to_string_lossy();
    let result = handshake(env!("CARGO_PKG_VERSION"), CLIENT_KIND, || {})
        .and_then(|mut client| client.onboard_workspace(&workspace, &workspace));
    match result {
        Ok(_) => {
            let result = NewSessionResponse::new(uuid::Uuid::now_v7().to_string());
            json!({"jsonrpc": "2.0", "id": id, "result": result})
        }
        Err(error) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32000, "message": pairing_failure(error)}
        }),
    }
}

fn invalid_params(id: Value, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": -32602, "message": message}
    })
}

fn pairing_failure(error: ClientError) -> &'static str {
    match error {
        ClientError::UnsupportedPlatform => "Muniment runtime attach is unsupported",
        ClientError::AuthorizationExpired => "Muniment runtime attach authorization expired",
        ClientError::RequestRejected => "Muniment runtime rejected workspace registration",
        ClientError::DesktopFailed => "Muniment runtime failed workspace registration",
        ClientError::RuntimeDirectoryMissing => "Muniment runtime directory is unavailable",
        ClientError::RuntimeDirectoryRelative => "Muniment runtime directory is invalid",
        ClientError::DesktopUnavailable => "Muniment runtime attach is unavailable",
        ClientError::Timeout => "Muniment runtime pairing timed out",
        ClientError::ConnectionClosed => "Muniment runtime pairing was denied or closed",
        ClientError::MalformedFrame => "Muniment runtime sent a malformed attach message",
        ClientError::PayloadTooLarge => "Muniment runtime sent an oversized attach message",
        ClientError::UnexpectedMessage => "Muniment runtime sent an invalid pairing message",
        ClientError::ProtocolIncompatible => "Muniment runtime attach protocol is incompatible",
        ClientError::RandomnessUnavailable => {
            "Muniment runtime pairing could not create an identity"
        }
    }
}

fn initialize(id: Value, params: Option<&Value>) -> Value {
    let request = params
        .cloned()
        .and_then(|params| serde_json::from_value::<InitializeRequest>(params).ok());
    if request.is_none() {
        return json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32602, "message": "Invalid params"}
        });
    }

    let capabilities = AgentCapabilities::new().load_session(true);
    let result = InitializeResponse::new(ProtocolVersion::V1).agent_capabilities(capabilities);
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn write_message(output: &mut impl Write, message: Value) -> io::Result<()> {
    serde_json::to_writer(&mut *output, &message)?;
    output.write_all(b"\n")?;
    output.flush()
}
