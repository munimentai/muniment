use muniment_core::projects;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

fn profile() -> Result<std::path::PathBuf, String> {
    muniment_runtime::profile_directory().map_err(|_| "The project catalog is unavailable.".into())
}

#[tauri::command]
pub fn project_list() -> Result<projects::Catalog, String> {
    projects::list(&profile()?)
}
#[tauri::command]
pub fn project_create(name: String) -> Result<String, String> {
    projects::create(&profile()?, &name)
}
#[tauri::command]
pub fn project_rename(project_id: String, name: String) -> Result<(), String> {
    projects::rename(&profile()?, &project_id, &name)
}
#[tauri::command]
pub fn project_open(app: AppHandle, project_id: String) -> Result<(), String> {
    let folder = projects::folder(&profile()?, &project_id)?;
    app.opener()
        .open_path(folder.to_string_lossy(), None::<String>)
        .map_err(|_| "The project folder could not be opened.".into())
}

pub(crate) fn prepare_thread(
    app: &AppHandle,
    state: &crate::chat::ChatState,
    subject: Option<&str>,
    project_id: Option<&str>,
    fresh: bool,
) -> Result<String, String> {
    let profile = profile()?;
    if !fresh {
        if let Some(thread) = state.session_thread.current(subject) {
            projects::workspace(&profile, &thread)?;
            return Ok(thread);
        }
    }
    if let Some(project) = project_id {
        projects::folder(&profile, project)?;
    }
    let attach = app.state::<crate::attach_service::AttachCompanionState>();
    let client = match attach.desktop_client_session() {
        crate::attach_service::DesktopClientSession::Connected(client) => client,
        #[cfg(target_os = "linux")]
        crate::attach_service::DesktopClientSession::NoSupervisor => {
            if fresh {
                *state
                    .pending_project
                    .lock()
                    .map_err(|_| "The project choice is unavailable.")? =
                    project_id.map(str::to_owned);
                state.session_thread.fresh(subject);
            }
            return Ok(String::new());
        }
        _ => return Err(crate::auth::background_service_error()),
    };
    let thread = client
        .create_thread()
        .map_err(crate::auth::desktop_client_error)?;
    let workspace = match project_id {
        Some(project) => projects::assign(&profile, &thread, project)
            .and_then(|_| projects::workspace(&profile, &thread)),
        None => projects::workspace(&profile, &thread),
    };
    if let Err(error) = workspace {
        let _ = client.delete_thread(&thread);
        return Err(error);
    }
    state.session_thread.select(thread.clone(), subject);
    Ok(thread)
}
