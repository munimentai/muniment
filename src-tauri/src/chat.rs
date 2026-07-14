use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{SecondsFormat, Utc};
use muniment_core::journal::reducer::{
    project_chat, reduce, ChatProjection, ChatProjector, PermissionGate, PermissionRequest,
    RunStatus,
};
use muniment_core::journal::{EventEnvelope, EventPayload, Provenance, RunJournal};
use muniment_core::sidecar::pi_chat::{
    cancel_command, ExtensionUiDialog, ExtensionUiRequest, PiChatEvent, PiRunAdapter, Receipt,
};
use muniment_core::sidecar::pi_install::resolve_current;
use muniment_core::sidecar::{
    pi_sidecar_config, validate_pi_session, PiRpcTransport, PiRpcWiring, SidecarStatus,
    SidecarSupervisor,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pending_permission: Option<ChatPendingPermission>,
    resumable: bool,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pending_permission: Option<ChatPendingPermission>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatPendingPermission {
    gate_id: String,
    #[serde(flatten)]
    request: PermissionRequest,
}

fn chat_pending_permission(gate: Option<PermissionGate>) -> Option<ChatPendingPermission> {
    gate.map(|gate| ChatPendingPermission {
        gate_id: gate.gate_id,
        request: gate.request,
    })
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
        std::fs::create_dir_all(directory.join("pi-sessions"))?;
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
        // A blocking permission remains a blocking permission after restart;
        // explicit resume must never stand in for the user's answer.
        if matches!(state.status, RunStatus::PendingPermission(_)) {
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
        Some(RunStatus::PendingPermission(_)) => "pending-permission",
        _ => "thinking",
    }
}

fn history_entries(
    journal: &mut RunJournal,
    subject: Option<&str>,
    session_root: &std::path::Path,
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
        let state =
            reduce(&events).map_err(|_| "Conversation history is unavailable.".to_string())?;
        let resumable = matches!(state.status, RunStatus::NeedsAttention(_))
            && projection.pending_permission.is_none()
            && state
                .pi_session
                .as_ref()
                .is_some_and(|binding| validate_pi_session(session_root, &binding.locator).is_ok());
        entries.push(HistoryEntry {
            prompt: load_prompt(&run_id, subject)?,
            phase: projection_phase(&projection.status).into(),
            text: projection.text,
            receipt: projection.receipt,
            tool_activity: chat_tool_activity(&projection.tool_activity),
            pending_permission: chat_pending_permission(projection.pending_permission),
            resumable,
            run_id,
        });
    }
    Ok(entries)
}

#[tauri::command]
pub async fn chat_history(
    app: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
) -> Result<Vec<HistoryEntry>, String> {
    // History is conversation data and follows the same signed-in gate as send.
    let tokens = auth::fresh_tokens(&auth_state)?;
    let mut journal = state
        .journal
        .lock()
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    let session_root = app
        .path()
        .app_data_dir()
        .map_err(|_| "Conversation history is unavailable.".to_string())?
        .join("pi-sessions");
    history_entries(&mut journal, tokens.subject.as_deref(), &session_root)
}

const RESUME_REQUEST: &str = "Continue the interrupted reply from the current session state. Do not repeat completed work or prior effects.";

