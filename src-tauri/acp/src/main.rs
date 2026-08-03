use agent_client_protocol::schema::{
    v1::{
        AgentCapabilities, ContentBlock, ContentChunk, InitializeRequest, InitializeResponse,
        LoadSessionRequest, LoadSessionResponse, NewSessionRequest, NewSessionResponse,
        PermissionOption, PermissionOptionKind, PromptRequest, PromptResponse,
        RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
        SessionNotification, SessionUpdate, StopReason, TextContent, ToolCallContent,
        ToolCallUpdate, ToolCallUpdateFields,
    },
    ProtocolVersion,
};
use muniment_attach::{
    handshake_as_with_credential, AuthorizedClient, ClientError, Id, PermissionDecision,
    RunStreamMessage,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;

const CLIENT_KIND: &str = "acp-adapter";
const CLIENT_ID_FILE: &str = "acp-client-id";
const CLIENT_CREDENTIAL_FILE: &str = "acp-client-credential";
const SESSION_DIRECTORY: &str = "acp-sessions";
const SESSION_RECORD_VERSION: u32 = 1;

enum PromptFailure {
    Client(ClientError),
    Run,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SessionRecord {
    version: u32,
    session_id: String,
    workspace_root: String,
    thread_id: String,
    profile_id: String,
}

impl From<ClientError> for PromptFailure {
    fn from(error: ClientError) -> Self {
        Self::Client(error)
    }
}

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    serve(BufReader::new(stdin), stdout.lock())
}

fn serve(input: impl BufRead + Send + 'static, mut output: impl Write) -> io::Result<()> {
    let (sender, receiver) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in input.lines() {
            let is_error = line.is_err();
            if sender.send(line).is_err() || is_error {
                break;
            }
        }
    });
    let mut pending = VecDeque::new();
    let mut next_request_id = 1_u64;
    let result = (|| {
        loop {
            let line = match pending.pop_front() {
                Some(line) => line,
                None => match receiver.recv() {
                    Ok(line) => line,
                    Err(_) => break,
                },
            };
            let line = line?;
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                write_message(
                    &mut output,
                    json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "Parse error"}}),
                )?;
                continue;
            };
            if let Some(response) = response(
                message,
                &mut output,
                &receiver,
                &mut pending,
                &mut next_request_id,
            ) {
                write_message(&mut output, response)?;
            }
        }
        Ok(())
    })();
    reader
        .join()
        .map_err(|_| io::Error::other("stdin reader thread panicked"))?;
    result
}

fn response(
    message: Value,
    output: &mut impl Write,
    input: &Receiver<io::Result<String>>,
    pending: &mut VecDeque<io::Result<String>>,
    next_request_id: &mut u64,
) -> Option<Value> {
    let object = message.as_object()?;
    let method = object.get("method")?.as_str()?;
    let id = object.get("id").cloned();
    let id = id?;

    match method {
        "initialize" => Some(initialize(id, object.get("params"))),
        "session/new" => Some(new_session(id, object.get("params"))),
        "session/prompt" => Some(prompt(
            id,
            object.get("params"),
            output,
            input,
            pending,
            next_request_id,
        )),
        "session/load" => Some(load_session(id, object.get("params"), output)),
        _ => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32601, "message": "Method not found"}
        })),
    }
}

