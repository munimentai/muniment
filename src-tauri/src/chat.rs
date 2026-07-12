use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{SecondsFormat, Utc};
use muniment_core::journal::reducer::{project_chat, reduce, RunStatus};
use muniment_core::journal::{EventEnvelope, EventPayload, Provenance, RunJournal};
use muniment_core::sidecar::pi_chat::{cancel_command, PiChatEvent, PiRunAdapter, Receipt};
use muniment_core::sidecar::pi_install::resolve_current;
use muniment_core::sidecar::{
    pi_sidecar_config, PiRpcTransport, PiRpcWiring, SidecarStatus, SidecarSupervisor,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{Emitter, Manager};
use uuid::Uuid;

use crate::auth;

const RPC_TIMEOUT: Duration = Duration::from_secs(30);
const QUEUE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChatGrant {
    gateway_url: String,
    virtual_key: String,
    #[serde(default)]
    model: Option<String>,
    receipt_url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitResult {
    run_id: String,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatDelivery {
    Steer,
    FollowUp,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatQueueRequest {
    run_id: String,
    delivery: ChatDelivery,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    run_id: String,
    prompt: Option<String>,
    phase: String,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    receipt: Option<Value>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatEvent {
    run_id: String,
    phase: String,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    receipt: Option<Value>,
}

struct ActiveRun {
    id: String,
    cancelled: Arc<AtomicBool>,
    transport: Arc<Mutex<Option<Arc<PiRpcTransport>>>>,
    adapter: Arc<Mutex<Option<Arc<PiRunAdapter>>>>,
}

struct PiRuntime {
    supervisor: SidecarSupervisor,
    wiring: PiRpcWiring,
    gateway_url: String,
    virtual_key: String,
    model: Option<String>,
}

pub struct ChatState {
    journal: Arc<Mutex<RunJournal>>,
    active: Mutex<Option<ActiveRun>>,
    runtime: Arc<Mutex<Option<PiRuntime>>>,
}

impl ChatState {
    pub fn new(app: &tauri::AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let directory = app.path().app_data_dir()?;
        std::fs::create_dir_all(&directory)?;
        let mut journal = RunJournal::open(directory.join("runs.sqlite3"))?;
        reconcile_interrupted_runs(&mut journal);
        Ok(Self {
            journal: Arc::new(Mutex::new(journal)),
            active: Mutex::new(None),
            runtime: Arc::new(Mutex::new(None)),
        })
    }
}

fn reconcile_interrupted_runs(journal: &mut RunJournal) {
    let Ok(run_ids) = journal.run_ids() else {
        return;
    };
    for run_id in run_ids {
        let Ok(events) = journal.events(&run_id) else {
            continue;
        };
        let Ok(state) = reduce(&events) else {
            continue;
        };
        if state.is_terminal() {
            continue;
        }
        let envelope = event_envelope(
            &run_id,
            state.last_seq + 1,
            "run.needs_attention",
            json!({"reason": "interrupted"}),
        );
        let _ = journal.append(state.last_seq, &envelope);
    }
}

const PROMPT_SERVICE: &str = "ai.muniment.desktop.chat";
const PROMPT_USER: &str = "protected-prompts";

fn prompt_user(subject: Option<&str>, run_id: &str) -> String {
    subject.filter(|value| !value.is_empty()).map_or_else(
        || format!("{PROMPT_USER}:{run_id}"),
        |value| format!("{PROMPT_USER}:{value}:{run_id}"),
    )
}

fn load_prompt(run_id: &str, subject: Option<&str>) -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(PROMPT_SERVICE, &prompt_user(subject, run_id))
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    match entry.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err("Conversation history is unavailable.".to_string()),
    }
}

fn protect_prompt(run_id: &str, prompt: &str, subject: Option<&str>) -> Result<(), String> {
    keyring::Entry::new(PROMPT_SERVICE, &prompt_user(subject, run_id))
        .and_then(|entry| entry.set_password(prompt))
        .map_err(|_| "Conversation history is unavailable.".to_string())
}

fn projection_phase(status: &Option<RunStatus>) -> &'static str {
    match status {
        Some(RunStatus::Streaming) => "streaming",
        Some(RunStatus::Completed) => "complete",
        Some(RunStatus::Cancelled) => "cancelled",
        Some(RunStatus::Failed { .. }) => "failed",
        Some(RunStatus::NeedsAttention(_)) => "interrupted",
        _ => "thinking",
    }
}

#[tauri::command]
pub async fn chat_history(
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
) -> Result<Vec<HistoryEntry>, String> {
    // History is conversation data and follows the same signed-in gate as send.
    let tokens = auth::fresh_tokens(&auth_state)?;
    let journal = state
        .journal
        .lock()
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    journal
        .run_ids()
        .map_err(|_| "Conversation history is unavailable.".to_string())?
        .into_iter()
        .map(|run_id| {
            let projection = project_chat(
                &journal
                    .events(&run_id)
                    .map_err(|_| "Conversation history is unavailable.".to_string())?,
            )
            .map_err(|_| "Conversation history is unavailable.".to_string())?;
            Ok(HistoryEntry {
                prompt: load_prompt(&run_id, tokens.subject.as_deref())?,
                phase: projection_phase(&projection.status).into(),
                text: projection.text,
                receipt: projection.receipt,
                run_id,
            })
        })
        .collect()
}

#[tauri::command]
pub async fn chat_submit(
    app: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
    prompt: String,
) -> Result<SubmitResult, String> {
    let prompt = prompt.trim().to_owned();
    if prompt.is_empty() {
        return Err("Enter a message before sending.".into());
    }
    let tokens = auth::fresh_tokens(&auth_state)?;
    let mut active = state
        .active
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if active.is_some() {
        return Err("A reply is already in progress.".into());
    }

    let grant = fetch_grant(&tokens.access_token)?;
    validate_grant(&grant)?;
    let run_id = Uuid::now_v7().to_string();
    protect_prompt(&run_id, &prompt, tokens.subject.as_deref())?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let transport = Arc::new(Mutex::new(None));
    let adapter = Arc::new(Mutex::new(None));
    *active = Some(ActiveRun {
        id: run_id.clone(),
        cancelled: Arc::clone(&cancelled),
        transport: Arc::clone(&transport),
        adapter: Arc::clone(&adapter),
    });
    drop(active);

    let journal = Arc::clone(&state.journal);
    let runtime = Arc::clone(&state.runtime);
    let result_id = run_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        coordinate(
            app.clone(),
            journal,
            runtime,
            run_id.clone(),
            prompt,
            tokens.access_token,
            grant,
            cancelled,
            transport,
            adapter,
        );
        if let Some(state) = app.try_state::<ChatState>() {
            let mut active = state
                .active
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if active.as_ref().is_some_and(|current| current.id == run_id) {
                *active = None;
            }
        }
    });
    Ok(SubmitResult { run_id: result_id })
}

