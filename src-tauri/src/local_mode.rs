use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::Manager;
use uuid::Uuid;

use muniment_core::local_mode::LOCAL_MODE_MARKER;
use muniment_core::pi_settings::{merge_pi_settings, pi_agent_directory};
use muniment_core::sidecar::pi_install::PI_SELECTED_ARTIFACT;

const CLOUD_PROVIDERS: [&str; 3] = ["anthropic", "google", "openai"];
const PROVIDERS: [&str; 4] = ["anthropic", "google", "openai", "ollama"];
const OLLAMA_PROVIDER: &str = "ollama";
const OLLAMA_MODEL: &str = "llama3.2:latest";
const AUTH_LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const AUTH_LOCK_RETRY: Duration = Duration::from_millis(20);

struct PiAuthLock(PathBuf);

#[derive(Debug, Serialize)]
pub(crate) struct ProviderStatus {
    provider: &'static str,
    configured: bool,
}

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

fn pi_auth_file(
    home_directory: &Path,
    agent_directory: Option<&std::ffi::OsStr>,
) -> Result<PathBuf, String> {
    pi_agent_directory(home_directory, agent_directory)
        .map(|directory| directory.join("auth.json"))
        .map_err(|_| "The Pi agent directory is invalid.".to_string())
}

fn pi_models_file(
    home_directory: &Path,
    agent_directory: Option<&std::ffi::OsStr>,
) -> Result<PathBuf, String> {
    pi_agent_directory(home_directory, agent_directory)
        .map(|directory| directory.join("models.json"))
        .map_err(|_| "The Pi agent directory is invalid.".to_string())
}

fn pi_settings_file(models_file: &Path) -> PathBuf {
    models_file.with_file_name("settings.json")
}

fn read_json_store(
    path: &Path,
) -> Result<Option<serde_json::Map<String, serde_json::Value>>, String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        match parent.try_exists() {
            Ok(true) => {}
            Ok(false) => return Ok(None),
            Err(_) => return Err("Pi credentials could not be read.".into()),
        }
    }
    let _lock =
        lock_pi_auth_file(path).map_err(|_| "Pi credentials could not be read.".to_string())?;
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Pi credentials could not be read.".into()),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| "Pi credentials could not be read.".to_string())
}

fn provider_status(auth_file: &Path, models_file: &Path) -> Result<Vec<ProviderStatus>, String> {
    let auth = read_json_store(auth_file)?;
    let models = read_json_store(models_file)?;
    Ok(PROVIDERS
        .iter()
        .map(|provider| {
            let configured = if *provider == OLLAMA_PROVIDER {
                models
                    .as_ref()
                    .and_then(|root| root.get("providers"))
                    .and_then(serde_json::Value::as_object)
                    .and_then(|providers| providers.get(*provider))
                    .is_some_and(serde_json::Value::is_object)
            } else {
                auth.as_ref()
                    .and_then(|entries| entries.get(*provider))
                    .is_some_and(serde_json::Value::is_object)
            };
            ProviderStatus {
                provider,
                configured,
            }
        })
        .collect())
}

fn store_provider_key(auth_file: &Path, provider: &str, key: &str) -> Result<(), String> {
    if !CLOUD_PROVIDERS.contains(&provider)
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

fn store_local_provider(models_file: &Path, base_url: &str) -> Result<(), String> {
    if base_url.is_empty() || base_url.len() > 16 * 1024 || base_url.trim() != base_url {
        return Err("Enter a valid Ollama server URL.".into());
    }
    let parsed = url::Url::parse(base_url).map_err(|_| "Enter a valid Ollama server URL.")?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err("Enter a valid Ollama server URL.".into());
    }

    let parent = models_file
        .parent()
        .ok_or_else(|| "Pi provider settings could not be saved.".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|_| "Pi provider settings could not be saved.".to_string())?;
    let settings_file = pi_settings_file(models_file);
    let _models_lock = lock_pi_auth_file(models_file)
        .map_err(|_| "Pi provider settings could not be saved.".to_string())?;
    let settings_lock = muniment_core::pi_settings::lock_settings(&settings_file)
        .map_err(|_| "Pi provider settings could not be saved.".to_string())?;
    let mut models = read_json_for_update(models_file)?;
    let mut settings = read_json_for_update(&settings_file)?;
    let providers = models
        .entry("providers")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "Pi provider settings could not be saved.".to_string())?;
    providers.insert(
        OLLAMA_PROVIDER.to_owned(),
        serde_json::json!({
            "baseUrl": base_url,
            "api": "openai-completions",
            "apiKey": OLLAMA_PROVIDER,
            "compat": {
                "supportsDeveloperRole": false,
                "supportsReasoningEffort": false
            },
            "models": [{ "id": OLLAMA_MODEL }]
        }),
    );
    settings.insert("defaultProvider".to_owned(), OLLAMA_PROVIDER.into());
    settings.insert("defaultModel".to_owned(), OLLAMA_MODEL.into());
    merge_pi_settings(&mut settings, PI_SELECTED_ARTIFACT);

    // Write the route first. A later models write failure cannot fall back to a cloud model.
    settings_lock
        .check()
        .map_err(|_| "Pi provider settings could not be saved.".to_string())?;
    write_json_for_update(&settings_file, &settings)?;
    write_json_for_update(models_file, &models)
}

