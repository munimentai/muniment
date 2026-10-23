use super::*;

/// Checks a file or folder the composer attaches. A folder in Documents or
/// another protected place can wait on a macOS prompt, so the check runs off
/// the main thread.
#[tauri::command]
pub async fn chat_file_metadata(path: PathBuf) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || file_metadata(&path))
        .await
        .map_err(|_| attachment_error())?
}

fn file_metadata(path: &std::path::Path) -> Result<serde_json::Value, String> {
    let metadata = std::fs::metadata(path).map_err(|_| attachment_error())?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(attachment_error());
    }
    if metadata.is_dir() {
        std::fs::read_dir(path).map_err(|_| attachment_error())?;
    } else {
        std::fs::File::open(path).map_err(|_| attachment_error())?;
    }
    let display_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(attachment_error)?
        .to_owned();
    Ok(serde_json::json!({
        "displayName": display_name,
        "byteLength": if metadata.is_dir() { 0 } else { metadata.len() },
        "mediaType": null,
        "isDirectory": metadata.is_dir(),
    }))
}

/// Reads a user-selected local text file for the file panel. Relative tool paths
/// resolve against the same Home directory as the agent's tools.
#[tauri::command]
pub async fn chat_file_content(path: PathBuf, thread_id: Option<String>) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        use std::io::Read;
        let path = if path.is_absolute() {
            path
        } else {
            let profile = muniment_runtime::profile_directory()
                .map_err(|_| "The Home folder is unavailable.")?;
            project_working_directory(&profile, thread_id.as_deref()).map_err(|_| "The project folder is unavailable.")?.join(path)
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
    .map_err(|error| error.to_string())
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
pub async fn chat_search_files(query: String, thread_id: Option<String>, project_id: Option<String>, directory: Option<PathBuf>) -> Result<Vec<ComposerFile>, String> {
    if query.chars().count() > 256 { return Ok(Vec::new()); }
    tauri::async_runtime::spawn_blocking(move || {
        let profile = muniment_runtime::profile_directory().map_err(|_| "The Home folder is unavailable.")?;
        let root = if let Some(path) = directory {
            crate::workspace_tools::directory(&path)?
        } else if let Some(id) = project_id {
            muniment_core::projects::folder(&profile, &id)?
        } else { project_working_directory(&profile, thread_id.as_deref())? };
        Ok(search_home_files(&root, &query))
    }).await.map_err(|_| "File search could not finish.".to_owned())?
}

fn project_working_directory(profile: &std::path::Path, thread: Option<&str>) -> Result<PathBuf, String> {
    if let Some(thread) = thread {
        return muniment_core::projects::workspace(profile, thread);
    }
    muniment_core::launch_facts::working_directory(profile).ok_or_else(|| "The Home folder is unavailable.".into())
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

#[cfg(test)]
mod folder_selection_tests {
    #[test]
    fn metadata_distinguishes_readable_folders_from_files() {
        let root = std::env::temp_dir().join(format!("muniment-folder-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir(&root).unwrap();
        let file = root.join("brief.txt");
        std::fs::write(&file, "brief").unwrap();
        let metadata = |path| tauri::async_runtime::block_on(super::chat_file_metadata(path));
        let folder = metadata(root.clone()).unwrap();
        assert_eq!(folder["isDirectory"], true);
        assert_eq!(folder["byteLength"], 0);
        assert_eq!(metadata(file).unwrap()["isDirectory"], false);
        assert!(metadata(root.join("missing")).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
