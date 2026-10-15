#[cfg(any(unix, target_os = "windows"))]
use super::commands::{handle_run_resume, handle_run_submit, RunCommandSession};
use super::*;

pub(crate) fn state_session_root(_app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    muniment_runtime::profile_directory()
        .map(|path| ChatProfile::new(path).pi_session_root())
        .map_err(|_| "Conversation history is unavailable.".to_string())
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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmissionError {
    source: &'static str,
    message: String,
}

impl SubmissionError {
    fn at(source: &'static str) -> impl FnOnce(String) -> Self {
        move |message| Self { source, message }
    }
}

#[tauri::command]
pub async fn chat_submit(
    app: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
    prompt: String,
    files: Option<Vec<SelectedFile>>,
    thinking_level: Option<String>,
) -> Result<SubmitResult, SubmissionError> {
    #[cfg(any(unix, target_os = "windows"))]
    {
        let session: RunCommandSession<_> = app
            .state::<crate::attach_service::AttachCompanionState>()
            .desktop_client_session()
            .into();
        let subject = if matches!(session, RunCommandSession::Connected(_))
            && !crate::local_mode::is_active(&app).map_err(SubmissionError::at("local-mode"))?
        {
            let tokens = auth::fresh_tokens_async(&auth_state, &app)
                .await
                .map_err(SubmissionError::at("auth"))?;
            tokens.subject
        } else {
            None
        };
        if let Some(thread) = state.session_thread.current(subject.as_deref()) {
            let profile = muniment_runtime::profile_directory().map_err(|_| {
                SubmissionError::at("thread")("The project catalog is unavailable.".into())
            })?;
            muniment_desktop_integration::workspace_metadata::recover_thread_metadata(
                &profile, &thread,
            )
            .map_err(SubmissionError::at("thread"))?;
        }
        crate::projects::prepare_thread(&app, &state, subject.as_deref(), None, false)
            .map_err(SubmissionError::at("thread"))?;
        let selected_files = files.unwrap_or_default();
        return handle_run_submit(
            session,
            &state,
            subject.as_deref(),
            &prompt,
            selected_files,
            thinking_level.as_deref(),
        )
        .await
        .map_err(SubmissionError::at("submit"));
    }
}

#[tauri::command]
pub async fn chat_resume(app: tauri::AppHandle, run_id: String) -> Result<SubmitResult, String> {
    #[cfg(any(unix, target_os = "windows"))]
    return handle_run_resume(
        app.state::<crate::attach_service::AttachCompanionState>()
            .desktop_client_session()
            .into(),
        &run_id,
    )
    .await;
}
