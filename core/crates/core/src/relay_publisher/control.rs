//! The pair's control session: the operations the paired phone may ask of
//! this runtime, the fleet snapshot it reads, the pages a response travels
//! in, and the live events it follows.
//!
//! The phone is a window. Every operation runs through the runtime's own
//! attach dispatch with the phone's scopes, `thread.read` and `run.write`,
//! and an action on another member's item goes to its owner as a forwarded
//! action. Nothing here runs a model or a tool for the phone.

use muniment_attach::{EventName, Operation};
use serde_json::{json, Value};

use crate::relay_encryption::{ControlContent, MAX_CONTROL_PLAINTEXT_BYTES, MAX_RESPONSE_PAGES};

use super::RelayRuntime;

/// The operations the phone may send on the control session. Each runs as
/// the attach operation of the same name, except `fleet.snapshot`, which
/// reads several of them at once. A `thread.create` that names another
/// member's `runtime_id` starts the thread and its first run there, and a
/// `blob.read` answers one chunk of a blob a fleet item names, which may
/// span several pages.
pub const PHONE_OPERATIONS: [&str; 17] = [
    "fleet.snapshot",
    "thread.summaries",
    "thread.history",
    "thread.history_fetch",
    "thread.post",
    "thread.create",
    "agent.list",
    "agent.read",
    "agent.run_now",
    "agent.approvals",
    "agent.approval_answer",
    "run.cancel",
    "run.steer",
    "blob.read",
    "fleet.op_status",
    "fleet.sync_status",
    "preferences.get",
];

/// The live events the phone follows, with whether each wakes the phone.
pub const FORWARDED_EVENTS: [(&str, bool); 8] = [
    ("swarm.changed", false),
    ("agent.activity", false),
    ("group.posted", false),
    ("agent.approval_pending", true),
    ("agent.approval_resolved", false),
    ("fleet.op_updated", false),
    ("fleet.sync_changed", false),
    ("preferences.changed", false),
];

/// How many threads a snapshot lists at most.
const SNAPSHOT_THREADS: usize = 200;

/// The largest response, as its JSON text, before it becomes `too_large`.
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

/// The JSON text of one page's body stays under this, which leaves room in
/// the sealed plaintext for the counter and the page fields.
const MAX_PAGE_BODY_BYTES: usize = MAX_CONTROL_PLAINTEXT_BYTES - 512;

/// Why a request failed, as the phone reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestError {
    /// `unsupported_operation`, `unauthorized`, `invalid_request`, `failed`,
    /// `unavailable`, `expired` or `too_large`.
    pub code: String,
    pub message: String,
}

impl RequestError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
        }
    }

    pub fn unsupported() -> Self {
        Self::new(
            "unsupported_operation",
            "This machine does not offer that action to a phone.",
        )
    }
}

/// The control session of the pair `pair_id`: `ctl_` and the pair id's hex.
pub fn control_session_id(pair_id: &str) -> String {
    let hex: String = pair_id
        .chars()
        .filter(char::is_ascii_hexdigit)
        .map(|character| character.to_ascii_lowercase())
        .collect();
    format!("ctl_{hex}")
}

/// Whether a live event goes to the phone, and whether it wakes it.
pub fn forwarded(event: &EventName) -> Option<(String, bool)> {
    let name = serde_json::to_value(event).ok()?;
    let name = name.as_str()?;
    FORWARDED_EVENTS
        .iter()
        .find(|(forwarded, _)| *forwarded == name)
        .map(|(name, wake)| ((*name).to_owned(), *wake))
}

/// Answers one phone request through the runtime.
pub fn answer(
    runtime: &dyn RelayRuntime,
    operation: &str,
    body: Value,
    request_id: &str,
) -> Result<Value, RequestError> {
    if !PHONE_OPERATIONS.contains(&operation) {
        return Err(RequestError::unsupported());
    }
    if !body.is_object() {
        return Err(RequestError::new(
            "invalid_request",
            "The request body is not an object.",
        ));
    }
    match operation {
        "fleet.snapshot" => snapshot(runtime, &body, request_id),
        "agent.read" => agent_read(runtime, body, request_id),
        _ => run(runtime, operation, body, request_id),
    }
}

