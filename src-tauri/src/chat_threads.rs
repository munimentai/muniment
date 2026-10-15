use serde::Serialize;
use tauri::Manager;

#[cfg(any(unix, target_os = "windows"))]
use crate::attach_service::{AttachCompanionState, DesktopClientSession};
use crate::auth;
use crate::chat::{state_session_root, ChatState};
#[cfg(any(unix, target_os = "windows"))]
use muniment_core::attach::ClientError;
use muniment_core::session_thread::SessionThread;

fn chat_subject(
    app: &tauri::AppHandle,
    auth_state: &auth::AuthState,
) -> Result<Option<String>, String> {
    if crate::local_mode::is_active(app)? {
        Ok(None)
    } else {
        Ok(auth::fresh_tokens(auth_state, app)?.subject)
    }
}

/// How long the first history read waits for the runtime connection before it
/// reports the service unreachable.
#[cfg(any(unix, target_os = "windows"))]
const HISTORY_CONNECT_WAIT: std::time::Duration = std::time::Duration::from_secs(3);

#[cfg(target_os = "macos")]
fn require_runtime(session: &DesktopClientSession) -> Result<(), String> {
    match session {
        DesktopClientSession::Connected(_) => Ok(()),
        DesktopClientSession::NoSupervisor | DesktopClientSession::Disconnected => {
            Err(auth::background_service_error())
        }
    }
}

#[cfg(any(unix, target_os = "windows"))]
fn rename_thread_command(
    attach_state: &AttachCompanionState,
    thread_id: &str,
    title: &str,
) -> Result<(), String> {
    match attach_state.desktop_client_session() {
        DesktopClientSession::NoSupervisor => Err(auth::background_service_error()),
        DesktopClientSession::Connected(client) => client
            .rename_thread(thread_id, title)
            .map_err(auth::desktop_client_error),
        DesktopClientSession::Disconnected => {
            Err("Muniment cannot reach its background service.".into())
        }
    }
}

#[cfg(any(unix, target_os = "windows"))]
trait ThreadDeleteClient {
    fn delete_thread(&self, thread_id: &str) -> Result<(), ClientError>;
}

#[cfg(any(unix, target_os = "windows"))]
impl ThreadDeleteClient for muniment_core::attach::DesktopClientHolder {
    fn delete_thread(&self, thread_id: &str) -> Result<(), ClientError> {
        muniment_core::attach::DesktopClientHolder::delete_thread(self, thread_id)
    }
}

#[cfg(any(unix, target_os = "windows"))]
enum ThreadDeleteSession<C> {
    NoSupervisor,
    Connected(C),
    Disconnected,
}

#[cfg(any(unix, target_os = "windows"))]
impl From<DesktopClientSession>
    for ThreadDeleteSession<muniment_core::attach::DesktopClientHolder>
{
    fn from(session: DesktopClientSession) -> Self {
        match session {
            DesktopClientSession::NoSupervisor => Self::NoSupervisor,
            DesktopClientSession::Connected(client) => Self::Connected(client),
            DesktopClientSession::Disconnected => Self::Disconnected,
        }
    }
}

#[cfg(any(unix, target_os = "windows"))]
fn delete_thread_command<C: ThreadDeleteClient>(
    session: ThreadDeleteSession<C>,
    session_thread: &SessionThread,
    subject: Option<&str>,
    thread_id: &str,
) -> Result<(), String> {
    match session {
        ThreadDeleteSession::NoSupervisor => Err(auth::background_service_error()),
        ThreadDeleteSession::Connected(client) => {
            client
                .delete_thread(thread_id)
                .map_err(auth::desktop_client_error)?;
            session_thread.fresh_if_current(thread_id, subject);
            Ok(())
        }
        ThreadDeleteSession::Disconnected => {
            Err("Muniment cannot reach its background service.".into())
        }
    }
}

#[cfg(any(unix, target_os = "windows"))]
trait ThreadSelectClient {
    fn thread_select(&self, thread_id: &str) -> Result<(), ClientError>;
}

#[cfg(any(unix, target_os = "windows"))]
impl ThreadSelectClient for muniment_core::attach::DesktopClientHolder {
    fn thread_select(&self, thread_id: &str) -> Result<(), ClientError> {
        muniment_core::attach::DesktopClientHolder::thread_select(self, thread_id)
    }
}

#[cfg(any(unix, target_os = "windows"))]
enum ThreadSelectSession<C> {
    NoSupervisor,
    Connected(C),
    Disconnected,
}

#[cfg(any(unix, target_os = "windows"))]
impl From<DesktopClientSession>
    for ThreadSelectSession<muniment_core::attach::DesktopClientHolder>
{
    fn from(session: DesktopClientSession) -> Self {
        match session {
            DesktopClientSession::NoSupervisor => Self::NoSupervisor,
            DesktopClientSession::Connected(client) => Self::Connected(client),
            DesktopClientSession::Disconnected => Self::Disconnected,
        }
    }
}

