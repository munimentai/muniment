use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::Manager;
use uuid::Uuid;

use muniment_core::local_mode::LOCAL_MODE_MARKER;
use muniment_core::model_router::config::ROUTER_PROVIDER;
use muniment_core::pi_settings::merge_pi_settings;
use muniment_core::sidecar::pi_install::PI_SELECTED_ARTIFACT;
use muniment_core::state_root::agent_directory;

const PROVIDERS: [&str; 4] = ["anthropic", "google", "openai", "ollama"];
const OLLAMA_PROVIDER: &str = "ollama";
const OLLAMA_MODEL: &str = "llama3.2:latest";
const AUTH_LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const AUTH_LOCK_RETRY: Duration = Duration::from_millis(20);
const READ_SETTINGS_ERROR: &str =
    "Muniment cannot read provider settings. Check folder access, then retry.";
const SAVE_SETTINGS_ERROR: &str =
    "Muniment cannot save provider settings. Check folder access, then retry.";
const SETTINGS_DIRECTORY_ERROR: &str =
    "The settings folder is invalid. Check its location, then retry.";

pub(crate) struct PiAuthLock(PathBuf);

#[derive(Debug, Serialize)]
pub(crate) struct ProviderStatus {
    provider: &'static str,
    configured: bool,
}

/// Pi's built-in providers that take an API key in `auth.json`, by Pi id and display name.
const KEY_PROVIDERS: &[(&str, &str)] = &[
    ("anthropic", "Anthropic"),
    ("openai", "OpenAI"),
    ("google", "Google"),
    ("xai", "xAI"),
    ("openrouter", "OpenRouter"),
    ("deepseek", "DeepSeek"),
    ("mistral", "Mistral"),
    ("groq", "Groq"),
    ("cerebras", "Cerebras"),
    ("nvidia", "NVIDIA NIM"),
    ("amazon-bedrock", "Amazon Bedrock"),
    ("azure-openai-responses", "Azure OpenAI"),
    ("vercel-ai-gateway", "Vercel Gateway"),
    ("cloudflare-ai-gateway", "Cloudflare Gateway"),
    ("cloudflare-workers-ai", "Cloudflare Workers"),
    ("zai", "ZAI Coding Plan"),
    ("opencode", "OpenCode Zen"),
    ("opencode-go", "OpenCode Go"),
    ("huggingface", "Hugging Face"),
    ("fireworks", "Fireworks"),
    ("together", "Together"),
    ("baseten", "Baseten"),
    ("kimi-coding", "Kimi For Coding"),
    ("minimax", "MiniMax"),
    ("qwen-token-plan", "Qwen Token Plan"),
    ("radius", "Radius"),
    ("xiaomi", "Xiaomi"),
    ("moonshotai", "Moonshot AI"),
    ("zai-coding-cn", "ZAI Coding China"),
    ("xiaomi-token-plan-sgp", "Xiaomi Token Plan Singapore"),
    ("qwen-token-plan-individual", "Qwen Token Plan Individual"),
    ("moonshotai-cn", "Moonshot AI China"),
    ("xiaomi-token-plan-cn", "Xiaomi Token Plan China"),
    ("xiaomi-token-plan-ams", "Xiaomi Token Plan Amsterdam"),
    ("typesafe", "TypeSafe"),
    ("qwen-token-plan-cn", "Qwen Token Plan China"),
    ("google-vertex", "Google Vertex AI"),
    ("ant-ling", "Ant Ling"),
    ("minimax-cn", "MiniMax China"),
];
/// Providers Pi signs into with an account, by Pi id and display name.
const ACCOUNT_PROVIDERS: &[(&str, &str)] = &[
    ("openai-codex", "OpenAI"),
    ("xai", "xAI"),
    ("openrouter", "OpenRouter"),
    ("github-copilot", "GitHub Copilot"),
    ("anthropic", "Anthropic"),
];
/// The pi-claude-bridge provider: Anthropic through the Claude Code sign-in.
const CLAUDE_BRIDGE_PROVIDER: &str = "claude-bridge";
const LM_STUDIO_PROVIDER: &str = "lmstudio";
const CUSTOM_PROVIDER_PREFIX: &str = "custom-";
/// The desktop's own record beside Pi's files: hidden models, endpoint names, the Claude Code connection.
const MODELS_RECORD_FILE: &str = "muniment-models.json";
const CLAUDE_BRIDGE_CONFIG_FILE: &str = "claude-bridge.json";
const LIST_MODELS_TIMEOUT: Duration = Duration::from_secs(30);
/// One adoption probe, and how many candidates a connect tries before it settles.
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);
const PROBE_LIMIT: usize = 5;
const CLAUDE_STATUS_TIMEOUT: Duration = Duration::from_secs(10);
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct InventoryModel {
    id: String,
    context: String,
    max_out: String,
    thinking: bool,
    images: bool,
}

#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct InventoryProvider {
    id: String,
    name: String,
    /// `key`, `account`, `local`, `custom` or `claude-code`.
    source: &'static str,
    base_url: Option<String>,
    models: Vec<InventoryModel>,
}

#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct ProviderInventory {
    providers: Vec<InventoryProvider>,
    default_provider: Option<String>,
    default_model: Option<String>,
    hidden: Vec<String>,
    /// The classifier model that picks a route per turn, when the router runs
    /// one. The composer's picker puts the router first and names it.
    router_classifier: Option<String>,
    /// Every model the router serves, with the family and model it stands for
    /// and how many accounts stand behind it. The picker lists each under its
    /// provider once and marks the ones two or more accounts serve, because
    /// picking one of those balances across them.
    router_models: Vec<RouterModel>,
}

/// One model the router serves, as the picker places it under its provider.
#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct RouterModel {
    /// The id the router serves it as: `family/model`, or the user's name.
    id: String,
    family: String,
    model: String,
    accounts: u32,
}

#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct ClaudeCodeStatus {
    installed: bool,
    logged_in: bool,
    path: Option<String>,
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

pub(crate) fn lock_pi_auth_file(auth_file: &Path) -> Result<PiAuthLock, String> {
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
            Err(_) => return Err(SAVE_SETTINGS_ERROR.into()),
        }
    }
}

fn config_directory<R: tauri::Runtime>(_app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    // The runtime and the desktop read the same local-mode marker under one root.
    muniment_runtime::profile_directory()
        .map_err(|_| "Muniment cannot find the local mode folder.".to_string())
}

/// The agent harness's own directory under the state root.
pub(crate) fn harness_agent_directory(error: &str) -> Result<PathBuf, String> {
    muniment_runtime::profile_directory()
        .map(|state| agent_directory(&state))
        .map_err(|_| error.to_string())
}

pub(crate) fn is_active<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<bool, String> {
    Ok(muniment_core::local_mode::is_local_mode(&config_directory(
        app,
    )?))
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

fn pi_auth_file(agent: &Path) -> PathBuf {
    agent.join("auth.json")
}

pub(crate) fn pi_models_file(agent: &Path) -> PathBuf {
    agent.join("models.json")
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
            Err(_) => return Err(READ_SETTINGS_ERROR.into()),
        }
    }
    let _lock = lock_pi_auth_file(path).map_err(|_| READ_SETTINGS_ERROR.to_string())?;
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(READ_SETTINGS_ERROR.into()),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| READ_SETTINGS_ERROR.to_string())
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

fn key_provider_name(provider: &str) -> Option<&'static str> {
    KEY_PROVIDERS
        .iter()
        .find(|(id, _)| *id == provider)
        .map(|(_, name)| *name)
}

