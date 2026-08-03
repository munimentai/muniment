use std::collections::{BTreeMap, VecDeque};
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
    project_chat, reduce, ChatProjector, PermissionGate, PermissionRequest, ProjectedAttachment,
    RunStatus,
};
use muniment_core::journal::{
    EventEnvelope, EventPayload, JournalError, Provenance, RunEventType, RunJournal,
};
use muniment_core::sidecar::pi_chat::{
    cancel_command, ExtensionUiAnswer, PiChatEvent, PiImageContent, PiRunAdapter, PromptCommand,
    Receipt,
};
use muniment_core::sidecar::{
    validate_pi_session, PiRpcTransport, PiRpcWiring, PiSessionLocator, SidecarSupervisor,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use tauri::Manager;
use uuid::Uuid;

use crate::auth;
use crate::chat_coordinate::{append_emit, coordinate};
use crate::chat_threads::newest_owned_workspace_thread;
use crate::session_thread::{OfferedThread, SessionThread};

pub(super) const RPC_TIMEOUT: Duration = Duration::from_secs(30);
const QUEUE_TIMEOUT: Duration = Duration::from_secs(2);

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
    pub(crate) path: PathBuf,
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

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatToolActivity {
    pub(crate) effect_id: String,
    pub(crate) display_name: Option<String>,
    pub(crate) status: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ChatEvent {
    pub(super) run_id: String,
    pub(super) phase: String,
    pub(super) text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) receipt: Option<Value>,
    pub(super) tool_activity: Vec<ChatToolActivity>,
    pub(super) attachments: Vec<ChatAttachment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) pending_permission: Option<ChatPendingPermission>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatPendingPermission {
    pub(crate) gate_id: String,
    #[serde(flatten)]
    pub(crate) request: PermissionRequest,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAttachment {
    pub(crate) display_name: String,
    pub(crate) byte_length: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) media_type: Option<String>,
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

pub(crate) fn chat_pending_permission(
    gate: Option<PermissionGate>,
) -> Option<ChatPendingPermission> {
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
    pub(super) fn pi_answer(&self) -> ExtensionUiAnswer {
        match self {
            Self::Select(value) => ExtensionUiAnswer::Selection(value.clone()),
            Self::Confirm(value) => ExtensionUiAnswer::Confirmation(*value),
            Self::Input(value) => ExtensionUiAnswer::Input(value.clone()),
            Self::Editor(value) => ExtensionUiAnswer::Editor(value.clone()),
            Self::Cancelled => ExtensionUiAnswer::Cancelled,
        }
    }

    pub(super) fn decision(&self) -> Value {
        serde_json::to_value(self).expect("permission answers serialize")
    }
}

#[derive(Clone, Debug)]
pub(super) struct PendingPermissionAnswer {
    pub(super) gate_id: String,
    pub(super) answer: ChatPermissionAnswer,
}

pub(super) struct PiRuntime {
    pub(super) supervisor: SidecarSupervisor,
    pub(super) wiring: PiRpcWiring,
}

pub(crate) struct ChatStorage {
    pub(crate) journal: RunJournal,
    pub(crate) cas: LocalCas,
}

pub(crate) type SharedStorage = Arc<Mutex<ChatStorage>>;

pub struct ChatState {
    pub(crate) storage: SharedStorage,
    active: Mutex<Option<ActiveRun>>,
    runtime: Arc<Mutex<Option<PiRuntime>>>,
    pub(crate) session_thread: SessionThread,
}

pub(crate) struct RunStartRequest {
    pub(crate) prompt: String,
    pub(crate) files: Vec<SelectedFile>,
    pub(crate) workspace: Option<String>,
    pub(crate) provenance: Option<Provenance>,
    pub(crate) thread_id: Option<String>,
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
        thread_id: Option<&str>,
    ) -> Result<(u64, ChatProjector), RunStartError>;
    fn project_attachments(
        &self,
        projector: &ChatProjector,
    ) -> Result<Vec<ChatAttachment>, RunStartError>;
    fn run_thread_id(&self, run_id: &str) -> Result<String, RunStartError>;
    fn fail_prepared_run(&self, launch: &RunStartLaunch) -> Result<(), RunStartError>;
    fn clear_active_run(&self, run_id: &str);
    fn launch(&self, launch: RunStartLaunch);
}

#[derive(Debug)]
pub(crate) enum RunStartError {
    Unauthorized(String),
    InvalidRequest(String),
    ThreadNotFound,
    Persistence(String),
}

impl RunStartError {
    #[cfg(target_os = "linux")]
    pub(crate) fn protocol_error(&self) -> ProtocolError {
        match self {
            Self::Unauthorized(_) => ProtocolError::unauthorized(),
            Self::InvalidRequest(_) => ProtocolError::invalid_request(),
            Self::ThreadNotFound => ProtocolError::thread_not_found(),
            Self::Persistence(_) => ProtocolError::persistence_failed(),
        }
    }

    fn into_message(self) -> String {
        match self {
            Self::Unauthorized(message)
            | Self::InvalidRequest(message)
            | Self::Persistence(message) => message,
            Self::ThreadNotFound => "The thread was not found.".into(),
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
    let prepared = match boundaries.prepare_run(
        &run_id,
        &grant,
        &tokens,
        request.files,
        request.provenance,
        request.thread_id.as_deref(),
    ) {
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
        thread_id: Option<&str>,
    ) -> Result<(u64, ChatProjector), RunStartError> {
        let state = self.state();
        let result = match thread_id {
            Some(thread_id) => prepare_new_run_in_thread(
                &state.storage,
                run_id,
                &grant.workspace,
                tokens.subject.as_deref(),
                files,
                provenance,
                thread_id,
            ),
            None => prepare_new_run_with_session_thread(
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
            ),
        };
        result.map_err(|error| {
            if error == "thread_not_found" {
                RunStartError::ThreadNotFound
            } else {
                RunStartError::Persistence(error)
            }
        })
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

    fn run_thread_id(&self, run_id: &str) -> Result<String, RunStartError> {
        self.state()
            .storage
            .lock()
            .map_err(|_| RunStartError::Persistence(attachment_error()))?
            .journal
            .run_thread_id(run_id)
            .map_err(|_| RunStartError::Persistence(attachment_error()))?
            .ok_or_else(|| RunStartError::Persistence(attachment_error()))
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

pub(crate) struct ResumeContext {
    pub(super) events: Vec<EventEnvelope>,
    pub(super) locator: PiSessionLocator,
}

pub(super) struct ResumeAttempt {
    result: Option<std::sync::mpsc::Sender<Result<(), String>>>,
}

impl ResumeAttempt {
    pub(super) fn new(result: Option<std::sync::mpsc::Sender<Result<(), String>>>) -> Self {
        Self { result }
    }

    pub(super) fn accepted(&mut self) {
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

pub(crate) fn reconcile_interrupted_runs(journal: &mut RunJournal) {
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

pub(crate) const PROMPT_SERVICE: &str = "ai.muniment.desktop.chat";
const PROMPT_USER: &str = "protected-prompts";

pub(crate) fn prompt_user(subject: Option<&str>, run_id: &str) -> String {
    subject.filter(|value| !value.is_empty()).map_or_else(
        || format!("{PROMPT_USER}:{run_id}"),
        |value| format!("{PROMPT_USER}:{value}:{run_id}"),
    )
}

fn protect_prompt(run_id: &str, prompt: &str, subject: Option<&str>) -> Result<(), String> {
    keyring::Entry::new(PROMPT_SERVICE, &prompt_user(subject, run_id))
        .and_then(|entry| entry.set_password(prompt))
        .map_err(|_| "Conversation history is unavailable.".to_string())
}

pub(crate) fn state_session_root(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("pi-sessions"))
        .map_err(|_| "Conversation history is unavailable.".to_string())
}

pub(crate) fn resumable_context(
    events: &[EventEnvelope],
    subject: Option<&str>,
    session_root: &std::path::Path,
) -> Result<ResumeContext, String> {
    let state = reduce(events).map_err(|_| "This reply cannot be resumed.".to_string())?;
    let locator = resumable_locator(events.first(), &state, subject, session_root)?;
    Ok(ResumeContext {
        events: events.to_vec(),
        locator,
    })
}

pub(crate) fn resumable_locator(
    first_event: Option<&EventEnvelope>,
    state: &muniment_core::journal::reducer::RunState,
    subject: Option<&str>,
    session_root: &std::path::Path,
) -> Result<PiSessionLocator, String> {
    if first_event.is_none()
        || matches!(
            first_event.and_then(|event| event.provenance.actor_id.as_deref()),
            Some(owner) if Some(owner) != subject
        )
    {
        return Err("This reply cannot be resumed.".into());
    }
    if !matches!(&state.status, RunStatus::NeedsAttention(_))
        || state.pending_permission.is_some()
        || !state.running_effects.is_empty()
    {
        return Err("This reply cannot be resumed.".into());
    }
    let binding = state
        .pi_session
        .as_ref()
        .ok_or_else(|| "This reply cannot be resumed.".to_string())?;
    let (locator, _) = validate_pi_session(session_root, &binding.locator)
        .map_err(|_| "This reply cannot be resumed.".to_string())?;
    Ok(locator)
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
                thread_id: None,
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

pub(super) fn prepared_pi_prompt<'a>(
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
pub(crate) struct SessionThreadStart<'a> {
    pub(crate) tracker: &'a SessionThread,
    pub(crate) continue_existing: bool,
}

pub(crate) fn prepare_new_run_with_session_thread(
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
        None,
    )
}

fn prepare_new_run_in_thread(
    storage: &SharedStorage,
    run_id: &str,
    workspace: &str,
    subject: Option<&str>,
    files: Vec<SelectedFile>,
    provenance: Option<Provenance>,
    thread_id: &str,
) -> Result<(u64, ChatProjector), String> {
    let files = open_selected_files(files)?;
    prepare_opened_run(
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
        Some(thread_id),
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
    requested_thread_id: Option<&str>,
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
        let thread_id = if let Some(thread_id) = requested_thread_id {
            journal
                .append_new_run_in_thread(workspace, thread_id, &started)
                .map(|()| thread_id.to_owned())
        } else if session_thread.continue_existing {
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
        .map_err(|error| {
            if requested_thread_id.is_some() && matches!(error, JournalError::InvalidEnvelope(_)) {
                "thread_not_found".to_owned()
            } else {
                attachment_error()
            }
        })?;
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
pub(crate) fn prepare_new_run(
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
pub(super) enum PreparedPromptError {
    Start,
    SessionRoot,
    Binding,
    Journal,
}

pub(super) fn coordinate_prepared_prompt<R: tauri::Runtime, T>(
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

pub(crate) fn chat_tool_activity(
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

pub(super) fn fetch_receipt(url: &str, access_token: &str, run_id: &str) -> Result<Receipt, ()> {
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
    use crate::test_support::{append_test_event, FakeRunStartBoundaries};
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
                thread_id: None,
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
                thread_id: None,
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
        tracker.fresh(Some("owner"));
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
            assert!(prompt.get("classification").is_none());
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