#[tauri::command]
pub async fn chat_queue(
    state: tauri::State<'_, ChatState>,
    request: ChatQueueRequest,
) -> Result<(), String> {
    queue_message(&state.active, request)
}

fn queue_message(
    active: &Mutex<Option<ActiveRun>>,
    request: ChatQueueRequest,
) -> Result<(), String> {
    let active = active
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let run = active
        .as_ref()
        .filter(|run| run.id == request.run_id)
        .ok_or_else(|| "That reply is no longer active.".to_string())?;
    let transport = run
        .transport
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let adapter = run
        .adapter
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    drop(active);
    let (transport, adapter) = transport
        .zip(adapter)
        .ok_or_else(|| "The reply is not ready for messages yet.".to_string())?;
    let result = match request.delivery {
        ChatDelivery::Steer => adapter.steer(&transport, &request.message, QUEUE_TIMEOUT),
        ChatDelivery::FollowUp => adapter.follow_up(&transport, &request.message, QUEUE_TIMEOUT),
    };
    result.map_err(|error| {
        if error == "Pi queued message must not be empty" {
            "Enter a message before sending.".to_string()
        } else {
            "The message could not be queued. Try again.".to_string()
        }
    })
}

#[tauri::command]
pub async fn chat_cancel(state: tauri::State<'_, ChatState>, run_id: String) -> Result<(), String> {
    let active = state
        .active
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let run = active
        .as_ref()
        .filter(|run| run.id == run_id)
        .ok_or_else(|| "That reply is no longer active.".to_string())?;
    run.cancelled.store(true, Ordering::SeqCst);
    if let Some(transport) = run
        .transport
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
    {
        transport
            .call(cancel_command(), Duration::from_secs(2))
            .map_err(|_| "The reply could not be stopped yet. Try again.".to_string())?;
    }
    Ok(())
}