fn store_provider_key(auth_file: &Path, provider: &str, key: &str) -> Result<(), String> {
    if key_provider_name(provider).is_none()
        || key.is_empty()
        || key.len() > 16 * 1024
        || key.trim() != key
    {
        return Err("Enter a valid provider and API key.".into());
    }
    let parent = auth_file
        .parent()
        .ok_or_else(|| SAVE_SETTINGS_ERROR.to_string())?;
    fs::create_dir_all(parent).map_err(|_| SAVE_SETTINGS_ERROR.to_string())?;
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
            Err(_) => return Err(SAVE_SETTINGS_ERROR.into()),
        }
    }
    let _lock = lock_pi_auth_file(auth_file)?;
    let mut auth = match fs::read(auth_file) {
        Ok(bytes) => serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>(&bytes)
            .map_err(|_| SAVE_SETTINGS_ERROR.to_string())?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::Map::new(),
        Err(_) => return Err(SAVE_SETTINGS_ERROR.into()),
    };
    auth.insert(
        provider.to_owned(),
        serde_json::json!({"type": "api_key", "key": key}),
    );
    let bytes = serde_json::to_vec_pretty(&auth).map_err(|_| SAVE_SETTINGS_ERROR.to_string())?;
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
    result.map_err(|_| SAVE_SETTINGS_ERROR.into())
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
        .ok_or_else(|| SAVE_SETTINGS_ERROR.to_string())?;
    fs::create_dir_all(parent).map_err(|_| SAVE_SETTINGS_ERROR.to_string())?;
    let settings_file = pi_settings_file(models_file);
    let _models_lock =
        lock_pi_auth_file(models_file).map_err(|_| SAVE_SETTINGS_ERROR.to_string())?;
    let settings_lock = muniment_core::pi_settings::lock_settings(&settings_file)
        .map_err(|_| SAVE_SETTINGS_ERROR.to_string())?;
    let mut models = read_json_for_update(models_file)?;
    let mut settings = read_json_for_update(&settings_file)?;
    let providers = models
        .entry("providers")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| SAVE_SETTINGS_ERROR.to_string())?;
    // The server names its own models; the pinned id stands in when it does not answer.
    let discovered = muniment_core::endpoint_models::discover_models(base_url, DISCOVERY_TIMEOUT);
    let model_ids = discovered.unwrap_or_else(|| vec![OLLAMA_MODEL.to_owned()]);
    if model_ids.is_empty() {
        return Err("The server has no chat models. Add a chat model, then try again.".into());
    }
    let default_model = if model_ids.iter().any(|id| id == OLLAMA_MODEL) {
        OLLAMA_MODEL.to_owned()
    } else {
        model_ids[0].clone()
    };
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
            "models": model_ids.iter().map(|id| serde_json::json!({ "id": id })).collect::<Vec<_>>()
        }),
    );
    settings.insert("defaultProvider".to_owned(), OLLAMA_PROVIDER.into());
    settings.insert("defaultModel".to_owned(), default_model.into());
    merge_pi_settings(&mut settings, PI_SELECTED_ARTIFACT);

    // Write the route first. A later models write failure cannot fall back to a cloud model.
    settings_lock
        .check()
        .map_err(|_| SAVE_SETTINGS_ERROR.to_string())?;
    write_json_for_update(&settings_file, &settings)?;
    write_json_for_update(models_file, &models)
}

pub(crate) fn read_json_for_update(
    path: &Path,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| SAVE_SETTINGS_ERROR.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::Map::new()),
        Err(_) => Err(SAVE_SETTINGS_ERROR.into()),
    }
}

pub(crate) fn write_json_for_update(
    path: &Path,
    root: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(root).map_err(|_| SAVE_SETTINGS_ERROR.to_string())?;
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
    result.map_err(|_| SAVE_SETTINGS_ERROR.into())
}

#[tauri::command]
pub(crate) fn local_mode_status(app: tauri::AppHandle) -> Result<bool, String> {
    is_active(&app)
}

#[tauri::command]
pub(crate) fn local_mode_enter(app: tauri::AppHandle) -> Result<(), String> {
    set_local_mode(&config_directory(&app)?, true)
}

#[tauri::command]
pub(crate) fn local_mode_leave(app: tauri::AppHandle) -> Result<(), String> {
    set_local_mode(&config_directory(&app)?, false)
}

#[tauri::command]
pub(crate) fn local_mode_provider_status(
    _app: tauri::AppHandle,
) -> Result<Vec<ProviderStatus>, String> {
    let agent = harness_agent_directory(READ_SETTINGS_ERROR)?;
    provider_status(&pi_auth_file(&agent), &pi_models_file(&agent))
}

#[tauri::command]
pub(crate) async fn local_mode_store_provider_key(
    _app: tauri::AppHandle,
    provider: String,
    key: String,
) -> Result<(), String> {
    let agent = harness_agent_directory(SAVE_SETTINGS_ERROR)?;
    tauri::async_runtime::spawn_blocking(move || {
        store_provider_key(&pi_auth_file(&agent), &provider, &key)?;
        if default_model_unset(&agent) {
            let _ = adopt_provider_default(&agent, &provider, false);
        }
        Ok(())
    })
    .await
    .map_err(|_| SAVE_SETTINGS_ERROR.to_string())?
}

#[tauri::command]
pub(crate) fn local_mode_store_local_provider(
    _app: tauri::AppHandle,
    base_url: String,
) -> Result<(), String> {
    let agent = harness_agent_directory(SAVE_SETTINGS_ERROR)?;
    store_local_provider(&pi_models_file(&agent), &base_url)
}

fn models_record_file(agent: &Path) -> PathBuf {
    agent.join(MODELS_RECORD_FILE)
}

fn claude_bridge_config_file(agent: &Path) -> PathBuf {
    agent.join(CLAUDE_BRIDGE_CONFIG_FILE)
}

fn read_models_record(agent: &Path) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    match fs::read(models_record_file(agent)) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| READ_SETTINGS_ERROR.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::Map::new()),
        Err(_) => Err(READ_SETTINGS_ERROR.into()),
    }
}

fn write_models_record(
    agent: &Path,
    record: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    fs::create_dir_all(agent).map_err(|_| SAVE_SETTINGS_ERROR.to_string())?;
    write_json_for_update(&models_record_file(agent), record)
}

fn record_strings(record: &serde_json::Map<String, serde_json::Value>, key: &str) -> Vec<String> {
    record
        .get(key)
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn provider_display_name(
    provider: &str,
    record: &serde_json::Map<String, serde_json::Value>,
) -> String {
    if let Some(name) = record
        .get("names")
        .and_then(serde_json::Value::as_object)
        .and_then(|names| names.get(provider))
        .and_then(serde_json::Value::as_str)
    {
        return name.to_owned();
    }
    if provider == OLLAMA_PROVIDER {
        return "Ollama".into();
    }
    if provider == LM_STUDIO_PROVIDER {
        return "LM Studio".into();
    }
    if provider == CLAUDE_BRIDGE_PROVIDER {
        return "Anthropic".into();
    }
    if provider == ROUTER_PROVIDER {
        return "Model router".into();
    }
    key_provider_name(provider)
        .or_else(|| {
            ACCOUNT_PROVIDERS
                .iter()
                .find(|(id, _)| *id == provider)
                .map(|(_, name)| *name)
        })
        .map(str::to_owned)
        .unwrap_or_else(|| provider.to_owned())
}

/// Parses Pi's `--list-models` table: a header row, then one row per model.
fn parse_model_table(output: &str) -> Vec<(String, InventoryModel)> {
    let mut rows = Vec::new();
    for line in output
        .lines()
        .skip_while(|line| !line.starts_with("provider"))
        .skip(1)
    {
        let columns: Vec<&str> = line.split_whitespace().collect();
        if columns.len() < 6 {
            continue;
        }
        rows.push((
            columns[0].to_owned(),
            InventoryModel {
                id: columns[1].to_owned(),
                context: columns[2].to_owned(),
                max_out: columns[3].to_owned(),
                thinking: columns[4] == "yes",
                images: columns[5] == "yes",
            },
        ));
    }
    rows
}

fn run_with_timeout(
    mut command: std::process::Command,
    timeout: Duration,
) -> Option<std::process::Output> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(command.output());
    });
    receiver.recv_timeout(timeout).ok().and_then(Result::ok)
}

