#[cfg(any(unix, target_os = "windows"))]
use super::resume::attach_permission_answer;
use super::*;

#[cfg(any(unix, target_os = "windows"))]
pub(super) trait RunCommandClient {
    fn run_submit(
        &self,
        text: &str,
        files: &[String],
        thread_id: Option<&str>,
    ) -> Result<RunSubmitAccepted, String>;
    fn run_resume(&self, run_id: &str) -> Result<RunResumeAccepted, ClientError>;
    fn run_steer(&self, run_id: &str, text: &str) -> Result<RunMessageAccepted, ClientError>;
    fn run_follow_up(&self, run_id: &str, text: &str) -> Result<RunMessageAccepted, ClientError>;
    fn run_cancel(&self, run_id: &str) -> Result<RunCancelAccepted, ClientError>;
    fn run_permission_answer(
        &self,
        run_id: &str,
        gate_id: &str,
        answer: AttachChatPermissionAnswer,
    ) -> Result<RunPermissionAnswerAccepted, ClientError>;
}

#[cfg(any(unix, target_os = "windows"))]
impl RunCommandClient for DesktopClientHolder {
    fn run_submit(
        &self,
        text: &str,
        files: &[String],
        thread_id: Option<&str>,
    ) -> Result<RunSubmitAccepted, String> {
        self.run_submit_with_reason(
            text,
            files,
            thread_id,
            crate::attach_service::runtime_version_compatible,
        )
        .map_err(|(error, response)| auth::desktop_request_error(error, response.as_ref()))
    }

    fn run_resume(&self, run_id: &str) -> Result<RunResumeAccepted, ClientError> {
        self.run_resume_if_compatible(run_id, crate::attach_service::runtime_version_compatible)
    }

    fn run_steer(&self, run_id: &str, text: &str) -> Result<RunMessageAccepted, ClientError> {
        self.run_steer_if_compatible(
            run_id,
            text,
            crate::attach_service::runtime_version_compatible,
        )
    }

    fn run_follow_up(&self, run_id: &str, text: &str) -> Result<RunMessageAccepted, ClientError> {
        self.run_follow_up_if_compatible(
            run_id,
            text,
            crate::attach_service::runtime_version_compatible,
        )
    }

    fn run_cancel(&self, run_id: &str) -> Result<RunCancelAccepted, ClientError> {
        DesktopClientHolder::run_cancel(self, run_id)
    }

    fn run_permission_answer(
        &self,
        run_id: &str,
        gate_id: &str,
        answer: AttachChatPermissionAnswer,
    ) -> Result<RunPermissionAnswerAccepted, ClientError> {
        DesktopClientHolder::run_permission_answer(self, run_id, gate_id, answer)
    }
}

#[cfg(any(unix, target_os = "windows"))]
pub(super) enum RunCommandSession<C> {
    NoSupervisor,
    Connected(C),
    Disconnected,
}

#[cfg(any(unix, target_os = "windows"))]
impl From<DesktopClientSession> for RunCommandSession<DesktopClientHolder> {
    fn from(session: DesktopClientSession) -> Self {
        match session {
            DesktopClientSession::NoSupervisor => Self::NoSupervisor,
            DesktopClientSession::Connected(client) => Self::Connected(client),
            DesktopClientSession::Disconnected => Self::Disconnected,
        }
    }
}