#[cfg(any(unix, target_os = "windows"))]
fn select_thread_command<C: ThreadSelectClient>(
    session: ThreadSelectSession<C>,
    session_thread: &SessionThread,
    subject: Option<&str>,
    thread_id: &str,
) -> Result<(), String> {
    match session {
        ThreadSelectSession::NoSupervisor => Err(auth::background_service_error()),
        ThreadSelectSession::Connected(client) => {
            client
                .thread_select(thread_id)
                .map_err(auth::desktop_client_error)?;
            session_thread.select(thread_id.to_owned(), subject);
            Ok(())
        }
        ThreadSelectSession::Disconnected => Err(auth::background_service_error()),
    }
}

#[cfg(any(unix, target_os = "windows"))]
fn desktop_thread_history_error(error: ClientError) -> String {
    match error {
        ClientError::DesktopBusy | ClientError::AuthorizationExpired => {
            auth::desktop_client_error(error)
        }
        _ => "Conversation history is unavailable.".to_string(),
    }
}

#[cfg(any(unix, target_os = "windows"))]
fn chat_thread_summaries_command(
    session: DesktopClientSession,
    limit: usize,
    cursor: Option<&str>,
) -> Result<serde_json::Value, String> {
    match session {
        DesktopClientSession::NoSupervisor => Err(auth::background_service_error()),
        DesktopClientSession::Connected(client) => client
            .thread_summaries(
                limit
                    .try_into()
                    .map_err(|_| "Conversation history is unavailable.".to_string())?,
                cursor,
            )
            .map_err(desktop_thread_history_error),
        DesktopClientSession::Disconnected => Err(auth::background_service_error()),
    }
}

#[cfg(any(unix, target_os = "windows"))]
fn chat_thread_open_command(
    attach_state: &AttachCompanionState,
    thread_id: &str,
    limit: usize,
    cursor: Option<&str>,
) -> Result<serde_json::Value, String> {
    match attach_state.desktop_client_session() {
        DesktopClientSession::NoSupervisor => Err(auth::background_service_error()),
        DesktopClientSession::Connected(client) => client
            .thread_history(
                thread_id,
                limit
                    .try_into()
                    .map_err(|_| "Conversation history is unavailable.".to_string())?,
                cursor,
            )
            .map_err(desktop_thread_history_error),
        DesktopClientSession::Disconnected => Err(auth::background_service_error()),
    }
}

#[tauri::command]
pub async fn chat_current_thread(
    app_handle: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
) -> Result<Option<String>, String> {
    let subject = chat_subject(&app_handle, &auth_state)?;
    Ok(state.session_thread.current(subject.as_deref()))
}

#[cfg(any(unix, target_os = "windows"))]
#[tauri::command]
pub async fn chat_thread_summaries(
    app_handle: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    attach_state: tauri::State<'_, AttachCompanionState>,
    limit: usize,
    cursor: Option<String>,
) -> Result<serde_json::Value, String> {
    let waiting_app = app_handle.clone();
    let session = tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager;
        waiting_app
            .state::<AttachCompanionState>()
            .desktop_client_session_within(HISTORY_CONNECT_WAIT)
    })
    .await
    .map_err(|_| auth::background_service_error())?;
    let _ = &attach_state;
    #[cfg(target_os = "macos")]
    require_runtime(&session)?;
    // An account that cannot name its subject gets that error before the runtime call.
    chat_subject(&app_handle, &auth_state)?;
    chat_thread_summaries_command(session, limit, cursor.as_deref())
}

#[cfg(any(unix, target_os = "windows"))]
#[tauri::command]
pub async fn chat_select_thread(
    app_handle: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
    attach_state: tauri::State<'_, AttachCompanionState>,
    thread_id: String,
) -> Result<(), String> {
    let session: ThreadSelectSession<_> = attach_state.desktop_client_session().into();
    if matches!(&session, ThreadSelectSession::Disconnected) {
        return Err(auth::background_service_error());
    }
    if matches!(&session, ThreadSelectSession::NoSupervisor) {
        return Err(auth::background_service_error());
    }
    let subject = chat_subject(&app_handle, &auth_state)?;
    select_thread_command(
        session,
        &state.session_thread,
        subject.as_deref(),
        &thread_id,
    )
}

#[cfg(any(unix, target_os = "windows"))]
#[tauri::command]
pub async fn chat_rename_thread(
    app_handle: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    attach_state: tauri::State<'_, AttachCompanionState>,
    thread_id: String,
    title: String,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    require_runtime(&attach_state.desktop_client_session())?;
    // An account that cannot name its subject gets that error before the runtime call.
    chat_subject(&app_handle, &auth_state)?;
    rename_thread_command(&attach_state, &thread_id, &title)
}

