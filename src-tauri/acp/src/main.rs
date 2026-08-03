use agent_client_protocol::schema::{
    v1::{
        AgentCapabilities, ContentBlock, ContentChunk, InitializeRequest, InitializeResponse,
        NewSessionRequest, NewSessionResponse, PromptRequest, PromptResponse, SessionNotification,
        SessionUpdate, StopReason, TextContent,
    },
    ProtocolVersion,
};
use muniment_attach::{
    handshake_as_with_credential, ClientError, Id, PermissionDecision, RunStreamMessage,
};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::io::{self, BufRead, BufReader, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;

const NO_ATTACH: &str = "no authorized Muniment runtime attach exists";
const CLIENT_KIND: &str = "acp-adapter";
const CLIENT_ID_FILE: &str = "acp-client-id";
const CLIENT_CREDENTIAL_FILE: &str = "acp-client-credential";

enum PromptFailure {
    Client(ClientError),
    Run,
}

struct Session {
    workspace: String,
    thread_id: Option<String>,
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
    let mut sessions = HashMap::new();
    let mut pending = VecDeque::new();
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
            if let Some(response) =
                response(message, &mut sessions, &mut output, &receiver, &mut pending)
            {
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
    sessions: &mut HashMap<String, Session>,
    output: &mut impl Write,
    input: &Receiver<io::Result<String>>,
    pending: &mut VecDeque<io::Result<String>>,
) -> Option<Value> {
    let object = message.as_object()?;
    let method = object.get("method")?.as_str()?;
    let id = object.get("id").cloned();
    let id = id?;

    match method {
        "initialize" => Some(initialize(id, object.get("params"))),
        "session/new" => Some(new_session(id, object.get("params"), sessions)),
        "session/prompt" => Some(prompt(
            id,
            object.get("params"),
            sessions,
            output,
            input,
            pending,
        )),
        "session/load" => Some(json!({
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

fn new_session(
    id: Value,
    params: Option<&Value>,
    sessions: &mut HashMap<String, Session>,
) -> Value {
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
        client.onboard_workspace(&workspace, &workspace)
    })();
    match result {
        Ok(onboarded) => {
            let session_id = uuid::Uuid::now_v7().to_string();
            sessions.insert(
                session_id.clone(),
                Session {
                    workspace: onboarded.opened_directory,
                    thread_id: None,
                },
            );
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
    sessions: &mut HashMap<String, Session>,
    output: &mut impl Write,
    input: &Receiver<io::Result<String>>,
    pending: &mut VecDeque<io::Result<String>>,
) -> Value {
    let Some(params) = params else {
        return invalid_params(id, "session/prompt parameters are invalid");
    };
    let Ok(request) = serde_json::from_value::<PromptRequest>(params.clone()) else {
        return invalid_params(id, "session/prompt parameters are invalid");
    };
    let session_id = request.session_id.0.to_string();
    let Some(session) = sessions.get(&session_id) else {
        return json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32002, "message": "Resource not found"}
        });
    };
    let workspace = session.workspace.clone();
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
            thread_id.as_deref(),
        ) {
            Err(ClientError::ThreadNotFound) if thread_id.is_some() => {
                sessions
                    .get_mut(&session_id)
                    .expect("session exists")
                    .thread_id = None;
                client.start_run_in_workspace_thread(&text, None, Some(&workspace), None)?
            }
            result => result?,
        };
        sessions
            .get_mut(&session_id)
            .expect("session exists")
            .thread_id = Some(accepted.thread_id.clone());
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
                    client.answer_permission(
                        &accepted.run_id,
                        &permission.gate_id,
                        PermissionDecision::Deny,
                    )?;
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
