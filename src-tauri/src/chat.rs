use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{SecondsFormat, Utc};
#[cfg(target_os = "linux")]
use muniment_core::attach::linux::{
    ThreadListPage, ThreadListRequest, ThreadListService, ThreadOpenPage, ThreadOpenRequest,
};
#[cfg(target_os = "linux")]
use muniment_core::attach::ProtocolError;
use muniment_core::attachment::{ingest_attachment, prepare_pi_images, AttachmentMetadata};
use muniment_core::auth::TokenSet;
use muniment_core::cas::LocalCas;
use muniment_core::journal::reducer::{
    project_chat, reduce, ChatProjection, ChatProjector, PermissionGate, PermissionRequest,
    ProjectedAttachment, RunStatus,
};
use muniment_core::journal::thread_summaries::ThreadSummary;
use muniment_core::journal::{EventEnvelope, EventPayload, Provenance, RunEventType, RunJournal};
use muniment_core::sidecar::pi_chat::{
    cancel_command, ExtensionUiAnswer, ExtensionUiDialog, ExtensionUiRequest, ExtensionUiResponse,
    PiChatEvent, PiImageContent, PiRunAdapter, PromptCommand, Receipt,
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
use crate::session_thread::{OfferedThread, SessionThread};

const RPC_TIMEOUT: Duration = Duration::from_secs(30);
const QUEUE_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_THREAD_SUMMARY_CORE_PAGES: usize = 100;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChatGrant {
    pub(crate) workspace: String,
    pub(crate) gateway_url: String,
    pub(crate) virtual_key: String,
    #[serde(default)]
    pub(crate) model: Option<String>,
    pub(crate) receipt_url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitResult {
    pub(crate) run_id: String,
    attachments: Vec<ChatAttachment>,
    #[serde(skip)]
    pub(crate) committed_seq: u64,
    #[serde(skip)]
    pub(crate) accepted_at: String,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatThreadSummary {
    thread_id: String,
    title: String,
    updated_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatThreadSummaryPage {
    summaries: Vec<ChatThreadSummary>,
    next_cursor: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatThreadOpenPage {
    entries: Vec<HistoryEntry>,
    next_cursor: Option<String>,
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

pub(crate) fn chat_attachments(attachments: &[ProjectedAttachment]) -> Vec<ChatAttachment> {
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

pub(crate) struct ActiveRun {
    pub(crate) id: String,
    pub(crate) cancelled: Arc<AtomicBool>,
    pub(crate) transport: Arc<Mutex<Option<Arc<PiRpcTransport>>>>,
    pub(crate) adapter: Arc<Mutex<Option<Arc<PiRunAdapter>>>>,
    permission_answers: Arc<Mutex<VecDeque<PendingPermissionAnswer>>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ChatPermissionAnswer {
    Select(String),
    Confirm(bool),
    Input(String),
    Editor(String),
    Cancelled,
}

impl ChatPermissionAnswer {
    fn pi_answer(&self) -> ExtensionUiAnswer {
        match self {
            Self::Select(value) => ExtensionUiAnswer::Selection(value.clone()),
            Self::Confirm(value) => ExtensionUiAnswer::Confirmation(*value),
            Self::Input(value) => ExtensionUiAnswer::Input(value.clone()),
            Self::Editor(value) => ExtensionUiAnswer::Editor(value.clone()),
            Self::Cancelled => ExtensionUiAnswer::Cancelled,
        }
    }

    fn decision(&self) -> Value {
        serde_json::to_value(self).expect("permission answers serialize")
    }
}

#[derive(Clone, Debug)]
struct PendingPermissionAnswer {
    gate_id: String,
    answer: ChatPermissionAnswer,
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
    session_thread: SessionThread,
}

pub(crate) struct RunStartRequest {
    pub(crate) prompt: String,
    pub(crate) files: Vec<SelectedFile>,
    pub(crate) workspace: Option<String>,
    pub(crate) provenance: Option<Provenance>,
}

pub(crate) struct RunStartLaunch {
    pub(crate) run_id: String,
    pub(crate) prompt: String,
    pub(crate) tokens: TokenSet,
    pub(crate) grant: ChatGrant,
    pub(crate) cancelled: Arc<AtomicBool>,
    pub(crate) transport: Arc<Mutex<Option<Arc<PiRpcTransport>>>>,
    pub(crate) adapter: Arc<Mutex<Option<Arc<PiRunAdapter>>>>,
    permission_answers: Arc<Mutex<VecDeque<PendingPermissionAnswer>>>,
    pub(crate) prepared: (u64, ChatProjector),
}

pub(crate) trait RunStartBoundaries {
    #[cfg(target_os = "linux")]
    fn list_threads(
        &self,
        workspace: &str,
        request: ThreadListRequest,
    ) -> Result<ThreadListPage, ProtocolError>;
    #[cfg(target_os = "linux")]
    fn open_thread(
        &self,
        workspace: &str,
        request: ThreadOpenRequest,
    ) -> Result<ThreadOpenPage, ProtocolError>;
    fn active_run_exists(&self) -> bool;
    fn fresh_tokens(&self) -> Result<TokenSet, RunStartError>;
    fn configure_run(
        &self,
        run_id: &str,
        prompt: &str,
        tokens: &TokenSet,
        requested_workspace: Option<&str>,
    ) -> Result<ChatGrant, RunStartError>;
    fn install_active_run(&self, run: ActiveRun) -> Result<(), RunStartError>;
    fn prepare_run(
        &self,
        run_id: &str,
        grant: &ChatGrant,
        tokens: &TokenSet,
        files: Vec<SelectedFile>,
        provenance: Option<Provenance>,
    ) -> Result<(u64, ChatProjector), RunStartError>;
    fn project_attachments(
        &self,
        projector: &ChatProjector,
    ) -> Result<Vec<ChatAttachment>, RunStartError>;
    fn fail_prepared_run(&self, launch: &RunStartLaunch) -> Result<(), RunStartError>;
    fn clear_active_run(&self, run_id: &str);
    fn launch(&self, launch: RunStartLaunch);
}

#[derive(Debug)]
pub(crate) enum RunStartError {
    Unauthorized(String),
    InvalidRequest(String),
    Persistence(String),
}

impl RunStartError {
    #[cfg(target_os = "linux")]
    pub(crate) fn protocol_error(&self) -> ProtocolError {
        match self {
            Self::Unauthorized(_) => ProtocolError::unauthorized(),
            Self::InvalidRequest(_) => ProtocolError::invalid_request(),
            Self::Persistence(_) => ProtocolError::persistence_failed(),
        }
    }

    fn into_message(self) -> String {
        match self {
            Self::Unauthorized(message)
            | Self::InvalidRequest(message)
            | Self::Persistence(message) => message,
        }
    }
}

/// Shared, channel-neutral acceptance path for starting a desktop-owned run.
/// Channel adapters supply boundaries but cannot bypass validation or ownership.
fn start_desktop_run(
    boundaries: &impl RunStartBoundaries,
    request: RunStartRequest,
) -> Result<SubmitResult, RunStartError> {
    let (result, launch) = prepare_desktop_run(boundaries, request)?;
    boundaries.launch(launch);
    Ok(result)
}

pub(crate) fn prepare_desktop_run(
    boundaries: &impl RunStartBoundaries,
    request: RunStartRequest,
) -> Result<(SubmitResult, RunStartLaunch), RunStartError> {
    let prompt = request.prompt.trim().to_owned();
    if prompt.is_empty() {
        return Err(RunStartError::InvalidRequest(
            "Enter a message before sending.".into(),
        ));
    }
    if boundaries.active_run_exists() {
        return Err(RunStartError::InvalidRequest(
            "A reply is already in progress.".into(),
        ));
    }

    let tokens = boundaries.fresh_tokens()?;
    let run_id = Uuid::now_v7().to_string();
    let grant =
        boundaries.configure_run(&run_id, &prompt, &tokens, request.workspace.as_deref())?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let transport = Arc::new(Mutex::new(None));
    let adapter = Arc::new(Mutex::new(None));
    let permission_answers = Arc::new(Mutex::new(VecDeque::new()));
    if let Err(error) = boundaries.install_active_run(ActiveRun {
        id: run_id.clone(),
        cancelled: Arc::clone(&cancelled),
        transport: Arc::clone(&transport),
        adapter: Arc::clone(&adapter),
        permission_answers: Arc::clone(&permission_answers),
    }) {
        boundaries.clear_active_run(&run_id);
        return Err(error);
    }
    let prepared =
        match boundaries.prepare_run(&run_id, &grant, &tokens, request.files, request.provenance) {
            Ok(prepared) => prepared,
            Err(error) => {
                boundaries.clear_active_run(&run_id);
                return Err(error);
            }
        };
    let attachments = match boundaries.project_attachments(&prepared.1) {
        Ok(attachments) => attachments,
        Err(error) => {
            boundaries.clear_active_run(&run_id);
            return Err(error);
        }
    };
    let result = SubmitResult {
        run_id: run_id.clone(),
        attachments,
        committed_seq: prepared.0,
        accepted_at: Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true),
    };
    let launch = RunStartLaunch {
        run_id,
        prompt,
        tokens,
        grant,
        cancelled,
        transport,
        adapter,
        permission_answers,
        prepared,
    };
    Ok((result, launch))
}

pub(crate) struct TauriRunStartBoundaries<R: tauri::Runtime> {
    pub(crate) app: tauri::AppHandle<R>,
    pub(crate) continue_session_thread: bool,
}

impl<R: tauri::Runtime> TauriRunStartBoundaries<R> {
    fn state(&self) -> tauri::State<'_, ChatState> {
        self.app.state::<ChatState>()
    }
}

impl<R: tauri::Runtime> RunStartBoundaries for TauriRunStartBoundaries<R> {
    #[cfg(target_os = "linux")]
    fn list_threads(
        &self,
        workspace: &str,
        request: ThreadListRequest,
    ) -> Result<ThreadListPage, ProtocolError> {
        let state = self.state();
        let mut storage = state
            .storage
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        ThreadListService::list_threads(&mut storage.journal, workspace, request)
    }

    #[cfg(target_os = "linux")]
    fn open_thread(
        &self,
        workspace: &str,
        request: ThreadOpenRequest,
    ) -> Result<ThreadOpenPage, ProtocolError> {
        let state = self.state();
        let mut storage = state
            .storage
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        ThreadListService::open_thread(&mut storage.journal, workspace, request)
    }

    fn active_run_exists(&self) -> bool {
        self.state()
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
    }

    fn fresh_tokens(&self) -> Result<TokenSet, RunStartError> {
        auth::fresh_tokens(&self.app.state::<auth::AuthState>(), &self.app)
            .map_err(RunStartError::Unauthorized)
    }

    fn configure_run(
        &self,
        run_id: &str,
        prompt: &str,
        tokens: &TokenSet,
        requested_workspace: Option<&str>,
    ) -> Result<ChatGrant, RunStartError> {
        let grant = fetch_grant(&tokens.access_token).map_err(map_fetch_grant_error)?;
        validate_grant(&grant).map_err(RunStartError::Persistence)?;
        if requested_workspace.is_some_and(|workspace| workspace != grant.workspace) {
            return Err(RunStartError::Unauthorized(
                "The capability is not authorized.".into(),
            ));
        }
        protect_prompt(run_id, prompt, tokens.subject.as_deref())
            .map_err(RunStartError::Persistence)?;
        Ok(grant)
    }

    fn install_active_run(&self, run: ActiveRun) -> Result<(), RunStartError> {
        install_active_run(&self.state().active, run).map_err(RunStartError::InvalidRequest)
    }

    fn prepare_run(
        &self,
        run_id: &str,
        grant: &ChatGrant,
        tokens: &TokenSet,
        files: Vec<SelectedFile>,
        provenance: Option<Provenance>,
    ) -> Result<(u64, ChatProjector), RunStartError> {
        let state = self.state();
        prepare_new_run_with_session_thread(
            &state.storage,
            SessionThreadStart {
                tracker: &state.session_thread,
                continue_existing: self.continue_session_thread,
            },
            run_id,
            &grant.workspace,
            tokens.subject.as_deref(),
            files,
            provenance,
        )
        .map_err(RunStartError::Persistence)
    }

    fn project_attachments(
        &self,
        projector: &ChatProjector,
    ) -> Result<Vec<ChatAttachment>, RunStartError> {
        projector
            .projection()
            .map(|projection| chat_attachments(&projection.attachments))
            .map_err(|_| RunStartError::Persistence(attachment_error()))
    }

    fn fail_prepared_run(&self, launch: &RunStartLaunch) -> Result<(), RunStartError> {
        let failed = event_envelope(
            &launch.run_id,
            launch.prepared.0 + 1,
            "run.failed",
            json!({"reason": "persistence"}),
            launch.tokens.subject.as_deref(),
        );
        self.state()
            .storage
            .lock()
            .map_err(|_| RunStartError::Persistence(attachment_error()))?
            .journal
            .append(launch.prepared.0, &failed)
            .map_err(|_| RunStartError::Persistence(attachment_error()))
    }

    fn clear_active_run(&self, run_id: &str) {
        clear_active_run(&self.state().active, run_id);
    }

    fn launch(&self, launch: RunStartLaunch) {
        let app = self.app.clone();
        let state = self.state();
        let storage = Arc::clone(&state.storage);
        let runtime = Arc::clone(&state.runtime);
        tauri::async_runtime::spawn_blocking(move || {
            coordinate(
                app.clone(),
                storage,
                runtime,
                launch.run_id.clone(),
                launch.prompt,
                launch.tokens.access_token,
                launch.tokens.subject,
                launch.grant,
                launch.cancelled,
                launch.transport,
                launch.adapter,
                launch.permission_answers,
                None,
                None,
                Some(launch.prepared),
            );
            if let Some(state) = app.try_state::<ChatState>() {
                clear_active_run(&state.active, &launch.run_id);
            }
        });
    }
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
            session_thread: SessionThread::default(),
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
    let Ok(event_types) = journal.run_event_types() else {
        return;
    };
    for run in event_types.chunk_by(|left, right| left.run_id == right.run_id) {
        if event_types_are_terminal(run) {
            continue;
        }
        let run_id = &run[0].run_id;
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

fn event_types_are_terminal(events: &[RunEventType]) -> bool {
    let mut terminal = false;
    for (index, event) in events.iter().enumerate() {
        if event.run_seq != index as u64 + 1 {
            return false;
        }
        match event.event_type.as_str() {
            "run.completed" | "run.cancelled" | "run.failed" | "run.needs_attention" => {
                terminal = true;
            }
            "run.started"
            | "run.resumed"
            | "chat.attachment.ingested"
            | "runtime.pi_session.bound"
            | "model.prompt.accepted"
            | "model.stream.delta"
            | "permission.requested"
            | "permission.resolved"
            | "tool.effect.started"
            | "tool.effect.completed"
            | "tool.effect.failed" => terminal = false,
            "user.prompt.submitted"
            | "route.selected"
            | "capability.used"
            | "receipt.finalized" => {}
            _ => return false,
        }
    }
    terminal
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

fn subject_owns_first_run(
    journal: &mut RunJournal,
    thread_id: &str,
    subject: Option<&str>,
) -> Result<bool, String> {
    let first_run = journal
        .thread_run_ids(thread_id, 1, None)
        .map_err(|_| "Conversation history is unavailable.".to_string())?
        .run_ids
        .into_iter()
        .next()
        .ok_or_else(|| "Conversation history is unavailable.".to_string())?;
    let first = journal
        .first_envelope(&first_run)
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    Ok(!matches!(
        first.provenance.actor_id.as_deref(),
        Some(owner) if Some(owner) != subject
    ))
}

fn newest_owned_workspace_thread(
    journal: &mut RunJournal,
    workspace: &str,
    subject: Option<&str>,
) -> Result<Option<String>, String> {
    let mut cursor = None;
    for _ in 0..MAX_THREAD_SUMMARY_CORE_PAGES {
        let page = journal
            .workspace_thread_summaries(workspace, 100, cursor.as_deref())
            .map_err(|_| "Conversation history is unavailable.".to_string())?;
        for summary in page.summaries {
            if subject_owns_first_run(journal, &summary.thread_id, subject)? {
                return Ok(Some(summary.thread_id));
            }
        }
        match page.next_cursor {
            Some(next_cursor) => cursor = Some(next_cursor),
            None => return Ok(None),
        }
    }
    Ok(None)
}

fn chat_thread_summaries_page(
    journal: &mut RunJournal,
    subject: Option<&str>,
    limit: usize,
    cursor: Option<&str>,
) -> Result<ChatThreadSummaryPage, String> {
    if !(1..=100).contains(&limit) {
        return Err("Conversation history is unavailable.".into());
    }
    let mut summaries = Vec::with_capacity(limit);
    let mut next_cursor = cursor.map(str::to_owned);
    for _ in 0..MAX_THREAD_SUMMARY_CORE_PAGES {
        let page = journal
            .thread_summaries(limit - summaries.len(), next_cursor.as_deref())
            .map_err(|_| "Conversation history is unavailable.".to_string())?;
        for summary in page.summaries {
            if subject_owns_first_run(journal, &summary.thread_id, subject)? {
                summaries.push(summary);
            }
        }
        next_cursor = page.next_cursor;
        if summaries.len() == limit || next_cursor.is_none() {
            break;
        }
    }
    Ok(ChatThreadSummaryPage {
        summaries: summaries
            .into_iter()
            .map(
                |ThreadSummary {
                     thread_id,
                     title,
                     updated_at,
                 }| ChatThreadSummary {
                    thread_id,
                    title,
                    updated_at,
                },
            )
            .collect(),
        next_cursor,
    })
}

fn project_history_entry(
    journal: &mut RunJournal,
    run_id: String,
    subject: Option<&str>,
    session_root: &std::path::Path,
) -> Result<HistoryEntry, String> {
    let events = journal
        .events(&run_id)
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    let projection =
        project_chat(&events).map_err(|_| "Conversation history is unavailable.".to_string())?;
    let resumable = resumable_context(&events, subject, session_root).is_ok();
    Ok(HistoryEntry {
        prompt: load_prompt(&run_id, subject)?,
        phase: projection_phase(&projection.status).into(),
        text: projection.text,
        receipt: projection.receipt,
        tool_activity: chat_tool_activity(&projection.tool_activity),
        attachments: chat_attachments(&projection.attachments),
        pending_permission: chat_pending_permission(projection.pending_permission),
        resumable,
        run_id,
    })
}

fn chat_thread_open_page(
    journal: &mut RunJournal,
    subject: Option<&str>,
    session_root: &std::path::Path,
    thread_id: &str,
    limit: usize,
    cursor: Option<&str>,
) -> Result<ChatThreadOpenPage, String> {
    if !subject_owns_first_run(journal, thread_id, subject)? {
        return Err("Conversation history is unavailable.".into());
    }
    let page = journal
        .thread_run_ids(thread_id, limit, cursor)
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    let entries = page
        .run_ids
        .into_iter()
        .map(|run_id| project_history_entry(journal, run_id, subject, session_root))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ChatThreadOpenPage {
        entries,
        next_cursor: page.next_cursor,
    })
}

fn select_session_thread(
    journal: &mut RunJournal,
    tracker: &SessionThread,
    subject: Option<&str>,
    thread_id: &str,
) -> Result<(), String> {
    if !subject_owns_first_run(journal, thread_id, subject)? {
        return Err("Conversation history is unavailable.".into());
    }
    tracker.select(thread_id.to_owned(), subject);
    Ok(())
}

#[tauri::command]
pub async fn chat_thread_summaries(
    app_handle: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
    limit: usize,
    cursor: Option<String>,
) -> Result<ChatThreadSummaryPage, String> {
    let tokens = auth::fresh_tokens(&auth_state, &app_handle)?;
    let mut storage = state
        .storage
        .lock()
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    chat_thread_summaries_page(
        &mut storage.journal,
        tokens.subject.as_deref(),
        limit,
        cursor.as_deref(),
    )
}

#[tauri::command]
pub async fn chat_select_thread(
    app_handle: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
    thread_id: String,
) -> Result<(), String> {
    let tokens = auth::fresh_tokens(&auth_state, &app_handle)?;
    let mut storage = state
        .storage
        .lock()
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    select_session_thread(
        &mut storage.journal,
        &state.session_thread,
        tokens.subject.as_deref(),
        &thread_id,
    )
}

#[tauri::command]
pub async fn chat_new_thread(
    app_handle: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
) -> Result<(), String> {
    auth::fresh_tokens(&auth_state, &app_handle)?;
    state.session_thread.fresh();
    Ok(())
}

#[tauri::command]
pub async fn chat_thread_open(
    app_handle: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
    thread_id: String,
    limit: usize,
    cursor: Option<String>,
) -> Result<ChatThreadOpenPage, String> {
    let tokens = auth::fresh_tokens(&auth_state, &app_handle)?;
    let session_root = state_session_root(&app_handle)?;
    let mut storage = state
        .storage
        .lock()
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    chat_thread_open_page(
        &mut storage.journal,
        tokens.subject.as_deref(),
        &session_root,
        &thread_id,
        limit,
        cursor.as_deref(),
    )
}

#[tauri::command]
pub async fn chat_history(
    app_handle: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
) -> Result<Vec<HistoryEntry>, String> {
    // History is conversation data and follows the same signed-in gate as send.
    let tokens = auth::fresh_tokens(&auth_state, &app_handle)?;
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
    // Preserve the command argument names while the channel-neutral boundary
    // resolves the same managed values from the owned app handle.
    let _ = (&auth_state, &state);
    tauri::async_runtime::spawn_blocking(move || {
        start_desktop_run(
            &TauriRunStartBoundaries {
                app,
                continue_session_thread: true,
            },
            RunStartRequest {
                prompt,
                files: files.unwrap_or_default(),
                workspace: None,
                provenance: None,
            },
        )
        .map_err(RunStartError::into_message)
    })
    .await
    .map_err(|_| "Chat configuration is temporarily unavailable.".to_string())?
}

#[tauri::command]
pub async fn chat_resume(
    app: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
    run_id: String,
) -> Result<SubmitResult, String> {
    let tokens = auth::fresh_tokens_async(&auth_state, &app).await?;
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
        let grant = fetch_grant(&access_token).map_err(FetchGrantError::into_message)?;
        validate_grant(&grant)?;
        Ok::<_, String>(grant)
    })
    .await
    .map_err(|_| "Chat configuration is temporarily unavailable.".to_string())??;
    let cancelled = Arc::new(AtomicBool::new(false));
    let transport = Arc::new(Mutex::new(None));
    let adapter = Arc::new(Mutex::new(None));
    let permission_answers = Arc::new(Mutex::new(VecDeque::new()));
    install_active_run(
        &state.active,
        ActiveRun {
            id: run_id.clone(),
            cancelled: Arc::clone(&cancelled),
            transport: Arc::clone(&transport),
            adapter: Arc::clone(&adapter),
            permission_answers: Arc::clone(&permission_answers),
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
            permission_answers,
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
        committed_seq: 0,
        accepted_at: String::new(),
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

fn clear_active_run(active: &Mutex<Option<ActiveRun>>, run_id: &str) {
    let mut active = active
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if active.as_ref().is_some_and(|current| current.id == run_id) {
        *active = None;
    }
}

pub(crate) fn attachment_error() -> String {
    "One or more selected files could not be added. Check the files and try again.".into()
}

fn prepared_pi_images(
    storage: &SharedStorage,
    run_id: &str,
) -> Result<Vec<PiImageContent>, String> {
    let mut storage = storage.lock().map_err(|_| attachment_error())?;
    let events = storage
        .journal
        .events(run_id)
        .map_err(|_| attachment_error())?;
    prepare_pi_images(&storage.cas, &events)
        .map_err(|_| attachment_error())
        .map(|images| {
            images
                .into_iter()
                .map(|image| PiImageContent::new(image.data, image.mime_type))
                .collect()
        })
}

fn prepared_pi_prompt<'a>(
    storage: &SharedStorage,
    run_id: &str,
    prompt: &'a str,
) -> Result<PromptCommand<'a>, String> {
    prepared_pi_images(storage, run_id).map(|images| PromptCommand::with_images(prompt, images))
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

#[derive(Clone, Copy)]
struct SessionThreadStart<'a> {
    tracker: &'a SessionThread,
    continue_existing: bool,
}

fn prepare_new_run_with_session_thread(
    storage: &SharedStorage,
    session_thread: SessionThreadStart<'_>,
    run_id: &str,
    workspace: &str,
    subject: Option<&str>,
    files: Vec<SelectedFile>,
    provenance: Option<Provenance>,
) -> Result<(u64, ChatProjector), String> {
    // Open and validate every selection before creating a run, so ordinary
    // selection failures cannot leave a rejected submission in the journal.
    let files = open_selected_files(files)?;
    prepare_opened_run(
        storage,
        session_thread,
        run_id,
        workspace,
        subject,
        files,
        provenance,
    )
}

fn prepare_opened_run(
    storage: &SharedStorage,
    session_thread: SessionThreadStart<'_>,
    run_id: &str,
    workspace: &str,
    subject: Option<&str>,
    files: Vec<OpenSelectedFile>,
    provenance: Option<Provenance>,
) -> Result<(u64, ChatProjector), String> {
    let mut storage = storage.lock().map_err(|_| attachment_error())?;
    let ChatStorage { journal, cas } = &mut *storage;
    let mut projector = ChatProjector::new();
    let mut seq = 1;
    let mut started = event_envelope(run_id, seq, "run.started", json!({}), subject);
    if let Some(provenance) = provenance {
        started.provenance = Provenance {
            actor_id: provenance.actor_id.or_else(|| subject.map(str::to_owned)),
            ..provenance
        };
    }
    projector.apply(&started).map_err(|_| attachment_error())?;
    if !workspace.is_empty() {
        let thread_id = if session_thread.continue_existing {
            let offered = match session_thread.tracker.offered(workspace, subject) {
                OfferedThread::AdoptNewest => {
                    newest_owned_workspace_thread(journal, workspace, subject)
                        .ok()
                        .flatten()
                }
                OfferedThread::Selected(thread_id) => Some(thread_id),
                OfferedThread::Fresh => None,
            };
            match offered {
                Some(thread_id) => journal
                    .append_new_run_in_thread(workspace, &thread_id, &started)
                    .map(|()| thread_id)
                    .or_else(|_| journal.append_new_run(workspace, &started)),
                None => journal.append_new_run(workspace, &started),
            }
        } else {
            journal.append_new_run(workspace, &started)
        }
        .map_err(|_| attachment_error())?;
        if session_thread.continue_existing {
            session_thread.tracker.record(thread_id, workspace, subject);
        }
    } else {
        journal
            .append(0, &started)
            .map_err(|_| attachment_error())?;
    }

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

#[cfg(test)]
fn prepare_new_run(
    storage: &SharedStorage,
    run_id: &str,
    workspace: &str,
    subject: Option<&str>,
    files: Vec<SelectedFile>,
    provenance: Option<Provenance>,
) -> Result<(u64, ChatProjector), String> {
    prepare_new_run_with_session_thread(
        storage,
        SessionThreadStart {
            tracker: &SessionThread::default(),
            continue_existing: false,
        },
        run_id,
        workspace,
        subject,
        files,
        provenance,
    )
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

#[derive(Debug)]
enum PreparedPromptError {
    Start,
    SessionRoot,
    Binding,
    Journal,
}

fn coordinate_prepared_prompt<R: tauri::Runtime, T>(
    app: &tauri::AppHandle<R>,
    journal: &SharedStorage,
    projector: &mut ChatProjector,
    run_id: &str,
    seq: &mut u64,
    subject: Option<&str>,
    submit: impl FnOnce() -> Result<(T, PiSessionLocator, Vec<PiChatEvent>), PreparedPromptError>,
) -> Result<(T, Vec<PiChatEvent>), PreparedPromptError> {
    let (handle, locator, buffered_events) = submit()?;
    append_emit(
        app,
        journal,
        projector,
        run_id,
        seq,
        "runtime.pi_session.bound",
        json!({"run_id": run_id, "locator": locator.as_str()}),
        subject,
    )
    .map_err(|_| PreparedPromptError::Journal)?;
    append_emit(
        app,
        journal,
        projector,
        run_id,
        seq,
        "model.prompt.accepted",
        json!({}),
        subject,
    )
    .map_err(|_| PreparedPromptError::Journal)?;
    Ok((handle, buffered_events))
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

#[tauri::command]
pub async fn chat_answer_permission(
    state: tauri::State<'_, ChatState>,
    run_id: String,
    gate_id: String,
    answer: ChatPermissionAnswer,
) -> Result<(), String> {
    queue_permission_answer(&state.active, run_id, gate_id, answer)
}

fn queue_permission_answer(
    active: &Mutex<Option<ActiveRun>>,
    run_id: String,
    gate_id: String,
    answer: ChatPermissionAnswer,
) -> Result<(), String> {
    let active = active
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let run = active
        .as_ref()
        .filter(|run| run.id == run_id)
        .ok_or_else(|| "That reply is no longer active.".to_string())?;
    run.permission_answers
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push_back(PendingPermissionAnswer { gate_id, answer });
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
    permission_answers: Arc<Mutex<VecDeque<PendingPermissionAnswer>>>,
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
    let prepared_prompt = if resume.is_some() {
        None
    } else {
        let prepared_prompt = match prepared_pi_prompt(&journal, &run_id, &prompt) {
            Ok(prepared_prompt) => prepared_prompt,
            Err(message) => {
                fail_start(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    &message,
                    subject.as_deref(),
                    false,
                );
                return;
            }
        };
        Some(prepared_prompt)
    };
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
    let (adapter, buffered_events) = if resume.is_some() {
        let (adapter, _) =
            match PiRunAdapter::start(run_id.clone(), &transport, &prompt, RPC_TIMEOUT) {
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
                        true,
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
        (adapter, Vec::new())
    } else {
        let result = coordinate_prepared_prompt(
            &app,
            &journal,
            &mut projector,
            &run_id,
            &mut seq,
            subject.as_deref(),
            || {
                let prepared_prompt = prepared_prompt.expect("new runs prepare a Pi prompt");
                let (adapter, _) = PiRunAdapter::start_with_images(
                    run_id.clone(),
                    &transport,
                    prepared_prompt.message,
                    prepared_prompt.images,
                    RPC_TIMEOUT,
                )
                .map_err(|_| PreparedPromptError::Start)?;
                let adapter = Arc::new(adapter);
                *active_adapter
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) =
                    Some(Arc::clone(&adapter));
                let session_root = app
                    .path()
                    .app_data_dir()
                    .map(|path| path.join("pi-sessions"))
                    .map_err(|_| PreparedPromptError::SessionRoot)?;
                let (locator, events) = adapter
                    .await_session_binding(&transport, &session_root, RPC_TIMEOUT)
                    .map_err(|_| PreparedPromptError::Binding)?;
                Ok((adapter, locator, events))
            },
        );
        match result {
            Ok(value) => value,
            Err(error @ (PreparedPromptError::Journal | PreparedPromptError::SessionRoot)) => {
                let adapter = active_adapter
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone();
                if adapter.is_some_and(|adapter| {
                    adapter
                        .cancel_and_drain(&transport, Duration::from_secs(2))
                        .is_err()
                }) {
                    let _ = runtime.supervisor.shutdown();
                }
                if matches!(error, PreparedPromptError::SessionRoot) {
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
            Err(PreparedPromptError::Binding) => {
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
            Err(PreparedPromptError::Start) => {
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
    let mut buffered_events = buffered_events.into_iter();
    let mut aborting = false;
    let mut open_effects = BTreeSet::new();
    let mut pending_permission = None;
    'coordinate: loop {
        if cancelled.swap(false, Ordering::SeqCst) {
            aborting = true;
            let _ = transport.call(cancel_command(), Duration::from_secs(2));
        }
        let answers: Vec<_> = permission_answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .drain(..)
            .collect();
        for answer in answers {
            if coordinate_permission_answer(
                &mut pending_permission,
                answer,
                |request, answer| adapter.answer_extension_ui(&transport, request, answer),
                |kind, payload| {
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
                },
            )
            .is_err()
            {
                break 'coordinate;
            }
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
                if coordinate_extension_ui_request(
                    event,
                    &mut pending_permission,
                    |kind, payload| {
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
                    },
                )
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
    pending: &mut Option<ExtensionUiRequest>,
    append: impl FnOnce(&str, Value) -> Result<(), ()>,
) -> Result<(), ()> {
    let PiChatEvent::ExtensionUiRequest(request) = event else {
        return Ok(());
    };
    append("permission.requested", permission_journal_payload(&request))?;
    *pending = Some(request);
    Ok(())
}

fn coordinate_permission_answer(
    pending: &mut Option<ExtensionUiRequest>,
    queued: PendingPermissionAnswer,
    send: impl FnOnce(&ExtensionUiRequest, ExtensionUiAnswer) -> Result<(), String>,
    append: impl FnOnce(&str, Value) -> Result<(), ()>,
) -> Result<(), ()> {
    let Some(request) = pending
        .as_ref()
        .filter(|request| request.id == queued.gate_id)
    else {
        return Ok(());
    };
    let answer = queued.answer.pi_answer();
    if ExtensionUiResponse::new(request, answer.clone()).is_err() {
        return Ok(());
    }
    if send(request, answer).is_err() {
        return Ok(());
    }
    append(
        "permission.resolved",
        json!({"gate_id": queued.gate_id, "decision": queued.answer.decision()}),
    )?;
    *pending = None;
    Ok(())
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

pub(crate) fn event_envelope(
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

enum FetchGrantError {
    Unauthorized,
    Internal(String),
}

impl FetchGrantError {
    fn into_message(self) -> String {
        match self {
            Self::Unauthorized => "The capability is not authorized.".into(),
            Self::Internal(message) => message,
        }
    }
}

fn map_fetch_grant_error(error: FetchGrantError) -> RunStartError {
    match error {
        FetchGrantError::Unauthorized => {
            RunStartError::Unauthorized("The capability is not authorized.".into())
        }
        FetchGrantError::Internal(message) => RunStartError::Persistence(message),
    }
}

fn grant_status_error(status: u16) -> FetchGrantError {
    if matches!(status, 401 | 403) {
        FetchGrantError::Unauthorized
    } else {
        FetchGrantError::Internal("Chat configuration is temporarily unavailable.".into())
    }
}

fn fetch_grant(access_token: &str) -> Result<ChatGrant, FetchGrantError> {
    let issuer =
        std::env::var("MUNIMENT_ISSUER").unwrap_or_else(|_| "https://api.muniment.ai".into());
    ureq::post(&format!(
        "{}/v1/desktop/chat/config",
        issuer.trim_end_matches('/')
    ))
    .set("Authorization", &format!("Bearer {access_token}"))
    .call()
    .map_err(|error| match error {
        ureq::Error::Status(status, _) => grant_status_error(status),
        _ => FetchGrantError::Internal("Chat configuration is temporarily unavailable.".into()),
    })?
    .into_json()
    .map_err(|_| FetchGrantError::Internal("The chat configuration response was invalid.".into()))
}

fn validate_grant(grant: &ChatGrant) -> Result<(), String> {
    if !grant.gateway_url.starts_with("https://")
        || !grant.receipt_url.starts_with("https://")
        || grant.virtual_key.trim().is_empty()
        || grant.workspace.trim().is_empty()
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
    use crate::test_support::FakeRunStartBoundaries;
    use base64::{engine::general_purpose::STANDARD, Engine};
    use std::sync::atomic::AtomicUsize;

    static PI_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn run_start_coordinator_accepts_through_injected_boundaries() {
        let boundaries = FakeRunStartBoundaries::accepting();

        let result = start_desktop_run(
            &boundaries,
            RunStartRequest {
                prompt: "  hello  ".into(),
                files: Vec::new(),
                workspace: None,
                provenance: None,
            },
        )
        .unwrap();

        assert_eq!(boundaries.auth_calls.load(Ordering::SeqCst), 1);
        assert_eq!(result.attachments.len(), 0);
        assert_eq!(
            boundaries.launched_run.lock().unwrap().as_deref(),
            Some(result.run_id.as_str())
        );
    }

    #[test]
    fn run_start_coordinator_rejects_when_another_run_is_active() {
        let boundaries = FakeRunStartBoundaries {
            active: true,
            ..FakeRunStartBoundaries::accepting()
        };

        let error = start_desktop_run(
            &boundaries,
            RunStartRequest {
                prompt: "hello".into(),
                files: Vec::new(),
                workspace: None,
                provenance: None,
            },
        )
        .err()
        .unwrap();

        assert_eq!(error.into_message(), "A reply is already in progress.");
        assert_eq!(boundaries.auth_calls.load(Ordering::SeqCst), 0);
        assert!(boundaries.launched_run.lock().unwrap().is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn grant_http_authentication_statuses_are_unauthorized_and_redacted() {
        for status in [401, 403] {
            let error = map_fetch_grant_error(grant_status_error(status)).protocol_error();
            let encoded = serde_json::to_string(&error).unwrap();
            assert!(encoded.contains("unauthorized"), "{encoded}");
            for secret in ["private-prompt", "secret-token", "/private/work", "sidecar"] {
                assert!(!encoded.contains(secret), "leaked {secret}: {encoded}");
            }
        }

        let internal = map_fetch_grant_error(grant_status_error(500)).protocol_error();
        assert_eq!(
            serde_json::to_value(internal).unwrap()["code"],
            "persistence_failed"
        );
    }

    fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        match mutex.lock() {
            Ok(guard) => guard,
            Err(error) => error.into_inner(),
        }
    }

    // Acquire the shared PI-environment lock without propagating poisoning: if a
    // prior test panicked while holding the guard (e.g. a slow CI VM tripped the
    // receipt-handshake deadline), recover the guard instead of turning that one
    // flake into cascaded PoisonError failures across every sibling test.
    fn lock_pi_environment() -> std::sync::MutexGuard<'static, ()> {
        lock_unpoisoned(&PI_ENV_LOCK)
    }

    #[test]
    fn lock_unpoisoned_recovers_guard_after_panic() {
        let mutex = Mutex::new(0);

        std::thread::scope(|scope| {
            let result = scope.spawn(|| {
                let mut value = mutex.lock().unwrap();
                *value = 42;
                panic!("poison mutex");
            });
            assert!(result.join().is_err());
        });

        assert!(mutex.is_poisoned());
        let value = lock_unpoisoned(&mutex);
        assert_eq!(*value, 42);
    }

    fn accept_receipt_request(listener: std::net::TcpListener) -> std::net::TcpStream {
        listener.set_nonblocking(true).unwrap();
        // 120s (not 10s): on the contended single linux CI VM the coordinator can
        // take over a minute to open its receipt connection. A healthy run still
        // connects in milliseconds; this is only a generous ceiling before we
        // declare the handshake genuinely broken.
        let deadline = std::time::Instant::now() + Duration::from_secs(120);
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

    fn read_sidecar_request_log(path: &std::path::Path) -> String {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match std::fs::read_to_string(path) {
                Ok(requests) if !requests.is_empty() => return requests,
                Ok(_) => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "sidecar recorded an empty coordinator request at {}",
                        path.display()
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "sidecar did not record a coordinator request at {}",
                        path.display()
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!(
                    "failed to read sidecar coordinator requests at {}: {error}",
                    path.display()
                ),
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
            permission_answers: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    #[test]
    fn session_thread_continues_at_the_next_ordinal() {
        let directory =
            std::env::temp_dir().join(format!("muniment-session-thread-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let database = directory.join("runs.sqlite3");
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(&database).unwrap(),
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let tracker = SessionThread::default();
        let first_run = Uuid::now_v7().to_string();
        let second_run = Uuid::now_v7().to_string();

        prepare_new_run_with_session_thread(
            &storage,
            SessionThreadStart {
                tracker: &tracker,
                continue_existing: true,
            },
            &first_run,
            "workspace-a",
            Some("owner"),
            Vec::new(),
            None,
        )
        .unwrap();
        let OfferedThread::Selected(thread_id) = tracker.offered("workspace-a", Some("owner"))
        else {
            panic!("the first run must record its thread");
        };
        prepare_new_run_with_session_thread(
            &storage,
            SessionThreadStart {
                tracker: &tracker,
                continue_existing: true,
            },
            &second_run,
            "workspace-a",
            Some("owner"),
            Vec::new(),
            None,
        )
        .unwrap();

        let connection = rusqlite::Connection::open(&database).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT thread_run_ordinal FROM run_threads \
                     WHERE run_id=?1 AND thread_id=?2",
                    (&second_run, &thread_id),
                    |row| row.get::<_, u64>(0),
                )
                .unwrap(),
            2
        );
        assert_eq!(
            tracker.offered("workspace-a", Some("owner")),
            OfferedThread::Selected(thread_id)
        );

        drop(connection);
        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn fresh_session_tracker_adopts_the_newest_owned_workspace_thread() {
        let directory =
            std::env::temp_dir().join(format!("muniment-session-adoption-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let database = directory.join("runs.sqlite3");
        let mut journal = RunJournal::open(&database).unwrap();
        let first_run = Uuid::now_v7().to_string();
        journal
            .append_new_run(
                "workspace-a",
                &event_envelope(&first_run, 1, "run.started", json!({}), Some("owner")),
            )
            .unwrap();
        drop(journal);
        let connection = rusqlite::Connection::open(&database).unwrap();
        let thread_id = connection
            .query_row(
                "SELECT thread_id FROM run_threads WHERE run_id=?1",
                [&first_run],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        drop(connection);
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(&database).unwrap(),
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let tracker = SessionThread::default();
        let second_run = Uuid::now_v7().to_string();

        prepare_new_run_with_session_thread(
            &storage,
            SessionThreadStart {
                tracker: &tracker,
                continue_existing: true,
            },
            &second_run,
            "workspace-a",
            Some("owner"),
            Vec::new(),
            None,
        )
        .unwrap();

        let connection = rusqlite::Connection::open(&database).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT thread_run_ordinal FROM run_threads \
                     WHERE run_id=?1 AND thread_id=?2",
                    (&second_run, &thread_id),
                    |row| row.get::<_, u64>(0),
                )
                .unwrap(),
            2
        );
        assert_eq!(
            tracker.offered("workspace-a", Some("owner")),
            OfferedThread::Selected(thread_id)
        );

        drop(connection);
        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn fresh_session_tracker_skips_a_newer_foreign_thread() {
        let directory =
            std::env::temp_dir().join(format!("muniment-session-owner-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let database = directory.join("runs.sqlite3");
        let owner_run = Uuid::now_v7().to_string();
        let foreign_run = Uuid::now_v7().to_string();
        let mut owner_started =
            event_envelope(&owner_run, 1, "run.started", json!({}), Some("owner"));
        owner_started.recorded_at = "2026-01-01T00:00:00Z".into();
        let mut foreign_started =
            event_envelope(&foreign_run, 1, "run.started", json!({}), Some("other"));
        foreign_started.recorded_at = "2026-01-02T00:00:00Z".into();
        let mut journal = RunJournal::open(&database).unwrap();
        journal
            .append_new_run("workspace-a", &owner_started)
            .unwrap();
        journal
            .append_new_run("workspace-a", &foreign_started)
            .unwrap();
        drop(journal);
        let connection = rusqlite::Connection::open(&database).unwrap();
        let owner_thread = connection
            .query_row(
                "SELECT thread_id FROM run_threads WHERE run_id=?1",
                [&owner_run],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        drop(connection);
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(&database).unwrap(),
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let tracker = SessionThread::default();
        let continued_run = Uuid::now_v7().to_string();

        prepare_new_run_with_session_thread(
            &storage,
            SessionThreadStart {
                tracker: &tracker,
                continue_existing: true,
            },
            &continued_run,
            "workspace-a",
            Some("owner"),
            Vec::new(),
            None,
        )
        .unwrap();

        assert_eq!(
            tracker.offered("workspace-a", Some("owner")),
            OfferedThread::Selected(owner_thread)
        );

        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn fresh_session_tracker_mints_a_thread_when_no_candidate_exists() {
        let directory =
            std::env::temp_dir().join(format!("muniment-session-empty-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let database = directory.join("runs.sqlite3");
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(&database).unwrap(),
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let tracker = SessionThread::default();
        let run_id = Uuid::now_v7().to_string();

        prepare_new_run_with_session_thread(
            &storage,
            SessionThreadStart {
                tracker: &tracker,
                continue_existing: true,
            },
            &run_id,
            "workspace-a",
            Some("owner"),
            Vec::new(),
            None,
        )
        .unwrap();

        let connection = rusqlite::Connection::open(&database).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM events \
                     WHERE run_id=?1 AND event_type='run.started'",
                    [&run_id],
                    |row| row.get::<_, u64>(0),
                )
                .unwrap(),
            1
        );
        assert!(matches!(
            tracker.offered("workspace-a", Some("owner")),
            OfferedThread::Selected(_)
        ));

        drop(connection);
        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn selected_thread_overrides_the_newest_workspace_thread() {
        let directory =
            std::env::temp_dir().join(format!("muniment-session-selected-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let database = directory.join("runs.sqlite3");
        let selected_run = Uuid::now_v7().to_string();
        let newest_run = Uuid::now_v7().to_string();
        let mut journal = RunJournal::open(&database).unwrap();
        journal
            .append_new_run(
                "workspace-a",
                &event_envelope(&selected_run, 1, "run.started", json!({}), Some("owner")),
            )
            .unwrap();
        journal
            .append_new_run(
                "workspace-a",
                &event_envelope(&newest_run, 1, "run.started", json!({}), Some("owner")),
            )
            .unwrap();
        drop(journal);
        let connection = rusqlite::Connection::open(&database).unwrap();
        let selected_thread = connection
            .query_row(
                "SELECT thread_id FROM run_threads WHERE run_id=?1",
                [&selected_run],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        drop(connection);
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(&database).unwrap(),
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let tracker = SessionThread::default();
        tracker.select(selected_thread.clone(), Some("owner"));
        let continued_run = Uuid::now_v7().to_string();

        prepare_new_run_with_session_thread(
            &storage,
            SessionThreadStart {
                tracker: &tracker,
                continue_existing: true,
            },
            &continued_run,
            "workspace-a",
            Some("owner"),
            Vec::new(),
            None,
        )
        .unwrap();

        let connection = rusqlite::Connection::open(&database).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT thread_id FROM run_threads WHERE run_id=?1",
                    [&continued_run],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            selected_thread
        );

        drop(connection);
        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn fresh_choice_overrides_the_newest_workspace_thread() {
        let directory =
            std::env::temp_dir().join(format!("muniment-session-fresh-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let database = directory.join("runs.sqlite3");
        let existing_run = Uuid::now_v7().to_string();
        let mut journal = RunJournal::open(&database).unwrap();
        journal
            .append_new_run(
                "workspace-a",
                &event_envelope(&existing_run, 1, "run.started", json!({}), Some("owner")),
            )
            .unwrap();
        drop(journal);
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(&database).unwrap(),
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let tracker = SessionThread::default();
        tracker.fresh();
        let fresh_run = Uuid::now_v7().to_string();

        prepare_new_run_with_session_thread(
            &storage,
            SessionThreadStart {
                tracker: &tracker,
                continue_existing: true,
            },
            &fresh_run,
            "workspace-a",
            Some("owner"),
            Vec::new(),
            None,
        )
        .unwrap();

        let connection = rusqlite::Connection::open(&database).unwrap();
        let thread_for = |run_id: &str| {
            connection
                .query_row(
                    "SELECT thread_id FROM run_threads WHERE run_id=?1",
                    [run_id],
                    |row| row.get::<_, String>(0),
                )
                .unwrap()
        };
        assert_ne!(thread_for(&fresh_run), thread_for(&existing_run));
        assert_eq!(
            tracker.offered("workspace-a", Some("owner")),
            OfferedThread::Selected(thread_for(&fresh_run))
        );

        drop(connection);
        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejected_session_thread_falls_back_without_duplicate_start() {
        let directory =
            std::env::temp_dir().join(format!("muniment-session-fallback-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let database = directory.join("runs.sqlite3");
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(&database).unwrap(),
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let tracker = SessionThread::default();
        tracker.record("unknown-thread".into(), "workspace-a", Some("owner"));
        let run_id = Uuid::now_v7().to_string();

        prepare_new_run_with_session_thread(
            &storage,
            SessionThreadStart {
                tracker: &tracker,
                continue_existing: true,
            },
            &run_id,
            "workspace-a",
            Some("owner"),
            Vec::new(),
            None,
        )
        .unwrap();

        let connection = rusqlite::Connection::open(&database).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM events \
                     WHERE run_id=?1 AND event_type='run.started'",
                    [&run_id],
                    |row| row.get::<_, u64>(0),
                )
                .unwrap(),
            1
        );
        assert_ne!(
            tracker.offered("workspace-a", Some("owner")),
            OfferedThread::Selected("unknown-thread".into())
        );

        drop(connection);
        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn disabled_continuation_leaves_the_session_thread_unchanged() {
        let directory =
            std::env::temp_dir().join(format!("muniment-session-disabled-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(directory.join("runs.sqlite3")).unwrap(),
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let tracker = SessionThread::default();
        tracker.record("thread-a".into(), "workspace-a", Some("owner"));

        prepare_new_run_with_session_thread(
            &storage,
            SessionThreadStart {
                tracker: &tracker,
                continue_existing: false,
            },
            &Uuid::now_v7().to_string(),
            "workspace-a",
            Some("owner"),
            Vec::new(),
            None,
        )
        .unwrap();

        assert_eq!(
            tracker.offered("workspace-a", Some("owner")),
            OfferedThread::Selected("thread-a".into())
        );

        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
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
            "workspace-a",
            Some("owner"),
            vec![SelectedFile { path: first }, SelectedFile { path: second }],
            None,
        )
        .unwrap();
        assert_eq!(seq, 3);
        let mut storage = storage.lock().unwrap();
        assert!(storage
            .journal
            .run_belongs_to_workspace(&run_id, "workspace-a")
            .unwrap());
        assert!(!storage
            .journal
            .run_belongs_to_workspace(&run_id, "owner")
            .unwrap());
        assert_eq!(
            storage
                .journal
                .workspace_thread_summaries("workspace-a", 10, None)
                .unwrap()
                .summaries
                .len(),
            1
        );
        assert!(storage
            .journal
            .projected_thread_entries("workspace-a", &run_id, seq, 0, 10)
            .is_ok());
        assert!(storage
            .journal
            .projected_thread_entries("owner", &run_id, seq, 0, 10)
            .is_err());
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
        let _environment = lock_pi_environment();
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
            "workspace-a",
            Some("owner"),
            vec![SelectedFile {
                path: missing.clone(),
            }],
            None,
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
            "workspace-a",
            Some("owner"),
            vec![SelectedFile {
                path: directory.clone(),
            }],
            None,
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
        let _environment = lock_pi_environment();
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

        let error = match prepare_opened_run(
            &storage,
            SessionThreadStart {
                tracker: &SessionThread::default(),
                continue_existing: false,
            },
            &run_id,
            "workspace-a",
            Some("owner"),
            opened,
            None,
        ) {
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
        let app = tauri::test::mock_app();
        let directory =
            std::env::temp_dir().join(format!("muniment-coordinate-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let first = directory.join("first.txt");
        let second = directory.join("second.txt");
        std::fs::write(&first, b"first attachment").unwrap();
        std::fs::write(&second, b"second attachment").unwrap();
        std::fs::write(directory.join("session.jsonl"), "{}\n").unwrap();
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(directory.join("runs.sqlite3")).unwrap(),
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let run_id = Uuid::now_v7().to_string();
        let prepared = prepare_new_run(
            &storage,
            &run_id,
            "workspace-a",
            Some("owner"),
            vec![SelectedFile { path: first }, SelectedFile { path: second }],
            None,
        )
        .unwrap();
        let (locator, _) = validate_pi_session(&directory, "session.jsonl").unwrap();

        assert_eq!(prepared.0, 3);
        let (mut seq, mut projector) = prepared;
        let prompt_submissions = AtomicUsize::new(0);
        coordinate_prepared_prompt(
            app.handle(),
            &storage,
            &mut projector,
            &run_id,
            &mut seq,
            Some("owner"),
            || {
                prompt_submissions.fetch_add(1, Ordering::SeqCst);
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
                Ok(((), locator, Vec::new()))
            },
        )
        .unwrap();

        assert_eq!(prompt_submissions.load(Ordering::SeqCst), 1);
        let events = storage.lock().unwrap().journal.events(&run_id).unwrap();
        assert_eq!(
            events.iter().map(|event| event.run_seq).collect::<Vec<_>>(),
            (1..=events.len() as u64).collect::<Vec<_>>()
        );
        assert_eq!(events.len(), 5);
        assert_eq!(events[1].event_type, "chat.attachment.ingested");
        assert_eq!(events[2].event_type, "chat.attachment.ingested");
        assert_eq!(events[3].run_seq, 4);
        assert_eq!(events[3].event_type, "runtime.pi_session.bound");
        assert_eq!(events[4].event_type, "model.prompt.accepted");
        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn coordinator_prepares_supported_images_in_journal_order_and_omits_unsupported_files() {
        let directory = std::env::temp_dir().join(format!("muniment-images-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let png = directory.join("renamed.bin");
        let unsupported = directory.join("notes.png");
        let gif = directory.join("second.dat");
        std::fs::write(&png, valid_test_png()).unwrap();
        std::fs::write(&unsupported, b"durable but not an image").unwrap();
        let gif_data = "R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==";
        std::fs::write(&gif, STANDARD.decode(gif_data).unwrap()).unwrap();
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(directory.join("runs.sqlite3")).unwrap(),
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let run_id = Uuid::now_v7().to_string();
        prepare_new_run(
            &storage,
            &run_id,
            "workspace-a",
            Some("owner"),
            vec![
                SelectedFile { path: png },
                SelectedFile { path: unsupported },
                SelectedFile { path: gif },
            ],
            None,
        )
        .unwrap();

        assert_eq!(
            prepared_pi_images(&storage, &run_id).unwrap(),
            vec![
                PiImageContent::new(
                    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=",
                    "image/png"
                ),
                PiImageContent::new(gif_data, "image/gif"),
            ]
        );
        assert_eq!(
            storage
                .lock()
                .unwrap()
                .journal
                .events(&run_id)
                .unwrap()
                .iter()
                .filter(|event| matches!(event.payload, EventPayload::Attachment { .. }))
                .count(),
            3
        );

        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn coordinator_sends_exact_ordered_images_and_preserves_text_only_prompt_shape() {
        let _environment = lock_pi_environment();
        for with_images in [true, false] {
            let app = tauri::test::mock_app();
            // The coordinator refuses to create the session root itself
            // (ownership must be established by the install flow), so a fresh
            // machine needs it created here, exactly like the resume test.
            let sessions = app.path().app_data_dir().unwrap().join("pi-sessions");
            std::fs::create_dir_all(&sessions).unwrap();
            let directory =
                std::env::temp_dir().join(format!("muniment-prompt-capture-{}", Uuid::now_v7()));
            std::fs::create_dir_all(&directory).unwrap();
            let request_log = directory.join("requests.jsonl");
            let mut files = Vec::new();
            if with_images {
                let png = directory.join("first.bin");
                let unsupported = directory.join("local-only.png");
                let gif = directory.join("second.dat");
                std::fs::write(&png, valid_test_png()).unwrap();
                std::fs::write(&unsupported, b"not an image").unwrap();
                std::fs::write(
                    &gif,
                    STANDARD
                        .decode("R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==")
                        .unwrap(),
                )
                .unwrap();
                files = vec![
                    SelectedFile { path: png },
                    SelectedFile { path: unsupported },
                    SelectedFile { path: gif },
                ];
            }
            let storage = Arc::new(Mutex::new(ChatStorage {
                journal: RunJournal::open(directory.join("runs.sqlite3")).unwrap(),
                cas: LocalCas::open(&directory.join("cas")).unwrap(),
            }));
            let run_id = Uuid::now_v7().to_string();
            let prepared =
                prepare_new_run(&storage, &run_id, "workspace-a", Some("owner"), files, None)
                    .unwrap();

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
            listener.set_nonblocking(true).unwrap();
            let (stop_receipt_server, receipt_server_stop) = std::sync::mpsc::channel();
            let receipt_server = std::thread::spawn(move || {
                use std::io::{Read, Write};
                loop {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let mut request = [0; 4096];
                            let _ = stream.read(&mut request).unwrap();
                            let body = r#"{"route":"capture-stub","model":"test"}"#;
                            write!(
                                stream,
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                body.len(),
                                body
                            )
                            .unwrap();
                            break;
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            if receipt_server_stop.try_recv().is_ok() {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(error) => panic!("receipt listener failed: {error}"),
                    }
                }
            });
            std::env::set_var("MUNIMENT_PI_ROOT", &directory);
            std::env::set_var("MUNIMENT_PI_TEST_EXECUTABLE", &stub);
            std::env::set_var("PI_RESUME_STUB_REQUESTS", &request_log);
            coordinate(
                app.handle().clone(),
                Arc::clone(&storage),
                Arc::new(Mutex::new(None)),
                run_id,
                "original text prompt".into(),
                "token".into(),
                Some("owner".into()),
                ChatGrant {
                    workspace: "workspace-a".into(),
                    gateway_url: "https://gateway.invalid".into(),
                    virtual_key: "virtual-key".into(),
                    model: None,
                    receipt_url,
                },
                Arc::new(AtomicBool::new(false)),
                Arc::new(Mutex::new(None)),
                Arc::new(Mutex::new(None)),
                Arc::new(Mutex::new(VecDeque::new())),
                None,
                None,
                Some(prepared),
            );
            let _ = stop_receipt_server.send(());
            receipt_server.join().unwrap();
            for key in [
                "MUNIMENT_PI_ROOT",
                "MUNIMENT_PI_TEST_EXECUTABLE",
                "PI_RESUME_STUB_REQUESTS",
            ] {
                std::env::remove_var(key);
            }

            let requests = read_sidecar_request_log(&request_log);
            let prompt: serde_json::Value =
                serde_json::from_str(requests.lines().next().unwrap()).unwrap();
            assert_eq!(prompt["type"], "prompt");
            assert_eq!(prompt["message"], "original text prompt");
            if with_images {
                assert_eq!(
                    prompt["images"],
                    json!([
                        {
                            "type": "image",
                            "data": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=",
                            "mimeType": "image/png"
                        },
                        {
                            "type": "image",
                            "data": "R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==",
                            "mimeType": "image/gif"
                        }
                    ])
                );
            } else {
                assert!(prompt.get("images").is_none());
            }
            drop(storage);
            std::fs::remove_dir_all(directory).unwrap();
        }
    }

    #[test]
    fn missing_oversized_and_ambiguous_images_fail_before_prompt_with_non_leaking_copy() {
        let _environment = lock_pi_environment();
        for (name, bytes, remove_object) in [
            ("private-missing.png", valid_test_png(), true),
            (
                "private-large.jpg",
                {
                    let mut bytes =
                        vec![
                            0_u8;
                            usize::try_from(muniment_core::attachment::MAX_PI_IMAGE_BYTES + 1)
                                .unwrap()
                        ];
                    bytes[..3].copy_from_slice(b"\xff\xd8\xff");
                    let length = bytes.len();
                    bytes[length - 2..].copy_from_slice(b"\xff\xd9");
                    bytes
                },
                false,
            ),
            (
                "private-malformed.jpg",
                b"\xff\xd8\xffprivate-attachment-marker\xff\xd9".to_vec(),
                false,
            ),
        ] {
            let directory =
                std::env::temp_dir().join(format!("muniment-image-fail-{}", Uuid::now_v7()));
            std::fs::create_dir_all(&directory).unwrap();
            let path = directory.join(name);
            std::fs::write(&path, &bytes).unwrap();
            let storage = Arc::new(Mutex::new(ChatStorage {
                journal: RunJournal::open(directory.join("runs.sqlite3")).unwrap(),
                cas: LocalCas::open(&directory.join("cas")).unwrap(),
            }));
            let run_id = Uuid::now_v7().to_string();
            let prepared = prepare_new_run(
                &storage,
                &run_id,
                "workspace-a",
                Some("owner"),
                vec![SelectedFile { path: path.clone() }],
                None,
            )
            .unwrap();
            let attachment_hash = {
                let mut storage = storage.lock().unwrap();
                let events = storage.journal.events(&run_id).unwrap();
                let EventPayload::Attachment { attachment } = &events[1].payload else {
                    panic!("attachment event")
                };
                attachment.sha256().clone()
            };
            if remove_object {
                storage
                    .lock()
                    .unwrap()
                    .cas
                    .remove(&attachment_hash)
                    .unwrap();
            }

            let request_log = directory.join("requests.jsonl");
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
            listener.set_nonblocking(true).unwrap();
            let (stop_receipt_server, receipt_server_stop) = std::sync::mpsc::channel();
            let receipt_server = std::thread::spawn(move || {
                use std::io::{Read, Write};
                loop {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let mut request = [0; 4096];
                            let _ = stream.read(&mut request).unwrap();
                            let body = r#"{"route":"capture-stub","model":"test"}"#;
                            write!(
                                stream,
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                body.len(),
                                body
                            )
                            .unwrap();
                            break;
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            if receipt_server_stop.try_recv().is_ok() {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(error) => panic!("receipt listener failed: {error}"),
                    }
                }
            });
            std::env::set_var("MUNIMENT_PI_ROOT", &directory);
            std::env::set_var("MUNIMENT_PI_TEST_EXECUTABLE", &stub);
            std::env::set_var("PI_RESUME_STUB_REQUESTS", &request_log);
            coordinate(
                tauri::test::mock_app().handle().clone(),
                Arc::clone(&storage),
                Arc::new(Mutex::new(None)),
                run_id.clone(),
                "private prompt bytes".into(),
                "token".into(),
                Some("owner".into()),
                ChatGrant {
                    workspace: "workspace-a".into(),
                    gateway_url: "https://gateway.invalid".into(),
                    virtual_key: "virtual-key".into(),
                    model: None,
                    receipt_url,
                },
                Arc::new(AtomicBool::new(false)),
                Arc::new(Mutex::new(None)),
                Arc::new(Mutex::new(None)),
                Arc::new(Mutex::new(VecDeque::new())),
                None,
                None,
                Some(prepared),
            );
            stop_receipt_server.send(()).unwrap();
            receipt_server.join().unwrap();
            std::env::remove_var("MUNIMENT_PI_ROOT");
            std::env::remove_var("MUNIMENT_PI_TEST_EXECUTABLE");
            std::env::remove_var("PI_RESUME_STUB_REQUESTS");
            assert!(!request_log.exists(), "Pi must receive zero prompts");
            let events = storage.lock().unwrap().journal.events(&run_id).unwrap();
            assert_eq!(events.last().unwrap().event_type, "run.failed");
            let public_error = serde_json::to_string(events.last().unwrap()).unwrap();
            assert!(public_error.contains(&attachment_error()));
            for secret in [
                path.to_string_lossy().as_ref(),
                attachment_hash.as_str(),
                &STANDARD.encode(&bytes),
                "private-attachment-marker",
            ] {
                assert!(!public_error.contains(secret));
            }

            drop(storage);
            std::fs::remove_dir_all(directory).unwrap();
        }
    }

    fn valid_test_png() -> Vec<u8> {
        STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")
            .unwrap()
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
    fn permission_answer_handoff_rejects_stale_runs_and_queues_typed_answers() {
        let active = Mutex::new(Some(inactive_transport_run("run-1")));
        assert_eq!(
            queue_permission_answer(
                &active,
                "stale-run".into(),
                "gate-1".into(),
                ChatPermissionAnswer::Confirm(true),
            )
            .unwrap_err(),
            "That reply is no longer active."
        );
        queue_permission_answer(
            &active,
            "run-1".into(),
            "gate-1".into(),
            ChatPermissionAnswer::Select("A".into()),
        )
        .unwrap();
        let active = active.lock().unwrap();
        let queued = active
            .as_ref()
            .unwrap()
            .permission_answers
            .lock()
            .unwrap()
            .pop_front()
            .unwrap();
        assert_eq!(queued.gate_id, "gate-1");
        assert!(matches!(queued.answer, ChatPermissionAnswer::Select(value) if value == "A"));
    }

    #[test]
    fn permission_answers_are_closed_and_cover_each_dialog() {
        for value in [
            json!({"type": "select", "value": "A"}),
            json!({"type": "confirm", "value": true}),
            json!({"type": "input", "value": "text"}),
            json!({"type": "editor", "value": "draft"}),
            json!({"type": "cancelled"}),
        ] {
            assert!(serde_json::from_value::<ChatPermissionAnswer>(value).is_ok());
        }
        for value in [
            json!({"type": "select", "value": 1}),
            json!({"type": "unknown", "value": "A"}),
            json!({"type": "cancelled", "extra": true}),
        ] {
            assert!(serde_json::from_value::<ChatPermissionAnswer>(value).is_err());
        }
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
        let _environment = lock_pi_environment();
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
                workspace: "workspace-a".into(),
                gateway_url: "https://gateway.invalid".into(),
                virtual_key: "virtual-key".into(),
                model: None,
                receipt_url: "https://receipt.invalid".into(),
            },
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(VecDeque::new())),
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
        let _environment = lock_pi_environment();
        let app = tauri::test::mock_app();
        let directory = std::env::temp_dir().join(format!("muniment-resume-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let args_log = directory.join("args.txt");
        let prompt_log = directory.join("prompt.txt");
        let request_log = directory.join("requests.jsonl");
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
        std::env::set_var("PI_RESUME_STUB_REQUESTS", &request_log);
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
                workspace: "workspace-a".into(),
                gateway_url: "https://gateway.invalid".into(),
                virtual_key: "virtual-key".into(),
                model: None,
                receipt_url,
            },
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(VecDeque::new())),
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
            "PI_RESUME_STUB_REQUESTS",
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
        let request: serde_json::Value =
            serde_json::from_str(std::fs::read_to_string(request_log).unwrap().trim()).unwrap();
        assert!(request.get("images").is_none());

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
        let mut pending = None;

        coordinate_extension_ui_request(
            PiChatEvent::ExtensionUiRequest(ExtensionUiRequest {
                id: "pi-request-1".into(),
                dialog: ExtensionUiDialog::Confirm {
                    title: "Allow?".into(),
                    message: "Proceed?".into(),
                },
                timeout: Some(5_000),
            }),
            &mut pending,
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
            &mut pending,
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
            &mut pending,
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
    fn coordinator_resolves_only_matching_valid_permission_answers() {
        let request = ExtensionUiRequest {
            id: "gate-1".into(),
            dialog: ExtensionUiDialog::Select {
                title: "Choose".into(),
                options: vec!["A".into(), "B".into()],
            },
            timeout: None,
        };
        let mut pending = Some(request.clone());
        let mut sent = Vec::new();
        let mut appended = Vec::new();
        coordinate_permission_answer(
            &mut pending,
            PendingPermissionAnswer {
                gate_id: "other-gate".into(),
                answer: ChatPermissionAnswer::Select("A".into()),
            },
            |_, answer| {
                sent.push(answer);
                Ok(())
            },
            |kind, payload| {
                appended.push((kind.to_string(), payload));
                Ok(())
            },
        )
        .unwrap();
        assert!(sent.is_empty());
        assert!(appended.is_empty());
        assert_eq!(pending, Some(request.clone()));

        coordinate_permission_answer(
            &mut pending,
            PendingPermissionAnswer {
                gate_id: "gate-1".into(),
                answer: ChatPermissionAnswer::Select("C".into()),
            },
            |_, answer| {
                sent.push(answer);
                Ok(())
            },
            |kind, payload| {
                appended.push((kind.to_string(), payload));
                Ok(())
            },
        )
        .unwrap();
        assert!(sent.is_empty());
        assert!(appended.is_empty());
        assert_eq!(pending, Some(request));

        coordinate_permission_answer(
            &mut pending,
            PendingPermissionAnswer {
                gate_id: "gate-1".into(),
                answer: ChatPermissionAnswer::Select("B".into()),
            },
            |_, answer| {
                sent.push(answer);
                Ok(())
            },
            |kind, payload| {
                appended.push((kind.to_string(), payload));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(sent, [ExtensionUiAnswer::Selection("B".into())]);
        assert_eq!(
            appended,
            [(
                "permission.resolved".into(),
                json!({
                    "gate_id": "gate-1",
                    "decision": {"type": "select", "value": "B"}
                }),
            )]
        );
        assert!(pending.is_none());
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
    fn thread_summary_page_fills_across_foreign_core_page_boundaries() {
        let directory = std::env::temp_dir().join(format!("muniment-chat-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("runs.sqlite3");
        let runs = [
            ("01900000-0000-7000-8000-000000000021", "other"),
            ("01900000-0000-7000-8000-000000000022", "owner"),
            ("01900000-0000-7000-8000-000000000023", "other"),
            ("01900000-0000-7000-8000-000000000024", "owner"),
        ];
        let mut journal = RunJournal::open(&path).unwrap();
        for (index, (run_id, subject)) in runs.into_iter().enumerate() {
            let mut envelope = event_envelope(run_id, 1, "run.started", json!({}), Some(subject));
            envelope.recorded_at = format!("2026-01-{:02}T00:00:00Z", 4 - index);
            journal.append_new_run("workspace-a", &envelope).unwrap();
        }

        let page = chat_thread_summaries_page(&mut journal, Some("owner"), 2, None).unwrap();
        assert_eq!(page.summaries.len(), 2);
        assert!(page.next_cursor.is_none());

        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn thread_summary_page_exhausts_an_all_foreign_journal() {
        let directory = std::env::temp_dir().join(format!("muniment-chat-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("runs.sqlite3");
        let mut journal = RunJournal::open(&path).unwrap();
        for index in 0..3 {
            let run_id = format!("01900000-0000-7000-8000-{index:012x}");
            journal
                .append_new_run(
                    "workspace-a",
                    &event_envelope(&run_id, 1, "run.started", json!({}), Some("other")),
                )
                .unwrap();
        }

        let page = chat_thread_summaries_page(&mut journal, Some("owner"), 2, None).unwrap();
        assert!(page.summaries.is_empty());
        assert!(page.next_cursor.is_none());

        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn thread_summary_page_returns_advanced_cursor_at_scan_bound() {
        let directory = std::env::temp_dir().join(format!("muniment-chat-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("runs.sqlite3");
        let mut journal = RunJournal::open(&path).unwrap();
        for index in 0..(MAX_THREAD_SUMMARY_CORE_PAGES + 2) {
            let run_id = format!("01900000-0000-7000-8001-{index:012x}");
            let subject = if index == 0 || index == MAX_THREAD_SUMMARY_CORE_PAGES + 1 {
                "owner"
            } else {
                "other"
            };
            let mut envelope = event_envelope(&run_id, 1, "run.started", json!({}), Some(subject));
            envelope.recorded_at = format!("2026-01-01T00:{:02}:{:02}Z", index / 60, index % 60);
            journal.append_new_run("workspace-a", &envelope).unwrap();
        }

        let first = chat_thread_summaries_page(&mut journal, Some("owner"), 2, None).unwrap();
        assert_eq!(first.summaries.len(), 1);
        assert!(first.next_cursor.is_some());

        let second = chat_thread_summaries_page(
            &mut journal,
            Some("owner"),
            2,
            first.next_cursor.as_deref(),
        )
        .unwrap();
        assert_eq!(second.summaries.len(), 1);
        assert!(second.next_cursor.is_none());

        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn thread_history_pages_and_rejects_inaccessible_threads() {
        keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
        let directory = std::env::temp_dir().join(format!("muniment-chat-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("runs.sqlite3");
        let owner_runs = [
            "01900000-0000-7000-8000-000000000011",
            "01900000-0000-7000-8000-000000000012",
            "01900000-0000-7000-8000-000000000013",
            "01900000-0000-7000-8000-000000000014",
        ];
        let foreign_run = "01900000-0000-7000-8000-000000000015";
        let mut journal = RunJournal::open(&path).unwrap();
        for run_id in owner_runs {
            journal
                .append_new_run(
                    "workspace-a",
                    &event_envelope(run_id, 1, "run.started", json!({}), Some("owner")),
                )
                .unwrap();
        }
        journal
            .append_new_run(
                "workspace-a",
                &event_envelope(foreign_run, 1, "run.started", json!({}), Some("other")),
            )
            .unwrap();
        drop(journal);

        let connection = rusqlite::Connection::open(&path).unwrap();
        let thread_for = |run_id: &str| {
            connection
                .query_row(
                    "SELECT thread_id FROM run_threads WHERE run_id=?1",
                    [run_id],
                    |row| row.get::<_, String>(0),
                )
                .unwrap()
        };
        let open_thread = thread_for(owner_runs[0]);
        let merged_thread = thread_for(owner_runs[1]);
        let deleted_thread = thread_for(owner_runs[2]);
        let foreign_thread = thread_for(foreign_run);
        connection
            .execute(
                "UPDATE run_threads SET thread_id=?1,thread_run_ordinal=2 WHERE run_id=?2",
                rusqlite::params![open_thread, owner_runs[1]],
            )
            .unwrap();
        connection
            .execute(
                "DELETE FROM thread_events WHERE thread_id=?1",
                [merged_thread],
            )
            .unwrap();
        drop(connection);

        let mut journal = RunJournal::open(&path).unwrap();
        let first = chat_thread_summaries_page(&mut journal, Some("owner"), 1, None).unwrap();
        let second = chat_thread_summaries_page(
            &mut journal,
            Some("owner"),
            1,
            first.next_cursor.as_deref(),
        )
        .unwrap();
        let third = chat_thread_summaries_page(
            &mut journal,
            Some("owner"),
            1,
            second.next_cursor.as_deref(),
        )
        .unwrap();
        let fourth = chat_thread_summaries_page(
            &mut journal,
            Some("owner"),
            1,
            third.next_cursor.as_deref(),
        )
        .unwrap();
        let visible = [first, second, third, fourth]
            .into_iter()
            .flat_map(|page| page.summaries)
            .map(|summary| summary.thread_id)
            .collect::<BTreeSet<_>>();
        assert_eq!(visible.len(), 3);
        assert!(!visible.contains(&foreign_thread));

        let first_open = chat_thread_open_page(
            &mut journal,
            Some("owner"),
            &directory,
            &open_thread,
            1,
            None,
        )
        .unwrap();
        assert_eq!(first_open.entries[0].run_id, owner_runs[0]);
        let second_open = chat_thread_open_page(
            &mut journal,
            Some("owner"),
            &directory,
            &open_thread,
            1,
            first_open.next_cursor.as_deref(),
        )
        .unwrap();
        assert_eq!(second_open.entries[0].run_id, owner_runs[1]);
        assert!(second_open.next_cursor.is_none());

        let tracker = SessionThread::default();
        select_session_thread(&mut journal, &tracker, Some("owner"), &open_thread).unwrap();
        for thread_id in ["unknown", foreign_thread.as_str()] {
            let error =
                chat_thread_open_page(&mut journal, Some("owner"), &directory, thread_id, 1, None)
                    .err()
                    .unwrap();
            assert_eq!(error, "Conversation history is unavailable.");
            let error = select_session_thread(&mut journal, &tracker, Some("owner"), thread_id)
                .unwrap_err();
            assert_eq!(error, "Conversation history is unavailable.");
            assert_eq!(
                tracker.offered("workspace-a", Some("owner")),
                OfferedThread::Selected(open_thread.clone())
            );
        }
        journal
            .append_thread_deleted(
                1,
                &deleted_thread,
                &Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
                &Provenance {
                    source: "test".into(),
                    source_version: "1".into(),
                    actor_id: Some("owner".into()),
                    device_id: None,
                    rpc_request_id: None,
                    capability_versions: None,
                    extra: BTreeMap::new(),
                },
            )
            .unwrap();
        let error = chat_thread_open_page(
            &mut journal,
            Some("owner"),
            &directory,
            &deleted_thread,
            1,
            None,
        )
        .err()
        .unwrap();
        assert_eq!(error, "Conversation history is unavailable.");
        let error = select_session_thread(&mut journal, &tracker, Some("owner"), &deleted_thread)
            .unwrap_err();
        assert_eq!(error, "Conversation history is unavailable.");
        assert_eq!(
            tracker.offered("workspace-a", Some("owner")),
            OfferedThread::Selected(open_thread)
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

    #[test]
    fn event_type_classifier_matches_reducer_for_representative_sequences() {
        let run_id = Uuid::now_v7().to_string();
        let classify = |types: &[&str]| {
            let events: Vec<_> = types
                .iter()
                .enumerate()
                .map(|(index, event_type)| {
                    event_envelope(
                        &run_id,
                        index as u64 + 1,
                        event_type,
                        match *event_type {
                            "runtime.pi_session.bound" => {
                                json!({"run_id": run_id, "locator": "session.jsonl"})
                            }
                            _ => json!({}),
                        },
                        None,
                    )
                })
                .collect();
            let event_types: Vec<_> = events
                .iter()
                .map(|event| RunEventType {
                    run_id: event.run_id.clone(),
                    run_seq: event.run_seq,
                    event_type: event.event_type.clone(),
                })
                .collect();
            assert_eq!(
                event_types_are_terminal(&event_types),
                reduce(&events).unwrap().is_terminal(),
                "{types:?}"
            );
        };

        for types in [
            &["run.started"][..],
            &["run.started", "run.completed"],
            &["run.started", "run.cancelled"],
            &["run.started", "run.failed"],
            &["run.started", "run.needs_attention"],
            &[
                "run.started",
                "run.completed",
                "receipt.finalized",
                "capability.used",
            ],
            &[
                "run.started",
                "runtime.pi_session.bound",
                "run.needs_attention",
                "run.resumed",
                "model.prompt.accepted",
            ],
        ] {
            classify(types);
        }
    }

    #[test]
    fn event_type_classifier_treats_unknown_types_as_candidates() {
        let run_id = Uuid::now_v7().to_string();
        let events = [
            RunEventType {
                run_id: run_id.clone(),
                run_seq: 1,
                event_type: "run.started".into(),
            },
            RunEventType {
                run_id: run_id.clone(),
                run_seq: 2,
                event_type: "run.completed".into(),
            },
            RunEventType {
                run_id,
                run_seq: 3,
                event_type: "run.future_terminal".into(),
            },
        ];
        assert!(!event_types_are_terminal(&events));
    }
}
