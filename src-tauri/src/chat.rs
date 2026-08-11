use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{SecondsFormat, Utc};
use muniment_core::active_run::{
    cancel_active_run, queue_message, queue_permission_answer, ChatDelivery, ChatQueueRequest,
};
#[cfg(target_os = "linux")]
use muniment_core::attach::linux::{
    ThreadListPage, ThreadListRequest, ThreadListService, ThreadOpenPage, ThreadOpenRequest,
};
#[cfg(target_os = "linux")]
use muniment_core::attach::ProtocolError;
use muniment_core::attach::{RuntimeActivityGuard, RuntimeActivityRegistry};
use muniment_core::attachment::{ingest_attachment, AttachmentDeliveryError, AttachmentMetadata};
use muniment_core::auth::TokenSet;
use muniment_core::cas::LocalCas;
use muniment_core::chat_coordinate::coordinate;
use muniment_core::chat_grant::{
    fetch_grant as core_fetch_grant, validate_grant as core_validate_grant, ChatGrant,
    FetchGrantError,
};
use muniment_core::chat_profile::ChatProfile;
pub(crate) use muniment_core::chat_resume::ResumeContext;
use muniment_core::chat_resume::{
    clear_active_run, install_active_run, install_resume_run,
    resumable_context as core_resumable_context, run_resume, ChatResumeError, ResumeLaunch,
};
use muniment_core::chat_view::{chat_attachments, ChatAttachment, SelectedFile};
use muniment_core::journal::reconciliation::reconcile_interrupted_runs;
use muniment_core::journal::reducer::{project_chat, ChatProjector};
use muniment_core::journal::{
    EventEnvelope, EventPayload, JournalCommitHint, JournalError, Provenance, RunJournal,
};
use muniment_core::memory_index::ModelMemoryCapability;
use muniment_core::permission_gate::{ChatPermissionAnswer, PendingPermissionAnswer};
pub(crate) use muniment_core::pi_execution::{
    attachment_delivery_error, attachment_error, PiRuntime,
};
use muniment_core::pi_launch::{PiLaunchBoundaries, PiLaunchError};
use muniment_core::run_events::{ChatEvent, ChatEventSink};
pub(crate) use muniment_core::run_events::{ChatStorage, SharedStorage};
use muniment_core::run_start::{
    start_desktop_run, ActiveRun, RunStartBoundaries, RunStartError, RunStartLaunch,
    RunStartRequest, SubmitResult,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use tauri::{Emitter, Manager};
use uuid::Uuid;

use crate::auth;
use crate::chat_threads::newest_owned_workspace_thread;
use muniment_core::session_thread::{OfferedThread, SessionThread};

pub(crate) struct TauriChatEventSink<R: tauri::Runtime> {
    app: tauri::AppHandle<R>,
    memory_runtime: Arc<crate::memory::ApplicationMemoryRuntime>,
}

impl<R: tauri::Runtime> TauriChatEventSink<R> {
    fn new(
        app: tauri::AppHandle<R>,
        memory_runtime: Arc<crate::memory::ApplicationMemoryRuntime>,
    ) -> Self {
        Self {
            app,
            memory_runtime,
        }
    }
}

impl<R: tauri::Runtime> ChatEventSink for TauriChatEventSink<R> {
    fn deliver(&self, event: ChatEvent) -> Result<(), ()> {
        self.app.emit("chat-event", event).map_err(|_| ())
    }
}

impl<R: tauri::Runtime> PiLaunchBoundaries for TauriChatEventSink<R> {
    fn pi_session_root(&self) -> Result<PathBuf, PiLaunchError> {
        self.app
            .path()
            .app_data_dir()
            .map(|path| ChatProfile::new(path).pi_session_root())
            .map_err(|_| PiLaunchError::UnavailableSessionRoot)
    }

    fn memory_agent_extension_path(&self) -> Option<PathBuf> {
        Some(self.memory_runtime.agent_extension_path())
    }
}

pub struct ChatState {
    pub(crate) storage: SharedStorage,
    active: Arc<Mutex<Option<ActiveRun>>>,
    runtime: Arc<Mutex<Option<PiRuntime>>>,
    pub(crate) session_thread: SessionThread,
    pub(crate) runtime_activity: RuntimeActivityRegistry,
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
    fn mark_active_run(&self) -> RuntimeActivityGuard {
        self.state().runtime_activity.mark_active_run()
    }

    #[cfg(target_os = "linux")]
    fn control_migration(
        &self,
        request: muniment_core::attach::linux::MigrationControlRequest,
        peer_pid: u32,
    ) -> Result<(), ProtocolError> {
        crate::attach_service::control_desktop_migration(&self.app, request, peer_pid)
    }

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

    #[cfg(target_os = "linux")]
    fn create_thread(
        &self,
        workspace: &str,
        provenance: Provenance,
    ) -> Result<String, ProtocolError> {
        self.state()
            .storage
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?
            .journal
            .create_thread(
                workspace,
                &Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
                provenance,
            )
            .map_err(|_| ProtocolError::persistence_failed())
    }

    #[cfg(target_os = "linux")]
    fn stream_run(
        &self,
        workspace: &str,
        run_id: &str,
        after_run_seq: u64,
    ) -> Result<muniment_core::attach::linux::RunStreamPage, ProtocolError> {
        let state = self.state();
        let mut storage = state
            .storage
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        ThreadListService::stream_run(&mut storage.journal, workspace, run_id, after_run_seq)
    }

    #[cfg(target_os = "linux")]
    fn subscribe_run_commits(
        &self,
        run_id: &str,
    ) -> Result<(u64, std::sync::mpsc::Receiver<JournalCommitHint>), ProtocolError> {
        self.state()
            .storage
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?
            .journal
            .subscribe_commits(run_id)
            .map_err(|_| ProtocolError::persistence_failed())
    }

    #[cfg(target_os = "linux")]
    fn queue_attach_permission_answer(
        &self,
        workspace: &str,
        run_id: &str,
        gate_id: &str,
        answer: ChatPermissionAnswer,
    ) -> Result<std::sync::mpsc::Receiver<Option<u64>>, RunStartError> {
        let state = self.state();
        let active = state
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let run = active
            .as_ref()
            .filter(|run| run.id == run_id && run.workspace == workspace)
            .ok_or_else(|| {
                RunStartError::InvalidRequest("That reply is no longer active.".into())
            })?;
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        run.permission_answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_back(PendingPermissionAnswer {
                gate_id: gate_id.to_owned(),
                answer,
                resolved: Some(sender),
            });
        Ok(receiver)
    }

    fn active_run_exists(&self) -> bool {
        self.state()
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
    }

    fn cancel_run(&self, workspace: &str, run_id: &str) -> Result<(), RunStartError> {
        cancel_active_run(&self.state().active, run_id, Some(workspace))
            .map_err(RunStartError::InvalidRequest)
    }

    fn fresh_tokens(&self) -> Result<TokenSet, RunStartError> {
        auth::fresh_tokens(&self.app.state::<auth::AuthState>(), &self.app)
            .map_err(RunStartError::Unauthorized)
    }

    fn configure_run(
        &self,
        _run_id: &str,
        _prompt: &str,
        tokens: &TokenSet,
        requested_workspace: Option<&str>,
    ) -> Result<ChatGrant, RunStartError> {
        let grant = fetch_grant(&tokens.access_token).map_err(map_fetch_grant_error)?;
        validate_grant(&grant).map_err(RunStartError::Persistence)?;
        self.app
            .state::<crate::attach_service::AttachCompanionState>()
            .record_workspace(grant.workspace.clone());
        if requested_workspace.is_some_and(|workspace| workspace != grant.workspace) {
            return Err(RunStartError::Unauthorized(
                "The capability is not authorized.".into(),
            ));
        }
        Ok(grant)
    }

    #[cfg(target_os = "linux")]
    fn attach_approval(&self) -> Option<muniment_core::attach::Approval> {
        self.app
            .state::<crate::attach_service::AttachCompanionState>()
            .approval()
    }

    fn install_active_run(&self, run: ActiveRun) -> Result<(), RunStartError> {
        install_active_run(&self.state().active, run).map_err(RunStartError::InvalidRequest)
    }

    fn prepare_run(
        &self,
        run_id: &str,
        prompt: &str,
        grant: &ChatGrant,
        tokens: &TokenSet,
        files: Vec<SelectedFile>,
        provenance: Option<Provenance>,
        thread_id: Option<&str>,
    ) -> Result<(u64, ChatProjector), RunStartError> {
        let state = self.state();
        let result = match thread_id {
            Some(thread_id) => prepare_new_run_in_thread_after_validation(
                &state.storage,
                run_id,
                &grant.workspace,
                tokens.subject.as_deref(),
                files,
                provenance,
                thread_id,
                || protect_prompt(run_id, prompt, tokens.subject.as_deref()),
            ),
            None => prepare_new_run_with_session_thread_after_validation(
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
                || protect_prompt(run_id, prompt, tokens.subject.as_deref()),
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

    fn open_memory_session(
        &self,
        run_id: &str,
        thread_id: &str,
        minimum_cacheable_prefix_characters: usize,
    ) -> Result<(), RunStartError> {
        self.app
            .state::<Arc<crate::memory::ApplicationMemoryRuntime>>()
            .open_session(
                run_id,
                thread_id,
                ModelMemoryCapability {
                    minimum_cacheable_prefix_characters,
                },
            )
            .map_err(|_| RunStartError::Persistence(attachment_error()))
    }

    fn close_memory_session(&self, run_id: &str) {
        self.app
            .state::<Arc<crate::memory::ApplicationMemoryRuntime>>()
            .close_session(run_id);
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
        let runtime_activity = state.runtime_activity.clone();
        let memory_runtime = Arc::clone(
            self.app
                .state::<Arc<crate::memory::ApplicationMemoryRuntime>>()
                .inner(),
        );
        tauri::async_runtime::spawn_blocking(move || {
            let sink = TauriChatEventSink::new(app.clone(), Arc::clone(&memory_runtime));
            coordinate(
                sink,
                storage,
                runtime,
                runtime_activity,
                memory_runtime,
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
                app.state::<Arc<crate::memory::ApplicationMemoryRuntime>>()
                    .close_session(&launch.run_id);
                clear_active_run(&state.active, &launch.run_id);
            }
        });
    }
}

impl ChatState {
    pub fn new(
        app: &tauri::AppHandle,
        runtime_activity: RuntimeActivityRegistry,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let directory = app.path().app_data_dir()?;
        let profile = ChatProfile::new(directory);
        let (mut journal, cas) = profile.open_storage()?;
        reconcile_interrupted_runs(&mut journal, &desktop_provenance(None));
        Ok(Self {
            storage: Arc::new(Mutex::new(ChatStorage { journal, cas })),
            active: Arc::new(Mutex::new(None)),
            runtime: Arc::new(Mutex::new(None)),
            session_thread: SessionThread::default(),
            runtime_activity,
        })
    }
}

#[cfg(test)]
const RESUME_PROMPT: &str =
    "Continue the interrupted response from the existing session. Do not repeat completed work.";

fn protect_prompt(run_id: &str, prompt: &str, subject: Option<&str>) -> Result<(), String> {
    muniment_core::chat_prompt::store_prompt(run_id, prompt, subject)
        .map_err(|_| "Conversation history is unavailable.".to_string())
}

pub(crate) fn state_session_root(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| ChatProfile::new(path).pi_session_root())
        .map_err(|_| "Conversation history is unavailable.".to_string())
}

pub(crate) fn resumable_context(
    events: &[EventEnvelope],
    subject: Option<&str>,
    session_root: &std::path::Path,
) -> Result<ResumeContext, String> {
    core_resumable_context(events, subject, session_root).map_err(chat_resume_error_message)
}

fn chat_resume_error_message(error: ChatResumeError) -> String {
    match error {
        ChatResumeError::InvalidEvents
        | ChatResumeError::MissingFirstEvent
        | ChatResumeError::SubjectMismatch
        | ChatResumeError::StatusNotResumable
        | ChatResumeError::PendingPermission
        | ChatResumeError::RunningEffect
        | ChatResumeError::MissingPiSession
        | ChatResumeError::InvalidPiSession => "This reply cannot be resumed.".into(),
    }
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
    let (resume, thread_id) = {
        let mut storage = state
            .storage
            .lock()
            .map_err(|_| "This reply cannot be resumed.".to_string())?;
        let events = storage
            .journal
            .events(&run_id)
            .map_err(|_| "This reply cannot be resumed.".to_string())?;
        let thread_id = storage
            .journal
            .run_thread_id(&run_id)
            .map_err(|_| "This reply cannot be resumed.".to_string())?
            .ok_or_else(|| "This reply cannot be resumed.".to_string())?;
        (
            resumable_context(&events, tokens.subject.as_deref(), &session_root)?,
            thread_id,
        )
    };
    let attachments = chat_attachments(
        &project_chat(&resume.events)
            .map_err(|_| "This reply could not be resumed.".to_string())?
            .attachments,
    );
    let access_token = tokens.access_token.clone();
    let grant = tauri::async_runtime::spawn_blocking(move || {
        let grant = fetch_grant(&access_token).map_err(fetch_grant_error_message)?;
        validate_grant(&grant)?;
        Ok::<_, String>(grant)
    })
    .await
    .map_err(|_| "Chat configuration is temporarily unavailable.".to_string())??;
    app.state::<crate::attach_service::AttachCompanionState>()
        .record_workspace(grant.workspace.clone());
    let cancelled = Arc::new(AtomicBool::new(false));
    let transport = Arc::new(Mutex::new(None));
    let adapter = Arc::new(Mutex::new(None));
    let permission_answers = Arc::new(Mutex::new(VecDeque::new()));
    // A resumed run owns its own memory session. Open it before the coordinator
    // starts so the declared memory-search tool can answer every call.
    install_resume_run(
        app.state::<Arc<crate::memory::ApplicationMemoryRuntime>>()
            .inner(),
        &state.active,
        ActiveRun {
            id: run_id.clone(),
            workspace: grant.workspace.clone(),
            cancelled: Arc::clone(&cancelled),
            transport: Arc::clone(&transport),
            adapter: Arc::clone(&adapter),
            permission_answers: Arc::clone(&permission_answers),
            _activity: state.runtime_activity.mark_active_run(),
        },
        &thread_id,
        grant.minimum_cacheable_prefix_characters,
    )?;
    let result_id = run_id.clone();
    let (attempt_sender, attempt_receiver) = std::sync::mpsc::channel();
    let runtime_activity = state.runtime_activity.clone();
    let memory_runtime = Arc::clone(
        app.state::<Arc<crate::memory::ApplicationMemoryRuntime>>()
            .inner(),
    );
    let launch = ResumeLaunch {
        sink: TauriChatEventSink::new(app, Arc::clone(&memory_runtime)),
        storage: Arc::clone(&state.storage),
        runtime: Arc::clone(&state.runtime),
        runtime_activity,
        memory_runtime,
        active: Arc::clone(&state.active),
        run_id,
        tokens,
        grant,
        cancelled,
        transport,
        adapter,
        permission_answers,
        resume,
        attempt: attempt_sender,
    };
    tauri::async_runtime::spawn_blocking(move || run_resume(launch));
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
    prepare_new_run_with_session_thread_after_validation(
        storage,
        session_thread,
        run_id,
        workspace,
        subject,
        files,
        provenance,
        || Ok(()),
    )
}

fn prepare_new_run_with_session_thread_after_validation<F>(
    storage: &SharedStorage,
    session_thread: SessionThreadStart<'_>,
    run_id: &str,
    workspace: &str,
    subject: Option<&str>,
    files: Vec<SelectedFile>,
    provenance: Option<Provenance>,
    after_validation: F,
) -> Result<(u64, ChatProjector), String>
where
    F: FnOnce() -> Result<(), String>,
{
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
        after_validation,
    )
}

fn prepare_new_run_in_thread_after_validation<F>(
    storage: &SharedStorage,
    run_id: &str,
    workspace: &str,
    subject: Option<&str>,
    files: Vec<SelectedFile>,
    provenance: Option<Provenance>,
    thread_id: &str,
    after_validation: F,
) -> Result<(u64, ChatProjector), String>
where
    F: FnOnce() -> Result<(), String>,
{
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
        after_validation,
    )
}

fn prepare_opened_run<F>(
    storage: &SharedStorage,
    session_thread: SessionThreadStart<'_>,
    run_id: &str,
    workspace: &str,
    subject: Option<&str>,
    files: Vec<OpenSelectedFile>,
    provenance: Option<Provenance>,
    requested_thread_id: Option<&str>,
    after_validation: F,
) -> Result<(u64, ChatProjector), String>
where
    F: FnOnce() -> Result<(), String>,
{
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
    let mut after_validation = Some(after_validation);
    if !workspace.is_empty() {
        let thread_id = if let Some(thread_id) = requested_thread_id {
            journal
                .append_new_run_in_thread_after_validation(workspace, thread_id, &started, || {
                    after_validation.take().unwrap()()
                })
                .and_then(|result| result.map_err(|_| JournalError::Corrupt(attachment_error())))
                .map(|()| thread_id.to_owned())
        } else {
            after_validation.take().unwrap()()?;
            if session_thread.continue_existing {
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
        after_validation.take().unwrap()()?;
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

#[tauri::command]
pub async fn chat_cancel(state: tauri::State<'_, ChatState>, run_id: String) -> Result<(), String> {
    cancel_active_run(&state.active, &run_id, None)
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
        provenance: desktop_provenance(subject),
        extra: BTreeMap::new(),
    }
}

pub(crate) fn desktop_provenance(subject: Option<&str>) -> Provenance {
    Provenance {
        source: "muniment-desktop".into(),
        source_version: env!("CARGO_PKG_VERSION").into(),
        actor_id: subject.map(str::to_owned),
        device_id: None,
        rpc_request_id: None,
        capability_versions: None,
        extra: BTreeMap::new(),
    }
}

fn fetch_grant_error_message(error: FetchGrantError) -> String {
    match error {
        FetchGrantError::Unauthorized => "The capability is not authorized.".into(),
        FetchGrantError::Unavailable => "Chat configuration is temporarily unavailable.".into(),
        FetchGrantError::InvalidResponse => "The chat configuration response was invalid.".into(),
    }
}

fn map_fetch_grant_error(error: FetchGrantError) -> RunStartError {
    match error {
        FetchGrantError::Unauthorized => {
            RunStartError::Unauthorized("The capability is not authorized.".into())
        }
        FetchGrantError::Unavailable => {
            RunStartError::Persistence("Chat configuration is temporarily unavailable.".into())
        }
        FetchGrantError::InvalidResponse => {
            RunStartError::Persistence("The chat configuration response was invalid.".into())
        }
    }
}

fn fetch_grant(access_token: &str) -> Result<ChatGrant, FetchGrantError> {
    let issuer =
        std::env::var("MUNIMENT_ISSUER").unwrap_or_else(|_| "https://api.muniment.ai".into());
    core_fetch_grant(&issuer, access_token)
}

fn validate_grant(grant: &ChatGrant) -> Result<(), String> {
    core_validate_grant(grant).map_err(fetch_grant_error_message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::append_test_event;
    use base64::{engine::general_purpose::STANDARD, Engine};
    use muniment_core::journal::reducer::reduce;
    use muniment_core::sidecar::validate_pi_session;

    static PI_ENV_LOCK: Mutex<()> = Mutex::new(());

    struct FakeCoordinateSink {
        session_root: PathBuf,
    }

    impl FakeCoordinateSink {
        fn new(app_data_dir: &std::path::Path) -> Self {
            Self {
                session_root: ChatProfile::new(app_data_dir.to_owned()).pi_session_root(),
            }
        }
    }

    impl ChatEventSink for FakeCoordinateSink {
        fn deliver(&self, _event: ChatEvent) -> Result<(), ()> {
            Ok(())
        }
    }

    impl PiLaunchBoundaries for FakeCoordinateSink {
        fn pi_session_root(&self) -> Result<PathBuf, PiLaunchError> {
            Ok(self.session_root.clone())
        }

        fn memory_agent_extension_path(&self) -> Option<PathBuf> {
            None
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn grant_errors_are_mapped_and_redacted() {
        let error = map_fetch_grant_error(FetchGrantError::Unauthorized).protocol_error();
        let encoded = serde_json::to_string(&error).unwrap();
        assert!(encoded.contains("unauthorized"), "{encoded}");
        for secret in ["private-prompt", "secret-token", "/private/work", "sidecar"] {
            assert!(!encoded.contains(secret), "leaked {secret}: {encoded}");
        }

        let internal = map_fetch_grant_error(FetchGrantError::Unavailable).protocol_error();
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
            || Ok(()),
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
    fn coordinator_sends_exact_ordered_images_and_preserves_text_only_prompt_shape() {
        let _environment = lock_pi_environment();
        for with_images in [true, false] {
            let directory =
                std::env::temp_dir().join(format!("muniment-prompt-capture-{}", Uuid::now_v7()));
            std::fs::create_dir_all(&directory).unwrap();
            let sessions = ChatProfile::new(directory.clone()).pi_session_root();
            std::fs::create_dir_all(&sessions).unwrap();
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
                FakeCoordinateSink::new(&directory),
                Arc::clone(&storage),
                Arc::new(Mutex::new(None)),
                RuntimeActivityRegistry::new(),
                Arc::new(crate::memory::ApplicationMemoryRuntime::new(
                    directory.join("memory-config"),
                    directory.join("memory-cache"),
                )),
                run_id,
                "original text prompt".into(),
                "token".into(),
                Some("owner".into()),
                ChatGrant {
                    workspace: "workspace-a".into(),
                    gateway_url: "https://gateway.invalid".into(),
                    virtual_key: "virtual-key".into(),
                    model: None,
                    minimum_cacheable_prefix_characters: 8_192,
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
                FakeCoordinateSink::new(&directory),
                Arc::clone(&storage),
                Arc::new(Mutex::new(None)),
                RuntimeActivityRegistry::new(),
                Arc::new(crate::memory::ApplicationMemoryRuntime::new(
                    directory.join("memory-config"),
                    directory.join("memory-cache"),
                )),
                run_id.clone(),
                "private prompt bytes".into(),
                "token".into(),
                Some("owner".into()),
                ChatGrant {
                    workspace: "workspace-a".into(),
                    gateway_url: "https://gateway.invalid".into(),
                    virtual_key: "virtual-key".into(),
                    model: None,
                    minimum_cacheable_prefix_characters: 8_192,
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
            let expected_error = match name {
                "private-large.jpg" => {
                    attachment_delivery_error(AttachmentDeliveryError::ImageSizeLimit {
                        display_name: name.into(),
                    })
                }
                "private-malformed.jpg" => {
                    attachment_delivery_error(AttachmentDeliveryError::AmbiguousFormat {
                        display_name: name.into(),
                    })
                }
                _ => attachment_error(),
            };
            assert!(public_error.contains(&expected_error));
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
    fn event_envelope_records_the_owning_subject() {
        let owned = event_envelope("run-1", 1, "run.started", json!({}), Some("sub-a"));
        assert_eq!(owned.provenance.actor_id.as_deref(), Some("sub-a"));

        let unowned = event_envelope("run-2", 1, "run.started", json!({}), None);
        assert_eq!(unowned.provenance.actor_id, None);
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
    fn resume_validation_fails_closed_for_every_unsafe_projection() {
        let directory = std::env::temp_dir().join(format!("muniment-resume-{}", Uuid::now_v7()));
        let sessions = ChatProfile::new(&directory).pi_session_root();
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
        let sessions = ChatProfile::new(&directory).pi_session_root();
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
            FakeCoordinateSink::new(&directory),
            Arc::clone(&shared),
            Arc::new(Mutex::new(None)),
            RuntimeActivityRegistry::new(),
            Arc::new(crate::memory::ApplicationMemoryRuntime::new(
                directory.join("memory-config"),
                directory.join("memory-cache"),
            )),
            run_id.clone(),
            RESUME_PROMPT.into(),
            "token".into(),
            Some("owner".into()),
            ChatGrant {
                workspace: "workspace-a".into(),
                gateway_url: "https://gateway.invalid".into(),
                virtual_key: "virtual-key".into(),
                model: None,
                minimum_cacheable_prefix_characters: 8_192,
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
        let directory = std::env::temp_dir().join(format!("muniment-resume-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let args_log = directory.join("args.txt");
        let prompt_log = directory.join("prompt.txt");
        let request_log = directory.join("requests.jsonl");
        let session_name = format!("{}.jsonl", Uuid::now_v7());
        let sessions = ChatProfile::new(directory.clone()).pi_session_root();
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
            FakeCoordinateSink::new(&directory),
            Arc::clone(&shared),
            Arc::new(Mutex::new(None)),
            RuntimeActivityRegistry::new(),
            Arc::new(crate::memory::ApplicationMemoryRuntime::new(
                directory.join("memory-config"),
                directory.join("memory-cache"),
            )),
            run_id.clone(),
            RESUME_PROMPT.into(),
            "token".into(),
            Some("owner".into()),
            ChatGrant {
                workspace: "workspace-a".into(),
                gateway_url: "https://gateway.invalid".into(),
                virtual_key: "virtual-key".into(),
                model: None,
                minimum_cacheable_prefix_characters: 8_192,
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
    fn resumed_run_answers_a_memory_search_and_ends_without_a_session() {
        let _environment = lock_pi_environment();
        let app = tauri::test::mock_app();
        let directory =
            std::env::temp_dir().join(format!("muniment-resume-memory-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let config = directory.join("config");
        let home = directory.join("home");
        muniment_core::home::confirm_home(&config, &home).unwrap();
        std::fs::write(home.join("memory/fact.md"), "saffron belongs in the pantry").unwrap();
        app.manage(Arc::new(crate::memory::ApplicationMemoryRuntime::new(
            config,
            directory.join("cache"),
        )));

        let session_name = format!("{}.jsonl", Uuid::now_v7());
        let sessions = ChatProfile::new(app.path().app_data_dir().unwrap()).pi_session_root();
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(sessions.join(&session_name), "persisted Pi data\n").unwrap();

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
        journal
            .append_new_run(
                "workspace-a",
                &event_envelope(&run_id, 1, "run.started", json!({}), Some("owner")),
            )
            .unwrap();
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
        let thread_id = journal.run_thread_id(&run_id).unwrap().unwrap();
        let existing = journal.events(&run_id).unwrap();
        let (locator, _) = validate_pi_session(&sessions, &session_name).unwrap();
        let shared = Arc::new(Mutex::new(ChatStorage {
            journal,
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));

        let cancelled = Arc::new(AtomicBool::new(false));
        let transport = Arc::new(Mutex::new(None));
        let adapter = Arc::new(Mutex::new(None));
        let permission_answers = Arc::new(Mutex::new(VecDeque::new()));
        let runtime_activity = RuntimeActivityRegistry::new();
        let active = Arc::new(Mutex::new(None));
        install_resume_run(
            app.state::<Arc<crate::memory::ApplicationMemoryRuntime>>()
                .inner(),
            &active,
            ActiveRun {
                id: run_id.clone(),
                workspace: "workspace-a".into(),
                cancelled: Arc::clone(&cancelled),
                transport: Arc::clone(&transport),
                adapter: Arc::clone(&adapter),
                permission_answers: Arc::clone(&permission_answers),
                _activity: runtime_activity.mark_active_run(),
            },
            &thread_id,
            8_192,
        )
        .unwrap();
        std::env::set_var("MUNIMENT_PI_ROOT", &directory);
        std::env::set_var("MUNIMENT_PI_TEST_EXECUTABLE", &stub);
        std::env::set_var("PI_RESUME_STUB_MEMORY_QUERY", "saffron");
        let (sender, receiver) = std::sync::mpsc::channel();
        run_resume(ResumeLaunch {
            sink: TauriChatEventSink::new(
                app.handle().clone(),
                Arc::clone(
                    app.state::<Arc<crate::memory::ApplicationMemoryRuntime>>()
                        .inner(),
                ),
            ),
            storage: Arc::clone(&shared),
            runtime: Arc::new(Mutex::new(None)),
            runtime_activity: runtime_activity.clone(),
            memory_runtime: Arc::clone(
                app.state::<Arc<crate::memory::ApplicationMemoryRuntime>>()
                    .inner(),
            ),
            active: Arc::clone(&active),
            run_id: run_id.clone(),
            tokens: TokenSet {
                access_token: "token".into(),
                refresh_token: None,
                expires_at: None,
                subject: Some("owner".into()),
            },
            grant: ChatGrant {
                workspace: "workspace-a".into(),
                gateway_url: "https://gateway.invalid".into(),
                virtual_key: "virtual-key".into(),
                model: None,
                minimum_cacheable_prefix_characters: 8_192,
                receipt_url,
            },
            cancelled,
            transport,
            adapter,
            permission_answers,
            resume: ResumeContext {
                events: existing,
                locator,
            },
            attempt: sender,
        });
        assert_eq!(receiver.recv().unwrap(), Ok(()));
        receipt_server.join().unwrap();
        for key in [
            "MUNIMENT_PI_ROOT",
            "MUNIMENT_PI_TEST_EXECUTABLE",
            "PI_RESUME_STUB_MEMORY_QUERY",
        ] {
            std::env::remove_var(key);
        }

        let events = shared.lock().unwrap().journal.events(&run_id).unwrap();
        assert_eq!(events.last().unwrap().event_type, "run.completed");
        let recalls: Vec<_> = events
            .iter()
            .filter(|event| event.event_type == "memory.recalled")
            .collect();
        assert_eq!(recalls.len(), 1);
        let EventPayload::Inline { payload_json } = &recalls[0].payload else {
            panic!("memory recall must use an inline payload");
        };
        assert!(payload_json["files"]
            .as_array()
            .unwrap()
            .contains(&json!("memory/fact.md")));
        assert_eq!(payload_json["thread"], json!(thread_id));
        assert_eq!(payload_json["character_budget"], json!(8_192));
        assert!(app
            .state::<Arc<crate::memory::ApplicationMemoryRuntime>>()
            .dispatch_tool_call(&run_id, "memory-search", br#"{"query":"saffron"}"#)
            .is_err());

        std::fs::remove_file(sessions.join(session_name)).unwrap();
        drop(shared);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
