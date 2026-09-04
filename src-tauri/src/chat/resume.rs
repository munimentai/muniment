#[cfg(any(unix, target_os = "windows"))]
use super::commands::{handle_run_resume, handle_run_submit, RunCommandSession};
use super::run_preparation::{fetch_grant, fetch_grant_error_message, validate_grant};
use super::*;

#[cfg(test)]
pub(super) const RESUME_PROMPT: &str =
    "Continue the interrupted response from the existing session. Do not repeat completed work.";

pub(super) fn protect_prompt(
    run_id: &str,
    prompt: &str,
    subject: Option<&str>,
) -> Result<(), String> {
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

pub(super) fn chat_resume_error_message(error: ChatResumeError) -> String {
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

#[cfg(any(unix, target_os = "windows"))]
pub(super) fn attach_permission_answer(answer: ChatPermissionAnswer) -> AttachChatPermissionAnswer {
    match answer {
        ChatPermissionAnswer::Select(value) => AttachChatPermissionAnswer::Select(value),
        ChatPermissionAnswer::Confirm(value) => AttachChatPermissionAnswer::Confirm(value),
        ChatPermissionAnswer::Input(value) => AttachChatPermissionAnswer::Input(value),
        ChatPermissionAnswer::Editor(value) => AttachChatPermissionAnswer::Editor(value),
        ChatPermissionAnswer::Cancelled => AttachChatPermissionAnswer::Cancelled,
        ChatPermissionAnswer::CodeDiff {
            gate_id,
            effect_id,
            code_diff_id,
            diff_sha256,
            write_plan_sha256,
        } => AttachChatPermissionAnswer::CodeDiff {
            gate_id,
            effect_id,
            code_diff_id,
            diff_sha256,
            write_plan_sha256,
        },
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
    #[cfg(any(unix, target_os = "windows"))]
    {
        let session: RunCommandSession<_> = app
            .state::<crate::attach_service::AttachCompanionState>()
            .desktop_client_session()
            .into();
        let subject = if matches!(session, RunCommandSession::Connected(_))
            && !crate::local_mode::is_active(&app)?
        {
            let tokens = auth::fresh_tokens_async(&auth_state, &app).await?;
            tokens.subject
        } else {
            None
        };
        let selected_files = files.unwrap_or_default();
        #[cfg(target_os = "linux")]
        let local_prompt = prompt.clone();
        return handle_run_submit(
            session,
            &state,
            subject.as_deref(),
            &prompt,
            selected_files,
            #[cfg(target_os = "linux")]
            |local_files| local_chat_submit(app, local_prompt, local_files),
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            |_| async { Err(auth::background_service_error()) },
        )
        .await;
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(super) async fn local_chat_submit(
    app: tauri::AppHandle,
    prompt: String,
    files: Vec<SelectedFile>,
) -> Result<SubmitResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        start_desktop_run(
            &TauriRunStartBoundaries {
                app,
                continue_session_thread: true,
            },
            RunStartRequest {
                prompt,
                files,
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
    #[cfg(any(unix, target_os = "windows"))]
    {
        let session = app
            .state::<crate::attach_service::AttachCompanionState>()
            .desktop_client_session()
            .into();
        #[cfg(target_os = "linux")]
        let local_state = state.clone();
        #[cfg(target_os = "linux")]
        let local_run_id = run_id.clone();
        return handle_run_resume(session, &state, &run_id, || {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            return async { Err(auth::background_service_error()) };
            #[cfg(target_os = "linux")]
            local_chat_resume(app, auth_state, local_state, local_run_id)
        })
        .await;
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(super) async fn local_chat_resume(
    app: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
    run_id: String,
) -> Result<SubmitResult, String> {
    let local_mode = crate::local_mode::is_active(&app)?;
    let tokens = if local_mode {
        TokenSet {
            access_token: String::new(),
            refresh_token: None,
            expires_at: None,
            subject: None,
        }
    } else {
        auth::fresh_tokens_async(&auth_state, &app).await?
    };
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
    let grant = if local_mode {
        ChatGrant::local()
    } else {
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
        grant
    };
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
    let workspace = desktop_pi_workspace(&grant);
    let launch = ResumeLaunch {
        sink: TauriChatEventSink::new(app, Arc::clone(&memory_runtime), workspace),
        storage: Arc::clone(state.storage()?),
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
