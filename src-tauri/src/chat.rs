use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{SecondsFormat, Utc};
use muniment_core::attachment::{ingest_attachment, AttachmentMetadata};
use muniment_core::cas::LocalCas;
use muniment_core::journal::reducer::{
    project_chat, reduce, ChatProjection, ChatProjector, PermissionGate, PermissionRequest,
    ProjectedAttachment, RunStatus,
};
use muniment_core::journal::{EventEnvelope, EventPayload, Provenance, RunJournal};
use muniment_core::sidecar::pi_chat::{
    cancel_command, ExtensionUiDialog, ExtensionUiRequest, PiChatEvent, PiRunAdapter, Receipt,
};
use muniment_core::sidecar::pi_install::resolve_current;
use muniment_core::sidecar::{
    pi_sidecar_config, validate_pi_session, PiRpcTransport, PiRpcWiring, PiSessionLocator,
    SidecarStatus, SidecarSupervisor,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
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
    attachments: Vec<ChatAttachment>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectedFile {
    path: PathBuf,
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
    attachments: Vec<ChatAttachment>,
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
    attachments: Vec<ChatAttachment>,
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

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAttachment {
    display_name: String,
    byte_length: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    media_type: Option<String>,
}

fn chat_attachments(attachments: &[ProjectedAttachment]) -> Vec<ChatAttachment> {
    attachments
        .iter()
        .map(|attachment| ChatAttachment {
            display_name: attachment.display_name.clone(),
            byte_length: attachment.byte_length,
            media_type: attachment.media_type.clone(),
        })
        .collect()
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

struct ChatStorage {
    journal: RunJournal,
    cas: LocalCas,
}

type SharedStorage = Arc<Mutex<ChatStorage>>;

pub struct ChatState {
    storage: SharedStorage,
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
            storage: Arc::new(Mutex::new(ChatStorage {
                journal,
                cas: LocalCas::open(&directory.join("cas"))?,
            })),
            active: Mutex::new(None),
            runtime: Arc::new(Mutex::new(None)),
        })
    }
}

const RESUME_PROMPT: &str =
    "Continue the interrupted response from the existing session. Do not repeat completed work.";

struct ResumeContext {
    events: Vec<EventEnvelope>,
    locator: PiSessionLocator,
}

struct ResumeAttempt {
    result: Option<std::sync::mpsc::Sender<Result<(), String>>>,
}

impl ResumeAttempt {
    fn new(result: Option<std::sync::mpsc::Sender<Result<(), String>>>) -> Self {
        Self { result }
    }

    fn accepted(&mut self) {
        if let Some(result) = self.result.take() {
            let _ = result.send(Ok(()));
        }
    }
}

impl Drop for ResumeAttempt {
    fn drop(&mut self) {
        if let Some(result) = self.result.take() {
            let _ = result.send(Err("This reply could not be resumed. Try again.".into()));
        }
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
        let resumable = resumable_context(&events, subject, session_root).is_ok();
        entries.push(HistoryEntry {
            prompt: load_prompt(&run_id, subject)?,
            phase: projection_phase(&projection.status).into(),
            text: projection.text,
            receipt: projection.receipt,
            tool_activity: chat_tool_activity(&projection.tool_activity),
            attachments: chat_attachments(&projection.attachments),
            pending_permission: chat_pending_permission(projection.pending_permission),
            resumable,
            run_id,
        });
    }
    Ok(entries)
}

#[tauri::command]
pub async fn chat_history(
    app_handle: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
) -> Result<Vec<HistoryEntry>, String> {
    // History is conversation data and follows the same signed-in gate as send.
    let tokens = auth::fresh_tokens(&auth_state)?;
    let mut storage = state
        .storage
        .lock()
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    let session_root = state_session_root(&app_handle)?;
    history_entries(
        &mut storage.journal,
        tokens.subject.as_deref(),
        &session_root,
    )
}

fn state_session_root(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("pi-sessions"))
        .map_err(|_| "Conversation history is unavailable.".to_string())
}

fn resumable_context(
    events: &[EventEnvelope],
    subject: Option<&str>,
    session_root: &std::path::Path,
) -> Result<ResumeContext, String> {
    if events.is_empty()
        || matches!(
            events.first().and_then(|event| event.provenance.actor_id.as_deref()),
            Some(owner) if Some(owner) != subject
        )
    {
        return Err("This reply cannot be resumed.".into());
    }
    let state = reduce(events).map_err(|_| "This reply cannot be resumed.".to_string())?;
    if !matches!(state.status, RunStatus::NeedsAttention(_))
        || state.pending_permission.is_some()
        || !state.running_effects.is_empty()
    {
        return Err("This reply cannot be resumed.".into());
    }
    let binding = state
        .pi_session
        .ok_or_else(|| "This reply cannot be resumed.".to_string())?;
    let (locator, _) = validate_pi_session(session_root, &binding.locator)
        .map_err(|_| "This reply cannot be resumed.".to_string())?;
    Ok(ResumeContext {
        events: events.to_vec(),
        locator,
    })
}

