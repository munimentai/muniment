use std::collections::{BTreeMap, BTreeSet};
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
    tool_activity: Vec<ChatToolActivity>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatToolActivity {
    effect_id: String,
    display_name: Option<String>,
    status: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatEvent {
    run_id: String,
    phase: String,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    receipt: Option<Value>,
    tool_activity: Vec<ChatToolActivity>,
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
            None,
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

fn history_entries(
    journal: &RunJournal,
    subject: Option<&str>,
) -> Result<Vec<HistoryEntry>, String> {
    let mut entries = Vec::new();
    for run_id in journal
        .run_ids()
        .map_err(|_| "Conversation history is unavailable.".to_string())?
    {
        let events = journal
            .events(&run_id)
            .map_err(|_| "Conversation history is unavailable.".to_string())?;
        if matches!(
            events.first().and_then(|event| event.provenance.actor_id.as_deref()),
            Some(owner) if Some(owner) != subject
        ) {
            continue;
        }
        let projection = project_chat(&events)
            .map_err(|_| "Conversation history is unavailable.".to_string())?;
        entries.push(HistoryEntry {
            prompt: load_prompt(&run_id, subject)?,
            phase: projection_phase(&projection.status).into(),
            text: projection.text,
            receipt: projection.receipt,
            tool_activity: chat_tool_activity(&projection.tool_activity),
            run_id,
        });
    }
    Ok(entries)
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
    history_entries(&journal, tokens.subject.as_deref())
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
    {
        let active = state
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if active.is_some() {
            return Err("A reply is already in progress.".into());
        }
    }

    let tokens = auth::fresh_tokens_async(&auth_state).await?;
    let run_id = Uuid::now_v7().to_string();
    let access_token = tokens.access_token.clone();
    let protected_run_id = run_id.clone();
    let protected_prompt = prompt.clone();
    let subject = tokens.subject.clone();
    let grant = tauri::async_runtime::spawn_blocking(move || {
        let grant = fetch_grant(&access_token)?;
        validate_grant(&grant)?;
        protect_prompt(&protected_run_id, &protected_prompt, subject.as_deref())?;
        Ok::<_, String>(grant)
    })
    .await
    .map_err(|_| "Chat configuration is temporarily unavailable.".to_string())??;
    let cancelled = Arc::new(AtomicBool::new(false));
    let transport = Arc::new(Mutex::new(None));
    let adapter = Arc::new(Mutex::new(None));
    install_active_run(
        &state.active,
        ActiveRun {
            id: run_id.clone(),
            cancelled: Arc::clone(&cancelled),
            transport: Arc::clone(&transport),
            adapter: Arc::clone(&adapter),
        },
    )?;

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
            tokens.subject,
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

fn install_active_run(active: &Mutex<Option<ActiveRun>>, run: ActiveRun) -> Result<(), String> {
    let mut active = active
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if active.is_some() {
        return Err("A reply is already in progress.".into());
    }
    *active = Some(run);
    Ok(())
}

#[tauri::command]
pub async fn chat_queue(
    state: tauri::State<'_, ChatState>,
    run_id: String,
    delivery: ChatDelivery,
    message: String,
) -> Result<(), String> {
    queue_message(
        &state.active,
        ChatQueueRequest { run_id, delivery, message },
    )
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
    let cancelled = Arc::clone(&run.cancelled);
    let transport = run
        .transport
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    drop(active);
    cancelled.store(true, Ordering::SeqCst);
    if let Some(transport) = transport {
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
    subject: Option<String>,
    grant: ChatGrant,
    cancelled: Arc<AtomicBool>,
    active_transport: Arc<Mutex<Option<Arc<PiRpcTransport>>>>,
    active_adapter: Arc<Mutex<Option<Arc<PiRunAdapter>>>>,
) {
    let mut seq = 0;
    if append_emit(
        &app,
        &journal,
        &run_id,
        &mut seq,
        "run.started",
        json!({}),
        subject.as_deref(),
    )
    .is_err()
    {
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
            subject.as_deref(),
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
                    subject.as_deref(),
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
                    subject.as_deref(),
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
                        subject.as_deref(),
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
                subject.as_deref(),
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
            subject.as_deref(),
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
            subject.as_deref(),
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
                subject.as_deref(),
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
        subject.as_deref(),
    )
    .is_err()
    {
        return;
    }
    let mut aborting = false;
    let mut open_effects = BTreeSet::new();
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
                    subject.as_deref(),
                )
                .is_err()
                {
                    break;
                }
            }
            Ok(PiChatEvent::Completed) if aborting => {
                let _ = append_terminal(
                    &app,
                    &journal,
                    &run_id,
                    &mut seq,
                    &mut open_effects,
                    "run.cancelled",
                    json!({}),
                    subject.as_deref(),
                );
                break;
            }
            Ok(PiChatEvent::Completed) => {
                match fetch_receipt(&grant.receipt_url, &access_token, &run_id) {
                    Ok(receipt) => {
                        let _ = append_terminal(
                            &app,
                            &journal,
                            &run_id,
                            &mut seq,
                            &mut open_effects,
                            "run.completed",
                            json!({"receipt": receipt}),
                            subject.as_deref(),
                        );
                        break;
                    }
                    Err(_) => {
                        fail_with_open_effects(
                            &app,
                            &journal,
                            &run_id,
                            &mut seq,
                            &mut open_effects,
                            "The reply finished, but its receipt was unavailable.",
                            subject.as_deref(),
                        );
                        break;
                    }
                }
            }
            Ok(PiChatEvent::Cancelled) => {
                let _ = append_terminal(
                    &app,
                    &journal,
                    &run_id,
                    &mut seq,
                    &mut open_effects,
                    "run.cancelled",
                    json!({}),
                    subject.as_deref(),
                );
                break;
            }
            Ok(PiChatEvent::Failed) => {
                fail_with_open_effects(
                    &app,
                    &journal,
                    &run_id,
                    &mut seq,
                    &mut open_effects,
                    "The model could not complete this reply.",
                    subject.as_deref(),
                );
                break;
            }
            Ok(event @ (PiChatEvent::ToolStarted { .. } | PiChatEvent::ToolFinished { .. })) => {
                if let Some((kind, payload)) = tool_journal_entry(&event, &mut open_effects) {
                    if append_emit(
                        &app,
                        &journal,
                        &run_id,
                        &mut seq,
                        kind,
                        payload,
                        subject.as_deref(),
                    )
                    .is_err()
                    {
                        break;
                    }
                }
            }
            Ok(PiChatEvent::Interleaved | PiChatEvent::PromptAccepted) => {}
            Err(error) if error == "timed out waiting for Pi stream" => {
                if matches!(
                    runtime.supervisor.status(),
                    SidecarStatus::Failed | SidecarStatus::Stopped
                ) {
                    fail_with_open_effects(
                        &app,
                        &journal,
                        &run_id,
                        &mut seq,
                        &mut open_effects,
                        "The agent runtime stopped unexpectedly.",
                        subject.as_deref(),
                    );
                    break;
                }
            }
            Err(_) => {
                fail_with_open_effects(
                    &app,
                    &journal,
                    &run_id,
                    &mut seq,
                    &mut open_effects,
                    "The agent runtime stopped unexpectedly.",
                    subject.as_deref(),
                );
                break;
            }
        }
    }
}

