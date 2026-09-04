use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tauri::Manager;
use uuid::Uuid;

use muniment_core::local_mode::LOCAL_MODE_MARKER;

const PROVIDERS: [&str; 3] = ["anthropic", "google", "openai"];
const AUTH_LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const AUTH_LOCK_RETRY: Duration = Duration::from_millis(20);

struct PiAuthLock(PathBuf);

impl Drop for PiAuthLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.0);
    }
}

fn pi_auth_lock_path(auth_file: &Path) -> PathBuf {
    let mut name = OsString::from(auth_file.as_os_str());
    name.push(".lock");
    PathBuf::from(name)
}

fn lock_pi_auth_file(auth_file: &Path) -> Result<PiAuthLock, String> {
    let lock_path = pi_auth_lock_path(auth_file);
    let deadline = Instant::now() + AUTH_LOCK_TIMEOUT;
    loop {
        match fs::create_dir(&lock_path) {
            Ok(()) => return Ok(PiAuthLock(lock_path)),
            Err(error)
                if error.kind() == std::io::ErrorKind::AlreadyExists
                    && Instant::now() < deadline =>
            {
                std::thread::sleep(AUTH_LOCK_RETRY);
            }
            Err(_) => return Err("Pi credentials could not be saved.".into()),
        }
    }
}

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

fn pi_auth_file(home_directory: &Path, agent_directory: Option<&std::ffi::OsStr>) -> PathBuf {
    let agent_directory = match agent_directory.filter(|value| !value.is_empty()) {
        Some(value) if value == "~" => home_directory.to_owned(),
        Some(value) => match value.to_str() {
            Some(value) if value.starts_with("~/") => home_directory.join(&value[2..]),
            _ => PathBuf::from(value),
        },
        None => home_directory.join(".pi").join("agent"),
    };
    agent_directory.join("auth.json")
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
    if !auth_file.exists() {
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options
            .open(auth_file)
            .and_then(|mut file| file.write_all(b"{}"))
        {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err("Pi credentials could not be saved.".into()),
        }
    }
    let _lock = lock_pi_auth_file(auth_file)?;
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
pub(crate) fn local_mode_status(app: tauri::AppHandle) -> Result<bool, String> {
    is_active(&app)
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
    let home_directory = app
        .path()
        .home_dir()
        .map_err(|_| "Pi credentials could not be saved.".to_string())?;
    let bundled_agent = app
        .path()
        .resource_dir()
        .map_err(|_| "Pi credentials could not be saved.".to_string())?
        .join("pi-agent");
    let configured_agent = if bundled_agent.is_dir() {
        Some(
            app.path()
                .app_config_dir()
                .map_err(|_| "Pi credentials could not be saved.".to_string())?
                .join("pi-agent")
                .into_os_string(),
        )
    } else {
        std::env::var_os("PI_CODING_AGENT_DIR")
    };
    let auth_file = pi_auth_file(&home_directory, configured_agent.as_deref());
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
    fn pi_auth_file_matches_pis_directory_resolution() {
        let home = Path::new("/home/tester");

        assert_eq!(
            pi_auth_file(home, None),
            PathBuf::from("/home/tester/.pi/agent/auth.json")
        );
        assert_eq!(
            pi_auth_file(home, Some(std::ffi::OsStr::new(""))),
            PathBuf::from("/home/tester/.pi/agent/auth.json")
        );
        assert_eq!(
            pi_auth_file(home, Some(std::ffi::OsStr::new("~"))),
            PathBuf::from("/home/tester/auth.json")
        );
        assert_eq!(
            pi_auth_file(home, Some(std::ffi::OsStr::new("~/pi-credentials"))),
            PathBuf::from("/home/tester/pi-credentials/auth.json")
        );
    }

    #[test]
    fn provider_key_command_uses_custom_pi_credential_directory() {
        let directory = temporary_directory();
        let custom_directory = directory.join("custom-pi-directory");
        let auth_file = pi_auth_file(&directory, Some(custom_directory.as_os_str()));

        store_provider_key(&auth_file, "openai", "test-key").unwrap();

        assert_eq!(auth_file, custom_directory.join("auth.json"));
        let auth: serde_json::Value =
            serde_json::from_slice(&fs::read(&auth_file).unwrap()).unwrap();
        assert_eq!(auth["openai"]["key"], "test-key");
        assert!(!directory.join(".pi").exists());
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
    fn provider_key_command_waits_for_a_concurrent_pi_credential_update() {
        let directory = temporary_directory();
        let auth_file = directory.join("auth.json");
        fs::write(&auth_file, "{}").unwrap();
        let lock_path = pi_auth_lock_path(&auth_file);
        fs::create_dir(&lock_path).unwrap();
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let writer_file = auth_file.clone();
        let writer = std::thread::spawn(move || {
            let result = store_provider_key(&writer_file, "anthropic", "test-key");
            finished_tx.send(()).unwrap();
            result
        });

        assert!(finished_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err());
        fs::write(
            &auth_file,
            r#"{"github-copilot":{"type":"oauth","access":"updated"}}"#,
        )
        .unwrap();
        fs::remove_dir(lock_path).unwrap();
        writer.join().unwrap().unwrap();

        let auth: serde_json::Value =
            serde_json::from_slice(&fs::read(&auth_file).unwrap()).unwrap();
        assert_eq!(auth["github-copilot"]["access"], "updated");
        assert_eq!(auth["anthropic"]["key"], "test-key");
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