#[tauri::command]
pub async fn chat_resume(
    app: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
    run_id: String,
) -> Result<SubmitResult, String> {
    if state
        .active
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_some()
    {
        return Err("A reply is already in progress.".into());
    }
    let tokens = auth::fresh_tokens_async(&auth_state).await?;
    let session_root = app
        .path()
        .app_data_dir()
        .map_err(|_| "This reply cannot be resumed. Try again.".to_string())?
        .join("pi-sessions");
    let locator = {
        let journal = state
            .journal
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let events = journal
            .events(&run_id)
            .map_err(|_| "This reply cannot be resumed. Try again.".to_string())?;
        let owner = events
            .first()
            .and_then(|event| event.provenance.actor_id.as_deref());
        if owner != tokens.subject.as_deref() {
            return Err("This reply cannot be resumed. Try again.".into());
        }
        let projection = project_chat(&events)
            .map_err(|_| "This reply cannot be resumed. Try again.".to_string())?;
        let reduced =
            reduce(&events).map_err(|_| "This reply cannot be resumed. Try again.".to_string())?;
        if !matches!(reduced.status, RunStatus::NeedsAttention(_))
            || projection.pending_permission.is_some()
        {
            return Err("This reply cannot be resumed. Try again.".into());
        }
        let binding = reduced
            .pi_session
            .ok_or_else(|| "This reply cannot be resumed. Try again.".to_string())?;
        validate_pi_session(&session_root, &binding.locator)
            .map_err(|_| "This reply cannot be resumed. Try again.".to_string())?;
        binding.locator
    };
    let access_token = tokens.access_token.clone();
    let grant = tauri::async_runtime::spawn_blocking(move || {
        let grant = fetch_grant(&access_token)?;
        validate_grant(&grant)?;
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
    let result_id = run_id.clone();
    let journal = Arc::clone(&state.journal);
    let runtime = Arc::clone(&state.runtime);
    tauri::async_runtime::spawn_blocking(move || {
        coordinate(
            app.clone(),
            journal,
            runtime,
            run_id.clone(),
            RESUME_REQUEST.into(),
            tokens.access_token,
            tokens.subject,
            grant,
            cancelled,
            transport,
            adapter,
            Some(locator),
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
            None,
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
        ChatQueueRequest {
            run_id,
            delivery,
            message,
        },
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
    resume_locator: Option<String>,
) {
    let mut seq = 0;
    let mut projector = ChatProjector::new();
    let mut journaled_effects = BTreeSet::new();
    if resume_locator.is_some() {
        let events = journal
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .events(&run_id);
        let Ok(events) = events else { return };
        for event in &events {
            if projector.apply(event).is_err() {
                return;
            }
            if event.event_type == "tool.effect.started" {
                if let EventPayload::Inline { payload_json } = &event.payload {
                    if let Some(effect_id) = payload_json.get("effect_id").and_then(Value::as_str) {
                        journaled_effects.insert(effect_id.to_owned());
                    }
                }
            }
        }
        seq = events.last().map_or(0, |event| event.run_seq);
    } else if append_emit(
        &app,
        &journal,
        &mut projector,
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
            &mut projector,
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
    // A local run owns one Pi conversation. Do not carry a previous run's
    // active session into this prompt.
    *runtime = None;
    {
        let root = match std::env::var("MUNIMENT_PI_ROOT") {
            Ok(root) => root,
            Err(_) => {
                fail(
                    &app,
                    &journal,
                    &mut projector,
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
                    &mut projector,
                    &run_id,
                    &mut seq,
                    "The agent runtime is unavailable.",
                    subject.as_deref(),
                );
                return;
            }
        };
        let session_root = match app.path().app_data_dir() {
            Ok(path) => path.join("pi-sessions"),
            Err(_) => {
                fail(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    "The agent runtime is unavailable.",
                    subject.as_deref(),
                );
                return;
            }
        };
        let mut config = match pi_sidecar_config(
            executable.to_string_lossy(),
            &session_root,
            resume_locator.as_deref(),
        ) {
            Ok(config) => config,
            Err(_) => {
                fail(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    "The agent runtime is unavailable.",
                    subject.as_deref(),
                );
                return;
            }
        };
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
                        &mut projector,
                        &run_id,
                        &mut seq,
                        "The agent runtime could not start.",
                        subject.as_deref(),
                    );
                    return;
                }
            };
        *runtime = Some(PiRuntime { supervisor, wiring });
    }
    let runtime = runtime.as_mut().expect("runtime was initialized");
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while runtime.supervisor.status() == SidecarStatus::Starting
        && std::time::Instant::now() < deadline
    {
        if cancelled.load(Ordering::SeqCst) {
            let _ = append_emit(
                &app,
                &journal,
                &mut projector,
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
            &mut projector,
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
            &mut projector,
            &run_id,
            &mut seq,
            "run.cancelled",
            json!({}),
            subject.as_deref(),
        );
        return;
    }
    if resume_locator.is_some()
        && append_emit(
            &app,
            &journal,
            &mut projector,
            &run_id,
            &mut seq,
            "run.resumed",
            json!({}),
            subject.as_deref(),
        )
        .is_err()
    {
        return;
    }
    let (adapter, _) = match PiRunAdapter::start(run_id.clone(), &transport, &prompt, RPC_TIMEOUT) {
        Ok(value) => value,
        Err(_) => {
            if resume_locator.is_some() {
                let _ = append_emit(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    "run.needs_attention",
                    json!({"reason":"resume_failed"}),
                    subject.as_deref(),
                );
            } else {
                fail(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    "The reply could not be started.",
                    subject.as_deref(),
                );
            }
            return;
        }
    };
    let adapter = Arc::new(adapter);
    *active_adapter
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::clone(&adapter));
    let session_root = match app.path().app_data_dir() {
        Ok(path) => path.join("pi-sessions"),
        Err(_) => {
            if adapter
                .cancel_and_drain(&transport, Duration::from_secs(2))
                .is_err()
            {
                let _ = runtime.supervisor.shutdown();
            }
            fail(
                &app,
                &journal,
                &mut projector,
                &run_id,
                &mut seq,
                "The reply could not be started.",
                subject.as_deref(),
            );
            return;
        }
    };
    let buffered_events = if resume_locator.is_some() {
        Vec::new()
    } else {
        let (locator, buffered_events) =
            match adapter.await_session_binding(&transport, &session_root, RPC_TIMEOUT) {
                Ok(binding) => binding,
                Err(_) => {
                    // `await_session_binding` aborts and drains first. Reaping the
                    // supervised child is the final containment boundary if Pi did
                    // not acknowledge cancellation.
                    let _ = runtime.supervisor.shutdown();
                    fail(
                        &app,
                        &journal,
                        &mut projector,
                        &run_id,
                        &mut seq,
                        "The reply could not be started.",
                        subject.as_deref(),
                    );
                    return;
                }
            };
        if append_emit(
            &app,
            &journal,
            &mut projector,
            &run_id,
            &mut seq,
            "runtime.pi_session.bound",
            json!({"run_id": run_id, "locator": locator.as_str()}),
            subject.as_deref(),
        )
        .is_err()
        {
            if adapter
                .cancel_and_drain(&transport, Duration::from_secs(2))
                .is_err()
            {
                let _ = runtime.supervisor.shutdown();
            }
            return;
        }
        buffered_events
    };
    if append_emit(
        &app,
        &journal,
        &mut projector,
        &run_id,
        &mut seq,
        "model.prompt.accepted",
        json!({}),
        subject.as_deref(),
    )
    .is_err()
    {
        if adapter
            .cancel_and_drain(&transport, Duration::from_secs(2))
            .is_err()
        {
            let _ = runtime.supervisor.shutdown();
        }
        return;
    }
    let mut buffered_events = buffered_events.into_iter();
    let mut aborting = false;
    let mut open_effects = BTreeSet::new();
    loop {
        if cancelled.swap(false, Ordering::SeqCst) {
            aborting = true;
            let _ = transport.call(cancel_command(), Duration::from_secs(2));
        }
        let event = buffered_events
            .next()
            .map(Ok)
            .unwrap_or_else(|| adapter.next(Duration::from_millis(100)));
        match event {
            Ok(PiChatEvent::TextDelta(text)) => {
                if append_emit(
                    &app,
                    &journal,
                    &mut projector,
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
                    &mut projector,
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
                            &mut projector,
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
                            &mut projector,
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
                    &mut projector,
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
                    &mut projector,
                    &run_id,
                    &mut seq,
                    &mut open_effects,
                    "The model could not complete this reply.",
                    subject.as_deref(),
                );
                break;
            }
            Ok(event @ (PiChatEvent::ToolStarted { .. } | PiChatEvent::ToolFinished { .. })) => {
                if matches!(&event, PiChatEvent::ToolStarted { tool_call_id, .. } if journaled_effects.contains(tool_call_id))
                {
                    continue;
                }
                if let Some((kind, payload)) = tool_journal_entry(&event, &mut open_effects) {
                    if let PiChatEvent::ToolStarted { tool_call_id, .. } = &event {
                        journaled_effects.insert(tool_call_id.clone());
                    }
                    if append_emit(
                        &app,
                        &journal,
                        &mut projector,
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
            Ok(event @ PiChatEvent::ExtensionUiRequest(_)) => {
                if coordinate_extension_ui_request(event, |kind, payload| {
                    append_emit(
                        &app,
                        &journal,
                        &mut projector,
                        &run_id,
                        &mut seq,
                        kind,
                        payload,
                        subject.as_deref(),
                    )
                })
                .is_err()
                {
                    break;
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
                        &mut projector,
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
                    &mut projector,
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

fn coordinate_extension_ui_request(
    event: PiChatEvent,
    append: impl FnOnce(&str, Value) -> Result<(), ()>,
) -> Result<(), ()> {
    let PiChatEvent::ExtensionUiRequest(request) = event else {
        return Ok(());
    };
    append("permission.requested", permission_journal_payload(&request))
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

fn permission_journal_payload(request: &ExtensionUiRequest) -> Value {
    let gate_id = request.id.clone();
    let timeout = request.timeout;
    let request = match &request.dialog {
        ExtensionUiDialog::Select { title, options } => PermissionRequest::Select {
            title: title.clone(),
            options: options.clone(),
            timeout,
        },
        ExtensionUiDialog::Confirm { title, message } => PermissionRequest::Confirm {
            title: title.clone(),
            message: message.clone(),
            timeout,
        },
        ExtensionUiDialog::Input { title, placeholder } => PermissionRequest::Input {
            title: title.clone(),
            placeholder: placeholder.clone(),
            timeout,
        },
        ExtensionUiDialog::Editor { title, prefill } => PermissionRequest::Editor {
            title: title.clone(),
            prefill: prefill.clone(),
            timeout,
        },
    };
    serde_json::to_value(PermissionGate { gate_id, request })
        .expect("permission gate is serializable")
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
    projector: &mut ChatProjector,
    run_id: &str,
    seq: &mut u64,
    open_effects: &mut BTreeSet<String>,
    kind: &str,
    payload: Value,
    subject: Option<&str>,
) -> Result<(), ()> {
    close_open_effects(open_effects, |kind, payload| {
        append_emit(app, journal, projector, run_id, seq, kind, payload, subject)
    })?;
    append_emit(app, journal, projector, run_id, seq, kind, payload, subject)
}

fn fail_with_open_effects(
    app: &tauri::AppHandle,
    journal: &Arc<Mutex<RunJournal>>,
    projector: &mut ChatProjector,
    run_id: &str,
    seq: &mut u64,
    open_effects: &mut BTreeSet<String>,
    reason: &str,
    subject: Option<&str>,
) {
    let _ = append_terminal(
        app,
        journal,
        projector,
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
    projector: &mut ChatProjector,
    run_id: &str,
    seq: &mut u64,
    kind: &str,
    payload: Value,
    subject: Option<&str>,
) -> Result<(), ()> {
    *seq += 1;
    let envelope = event_envelope(run_id, *seq, kind, payload, subject);
    let mut next_projector = projector.clone();
    next_projector.apply(&envelope).map_err(|_| ())?;
    let projection = next_projector.projection().map_err(|_| ())?;
    {
        let mut journal = journal.lock().map_err(|_| ())?;
        journal.append(*seq - 1, &envelope).map_err(|_| ())?;
    }
    *projector = next_projector;
    app.emit("chat-event", chat_event(run_id, projection))
        .map_err(|_| ())
}

fn chat_event(run_id: &str, projection: ChatProjection) -> ChatEvent {
    ChatEvent {
        run_id: run_id.into(),
        phase: projection_phase(&projection.status).into(),
        text: projection.text,
        receipt: projection.receipt,
        tool_activity: chat_tool_activity(&projection.tool_activity),
        pending_permission: chat_pending_permission(projection.pending_permission),
    }
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
    projector: &mut ChatProjector,
    run_id: &str,
    seq: &mut u64,
    reason: &str,
    subject: Option<&str>,
) {
    let interrupted = projector
        .projection()
        .ok()
        .and_then(|projection| projection.status)
        .is_some_and(|status| matches!(status, RunStatus::NeedsAttention(_)));
    let _ = append_emit(
        app,
        journal,
        projector,
        run_id,
        seq,
        if interrupted {
            "run.needs_attention"
        } else {
            "run.failed"
        },
        json!({"reason": if interrupted { "resume_failed" } else { reason }}),
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
    fn blocking_dialogs_translate_to_permission_journal_payloads() {
        let cases = [
            (
                ExtensionUiDialog::Select {
                    title: "Choose".into(),
                    options: vec!["A".into(), "B".into()],
                },
                json!({"gate_id":"gate","kind":"select","title":"Choose","options":["A","B"],"timeout":5000}),
            ),
            (
                ExtensionUiDialog::Confirm {
                    title: "Allow?".into(),
                    message: "Proceed?".into(),
                },
                json!({"gate_id":"gate","kind":"confirm","title":"Allow?","message":"Proceed?","timeout":5000}),
            ),
            (
                ExtensionUiDialog::Input {
                    title: "Value".into(),
                    placeholder: Some("Type".into()),
                },
                json!({"gate_id":"gate","kind":"input","title":"Value","placeholder":"Type","timeout":5000}),
            ),
            (
                ExtensionUiDialog::Editor {
                    title: "Edit".into(),
                    prefill: Some("draft".into()),
                },
                json!({"gate_id":"gate","kind":"editor","title":"Edit","prefill":"draft","timeout":5000}),
            ),
        ];
        for (dialog, expected) in cases {
            assert_eq!(
                permission_journal_payload(&ExtensionUiRequest {
                    id: "gate".into(),
                    dialog,
                    timeout: Some(5000),
                }),
                expected
            );
        }
    }

    #[test]
    fn coordinator_journals_extension_ui_before_projecting_and_stops_on_failure() {
        let directory = std::env::temp_dir().join(format!("muniment-chat-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut journal = RunJournal::open(directory.join("runs.sqlite3")).unwrap();
        let run_id = Uuid::now_v7().to_string();
        append_test_event(&mut journal, &run_id, 1, "run.started", json!({}), None);
        let mut projector = ChatProjector::new();
        projector
            .apply(&journal.events(&run_id).unwrap()[0])
            .unwrap();
        let mut emitted = Vec::new();

        coordinate_extension_ui_request(
            PiChatEvent::ExtensionUiRequest(ExtensionUiRequest {
                id: "pi-request-1".into(),
                dialog: ExtensionUiDialog::Confirm {
                    title: "Allow?".into(),
                    message: "Proceed?".into(),
                },
                timeout: Some(5_000),
            }),
            |kind, payload| {
                let envelope = event_envelope(&run_id, 2, kind, payload, None);
                journal.append(1, &envelope).map_err(|_| ())?;
                assert_eq!(journal.events(&run_id).unwrap().len(), 2);
                projector.apply(&envelope).map_err(|_| ())?;
                emitted.push(chat_event(&run_id, projector.projection().map_err(|_| ())?));
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(
            journal.events(&run_id).unwrap()[1].event_type,
            "permission.requested"
        );
        let projection = emitted.pop().unwrap();
        assert_eq!(projection.phase, "pending-permission");
        assert_eq!(
            projection.pending_permission.unwrap().gate_id,
            "pi-request-1"
        );

        let mut append_attempts = 0;
        assert!(coordinate_extension_ui_request(
            PiChatEvent::ExtensionUiRequest(ExtensionUiRequest {
                id: "pi-request-2".into(),
                dialog: ExtensionUiDialog::Input {
                    title: "Secret".into(),
                    placeholder: None,
                },
                timeout: None,
            }),
            |_kind, _payload| {
                append_attempts += 1;
                Err(())
            },
        )
        .is_err());
        assert_eq!(append_attempts, 1);

        let mut emitted_after_projection_failure = false;
        assert!(coordinate_extension_ui_request(
            PiChatEvent::ExtensionUiRequest(ExtensionUiRequest {
                id: "pi-request-3".into(),
                dialog: ExtensionUiDialog::Select {
                    title: "Choose".into(),
                    options: vec!["A".into(), "B".into()],
                },
                timeout: None,
            }),
            |kind, payload| {
                let envelope = event_envelope(&run_id, 3, kind, payload, None);
                let mut next_projector = projector.clone();
                next_projector.apply(&envelope).map_err(|_| ())?;
                next_projector.projection().map_err(|_| ())?;
                journal.append(2, &envelope).map_err(|_| ())?;
                emitted_after_projection_failure = true;
                Ok(())
            },
        )
        .is_err());
        assert!(!emitted_after_projection_failure);
        // The coordinator handler has no response transport and therefore cannot
        // synthesize an allow/deny (or any other Pi extension-UI response).
        let events = journal.events(&run_id).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "run.started");
        assert_eq!(events[1].event_type, "permission.requested");
        let replayed = muniment_core::journal::reducer::project_chat(&events).unwrap();
        assert_eq!(replayed.pending_permission.unwrap().gate_id, "pi-request-1");

        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn history_projects_and_clears_a_typed_pending_permission() {
        keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
        let directory = std::env::temp_dir().join(format!("muniment-chat-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut journal = RunJournal::open(directory.join("runs.sqlite3")).unwrap();
        let run_id = Uuid::now_v7().to_string();
        append_test_event(&mut journal, &run_id, 1, "run.started", json!({}), None);
        append_test_event(
            &mut journal,
            &run_id,
            2,
            "permission.requested",
            json!({
                "gate_id": "pi-request-1",
                "kind": "editor",
                "title": "Review command",
                "prefill": "cargo test",
                "timeout": 30_000
            }),
            None,
        );

        let entries = history_entries(&mut journal, None).unwrap();
        let entry = entries.iter().find(|entry| entry.run_id == run_id).unwrap();
        assert_eq!(entry.phase, "pending-permission");
        let pending = entry.pending_permission.as_ref().unwrap();
        assert_eq!(pending.gate_id, "pi-request-1");
        assert!(matches!(
            &pending.request,
            PermissionRequest::Editor { title, prefill, timeout }
                if title == "Review command"
                    && prefill.as_deref() == Some("cargo test")
                    && *timeout == Some(30_000)
        ));

        append_test_event(
            &mut journal,
            &run_id,
            3,
            "permission.resolved",
            json!({"gate_id": "pi-request-1", "decision": "cancelled"}),
            None,
        );
        let entries = history_entries(&mut journal, None).unwrap();
        let entry = entries.iter().find(|entry| entry.run_id == run_id).unwrap();
        assert!(entry.pending_permission.is_none());
        assert_eq!(entry.phase, "thinking");

        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
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
        let run_a = "01900000-0000-7000-8000-000000000001";
        let run_a_reconciled = "01900000-0000-7000-8000-000000000002";
        let run_b = "01900000-0000-7000-8000-000000000003";
        let run_legacy = "01900000-0000-7000-8000-000000000004";

        append_test_event(
            &mut journal,
            run_a,
            1,
            "run.started",
            json!({}),
            Some("sub-a"),
        );
        append_test_event(
            &mut journal,
            run_a,
            2,
            "model.stream.delta",
            json!({"text": "private-a"}),
            Some("sub-a"),
        );
        append_test_event(
            &mut journal,
            run_a,
            3,
            "run.completed",
            json!({"receipt": {"owner": "sub-a"}}),
            Some("sub-a"),
        );
        append_test_event(
            &mut journal,
            run_a_reconciled,
            1,
            "run.started",
            json!({}),
            Some("sub-a"),
        );
        append_test_event(
            &mut journal,
            run_a_reconciled,
            2,
            "run.needs_attention",
            json!({"reason": "interrupted"}),
            None,
        );

        append_test_event(
            &mut journal,
            run_b,
            1,
            "run.started",
            json!({}),
            Some("sub-b"),
        );
        append_test_event(
            &mut journal,
            run_b,
            2,
            "model.stream.delta",
            json!({"text": "private-b"}),
            Some("sub-b"),
        );
        append_test_event(
            &mut journal,
            run_b,
            3,
            "run.completed",
            json!({"receipt": {"owner": "sub-b"}}),
            Some("sub-b"),
        );

        append_test_event(&mut journal, run_legacy, 1, "run.started", json!({}), None);
        append_test_event(
            &mut journal,
            run_legacy,
            2,
            "run.completed",
            json!({"receipt": {"legacy": true}}),
            None,
        );

        let sub_b = history_entries(&mut journal, Some("sub-b")).unwrap();
        assert_eq!(
            sub_b
                .iter()
                .map(|entry| entry.run_id.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([run_b, run_legacy])
        );
        assert!(sub_b.iter().all(|entry| !entry.text.contains("private-a")));
        assert!(sub_b.iter().all(|entry| {
            entry
                .receipt
                .as_ref()
                .and_then(|receipt| receipt.get("owner"))
                != Some(&json!("sub-a"))
        }));

        let sub_a = history_entries(&mut journal, Some("sub-a")).unwrap();
        assert_eq!(
            sub_a
                .iter()
                .map(|entry| entry.run_id.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([run_a, run_a_reconciled, run_legacy])
        );
        assert_eq!(
            sub_a
                .iter()
                .find(|entry| entry.run_id == run_a)
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
                "runtime.pi_session.bound",
                json!({"run_id": interrupted, "locator": "session.jsonl"}),
                None,
            );
            append_test_event(
                &mut journal,
                &interrupted,
                3,
                "permission.requested",
                json!({"gate_id":"gate-1","kind":"confirm","title":"Allow?","message":"Proceed?"}),
                None,
            );
            let pending = project_chat(&journal.events(&interrupted).unwrap()).unwrap();
            assert_eq!(projection_phase(&pending.status), "pending-permission");
            assert_eq!(pending.pending_permission.unwrap().gate_id, "gate-1");
            append_test_event(&mut journal, &completed, 1, "run.started", json!({}), None);
            append_test_event(
                &mut journal,
                &completed,
                2,
                "run.completed",
                json!({}),
                None,
            );
        }

        {
            let mut reopened = RunJournal::open(&path).unwrap();
            reconcile_interrupted_runs(&mut reopened);
            let interrupted_events = reopened.events(&interrupted).unwrap();
            assert_eq!(interrupted_events.len(), 4);
            assert_eq!(interrupted_events[3].event_type, "run.needs_attention");
            assert_eq!(interrupted_events[3].provenance.actor_id, None);
            let state = reduce(&interrupted_events).unwrap();
            assert!(matches!(state.status, RunStatus::NeedsAttention(_)));
            assert_eq!(state.pi_session.unwrap().locator, "session.jsonl");
            assert_eq!(reopened.events(&completed).unwrap().len(), 2);

            reconcile_interrupted_runs(&mut reopened);
            assert_eq!(reopened.events(&interrupted).unwrap().len(), 4);
            assert_eq!(reopened.events(&completed).unwrap().len(), 2);
        }

        std::fs::remove_dir_all(directory).unwrap();
    }
}