#[cfg(any(unix, target_os = "windows"))]
pub(super) async fn handle_run_submit<C, L, F>(
    session: RunCommandSession<C>,
    state: &ChatState,
    subject: Option<&str>,
    prompt: &str,
    files: Vec<SelectedFile>,
    local: L,
) -> Result<SubmitResult, String>
where
    C: RunCommandClient,
    L: FnOnce(Vec<SelectedFile>) -> F,
    F: std::future::Future<Output = Result<SubmitResult, String>>,
{
    match session {
        RunCommandSession::NoSupervisor => {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            return Err(auth::background_service_error());
            #[cfg(target_os = "linux")]
            local(files).await
        }
        RunCommandSession::Connected(client) => {
            let thread_id = state.session_thread.current(subject);
            let file_paths = files
                .iter()
                .map(|file| {
                    file.path
                        .to_str()
                        .map(str::to_owned)
                        .ok_or_else(attachment_error)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let accepted = client.run_submit(prompt, &file_paths, thread_id.as_deref())?;
            state
                .session_thread
                .select(accepted.thread_id.clone(), subject);
            Ok(submit_result(accepted))
        }
        RunCommandSession::Disconnected => Err(auth::background_service_error()),
    }
}

#[cfg(any(unix, target_os = "windows"))]
pub(super) async fn handle_run_resume<C, L, F>(
    session: RunCommandSession<C>,
    _state: &ChatState,
    run_id: &str,
    local: L,
) -> Result<SubmitResult, String>
where
    C: RunCommandClient,
    L: FnOnce() -> F,
    F: std::future::Future<Output = Result<SubmitResult, String>>,
{
    match session {
        RunCommandSession::NoSupervisor => {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            return Err(auth::background_service_error());
            #[cfg(target_os = "linux")]
            local().await
        }
        RunCommandSession::Connected(client) => client
            .run_resume(run_id)
            .map(resume_result)
            .map_err(auth::desktop_client_error),
        RunCommandSession::Disconnected => Err(auth::background_service_error()),
    }
}

#[cfg(any(unix, target_os = "windows"))]
pub(super) fn submit_result(accepted: RunSubmitAccepted) -> SubmitResult {
    SubmitResult {
        run_id: accepted.run_id,
        attachments: accepted
            .attachments
            .into_iter()
            .map(|attachment| ChatAttachment {
                display_name: attachment.display_name,
                byte_length: attachment.byte_length,
                media_type: attachment.media_type,
            })
            .collect(),
        committed_seq: accepted.committed_seq,
        accepted_at: accepted.accepted_at,
    }
}

#[cfg(any(unix, target_os = "windows"))]
pub(super) fn resume_result(accepted: RunResumeAccepted) -> SubmitResult {
    SubmitResult {
        run_id: accepted.run_id,
        attachments: Vec::new(),
        committed_seq: accepted.committed_seq,
        accepted_at: accepted.accepted_at,
    }
}

#[cfg(any(unix, target_os = "windows"))]
pub(super) fn handle_run_queue<C: RunCommandClient, L: FnOnce() -> Result<(), String>>(
    session: RunCommandSession<C>,
    run_id: &str,
    delivery: ChatDelivery,
    message: &str,
    local: L,
) -> Result<(), String> {
    match session {
        RunCommandSession::NoSupervisor => {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            return Err(auth::background_service_error());
            #[cfg(target_os = "linux")]
            local()
        }
        RunCommandSession::Connected(client) => match delivery {
            ChatDelivery::Steer => client.run_steer(run_id, message),
            ChatDelivery::FollowUp => client.run_follow_up(run_id, message),
        }
        .map(|_| ())
        .map_err(auth::desktop_client_error),
        RunCommandSession::Disconnected => Err(auth::background_service_error()),
    }
}

#[cfg(any(unix, target_os = "windows"))]
pub(super) fn handle_run_cancel<C: RunCommandClient, L: FnOnce() -> Result<(), String>>(
    session: RunCommandSession<C>,
    run_id: &str,
    local: L,
) -> Result<(), String> {
    match session {
        RunCommandSession::NoSupervisor => {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            return Err(auth::background_service_error());
            #[cfg(target_os = "linux")]
            local()
        }
        RunCommandSession::Connected(client) => client
            .run_cancel(run_id)
            .map(|_| ())
            .map_err(auth::desktop_client_error),
        RunCommandSession::Disconnected => Err(auth::background_service_error()),
    }
}

#[cfg(any(unix, target_os = "windows"))]
pub(super) fn handle_run_permission_answer<
    C: RunCommandClient,
    L: FnOnce() -> Result<(), String>,
>(
    session: RunCommandSession<C>,
    run_id: &str,
    gate_id: &str,
    answer: AttachChatPermissionAnswer,
    local: L,
) -> Result<(), String> {
    match session {
        RunCommandSession::NoSupervisor => {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            return Err(auth::background_service_error());
            #[cfg(target_os = "linux")]
            local()
        }
        RunCommandSession::Connected(client) => client
            .run_permission_answer(run_id, gate_id, answer)
            .map(|_| ())
            .map_err(auth::desktop_client_error),
        RunCommandSession::Disconnected => Err(auth::background_service_error()),
    }
}

#[tauri::command]
pub async fn chat_queue(
    app: tauri::AppHandle,
    state: tauri::State<'_, ChatState>,
    run_id: String,
    delivery: ChatDelivery,
    message: String,
) -> Result<(), String> {
    #[cfg(any(unix, target_os = "windows"))]
    let local_run_id = run_id.clone();
    #[cfg(any(unix, target_os = "windows"))]
    let local_message = message.clone();
    #[cfg(any(unix, target_os = "windows"))]
    return handle_run_queue(
        app.state::<crate::attach_service::AttachCompanionState>()
            .desktop_client_session()
            .into(),
        &run_id,
        delivery,
        &message,
        || local_chat_queue(&state, local_run_id, delivery, local_message),
    );
}

pub(super) fn local_chat_queue(
    state: &ChatState,
    run_id: String,
    delivery: ChatDelivery,
    message: String,
) -> Result<(), String> {
    queue_message(
        &state.active,
        ChatQueueRequest {
            run_id,
            workspace: None,
            delivery,
            message,
        },
    )
}

#[tauri::command]
pub async fn chat_cancel(
    app: tauri::AppHandle,
    state: tauri::State<'_, ChatState>,
    run_id: String,
) -> Result<(), String> {
    #[cfg(any(unix, target_os = "windows"))]
    return handle_run_cancel(
        app.state::<crate::attach_service::AttachCompanionState>()
            .desktop_client_session()
            .into(),
        &run_id,
        || cancel_active_run(&state.active, &run_id, None),
    );
}

#[tauri::command]
pub async fn chat_answer_permission(
    app: tauri::AppHandle,
    state: tauri::State<'_, ChatState>,
    run_id: String,
    gate_id: String,
    answer: ChatPermissionAnswer,
) -> Result<(), String> {
    #[cfg(any(unix, target_os = "windows"))]
    let local_run_id = run_id.clone();
    #[cfg(any(unix, target_os = "windows"))]
    let local_gate_id = gate_id.clone();
    #[cfg(any(unix, target_os = "windows"))]
    return handle_run_permission_answer(
        app.state::<crate::attach_service::AttachCompanionState>()
            .desktop_client_session()
            .into(),
        &run_id,
        &gate_id,
        attach_permission_answer(answer.clone()),
        || queue_permission_answer(&state.active, local_run_id, local_gate_id, answer),
    );
}
