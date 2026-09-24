use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri_plugin_opener::OpenerExt;

pub(crate) fn shell(webview: &tauri::Webview) -> Result<(), String> {
    if webview.label() == "main" {
        Ok(())
    } else {
        Err("Only the desktop can use workspace tools.".into())
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    label: String,
    path: PathBuf,
}
#[tauri::command]
pub fn workspace_folders(
    webview: tauri::Webview,
    thread_id: Option<String>,
    project_id: Option<String>,
    agent_id: Option<String>,
    artifact_id: Option<String>,
    catalog: Option<String>,
    thread_title: Option<String>,
) -> Result<Vec<Folder>, String> {
    shell(&webview)?;
    let profile = muniment_runtime::profile_directory().map_err(|e| e.to_string())?;
    let home = muniment_core::home::configured_home(&profile)
        .map_err(|e| e.to_string())?
        .ok_or("Choose a Home folder first.")?;
    muniment_core::projects::migrate_session_names(&profile)?;
    let mut folders = vec![Folder {
        label: "Muniment folder".into(),
        path: home.clone(),
    }];
    if let Some(kind) = catalog.filter(|s| matches!(s.as_str(), "agents" | "projects" | "artifacts")) {
        let path = home.join(&kind);
        std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
        folders.push(Folder { label: format!("{} folder", kind), path });
    } else if let Some(id) = artifact_id {
        folders.push(Folder { label: "Artifact folder".into(), path: muniment_core::creations::artifact_folder(&profile, &id)? });
    } else if let Some(id) = agent_id {
        folders.push(Folder { label: "Agent folder".into(), path: muniment_core::agents::folder(&profile, &id)? });
    } else if let Some(id) = project_id {
        folders.push(Folder { label: "Project folder".into(), path: muniment_core::projects::folder(&profile, &id)? });
    } else if let Some(id) = thread_id {
        if let Some(title) = thread_title.filter(|s| !s.trim().is_empty()) {
            muniment_core::workspace_names::resolve(&profile, &home.join("sessions"), &id, Some(&title), None)?;
        }
        folders.push(Folder { label: "Thread workspace".into(), path: muniment_core::projects::workspace(&profile, &id)? });
    }
    Ok(folders)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    name: String,
    path: PathBuf,
    directory: bool,
}
#[derive(Serialize)]
pub struct Listing {
    path: PathBuf,
    parent: Option<PathBuf>,
    entries: Vec<Entry>,
}
pub(crate) fn directory(path: &Path) -> Result<PathBuf, String> {
    let path = path
        .canonicalize()
        .map_err(|_| "This folder is unavailable.")?;
    if !path.is_dir() {
        return Err("Choose a folder.".into());
    }
    Ok(path)
}
fn list(path: &Path) -> Result<Listing, String> {
    let path = directory(path)?;
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&path).map_err(|e| e.to_string())? {
        if entries.len() >= 10000 {
            return Err(
                "This folder has more than 10,000 entries. Choose a smaller folder.".into(),
            );
        }
        let entry = entry.map_err(|e| e.to_string())?;
        let metadata = entry.metadata().map_err(|e| e.to_string())?;
        entries.push(Entry {
            name: entry.file_name().to_string_lossy().into_owned(),
            path: entry.path(),
            directory: metadata.is_dir(),
        });
    }
    entries.sort_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(Listing {
        parent: path.parent().map(Path::to_path_buf),
        path,
        entries,
    })
}
#[tauri::command]
pub async fn workspace_list(webview: tauri::Webview, path: PathBuf) -> Result<Listing, String> {
    shell(&webview)?;
    tauri::async_runtime::spawn_blocking(move || list(&path))
        .await
        .map_err(|e| e.to_string())?
}
#[cfg(target_os = "macos")]
const REVEAL: &str = "Reveal in Finder";
#[cfg(windows)]
const REVEAL: &str = "Reveal in File Explorer";
#[cfg(not(any(target_os = "macos", windows)))]
const REVEAL: &str = "Reveal in File Manager";