#[cfg(any(unix, target_os = "windows"))]
#[tauri::command]
pub async fn chat_delete_thread(
    app_handle: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
    attach_state: tauri::State<'_, AttachCompanionState>,
    thread_id: String,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    require_runtime(&attach_state.desktop_client_session())?;
    let subject = chat_subject(&app_handle, &auth_state)?;
    delete_thread_command(
        attach_state.desktop_client_session().into(),
        &state.session_thread,
        subject.as_deref(),
        &thread_id,
    )
}

#[cfg(any(unix, target_os = "windows"))]
#[tauri::command]
pub async fn chat_new_thread(
    app_handle: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
    project_id: Option<String>,
    agent_id: Option<String>,
) -> Result<String, String> {
    let subject = chat_subject(&app_handle, &auth_state)?;
    let profile = muniment_runtime::profile_directory().map_err(|_| "Agents are unavailable.")?;
    let agent = agent_id
        .as_deref()
        .map(|id| muniment_core::agents::get(&profile, id))
        .transpose()?;
    let project = project_id.or_else(|| agent.as_ref().and_then(|a| a.project_id.clone()));
    let thread = if let Some(agent) = &agent {
        muniment_core::agents::with_state(&profile, |catalog| {
            if let Some(thread) = muniment_core::agents::ensure_primary(catalog, &agent.id) {
                let attach = app_handle.state::<AttachCompanionState>();
                select_thread_command(
                    attach.desktop_client_session().into(),
                    &state.session_thread,
                    subject.as_deref(),
                    &thread,
                )?;
                state
                    .session_thread
                    .select(thread.clone(), subject.as_deref());
                return Ok(thread);
            }
            let thread = crate::projects::prepare_thread(
                &app_handle,
                &state,
                subject.as_deref(),
                project.as_deref(),
                true,
            )?;
            if !thread.is_empty() {
                catalog
                    .primary_threads
                    .insert(agent.id.clone(), thread.clone());
                catalog.threads.insert(thread.clone(), agent.id.clone());
            }
            Ok(thread)
        })?
    } else {
        crate::projects::prepare_thread(
            &app_handle,
            &state,
            subject.as_deref(),
            project.as_deref(),
            true,
        )?
    };
    if let Some(agent) = agent {
        muniment_core::agents::assign(&profile, &thread, &agent.id)?;
    }
    Ok(thread)
}

#[cfg(any(unix, target_os = "windows"))]
#[tauri::command]
pub async fn chat_thread_open(
    app_handle: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    attach_state: tauri::State<'_, AttachCompanionState>,
    thread_id: String,
    limit: usize,
    cursor: Option<String>,
) -> Result<serde_json::Value, String> {
    #[cfg(target_os = "macos")]
    require_runtime(&attach_state.desktop_client_session())?;
    // An account that cannot name its subject, or a missing profile, gets that
    // error before the runtime call.
    chat_subject(&app_handle, &auth_state)?;
    state_session_root(&app_handle)?;
    chat_thread_open_command(&attach_state, &thread_id, limit, cursor.as_deref())
}

/// How often a context command asks the runtime again while it reads.
const CONTEXT_POLL: std::time::Duration = std::time::Duration::from_millis(250);
/// The bound on one usage read, which can start the agent runtime.
const CONTEXT_READ_WAIT: std::time::Duration = std::time::Duration::from_secs(45);
/// The bound on one manual compaction, which waits for a model's summary.
const COMPACTION_WAIT: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// One thread's context window, as the meter shows it.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ThreadContextUsage {
    tokens: Option<u64>,
    context_window: Option<u64>,
    percent: Option<f64>,
    estimated: bool,
    window_known: bool,
    provider: Option<String>,
    model: Option<String>,
    auto_compact: bool,
    compact_at: Option<u64>,
    compacting: bool,
}

/// The token counts of one finished manual compaction.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ThreadCompaction {
    tokens_before: u64,
    estimated_tokens_after: Option<u64>,
}

fn context_response_error() -> String {
    "The background service returned an invalid context reading. Restart Muniment.".into()
}