/// The pinned Pi executable under the state root, once a chat has installed it.
pub(crate) fn pi_executable() -> Option<PathBuf> {
    let state = muniment_core::state_root::state_directory()?;
    let root = state.join(muniment_core::state_root::HARNESS_DIRECTORY_NAME);
    muniment_core::sidecar::pi_install::resolve_current_for(&root, PI_SELECTED_ARTIFACT).ok()
}

/// Every model Pi can serve from the agent directory, by provider, from the pinned executable.
fn list_pi_models(agent: &Path) -> Vec<(String, InventoryModel)> {
    let Some(executable) = pi_executable() else {
        return Vec::new();
    };
    let mut command = std::process::Command::new(executable);
    command
        .arg("--list-models")
        .env("PI_CODING_AGENT_DIR", agent)
        .env("PI_OFFLINE", "1")
        .env_remove("BUN_BE_BUN")
        .stdin(std::process::Stdio::null());
    run_with_timeout(command, LIST_MODELS_TIMEOUT)
        .filter(|output| output.status.success())
        .map(|output| parse_model_table(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_default()
}

/// Refresh provider catalogs under the same locks used for settings. Network
/// failures leave both the last catalog and explicit model overrides intact.
fn refresh_provider_models(
    agent: &Path,
    known: &[(String, InventoryModel)],
    force: bool,
) -> Result<bool, String> {
    use muniment_core::{
        model_router::{config::Credential, native_auth},
        provider_models,
    };
    static REFRESH: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _held = REFRESH.lock().unwrap_or_else(|error| error.into_inner());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let mut cache = provider_models::load(agent);
    let auth_file = pi_auth_file(agent);
    let auth = read_json_store(&auth_file)?.unwrap_or_default();
    let mut discovered = Vec::new();
    for (provider, entry) in &auth {
        let oauth = entry["type"] == "oauth";
        if provider_models::endpoint(provider, oauth).is_none() {
            continue;
        }
        let cache_key = format!("provider:{provider}");
        let fresh = cache
            .get(&cache_key)
            .is_some_and(|c| now >= c.checked_ms && now - c.checked_ms < provider_models::TTL_MS);
        if force || !fresh {
            // Refresh a supported subscription before listing models. Re-read
            // and compare under the auth lock so a concurrent sign-in survives.
            let mut current = entry.clone();
            if let Some(credential) = Credential::from_pi_auth(provider, entry) {
                // A subscription copied into the pool shares its refresh token.
                // Let the pool rotate it once, then mirror it to the provider slot.
                let pooled = muniment_core::model_router::config::load(agent).ok().and_then(|config| {
                    config.accounts.into_iter().find(|account| matches!(&account.credential,
                        Credential::Subscription { provider: p, access, refresh, .. }
                        if p == provider && (entry["access"] == *access || refresh.as_deref().is_some_and(|r| entry["refresh"] == r))))
                });
                let refreshed = if let Some(account) = pooled {
                    Some(
                        native_auth::refresh_account(agent, &account.id, now, DISCOVERY_TIMEOUT)
                            .map(|a| a.credential),
                    )
                } else {
                    native_auth::refresh_if_expiring(&credential, now, DISCOVERY_TIMEOUT)
                };
                if let Some(Ok(Credential::Subscription {
                    access,
                    refresh,
                    expires_ms,
                    ..
                })) = refreshed
                {
                    let _lock = lock_pi_auth_file(&auth_file)?;
                    let mut latest = read_json_for_update(&auth_file)?;
                    if latest.get(provider) == Some(entry) {
                        current["access"] = access.into();
                        if let Some(refresh) = refresh {
                            current["refresh"] = refresh.into();
                        }
                        if let Some(expires) = expires_ms {
                            current["expires"] = expires.into();
                        }
                        latest.insert(provider.clone(), current.clone());
                        write_json_for_update(&auth_file, &latest)?;
                    } else {
                        current = latest.get(provider).cloned().unwrap_or_default();
                    }
                }
            }
            if let Some(token) = current
                .get(if oauth { "access" } else { "key" })
                .and_then(serde_json::Value::as_str)
                .filter(|v| !v.is_empty())
            {
                if let Some(models) =
                    provider_models::discover(provider, oauth, token, DISCOVERY_TIMEOUT)
                {
                    cache.insert(
                        cache_key.clone(),
                        provider_models::Catalog {
                            checked_ms: now,
                            models,
                        },
                    );
                }
            }
        }
        if let Some(catalog) = cache.get(&cache_key) {
            let models: Vec<_> = catalog
                .models
                .iter()
                .filter(|model| {
                    !known
                        .iter()
                        .any(|(p, m)| p == provider && model["id"] == m.id)
                })
                .cloned()
                .collect();
            discovered.push((provider.clone(), models));
        }
    }
    // Pool accounts can use a different credential from the provider slot.
    if let Ok(config) = muniment_core::model_router::config::load(agent) {
        for account in config
            .accounts
            .iter()
            .filter(|a| a.enabled && a.base_url.is_none())
        {
            let oauth = matches!(account.credential, Credential::Subscription { .. });
            if provider_models::endpoint(&account.family, oauth).is_none() {
                continue;
            }
            let key = format!("account:{}", account.id);
            if !force
                && cache.get(&key).is_some_and(|c| {
                    now >= c.checked_ms && now - c.checked_ms < provider_models::TTL_MS
                })
            {
                continue;
            }
            let Ok(account) =
                native_auth::refresh_account(agent, &account.id, now, DISCOVERY_TIMEOUT)
            else {
                continue;
            };
            let token = match &account.credential {
                Credential::ApiKey { key } => key,
                Credential::Subscription { access, .. } => access,
            };
            if let Some(models) =
                provider_models::discover(&account.family, oauth, token, DISCOVERY_TIMEOUT)
            {
                cache.insert(
                    key,
                    provider_models::Catalog {
                        checked_ms: now,
                        models,
                    },
                );
            }
        }
    }
    provider_models::save(agent, &cache).map_err(|_| SAVE_SETTINGS_ERROR.to_string())?;
    let models_file = pi_models_file(agent);
    let _lock = lock_pi_auth_file(&models_file)?;
    let mut root = read_json_for_update(&models_file)?;
    let mut changed = false;
    for (provider, models) in discovered {
        if !models.is_empty() {
            changed |= provider_models::merge_models(&mut root, &provider, &models);
        }
    }
    if let (Some(endpoint), Ok(config)) = (
        muniment_core::model_router::server::read_endpoint(agent),
        muniment_core::model_router::config::load(agent),
    ) {
        if config.enabled {
            let before = root
                .get("providers")
                .and_then(|p| p.get(ROUTER_PROVIDER))
                .cloned();
            muniment_core::model_router::pi_provider::register(&mut root, &endpoint, &config);
            changed |=
                before.as_ref() != root.get("providers").and_then(|p| p.get(ROUTER_PROVIDER));
        }
    }
    if changed {
        write_json_for_update(&models_file, &root)?;
    }
    Ok(changed)
}

/// Rewrites each endpoint's model list from what its server serves now, and
/// answers whether any list changed.
fn refresh_endpoint_models(agent: &Path) -> Result<bool, String> {
    let models_file = pi_models_file(agent);
    if !models_file.is_file() {
        return Ok(false);
    }
    let _lock = lock_pi_auth_file(&models_file).map_err(|_| SAVE_SETTINGS_ERROR.to_string())?;
    let mut root = read_json_for_update(&models_file)?;
    let mut changed = false;
    if let Some(providers) = root
        .get_mut("providers")
        .and_then(serde_json::Value::as_object_mut)
    {
        for (_, entry) in providers
            .iter_mut()
            .filter(|(id, _)| {
                id.as_str() != ROUTER_PROVIDER
                    && !KEY_PROVIDERS.iter().any(|(known, _)| known == id)
            })
            .filter_map(|(id, value)| value.as_object_mut().map(|entry| (id, entry)))
        {
            let Some(base_url) = entry.get("baseUrl").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let Some(discovered) =
                muniment_core::endpoint_models::discover_models(base_url, DISCOVERY_TIMEOUT)
            else {
                continue;
            };
            let current: Vec<&str> = entry
                .get("models")
                .and_then(serde_json::Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(|model| model.get("id").and_then(serde_json::Value::as_str))
                        .collect()
                })
                .unwrap_or_default();
            if current != discovered.iter().map(String::as_str).collect::<Vec<_>>() {
                entry.insert(
                    "models".into(),
                    discovered
                        .iter()
                        .map(|id| serde_json::json!({ "id": id }))
                        .collect::<Vec<_>>()
                        .into(),
                );
                changed = true;
            }
        }
    }
    if changed {
        write_json_for_update(&models_file, &root)?;
    }
    Ok(changed)
}

fn provider_inventory(
    agent: &Path,
    models: Vec<(String, InventoryModel)>,
) -> Result<ProviderInventory, String> {
    let auth = read_json_store(&pi_auth_file(agent))?.unwrap_or_default();
    let models_file = read_json_store(&pi_models_file(agent))?.unwrap_or_default();
    let settings = read_json_store(&pi_settings_file(&pi_models_file(agent)))?.unwrap_or_default();
    let record = read_models_record(agent)?;
    let mut providers: Vec<InventoryProvider> = Vec::new();
    for (provider, entry) in auth.iter().filter(|(_, entry)| entry.is_object()) {
        let source = if entry.get("type").and_then(serde_json::Value::as_str) == Some("oauth") {
            "account"
        } else {
            "key"
        };
        providers.push(InventoryProvider {
            id: provider.clone(),
            name: provider_display_name(provider, &record),
            source,
            base_url: None,
            models: Vec::new(),
        });
    }
    if let Some(endpoints) = models_file
        .get("providers")
        .and_then(serde_json::Value::as_object)
    {
        for (provider, entry) in endpoints.iter().filter(|(_, entry)| entry.is_object()) {
            if providers.iter().any(|known| known.id == *provider) {
                continue;
            }
            let source = if provider == ROUTER_PROVIDER {
                "router"
            } else if provider == OLLAMA_PROVIDER || provider == LM_STUDIO_PROVIDER {
                "local"
            } else {
                "custom"
            };
            providers.push(InventoryProvider {
                id: provider.clone(),
                name: provider_display_name(provider, &record),
                source,
                base_url: entry
                    .get("baseUrl")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
                // Endpoint discovery is available before the first chat installs Pi.
                // Keep its model names available while the CLI catalog is absent.
                models: entry
                    .get("models")
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|model| model.get("id").and_then(serde_json::Value::as_str))
                    .filter(|id| !id.trim().is_empty())
                    .map(|id| InventoryModel {
                        id: id.to_owned(),
                        context: String::new(),
                        max_out: String::new(),
                        thinking: false,
                        images: false,
                    })
                    .collect(),
            });
        }
    }
    if record
        .get("claudeCode")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        providers.push(InventoryProvider {
            id: CLAUDE_BRIDGE_PROVIDER.into(),
            name: provider_display_name(CLAUDE_BRIDGE_PROVIDER, &record),
            source: "claude-code",
            base_url: None,
            models: Vec::new(),
        });
    }
    for (provider, model) in models {
        if let Some(known) = providers.iter_mut().find(|known| known.id == provider) {
            if let Some(existing) = known.models.iter_mut().find(|entry| entry.id == model.id) {
                *existing = model;
            } else {
                known.models.push(model);
            }
        }
    }
    Ok(ProviderInventory {
        providers,
        default_provider: settings
            .get("defaultProvider")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        default_model: settings
            .get("defaultModel")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        hidden: record_strings(&record, "hidden"),
        router_classifier: router_classifier(agent),
        router_models: router_models(agent),
    })
}