fn load_session(id: Value, params: Option<&Value>, output: &mut impl Write) -> Value {
    let Some(params) = params else {
        return invalid_params(id, "session/load parameters are invalid");
    };
    if params
        .get("mcpServers")
        .and_then(Value::as_array)
        .is_some_and(|servers| !servers.is_empty())
    {
        return invalid_params(id, "session/load does not accept MCP servers");
    }
    let Ok(request) = serde_json::from_value::<LoadSessionRequest>(params.clone()) else {
        return invalid_params(id, "session/load parameters are invalid");
    };
    if !request.cwd.is_absolute() {
        return invalid_params(id, "session/load cwd must be absolute");
    }
    let session_id = request.session_id.0.to_string();
    let Ok(record) = read_session_record(&session_id) else {
        return json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32002, "message": "Resource not found"}
        });
    };

    let result: Result<(), ClientError> = (|| {
        let identity = authorized_client_identity().map_err(|_| ClientError::UnexpectedMessage)?;
        let credential =
            authorized_client_credential().map_err(|_| ClientError::UnexpectedMessage)?;
        let mut client = handshake_as_with_credential(
            env!("CARGO_PKG_VERSION"),
            CLIENT_KIND,
            &identity,
            credential.as_deref(),
            || {},
        )?;
        persist_authorized_client_credential(client.authorized_client_credential())
            .map_err(|_| ClientError::UnexpectedMessage)?;
        let workspace = request.cwd.to_string_lossy();
        let onboarded = client.onboard_workspace(&workspace, &workspace)?;
        if onboarded.opened_directory != record.workspace_root
            || client.profile_id() != record.profile_id
        {
            return Err(ClientError::UnexpectedMessage);
        }

        let mut cursor = None;
        let mut updates = Vec::new();
        loop {
            let page = client.open_thread(&record.thread_id, cursor.as_deref())?;
            for entry in page.entries {
                let content = ContentChunk::new(ContentBlock::Text(TextContent::new(
                    entry.text.unwrap_or_default(),
                )));
                let update = match entry.kind.as_str() {
                    "user_message" | "attachment" => SessionUpdate::UserMessageChunk(content),
                    "assistant_message" => SessionUpdate::AgentMessageChunk(content),
                    _ => return Err(ClientError::UnexpectedMessage),
                };
                updates.push(update);
            }
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        for update in updates {
            let notification = SessionNotification::new(session_id.clone(), update);
            write_message(
                output,
                json!({"jsonrpc": "2.0", "method": "session/update", "params": notification}),
            )
            .map_err(|_| ClientError::ConnectionClosed)?;
        }
        Ok(())
    })();

    match result {
        Ok(()) => json!({"jsonrpc": "2.0", "id": id, "result": LoadSessionResponse::new()}),
        Err(error) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32000, "message": pairing_failure(error)}
        }),
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
    let result = (|| {
        let identity = authorized_client_identity().map_err(|_| ClientError::UnexpectedMessage)?;
        let credential =
            authorized_client_credential().map_err(|_| ClientError::UnexpectedMessage)?;
        let mut client = handshake_as_with_credential(
            env!("CARGO_PKG_VERSION"),
            CLIENT_KIND,
            &identity,
            credential.as_deref(),
            || {},
        )?;
        persist_authorized_client_credential(client.authorized_client_credential())
            .map_err(|_| ClientError::UnexpectedMessage)?;
        let onboarded = client.onboard_workspace(&workspace, &workspace)?;
        let thread = client.create_thread()?;
        let session_id = uuid::Uuid::now_v7().to_string();
        let record = SessionRecord {
            version: SESSION_RECORD_VERSION,
            session_id: session_id.clone(),
            workspace_root: onboarded.opened_directory,
            thread_id: thread.thread_id,
            profile_id: client.profile_id().to_owned(),
        };
        write_session_record(&record).map_err(|_| ClientError::UnexpectedMessage)?;
        Ok(session_id)
    })();
    match result {
        Ok(session_id) => {
            let result = NewSessionResponse::new(session_id);
            json!({"jsonrpc": "2.0", "id": id, "result": result})
        }
        Err(error) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32000, "message": pairing_failure(error)}
        }),
    }
}