fn coordinate(
    app: tauri::AppHandle,
    journal: Arc<Mutex<RunJournal>>,
    runtime: Arc<Mutex<Option<PiRuntime>>>,
    run_id: String,
    prompt: String,
    access_token: String,
    grant: ChatGrant,
    cancelled: Arc<AtomicBool>,
    active_transport: Arc<Mutex<Option<Arc<PiRpcTransport>>>>,
    active_adapter: Arc<Mutex<Option<Arc<PiRunAdapter>>>>,
) {
    let mut seq = 0;
    if append_emit(&app, &journal, &run_id, &mut seq, "run.started", json!({})).is_err() {
        return;
    }
    if cancelled.load(Ordering::SeqCst) {
        let _ = append_emit(
            &app,
            &journal,
            &run_id,
            &mut seq,
            "run.cancelled",
            json!({}),
        );
        return;
    }
    let mut runtime = runtime
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let same_grant = runtime.as_ref().is_some_and(|runtime| {
        runtime.gateway_url == grant.gateway_url
            && runtime.virtual_key == grant.virtual_key
            && runtime.model == grant.model
    });
    if !same_grant {
        *runtime = None;
        let root = match std::env::var("MUNIMENT_PI_ROOT") {
            Ok(root) => root,
            Err(_) => {
                fail(
                    &app,
                    &journal,
                    &run_id,
                    &mut seq,
                    "The agent runtime is not installed.",
                );
                return;
            }
        };
        let executable = match resolve_current(std::path::Path::new(&root)) {
            Ok(path) => path,
            Err(_) => {
                fail(
                    &app,
                    &journal,
                    &run_id,
                    &mut seq,
                    "The agent runtime is unavailable.",
                );
                return;
            }
        };
        let mut config = pi_sidecar_config(executable.to_string_lossy());
        // This is a scoped LiteLLM virtual key, never a provider credential. It is
        // inherited by the supervised child only and never serialized or logged.
        config
            .env
            .insert("OPENAI_API_KEY".into(), grant.virtual_key.clone());
        config
            .env
            .insert("OPENAI_BASE_URL".into(), grant.gateway_url.clone());
        if let Some(model) = &grant.model {
            config.env.insert("PI_DEFAULT_MODEL".into(), model.clone());
        }
        let wiring = PiRpcWiring::new();
        let supervisor =
            match SidecarSupervisor::spawn(config, wiring.readiness_probe(Duration::from_secs(10)))
            {
                Ok(value) => value,
                Err(_) => {
                    fail(
                        &app,
                        &journal,
                        &run_id,
                        &mut seq,
                        "The agent runtime could not start.",
                    );
                    return;
                }
            };
        *runtime = Some(PiRuntime {
            supervisor,
            wiring,
            gateway_url: grant.gateway_url.clone(),
            virtual_key: grant.virtual_key.clone(),
            model: grant.model.clone(),
        });
    }
    let runtime = runtime.as_ref().expect("runtime was initialized");
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while runtime.supervisor.status() == SidecarStatus::Starting
        && std::time::Instant::now() < deadline
    {
        if cancelled.load(Ordering::SeqCst) {
            let _ = append_emit(
                &app,
                &journal,
                &run_id,
                &mut seq,
                "run.cancelled",
                json!({}),
            );
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let Some(transport) = runtime.wiring.transport() else {
        fail(
            &app,
            &journal,
            &run_id,
            &mut seq,
            "The agent runtime did not become ready.",
        );
        return;
    };
    *active_transport
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::clone(&transport));
    if cancelled.load(Ordering::SeqCst) {
        let _ = append_emit(
            &app,
            &journal,
            &run_id,
            &mut seq,
            "run.cancelled",
            json!({}),
        );
        return;
    }
    let (adapter, _) = match PiRunAdapter::start(run_id.clone(), &transport, &prompt, RPC_TIMEOUT) {
        Ok(value) => value,
        Err(_) => {
            fail(
                &app,
                &journal,
                &run_id,
                &mut seq,
                "The reply could not be started.",
            );
            return;
        }
    };
    let adapter = Arc::new(adapter);
    *active_adapter
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::clone(&adapter));
    if append_emit(
        &app,
        &journal,
        &run_id,
        &mut seq,
        "model.prompt.accepted",
        json!({}),
    )
    .is_err()
    {
        return;
    }
    let mut aborting = false;
    loop {
        if cancelled.swap(false, Ordering::SeqCst) {
            aborting = true;
            let _ = transport.call(cancel_command(), Duration::from_secs(2));
        }
        match adapter.next(Duration::from_millis(100)) {
            Ok(PiChatEvent::TextDelta(text)) => {
                if append_emit(
                    &app,
                    &journal,
                    &run_id,
                    &mut seq,
                    "model.stream.delta",
                    json!({"text": text}),
                )
                .is_err()
                {
                    break;
                }
            }
            Ok(PiChatEvent::Completed) if aborting => {
                let _ = append_emit(
                    &app,
                    &journal,
                    &run_id,
                    &mut seq,
                    "run.cancelled",
                    json!({}),
                );
                break;
            }
            Ok(PiChatEvent::Completed) => {
                match fetch_receipt(&grant.receipt_url, &access_token, &run_id) {
                    Ok(receipt) => {
                        let _ = append_emit(
                            &app,
                            &journal,
                            &run_id,
                            &mut seq,
                            "run.completed",
                            json!({"receipt": receipt}),
                        );
                        break;
                    }
                    Err(_) => {
                        fail(
                            &app,
                            &journal,
                            &run_id,
                            &mut seq,
                            "The reply finished, but its receipt was unavailable.",
                        );
                        break;
                    }
                }
            }
            Ok(PiChatEvent::Cancelled) => {
                let _ = append_emit(
                    &app,
                    &journal,
                    &run_id,
                    &mut seq,
                    "run.cancelled",
                    json!({}),
                );
                break;
            }
            Ok(PiChatEvent::Failed) => {
                fail(
                    &app,
                    &journal,
                    &run_id,
                    &mut seq,
                    "The model could not complete this reply.",
                );
                break;
            }
            Ok(PiChatEvent::Interleaved | PiChatEvent::PromptAccepted) => {}
            Err(error) if error == "timed out waiting for Pi stream" => {
                if matches!(
                    runtime.supervisor.status(),
                    SidecarStatus::Failed | SidecarStatus::Stopped
                ) {
                    fail(
                        &app,
                        &journal,
                        &run_id,
                        &mut seq,
                        "The agent runtime stopped unexpectedly.",
                    );
                    break;
                }
            }
            Err(_) => {
                fail(
                    &app,
                    &journal,
                    &run_id,
                    &mut seq,
                    "The agent runtime stopped unexpectedly.",
                );
                break;
            }
        }
    }
}