fn run(
    runtime: &dyn RelayRuntime,
    operation: &str,
    body: Value,
    request_id: &str,
) -> Result<Value, RequestError> {
    // An operation this runtime's protocol does not name yet is not offered.
    let operation = serde_json::from_value::<Operation>(json!(operation))
        .map_err(|_| RequestError::unsupported())?;
    runtime.request(operation, body, request_id)
}

/// `agent.read` of an agent another member runs answers its directory
/// entry, since only its primary holds the definition.
fn agent_read(
    runtime: &dyn RelayRuntime,
    body: Value,
    request_id: &str,
) -> Result<Value, RequestError> {
    let id = body["id"].as_str().unwrap_or_default().to_owned();
    match run(runtime, "agent.read", body, request_id) {
        Ok(agent) => Ok(agent),
        Err(error) => {
            let agents = run(runtime, "swarm.agents", json!({}), request_id).ok();
            let entry = agents.as_ref().and_then(|agents| {
                agents["agents"]
                    .as_array()?
                    .iter()
                    .find(|agent| !id.is_empty() && agent["agent_id"] == id.as_str())
                    .cloned()
            });
            entry
                .map(|agent| json!({"agent": agent, "remote": true}))
                .ok_or(error)
        }
    }
}

/// The phone's view of the fleet: its machines, every agent, the threads
/// changed since `since`, pending approvals, groups and the sync status.
fn snapshot(
    runtime: &dyn RelayRuntime,
    body: &Value,
    request_id: &str,
) -> Result<Value, RequestError> {
    let since = match body.get("since") {
        None | Some(Value::Null) => None,
        Some(Value::String(since)) if !since.is_empty() && since.len() <= 64 => Some(since.clone()),
        Some(_) => {
            return Err(RequestError::new(
                "invalid_request",
                "The snapshot version is invalid.",
            ))
        }
    };
    if body
        .as_object()
        .is_some_and(|body| body.keys().any(|key| key != "since"))
    {
        return Err(RequestError::new(
            "invalid_request",
            "A snapshot takes only `since`.",
        ));
    }
    let mut errors = serde_json::Map::new();
    let mut part = |operation: &str, body: Value| -> Option<Value> {
        match run(runtime, operation, body, request_id) {
            Ok(value) => Some(value),
            Err(error) => {
                errors.insert(operation.to_owned(), json!(error.message));
                None
            }
        }
    };
    let members_view = part("swarm.members", json!({}));
    let agents = part("swarm.agents", json!({}));
    let approvals = part("agent.approvals", json!({}));
    let groups = part("group.list", json!({}));
    let sync = part("fleet.sync_status", json!({}));
    let (threads, threads_cursor) = threads(&mut part, since.as_deref());
    // The newest thread's own time, so the next `since` compares like with like.
    let version = threads
        .first()
        .and_then(|thread| thread["updatedAt"].as_str().map(str::to_owned))
        .or_else(|| since.clone())
        .unwrap_or_else(|| "0".to_owned());
    let self_id = members_view
        .as_ref()
        .and_then(|view| view["self"].as_str().map(str::to_owned));
    let members = members_view
        .as_ref()
        .and_then(|view| view["members"].as_array().cloned())
        .unwrap_or_else(|| {
            // A runtime outside a swarm is a fleet of one.
            vec![json!({
                "runtime_id": null, "runtime_name": null, "form_factor": null, "self": true,
                "online": true, "stale": false, "heartbeat_at": null, "last_client_at": null,
                "link": null,
            })]
        });
    Ok(json!({
        "version": version,
        "full": since.is_none(),
        "self": self_id,
        "members": members,
        "agents": agents.map_or(Value::Array(Vec::new()), |agents| agents["agents"].clone()),
        "threads": threads,
        "threads_cursor": threads_cursor,
        "approvals": approvals.map_or(Value::Array(Vec::new()), |approvals| approvals["approvals"].clone()),
        "groups": groups.map_or(Value::Array(Vec::new()), |groups| groups["groups"].clone()),
        "sync": sync,
        "errors": errors,
    }))
}

