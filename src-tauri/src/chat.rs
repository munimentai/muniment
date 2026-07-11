//! Native chat bridge. Session credentials remain on the Rust side: the
//! control plane resolves the signed-in user's LiteLLM virtual key server-side.

use std::io::{BufRead, BufReader};
use std::time::{SystemTime, UNIX_EPOCH};

use muniment_core::auth;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::auth::AuthState;

const MAX_EVENT_LINE: usize = 1_048_576;

#[derive(Clone, Serialize)]
struct ChatDelta<'a> {
    request_id: &'a str,
    delta: &'a str,
}

#[derive(Clone, Serialize)]
struct ChatDone<'a> {
    request_id: &'a str,
    route: &'a str,
    model: &'a str,
}

#[derive(Deserialize)]
struct ThreadEnvelope {
    thread: Thread,
}

#[derive(Deserialize)]
struct Thread {
    id: String,
}

fn endpoint(path: &str) -> Result<String, String> {
    let mut issuer = url::Url::parse(&crate::auth::config().issuer)
        .map_err(|_| "The control-plane address is invalid.".to_string())?;
    issuer.set_path(path);
    issuer.set_query(None);
    issuer.set_fragment(None);
    Ok(issuer.to_string())
}

fn create_thread(agent: &ureq::Agent, token: &str, title: &str) -> Result<String, String> {
    let response = agent.post(&endpoint("/v1/threads")?)
        .set("Authorization", &format!("Bearer {token}"))
        .send_json(json!({ "title": title }))
        .map_err(http_error)?;
    response.into_json::<ThreadEnvelope>().map(|value| value.thread.id)
        .map_err(|_| "The thread response was unreadable.".to_string())
}

fn http_error(error: ureq::Error) -> String {
    match error {
        ureq::Error::Status(401, _) => "Your session expired. Sign in again.".into(),
        ureq::Error::Status(409, _) => "Chat is not provisioned for this account yet.".into(),
        ureq::Error::Status(code, _) => format!("The chat service returned status {code}."),
        ureq::Error::Transport(_) => "The chat service could not be reached.".into(),
    }
}

#[tauri::command]
pub async fn chat_send(app: AppHandle, state: tauri::State<'_, AuthState>, request_id: String,
    thread_id: Option<String>, content: String) -> Result<String, String> {
    let content = content.trim().to_owned();
    if content.is_empty() || content.len() > 100_000 { return Err("Enter a message under 100,000 characters.".into()); }
    let store_state = state.inner().store();
    let config = crate::auth::config();
    let skew = crate::auth::refresh_skew();
    tauri::async_runtime::spawn_blocking(move || {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|v| v.as_secs()).unwrap_or(0);
        auth::ensure_fresh(store_state.as_ref(), &config, now, skew).map_err(|e| e.to_string())?;
        let token = store_state.load().map_err(|e| e.to_string())?.map(|t| t.access_token)
            .ok_or_else(|| "Sign in again to send a message.".to_string())?;
        let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(120)).build();
        let active_thread = match thread_id { Some(id) => id, None => create_thread(&agent, &token, &content.chars().take(72).collect::<String>())? };
        let response = agent.post(&endpoint("/v1/chat")?)
            .set("Authorization", &format!("Bearer {token}"))
            .send_json(json!({ "thread_id": active_thread, "content": content }))
            .map_err(http_error)?;
        let mut event = String::new();
        let mut route = String::new(); let mut model = String::new();
        for line in BufReader::new(response.into_reader()).lines() {
            let line = line.map_err(|_| "The response stream was interrupted.".to_string())?;
            if line.len() > MAX_EVENT_LINE { return Err("The response stream contained an oversized event.".into()); }
            if let Some(value) = line.strip_prefix("event:") { event = value.trim().to_owned(); continue; }
            let Some(data) = line.strip_prefix("data:").map(str::trim) else { continue; };
            let parsed: Value = serde_json::from_str(data).map_err(|_| "The response stream was unreadable.".to_string())?;
            if event == "error" { return Err(parsed.pointer("/error/message").and_then(Value::as_str).unwrap_or("Chat could not complete.").to_owned()); }
            if event == "done" {
                route = parsed.get("route").and_then(Value::as_str).unwrap_or("").to_owned();
                model = parsed.get("model").and_then(Value::as_str).unwrap_or("").to_owned();
            } else if let Some(delta) = parsed.pointer("/choices/0/delta/content").and_then(Value::as_str) {
                app.emit("chat://delta", ChatDelta { request_id: &request_id, delta }).map_err(|_| "Could not deliver the response.".to_string())?;
            }
            event.clear();
        }
        app.emit("chat://done", ChatDone { request_id: &request_id, route: &route, model: &model }).map_err(|_| "Could not finish the response.".to_string())?;
        Ok(active_thread)
    }).await.map_err(|e| format!("Chat task failed: {e}"))?
}
