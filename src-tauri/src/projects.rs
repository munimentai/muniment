use muniment_core::{projects, sidebar};
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
pub fn project_settings(project_id: String) -> Result<projects::Settings, String> {
    projects::settings(&profile()?, &project_id)
}
#[tauri::command]
pub fn project_settings_save(
    project_id: String,
    settings: projects::Settings,
) -> Result<(), String> {
    projects::save_settings(&profile()?, &project_id, &settings)
}
/// The sidebar's sections, arranged by core from what the window knows.
#[tauri::command]
pub fn sidebar_arrange(input: sidebar::Input) -> sidebar::Sidebar {
    sidebar::arrange(&input)
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
        _ => return Err(crate::auth::background_service_error()),
    };
    let thread = client
        .create_thread()
        .map_err(crate::auth::desktop_client_error)?;
    if let Err(error) = ready_new_thread_folder(&profile, &thread, project_id) {
        let _ = client.delete_thread(&thread);
        return Err(error);
    }
    state.session_thread.select(thread.clone(), subject);
    Ok(thread)
}

/// Readies a new thread's folder. A project thread checks its project folder
/// now. A plain thread gets its session folder at its first run, so a thread
/// left without a message leaves no empty session folder.
fn ready_new_thread_folder(
    profile: &std::path::Path,
    thread: &str,
    project_id: Option<&str>,
) -> Result<(), String> {
    let Some(project) = project_id else {
        return Ok(());
    };
    projects::assign(profile, thread, project)?;
    projects::workspace(profile, thread).map(drop)
}

#[cfg(test)]
mod tests {
    use super::ready_new_thread_folder;

    #[test]
    fn a_new_plain_thread_creates_no_session_folder() {
        let root =
            std::env::temp_dir().join(format!("muniment-new-thread-{}", uuid::Uuid::new_v4()));
        let (profile, home) = (root.join("profile"), root.join("home"));
        std::fs::create_dir_all(&profile).unwrap();
        muniment_core::home::confirm_home(&profile, &home).unwrap();
        let thread = "01900000-0000-7000-8000-000000000001";

        ready_new_thread_folder(&profile, thread, None).unwrap();
        let folders = std::fs::read_dir(home.join("sessions"))
            .unwrap()
            .filter(|entry| entry.as_ref().unwrap().path().is_dir())
            .count();
        assert_eq!(folders, 0);

        let project = muniment_core::projects::create(&profile, "Research").unwrap();
        ready_new_thread_folder(&profile, thread, Some(&project)).unwrap();
        assert_eq!(
            muniment_core::projects::thread_folder(&profile, thread).unwrap(),
            Some(muniment_core::projects::folder(&profile, &project).unwrap())
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