fn prompt(
    id: Value,
    params: Option<&Value>,
    output: &mut impl Write,
    input: &Receiver<io::Result<String>>,
    pending: &mut VecDeque<io::Result<String>>,
    next_request_id: &mut u64,
) -> Value {
    let Some(params) = params else {
        return invalid_params(id, "session/prompt parameters are invalid");
    };
    let Ok(request) = serde_json::from_value::<PromptRequest>(params.clone()) else {
        return invalid_params(id, "session/prompt parameters are invalid");
    };
    let session_id = request.session_id.0.to_string();
    let Ok(mut session) = read_session_record(&session_id) else {
        return json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32002, "message": "Resource not found"}
        });
    };
    let workspace = session.workspace_root.clone();
    let thread_id = session.thread_id.clone();
    let mut text = String::new();
    for block in request.prompt {
        match block {
            ContentBlock::Text(content) => text.push_str(&content.text),
            _ => return invalid_params(id, "session/prompt accepts text content only"),
        }
    }
    if text.trim().is_empty() {
        return invalid_params(id, "session/prompt requires text content");
    }

    let result: Result<StopReason, PromptFailure> = (|| {
        let identity = authorized_client_identity().map_err(|_| ClientError::UnexpectedMessage)?;
        let credential =
            authorized_client_credential().map_err(|_| ClientError::UnexpectedMessage)?;
        let mut client = handshake_as_with_credential(
            env!("CARGO_PKG_VERSION"),
            CLIENT_KIND,
            &identity,
            credential.as_deref(),
            || {},
        )?;
        persist_authorized_client_credential(client.authorized_client_credential())
            .map_err(|_| ClientError::UnexpectedMessage)?;
        let accepted = match client.start_run_in_workspace_thread(
            &text,
            None,
            Some(&workspace),
            Some(&thread_id),
        ) {
            Err(ClientError::ThreadNotFound) => {
                client.start_run_in_workspace_thread(&text, None, Some(&workspace), None)?
            }
            result => result?,
        };
        if accepted.thread_id != session.thread_id {
            session.thread_id.clone_from(&accepted.thread_id);
            write_session_record(&session)
                .map_err(|_| PromptFailure::Client(ClientError::UnexpectedMessage))?;
        }
        client.subscribe_run(&accepted.run_id, accepted.committed_seq)?;
        loop {
            let (run_seq, terminal) = match client.read_run_stream_message()? {
                RunStreamMessage::Event(event) => {
                    if let Some(text) = event.text {
                        let update = SessionNotification::new(
                            session_id.clone(),
                            SessionUpdate::AgentMessageChunk(ContentChunk::new(
                                ContentBlock::Text(TextContent::new(text)),
                            )),
                        );
                        write_message(
                            output,
                            json!({"jsonrpc": "2.0", "method": "session/update", "params": update}),
                        )
                        .map_err(|_| PromptFailure::Client(ClientError::ConnectionClosed))?;
                    }
                    let terminal = match event.event_type.as_str() {
                        "run.completed" => Some(Ok(StopReason::EndTurn)),
                        "run.cancelled" => Some(Ok(StopReason::Cancelled)),
                        "run.failed" => Some(Err(PromptFailure::Run)),
                        _ => None,
                    };
                    (Some(event.run_seq), terminal)
                }
                RunStreamMessage::PermissionPending(permission) => {
                    let request_id = *next_request_id;
                    *next_request_id = next_request_id.wrapping_add(1);
                    let content = permission
                        .message
                        .map(|message| {
                            vec![ToolCallContent::from(ContentBlock::Text(TextContent::new(
                                message,
                            )))]
                        })
                        .unwrap_or_default();
                    let tool_call = ToolCallUpdate::new(
                        permission.gate_id.clone(),
                        ToolCallUpdateFields::new()
                            .title(permission.title)
                            .content(content),
                    );
                    let request = RequestPermissionRequest::new(
                        session_id.clone(),
                        tool_call,
                        vec![
                            PermissionOption::new(
                                "allow_once",
                                "Allow once",
                                PermissionOptionKind::AllowOnce,
                            ),
                            PermissionOption::new(
                                "reject_once",
                                "Reject once",
                                PermissionOptionKind::RejectOnce,
                            ),
                        ],
                    );
                    write_message(
                        output,
                        json!({
                            "jsonrpc": "2.0",
                            "id": request_id,
                            "method": "session/request_permission",
                            "params": request
                        }),
                    )
                    .map_err(|_| PromptFailure::Client(ClientError::ConnectionClosed))?;
                    let decision = wait_for_permission_response(
                        request_id,
                        &session_id,
                        &accepted.run_id,
                        &mut client,
                        input,
                        pending,
                    );
                    client.answer_permission(&accepted.run_id, &permission.gate_id, decision)?;
                    (Some(permission.run_seq), None)
                }
                RunStreamMessage::CaughtUp { .. } => (None, None),
            };
            if let Some(run_seq) = run_seq {
                client.acknowledge_run_cursor(run_seq)?;
            }
            if let Some(terminal) = terminal {
                break terminal;
            }
            while let Ok(line) = input.try_recv() {
                let cancel = line.as_ref().ok().and_then(|line| {
                    let message = serde_json::from_str::<Value>(line).ok()?;
                    let object = message.as_object()?;
                    (object.get("id").is_none()
                        && object.get("method").and_then(Value::as_str) == Some("session/cancel")
                        && object
                            .get("params")
                            .and_then(|params| params.get("sessionId"))
                            .and_then(Value::as_str)
                            == Some(&session_id))
                    .then_some(())
                });
                if cancel.is_some() {
                    let _ = client.run_cancel(&accepted.run_id);
                } else {
                    pending.push_back(line);
                }
            }
        }
    })();

    match result {
        Ok(stop_reason) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": PromptResponse::new(stop_reason)
        }),
        Err(PromptFailure::Run) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32603, "message": "Muniment run failed"}
        }),
        Err(PromptFailure::Client(error)) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32000, "message": pairing_failure(error)}
        }),
    }
}