#[tauri::command]
pub async fn chat_submit(
    app: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
    prompt: String,
    files: Option<Vec<SelectedFile>>,
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
    let storage = Arc::clone(&state.storage);
    let prepared_storage = Arc::clone(&storage);
    let prepared_run_id = run_id.clone();
    let prepared_subject = tokens.subject.clone();
    let prepared = tauri::async_runtime::spawn_blocking(move || {
        prepare_new_run(
            &prepared_storage,
            &prepared_run_id,
            prepared_subject.as_deref(),
            files.unwrap_or_default(),
        )
    })
    .await
    .map_err(|_| attachment_error())
    .and_then(|result| result);
    let (seq, projector) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            let mut active = state
                .active
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if active.as_ref().is_some_and(|current| current.id == run_id) {
                *active = None;
            }
            return Err(error);
        }
    };
    let attachments = chat_attachments(
        &projector
            .projection()
            .map_err(|_| attachment_error())?
            .attachments,
    );
    let runtime = Arc::clone(&state.runtime);
    let result_id = run_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        coordinate(
            app.clone(),
            storage,
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
            None,
            Some((seq, projector)),
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
    Ok(SubmitResult {
        run_id: result_id,
        attachments,
    })
}

#[tauri::command]
pub async fn chat_resume(
    app: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
    run_id: String,
) -> Result<SubmitResult, String> {
    let tokens = auth::fresh_tokens_async(&auth_state).await?;
    let session_root = state_session_root(&app)?;
    let resume = {
        let mut storage = state
            .storage
            .lock()
            .map_err(|_| "This reply cannot be resumed.".to_string())?;
        let events = storage
            .journal
            .events(&run_id)
            .map_err(|_| "This reply cannot be resumed.".to_string())?;
        resumable_context(&events, tokens.subject.as_deref(), &session_root)?
    };
    let attachments = chat_attachments(
        &project_chat(&resume.events)
            .map_err(|_| "This reply could not be resumed.".to_string())?
            .attachments,
    );
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
    let storage = Arc::clone(&state.storage);
    let runtime = Arc::clone(&state.runtime);
    let result_id = run_id.clone();
    let (attempt_sender, attempt_receiver) = std::sync::mpsc::channel();
    tauri::async_runtime::spawn_blocking(move || {
        coordinate(
            app.clone(),
            storage,
            runtime,
            run_id.clone(),
            RESUME_PROMPT.into(),
            tokens.access_token,
            tokens.subject,
            grant,
            cancelled,
            transport,
            adapter,
            Some(resume),
            Some(attempt_sender),
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
    tauri::async_runtime::spawn_blocking(move || attempt_receiver.recv())
        .await
        .map_err(|_| "This reply could not be resumed. Try again.".to_string())?
        .map_err(|_| "This reply could not be resumed. Try again.".to_string())??;
    Ok(SubmitResult {
        run_id: result_id,
        attachments,
    })
}

#[tauri::command]
pub fn chat_file_metadata(path: PathBuf) -> Result<ChatAttachment, String> {
    let file = std::fs::File::open(&path).map_err(|_| attachment_error())?;
    let metadata = file.metadata().map_err(|_| attachment_error())?;
    if !metadata.is_file() {
        return Err(attachment_error());
    }
    let display_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(attachment_error)?
        .to_owned();
    Ok(ChatAttachment {
        display_name,
        byte_length: metadata.len(),
        media_type: None,
    })
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

fn attachment_error() -> String {
    "One or more selected files could not be added. Check the files and try again.".into()
}

struct OpenSelectedFile {
    file: std::fs::File,
    display_name: String,
    byte_length: u64,
}

fn open_selected_files(files: Vec<SelectedFile>) -> Result<Vec<OpenSelectedFile>, String> {
    files
        .into_iter()
        .map(|selected| {
            // Validate the handle that will actually be read. Checking the path before
            // opening leaves a window where it can be replaced with a different object.
            let file = std::fs::File::open(&selected.path).map_err(|_| attachment_error())?;
            let metadata = file.metadata().map_err(|_| attachment_error())?;
            if !metadata.is_file() {
                return Err(attachment_error());
            }
            let display_name = selected
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(attachment_error)?
                .to_owned();
            Ok(OpenSelectedFile {
                file,
                display_name,
                byte_length: metadata.len(),
            })
        })
        .collect()
}

fn prepare_new_run(
    storage: &SharedStorage,
    run_id: &str,
    subject: Option<&str>,
    files: Vec<SelectedFile>,
) -> Result<(u64, ChatProjector), String> {
    // Open and validate every selection before creating a run, so ordinary
    // selection failures cannot leave a rejected submission in the journal.
    let files = open_selected_files(files)?;
    prepare_opened_run(storage, run_id, subject, files)
}

fn prepare_opened_run(
    storage: &SharedStorage,
    run_id: &str,
    subject: Option<&str>,
    files: Vec<OpenSelectedFile>,
) -> Result<(u64, ChatProjector), String> {
    let mut storage = storage.lock().map_err(|_| attachment_error())?;
    let ChatStorage { journal, cas } = &mut *storage;
    let mut projector = ChatProjector::new();
    let mut seq = 1;
    let started = event_envelope(run_id, seq, "run.started", json!({}), subject);
    projector.apply(&started).map_err(|_| attachment_error())?;
    journal
        .append(0, &started)
        .map_err(|_| attachment_error())?;

    for mut selected in files {
        let next_seq = seq + 1;
        let envelope_subject = subject.map(str::to_owned);
        let attachment = match ingest_attachment(
            cas,
            journal,
            seq,
            &mut selected.file,
            AttachmentMetadata {
                display_name: &selected.display_name,
                byte_length: selected.byte_length,
                media_type: None,
            },
            |attachment| {
                let mut envelope = event_envelope(
                    run_id,
                    next_seq,
                    "chat.attachment.ingested",
                    json!({}),
                    envelope_subject.as_deref(),
                );
                envelope.payload = EventPayload::Attachment { attachment };
                envelope
            },
        ) {
            Ok(attachment) => attachment,
            Err(_) => {
                record_preparation_failure(journal, &mut projector, run_id, seq, subject);
                return Err(attachment_error());
            }
        };
        let attachment_event = match journal
            .events(run_id)
            .ok()
            .and_then(|events| events.last().cloned())
        {
            Some(event) => event,
            None => {
                record_preparation_failure(journal, &mut projector, run_id, seq, subject);
                return Err(attachment_error());
            }
        };
        if projector.apply(&attachment_event).is_err() {
            record_preparation_failure(journal, &mut projector, run_id, seq, subject);
            return Err(attachment_error());
        }
        seq = next_seq;
        if cas.verify(attachment.sha256()).is_err() {
            record_preparation_failure(journal, &mut projector, run_id, seq, subject);
            return Err(attachment_error());
        }
    }
    Ok((seq, projector))
}

fn record_preparation_failure(
    journal: &mut RunJournal,
    projector: &mut ChatProjector,
    run_id: &str,
    seq: u64,
    subject: Option<&str>,
) {
    let failed = event_envelope(
        run_id,
        seq + 1,
        "run.failed",
        json!({"reason": "attachment"}),
        subject,
    );
    if projector.apply(&failed).is_ok() {
        let _ = journal.append(seq, &failed);
    }
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

fn coordinate<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    journal: SharedStorage,
    runtime: Arc<Mutex<Option<PiRuntime>>>,
    run_id: String,
    prompt: String,
    access_token: String,
    subject: Option<String>,
    grant: ChatGrant,
    cancelled: Arc<AtomicBool>,
    active_transport: Arc<Mutex<Option<Arc<PiRpcTransport>>>>,
    active_adapter: Arc<Mutex<Option<Arc<PiRunAdapter>>>>,
    resume: Option<ResumeContext>,
    resume_result: Option<std::sync::mpsc::Sender<Result<(), String>>>,
    prepared: Option<(u64, ChatProjector)>,
) {
    let mut resume_attempt = ResumeAttempt::new(resume_result);
    let (mut seq, mut projector) = prepared.unwrap_or_else(|| {
        (
            resume.as_ref().map_or(0, |resume| {
                resume.events.last().map_or(0, |event| event.run_seq)
            }),
            ChatProjector::new(),
        )
    });
    if let Some(resume) = &resume {
        for event in &resume.events {
            if projector.apply(event).is_err() {
                return;
            }
        }
    } else if seq == 0
        && append_emit(
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
        if resume.is_none() {
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
        }
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
                fail_start(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    "The agent runtime is not installed.",
                    subject.as_deref(),
                    resume.is_some(),
                );
                return;
            }
        };
        #[cfg(test)]
        let executable = std::env::var_os("MUNIMENT_PI_TEST_EXECUTABLE")
            .map(std::path::PathBuf::from)
            .map(Ok)
            .unwrap_or_else(|| resolve_current(std::path::Path::new(&root)));
        #[cfg(not(test))]
        let executable = resolve_current(std::path::Path::new(&root));
        let executable = match executable {
            Ok(path) => path,
            Err(_) => {
                fail_start(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    "The agent runtime is unavailable.",
                    subject.as_deref(),
                    resume.is_some(),
                );
                return;
            }
        };
        let session_root = match app.path().app_data_dir() {
            Ok(path) => path.join("pi-sessions"),
            Err(_) => {
                fail_start(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    "The agent runtime is unavailable.",
                    subject.as_deref(),
                    resume.is_some(),
                );
                return;
            }
        };
        let mut config = match pi_sidecar_config(
            executable.to_string_lossy(),
            &session_root,
            resume.as_ref().map(|resume| &resume.locator),
        ) {
            Ok(config) => config,
            Err(_) => {
                fail_start(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    "The agent runtime is unavailable.",
                    subject.as_deref(),
                    resume.is_some(),
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
                    fail_start(
                        &app,
                        &journal,
                        &mut projector,
                        &run_id,
                        &mut seq,
                        "The agent runtime could not start.",
                        subject.as_deref(),
                        resume.is_some(),
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
            if resume.is_none() {
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
            }
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let Some(transport) = runtime.wiring.transport() else {
        fail_start(
            &app,
            &journal,
            &mut projector,
            &run_id,
            &mut seq,
            "The agent runtime did not become ready.",
            subject.as_deref(),
            resume.is_some(),
        );
        return;
    };
    *active_transport
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::clone(&transport));
    if cancelled.load(Ordering::SeqCst) {
        if resume.is_none() {
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
        }
        return;
    }
    let (adapter, _) = match PiRunAdapter::start(run_id.clone(), &transport, &prompt, RPC_TIMEOUT) {
        Ok(value) => value,
        Err(_) => {
            fail_start(
                &app,
                &journal,
                &mut projector,
                &run_id,
                &mut seq,
                "The reply could not be started.",
                subject.as_deref(),
                resume.is_some(),
            );
            return;
        }
    };
    let adapter = Arc::new(adapter);
    *active_adapter
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::clone(&adapter));
    if resume.is_some() {
        if append_emit(
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
            if adapter
                .cancel_and_drain(&transport, Duration::from_secs(2))
                .is_err()
            {
                let _ = runtime.supervisor.shutdown();
            }
            return;
        }
        // Prompt acknowledgement only proves that Pi accepted work. Report a
        // successful resume after the transition is durable so callers never
        // observe an active continuation that the journal still calls
        // interrupted.
        resume_attempt.accepted();
    }
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
    let (locator, buffered_events) = if resume.is_some() {
        (None, Vec::new())
    } else {
        match adapter.await_session_binding(&transport, &session_root, RPC_TIMEOUT) {
            Ok((locator, events)) => (Some(locator), events),
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
        }
    };
    if let Some(locator) = locator {
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
    }
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
                if let Some((kind, payload)) = tool_journal_entry(&event, &mut open_effects) {
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

fn append_terminal<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    journal: &SharedStorage,
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

fn fail_with_open_effects<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    journal: &SharedStorage,
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

fn append_emit<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    journal: &SharedStorage,
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
        let mut storage = journal.lock().map_err(|_| ())?;
        storage
            .journal
            .append(*seq - 1, &envelope)
            .map_err(|_| ())?;
    }
    *projector = next_projector;
    app.emit("chat-event", chat_event(run_id, projection))
        .map_err(|_| ())
}

fn chat_event(run_id: &str, projection: ChatProjection) -> ChatEvent {
    let attachments = chat_attachments(&projection.attachments);
    ChatEvent {
        run_id: run_id.into(),
        phase: projection_phase(&projection.status).into(),
        text: projection.text,
        receipt: projection.receipt,
        tool_activity: chat_tool_activity(&projection.tool_activity),
        attachments,
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

fn fail<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    journal: &SharedStorage,
    projector: &mut ChatProjector,
    run_id: &str,
    seq: &mut u64,
    reason: &str,
    subject: Option<&str>,
) {
    let _ = append_emit(
        app,
        journal,
        projector,
        run_id,
        seq,
        "run.failed",
        json!({"reason": reason}),
        subject,
    );
}

fn fail_start<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    journal: &SharedStorage,
    projector: &mut ChatProjector,
    run_id: &str,
    seq: &mut u64,
    reason: &str,
    subject: Option<&str>,
    resuming: bool,
) {
    if !resuming {
        fail(app, journal, projector, run_id, seq, reason, subject);
    }
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

    static PI_ENV_LOCK: Mutex<()> = Mutex::new(());

    fn accept_receipt_request(listener: std::net::TcpListener) -> std::net::TcpStream {
        listener.set_nonblocking(true).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            match listener.accept() {
                Ok((stream, _)) => return stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "coordinator did not request its receipt"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("receipt listener failed: {error}"),
            }
        }
    }

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
    fn new_run_ingests_multiple_files_into_one_sequence_and_projector() {
        keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
        let directory =
            std::env::temp_dir().join(format!("muniment-attachments-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let first = directory.join("first.txt");
        let second = directory.join("second.bin");
        std::fs::write(&first, b"first attachment").unwrap();
        std::fs::write(&second, b"second attachment").unwrap();
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(directory.join("runs.sqlite3")).unwrap(),
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let run_id = Uuid::now_v7().to_string();

        let (seq, mut projector) = prepare_new_run(
            &storage,
            &run_id,
            Some("owner"),
            vec![SelectedFile { path: first }, SelectedFile { path: second }],
        )
        .unwrap();
        assert_eq!(seq, 3);
        let mut storage = storage.lock().unwrap();
        let events = storage.journal.events(&run_id).unwrap();
        assert_eq!(
            events
                .iter()
                .map(|event| event.event_type.as_str())
                .collect::<Vec<_>>(),
            [
                "run.started",
                "chat.attachment.ingested",
                "chat.attachment.ingested",
            ]
        );
        for event in &events[1..] {
            let EventPayload::Attachment { attachment } = &event.payload else {
                panic!("attachment payload")
            };
            storage.cas.verify(attachment.sha256()).unwrap();
        }
        let projection = projector.projection().unwrap();
        assert_eq!(
            projection
                .attachments
                .iter()
                .map(|attachment| attachment.display_name.as_str())
                .collect::<Vec<_>>(),
            ["first.txt", "second.bin"]
        );
        let public = serde_json::to_string(&chat_attachments(&projection.attachments)).unwrap();
        assert!(!public.contains("sha256"));
        assert!(!public.contains(directory.to_string_lossy().as_ref()));
        let history = history_entries(&mut storage.journal, Some("owner"), &directory).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].attachments.len(), 2);
        let restored = serde_json::to_string(&history).unwrap();
        assert!(!restored.contains("sha256"));
        assert!(!restored.contains(directory.to_string_lossy().as_ref()));
        let serialized = serde_json::to_string(&events).unwrap();
        assert!(!serialized.contains(directory.to_string_lossy().as_ref()));
        let next = event_envelope(
            &run_id,
            4,
            "assistant.delta",
            json!({"text":"ready"}),
            Some("owner"),
        );
        projector.apply(&next).unwrap();
        storage.journal.append(3, &next).unwrap();
        assert_eq!(
            storage
                .journal
                .events(&run_id)
                .unwrap()
                .last()
                .unwrap()
                .run_seq,
            4
        );
        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn missing_attachment_returns_non_path_leaking_copy_before_pi_can_start() {
        let _environment = PI_ENV_LOCK.lock().unwrap();
        let directory = std::env::temp_dir().join(format!("muniment-missing-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let prompt_log = directory.join("prompt.txt");
        std::env::set_var("PI_RESUME_STUB_PROMPTS", &prompt_log);
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(directory.join("runs.sqlite3")).unwrap(),
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let missing = directory.join("private-name.txt");
        let error = match prepare_new_run(
            &storage,
            &Uuid::now_v7().to_string(),
            Some("owner"),
            vec![SelectedFile {
                path: missing.clone(),
            }],
        ) {
            Ok(_) => panic!("missing attachment must fail"),
            Err(error) => error,
        };
        assert_eq!(error, attachment_error());
        assert!(!error.contains(missing.to_string_lossy().as_ref()));
        assert!(!prompt_log.exists(), "Pi must not receive a prompt");
        std::env::remove_var("PI_RESUME_STUB_PROMPTS");
        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn non_regular_attachment_is_rejected_before_a_run_is_created() {
        let directory =
            std::env::temp_dir().join(format!("muniment-non-regular-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(directory.join("runs.sqlite3")).unwrap(),
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let run_id = Uuid::now_v7().to_string();

        assert!(prepare_new_run(
            &storage,
            &run_id,
            Some("owner"),
            vec![SelectedFile {
                path: directory.clone(),
            }],
        )
        .is_err());
        assert!(storage
            .lock()
            .unwrap()
            .journal
            .events(&run_id)
            .unwrap()
            .is_empty());

        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn changed_second_attachment_fails_the_run_without_prompting_pi() {
        let _environment = PI_ENV_LOCK.lock().unwrap();
        let directory = std::env::temp_dir().join(format!("muniment-changed-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let first = directory.join("first.txt");
        let second = directory.join("private-second.txt");
        let prompt_log = directory.join("prompt.txt");
        std::fs::write(&first, b"first attachment").unwrap();
        std::fs::write(&second, b"second attachment").unwrap();
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(directory.join("runs.sqlite3")).unwrap(),
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let run_id = Uuid::now_v7().to_string();
        let opened = open_selected_files(vec![
            SelectedFile { path: first },
            SelectedFile {
                path: second.clone(),
            },
        ])
        .unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&second)
            .unwrap()
            .set_len(1)
            .unwrap();
        std::env::set_var("PI_RESUME_STUB_PROMPTS", &prompt_log);

        let error = match prepare_opened_run(&storage, &run_id, Some("owner"), opened) {
            Ok(_) => panic!("changed attachment length must fail"),
            Err(error) => error,
        };
        assert_eq!(error, attachment_error());
        assert!(!error.contains(second.to_string_lossy().as_ref()));
        assert!(!prompt_log.exists(), "Pi must receive zero prompts");
        let events = storage.lock().unwrap().journal.events(&run_id).unwrap();
        assert_eq!(
            events
                .iter()
                .map(|event| event.event_type.as_str())
                .collect::<Vec<_>>(),
            ["run.started", "chat.attachment.ingested", "run.failed"]
        );
        assert!(reduce(&events).unwrap().is_terminal());

        std::env::remove_var("PI_RESUME_STUB_PROMPTS");
        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn prepared_attachments_reach_pi_before_coordinator_events_continue() {
        let _environment = PI_ENV_LOCK.lock().unwrap();
        let app = tauri::test::mock_app();
        let directory =
            std::env::temp_dir().join(format!("muniment-coordinate-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let first = directory.join("first.txt");
        let second = directory.join("second.txt");
        let prompt_log = directory.join("prompt.txt");
        std::fs::write(&first, b"first attachment").unwrap();
        std::fs::write(&second, b"second attachment").unwrap();
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(directory.join("runs.sqlite3")).unwrap(),
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let run_id = Uuid::now_v7().to_string();
        let prepared = prepare_new_run(
            &storage,
            &run_id,
            Some("owner"),
            vec![SelectedFile { path: first }, SelectedFile { path: second }],
        )
        .unwrap();

        let before_pi = storage.lock().unwrap().journal.events(&run_id).unwrap();
        assert_eq!(prepared.0, 3);
        assert_eq!(
            before_pi
                .iter()
                .map(|event| event.event_type.as_str())
                .collect::<Vec<_>>(),
            [
                "run.started",
                "chat.attachment.ingested",
                "chat.attachment.ingested",
            ]
        );
        assert!(!prompt_log.exists());

        let executable_name = if cfg!(windows) {
            "sidecar-test-stub.exe"
        } else {
            "sidecar-test-stub"
        };
        let test_executable = std::env::current_exe().unwrap();
        let stub = test_executable
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("examples")
            .join(executable_name);
        assert!(stub.is_file(), "sidecar test stub was not built");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let receipt_url = format!("http://{}/receipt", listener.local_addr().unwrap());
        let receipt_server = std::thread::spawn(move || {
            use std::io::{Read, Write};
            let mut stream = accept_receipt_request(listener);
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = [0; 4096];
            let _ = stream.read(&mut request).unwrap();
            let body = r#"{"route":"attachment-stub","model":"test"}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        std::env::set_var("MUNIMENT_PI_ROOT", &directory);
        std::env::set_var("MUNIMENT_PI_TEST_EXECUTABLE", &stub);
        std::env::set_var("PI_RESUME_STUB_PROMPTS", &prompt_log);

        coordinate(
            app.handle().clone(),
            Arc::clone(&storage),
            Arc::new(Mutex::new(None)),
            run_id.clone(),
            "Review both files".into(),
            "token".into(),
            Some("owner".into()),
            ChatGrant {
                gateway_url: "https://gateway.invalid".into(),
                virtual_key: "virtual-key".into(),
                model: None,
                receipt_url,
            },
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(None)),
            None,
            None,
            Some(prepared),
        );
        receipt_server.join().unwrap();
        for key in [
            "MUNIMENT_PI_ROOT",
            "MUNIMENT_PI_TEST_EXECUTABLE",
            "PI_RESUME_STUB_PROMPTS",
        ] {
            std::env::remove_var(key);
        }

        assert_eq!(
            std::fs::read_to_string(&prompt_log).unwrap(),
            "Review both files\n"
        );
        let events = storage.lock().unwrap().journal.events(&run_id).unwrap();
        assert_eq!(
            events.iter().map(|event| event.run_seq).collect::<Vec<_>>(),
            (1..=events.len() as u64).collect::<Vec<_>>()
        );
        assert_eq!(events[3].run_seq, 4);
        assert_eq!(events[3].event_type, "runtime.pi_session.bound");
        assert_eq!(events[4].event_type, "model.prompt.accepted");
        assert_eq!(events[5].event_type, "model.stream.delta");
        assert_eq!(events.last().unwrap().event_type, "run.completed");
        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
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
    fn resume_attempt_reports_acceptance_and_every_early_return() {
        let (sender, receiver) = std::sync::mpsc::channel();
        drop(ResumeAttempt::new(Some(sender)));
        assert_eq!(
            receiver.recv().unwrap().unwrap_err(),
            "This reply could not be resumed. Try again."
        );

        let (sender, receiver) = std::sync::mpsc::channel();
        let mut attempt = ResumeAttempt::new(Some(sender));
        attempt.accepted();
        drop(attempt);
        assert_eq!(receiver.recv().unwrap(), Ok(()));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn resume_validation_fails_closed_for_every_unsafe_projection() {
        let directory = std::env::temp_dir().join(format!("muniment-resume-{}", Uuid::now_v7()));
        let sessions = directory.join("pi-sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(sessions.join("session.jsonl"), "{}\n").unwrap();
        let run_id = Uuid::now_v7().to_string();
        let events = |tail: Vec<(&str, Value)>| {
            let mut values = vec![event_envelope(
                &run_id,
                1,
                "run.started",
                json!({}),
                Some("owner"),
            )];
            values.extend(
                tail.into_iter()
                    .enumerate()
                    .map(|(index, (kind, payload))| {
                        event_envelope(&run_id, index as u64 + 2, kind, payload, Some("owner"))
                    }),
            );
            values
        };
        let eligible = events(vec![
            (
                "runtime.pi_session.bound",
                json!({"run_id":run_id, "locator":"session.jsonl"}),
            ),
            ("run.needs_attention", json!({"reason":"interrupted"})),
        ]);
        assert!(resumable_context(&eligible, Some("owner"), &sessions).is_ok());
        assert!(resumable_context(&eligible, Some("another-subject"), &sessions).is_err());

        for unsafe_events in [
            events(vec![(
                "run.needs_attention",
                json!({"reason":"interrupted"}),
            )]),
            events(vec![
                (
                    "runtime.pi_session.bound",
                    json!({"run_id":run_id, "locator":"missing.jsonl"}),
                ),
                ("run.needs_attention", json!({"reason":"interrupted"})),
            ]),
            events(vec![("run.completed", json!({}))]),
            events(vec![
                (
                    "runtime.pi_session.bound",
                    json!({"run_id":run_id, "locator":"session.jsonl"}),
                ),
                (
                    "permission.requested",
                    json!({"gate_id":"gate", "kind":"confirm", "title":"Allow?", "message":"Proceed?"}),
                ),
                ("run.needs_attention", json!({"reason":"interrupted"})),
            ]),
            events(vec![
                (
                    "runtime.pi_session.bound",
                    json!({"run_id":run_id, "locator":"session.jsonl"}),
                ),
                (
                    "tool.effect.started",
                    json!({"effect_id":"effect", "display_name":"Command"}),
                ),
                ("run.needs_attention", json!({"reason":"interrupted"})),
            ]),
            Vec::new(),
        ] {
            assert!(resumable_context(&unsafe_events, Some("owner"), &sessions).is_err());
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn resume_runtime_failure_leaves_the_existing_journal_event_for_event_unchanged() {
        let _environment = PI_ENV_LOCK.lock().unwrap();
        let app = tauri::test::mock_app();
        let directory = std::env::temp_dir().join(format!("muniment-resume-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("runs.sqlite3");
        let run_id = Uuid::now_v7().to_string();
        let mut journal = RunJournal::open(&path).unwrap();
        append_test_event(
            &mut journal,
            &run_id,
            1,
            "run.started",
            json!({}),
            Some("owner"),
        );
        append_test_event(
            &mut journal,
            &run_id,
            2,
            "runtime.pi_session.bound",
            json!({"run_id":run_id, "locator":"session.jsonl"}),
            Some("owner"),
        );
        append_test_event(
            &mut journal,
            &run_id,
            3,
            "run.needs_attention",
            json!({"reason":"interrupted"}),
            Some("owner"),
        );
        let before = journal.events(&run_id).unwrap();
        let sessions = directory.join("pi-sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(sessions.join("session.jsonl"), "{}\n").unwrap();
        let (locator, _) = validate_pi_session(&sessions, "session.jsonl").unwrap();
        let shared = Arc::new(Mutex::new(ChatStorage {
            journal,
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let previous_root = std::env::var_os("MUNIMENT_PI_ROOT");
        std::env::remove_var("MUNIMENT_PI_ROOT");
        let (sender, receiver) = std::sync::mpsc::channel();
        coordinate(
            app.handle().clone(),
            Arc::clone(&shared),
            Arc::new(Mutex::new(None)),
            run_id.clone(),
            RESUME_PROMPT.into(),
            "token".into(),
            Some("owner".into()),
            ChatGrant {
                gateway_url: "https://gateway.invalid".into(),
                virtual_key: "virtual-key".into(),
                model: None,
                receipt_url: "https://receipt.invalid".into(),
            },
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(None)),
            Some(ResumeContext {
                events: before.clone(),
                locator,
            }),
            Some(sender),
            None,
        );
        if let Some(root) = previous_root {
            std::env::set_var("MUNIMENT_PI_ROOT", root);
        }
        assert!(receiver.recv().unwrap().is_err());
        assert_eq!(
            shared.lock().unwrap().journal.events(&run_id).unwrap(),
            before
        );
        drop(shared);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn resume_reopens_the_stub_session_and_completes_the_same_contiguous_run() {
        let _environment = PI_ENV_LOCK.lock().unwrap();
        let app = tauri::test::mock_app();
        let directory = std::env::temp_dir().join(format!("muniment-resume-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let args_log = directory.join("args.txt");
        let prompt_log = directory.join("prompt.txt");
        let session_name = format!("{}.jsonl", Uuid::now_v7());
        let sessions = app.path().app_data_dir().unwrap().join("pi-sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(sessions.join(&session_name), "persisted Pi data\n").unwrap();

        let executable_name = if cfg!(windows) {
            "sidecar-test-stub.exe"
        } else {
            "sidecar-test-stub"
        };
        let test_executable = std::env::current_exe().unwrap();
        let target_dir = test_executable.parent().unwrap().parent().unwrap();
        let stub = target_dir.join("examples").join(executable_name);
        assert!(stub.is_file(), "sidecar test stub was not built");

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let receipt_url = format!("http://{}/receipt", listener.local_addr().unwrap());
        let receipt_server = std::thread::spawn(move || {
            use std::io::{Read, Write};
            let mut stream = accept_receipt_request(listener);
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = [0; 4096];
            let _ = stream.read(&mut request).unwrap();
            let body = r#"{"route":"resume-stub","model":"test"}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        let run_id = Uuid::now_v7().to_string();
        let mut journal = RunJournal::open(directory.join("runs.sqlite3")).unwrap();
        append_test_event(
            &mut journal,
            &run_id,
            1,
            "run.started",
            json!({}),
            Some("owner"),
        );
        append_test_event(
            &mut journal,
            &run_id,
            2,
            "runtime.pi_session.bound",
            json!({"run_id":run_id, "locator":session_name}),
            Some("owner"),
        );
        append_test_event(
            &mut journal,
            &run_id,
            3,
            "run.needs_attention",
            json!({"reason":"interrupted"}),
            Some("owner"),
        );
        let existing = journal.events(&run_id).unwrap();
        let (locator, _) = validate_pi_session(&sessions, &session_name).unwrap();
        let shared = Arc::new(Mutex::new(ChatStorage {
            journal,
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));

        std::env::set_var("MUNIMENT_PI_ROOT", &directory);
        std::env::set_var("MUNIMENT_PI_TEST_EXECUTABLE", &stub);
        std::env::set_var("PI_RESUME_STUB_ARGS", &args_log);
        std::env::set_var("PI_RESUME_STUB_PROMPTS", &prompt_log);
        let (sender, receiver) = std::sync::mpsc::channel();
        coordinate(
            app.handle().clone(),
            Arc::clone(&shared),
            Arc::new(Mutex::new(None)),
            run_id.clone(),
            RESUME_PROMPT.into(),
            "token".into(),
            Some("owner".into()),
            ChatGrant {
                gateway_url: "https://gateway.invalid".into(),
                virtual_key: "virtual-key".into(),
                model: None,
                receipt_url,
            },
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(None)),
            Some(ResumeContext {
                events: existing,
                locator,
            }),
            Some(sender),
            None,
        );
        assert_eq!(receiver.recv().unwrap(), Ok(()));
        receipt_server.join().unwrap();
        for key in [
            "MUNIMENT_PI_ROOT",
            "MUNIMENT_PI_TEST_EXECUTABLE",
            "PI_RESUME_STUB_ARGS",
            "PI_RESUME_STUB_PROMPTS",
        ] {
            std::env::remove_var(key);
        }

        let events = shared.lock().unwrap().journal.events(&run_id).unwrap();
        assert_eq!(
            events.iter().map(|event| event.run_seq).collect::<Vec<_>>(),
            (1..=events.len() as u64).collect::<Vec<_>>()
        );
        assert_eq!(events.last().unwrap().event_type, "run.completed");
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event_type == "run.started")
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event_type == "runtime.pi_session.bound")
                .count(),
            1
        );
        let args = std::fs::read_to_string(args_log).unwrap();
        assert!(args.lines().any(|argument| argument == "--session"));
        assert!(args.contains(&session_name));
        let sent_prompt = std::fs::read_to_string(prompt_log).unwrap();
        assert_eq!(sent_prompt, format!("{RESUME_PROMPT}\n"));
        assert!(!sent_prompt.contains("ORIGINAL PROTECTED PROMPT"));

        std::fs::remove_file(sessions.join(session_name)).unwrap();
        drop(shared);
        std::fs::remove_dir_all(directory).unwrap();
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

        let entries = history_entries(&mut journal, None, std::path::Path::new(".")).unwrap();
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
        let entries = history_entries(&mut journal, None, std::path::Path::new(".")).unwrap();
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

        let sub_b =
            history_entries(&mut journal, Some("sub-b"), std::path::Path::new(".")).unwrap();
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

        let sub_a =
            history_entries(&mut journal, Some("sub-a"), std::path::Path::new(".")).unwrap();
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