/// Maps a finished `thread.context_usage` answer. A thread with no saved
/// conversation reports no tokens and the automatic compaction setting.
fn context_usage_view(
    snapshot: &serde_json::Value,
    auto_compact: impl FnOnce() -> Result<bool, String>,
) -> Result<ThreadContextUsage, String> {
    let compacting = snapshot["compacting"]
        .as_bool()
        .ok_or_else(context_response_error)?;
    let usage = &snapshot["usage"];
    if usage.is_null() {
        if let Some(error) = snapshot["error"].as_str() {
            return Err(error.to_owned());
        }
        return Ok(ThreadContextUsage {
            tokens: None,
            context_window: None,
            percent: None,
            estimated: false,
            window_known: false,
            provider: None,
            model: None,
            auto_compact: auto_compact()?,
            compact_at: None,
            compacting,
        });
    }
    let flag = |name: &str| usage[name].as_bool().ok_or_else(context_response_error);
    let text = |name: &str| usage[name].as_str().map(str::to_owned);
    Ok(ThreadContextUsage {
        tokens: usage["tokens"].as_u64(),
        context_window: usage["context_window"].as_u64(),
        percent: usage["percent"].as_f64(),
        estimated: flag("estimated")?,
        window_known: flag("window_known")?,
        provider: text("provider"),
        model: text("model"),
        auto_compact: flag("auto_compact")?,
        compact_at: usage["compact_at"].as_u64(),
        compacting,
    })
}

/// Reads the outcome of the compaction `run_id` names from a
/// `thread.context_usage` answer. `None` means it still runs.
fn compaction_outcome(
    snapshot: &serde_json::Value,
    run_id: &str,
) -> Option<Result<ThreadCompaction, String>> {
    let compaction = &snapshot["compaction"];
    if compaction["run_id"].as_str() != Some(run_id) {
        return Some(Err(context_response_error()));
    }
    match compaction["state"].as_str() {
        Some("running") => None,
        Some("completed") => Some(
            compaction["tokens_before"]
                .as_u64()
                .map(|tokens_before| ThreadCompaction {
                    tokens_before,
                    estimated_tokens_after: compaction["estimated_tokens_after"].as_u64(),
                })
                .ok_or_else(context_response_error),
        ),
        Some("failed") => Some(Err(compaction["error"]
            .as_str()
            .unwrap_or("Context compaction did not finish. Try again.")
            .to_owned())),
        _ => Some(Err(context_response_error())),
    }
}

/// Asks the runtime until `settle` answers or the bound passes. The client
/// lock is free between asks, so other requests proceed.
fn poll_runtime<T>(
    bound: std::time::Duration,
    late: &str,
    mut ask: impl FnMut() -> Result<serde_json::Value, String>,
    mut settle: impl FnMut(&serde_json::Value) -> Option<Result<T, String>>,
) -> Result<T, String> {
    let deadline = std::time::Instant::now() + bound;
    loop {
        if let Some(result) = settle(&ask()?) {
            return result;
        }
        if std::time::Instant::now() >= deadline {
            return Err(late.to_owned());
        }
        std::thread::sleep(CONTEXT_POLL);
    }
}

#[cfg(any(unix, target_os = "windows"))]
async fn context_client(
    app_handle: &tauri::AppHandle,
) -> Result<muniment_core::attach::DesktopClientHolder, String> {
    let waiting_app = app_handle.clone();
    let session = tauri::async_runtime::spawn_blocking(move || {
        waiting_app
            .state::<AttachCompanionState>()
            .desktop_client_session_within(HISTORY_CONNECT_WAIT)
    })
    .await
    .map_err(|_| auth::background_service_error())?;
    match session {
        DesktopClientSession::Connected(client) => Ok(client),
        DesktopClientSession::NoSupervisor | DesktopClientSession::Disconnected => {
            Err(auth::background_service_error())
        }
    }
}

/// Runs one compaction settings request and answers the settings in the
/// shape the Settings page reads.
#[cfg(any(unix, target_os = "windows"))]
pub(crate) async fn context_settings_request(
    app_handle: tauri::AppHandle,
    request: impl FnOnce(
            &muniment_core::attach::DesktopClientHolder,
        ) -> Result<
            serde_json::Value,
            (ClientError, Option<muniment_core::attach::ProtocolError>),
        > + Send
        + 'static,
) -> Result<serde_json::Value, String> {
    let client = context_client(&app_handle).await?;
    let answer = tauri::async_runtime::spawn_blocking(move || request(&client))
        .await
        .map_err(|_| auth::background_service_error())?
        .map_err(|(error, failure)| auth::desktop_request_error(error, failure.as_ref()))?;
    context_settings_view(&answer)
}

fn context_settings_view(answer: &serde_json::Value) -> Result<serde_json::Value, String> {
    match (
        answer["enabled"].as_bool(),
        answer["reserve_tokens"].as_u64(),
        answer["keep_recent_tokens"].as_u64(),
    ) {
        (Some(enabled), Some(reserve_tokens), Some(keep_recent_tokens)) => Ok(serde_json::json!({
            "enabled": enabled,
            "reserveTokens": reserve_tokens,
            "keepRecentTokens": keep_recent_tokens,
        })),
        _ => Err(context_response_error()),
    }
}