fn read_json_for_update(path: &Path) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| "Pi provider settings could not be saved.".to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::Map::new()),
        Err(_) => Err("Pi provider settings could not be saved.".into()),
    }
}

fn write_json_for_update(
    path: &Path,
    root: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(root)
        .map_err(|_| "Pi provider settings could not be saved.".to_string())?;
    let temporary = path.with_extension(format!("tmp-{}", Uuid::new_v4()));
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
        muniment_core::atomic_file::replace(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|_| "Pi provider settings could not be saved.".into())
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
pub(crate) fn local_mode_provider_status(
    app: tauri::AppHandle,
) -> Result<Vec<ProviderStatus>, String> {
    let home_directory = app
        .path()
        .home_dir()
        .map_err(|_| "Pi credentials could not be read.".to_string())?;
    let agent_directory = std::env::var_os("PI_CODING_AGENT_DIR");
    let auth_file = pi_auth_file(&home_directory, agent_directory.as_deref())?;
    let models_file = pi_models_file(&home_directory, agent_directory.as_deref())?;
    provider_status(&auth_file, &models_file)
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
    let agent_directory = std::env::var_os("PI_CODING_AGENT_DIR");
    let auth_file = pi_auth_file(&home_directory, agent_directory.as_deref())?;
    store_provider_key(&auth_file, &provider, &key)
}

#[tauri::command]
pub(crate) fn local_mode_store_local_provider(
    app: tauri::AppHandle,
    base_url: String,
) -> Result<(), String> {
    let home_directory = app
        .path()
        .home_dir()
        .map_err(|_| "Pi provider settings could not be saved.".to_string())?;
    let agent_directory = std::env::var_os("PI_CODING_AGENT_DIR");
    let models_file = pi_models_file(&home_directory, agent_directory.as_deref())?;
    store_local_provider(&models_file, &base_url)
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
            pi_auth_file(home, None).unwrap(),
            PathBuf::from("/home/tester/.pi/agent/auth.json")
        );
        assert_eq!(
            pi_auth_file(home, Some(std::ffi::OsStr::new(""))).unwrap(),
            PathBuf::from("/home/tester/.pi/agent/auth.json")
        );
        assert_eq!(
            pi_auth_file(home, Some(std::ffi::OsStr::new("~"))).unwrap(),
            PathBuf::from("/home/tester/auth.json")
        );
        assert_eq!(
            pi_auth_file(home, Some(std::ffi::OsStr::new("~/pi-credentials"))).unwrap(),
            PathBuf::from("/home/tester/pi-credentials/auth.json")
        );
    }

    #[test]
    fn provider_status_reports_missing_store_as_not_configured() {
        let directory = temporary_directory();
        let statuses = provider_status(
            &directory.join("missing-auth.json"),
            &directory.join("missing-models.json"),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(statuses).unwrap(),
            serde_json::json!([
                {"provider":"anthropic","configured":false},
                {"provider":"google","configured":false},
                {"provider":"openai","configured":false},
                {"provider":"ollama","configured":false}
            ])
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn provider_status_reports_object_entries_without_exposing_credentials() {
        let directory = temporary_directory();
        let auth_file = directory.join("auth.json");
        fs::write(
            &auth_file,
            r#"{"anthropic":{"type":"api_key","key":"secret-key"},"github-copilot":{"type":"oauth","access":"saved"}}"#,
        )
        .unwrap();

        let serialized = serde_json::to_string(
            &provider_status(&auth_file, &directory.join("models.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            serialized,
            r#"[{"provider":"anthropic","configured":true},{"provider":"google","configured":false},{"provider":"openai","configured":false},{"provider":"ollama","configured":false}]"#
        );
        assert!(!serialized.contains("secret-key"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn provider_status_treats_oauth_provider_entry_as_configured() {
        let directory = temporary_directory();
        let auth_file = directory.join("auth.json");
        fs::write(
            &auth_file,
            r#"{"google":{"type":"oauth","access":"saved"}}"#,
        )
        .unwrap();

        let statuses = serde_json::to_value(
            provider_status(&auth_file, &directory.join("models.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(statuses[1]["configured"], true);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn provider_status_rejects_malformed_json() {
        let directory = temporary_directory();
        let auth_file = directory.join("auth.json");
        fs::write(&auth_file, "not json").unwrap();

        assert_eq!(
            provider_status(&auth_file, &directory.join("models.json")).unwrap_err(),
            "Pi credentials could not be read."
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn provider_status_rejects_unreadable_store() {
        let directory = temporary_directory();
        let auth_file = directory.join("auth.json");
        fs::create_dir(&auth_file).unwrap();

        assert_eq!(
            provider_status(&auth_file, &directory.join("models.json")).unwrap_err(),
            "Pi credentials could not be read."
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn provider_key_command_uses_custom_pi_credential_directory() {
        let directory = temporary_directory();
        let custom_directory = directory.join("custom-pi-directory");
        let auth_file = pi_auth_file(&directory, Some(custom_directory.as_os_str())).unwrap();

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
        assert!(store_provider_key(&auth_file, "ollama", "key").is_err());
        assert!(store_provider_key(&auth_file, "openai", " ").is_err());
        assert!(!auth_file.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn local_provider_writes_models_file_and_preserves_other_providers() {
        let directory = temporary_directory();
        let models_file = directory.join("models.json");
        fs::write(
            &models_file,
            r#"{"providers":{"custom":{"baseUrl":"http://localhost:9000/v1"}}}"#,
        )
        .unwrap();

        store_local_provider(&models_file, "http://127.0.0.1:11434/v1").unwrap();

        let models: serde_json::Value =
            serde_json::from_slice(&fs::read(&models_file).unwrap()).unwrap();
        assert_eq!(
            models["providers"]["ollama"],
            serde_json::json!({
                "baseUrl": "http://127.0.0.1:11434/v1",
                "api": "openai-completions",
                "apiKey": "ollama",
                "compat": {
                    "supportsDeveloperRole": false,
                    "supportsReasoningEffort": false
                },
                "models": [{"id": "llama3.2:latest"}]
            })
        );
        assert_eq!(
            models["providers"]["custom"]["baseUrl"],
            "http://localhost:9000/v1"
        );
        let statuses = provider_status(&directory.join("auth.json"), &models_file).unwrap();
        assert!(statuses
            .iter()
            .any(|status| status.provider == "ollama" && status.configured));
        assert!(!directory.join("auth.json").exists());
        let settings: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.join("settings.json")).unwrap()).unwrap();
        assert_eq!(settings["defaultProvider"], "ollama");
        assert_eq!(settings["defaultModel"], "llama3.2:latest");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn local_provider_pins_ollama_when_cloud_credentials_exist() {
        let directory = temporary_directory();
        let auth_file = directory.join("auth.json");
        let models_file = directory.join("models.json");
        fs::write(
            &auth_file,
            r#"{"openai":{"type":"api_key","key":"paid-cloud-key"}}"#,
        )
        .unwrap();
        fs::write(
            directory.join("settings.json"),
            r#"{"defaultProvider":"openai","defaultModel":"gpt-5","theme":"dark"}"#,
        )
        .unwrap();

        store_local_provider(&models_file, "http://127.0.0.1:11434/v1").unwrap();

        let settings: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.join("settings.json")).unwrap()).unwrap();
        assert_eq!(settings["defaultProvider"], "ollama");
        assert_eq!(settings["defaultModel"], "llama3.2:latest");
        assert_eq!(settings["theme"], "dark");
        let auth: serde_json::Value =
            serde_json::from_slice(&fs::read(auth_file).unwrap()).unwrap();
        assert_eq!(auth["openai"]["key"], "paid-cloud-key");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn local_provider_rejects_malformed_base_urls() {
        let directory = temporary_directory();
        let models_file = directory.join("models.json");

        for base_url in [
            "localhost:11434/v1",
            "ftp://localhost/v1",
            "http://",
            " http://localhost/v1",
            "http://user:secret@localhost/v1",
            "http://localhost/v1?token=secret",
        ] {
            assert!(store_local_provider(&models_file, base_url).is_err());
        }
        assert!(!models_file.exists());
        fs::remove_dir_all(directory).unwrap();
    }
}