fn wait_for_permission_response(
    request_id: u64,
    session_id: &str,
    run_id: &str,
    client: &mut AuthorizedClient,
    input: &Receiver<io::Result<String>>,
    pending: &mut VecDeque<io::Result<String>>,
) -> PermissionDecision {
    loop {
        let Ok(line) = input.recv() else {
            return PermissionDecision::Deny;
        };
        let Ok(line) = line else {
            return PermissionDecision::Deny;
        };
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            pending.push_back(Ok(line));
            continue;
        };
        if let Some(decision) = permission_response_decision(&message, request_id) {
            return decision;
        }
        let cancel = message.get("id").is_none()
            && message.get("method").and_then(Value::as_str) == Some("session/cancel")
            && message
                .get("params")
                .and_then(|params| params.get("sessionId"))
                .and_then(Value::as_str)
                == Some(session_id);
        if cancel {
            let _ = client.run_cancel(run_id);
        } else {
            pending.push_back(Ok(line));
        }
    }
}

fn permission_response_decision(message: &Value, request_id: u64) -> Option<PermissionDecision> {
    let object = message.as_object()?;
    if object.get("method").and_then(Value::as_str).is_some()
        || object.get("id") != Some(&json!(request_id))
    {
        return None;
    }
    let valid_envelope = !object.contains_key("method")
        && object.get("jsonrpc").and_then(Value::as_str) == Some("2.0")
        && object.contains_key("result") != object.contains_key("error");
    if !valid_envelope || object.contains_key("error") {
        return Some(PermissionDecision::Deny);
    }
    let response = object
        .get("result")
        .cloned()
        .and_then(|result| serde_json::from_value::<RequestPermissionResponse>(result).ok());
    Some(match response.map(|response| response.outcome) {
        Some(RequestPermissionOutcome::Selected(selected))
            if selected.option_id.0.as_ref() == "allow_once" =>
        {
            PermissionDecision::Allow
        }
        _ => PermissionDecision::Deny,
    })
}

#[cfg(test)]
mod permission_response_tests {
    use super::*;

    fn decision(message: Value) -> Option<PermissionDecision> {
        permission_response_decision(&message, 7)
    }

    fn selected(option_id: &str) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": 7,
            "result": {"outcome": {"outcome": "selected", "optionId": option_id}}
        })
    }

    #[test]
    fn maps_only_allow_once_to_allow() {
        assert!(matches!(
            decision(selected("allow_once")),
            Some(PermissionDecision::Allow)
        ));
        for message in [
            selected("reject_once"),
            selected("unknown"),
            json!({"jsonrpc": "2.0", "id": 7, "result": {"outcome": {"outcome": "cancelled"}}}),
        ] {
            assert!(matches!(decision(message), Some(PermissionDecision::Deny)));
        }
    }

    #[test]
    fn malformed_matching_responses_deny() {
        for message in [
            json!({"id": 7, "result": selected("allow_once")["result"]}),
            json!({"jsonrpc": "1.0", "id": 7, "result": selected("allow_once")["result"]}),
            json!({"jsonrpc": "2.0", "id": 7}),
            json!({"jsonrpc": "2.0", "id": 7, "result": selected("allow_once")["result"], "error": {"code": -1, "message": "error"}}),
            json!({"jsonrpc": "2.0", "id": 7, "method": null, "result": selected("allow_once")["result"]}),
            json!({"jsonrpc": "2.0", "id": 7, "result": {}}),
            json!({"jsonrpc": "2.0", "id": 7, "error": {"code": -1, "message": "error"}}),
        ] {
            assert!(matches!(decision(message), Some(PermissionDecision::Deny)));
        }
    }

    #[test]
    fn requests_and_other_responses_are_unrelated() {
        assert!(decision(json!({
            "jsonrpc": "2.0", "id": 7, "method": "initialize", "params": {}
        }))
        .is_none());
        assert!(permission_response_decision(&selected("allow_once"), 8).is_none());
    }
}

fn config_directory() -> io::Result<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .filter(|path| path.is_absolute())
        .map(|path| path.join("muniment"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "config directory is unavailable"))
}

fn authorized_client_identity() -> io::Result<String> {
    let directory = config_directory()?;
    let path = directory.join(CLIENT_ID_FILE);
    match read_private_file(&path) {
        Ok(value) => return valid_identity(&value),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    std::fs::create_dir_all(&directory)?;
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let value = format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    );
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    match options
        .open(&path)
        .and_then(|mut file| file.write_all(value.as_bytes()))
    {
        Ok(()) => Ok(value),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            valid_identity(&read_private_file(&path)?)
        }
        Err(error) => Err(error),
    }
}