/// The models the router serves and the accounts behind each: every enabled
/// account of the model's family that can take a turn on it. Empty while the
/// router is off.
fn router_models(agent: &Path) -> Vec<RouterModel> {
    use muniment_core::model_router::options;
    let Ok(config) = muniment_core::model_router::config::load(agent) else {
        return Vec::new();
    };
    if !config.enabled {
        return Vec::new();
    }
    options(&config)
        .into_iter()
        .map(|option| {
            let accounts = config
                .accounts
                .iter()
                .filter(|account| {
                    account.enabled
                        && account.weight > 0
                        && account.credential.servable()
                        && account.family == option.family
                        && account.serves(&option.model)
                })
                .count() as u32;
            RouterModel {
                id: option.key,
                family: option.family,
                model: option.model,
                accounts,
            }
        })
        .collect()
}

/// The classifier the router picks routes with, when it is on and ready. A
/// router with no classifier still balances its pools, so it names none.
fn router_classifier(agent: &Path) -> Option<String> {
    use muniment_core::model_router::{classifies, options};
    let config = muniment_core::model_router::config::load(agent).ok()?;
    if !config.enabled || !classifies(&config, &options(&config)) {
        return None;
    }
    let model = config.classifier.model();
    (!model.is_empty()).then(|| model.to_owned())
}

fn valid_identifier(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && value.trim() == value
        && !value.contains(char::is_control)
}

fn set_default_model(agent: &Path, provider: &str, model: &str) -> Result<(), String> {
    if !valid_identifier(provider, 128) || !valid_identifier(model, 512) {
        return Err("Choose a provider and a model.".into());
    }
    let settings_file = pi_settings_file(&pi_models_file(agent));
    fs::create_dir_all(agent).map_err(|_| SAVE_SETTINGS_ERROR.to_string())?;
    let lock = muniment_core::pi_settings::lock_settings(&settings_file)
        .map_err(|_| SAVE_SETTINGS_ERROR.to_string())?;
    let mut settings = read_json_for_update(&settings_file)?;
    settings.insert("defaultProvider".into(), provider.into());
    settings.insert("defaultModel".into(), model.into());
    merge_pi_settings(&mut settings, PI_SELECTED_ARTIFACT);
    lock.check().map_err(|_| SAVE_SETTINGS_ERROR.to_string())?;
    write_json_for_update(&settings_file, &settings)
}

/// Native Pi compaction settings apply when the next message starts.
#[tauri::command]
pub(crate) fn context_settings() -> Result<serde_json::Value, String> {
    let agent = harness_agent_directory(READ_SETTINGS_ERROR)?;
    read_context_settings(&agent)
}

fn read_context_settings(agent: &Path) -> Result<serde_json::Value, String> {
    let settings = read_json_store(&agent.join("settings.json"))?.unwrap_or_default();
    let compact = settings.get("compaction");
    Ok(serde_json::json!({
        "enabled": compact.and_then(|c| c.get("enabled")).and_then(serde_json::Value::as_bool).unwrap_or(true),
        "reserveTokens": compact.and_then(|c| c.get("reserveTokens")).and_then(serde_json::Value::as_u64).unwrap_or(16384),
        "keepRecentTokens": compact.and_then(|c| c.get("keepRecentTokens")).and_then(serde_json::Value::as_u64).unwrap_or(20000)
    }))
}

#[tauri::command]
pub(crate) fn context_settings_save(
    enabled: bool,
    reserve_tokens: u64,
    keep_recent_tokens: u64,
) -> Result<serde_json::Value, String> {
    let agent = harness_agent_directory(SAVE_SETTINGS_ERROR)?;
    save_context_settings(&agent, enabled, reserve_tokens, keep_recent_tokens)
}