fn tool_journal_entry(
    event: &PiChatEvent,
    open_effects: &mut BTreeSet<String>,
) -> Option<(&'static str, Value)> {
    match event {
        PiChatEvent::ToolStarted {
            tool_call_id,
            tool_name,
        } if open_effects.insert(tool_call_id.clone()) => Some((
            "tool.effect.started",
            json!({"effect_id": tool_call_id, "display_name": tool_name}),
        )),
        PiChatEvent::ToolFinished {
            tool_call_id,
            failed,
        } if open_effects.remove(tool_call_id) => Some((
            if *failed {
                "tool.effect.failed"
            } else {
                "tool.effect.completed"
            },
            json!({"effect_id": tool_call_id}),
        )),
        _ => None,
    }
}

fn close_open_effects(
    open_effects: &mut BTreeSet<String>,
    mut append: impl FnMut(&str, Value) -> Result<(), ()>,
) -> Result<(), ()> {
    for effect_id in open_effects.clone() {
        append("tool.effect.failed", json!({"effect_id": effect_id}))?;
        open_effects.remove(&effect_id);
    }
    Ok(())
}

fn append_terminal(
    app: &tauri::AppHandle,
    journal: &Arc<Mutex<RunJournal>>,
    run_id: &str,
    seq: &mut u64,
    open_effects: &mut BTreeSet<String>,
    kind: &str,
    payload: Value,
    subject: Option<&str>,
) -> Result<(), ()> {
    close_open_effects(open_effects, |kind, payload| {
        append_emit(app, journal, run_id, seq, kind, payload, subject)
    })?;
    append_emit(app, journal, run_id, seq, kind, payload, subject)
}