fn valid_identity(value: &str) -> io::Result<String> {
    let value = value.trim();
    Id::new(value)
        .map(|_| value.to_owned())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "client identity is invalid"))
}

fn authorized_client_credential() -> io::Result<Option<String>> {
    let path = config_directory()?.join(CLIENT_CREDENTIAL_FILE);
    match read_private_file(&path) {
        Ok(value)
            if value.trim().len() == 64
                && value.trim().bytes().all(|byte| byte.is_ascii_hexdigit()) =>
        {
            Ok(Some(value.trim().to_owned()))
        }
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "client credential is invalid",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn persist_authorized_client_credential(credential: &str) -> io::Result<()> {
    let directory = config_directory()?;
    std::fs::create_dir_all(&directory)?;
    atomic_write_private_file(
        &directory.join(CLIENT_CREDENTIAL_FILE),
        credential.as_bytes(),
    )
}

fn session_record_path(session_id: &str) -> io::Result<PathBuf> {
    let parsed = uuid::Uuid::parse_str(session_id)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "session ID is invalid"))?;
    if parsed.to_string() != session_id {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "session ID is invalid",
        ));
    }
    Ok(config_directory()?
        .join(SESSION_DIRECTORY)
        .join(format!("{session_id}.json")))
}

fn read_session_record(session_id: &str) -> io::Result<SessionRecord> {
    let value = read_private_file(&session_record_path(session_id)?)?;
    let record: SessionRecord = serde_json::from_str(&value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "session record is invalid"))?;
    if record.version != SESSION_RECORD_VERSION
        || record.session_id != session_id
        || !Path::new(&record.workspace_root).is_absolute()
        || Id::new(&record.thread_id).is_err()
        || record.profile_id.is_empty()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "session record is invalid",
        ));
    }
    Ok(record)
}

fn write_session_record(record: &SessionRecord) -> io::Result<()> {
    let path = session_record_path(&record.session_id)?;
    std::fs::create_dir_all(path.parent().expect("session record has a parent"))?;
    let value = serde_json::to_vec(record)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "session record is invalid"))?;
    atomic_write_private_file(&path, &value)
}

#[cfg(unix)]
fn read_private_file(path: &Path) -> io::Result<String> {
    let mut options = std::fs::OpenOptions::new();
    options
        .read(true)
        .custom_flags(unix_abi::O_NOFOLLOW | unix_abi::O_NONBLOCK);
    let mut file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != unix_abi::effective_uid()
        || metadata.mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "client authorization file is not private",
        ));
    }
    let mut value = String::new();
    file.read_to_string(&mut value)?;
    Ok(value)
}

#[cfg(unix)]
fn atomic_write_private_file(path: &Path, value: &[u8]) -> io::Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing parent directory"))?;
    let mut random = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
    let temporary = directory.join(format!(
        ".acp-client-credential.{}.tmp",
        random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ));
    let mut options = std::fs::OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(unix_abi::O_NOFOLLOW);
    let result = (|| {
        let mut file = options.open(&temporary)?;
        file.write_all(value)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(unix)]
mod unix_abi {
    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "solaris",
        target_os = "illumos"
    ))]
    pub const O_NOFOLLOW: i32 = 0x20000;
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    pub const O_NOFOLLOW: i32 = 0x100;
    pub const O_NONBLOCK: i32 = 0x4;

    unsafe extern "C" {
        fn geteuid() -> u32;
    }

    pub fn effective_uid() -> u32 {
        // SAFETY: geteuid takes no arguments and has no preconditions.
        unsafe { geteuid() }
    }
}

#[cfg(not(unix))]
fn read_private_file(path: &Path) -> io::Result<String> {
    std::fs::read_to_string(path)
}

#[cfg(not(unix))]
fn atomic_write_private_file(path: &Path, value: &[u8]) -> io::Result<()> {
    std::fs::write(path, value)
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
        ClientError::ThreadNotFound => "Muniment runtime could not find the thread",
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

    let capabilities = AgentCapabilities::new().load_session(false);
    let result = InitializeResponse::new(ProtocolVersion::V1).agent_capabilities(capabilities);
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn write_message(output: &mut impl Write, message: Value) -> io::Result<()> {
    serde_json::to_writer(&mut *output, &message)?;
    output.write_all(b"\n")?;
    output.flush()
}