fn save_context_settings(
    agent: &Path,
    enabled: bool,
    reserve_tokens: u64,
    keep_recent_tokens: u64,
) -> Result<serde_json::Value, String> {
    if !(4096..=131072).contains(&reserve_tokens) || !(4096..=131072).contains(&keep_recent_tokens)
    {
        return Err("Choose token counts between 4,096 and 131,072.".into());
    }
    let path = agent.join("settings.json");
    let lock = muniment_core::pi_settings::lock_settings(&path)
        .map_err(|_| SAVE_SETTINGS_ERROR.to_string())?;
    let mut settings = read_json_for_update(&path)?;
    let compact = settings
        .entry("compaction")
        .or_insert_with(|| serde_json::json!({}));
    let object = compact
        .as_object_mut()
        .ok_or("The compaction settings are invalid.")?;
    object.insert("enabled".into(), enabled.into());
    object.insert("reserveTokens".into(), reserve_tokens.into());
    object.insert("keepRecentTokens".into(), keep_recent_tokens.into());
    lock.check().map_err(|_| SAVE_SETTINGS_ERROR.to_string())?;
    write_json_for_update(&path, &settings)?;
    drop(lock);
    read_context_settings(agent)
}

/// Whether Pi's settings name no default model yet.
fn default_model_unset(agent: &Path) -> bool {
    read_json_store(&pi_settings_file(&pi_models_file(agent)))
        .ok()
        .flatten()
        .and_then(|settings| {
            settings
                .get("defaultProvider")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .is_none()
}

/// A context column from Pi's model table, `128K` or `1M`, as a token count.
fn context_tokens(context: &str) -> u64 {
    let digits: String = context
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let value: f64 = digits.parse().unwrap_or(0.0);
    let scale = match context.chars().last() {
        Some('K') | Some('k') => 1_000.0,
        Some('M') | Some('m') => 1_000_000.0,
        _ => 1.0,
    };
    (value * scale) as u64
}

/// The models a just-connected provider may start on, largest context first and
/// list order on a tie. The list is Pi's, so a narrow tuned variant that leads
/// it alphabetically never wins over the provider's general model.
fn adoption_candidates<'a>(models: &'a [(String, InventoryModel)], provider: &str) -> Vec<&'a str> {
    let mut candidates: Vec<&InventoryModel> = models
        .iter()
        .filter(|(known, _)| known == provider)
        .map(|(_, model)| model)
        .collect();
    candidates.sort_by_key(|model| std::cmp::Reverse(context_tokens(&model.context)));
    candidates
        .into_iter()
        .map(|model| model.id.as_str())
        .collect()
}

/// Whether one short exchange with the model comes back as a reply. An account
/// plan serves some of a provider's models and refuses the rest, and only the
/// provider says which.
fn model_answers(agent: &Path, provider: &str, model: &str) -> bool {
    let Some(executable) = pi_executable() else {
        return false;
    };
    let mut command = std::process::Command::new(executable);
    command
        .args(["-ne", "-p", "--provider", provider, "--model", model])
        .arg("Reply with the single word ready.")
        .env("PI_CODING_AGENT_DIR", agent)
        .env("PI_OFFLINE", "1")
        .env_remove("BUN_BE_BUN")
        .stdin(std::process::Stdio::null());
    run_with_timeout(command, PROBE_TIMEOUT).is_some_and(|output| {
        let text = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
        output.status.success() && !text.trim().is_empty() && !text.contains("error")
    })
}

/// Makes a just-connected provider's model the default, so the composer sends
/// to it without a second trip through Settings. With `probe`, the candidates
/// are tried in turn and the first that answers wins.
pub(crate) fn adopt_provider_default(
    agent: &Path,
    provider: &str,
    probe: bool,
) -> Result<(), String> {
    let models = list_pi_models(agent);
    let candidates = adoption_candidates(&models, provider);
    let Some(first) = candidates.first() else {
        return Err("No model answers for this provider yet.".into());
    };
    let chosen = if probe {
        candidates
            .iter()
            .take(PROBE_LIMIT)
            .find(|model| model_answers(agent, provider, model))
            .unwrap_or(first)
    } else {
        first
    };
    set_default_model(agent, provider, chosen)
}

fn set_model_hidden(agent: &Path, provider: &str, model: &str, hidden: bool) -> Result<(), String> {
    if !valid_identifier(provider, 128) || !valid_identifier(model, 512) {
        return Err("Choose a provider and a model.".into());
    }
    let key = format!("{provider}/{model}");
    let mut record = read_models_record(agent)?;
    let mut list = record_strings(&record, "hidden");
    list.retain(|entry| *entry != key);
    if hidden {
        list.push(key);
    }
    record.insert("hidden".into(), list.into());
    write_models_record(agent, &record)
}

fn remove_json_entry(path: &Path, key: &str) -> Result<(), String> {
    let _lock = lock_pi_auth_file(path).map_err(|_| SAVE_SETTINGS_ERROR.to_string())?;
    let mut root = read_json_for_update(path)?;
    let mut changed = root.remove(key).is_some();
    if let Some(providers) = root
        .get_mut("providers")
        .and_then(serde_json::Value::as_object_mut)
    {
        changed |= providers.remove(key).is_some();
    }
    if changed {
        write_json_for_update(path, &root)?;
    }
    Ok(())
}

fn disconnect_provider(agent: &Path, provider: &str) -> Result<(), String> {
    if !valid_identifier(provider, 128) {
        return Err("Choose a provider.".into());
    }
    if provider == CLAUDE_BRIDGE_PROVIDER {
        let mut record = read_models_record(agent)?;
        record.insert("claudeCode".into(), false.into());
        write_models_record(agent, &record)?;
        match fs::remove_file(claude_bridge_config_file(agent)) {
            Ok(()) | Err(_) => {}
        }
    } else {
        if pi_auth_file(agent).is_file() {
            remove_json_entry(&pi_auth_file(agent), provider)?;
        }
        if pi_models_file(agent).is_file() {
            remove_json_entry(&pi_models_file(agent), provider)?;
        }
    }
    let settings_file = pi_settings_file(&pi_models_file(agent));
    if settings_file.is_file() {
        let lock = muniment_core::pi_settings::lock_settings(&settings_file)
            .map_err(|_| SAVE_SETTINGS_ERROR.to_string())?;
        let mut settings = read_json_for_update(&settings_file)?;
        if settings
            .get("defaultProvider")
            .and_then(serde_json::Value::as_str)
            == Some(provider)
        {
            settings.remove("defaultProvider");
            settings.remove("defaultModel");
            merge_pi_settings(&mut settings, PI_SELECTED_ARTIFACT);
            lock.check().map_err(|_| SAVE_SETTINGS_ERROR.to_string())?;
            write_json_for_update(&settings_file, &settings)?;
        }
    }
    Ok(())
}

fn endpoint_url(base_url: &str, message: &str) -> Result<url::Url, String> {
    if base_url.is_empty() || base_url.len() > 16 * 1024 || base_url.trim() != base_url {
        return Err(message.into());
    }
    let parsed = url::Url::parse(base_url).map_err(|_| message.to_owned())?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(message.into());
    }
    Ok(parsed)
}

fn custom_provider_id(name: &str) -> String {
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    format!(
        "{CUSTOM_PROVIDER_PREFIX}{}",
        if slug.is_empty() {
            "endpoint".to_owned()
        } else {
            slug
        }
    )
}

