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
    session: DesktopClientSession,
    thread_id: &str,
    title: &str,
) -> Result<(), String> {
    match session {
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
    fn delete_thread(&self, thread_id: &str) -> Result<serde_json::Value, ClientError>;
}

#[cfg(any(unix, target_os = "windows"))]
impl ThreadDeleteClient for muniment_core::attach::DesktopClientHolder {
    fn delete_thread(&self, thread_id: &str) -> Result<serde_json::Value, ClientError> {
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
) -> Result<serde_json::Value, String> {
    match session {
        ThreadDeleteSession::NoSupervisor => Err(auth::background_service_error()),
        ThreadDeleteSession::Connected(client) => {
            let answer = client
                .delete_thread(thread_id)
                .map_err(auth::desktop_client_error)?;
            session_thread.fresh_if_current(thread_id, subject);
            Ok(answer)
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
    session: DesktopClientSession,
    thread_id: &str,
    limit: usize,
    cursor: Option<&str>,
) -> Result<serde_json::Value, String> {
    match session {
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

/// The id the merged thread list gives this computer's runtime.
#[cfg(any(unix, target_os = "windows"))]
const LOCAL_RUNTIME: &str = "local";

/// One runtime's page of thread summaries in the merged list.
#[cfg(any(unix, target_os = "windows"))]
struct RuntimePage {
    runtime_id: String,
    page: serde_json::Value,
}

/// The threads this computer's runtime deleted, from its page, when it
/// answered. A runtime that names none answers an empty list.
#[cfg(any(unix, target_os = "windows"))]
fn local_deleted_threads(pages: &[RuntimePage]) -> Option<Vec<String>> {
    let page = pages.iter().find(|page| page.runtime_id == LOCAL_RUNTIME)?;
    Some(
        page.page
            .get("deletedThreadIds")
            .and_then(serde_json::Value::as_array)
            .map(|ids| {
                ids.iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
    )
}

/// Merges each runtime's page into one list, newest first. Each summary names
/// its runtime, `runtimeIds` names the runtimes that answered, and the next
/// cursor holds the cursor of every runtime with more threads, as a JSON
/// object from runtime id to cursor. A paired runtime's copy of a thread this
/// computer deleted is left out: the delete here holds, whatever another
/// machine still keeps.
#[cfg(any(unix, target_os = "windows"))]
fn merge_thread_pages(
    pages: Vec<RuntimePage>,
    deleted_here: &std::collections::HashSet<String>,
) -> serde_json::Value {
    let mut summaries = Vec::new();
    let mut cursors = serde_json::Map::new();
    let mut answered = Vec::new();
    for RuntimePage { runtime_id, page } in pages {
        answered.push(runtime_id.clone());
        if let Some(listed) = page.get("summaries").and_then(serde_json::Value::as_array) {
            for summary in listed {
                let thread_id = summary
                    .get("threadId")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                if runtime_id != LOCAL_RUNTIME && deleted_here.contains(thread_id) {
                    continue;
                }
                let mut summary = summary.clone();
                if let Some(object) = summary.as_object_mut() {
                    object.insert("runtimeId".into(), runtime_id.clone().into());
                }
                summaries.push(summary);
            }
        }
        if let Some(next) = page.get("nextCursor").and_then(serde_json::Value::as_str) {
            cursors.insert(runtime_id, next.into());
        }
    }
    let updated = |summary: &serde_json::Value| {
        summary
            .get("updatedAt")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    summaries.sort_by_key(|summary| std::cmp::Reverse(updated(summary)));
    let next_cursor = (!cursors.is_empty()).then(|| serde_json::Value::Object(cursors).to_string());
    serde_json::json!({ "summaries": summaries, "nextCursor": next_cursor, "runtimeIds": answered })
}

/// This computer's fleet id: the owner its runtime names on a thread it
/// runs, when its page lists one.
#[cfg(any(unix, target_os = "windows"))]
fn local_fleet_id(pages: &[RuntimePage]) -> Option<String> {
    let page = pages.iter().find(|page| page.runtime_id == LOCAL_RUNTIME)?;
    page.page
        .get("summaries")?
        .as_array()?
        .iter()
        .filter(|summary| summary.get("replica").and_then(serde_json::Value::as_bool) != Some(true))
        .find_map(|summary| summary.get("owner_runtime")?.as_str())
        .map(str::to_owned)
}

/// The runtime each listed thread lives on, one per thread, whichever copy
/// lists first: the copy of the machine that runs it, then this computer's
/// copy, which posts through to its owner. A thread this computer runs
/// that only a paired machine listed, such as while this computer's runtime
/// did not answer, still lives here. Any other thread lives on the paired
/// machine with the lowest id that lists it.
#[cfg(any(unix, target_os = "windows"))]
fn thread_runtimes(
    listed: &[serde_json::Value],
    local_fleet_id: Option<&str>,
) -> Vec<(String, String)> {
    // Each thread's best copy so far: its rank, its runtime and its owner.
    let mut best: std::collections::BTreeMap<String, (u8, String, Option<String>)> =
        std::collections::BTreeMap::new();
    for summary in listed {
        let (Some(thread), Some(runtime)) = (
            summary.get("threadId").and_then(serde_json::Value::as_str),
            summary.get("runtimeId").and_then(serde_json::Value::as_str),
        ) else {
            continue;
        };
        let runs_it = summary.get("replica").and_then(serde_json::Value::as_bool) != Some(true);
        let rank = u8::from(runs_it) * 2 + u8::from(runtime == LOCAL_RUNTIME);
        let owner = summary
            .get("owner_runtime")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let better = best.get(thread).is_none_or(|(known, held, _)| {
            rank > *known || (rank == *known && runtime < held.as_str())
        });
        if better {
            best.insert(thread.to_owned(), (rank, runtime.to_owned(), owner));
        }
    }
    best.into_iter()
        .map(|(thread, (rank, runtime, owner))| {
            let ours = rank == 0 && local_fleet_id.is_some() && owner.as_deref() == local_fleet_id;
            (thread, if ours { LOCAL_RUNTIME.to_owned() } else { runtime })
        })
        .collect()
}

/// Merges the pages that answered and records where each listed thread
/// lives, which later actions on a thread read. `deletedThreadIds` names the
/// threads deleted here, so a run whose thread is gone says so in the window.
#[cfg(any(unix, target_os = "windows"))]
fn record_thread_list(attach: &AttachCompanionState, pages: Vec<RuntimePage>) -> serde_json::Value {
    if let Some(deleted) = local_deleted_threads(&pages) {
        attach.remember_deleted_threads(deleted);
    }
    if let Some(id) = local_fleet_id(&pages) {
        attach.remember_local_fleet_id(id);
    }
    let local_answered = pages.iter().any(|page| page.runtime_id == LOCAL_RUNTIME);
    let deleted = attach.deleted_threads();
    let mut merged = merge_thread_pages(pages, &deleted);
    let mut deleted: Vec<String> = deleted.into_iter().collect();
    deleted.sort();
    merged["deletedThreadIds"] = deleted.into();
    if let Some(listed) = merged["summaries"].as_array() {
        let fleet_id = attach.local_fleet_id();
        // A list without this computer's runtime moves none of its threads
        // to the paired machine that holds a copy. Until this computer's
        // runtime names its fleet id, as at a launch where a paired machine
        // answers first, any synced copy may be one of this computer's own
        // threads, so such a list records only the threads a paired machine
        // runs itself, and a select of a copy never selects its holder.
        if local_answered {
            attach.remember_thread_runtimes(thread_runtimes(listed, fleet_id.as_deref()));
        } else {
            let known: Vec<serde_json::Value> = listed
                .iter()
                .filter(|summary| {
                    fleet_id.is_some()
                        || summary.get("replica").and_then(serde_json::Value::as_bool) != Some(true)
                })
                .cloned()
                .collect();
            attach.keep_thread_runtimes(thread_runtimes(&known, fleet_id.as_deref()));
        }
    }
    merged
}

/// The runtimes a page request reads, with each one's cursor: every runtime
/// for the first page, then those the merged cursor names.
#[cfg(any(unix, target_os = "windows"))]
fn merged_cursor_targets(
    cursor: Option<&str>,
    runtimes: &[String],
) -> Result<Vec<(String, Option<String>)>, String> {
    let Some(cursor) = cursor else {
        return Ok(runtimes.iter().map(|id| (id.clone(), None)).collect());
    };
    let cursors: std::collections::BTreeMap<String, String> = serde_json::from_str(cursor)
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    Ok(cursors
        .into_iter()
        .map(|(id, cursor)| (id, Some(cursor)))
        .collect())
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
    let _ = &attach_state;
    // An account that cannot name its subject gets that error before the runtime call.
    chat_subject(&app_handle, &auth_state)?;
    let reading_app = app_handle.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let attach = reading_app.state::<AttachCompanionState>();
        let mut runtimes = vec![LOCAL_RUNTIME.to_owned()];
        runtimes.extend(attach.remote_ids());
        let targets = merged_cursor_targets(cursor.as_deref(), &runtimes)?;
        let mut pages = Vec::new();
        let mut first_error = None;
        for (runtime_id, cursor) in targets {
            let session = if runtime_id == LOCAL_RUNTIME {
                attach.local_client_session_within(HISTORY_CONNECT_WAIT)
            } else {
                attach.paired_session(&runtime_id)
            };
            match chat_thread_summaries_command(session, limit, cursor.as_deref()) {
                Ok(page) => pages.push(RuntimePage { runtime_id, page }),
                // A paired runtime that is offline leaves its threads out.
                Err(error) if runtime_id == LOCAL_RUNTIME => first_error = Some(error),
                Err(_) => {}
            }
        }
        if pages.is_empty() {
            return Err(first_error.unwrap_or_else(auth::background_service_error));
        }
        Ok(record_thread_list(&attach, pages))
    })
    .await
    .map_err(|_| auth::background_service_error())?
    .map_err(|error| crate::runtimes::name_unreachable(&app_handle, error))
}

#[cfg(any(unix, target_os = "windows"))]
#[tauri::command]
pub async fn chat_select_thread(
    app_handle: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
    attach_state: tauri::State<'_, AttachCompanionState>,
    thread_id: String,
    runtime_id: Option<String>,
) -> Result<(), String> {
    // The window names the machine its thread row shows, so requests and the
    // model list follow that machine even before a thread list names it.
    if let Some(runtime) = runtime_id.filter(|runtime| {
        runtime == LOCAL_RUNTIME || attach_state.remote_ids().iter().any(|id| id == runtime)
    }) {
        attach_state.remember_thread_runtimes([(thread_id.clone(), runtime)]);
    }
    crate::runtimes::follow_thread(&app_handle, &thread_id);
    // The select reaches the thread's own runtime, whichever one is active.
    let (runtime, session) = select_thread_session(&attach_state, &thread_id);
    let result = match session {
        ThreadSelectSession::NoSupervisor | ThreadSelectSession::Disconnected => {
            Err(auth::background_service_error())
        }
        session => chat_subject(&app_handle, &auth_state).and_then(|subject| {
            select_thread_command(
                session,
                &state.session_thread,
                subject.as_deref(),
                &thread_id,
            )
        }),
    };
    if let Err(error) = &result {
        muniment_core::runtime_eprintln!(
            "{}",
            thread_select_failure_record(&thread_id, &runtime, error)
        );
    }
    result
}

/// The runtime a select of `thread_id` reaches and its session: the runtime a
/// list, a select or the thread's creation named, else the active one.
#[cfg(any(unix, target_os = "windows"))]
fn select_thread_session(
    attach: &AttachCompanionState,
    thread_id: &str,
) -> (
    String,
    ThreadSelectSession<muniment_core::attach::DesktopClientHolder>,
) {
    let runtime = attach.thread_runtime_or_active(thread_id);
    let session = attach
        .thread_session_within(thread_id, std::time::Duration::ZERO)
        .into();
    (runtime, session)
}

/// One log line for a failed select. It names the thread and the runtime the
/// select went to, and the shell's message, which carries no credential.
#[cfg(any(unix, target_os = "windows"))]
fn thread_select_failure_record(thread_id: &str, runtime_id: &str, error: &str) -> String {
    let bounded = |value: &str, limit: usize| -> String {
        serde_json::Value::from(value.chars().take(limit).collect::<String>()).to_string()
    };
    format!(
        "muniment-desktop: thread select failed thread_id={} runtime_id={} error={}",
        bounded(thread_id, 128),
        bounded(runtime_id, 128),
        bounded(error, 512),
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
    // A rename reaches the thread's machine and leaves the selected one.
    let session = attach_state.thread_session_within(&thread_id, HISTORY_CONNECT_WAIT);
    #[cfg(target_os = "macos")]
    require_runtime(&session)?;
    // An account that cannot name its subject gets that error before the runtime call.
    chat_subject(&app_handle, &auth_state)?;
    rename_thread_command(session, &thread_id, &title)
}

#[cfg(any(unix, target_os = "windows"))]
#[tauri::command]
pub async fn chat_delete_thread(
    app_handle: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
    attach_state: tauri::State<'_, AttachCompanionState>,
    thread_id: String,
) -> Result<serde_json::Value, String> {
    // A delete reaches the thread's machine and leaves the selected one. A
    // runtime that holds a synced copy sends the delete on to its owner.
    // The answer names the thread's folder when it stays with the user's files.
    let session = attach_state.thread_session_within(&thread_id, HISTORY_CONNECT_WAIT);
    #[cfg(target_os = "macos")]
    require_runtime(&session)?;
    let subject = chat_subject(&app_handle, &auth_state)?;
    let answer = delete_thread_command(
        session.into(),
        &state.session_thread,
        subject.as_deref(),
        &thread_id,
    )?;
    // Other machines' copies stay out of the list until the owner's delete
    // reaches them.
    attach_state.remember_deleted_threads(vec![thread_id]);
    Ok(answer)
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
        let thread = crate::projects::prepare_thread(
            &app_handle,
            &state,
            subject.as_deref(),
            project.as_deref(),
            true,
        )?;
        crate::projects::seed_thread(&app_handle, &thread, project.as_deref()).await;
        thread
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
    // Selecting the thread chose its machine. A history read, such as a
    // refresh after the list changed, reads that machine and selects none.
    let session = attach_state.thread_session_within(&thread_id, HISTORY_CONNECT_WAIT);
    #[cfg(target_os = "macos")]
    require_runtime(&session)?;
    // An account that cannot name its subject, or a missing profile, gets that
    // error before the runtime call.
    chat_subject(&app_handle, &auth_state)?;
    state_session_root(&app_handle)?;
    chat_thread_open_command(session, &thread_id, limit, cursor.as_deref())
        .map_err(|error| crate::runtimes::name_unreachable(&app_handle, error))
}

#[cfg(any(unix, target_os = "windows"))]
fn chat_run_detail_command(
    attach_state: &AttachCompanionState,
    run_id: &str,
    effect_id: &str,
    kind: &str,
) -> Result<serde_json::Value, String> {
    match attach_state.desktop_client_session() {
        DesktopClientSession::NoSupervisor | DesktopClientSession::Disconnected => {
            Err(auth::background_service_error())
        }
        DesktopClientSession::Connected(client) => client
            .run_detail(run_id, effect_id, kind)
            .map_err(desktop_thread_history_error),
    }
}

/// Reads one tool output or one applied diff that chat events leave out.
#[cfg(any(unix, target_os = "windows"))]
#[tauri::command]
pub async fn chat_run_detail(
    app_handle: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    attach_state: tauri::State<'_, AttachCompanionState>,
    run_id: String,
    effect_id: String,
    kind: String,
) -> Result<serde_json::Value, String> {
    #[cfg(target_os = "macos")]
    require_runtime(&attach_state.desktop_client_session())?;
    // An account that cannot name its subject gets that error before the runtime call.
    chat_subject(&app_handle, &auth_state)?;
    chat_run_detail_command(&attach_state, &run_id, &effect_id, &kind)
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

/// The client of the runtime `thread_id` lives on, or of the selected
/// runtime for `None`.
#[cfg(any(unix, target_os = "windows"))]
async fn context_client(
    app_handle: &tauri::AppHandle,
    thread_id: Option<String>,
) -> Result<muniment_core::attach::DesktopClientHolder, String> {
    let waiting_app = app_handle.clone();
    let session = tauri::async_runtime::spawn_blocking(move || {
        let attach = waiting_app.state::<AttachCompanionState>();
        match thread_id {
            Some(thread_id) => attach.thread_session_within(&thread_id, HISTORY_CONNECT_WAIT),
            None => attach.desktop_client_session_within(HISTORY_CONNECT_WAIT),
        }
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
    let client = context_client(&app_handle, None).await?;
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
    let client = context_client(&app_handle, Some(thread_id.clone())).await?;
    tauri::async_runtime::spawn_blocking(move || {
        let snapshot = poll_runtime(
            CONTEXT_READ_WAIT,
            "The context usage did not arrive in time. Try again.",
            || {
                client
                    .thread_context_usage(&thread_id)
                    .map_err(|(error, failure)| {
                        auth::desktop_request_error(error, failure.as_ref())
                    })
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
    let client = context_client(&app_handle, Some(thread_id.clone())).await?;
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
                    .map_err(|(error, failure)| {
                        auth::desktop_request_error(error, failure.as_ref())
                    })
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
        fn delete_thread(&self, thread_id: &str) -> Result<serde_json::Value, ClientError> {
            self.calls.lock().unwrap().push(thread_id.to_owned());
            self.result.map(|()| {
                if thread_id == "kept-thread" {
                    serde_json::json!({"folder_kept": "/home/me/sessions/Kept-1", "message": "kept"})
                } else {
                    serde_json::json!({})
                }
            })
        }
    }

    #[cfg(any(unix, target_os = "windows"))]
    #[test]
    fn a_connected_delete_answers_the_folder_that_stays() {
        let tracker = SessionThread::default();
        let client = FakeThreadDeleteClient {
            calls: Arc::new(Mutex::new(Vec::new())),
            result: Ok(()),
        };
        let answer = delete_thread_command(
            ThreadDeleteSession::Connected(client),
            &tracker,
            Some("owner"),
            "kept-thread",
        )
        .unwrap();
        assert_eq!(answer["folder_kept"], "/home/me/sessions/Kept-1");
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
            rename_thread_command(attach_state.desktop_client_session(), thread_id, title)
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
            .map(drop)
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
            chat_thread_open_command(attach_state.desktop_client_session(), thread_id, limit, cursor)
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
        assert_eq!(
            compaction_outcome(&answer("running", "run-1"), "run-1"),
            None
        );
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

    #[cfg(any(unix, target_os = "windows"))]
    #[test]
    fn thread_pages_merge_newest_first_with_each_runtime_named() {
        let merged = merge_thread_pages(
            vec![
            RuntimePage {
                runtime_id: "local".into(),
                page: serde_json::json!({
                    "summaries": [
                        {"threadId": "a", "updatedAt": "2026-10-15T10:00:00Z"},
                        {"threadId": "b", "updatedAt": "2026-10-13T10:00:00Z"}
                    ],
                    "nextCursor": "local-2"
                }),
            },
            RuntimePage {
                runtime_id: "box".into(),
                page: serde_json::json!({
                    "summaries": [{"threadId": "c", "updatedAt": "2026-10-14T10:00:00Z"}],
                    "nextCursor": null
                }),
            },
        ],
            &Default::default(),
        );
        let order: Vec<(&str, &str)> = merged["summaries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|summary| {
                (
                    summary["threadId"].as_str().unwrap(),
                    summary["runtimeId"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(order, vec![("a", "local"), ("c", "box"), ("b", "local")]);
        assert_eq!(merged["runtimeIds"], serde_json::json!(["local", "box"]));
        let cursor = merged["nextCursor"].as_str().unwrap();
        assert_eq!(
            merged_cursor_targets(Some(cursor), &[]).unwrap(),
            vec![("local".to_owned(), Some("local-2".to_owned()))]
        );
        assert_eq!(
            merged_cursor_targets(None, &["local".to_owned(), "box".to_owned()]).unwrap(),
            vec![("local".to_owned(), None), ("box".to_owned(), None)]
        );
        assert!(merged_cursor_targets(Some("not json"), &[]).is_err());

        let last = merge_thread_pages(
            vec![RuntimePage {
                runtime_id: "local".into(),
                page: serde_json::json!({"summaries": [], "nextCursor": null}),
            }],
            &Default::default(),
        );
        assert!(last["nextCursor"].is_null());
    }

    #[cfg(any(unix, target_os = "windows"))]
    #[test]
    fn a_paired_copy_of_a_thread_deleted_here_stays_out_of_the_list() {
        let pages = vec![
            RuntimePage {
                runtime_id: "local".into(),
                page: serde_json::json!({
                    "summaries": [{"threadId": "kept", "updatedAt": "2026-10-31T10:00:00Z"}],
                    "nextCursor": null,
                    "deletedThreadIds": ["gone"]
                }),
            },
            RuntimePage {
                runtime_id: "hub".into(),
                page: serde_json::json!({
                    "summaries": [
                        {"threadId": "gone", "updatedAt": "2026-10-31T12:00:00Z", "owner_runtime": "core-local", "replica": true},
                        {"threadId": "hub-own", "updatedAt": "2026-10-30T12:00:00Z"}
                    ],
                    "nextCursor": null
                }),
            },
        ];
        let deleted: std::collections::HashSet<String> = local_deleted_threads(&pages)
            .unwrap()
            .into_iter()
            .collect();
        let merged = merge_thread_pages(pages, &deleted);
        let listed: Vec<&str> = merged["summaries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|summary| summary["threadId"].as_str().unwrap())
            .collect();
        assert_eq!(listed, vec!["kept", "hub-own"]);

        // A page without this computer's runtime names no deletes, so the
        // ones it named before still hold; an older runtime names none.
        let hub_only = vec![RuntimePage {
            runtime_id: "hub".into(),
            page: serde_json::json!({"summaries": [], "nextCursor": null}),
        }];
        assert_eq!(local_deleted_threads(&hub_only), None);
        let older = vec![RuntimePage {
            runtime_id: "local".into(),
            page: serde_json::json!({"summaries": [], "nextCursor": null}),
        }];
        assert_eq!(local_deleted_threads(&older), Some(Vec::new()));
    }

    #[cfg(any(unix, target_os = "windows"))]
    #[test]
    fn a_thread_listed_by_its_owner_and_as_a_replica_lives_on_its_owner() {
        let listed = vec![
            serde_json::json!({"threadId": "t", "runtimeId": "box", "owner_runtime": "core-box", "replica": false}),
            serde_json::json!({"threadId": "t", "runtimeId": "local", "owner_runtime": "core-box", "replica": true}),
            serde_json::json!({"threadId": "only-here", "runtimeId": "local", "owner_runtime": "core-box", "replica": true}),
            serde_json::json!({"threadId": "copies", "runtimeId": "hub", "owner_runtime": "core-box", "replica": true}),
            serde_json::json!({"threadId": "copies", "runtimeId": "box", "owner_runtime": "core-box", "replica": true}),
        ];
        let map: std::collections::HashMap<_, _> = thread_runtimes(&listed, None).into_iter().collect();
        assert_eq!(map["t"], "box");
        assert_eq!(map["only-here"], "local");
        // Two paired copies of a thread whose owner is away: the same one each
        // time, whichever lists first.
        assert_eq!(map["copies"], "box");
        let mut reversed = listed.clone();
        reversed.reverse();
        let again: std::collections::HashMap<_, _> = thread_runtimes(&reversed, None).into_iter().collect();
        assert_eq!(again, map);
    }

    #[cfg(any(unix, target_os = "windows"))]
    fn page(runtime_id: &str, summaries: serde_json::Value) -> RuntimePage {
        RuntimePage {
            runtime_id: runtime_id.into(),
            page: serde_json::json!({"summaries": summaries, "nextCursor": null}),
        }
    }

    #[cfg(any(unix, target_os = "windows"))]
    #[test]
    fn a_list_this_computer_did_not_answer_never_moves_its_threads_or_the_selection() {
        let attach = AttachCompanionState::default();
        let ours = serde_json::json!({"threadId": "ours", "updatedAt": "2026-10-31T10:00:00Z", "owner_runtime": "core-mac"});
        let hub_copy = serde_json::json!({"threadId": "ours", "updatedAt": "2026-10-31T11:00:00Z", "owner_runtime": "core-mac", "replica": true});
        record_thread_list(
            &attach,
            vec![page("local", serde_json::json!([ours])), page("hub", serde_json::json!([hub_copy]))],
        );
        assert_eq!(attach.thread_runtime("ours").as_deref(), Some("local"));
        assert_eq!(attach.local_fleet_id().as_deref(), Some("core-mac"));

        // This computer's runtime is busy past the wait, so only the hub
        // answers with its copy. The thread stays here.
        let merged = record_thread_list(&attach, vec![page("hub", serde_json::json!([hub_copy]))]);
        assert_eq!(merged["summaries"][0]["runtimeId"], "hub");
        assert_eq!(attach.thread_runtime("ours").as_deref(), Some("local"));

        // A thread this computer runs that only the hub lists, such as one
        // past this computer's first page, lives here too.
        let older = serde_json::json!({"threadId": "older", "updatedAt": "2026-10-01T10:00:00Z", "owner_runtime": "core-mac", "replica": true});
        record_thread_list(&attach, vec![page("local", serde_json::json!([ours])), page("hub", serde_json::json!([older]))]);
        assert_eq!(attach.thread_runtime("older").as_deref(), Some("local"));

        // An action on a hub thread reaches the hub's session and selects
        // nothing: the hub has no connection here, so the session says so.
        attach.remember_thread_runtimes([("hub-own".to_owned(), "hub".to_owned())]);
        assert!(matches!(
            attach.thread_session_within("hub-own", std::time::Duration::ZERO),
            DesktopClientSession::Disconnected
        ));
        assert_eq!(attach.remote_runtime_id(), None);
    }

    #[cfg(any(unix, target_os = "windows"))]
    #[test]
    fn a_launch_list_the_hub_answers_first_never_sends_this_computers_thread_to_the_hub() {
        let attach = AttachCompanionState::default();
        // At launch the hub connects first, and this computer's runtime has
        // not named its fleet id yet. The hub lists its synced copy of this
        // computer's thread and a thread it runs itself.
        let hub_copy = serde_json::json!({"threadId": "ours", "updatedAt": "2026-10-31T11:00:00Z", "owner_runtime": "core-mac", "replica": true});
        let hub_own = serde_json::json!({"threadId": "hub-own", "updatedAt": "2026-10-31T10:00:00Z", "owner_runtime": "core-hub"});
        let merged = record_thread_list(&attach, vec![page("hub", serde_json::json!([hub_copy, hub_own]))]);
        assert_eq!(merged["summaries"].as_array().map(Vec::len), Some(2));
        assert_eq!(attach.thread_runtime("ours"), None);
        assert_eq!(attach.thread_runtime("hub-own").as_deref(), Some("hub"));
        // A select of the copy reaches this computer, the active runtime,
        // and leaves the hub unselected.
        let (runtime, session) = select_thread_session(&attach, "ours");
        assert_eq!(runtime, "local");
        assert!(matches!(session, ThreadSelectSession::NoSupervisor));
        assert_eq!(attach.remote_runtime_id(), None);

        // This computer's runtime answers and names itself, so the thread lives here.
        let ours = serde_json::json!({"threadId": "ours", "updatedAt": "2026-10-31T10:00:00Z", "owner_runtime": "core-mac"});
        record_thread_list(&attach, vec![page("local", serde_json::json!([ours])), page("hub", serde_json::json!([hub_copy]))]);
        assert_eq!(attach.thread_runtime("ours").as_deref(), Some("local"));

        // Once the fleet id is known, a later list the hub answers alone
        // records the copies it names again, by owner.
        let later = serde_json::json!({"threadId": "later", "updatedAt": "2026-10-31T12:00:00Z", "owner_runtime": "core-mac", "replica": true});
        record_thread_list(&attach, vec![page("hub", serde_json::json!([later]))]);
        assert_eq!(attach.thread_runtime("later").as_deref(), Some("local"));
    }

    #[cfg(any(unix, target_os = "windows"))]
    #[test]
    fn a_select_reaches_the_runtime_a_new_thread_was_created_on() {
        let attach = AttachCompanionState::default();
        // This computer is active when the new thread starts, so its runtime
        // holds the thread. A thread with no run lists nowhere yet.
        attach.remember_created_thread("red-trek");
        // The user then opens a thread on the paired hub, which becomes active.
        attach.set_active(Some("hub".into()));

        // A select of the new thread with no runtime id reaches this
        // computer's runtime, not the active hub.
        let (runtime, session) = select_thread_session(&attach, "red-trek");
        assert_eq!(runtime, "local");
        assert!(matches!(session, ThreadSelectSession::NoSupervisor));

        // A thread no list, select or creation named reaches the active hub.
        let (runtime, session) = select_thread_session(&attach, "unknown");
        assert_eq!(runtime, "hub");
        assert!(matches!(session, ThreadSelectSession::Disconnected));

        // A thread created while the hub is active lives on the hub.
        attach.remember_created_thread("hub-new");
        attach.set_active(None);
        let (runtime, session) = select_thread_session(&attach, "hub-new");
        assert_eq!(runtime, "hub");
        assert!(matches!(session, ThreadSelectSession::Disconnected));
    }

    #[cfg(any(unix, target_os = "windows"))]
    #[test]
    fn a_failed_select_logs_its_thread_and_runtime() {
        assert_eq!(
            thread_select_failure_record(
                "019a0000-0000-7000-8000-00000000000a",
                "local",
                "The request could not be committed.",
            ),
            "muniment-desktop: thread select failed \
             thread_id=\"019a0000-0000-7000-8000-00000000000a\" runtime_id=\"local\" \
             error=\"The request could not be committed.\""
        );
        let long = "x".repeat(4096);
        let record = thread_select_failure_record(&long, "hub\nforged=1", &long);
        assert!(!record.contains('\n'));
        assert!(record.len() < 800);
    }

    #[cfg(any(unix, target_os = "windows"))]
    #[test]
    fn a_thread_deleted_here_leaves_every_later_list() {
        let attach = AttachCompanionState::default();
        attach.remember_deleted_threads(vec!["gone".into()]);
        // This computer's next page names no deletes of its own.
        let merged = record_thread_list(
            &attach,
            vec![
                page("local", serde_json::json!([])),
                page("hub", serde_json::json!([{"threadId": "gone", "updatedAt": "2026-10-31T11:00:00Z", "owner_runtime": "core-box", "replica": true}])),
            ],
        );
        assert_eq!(merged["summaries"], serde_json::json!([]));
        assert_eq!(merged["deletedThreadIds"], serde_json::json!(["gone"]));
    }
}
