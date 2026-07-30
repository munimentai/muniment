use agent_client_protocol::schema::{
    v1::{AgentCapabilities, InitializeRequest, InitializeResponse},
    ProtocolVersion,
};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

const NO_ATTACH: &str = "no authorized Muniment runtime attach exists";

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
        "session/new" | "session/load" | "session/prompt" => Some(json!({
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