/// An OpenAI-compatible endpoint: LM Studio or a named custom server such as a LiteLLM proxy.
fn store_endpoint_provider(
    agent: &Path,
    kind: &str,
    name: &str,
    base_url: &str,
    key: &str,
    models: &[String],
) -> Result<String, String> {
    const MESSAGE: &str = "Enter a valid server URL.";
    endpoint_url(base_url, MESSAGE)?;
    if key.len() > 16 * 1024 || key.trim() != key {
        return Err("Enter a valid API key.".into());
    }
    let mut models: Vec<String> = models
        .iter()
        .filter(|model| valid_identifier(model, 512))
        .cloned()
        .collect();
    if models.is_empty() {
        models = muniment_core::endpoint_models::discover_models(base_url, DISCOVERY_TIMEOUT)
            .unwrap_or_default();
    }
    if models.is_empty() {
        return Err("The server named no models. Start it, or list its models here.".into());
    }
    let provider = match kind {
        "lmstudio" => LM_STUDIO_PROVIDER.to_owned(),
        "custom" if valid_identifier(name, 80) => custom_provider_id(name),
        _ => return Err("Name the endpoint.".into()),
    };
    let models_file = pi_models_file(agent);
    fs::create_dir_all(agent).map_err(|_| SAVE_SETTINGS_ERROR.to_string())?;
    let _models_lock =
        lock_pi_auth_file(&models_file).map_err(|_| SAVE_SETTINGS_ERROR.to_string())?;
    let mut root = read_json_for_update(&models_file)?;
    let providers = root
        .entry("providers")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| SAVE_SETTINGS_ERROR.to_string())?;
    providers.insert(
        provider.clone(),
        serde_json::json!({
            "baseUrl": base_url,
            "api": "openai-completions",
            "apiKey": if key.is_empty() { "local" } else { key },
            "models": models.iter().map(|id| serde_json::json!({ "id": id })).collect::<Vec<_>>()
        }),
    );
    write_json_for_update(&models_file, &root)?;
    if kind == "custom" {
        let mut record = read_models_record(agent)?;
        let names = record
            .entry("names")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .ok_or_else(|| SAVE_SETTINGS_ERROR.to_string())?;
        names.insert(provider.clone(), name.into());
        write_models_record(agent, &record)?;
    }
    Ok(provider)
}

fn claude_code_executable() -> Option<PathBuf> {
    let home = muniment_core::state_root::home_directory_value().map(PathBuf::from);
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(paths) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&paths).map(|dir| dir.join("claude")));
    }
    if let Some(home) = &home {
        candidates.push(home.join(".local").join("bin").join("claude"));
        candidates.push(home.join(".claude").join("local").join("claude"));
    }
    candidates.push(PathBuf::from("/opt/homebrew/bin/claude"));
    candidates.push(PathBuf::from("/usr/local/bin/claude"));
    candidates.into_iter().find(|path| path.is_file())
}

fn claude_code_status() -> ClaudeCodeStatus {
    let Some(path) = claude_code_executable() else {
        return ClaudeCodeStatus {
            installed: false,
            logged_in: false,
            path: None,
        };
    };
    let mut command = std::process::Command::new(&path);
    command
        .args(["auth", "status"])
        .stdin(std::process::Stdio::null());
    let logged_in = run_with_timeout(command, CLAUDE_STATUS_TIMEOUT)
        .and_then(|output| serde_json::from_slice::<serde_json::Value>(&output.stdout).ok())
        .and_then(|status| status.get("loggedIn").and_then(serde_json::Value::as_bool))
        .unwrap_or(false);
    ClaudeCodeStatus {
        installed: true,
        logged_in,
        path: Some(path.to_string_lossy().into_owned()),
    }
}

/// Connects Anthropic through Claude Code: the bridge reads its executable path, and the desktop lists the provider.
fn connect_claude_code(agent: &Path, executable: &str) -> Result<(), String> {
    if !valid_identifier(executable, 4096) || !Path::new(executable).is_file() {
        return Err("Install Claude Code and sign in, then retry.".into());
    }
    fs::create_dir_all(agent).map_err(|_| SAVE_SETTINGS_ERROR.to_string())?;
    let mut config = read_json_for_update(&claude_bridge_config_file(agent))?;
    let provider = config
        .entry("provider")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| SAVE_SETTINGS_ERROR.to_string())?;
    provider.insert("pathToClaudeCodeExecutable".into(), executable.into());
    write_json_for_update(&claude_bridge_config_file(agent), &config)?;
    let mut record = read_models_record(agent)?;
    record.insert("claudeCode".into(), true.into());
    write_models_record(agent, &record)
}

#[tauri::command]
pub(crate) async fn local_mode_provider_inventory(
    _app: tauri::AppHandle,
    force: Option<bool>,
) -> Result<ProviderInventory, String> {
    let agent = harness_agent_directory(READ_SETTINGS_ERROR)?;
    tauri::async_runtime::spawn_blocking(move || {
        // The endpoint discovery and the Pi listing run side by side, so the
        // page waits for the slower one, not both. A changed endpoint list
        // relists once, so the inventory reads what the server serves now.
        let discovery_agent = agent.clone();
        let discovery = std::thread::spawn(move || refresh_endpoint_models(&discovery_agent));
        let mut models = list_pi_models(&agent);
        let provider_changed =
            refresh_provider_models(&agent, &models, force.unwrap_or(false)).unwrap_or(false);
        if discovery.join().ok().and_then(Result::ok) == Some(true) || provider_changed {
            models = list_pi_models(&agent);
        }
        let mut inventory = provider_inventory(&agent, models)?;
        adopt_shown_default(&agent, &mut inventory);
        Ok(inventory)
    })
    .await
    .map_err(|_| READ_SETTINGS_ERROR.to_string())?
}

/// The chip names the saved default when it is shown, else the first shown
/// model. Pi picks its own fallback when its settings name none, so a run could
/// answer from a model the chip never named. The first shown model becomes the
/// saved default here, so the chip and the run agree.
fn adopt_shown_default(agent: &Path, inventory: &mut ProviderInventory) {
    let hidden = |provider: &str, model: &str| {
        inventory
            .hidden
            .iter()
            .any(|key| key == &format!("{provider}/{model}"))
    };
    let saved_shown =
        match (&inventory.default_provider, &inventory.default_model) {
            (Some(provider), Some(model)) => {
                inventory.providers.iter().any(|group| {
                    &group.id == provider && group.models.iter().any(|m| &m.id == model)
                }) && !hidden(provider, model)
            }
            _ => false,
        };
    if saved_shown {
        return;
    }
    let first = inventory.providers.iter().find_map(|group| {
        group
            .models
            .iter()
            .find(|model| !hidden(&group.id, &model.id))
            .map(|model| (group.id.clone(), model.id.clone()))
    });
    let Some((provider, model)) = first else {
        return;
    };
    if set_default_model(agent, &provider, &model).is_ok() {
        inventory.default_provider = Some(provider);
        inventory.default_model = Some(model);
    }
}

#[tauri::command]
pub(crate) fn local_mode_set_default_model(
    _app: tauri::AppHandle,
    provider: String,
    model: String,
) -> Result<(), String> {
    let agent = harness_agent_directory(SAVE_SETTINGS_ERROR)?;
    set_default_model(&agent, &provider, &model)
}

#[tauri::command]
pub(crate) fn local_mode_set_model_hidden(
    _app: tauri::AppHandle,
    provider: String,
    model: String,
    hidden: bool,
) -> Result<(), String> {
    let agent = harness_agent_directory(SAVE_SETTINGS_ERROR)?;
    set_model_hidden(&agent, &provider, &model, hidden)
}

#[tauri::command]
pub(crate) fn local_mode_disconnect_provider(
    _app: tauri::AppHandle,
    provider: String,
) -> Result<(), String> {
    let agent = harness_agent_directory(SAVE_SETTINGS_ERROR)?;
    disconnect_provider(&agent, &provider)
}

#[tauri::command]
pub(crate) async fn local_mode_store_endpoint(
    _app: tauri::AppHandle,
    kind: String,
    name: String,
    base_url: String,
    key: String,
    models: Vec<String>,
) -> Result<String, String> {
    let agent = harness_agent_directory(SAVE_SETTINGS_ERROR)?;
    tauri::async_runtime::spawn_blocking(move || {
        let provider = store_endpoint_provider(&agent, &kind, &name, &base_url, &key, &models)?;
        if default_model_unset(&agent) {
            let _ = adopt_provider_default(&agent, &provider, false);
        }
        Ok(provider)
    })
    .await
    .map_err(|_| SAVE_SETTINGS_ERROR.to_string())?
}