/// Extensions the OS default handler runs as a program or installer.
#[cfg(target_os = "macos")]
fn runnable_extension(extension: &str) -> bool {
    matches!(
        extension,
        "app"
            | "command"
            | "tool"
            | "sh"
            | "pkg"
            | "mpkg"
            | "dmg"
            | "terminal"
            | "workflow"
            | "jar"
            | "scpt"
            | "applescript"
            | "fileloc"
            | "inetloc"
            | "py"
    )
}
#[cfg(windows)]
fn runnable_extension(extension: &str) -> bool {
    let pathext = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD;.VBS;.VBE;.JS;.JSE;.WSF;.WSH;.MSC".into());
    pathext
        .split(';')
        .filter_map(|entry| entry.trim().strip_prefix('.'))
        .any(|entry| entry.eq_ignore_ascii_case(extension))
        || matches!(
            extension,
            "com"
                | "exe"
                | "bat"
                | "cmd"
                | "lnk"
                | "url"
                | "msi"
                | "msp"
                | "scr"
                | "hta"
                | "jar"
                | "pif"
                | "cpl"
                | "reg"
                | "appref-ms"
                | "application"
                | "ps1"
                | "py"
                | "pyw"
        )
}
#[cfg(not(any(target_os = "macos", windows)))]
fn runnable_extension(extension: &str) -> bool {
    matches!(extension, "desktop" | "sh" | "appimage" | "run")
}

