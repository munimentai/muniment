use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use tauri::Manager;
use uuid::Uuid;

use muniment_core::local_mode::LOCAL_MODE_MARKER;

const PROVIDERS: [&str; 3] = ["anthropic", "google", "openai"];

pub(crate) fn is_active<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<bool, String> {
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|_| "Local mode could not be checked.".to_string())?
        .join(LOCAL_MODE_MARKER)
        .is_file())
}

fn set_local_mode(config_directory: &Path, enabled: bool) -> Result<(), String> {
    fs::create_dir_all(config_directory)
        .map_err(|_| "Local mode could not be changed.".to_string())?;
    let marker = config_directory.join(LOCAL_MODE_MARKER);
    if enabled {
        OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(marker)
            .and_then(|mut file| file.write_all(b"1"))
            .map_err(|_| "Local mode could not be changed.".to_string())
    } else {
        match fs::remove_file(marker) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err("Local mode could not be changed.".into()),
        }
    }
}

fn store_provider_key(auth_file: &Path, provider: &str, key: &str) -> Result<(), String> {
    if !PROVIDERS.contains(&provider)
        || key.is_empty()
        || key.len() > 16 * 1024
        || key.trim() != key
    {
        return Err("Enter a valid provider and API key.".into());
    }
    let parent = auth_file
        .parent()
        .ok_or_else(|| "Pi credentials could not be saved.".to_string())?;
    fs::create_dir_all(parent).map_err(|_| "Pi credentials could not be saved.".to_string())?;
    let mut auth = match fs::read(auth_file) {
        Ok(bytes) => serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>(&bytes)
            .map_err(|_| "Pi credentials could not be saved.".to_string())?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::Map::new(),
        Err(_) => return Err("Pi credentials could not be saved.".into()),
    };
    auth.insert(
        provider.to_owned(),
        serde_json::json!({"type": "api_key", "key": key}),
    );
    let bytes = serde_json::to_vec_pretty(&auth)
        .map_err(|_| "Pi credentials could not be saved.".to_string())?;
    let temporary = auth_file.with_extension(format!("tmp-{}", Uuid::new_v4()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        muniment_core::atomic_file::replace(&temporary, auth_file)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|_| "Pi credentials could not be saved.".into())
}

#[tauri::command]
pub(crate) fn local_mode_enter(app: tauri::AppHandle) -> Result<(), String> {
    let directory = app
        .path()
        .app_config_dir()
        .map_err(|_| "Local mode could not be changed.".to_string())?;
    set_local_mode(&directory, true)
}

#[tauri::command]
pub(crate) fn local_mode_leave(app: tauri::AppHandle) -> Result<(), String> {
    let directory = app
        .path()
        .app_config_dir()
        .map_err(|_| "Local mode could not be changed.".to_string())?;
    set_local_mode(&directory, false)
}

#[tauri::command]
pub(crate) fn local_mode_store_provider_key(
    app: tauri::AppHandle,
    provider: String,
    key: String,
) -> Result<(), String> {
    let auth_file = app
        .path()
        .home_dir()
        .map_err(|_| "Pi credentials could not be saved.".to_string())?
        .join(".pi")
        .join("agent")
        .join("auth.json");
    store_provider_key(&auth_file, &provider, &key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_directory() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("muniment-local-mode-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn marker_changes_local_mode_without_network_access() {
        let directory = temporary_directory();
        set_local_mode(&directory, true).unwrap();
        assert!(directory.join(LOCAL_MODE_MARKER).is_file());
        set_local_mode(&directory, false).unwrap();
        assert!(!directory.join(LOCAL_MODE_MARKER).exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn provider_key_command_writes_pis_auth_file_and_preserves_other_entries() {
        let directory = temporary_directory();
        let auth_file = directory.join("auth.json");
        fs::write(
            &auth_file,
            r#"{"github-copilot":{"type":"oauth","access":"saved"}}"#,
        )
        .unwrap();
        store_provider_key(&auth_file, "anthropic", "test-key").unwrap();
        let auth: serde_json::Value =
            serde_json::from_slice(&fs::read(&auth_file).unwrap()).unwrap();
        assert_eq!(
            auth["anthropic"],
            serde_json::json!({"type":"api_key","key":"test-key"})
        );
        assert_eq!(auth["github-copilot"]["access"], "saved");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn provider_key_command_rejects_unknown_providers_and_blank_keys() {
        let directory = temporary_directory();
        let auth_file = directory.join("auth.json");
        assert!(store_provider_key(&auth_file, "other", "key").is_err());
        assert!(store_provider_key(&auth_file, "openai", " ").is_err());
        assert!(!auth_file.exists());
        fs::remove_dir_all(directory).unwrap();
    }
}