/// Reads how much of its context window a thread fills. The runtime reads
/// the agent engine, and the command waits until that read ends.
#[cfg(any(unix, target_os = "windows"))]
#[tauri::command]
pub async fn thread_context_usage(
    app_handle: tauri::AppHandle,
    thread_id: String,
) -> Result<ThreadContextUsage, String> {
    let client = context_client(&app_handle).await?;
    tauri::async_runtime::spawn_blocking(move || {
        let snapshot = poll_runtime(
            CONTEXT_READ_WAIT,
            "The context usage did not arrive in time. Try again.",
            || {
                client
                    .thread_context_usage(&thread_id)
                    .map_err(|(error, failure)| auth::desktop_request_error(error, failure.as_ref()))
            },
            |snapshot| (snapshot["reading"] == false).then(|| Ok(snapshot.clone())),
        )?;
        context_usage_view(&snapshot, || {
            client
                .context_settings()
                .map_err(|(error, failure)| auth::desktop_request_error(error, failure.as_ref()))?
                ["enabled"]
                .as_bool()
                .ok_or_else(context_response_error)
        })
    })
    .await
    .map_err(|_| auth::background_service_error())?
}

/// Compacts a thread's conversation now, with optional instructions for the
/// summary. The runtime refuses while a reply runs in that thread.
#[cfg(any(unix, target_os = "windows"))]
#[tauri::command]
pub async fn thread_compact(
    app_handle: tauri::AppHandle,
    thread_id: String,
    instructions: Option<String>,
) -> Result<ThreadCompaction, String> {
    let client = context_client(&app_handle).await?;
    tauri::async_runtime::spawn_blocking(move || {
        let instructions = instructions
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty());
        let started = client
            .compact_thread(&thread_id, instructions)
            .map_err(|(error, failure)| auth::desktop_request_error(error, failure.as_ref()))?;
        let run_id = started["run_id"]
            .as_str()
            .ok_or_else(context_response_error)?
            .to_owned();
        poll_runtime(
            COMPACTION_WAIT,
            "Context compaction is still running. Its result appears in the thread when it ends.",
            || {
                client
                    .thread_context_usage(&thread_id)
                    .map_err(|(error, failure)| auth::desktop_request_error(error, failure.as_ref()))
            },
            |snapshot| compaction_outcome(snapshot, &run_id),
        )
    })
    .await
    .map_err(|_| auth::background_service_error())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(any(unix, target_os = "windows"))]
    use std::sync::{Arc, Mutex};

    #[cfg(any(unix, target_os = "windows"))]
    #[test]
    fn history_names_an_authorization_refusal() {
        assert_eq!(
            desktop_thread_history_error(ClientError::AuthorizationExpired),
            "The runtime refused the request as unauthorized. Enter local mode or sign in, then retry."
        );
        assert_eq!(
            desktop_thread_history_error(ClientError::ConnectionClosed),
            "Conversation history is unavailable."
        );
    }

    #[cfg(any(unix, target_os = "windows"))]
    #[derive(Clone)]
    struct FakeThreadSelectClient {
        calls: Arc<Mutex<Vec<String>>>,
        result: Result<(), ClientError>,
    }

    #[cfg(any(unix, target_os = "windows"))]
    impl ThreadSelectClient for FakeThreadSelectClient {
        fn thread_select(&self, thread_id: &str) -> Result<(), ClientError> {
            self.calls.lock().unwrap().push(thread_id.to_owned());
            self.result
        }
    }

    #[cfg(any(unix, target_os = "windows"))]
    #[derive(Clone)]
    struct FakeThreadDeleteClient {
        calls: Arc<Mutex<Vec<String>>>,
        result: Result<(), ClientError>,
    }

    #[cfg(any(unix, target_os = "windows"))]
    impl ThreadDeleteClient for FakeThreadDeleteClient {
        fn delete_thread(&self, thread_id: &str) -> Result<(), ClientError> {
            self.calls.lock().unwrap().push(thread_id.to_owned());
            self.result
        }
    }

    #[cfg(any(unix, target_os = "windows"))]
    #[test]
    fn connected_delete_resets_the_selected_thread() {
        let tracker = SessionThread::default();
        tracker.select("selected-thread".to_string(), Some("owner"));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let client = FakeThreadDeleteClient {
            calls: Arc::clone(&calls),
            result: Ok(()),
        };

        delete_thread_command(
            ThreadDeleteSession::Connected(client),
            &tracker,
            Some("owner"),
            "selected-thread",
        )
        .unwrap();

        assert_eq!(&*calls.lock().unwrap(), &["selected-thread"]);
        assert_eq!(tracker.current(Some("owner")), None);
    }

    #[cfg(any(unix, target_os = "windows"))]
    #[test]
    fn connected_delete_keeps_the_other_selected_thread_and_keeps_it_on_failure() {
        let tracker = SessionThread::default();
        tracker.select("selected-thread".to_string(), Some("owner"));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let successful_client = FakeThreadDeleteClient {
            calls: Arc::clone(&calls),
            result: Ok(()),
        };

        delete_thread_command(
            ThreadDeleteSession::Connected(successful_client),
            &tracker,
            Some("owner"),
            "other-thread",
        )
        .unwrap();
        assert_eq!(
            tracker.current(Some("owner")),
            Some("selected-thread".to_string())
        );

        let failed_client = FakeThreadDeleteClient {
            calls: Arc::clone(&calls),
            result: Err(ClientError::AuthorizationExpired),
        };
        assert!(delete_thread_command(
            ThreadDeleteSession::Connected(failed_client),
            &tracker,
            Some("owner"),
            "selected-thread",
        )
        .is_err());

        assert_eq!(
            &*calls.lock().unwrap(),
            &["other-thread", "selected-thread"]
        );
        assert_eq!(
            tracker.current(Some("owner")),
            Some("selected-thread".to_string())
        );
    }

    #[cfg(any(unix, target_os = "windows"))]
    #[test]
    fn delete_without_a_desktop_client_keeps_the_selected_thread() {
        let tracker = SessionThread::default();
        tracker.select("selected-thread".to_string(), Some("owner"));
        for session in [
            ThreadDeleteSession::<FakeThreadDeleteClient>::NoSupervisor,
            ThreadDeleteSession::Disconnected,
        ] {
            assert_eq!(
                delete_thread_command(session, &tracker, Some("owner"), "selected-thread")
                    .unwrap_err(),
                "Muniment cannot reach its background service."
            );
        }
        assert_eq!(
            tracker.current(Some("owner")),
            Some("selected-thread".to_string())
        );
    }

    #[cfg(any(unix, target_os = "windows"))]
    #[test]
    fn select_thread_routes_all_desktop_client_states_and_tracks_only_success() {
        let tracker = SessionThread::default();

        assert_eq!(
            select_thread_command::<FakeThreadSelectClient>(
                ThreadSelectSession::NoSupervisor,
                &tracker,
                Some("owner"),
                "local-thread",
            )
            .unwrap_err(),
            "Muniment cannot reach its background service."
        );
        assert_eq!(tracker.current(Some("owner")), None);

        let calls = Arc::new(Mutex::new(Vec::new()));
        let connected = FakeThreadSelectClient {
            calls: Arc::clone(&calls),
            result: Ok(()),
        };
        select_thread_command(
            ThreadSelectSession::Connected(connected),
            &tracker,
            Some("owner"),
            "remote-thread",
        )
        .unwrap();
        assert_eq!(&*calls.lock().unwrap(), &["remote-thread"]);
        assert_eq!(
            tracker.current(Some("owner")),
            Some("remote-thread".to_string())
        );

        let failed = FakeThreadSelectClient {
            calls,
            result: Err(ClientError::AuthorizationExpired),
        };
        assert!(select_thread_command(
            ThreadSelectSession::Connected(failed),
            &tracker,
            Some("owner"),
            "rejected-thread",
        )
        .is_err());
        assert_eq!(
            tracker.current(Some("owner")),
            Some("remote-thread".to_string())
        );

        assert_eq!(
            select_thread_command::<FakeThreadSelectClient>(
                ThreadSelectSession::Disconnected,
                &tracker,
                Some("owner"),
                "disconnected-thread",
            )
            .unwrap_err(),
            "Muniment cannot reach its background service."
        );
        assert_eq!(
            tracker.current(Some("owner")),
            Some("remote-thread".to_string())
        );
    }

    #[cfg(unix)]
    #[test]
    fn thread_commands_cover_each_desktop_client_state() {
        use muniment_core::attach::{
            encode_frame, reconnect_welcome, serve_desktop_client_at, Id, Protocol, Response,
            Success,
        };
        use serde_json::{json, Value};
        use std::io::{Read, Write};
        use std::os::unix::net::UnixListener;
        use std::sync::mpsc;
        use std::time::Duration;

        fn invoke_rename(
            attach_state: &AttachCompanionState,
            thread_id: &str,
            title: &str,
        ) -> Result<(), String> {
            rename_thread_command(attach_state, thread_id, title)
        }

        fn invoke_delete(
            attach_state: &AttachCompanionState,
            thread_id: &str,
        ) -> Result<(), String> {
            delete_thread_command(
                attach_state.desktop_client_session().into(),
                &SessionThread::default(),
                Some("owner"),
                thread_id,
            )
        }

        fn invoke_summaries(
            attach_state: &AttachCompanionState,
            limit: usize,
            cursor: Option<&str>,
        ) -> Result<Value, String> {
            chat_thread_summaries_command(attach_state.desktop_client_session(), limit, cursor)
        }

        fn invoke_open(
            attach_state: &AttachCompanionState,
            thread_id: &str,
            limit: usize,
            cursor: Option<&str>,
        ) -> Result<Value, String> {
            chat_thread_open_command(attach_state, thread_id, limit, cursor)
        }

        fn read_value(stream: &mut impl Read) -> Value {
            let mut length = [0; 4];
            stream.read_exact(&mut length).unwrap();
            let mut payload = vec![0; u32::from_be_bytes(length) as usize];
            stream.read_exact(&mut payload).unwrap();
            serde_json::from_slice(&payload).unwrap()
        }

        let unsupervised = AttachCompanionState::default();
        let errors = [
            invoke_summaries(&unsupervised, 100, None).unwrap_err(),
            invoke_open(&unsupervised, "thread-1", 100, None).unwrap_err(),
            invoke_rename(&unsupervised, "thread-1", "title").unwrap_err(),
            invoke_delete(&unsupervised, "thread-1").unwrap_err(),
        ];
        for error in errors {
            assert_eq!(error, auth::background_service_error());
        }

        let endpoint = crate::test_support::socket_temp_path();
        let listener = UnixListener::bind(&endpoint).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            assert_eq!(read_value(&mut stream)["client"]["kind"], "desktop-client");
            stream
                .write_all(
                    &encode_frame(&reconnect_welcome(1, "0.0.1", "11".repeat(16), "")).unwrap(),
                )
                .unwrap();
            stream
                .write_all(
                    &encode_frame(&json!({
                        "profile_id": "profile-1",
                        "capability": "33".repeat(32),
                        "expires_at": 60,
                        "idle_timeout_seconds": 30,
                        "workspace_scopes": {"/work/signed": ["threads:read"]}
                    }))
                    .unwrap(),
                )
                .unwrap();
            let mut requests = Vec::new();
            for _ in 0..4 {
                let request = read_value(&mut stream);
                requests.push((request["operation"].clone(), request["body"].clone()));
                let request_id = Id::new(request["request_id"].as_str().unwrap()).unwrap();
                let body = match request["operation"].as_str().unwrap() {
                    "thread.summaries" => json!({
                        "summaries": [{"threadId": "remote", "title": "Remote", "updatedAt": "now"}],
                        "nextCursor": "remote-next"
                    }),
                    "thread.history" => json!({
                        "entries": [{"remote": true}],
                        "nextCursor": "history-next"
                    }),
                    _ => json!({}),
                };
                stream
                    .write_all(
                        &encode_frame(&Response {
                            protocol: Protocol,
                            request_id,
                            ok: Success,
                            body,
                        })
                        .unwrap(),
                    )
                    .unwrap();
            }
            requests
        });
        let live_state = AttachCompanionState::default();
        let client_endpoint = endpoint.clone();
        let (connected_tx, connected_rx) = mpsc::channel();
        live_state.set_desktop_client_for_test(false, move |stop, holder| {
            std::thread::spawn(move || {
                let mut connected_tx = Some(connected_tx);
                serve_desktop_client_at(
                    &client_endpoint,
                    "0.0.1",
                    Duration::from_secs(1),
                    Duration::from_millis(10),
                    stop,
                    holder,
                    move |connected| {
                        if connected {
                            connected_tx.take().unwrap().send(()).unwrap();
                        }
                    },
                )
            })
        });
        connected_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        live_state.set_desktop_client_for_test(true, |_, _| std::thread::spawn(|| {}));
        let live_id = "thread-1";
        assert_eq!(
            invoke_summaries(&live_state, 100, Some("summary-cursor")).unwrap(),
            json!({
                "summaries": [{"threadId": "remote", "title": "Remote", "updatedAt": "now"}],
                "nextCursor": "remote-next"
            })
        );
        assert_eq!(
            invoke_open(&live_state, live_id, 99, Some("history-cursor")).unwrap(),
            json!({"entries": [{"remote": true}], "nextCursor": "history-next"})
        );
        invoke_rename(&live_state, live_id, "Remote title").unwrap();
        invoke_delete(&live_state, live_id).unwrap();
        let requests = server.join().unwrap();
        assert_eq!(
            requests,
            vec![
                (
                    json!("thread.summaries"),
                    json!({"limit": 100, "cursor": "summary-cursor"})
                ),
                (
                    json!("thread.history"),
                    json!({
                        "thread_id": live_id,
                        "limit": 99,
                        "cursor": "history-cursor",
                        "snapshot_chunks": true
                    })
                ),
                (
                    json!("thread.rename"),
                    json!({"thread_id": live_id, "title": "Remote title"})
                ),
                (json!("thread.delete"), json!({"thread_id": live_id})),
            ]
        );
        assert_eq!(
            invoke_summaries(&live_state, 100, None).unwrap_err(),
            "Conversation history is unavailable."
        );
        assert_eq!(
            invoke_open(&live_state, live_id, 100, None).unwrap_err(),
            "Conversation history is unavailable."
        );
        live_state.stop_desktop_client_for_test();

        let disconnected_state = AttachCompanionState::default();
        disconnected_state.set_desktop_client_for_test(false, |_, _| std::thread::spawn(|| {}));
        let errors = [
            invoke_summaries(&disconnected_state, 100, Some("cursor")).unwrap_err(),
            invoke_open(&disconnected_state, "thread-1", 100, Some("cursor")).unwrap_err(),
            invoke_rename(&disconnected_state, "thread-1", "title").unwrap_err(),
            invoke_delete(&disconnected_state, "thread-1").unwrap_err(),
        ];
        for error in errors {
            assert_eq!(error, "Muniment cannot reach its background service.");
        }
        disconnected_state.stop_desktop_client_for_test();
        let _ = std::fs::remove_file(endpoint);
    }

    #[test]
    fn context_settings_keep_the_settings_page_shape() {
        assert_eq!(
            context_settings_view(&serde_json::json!({
                "enabled": false, "reserve_tokens": 8192, "keep_recent_tokens": 12000
            }))
            .unwrap(),
            serde_json::json!({"enabled": false, "reserveTokens": 8192, "keepRecentTokens": 12000})
        );
        assert!(context_settings_view(&serde_json::json!({"enabled": true})).is_err());
    }

    #[test]
    fn a_context_answer_maps_to_the_meter_shape() {
        let snapshot = serde_json::json!({
            "usage": {
                "tokens": 31900, "context_window": 200000, "percent": 15.95,
                "estimated": true, "window_known": true, "provider": "local",
                "model": "model", "auto_compact": true, "compact_at": 183616
            },
            "reading": false, "error": null, "compacting": false, "compaction": null
        });
        assert_eq!(
            serde_json::to_value(
                context_usage_view(&snapshot, || panic!("usage names the setting")).unwrap()
            )
            .unwrap(),
            serde_json::json!({
                "tokens": 31900, "contextWindow": 200000, "percent": 15.95,
                "estimated": true, "windowKnown": true, "provider": "local",
                "model": "model", "autoCompact": true, "compactAt": 183616,
                "compacting": false
            })
        );
        let empty = serde_json::json!({
            "usage": null, "reading": false, "error": null, "compacting": false, "compaction": null
        });
        assert_eq!(
            serde_json::to_value(context_usage_view(&empty, || Ok(false)).unwrap()).unwrap(),
            serde_json::json!({
                "tokens": null, "contextWindow": null, "percent": null,
                "estimated": false, "windowKnown": false, "provider": null,
                "model": null, "autoCompact": false, "compactAt": null,
                "compacting": false
            })
        );
        let failed = serde_json::json!({
            "usage": null, "reading": false, "error": "The agent runtime is not installed.",
            "compacting": false, "compaction": null
        });
        assert_eq!(
            context_usage_view(&failed, || Ok(true)).unwrap_err(),
            "The agent runtime is not installed."
        );
    }

    #[test]
    fn a_compaction_waits_for_its_run_and_reports_its_counts_or_reason() {
        let answer = |state: &str, run_id: &str| {
            serde_json::json!({"compaction": {
                "run_id": run_id, "state": state, "tokens_before": 152400,
                "estimated_tokens_after": 31900, "error": "Nothing to compact"
            }})
        };
        assert_eq!(compaction_outcome(&answer("running", "run-1"), "run-1"), None);
        assert_eq!(
            compaction_outcome(&answer("completed", "run-1"), "run-1"),
            Some(Ok(ThreadCompaction {
                tokens_before: 152400,
                estimated_tokens_after: Some(31900),
            }))
        );
        assert_eq!(
            compaction_outcome(&answer("failed", "run-1"), "run-1"),
            Some(Err("Nothing to compact".into()))
        );
        assert!(compaction_outcome(&answer("completed", "run-2"), "run-1")
            .unwrap()
            .is_err());
        let mut answers = vec![answer("completed", "run-1"), answer("running", "run-1")];
        assert_eq!(
            poll_runtime(
                std::time::Duration::from_secs(5),
                "late",
                || Ok(answers.pop().unwrap()),
                |snapshot| compaction_outcome(snapshot, "run-1"),
            )
            .unwrap()
            .tokens_before,
            152400
        );
        assert_eq!(
            poll_runtime(
                std::time::Duration::ZERO,
                "late",
                || Ok(answer("running", "run-1")),
                |snapshot| compaction_outcome(snapshot, "run-1"),
            )
            .unwrap_err(),
            "late"
        );
    }
}