/// The newest threads, or with `since` the threads updated after it, and
/// the cursor of the next page when more are listed.
fn threads(
    part: &mut impl FnMut(&str, Value) -> Option<Value>,
    since: Option<&str>,
) -> (Vec<Value>, Option<String>) {
    let mut threads = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut body = json!({"limit": 100});
        if let Some(cursor) = &cursor {
            body["cursor"] = json!(cursor);
        }
        let Some(page) = part("thread.summaries", body) else {
            return (threads, None);
        };
        let next = page["nextCursor"].as_str().map(str::to_owned);
        for summary in page["summaries"].as_array().into_iter().flatten() {
            let updated = summary["updatedAt"].as_str().unwrap_or_default();
            if since.is_some_and(|since| updated <= since) {
                return (threads, None);
            }
            threads.push(summary.clone());
        }
        // Without `since` the first page is the snapshot; the phone pages on.
        if since.is_none() || next.is_none() || threads.len() >= SNAPSHOT_THREADS {
            return (threads, next);
        }
        cursor = next;
    }
}

/// The pages of one response.
pub fn pages(request_id: &str, result: Result<Value, RequestError>) -> Vec<ControlContent> {
    let envelope = |result: Result<Value, RequestError>| match result {
        Ok(result) => json!({"ok": true, "result": result}),
        Err(error) => json!({"ok": false, "error": {"code": error.code, "message": error.message}}),
    };
    let mut text = envelope(result).to_string();
    if text.len() > MAX_RESPONSE_BYTES {
        text = envelope(Err(RequestError::new(
            "too_large",
            "The answer is too large for the phone.",
        )))
        .to_string();
    }
    let chunks = chunks(&text);
    let chunks = if chunks.len() > MAX_RESPONSE_PAGES as usize {
        self::chunks(
            &envelope(Err(RequestError::new(
                "too_large",
                "The answer is too large for the phone.",
            )))
            .to_string(),
        )
    } else {
        chunks
    };
    let pages = u32::try_from(chunks.len()).unwrap_or(MAX_RESPONSE_PAGES);
    chunks
        .into_iter()
        .enumerate()
        .map(|(page, body)| ControlContent::Response {
            request_id: request_id.to_owned(),
            page: u32::try_from(page).unwrap_or(0),
            pages,
            body,
        })
        .collect()
}

/// Splits `text` on character boundaries into pieces whose JSON string form
/// fits one page.
fn chunks(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let mut end = rest.len().min(MAX_PAGE_BODY_BYTES);
        loop {
            while !rest.is_char_boundary(end) {
                end -= 1;
            }
            let escaped = serde_json::to_string(&rest[..end]).map_or(usize::MAX, |s| s.len());
            if escaped <= MAX_PAGE_BODY_BYTES {
                break;
            }
            end = end * 3 / 4;
        }
        chunks.push(rest[..end].to_owned());
        rest = &rest[end..];
    }
    if chunks.is_empty() {
        chunks.push(String::new());
    }
    chunks
}

/// The content of one live event, with its body when the body fits a page.
pub fn event(name: &str, body: Value) -> ControlContent {
    let fits = body.is_object()
        && serde_json::to_vec(&body).is_ok_and(|bytes| bytes.len() <= MAX_PAGE_BODY_BYTES);
    ControlContent::Event {
        event: name.to_owned(),
        body: fits.then_some(body),
    }
}

#[cfg(test)]
mod tests;