#[tauri::command]
pub(crate) async fn local_mode_claude_code_status(
    _app: tauri::AppHandle,
) -> Result<ClaudeCodeStatus, String> {
    tauri::async_runtime::spawn_blocking(claude_code_status)
        .await
        .map_err(|_| READ_SETTINGS_ERROR.to_string())
}

#[tauri::command]
pub(crate) fn local_mode_connect_claude_code(
    _app: tauri::AppHandle,
    executable: String,
) -> Result<(), String> {
    let agent = harness_agent_directory(SAVE_SETTINGS_ERROR)?;
    connect_claude_code(&agent, &executable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_connected_provider_adopts_its_largest_context_model_first_listed_on_a_tie() {
        let table = "provider  model                context  max-out  thinking  images\n\
openai-codex  gpt-5.3-codex-spark  128K  16.4K  yes  yes\n\
openai-codex  gpt-5.4              272K  128K   yes  yes\n\
openai-codex  gpt-6-astra          272K  128K   yes  yes\n\
ollama        gemma4:12b           128K  16.4K  no   no\n\
ollama        llama3.2:3b          128K  16.4K  no   no\n\
google        gemini-3-pro         1M    64K    yes  yes\n";
        let models = parse_model_table(table);
        assert_eq!(
            adoption_candidates(&models, "openai-codex"),
            ["gpt-5.4", "gpt-6-astra", "gpt-5.3-codex-spark"]
        );
        assert_eq!(
            adoption_candidates(&models, "ollama"),
            ["gemma4:12b", "llama3.2:3b"]
        );
        assert_eq!(adoption_candidates(&models, "google"), ["gemini-3-pro"]);
        assert!(adoption_candidates(&models, "xai").is_empty());
        assert_eq!(context_tokens("1M"), 1_000_000);
        assert_eq!(context_tokens("16.4K"), 16_400);
    }

    fn temporary_directory() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("muniment-local-mode-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn context_settings_save_returns_persisted_values_without_relocking_the_write() {
        let agent = temporary_directory();
        fs::write(
            agent.join("settings.json"),
            r#"{"defaultModel":"audit-model"}"#,
        )
        .unwrap();
        let saved = save_context_settings(&agent, false, 8192, 12000).unwrap();
        assert_eq!(
            saved,
            serde_json::json!({
                "enabled": false, "reserveTokens": 8192, "keepRecentTokens": 12000
            })
        );
        assert_eq!(read_context_settings(&agent).unwrap(), saved);
        assert_eq!(
            read_json_store(&agent.join("settings.json"))
                .unwrap()
                .unwrap()["defaultModel"],
            "audit-model"
        );
        assert!(save_context_settings(&agent, true, 0, 12000).is_err());
        assert_eq!(read_context_settings(&agent).unwrap(), saved);
        fs::remove_dir_all(agent).unwrap();
    }

    #[test]
    fn the_model_table_parses_pis_list_models_output() {
        let output = "provider  model        context  max-out  thinking  images\nollama    llama3.2:3b  128K     16.4K    no        no\nopenai-codex  gpt-5.5  400K  128K  yes  yes\n";
        let rows = parse_model_table(output);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "ollama");
        assert_eq!(
            rows[0].1,
            InventoryModel {
                id: "llama3.2:3b".into(),
                context: "128K".into(),
                max_out: "16.4K".into(),
                thinking: false,
                images: false
            }
        );
        assert_eq!(rows[1].0, "openai-codex");
        assert!(rows[1].1.thinking && rows[1].1.images);
        assert!(
            parse_model_table("No models available. Use /login to log into a provider.\n")
                .is_empty()
        );
    }

    #[test]
    fn a_missing_default_becomes_the_first_shown_model_so_the_chip_and_the_run_agree() {
        let agent = temporary_directory();
        fs::write(
            agent.join("auth.json"),
            r#"{"openai-codex":{"type":"oauth","access":"a"}}"#,
        )
        .unwrap();
        fs::write(agent.join("settings.json"), r#"{"defaultTools":["read"]}"#).unwrap();
        fs::write(
            agent.join(MODELS_RECORD_FILE),
            r#"{"hidden":["openai-codex/gpt-5.5"]}"#,
        )
        .unwrap();
        let model = |id: &str| InventoryModel {
            id: id.into(),
            context: "400K".into(),
            max_out: "128K".into(),
            thinking: true,
            images: true,
        };
        let models = vec![
            ("openai-codex".to_owned(), model("gpt-5.5")),
            ("openai-codex".to_owned(), model("gpt-5.6-luna")),
        ];
        let mut inventory = provider_inventory(&agent, models).unwrap();
        assert_eq!(inventory.default_model, None);
        adopt_shown_default(&agent, &mut inventory);
        assert_eq!(inventory.default_provider.as_deref(), Some("openai-codex"));
        assert_eq!(inventory.default_model.as_deref(), Some("gpt-5.6-luna"));
        let settings: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(agent.join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(settings["defaultProvider"], "openai-codex");
        assert_eq!(settings["defaultModel"], "gpt-5.6-luna");

        // A shown saved default stays.
        let mut again = provider_inventory(
            &agent,
            vec![
                ("openai-codex".to_owned(), model("gpt-5.5")),
                ("openai-codex".to_owned(), model("gpt-5.6-luna")),
            ],
        )
        .unwrap();
        adopt_shown_default(&agent, &mut again);
        assert_eq!(again.default_model.as_deref(), Some("gpt-5.6-luna"));
    }

    #[test]
    fn endpoint_models_are_selectable_before_the_first_chat_installs_pi() {
        let agent = temporary_directory();
        fs::write(agent.join("models.json"), serde_json::json!({
            "providers": {"ollama": {"baseUrl": "http://localhost:11434/v1", "models": [{"id": "local-model"}]}}
        }).to_string()).unwrap();
        let mut inventory = provider_inventory(&agent, Vec::new()).unwrap();
        adopt_shown_default(&agent, &mut inventory);
        assert_eq!(inventory.default_provider.as_deref(), Some("ollama"));
        assert_eq!(inventory.default_model.as_deref(), Some("local-model"));
        assert_eq!(inventory.providers[0].models[0].id, "local-model");

        let detailed = InventoryModel {
            id: "local-model".into(),
            context: "128K".into(),
            max_out: "8K".into(),
            thinking: true,
            images: true,
        };
        let inventory = provider_inventory(&agent, vec![("ollama".into(), detailed)]).unwrap();
        assert_eq!(inventory.providers[0].models.len(), 1);
        assert_eq!(inventory.providers[0].models[0].context, "128K");
        assert!(inventory.providers[0].models[0].images);
    }

    #[test]
    fn the_inventory_reads_keys_accounts_endpoints_the_bridge_and_the_default() {
        let agent = temporary_directory();
        fs::write(agent.join("auth.json"), r#"{"anthropic":{"type":"api_key","key":"k"},"openai-codex":{"type":"oauth","access":"a"}}"#).unwrap();
        fs::write(agent.join("models.json"), r#"{"providers":{"ollama":{"baseUrl":"http://localhost:11434/v1","api":"openai-completions"},"custom-proxy":{"baseUrl":"http://proxy:4000/v1","api":"openai-completions"}}}"#).unwrap();
        fs::write(
            agent.join("settings.json"),
            r#"{"defaultProvider":"ollama","defaultModel":"llama3.2:3b"}"#,
        )
        .unwrap();
        fs::write(agent.join(MODELS_RECORD_FILE), r#"{"hidden":["anthropic/claude-haiku-4-5"],"names":{"custom-proxy":"My proxy"},"claudeCode":true}"#).unwrap();
        let models = vec![
            (
                "ollama".to_owned(),
                InventoryModel {
                    id: "llama3.2:3b".into(),
                    context: "128K".into(),
                    max_out: "16K".into(),
                    thinking: false,
                    images: false,
                },
            ),
            (
                "anthropic".to_owned(),
                InventoryModel {
                    id: "claude-sonnet-5".into(),
                    context: "1M".into(),
                    max_out: "128K".into(),
                    thinking: true,
                    images: true,
                },
            ),
            (
                "unknown".to_owned(),
                InventoryModel {
                    id: "x".into(),
                    context: "1".into(),
                    max_out: "1".into(),
                    thinking: false,
                    images: false,
                },
            ),
        ];
        let inventory = provider_inventory(&agent, models).unwrap();
        let ids: Vec<(&str, &str)> = inventory
            .providers
            .iter()
            .map(|p| (p.id.as_str(), p.source))
            .collect();
        // Keys first, then endpoints, then the bridge; each group in the file's key order.
        assert_eq!(
            ids,
            vec![
                ("anthropic", "key"),
                ("openai-codex", "account"),
                ("custom-proxy", "custom"),
                ("ollama", "local"),
                ("claude-bridge", "claude-code")
            ]
        );
        let names: Vec<&str> = inventory
            .providers
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["Anthropic", "OpenAI", "My proxy", "Ollama", "Anthropic"]
        );
        assert_eq!(
            inventory.providers[3].base_url.as_deref(),
            Some("http://localhost:11434/v1")
        );
        assert_eq!(inventory.providers[3].models[0].id, "llama3.2:3b");
        assert_eq!(inventory.providers[0].models.len(), 1);
        assert_eq!(inventory.default_provider.as_deref(), Some("ollama"));
        assert_eq!(inventory.default_model.as_deref(), Some("llama3.2:3b"));
        assert_eq!(inventory.hidden, vec!["anthropic/claude-haiku-4-5"]);
        fs::remove_dir_all(agent).unwrap();
    }

    #[test]
    fn the_default_and_the_hidden_list_persist_and_a_disconnect_clears_both() {
        let agent = temporary_directory();
        fs::write(
            agent.join("auth.json"),
            r#"{"anthropic":{"type":"api_key","key":"k"},"google":{"type":"api_key","key":"g"}}"#,
        )
        .unwrap();
        set_default_model(&agent, "anthropic", "claude-sonnet-5").unwrap();
        let settings: serde_json::Value =
            serde_json::from_slice(&fs::read(agent.join("settings.json")).unwrap()).unwrap();
        assert_eq!(settings["defaultProvider"], "anthropic");
        assert_eq!(settings["defaultModel"], "claude-sonnet-5");
        assert!(set_default_model(&agent, "", "x").is_err());

        set_model_hidden(&agent, "anthropic", "claude-haiku-4-5", true).unwrap();
        set_model_hidden(&agent, "google", "gemini", true).unwrap();
        set_model_hidden(&agent, "anthropic", "claude-haiku-4-5", false).unwrap();
        let record = read_models_record(&agent).unwrap();
        assert_eq!(record_strings(&record, "hidden"), vec!["google/gemini"]);

        disconnect_provider(&agent, "anthropic").unwrap();
        let auth: serde_json::Value =
            serde_json::from_slice(&fs::read(agent.join("auth.json")).unwrap()).unwrap();
        assert!(auth.get("anthropic").is_none());
        assert!(auth.get("google").is_some());
        let settings: serde_json::Value =
            serde_json::from_slice(&fs::read(agent.join("settings.json")).unwrap()).unwrap();
        assert!(settings.get("defaultProvider").is_none());
        assert!(settings.get("defaultModel").is_none());
        fs::remove_dir_all(agent).unwrap();
    }

    #[test]
    fn endpoints_land_in_models_json_with_their_name_and_the_bridge_connects_through_the_record() {
        let agent = temporary_directory();
        assert_eq!(
            store_endpoint_provider(
                &agent,
                "lmstudio",
                "LM Studio",
                "http://localhost:1234/v1",
                "",
                &["qwen3".into()]
            )
            .unwrap(),
            "lmstudio"
        );
        let id = store_endpoint_provider(
            &agent,
            "custom",
            "My LiteLLM Proxy",
            "http://proxy:4000/v1",
            "sk-x",
            &["gpt-5.5".into(), "".into()],
        )
        .unwrap();
        assert_eq!(id, "custom-my-litellm-proxy");
        let models: serde_json::Value =
            serde_json::from_slice(&fs::read(agent.join("models.json")).unwrap()).unwrap();
        assert_eq!(models["providers"]["lmstudio"]["apiKey"], "local");
        assert_eq!(models["providers"]["lmstudio"]["models"][0]["id"], "qwen3");
        assert_eq!(models["providers"][&id]["apiKey"], "sk-x");
        assert_eq!(
            models["providers"][&id]["models"].as_array().unwrap().len(),
            1
        );
        let record = read_models_record(&agent).unwrap();
        assert_eq!(record["names"][&id], "My LiteLLM Proxy");
        assert!(
            store_endpoint_provider(&agent, "custom", "x", "ftp://bad", "", &["m".into()]).is_err()
        );
        assert!(store_endpoint_provider(
            &agent,
            "lmstudio",
            "",
            "http://localhost:1234/v1",
            "",
            &[]
        )
        .is_err());
        assert!(store_endpoint_provider(
            &agent,
            "other",
            "x",
            "http://localhost:1234/v1",
            "",
            &["m".into()]
        )
        .is_err());

        let executable = agent.join("claude");
        fs::write(&executable, "#!/bin/sh\n").unwrap();
        connect_claude_code(&agent, executable.to_str().unwrap()).unwrap();
        let config: serde_json::Value =
            serde_json::from_slice(&fs::read(agent.join(CLAUDE_BRIDGE_CONFIG_FILE)).unwrap())
                .unwrap();
        assert_eq!(
            config["provider"]["pathToClaudeCodeExecutable"],
            executable.to_str().unwrap()
        );
        assert_eq!(read_models_record(&agent).unwrap()["claudeCode"], true);
        assert!(connect_claude_code(&agent, "/nowhere/claude").is_err());
        disconnect_provider(&agent, CLAUDE_BRIDGE_PROVIDER).unwrap();
        assert_eq!(read_models_record(&agent).unwrap()["claudeCode"], false);
        assert!(!agent.join(CLAUDE_BRIDGE_CONFIG_FILE).exists());
        fs::remove_dir_all(agent).unwrap();
    }

    #[test]
    fn every_key_provider_stores_a_key_and_an_unknown_one_does_not() {
        let agent = temporary_directory();
        let auth_file = agent.join("auth.json");
        for (provider, _) in KEY_PROVIDERS {
            store_provider_key(&auth_file, provider, "k").unwrap();
        }
        assert!(store_provider_key(&auth_file, "nobody", "k").is_err());
        let auth: serde_json::Value =
            serde_json::from_slice(&fs::read(&auth_file).unwrap()).unwrap();
        assert_eq!(auth.as_object().unwrap().len(), KEY_PROVIDERS.len());
        fs::remove_dir_all(agent).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn desktop_local_mode_directory_matches_the_runtime() {
        let app = tauri::test::mock_app();
        assert_eq!(
            config_directory(app.handle()).unwrap(),
            muniment_runtime::config_directory().unwrap()
        );
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
    fn harness_files_sit_under_the_agent_directory() {
        let agent = Path::new("/home/tester/.muniment/agent");

        assert_eq!(
            pi_auth_file(agent),
            PathBuf::from("/home/tester/.muniment/agent/auth.json")
        );
        assert_eq!(
            pi_models_file(agent),
            PathBuf::from("/home/tester/.muniment/agent/models.json")
        );
        assert_eq!(
            pi_settings_file(&pi_models_file(agent)),
            PathBuf::from("/home/tester/.muniment/agent/settings.json")
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
            "Muniment cannot read provider settings. Check folder access, then retry."
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
            "Muniment cannot read provider settings. Check folder access, then retry."
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn provider_key_command_writes_under_the_agent_directory() {
        let directory = temporary_directory();
        let agent = directory.join("agent");
        let auth_file = pi_auth_file(&agent);

        store_provider_key(&auth_file, "openai", "test-key").unwrap();

        assert_eq!(auth_file, agent.join("auth.json"));
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