fn fail_with_open_effects(
    app: &tauri::AppHandle,
    journal: &Arc<Mutex<RunJournal>>,
    run_id: &str,
    seq: &mut u64,
    open_effects: &mut BTreeSet<String>,
    reason: &str,
    subject: Option<&str>,
) {
    let _ = append_terminal(
        app,
        journal,
        run_id,
        seq,
        open_effects,
        "run.failed",
        json!({"reason": reason}),
        subject,
    );
}

fn chat_tool_activity(
    activity: &[muniment_core::journal::reducer::ToolActivity],
) -> Vec<ChatToolActivity> {
    activity
        .iter()
        .map(|activity| ChatToolActivity {
            effect_id: activity.effect_id.clone(),
            display_name: activity.display_name.clone(),
            status: match activity.status {
                muniment_core::journal::reducer::ToolActivityStatus::Running => "running",
                muniment_core::journal::reducer::ToolActivityStatus::Completed => "completed",
                muniment_core::journal::reducer::ToolActivityStatus::Failed => "failed",
            }
            .into(),
        })
        .collect()
}

fn append_emit(
    app: &tauri::AppHandle,
    journal: &Arc<Mutex<RunJournal>>,
    run_id: &str,
    seq: &mut u64,
    kind: &str,
    payload: Value,
    subject: Option<&str>,
) -> Result<(), ()> {
    *seq += 1;
    let envelope = event_envelope(run_id, *seq, kind, payload, subject);
    let projection = {
        let mut journal = journal.lock().map_err(|_| ())?;
        journal.append(*seq - 1, &envelope).map_err(|_| ())?;
        project_chat(&journal.events(run_id).map_err(|_| ())?).map_err(|_| ())?
    };
    let phase = projection_phase(&projection.status);
    let tool_activity = chat_tool_activity(&projection.tool_activity);
    app.emit(
        "chat-event",
        ChatEvent {
            run_id: run_id.into(),
            phase: phase.into(),
            text: projection.text,
            receipt: projection.receipt,
            tool_activity,
        },
    )
    .map_err(|_| ())
}