fn append_emit(
    app: &tauri::AppHandle,
    journal: &Arc<Mutex<RunJournal>>,
    run_id: &str,
    seq: &mut u64,
    kind: &str,
    payload: Value,
) -> Result<(), ()> {
    *seq += 1;
    let envelope = event_envelope(run_id, *seq, kind, payload);
    let projection = {
        let mut journal = journal.lock().map_err(|_| ())?;
        journal.append(*seq - 1, &envelope).map_err(|_| ())?;
        project_chat(&journal.events(run_id).map_err(|_| ())?).map_err(|_| ())?
    };
    let phase = projection_phase(&projection.status);
    app.emit(
        "chat-event",
        ChatEvent {
            run_id: run_id.into(),
            phase: phase.into(),
            text: projection.text,
            receipt: projection.receipt,
        },
    )
    .map_err(|_| ())
}

fn event_envelope(run_id: &str, run_seq: u64, kind: &str, payload: Value) -> EventEnvelope {
    EventEnvelope {
        event_id: Uuid::now_v7().to_string(),
        run_id: run_id.into(),
        run_seq,
        event_type: kind.into(),
        event_version: 1,
        envelope_version: 1,
        recorded_at: Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true),
        occurred_at: None,
        correlation_id: None,
        causation_id: None,
        payload: EventPayload::Inline {
            payload_json: payload,
        },
        provenance: Provenance {
            source: "muniment-desktop".into(),
            source_version: env!("CARGO_PKG_VERSION").into(),
            actor_id: None,
            device_id: None,
            rpc_request_id: None,
            capability_versions: None,
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    }
}

fn fail(
    app: &tauri::AppHandle,
    journal: &Arc<Mutex<RunJournal>>,
    run_id: &str,
    seq: &mut u64,
    reason: &str,
) {
    let _ = append_emit(
        app,
        journal,
        run_id,
        seq,
        "run.failed",
        json!({"reason": reason}),
    );
}

