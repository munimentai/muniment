use super::*;

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

/// Reads a user-selected local text file for the file panel. Relative tool paths
/// resolve against the same Home directory as the agent's tools.
#[tauri::command]
pub async fn chat_file_content(path: PathBuf) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        use std::io::Read;
        let path = if path.is_absolute() {
            path
        } else {
            let profile = muniment_runtime::profile_directory()
                .map_err(|_| "The Home folder is unavailable.")?;
            muniment_core::launch_facts::working_directory(&profile)
                .ok_or("The Home folder is unavailable.")?
                .join(path)
        };
        let kind = std::fs::metadata(&path).map_err(|_| "This file is no longer available.")?;
        if !kind.is_file() {
            return Err("Choose a regular file.");
        }
        let file = std::fs::File::open(path).map_err(|_| "This file is no longer available.")?;
        let metadata = file.metadata().map_err(|_| "This file cannot be read.")?;
        if !metadata.is_file() {
            return Err("Choose a regular file.");
        }
        const LIMIT: u64 = 512 * 1024;
        if metadata.len() > LIMIT {
            return Err("This file is too large to preview (512 KB maximum).");
        }
        let mut bytes = Vec::new();
        file.take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "This file cannot be read.")?;
        if bytes.len() as u64 > LIMIT {
            return Err("This file is too large to preview (512 KB maximum).");
        }
        if bytes.contains(&0) {
            return Err("This binary file has no text preview.");
        }
        String::from_utf8(bytes).map_err(|_| "This file is not UTF-8 text.")
    })
    .await
    .map_err(|_| "This file cannot be read.".to_string())?
    .map_err(str::to_owned)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposerFile {
    path: String,
    display_name: String,
    relative_path: String,
}

/// Searches names only. Hidden folders, build output and symlinks are excluded.
#[tauri::command]
pub async fn chat_search_files(query: String) -> Result<Vec<ComposerFile>, String> {
    if query.chars().count() > 256 { return Ok(Vec::new()); }
    tauri::async_runtime::spawn_blocking(move || {
        let profile = muniment_runtime::profile_directory().map_err(|_| "The Home folder is unavailable.")?;
        let root = muniment_core::launch_facts::working_directory(&profile).ok_or("The Home folder is unavailable.")?;
        Ok(search_home_files(&root, &query))
    }).await.map_err(|_| "File search could not finish.".to_owned())?
}

pub(super) fn search_home_files(root: &std::path::Path, query: &str) -> Vec<ComposerFile> {
    let query = query.trim().to_lowercase();
    let mut stack = vec![(root.to_owned(), 0)];
    let mut results = Vec::new();
    let mut visited = 0;
    let started = std::time::Instant::now();
    while let Some((directory, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(directory) else { continue; };
        for entry in entries.flatten() {
            visited += 1;
            if visited > 20000 || started.elapsed() > std::time::Duration::from_millis(200) { return results; }
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || matches!(name.as_str(), "node_modules" | "target" | "dist" | "vendor") { continue; }
            let Ok(kind) = entry.file_type() else { continue; };
            let path = entry.path();
            if kind.is_dir() && depth < 8 { stack.push((path, depth + 1)); }
            else if kind.is_file() {
                let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().into_owned();
                if relative.to_lowercase().contains(&query) {
                    results.push(ComposerFile { path: path.to_string_lossy().into_owned(), display_name: name, relative_path: relative });
                    if results.len() == 50 { return results; }
                }
            }
        }
    }
    results.sort_by(|a,b| a.relative_path.cmp(&b.relative_path));
    results
}

pub(super) fn open_selected_files(
    files: Vec<SelectedFile>,
) -> Result<Vec<OpenSelectedFile>, String> {
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

#[allow(dead_code)]
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

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_new_run_with_session_thread_after_validation<F>(
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
    core_run_preparation::prepare_new_run_with_session_thread(
        storage,
        session_thread,
        run_id,
        workspace,
        subject,
        files,
        provenance,
        "muniment-desktop",
        env!("CARGO_PKG_VERSION"),
        after_validation,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_new_run_in_thread_after_validation<F>(
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
    core_run_preparation::prepare_new_run_in_thread_after_validation(
        storage,
        run_id,
        workspace,
        subject,
        files,
        provenance,
        thread_id,
        "muniment-desktop",
        env!("CARGO_PKG_VERSION"),
        after_validation,
    )
}

#[allow(clippy::too_many_arguments, dead_code)]
pub(super) fn prepare_opened_run<F>(
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
    core_run_preparation::prepare_opened_run(
        storage,
        session_thread,
        run_id,
        workspace,
        subject,
        files,
        provenance,
        requested_thread_id,
        "muniment-desktop",
        env!("CARGO_PKG_VERSION"),
        after_validation,
    )
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

#[allow(dead_code)]
pub(crate) fn event_envelope(
    run_id: &str,
    run_seq: u64,
    kind: &str,
    payload: Value,
    subject: Option<&str>,
) -> EventEnvelope {
    core_run_preparation::event_envelope(
        run_id,
        run_seq,
        kind,
        payload,
        subject,
        "muniment-desktop",
        env!("CARGO_PKG_VERSION"),
    )
}

pub(crate) fn desktop_provenance(subject: Option<&str>) -> Provenance {
    core_run_preparation::desktop_provenance(subject, "muniment-desktop", env!("CARGO_PKG_VERSION"))
}

pub(super) fn fetch_grant_error_message(error: FetchGrantError) -> String {
    error.into_message()
}

pub(super) fn map_fetch_grant_error(error: FetchGrantError) -> RunStartError {
    match error {
        error @ FetchGrantError::NotEntitled { .. } => {
            RunStartError::Unauthorized(error.into_message())
        }
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

pub(super) fn fetch_grant(_access_token: &str) -> Result<ChatGrant, FetchGrantError> {
    // Cloud runs use run.submit on the runtime, including grant recovery.
    Err(FetchGrantError::Unavailable)
}

pub(super) fn validate_grant(grant: &ChatGrant) -> Result<(), String> {
    core_validate_grant(grant).map_err(fetch_grant_error_message)
}