fn event_envelope(
    run_id: &str,
    run_seq: u64,
    kind: &str,
    payload: Value,
    subject: Option<&str>,
) -> EventEnvelope {
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
            actor_id: subject.map(str::to_owned),
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
    subject: Option<&str>,
) {
    let _ = append_emit(
        app,
        journal,
        run_id,
        seq,
        "run.failed",
        json!({"reason": reason}),
        subject,
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
        subject: Option<&str>,
    ) {
        journal
            .append(
                seq - 1,
                &event_envelope(run_id, seq, kind, payload, subject),
            )
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
    fn event_envelope_records_the_owning_subject() {
        let owned = event_envelope("run-1", 1, "run.started", json!({}), Some("sub-a"));
        assert_eq!(owned.provenance.actor_id.as_deref(), Some("sub-a"));

        let unowned = event_envelope("run-2", 1, "run.started", json!({}), None);
        assert_eq!(unowned.provenance.actor_id, None);
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
    fn concurrent_active_run_installs_allow_exactly_one_run() {
        let active = Arc::new(Mutex::new(None));
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|index| {
                let active = Arc::clone(&active);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    install_active_run(&active, inactive_transport_run(&format!("run-{index}")))
                })
            })
            .collect();

        let results: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let errors: Vec<_> = results
            .iter()
            .filter_map(|result| result.as_ref().err().map(String::as_str))
            .collect();
        assert_eq!(errors, ["A reply is already in progress."]);
    }

    #[test]
    fn tool_frames_ignore_duplicate_starts_and_unmatched_finishes() {
        let mut open_effects = BTreeSet::new();
        let started = PiChatEvent::ToolStarted {
            tool_call_id: "tool-1".into(),
            tool_name: "Read file".into(),
        };
        let unmatched = PiChatEvent::ToolFinished {
            tool_call_id: "missing".into(),
            failed: false,
        };

        assert_eq!(
            tool_journal_entry(&started, &mut open_effects),
            Some((
                "tool.effect.started",
                json!({"effect_id": "tool-1", "display_name": "Read file"})
            ))
        );
        assert!(tool_journal_entry(&started, &mut open_effects).is_none());
        assert!(tool_journal_entry(&unmatched, &mut open_effects).is_none());

        let finished = PiChatEvent::ToolFinished {
            tool_call_id: "tool-1".into(),
            failed: false,
        };
        assert_eq!(
            tool_journal_entry(&finished, &mut open_effects),
            Some(("tool.effect.completed", json!({"effect_id": "tool-1"})))
        );
        assert!(tool_journal_entry(&finished, &mut open_effects).is_none());

        let failed = PiChatEvent::ToolFinished {
            tool_call_id: "tool-2".into(),
            failed: true,
        };
        assert!(open_effects.insert("tool-2".into()));
        assert_eq!(
            tool_journal_entry(&failed, &mut open_effects),
            Some(("tool.effect.failed", json!({"effect_id": "tool-2"})))
        );
    }

    #[test]
    fn terminal_failure_closes_a_tool_before_projecting_the_run() {
        let directory = std::env::temp_dir().join(format!("muniment-chat-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut journal = RunJournal::open(directory.join("runs.sqlite3")).unwrap();
        let run_id = Uuid::now_v7().to_string();
        let mut seq = 1;
        append_test_event(&mut journal, &run_id, seq, "run.started", json!({}), None);
        seq += 1;
        append_test_event(
            &mut journal,
            &run_id,
            seq,
            "tool.effect.started",
            json!({"effect_id": "tool-1", "display_name": "Read file"}),
            None,
        );
        let mut open_effects = BTreeSet::from(["tool-1".to_string()]);

        close_open_effects(&mut open_effects, |kind, payload| {
            seq += 1;
            append_test_event(&mut journal, &run_id, seq, kind, payload, None);
            Ok(())
        })
        .unwrap();
        seq += 1;
        append_test_event(
            &mut journal,
            &run_id,
            seq,
            "run.failed",
            json!({"reason": "runtime stopped"}),
            None,
        );

        let events = journal.events(&run_id).unwrap();
        let projection = project_chat(&events).unwrap();
        assert!(open_effects.is_empty());
        assert!(matches!(projection.status, Some(RunStatus::Failed { .. })));
        assert_eq!(projection.tool_activity.len(), 1);
        assert_eq!(
            projection.tool_activity[0].status,
            muniment_core::journal::reducer::ToolActivityStatus::Failed
        );

        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn history_is_scoped_by_the_first_events_actor() {
        keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
        let directory = std::env::temp_dir().join(format!("muniment-chat-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut journal = RunJournal::open(directory.join("runs.sqlite3")).unwrap();

        append_test_event(
            &mut journal,
            "run-a",
            1,
            "run.started",
            json!({}),
            Some("sub-a"),
        );
        append_test_event(
            &mut journal,
            "run-a",
            2,
            "model.stream.delta",
            json!({"text": "private-a"}),
            Some("sub-a"),
        );
        append_test_event(
            &mut journal,
            "run-a",
            3,
            "run.completed",
            json!({"receipt": {"owner": "sub-a"}}),
            Some("sub-a"),
        );
        append_test_event(
            &mut journal,
            "run-a-reconciled",
            1,
            "run.started",
            json!({}),
            Some("sub-a"),
        );
        append_test_event(
            &mut journal,
            "run-a-reconciled",
            2,
            "run.needs_attention",
            json!({"reason": "interrupted"}),
            None,
        );

        append_test_event(
            &mut journal,
            "run-b",
            1,
            "run.started",
            json!({}),
            Some("sub-b"),
        );
        append_test_event(
            &mut journal,
            "run-b",
            2,
            "model.stream.delta",
            json!({"text": "private-b"}),
            Some("sub-b"),
        );
        append_test_event(
            &mut journal,
            "run-b",
            3,
            "run.completed",
            json!({"receipt": {"owner": "sub-b"}}),
            Some("sub-b"),
        );

        append_test_event(
            &mut journal,
            "run-legacy",
            1,
            "run.started",
            json!({}),
            None,
        );
        append_test_event(
            &mut journal,
            "run-legacy",
            2,
            "run.completed",
            json!({"receipt": {"legacy": true}}),
            None,
        );

        let sub_b = history_entries(&journal, Some("sub-b")).unwrap();
        assert_eq!(
            sub_b
                .iter()
                .map(|entry| entry.run_id.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["run-b", "run-legacy"])
        );
        assert!(sub_b.iter().all(|entry| !entry.text.contains("private-a")));
        assert!(sub_b.iter().all(|entry| {
            entry
                .receipt
                .as_ref()
                .and_then(|receipt| receipt.get("owner"))
                != Some(&json!("sub-a"))
        }));

        let sub_a = history_entries(&journal, Some("sub-a")).unwrap();
        assert_eq!(
            sub_a
                .iter()
                .map(|entry| entry.run_id.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["run-a", "run-a-reconciled", "run-legacy"])
        );
        assert_eq!(
            sub_a
                .iter()
                .find(|entry| entry.run_id == "run-a")
                .unwrap()
                .text,
            "private-a"
        );

        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn startup_reconciles_only_non_terminal_runs_once() {
        let directory = std::env::temp_dir().join(format!("muniment-chat-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("runs.sqlite3");
        let interrupted = Uuid::now_v7().to_string();
        let completed = Uuid::now_v7().to_string();

        {
            let mut journal = RunJournal::open(&path).unwrap();
            append_test_event(
                &mut journal,
                &interrupted,
                1,
                "run.started",
                json!({}),
                None,
            );
            append_test_event(
                &mut journal,
                &interrupted,
                2,
                "model.stream.delta",
                json!({"text": "partial"}),
                None,
            );
            append_test_event(&mut journal, &completed, 1, "run.started", json!({}), None);
            append_test_event(
                &mut journal,
                &completed,
                2,
                "run.completed",
                json!({}),
                None,
            );

            reconcile_interrupted_runs(&mut journal);

            let interrupted_events = journal.events(&interrupted).unwrap();
            assert_eq!(interrupted_events.len(), 3);
            assert_eq!(interrupted_events[2].event_type, "run.needs_attention");
            assert_eq!(interrupted_events[2].provenance.actor_id, None);
            assert!(matches!(
                reduce(&interrupted_events).unwrap().status,
                RunStatus::NeedsAttention(_)
            ));
            assert_eq!(journal.events(&completed).unwrap().len(), 2);
        }

        {
            let mut reopened = RunJournal::open(&path).unwrap();
            reconcile_interrupted_runs(&mut reopened);
            assert_eq!(reopened.events(&interrupted).unwrap().len(), 3);
            assert_eq!(reopened.events(&completed).unwrap().len(), 2);
        }

        std::fs::remove_dir_all(directory).unwrap();
    }
}