fn fetch_grant(access_token: &str) -> Result<ChatGrant, String> {
    let issuer =
        std::env::var("MUNIMENT_ISSUER").unwrap_or_else(|_| "https://api.muniment.ai".into());
    ureq::post(&format!(
        "{}/v1/desktop/chat/config",
        issuer.trim_end_matches('/')
    ))
    .set("Authorization", &format!("Bearer {access_token}"))
    .call()
    .map_err(|_| "Chat configuration is temporarily unavailable.".to_string())?
    .into_json()
    .map_err(|_| "The chat configuration response was invalid.".to_string())
}

fn validate_grant(grant: &ChatGrant) -> Result<(), String> {
    if !grant.gateway_url.starts_with("https://")
        || !grant.receipt_url.starts_with("https://")
        || grant.virtual_key.trim().is_empty()
    {
        return Err("The chat configuration response was invalid.".into());
    }
    Ok(())
}

fn fetch_receipt(url: &str, access_token: &str, run_id: &str) -> Result<Receipt, ()> {
    ureq::post(url)
        .set("Authorization", &format!("Bearer {access_token}"))
        .send_json(json!({"runId": run_id}))
        .map_err(|_| ())?
        .into_json()
        .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn append_test_event(
        journal: &mut RunJournal,
        run_id: &str,
        seq: u64,
        kind: &str,
        payload: Value,
    ) {
        journal
            .append(seq - 1, &event_envelope(run_id, seq, kind, payload))
            .unwrap();
    }

    fn inactive_transport_run(id: &str) -> ActiveRun {
        ActiveRun {
            id: id.into(),
            cancelled: Arc::new(AtomicBool::new(false)),
            transport: Arc::new(Mutex::new(None)),
            adapter: Arc::new(Mutex::new(None)),
        }
    }

    #[test]
    fn queue_request_is_closed_and_typed() {
        for value in [
            json!({"runId":"run-1", "delivery":"later", "message":"hello"}),
            json!({"runId":"run-1", "delivery":"steer", "message":"hello", "extra":true}),
        ] {
            assert!(serde_json::from_value::<ChatQueueRequest>(value).is_err());
        }
        assert!(serde_json::from_value::<ChatQueueRequest>(json!({
            "runId":"run-1", "delivery":"followUp", "message":"hello"
        }))
        .is_ok());
    }

    #[test]
    fn queue_rejects_mismatched_and_not_ready_runs_safely() {
        let active = Mutex::new(Some(inactive_transport_run("run-1")));
        let request = |run_id: &str| ChatQueueRequest {
            run_id: run_id.into(),
            delivery: ChatDelivery::Steer,
            message: "hello".into(),
        };
        assert_eq!(
            queue_message(&active, request("stale-run")).unwrap_err(),
            "That reply is no longer active."
        );
        assert_eq!(
            queue_message(&active, request("run-1")).unwrap_err(),
            "The reply is not ready for messages yet."
        );
    }

    #[test]
    fn startup_reconciles_only_non_terminal_runs_once() {
        let directory = std::env::temp_dir().join(format!("muniment-chat-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("runs.sqlite3");

        {
            let mut journal = RunJournal::open(&path).unwrap();
            append_test_event(&mut journal, "interrupted", 1, "run.started", json!({}));
            append_test_event(
                &mut journal,
                "interrupted",
                2,
                "model.stream.delta",
                json!({"text": "partial"}),
            );
            append_test_event(&mut journal, "completed", 1, "run.started", json!({}));
            append_test_event(&mut journal, "completed", 2, "run.completed", json!({}));

            reconcile_interrupted_runs(&mut journal);

            let interrupted = journal.events("interrupted").unwrap();
            assert_eq!(interrupted.len(), 3);
            assert_eq!(interrupted[2].event_type, "run.needs_attention");
            assert!(matches!(
                reduce(&interrupted).unwrap().status,
                RunStatus::NeedsAttention(_)
            ));
            assert_eq!(journal.events("completed").unwrap().len(), 2);
        }

        {
            let mut reopened = RunJournal::open(&path).unwrap();
            reconcile_interrupted_runs(&mut reopened);
            assert_eq!(reopened.events("interrupted").unwrap().len(), 3);
            assert_eq!(reopened.events("completed").unwrap().len(), 2);
        }

        std::fs::remove_dir_all(directory).unwrap();
    }
}