/// Refuses a path the OS default handler would run instead of display: a
/// program, script, installer, shortcut or app bundle, by name or execute bit.
/// The execute bit counts only without an extension, because the handler picks
/// an app by extension, and FAT and exFAT volumes set the bit on every file.
fn launchable(path: &Path, metadata: &std::fs::Metadata) -> bool {
    let extension = path.extension().and_then(|e| e.to_str());
    let runnable = extension.is_some_and(|e| runnable_extension(&e.to_ascii_lowercase()));
    #[cfg(unix)]
    let executable = {
        use std::os::unix::fs::PermissionsExt;
        extension.is_none() && metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    };
    #[cfg(not(unix))]
    let executable = {
        let _ = metadata;
        false
    };
    runnable || executable
}
fn openable(path: &Path) -> Result<PathBuf, String> {
    let canonical = path
        .canonicalize()
        .map_err(|_| "This file is unavailable.")?;
    let metadata = std::fs::metadata(&canonical).map_err(|_| "This file is unavailable.")?;
    if launchable(path, &metadata) || launchable(&canonical, &metadata) {
        return Err(format!(
            "This file can run programs, so Muniment does not open it. Use {REVEAL} to show it in its folder."
        ));
    }
    Ok(canonical)
}
#[tauri::command]
pub fn workspace_open(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    path: PathBuf,
) -> Result<(), String> {
    shell(&webview)?;
    let path = openable(&path)?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<String>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn workspace_image(webview: tauri::Webview, path: PathBuf) -> Result<String, String> {
    shell(&webview)?;
    tauri::async_runtime::spawn_blocking(move || {
        use base64::Engine;
        use std::io::Read;
        let mime = match path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "svg" => "image/svg+xml",
            "ico" => "image/x-icon",
            "avif" => "image/avif",
            _ => return Err("This image type has no preview.".into()),
        };
        let file = std::fs::File::open(path).map_err(|_| "This image is unavailable.")?;
        if !file
            .metadata()
            .map_err(|_| "This image is unavailable.")?
            .is_file()
        {
            return Err("Choose a regular file.".into());
        }
        let mut bytes = Vec::new();
        file.take(10_000_001)
            .read_to_end(&mut bytes)
            .map_err(|_| "This image cannot be read.")?;
        if bytes.len() > 10_000_000 {
            return Err("This image exceeds 10 MB. Open it in the default app.".into());
        }
        Ok(format!(
            "data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn web_url(value: &str) -> Result<url::Url, String> {
    let url = url::Url::parse(value).map_err(|_| "This link is invalid.")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Use an HTTP or HTTPS link without credentials.".into());
    }
    Ok(url)
}
fn save_link(address: &str, destination: &Path) -> Result<(), String> {
    use std::io::Read;
    let mut url = web_url(address)?;
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(std::time::Duration::from_secs(120))
        .build();
    let response = {
        let mut redirects = 0;
        loop {
            let response = agent
                .get(url.as_str())
                .call()
                .map_err(|e| format!("The link could not be saved: {e}"))?;
            if (300..400).contains(&response.status()) {
                redirects += 1;
                if redirects > 5 {
                    return Err("This link redirects too many times.".into());
                }
                let location = response
                    .header("Location")
                    .ok_or("The redirect has no destination.")?;
                url = web_url(
                    url.join(location)
                        .map_err(|_| "This redirect is invalid.")?
                        .as_str(),
                )?;
            } else {
                break response;
            }
        }
    };
    const LIMIT: u64 = 100_000_000;
    if response
        .header("Content-Length")
        .and_then(|s| s.parse::<u64>().ok())
        .is_some_and(|n| n > LIMIT)
    {
        return Err("This download exceeds 100 MB.".into());
    }
    let parent = directory(destination.parent().ok_or("Choose a save folder.")?)?;
    let name = destination.file_name().ok_or("Choose a file name.")?;
    let destination = parent.join(name);
    let temporary = parent.join(format!(".muniment-download-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        let count = std::io::copy(&mut response.into_reader().take(LIMIT + 1), &mut file)
            .map_err(|e| e.to_string())?;
        if count > LIMIT {
            return Err("This download exceeds 100 MB.".into());
        }
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        std::fs::rename(&temporary, &destination).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}
#[tauri::command]
pub async fn workspace_save_link(
    webview: tauri::Webview,
    url: String,
    path: PathBuf,
) -> Result<(), String> {
    shell(&webview)?;
    tauri::async_runtime::spawn_blocking(move || save_link(&url, &path))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_app_opens_refuse_programs_and_bundles() {
        let root = std::env::temp_dir().join(format!("muniment-open-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let document = root.join("notes.txt");
        std::fs::write(&document, "notes").unwrap();
        assert_eq!(
            openable(&document).unwrap(),
            document.canonicalize().unwrap()
        );
        #[cfg(target_os = "macos")]
        let refused = [
            "Tool.APP",
            "run.command",
            "setup.pkg",
            "disk.dmg",
            "build.sh",
        ];
        #[cfg(windows)]
        let refused = [
            "setup.EXE",
            "run.bat",
            "link.lnk",
            "site.url",
            "setup.msi",
            "page.hta",
        ];
        #[cfg(not(any(target_os = "macos", windows)))]
        let refused = ["app.desktop", "build.sh", "Tool.AppImage"];
        for name in refused {
            let path = root.join(name);
            if name.to_ascii_lowercase().ends_with(".app") {
                std::fs::create_dir(&path).unwrap();
            } else {
                std::fs::write(&path, "").unwrap();
            }
            let error = openable(&path).unwrap_err();
            assert!(error.contains(REVEAL), "{name}: {error}");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let script = root.join("script");
            std::fs::write(&script, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert!(openable(&script).is_err());
            let alias = root.join("alias.txt");
            std::os::unix::fs::symlink(&script, &alias).unwrap();
            assert!(openable(&alias).is_err());
            let report = root.join("report.pdf");
            std::fs::write(&report, "%PDF").unwrap();
            std::fs::set_permissions(&report, std::fs::Permissions::from_mode(0o777)).unwrap();
            assert!(openable(&report).is_ok());
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(openable(&script).is_ok());
            assert!(openable(&root).is_ok());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn text_saves_preserve_edits_and_reject_stale_revisions() {
        let root = std::env::temp_dir().join(format!("muniment-editor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("code.js");
        std::fs::write(&path, "const x = 1;\r\n").unwrap();
        let first = read_text(&path).unwrap();
        let saved = save_text(&path, "const x = 2;\r\n", &first.revision).unwrap();
        assert_eq!(saved.content, "const x = 2;\r\n");
        assert!(save_text(&path, "stale", &first.revision).is_err());
        assert_eq!(read_text(&path).unwrap().content, saved.content);
        std::fs::write(&path, [0, 1, 2]).unwrap();
        assert!(read_text(&path).is_err());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }
    fn response(status: u16, body: &str, location: Option<&str>) -> String {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://{}/file", server.server_addr());
        let mut response = tiny_http::Response::from_string(body).with_status_code(status);
        if let Some(location) = location {
            response.add_header(tiny_http::Header::from_bytes("Location", location).unwrap());
        }
        std::thread::spawn(move || {
            server.recv().unwrap().respond(response).unwrap();
        });
        url
    }
    #[test]
    fn saved_links_replace_only_after_a_complete_download() {
        let root = std::env::temp_dir().join(format!("muniment-links-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("page.html");
        std::fs::write(&path, "existing").unwrap();
        save_link(&response(200, "<h1>Saved</h1>", None), &path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "<h1>Saved</h1>");
        assert!(save_link(&response(503, "unavailable", None), &path).is_err());
        assert!(save_link(&response(302, "", Some("file:///etc/passwd")), &path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "<h1>Saved</h1>");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[derive(Serialize)]
pub struct TextFile {
    path: PathBuf,
    content: String,
    revision: String,
}
fn read_text(path: &Path) -> Result<TextFile, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Choose a regular file.".into());
    }
    let mut bytes = Vec::new();
    file.take(2_000_001)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 2_000_000 {
        return Err("This file exceeds the 2 MB editor limit.".into());
    }
    let revision = format!("{:x}", Sha256::digest(&bytes));
    let content = String::from_utf8(bytes).map_err(|_| "This file is not UTF-8 text.")?;
    if content.contains('\0') {
        return Err("This file contains binary data.".into());
    }
    Ok(TextFile {
        path,
        content,
        revision,
    })
}
fn save_text(path: &Path, content: &str, revision: &str) -> Result<TextFile, String> {
    use std::io::Write;
    if content.len() > 2_000_000 {
        return Err("This file exceeds the 2 MB editor limit.".into());
    }
    let original = read_text(path)?;
    if original.revision != revision {
        return Err(
            "The file changed on disk. Copy your edits, then reload the file before saving.".into(),
        );
    }
    let path = original.path;
    let temporary = path.with_file_name(format!(".muniment-edit-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        output
            .set_permissions(
                std::fs::metadata(&path)
                    .map_err(|e| e.to_string())?
                    .permissions(),
            )
            .map_err(|e| e.to_string())?;
        output
            .write_all(content.as_bytes())
            .map_err(|e| e.to_string())?;
        output.sync_all().map_err(|e| e.to_string())?;
        drop(output);
        if read_text(&path)?.revision != revision {
            return Err(
                "The file changed on disk. Copy your edits, then reload the file before saving."
                    .into(),
            );
        }
        std::fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
        read_text(&path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}
#[tauri::command]
pub async fn workspace_read_text(
    webview: tauri::Webview,
    path: PathBuf,
    thread_id: Option<String>,
) -> Result<TextFile, String> {
    shell(&webview)?;
    tauri::async_runtime::spawn_blocking(move || {
        let path = if path.is_absolute() {
            path
        } else {
            let profile = muniment_runtime::profile_directory().map_err(|e| e.to_string())?;
            muniment_core::projects::workspace(
                &profile,
                &thread_id.ok_or("Choose a thread for this file.")?,
            )?
            .join(path)
        };
        read_text(&path)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_save_text(
    webview: tauri::Webview,
    path: PathBuf,
    content: String,
    revision: String,
) -> Result<TextFile, String> {
    shell(&webview)?;
    tauri::async_runtime::spawn_blocking(move || save_text(&path, &content, &revision))
        .await
        .map_err(|e| e.to_string())?
}
