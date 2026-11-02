//! Pi's `models.json` in an agent directory: the local servers and the
//! OpenAI-compatible endpoints Pi reaches, each with the models its server
//! serves and their windows, and the provider models discovery adds. Every
//! write holds the file's lock, and a discovery never replaces a model entry
//! the user wrote. `muniment-models.json` beside it keeps the decision models
//! each endpoint reported.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

use crate::endpoint_models::discover_endpoint_models;
use crate::registry::{
    key_providers, CUSTOM_PROVIDER_PREFIX, LM_STUDIO_PROVIDER, OLLAMA_DECISIONS, OLLAMA_MODEL,
    OLLAMA_PROVIDER, ROUTER_PROVIDER,
};
use crate::window::Limits;

pub const READ_ERROR: &str =
    "Muniment cannot read provider settings. Check folder access, then retry.";
pub const SAVE_ERROR: &str =
    "Muniment cannot save provider settings. Check folder access, then retry.";
/// The desktop's own record beside Pi's files: the models turned off and on,
/// endpoint names, the Claude Code connection and each endpoint's decision models.
pub const RECORD_FILE: &str = "muniment-models.json";
/// The record key that keeps each endpoint's decision models by provider.
const DECISION_MODELS: &str = "decisionModels";
/// The record key that keeps each endpoint's name by provider.
const NAMES: &str = "names";
/// The record key that lists the endpoints whose models the person named,
/// so a refresh keeps exactly those models.
const NAMED_MODELS: &str = "namedModels";
const LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const LOCK_RETRY: Duration = Duration::from_millis(20);
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(4);

/// Pi's model file in an agent directory.
pub fn models_file(agent: &Path) -> PathBuf {
    agent.join("models.json")
}

/// The lock Pi and Muniment share on one of Pi's files: a directory beside
/// it, held until drop.
pub struct FileLock(PathBuf);

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.0);
    }
}

pub fn lock_path(file: &Path) -> PathBuf {
    let mut name = OsString::from(file.as_os_str());
    name.push(".lock");
    PathBuf::from(name)
}

/// Takes the lock on `file`, waiting while another writer holds it.
pub fn lock(file: &Path) -> io::Result<FileLock> {
    let path = lock_path(file);
    let deadline = Instant::now() + LOCK_TIMEOUT;
    loop {
        match fs::create_dir(&path) {
            Ok(()) => return Ok(FileLock(path)),
            Err(error)
                if error.kind() == io::ErrorKind::AlreadyExists && Instant::now() < deadline =>
            {
                std::thread::sleep(LOCK_RETRY);
            }
            Err(error) => return Err(error),
        }
    }
}

/// A JSON object file, empty when the file does not exist.
pub fn read_json(path: &Path) -> io::Result<Map<String, Value>> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Map::new()),
        Err(error) => Err(error),
    }
}

/// Replaces a JSON object file atomically, readable by its owner alone.
pub fn write_json(path: &Path, root: &Map<String, Value>) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(root)?;
    if muniment_atomic_file::holds_private(path, &bytes) {
        return Ok(());
    }
    let temporary = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
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
        // ReplaceFileW opens the replacement without sharing access.
        drop(file);
        muniment_atomic_file::replace(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn read_for_update(path: &Path) -> Result<Map<String, Value>, String> {
    read_json(path).map_err(|_| SAVE_ERROR.to_string())
}

fn write_for_update(path: &Path, root: &Map<String, Value>) -> Result<(), String> {
    write_json(path, root).map_err(|_| SAVE_ERROR.to_string())
}

pub fn read_record(agent: &Path) -> Result<Map<String, Value>, String> {
    match fs::read(agent.join(RECORD_FILE)) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| READ_ERROR.to_string()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Map::new()),
        Err(_) => Err(READ_ERROR.into()),
    }
}

pub fn write_record(agent: &Path, record: &Map<String, Value>) -> Result<(), String> {
    fs::create_dir_all(agent).map_err(|_| SAVE_ERROR.to_string())?;
    write_for_update(&agent.join(RECORD_FILE), record)
}

/// The decision models the record keeps for one endpoint.
pub fn decision_models(record: &Map<String, Value>, provider: &str) -> Vec<String> {
    record
        .get(DECISION_MODELS)
        .and_then(|providers| providers.get(provider))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

/// Whether a provider's model answers a chat turn: not a decision model the
/// record keeps for it and, on Ollama, not one Ollama names as a decision
/// model. Ollama refuses a chat turn to a decision model, so neither becomes
/// the composer's model.
pub fn chat_model(record: &Map<String, Value>, provider: &str, id: &str) -> bool {
    if decision_models(record, provider)
        .iter()
        .any(|known| known == id)
    {
        return false;
    }
    provider != OLLAMA_PROVIDER || !ollama_decision_name(id)
}

/// Whether Ollama names the model as one of its decision models, by the name
/// before the tag.
pub fn ollama_decision_name(id: &str) -> bool {
    let name = id.split(':').next().unwrap_or(id);
    OLLAMA_DECISIONS.iter().any(|(known, _)| *known == name)
}

pub fn store_decision_models(agent: &Path, provider: &str, ids: &[String]) -> Result<(), String> {
    let mut record = read_record(agent)?;
    if decision_models(&record, provider) == ids {
        return Ok(());
    }
    let providers = record.entry(DECISION_MODELS).or_insert_with(|| json!({}));
    if !providers.is_object() {
        *providers = json!({});
    }
    let providers = providers.as_object_mut().expect("an object");
    if ids.is_empty() {
        providers.remove(provider);
    } else {
        providers.insert(provider.to_owned(), json!(ids));
    }
    write_record(agent, &record)
}

/// The strings a record key lists.
fn record_list(record: &Map<String, Value>, key: &str) -> Vec<String> {
    record
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

/// Sets a record key's list, and drops the key when the list is empty.
fn set_record_list(record: &mut Map<String, Value>, key: &str, mut values: Vec<String>) {
    values.sort();
    values.dedup();
    if values.is_empty() {
        record.remove(key);
    } else {
        record.insert(key.into(), json!(values));
    }
}

/// Whether the person named the models of the endpoint `provider`.
pub fn named_models(record: &Map<String, Value>, provider: &str) -> bool {
    record_list(record, NAMED_MODELS)
        .iter()
        .any(|id| id == provider)
}

/// Drops what the record keeps for the endpoint `provider`: its name, its
/// named list and its decision models. Answers whether the record changed.
fn drop_endpoint(record: &mut Map<String, Value>, provider: &str) -> bool {
    let mut changed = false;
    for key in [NAMES, DECISION_MODELS] {
        if let Some(entries) = record.get_mut(key).and_then(Value::as_object_mut) {
            changed |= entries.remove(provider).is_some();
            if entries.is_empty() {
                record.remove(key);
            }
        }
    }
    let named = record_list(record, NAMED_MODELS);
    if named.iter().any(|id| id == provider) {
        let rest = named.into_iter().filter(|id| id != provider).collect();
        set_record_list(record, NAMED_MODELS, rest);
        changed = true;
    }
    changed
}

/// Drops what the record keeps for the endpoint `provider`, as after a
/// disconnect.
pub fn forget_endpoint(agent: &Path, provider: &str) -> Result<(), String> {
    let path = agent.join(RECORD_FILE);
    if !path.is_file() {
        return Ok(());
    }
    let mut record = read_record(agent)?;
    if drop_endpoint(&mut record, provider) {
        write_record(agent, &record)?;
    }
    Ok(())
}

/// Drops what the record keeps for each endpoint `models.json` no longer
/// holds, and answers how many endpoints it dropped. A `models.json` that
/// is missing or unreadable drops nothing, so a name never leaves while its
/// endpoint may exist.
pub fn sweep_endpoints(agent: &Path) -> Result<usize, String> {
    let models_file = models_file(agent);
    if !models_file.is_file() || !agent.join(RECORD_FILE).is_file() {
        return Ok(0);
    }
    let Ok(root) = read_json(&models_file) else {
        return Ok(0);
    };
    let Some(providers) = root.get("providers").and_then(Value::as_object) else {
        return Ok(0);
    };
    let mut record = read_record(agent)?;
    let mut kept: Vec<String> = Vec::new();
    for key in [NAMES, DECISION_MODELS] {
        kept.extend(
            record
                .get(key)
                .and_then(Value::as_object)
                .into_iter()
                .flat_map(|entries| entries.keys().cloned()),
        );
    }
    kept.extend(record_list(&record, NAMED_MODELS));
    kept.sort();
    kept.dedup();
    let mut dropped = 0;
    for provider in kept.iter().filter(|id| !providers.contains_key(*id)) {
        if drop_endpoint(&mut record, provider) {
            dropped += 1;
        }
    }
    if dropped > 0 {
        write_record(agent, &record)?;
    }
    Ok(dropped)
}

/// One endpoint model as Pi reads it, with its window when the server
/// reported one. Pi assumes its own defaults for a window it is not told.
pub fn model_entry(id: &str, window: Option<&Limits>) -> Value {
    let mut entry = json!({ "id": id });
    if let Some(context) = window.and_then(|window| window.context) {
        entry["contextWindow"] = context.into();
    }
    if let Some(output) = window.and_then(|window| window.output) {
        entry["maxTokens"] = output.into();
    }
    entry
}

fn model_entries(ids: &[String], windows: &BTreeMap<String, Limits>) -> Vec<Value> {
    ids.iter()
        .map(|id| model_entry(id, windows.get(id)))
        .collect()
}

/// How long a key command may run before the key counts as unresolved.
const KEY_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

/// The key a `models.json` `apiKey` value stands for. A value that starts with
/// `!` is a command, as Pi reads it: the key is its trimmed standard output,
/// and empty output, a failure or a timeout leaves it unresolved. Any other
/// value is the key itself.
pub fn resolve_key(value: &str) -> Option<String> {
    let Some(command) = value.strip_prefix('!') else {
        return (!value.is_empty()).then(|| value.to_owned());
    };
    let output = run_key_command(command)?;
    let key = String::from_utf8(output).ok()?.trim().to_owned();
    (!key.is_empty()).then_some(key)
}

/// Runs a key command through the platform shell and answers its standard
/// output when it exits zero within [`KEY_COMMAND_TIMEOUT`].
fn run_key_command(command: &str) -> Option<Vec<u8>> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    #[cfg(unix)]
    let mut shell = {
        let mut shell = Command::new("/bin/sh");
        shell.arg("-c").arg(command);
        shell
    };
    #[cfg(windows)]
    let mut shell = {
        use std::os::windows::process::CommandExt;
        // /S strips the outer quotes, so cmd runs the command exactly as written.
        let mut shell = Command::new("cmd");
        shell
            .args(["/D", "/S", "/C"])
            .raw_arg(format!("\"{command}\""));
        shell
    };
    let mut child = shell
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let deadline = Instant::now() + KEY_COMMAND_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(LOCK_RETRY),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let bytes = reader.join().ok()?.ok()?;
    status.success().then_some(bytes)
}

/// The `apiKey` value an endpoint provider already holds, never a stand-in: the
/// key or the command that answers it. Settings never reads a key back to show
/// it, so an edit with a blank key keeps this one.
pub fn saved_endpoint_key(models_file: &Path, provider: &str) -> Option<String> {
    let root: Value = serde_json::from_slice(&fs::read(models_file).ok()?).ok()?;
    root["providers"][provider]["apiKey"]
        .as_str()
        .filter(|key| !matches!(*key, "ollama" | "local") && !key.is_empty())
        .map(str::to_owned)
}

/// The limits an endpoint provider's saved models carry, by model id. A model
/// with no known limit has no entry.
fn saved_endpoint_windows(models_file: &Path, provider: &str) -> BTreeMap<String, Limits> {
    let Some(root) = fs::read(models_file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
    else {
        return BTreeMap::new();
    };
    root["providers"][provider]["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let limits = Limits::of_entry(entry);
            let known = limits.context.is_some() || limits.output.is_some();
            entry["id"]
                .as_str()
                .filter(|_| known)
                .map(|id| (id.to_owned(), limits))
        })
        .collect()
}

pub fn valid_identifier(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && value.trim() == value
        && !value.contains(char::is_control)
}

/// A plain http or https server URL with no credentials, query or fragment.
pub fn endpoint_url(base_url: &str, message: &str) -> Result<url::Url, String> {
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

pub fn custom_provider_id(name: &str) -> String {
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

/// Saves an Ollama server. A server on another host may sit behind a key, which
/// discovery sends and Pi sends with every turn. With no key, Pi sends a
/// stand-in. `route` makes the server's default model Pi's default before the
/// model file is written, so a later write failure cannot fall back to a cloud
/// model.
pub fn store_ollama(
    agent: &Path,
    base_url: &str,
    key: &str,
    route: impl FnOnce(&str) -> Result<(), String>,
) -> Result<(), String> {
    const MESSAGE: &str = "Enter a valid Ollama server URL.";
    if base_url.is_empty() || base_url.len() > 16 * 1024 || base_url.trim() != base_url {
        return Err(MESSAGE.into());
    }
    if key.len() > 16 * 1024 || key.trim() != key {
        return Err("Enter a valid API key.".into());
    }
    let models_file = models_file(agent);
    let saved = if key.is_empty() {
        saved_endpoint_key(&models_file, OLLAMA_PROVIDER)
    } else {
        None
    };
    let key = saved.as_deref().unwrap_or(key);
    endpoint_url(base_url, MESSAGE)?;

    fs::create_dir_all(agent).map_err(|_| SAVE_ERROR.to_string())?;
    let _models_lock = lock(&models_file).map_err(|_| SAVE_ERROR.to_string())?;
    let mut models = read_for_update(&models_file)?;
    let providers = models
        .entry("providers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| SAVE_ERROR.to_string())?;
    // The server names its own models; the pinned id stands in when it does not answer.
    let discovered = discover_endpoint_models(
        base_url,
        resolve_key(key).as_deref().or(Some(key)),
        DISCOVERY_TIMEOUT,
    );
    let (model_ids, decisions, windows) = match discovered {
        Some(found) => (found.chat, found.decisions, found.windows),
        None => (vec![OLLAMA_MODEL.to_owned()], Vec::new(), BTreeMap::new()),
    };
    if model_ids.is_empty() {
        return Err("The server has no chat models. Add a chat model, then try again.".into());
    }
    // A decision model Ollama lists beside its chat models never becomes the
    // default, so the first chat turn reaches a model that answers it.
    let default_model = if model_ids.iter().any(|id| id == OLLAMA_MODEL) {
        OLLAMA_MODEL
    } else {
        model_ids
            .iter()
            .find(|id| !ollama_decision_name(id))
            .unwrap_or(&model_ids[0])
            .as_str()
    };
    providers.insert(
        OLLAMA_PROVIDER.to_owned(),
        json!({
            "baseUrl": base_url,
            "api": "openai-completions",
            "apiKey": if key.is_empty() { OLLAMA_PROVIDER } else { key },
            "compat": {
                "supportsDeveloperRole": false,
                "supportsReasoningEffort": false
            },
            "models": model_entries(&model_ids, &windows)
        }),
    );
    route(default_model)?;
    write_for_update(&models_file, &models)?;
    store_decision_models(agent, OLLAMA_PROVIDER, &decisions)
}

/// Saves an OpenAI-compatible endpoint: LM Studio or a named custom server
/// such as a LiteLLM proxy. With no models named, the server names its own.
/// Answers the provider id.
pub fn store_endpoint(
    agent: &Path,
    kind: &str,
    name: &str,
    base_url: &str,
    key: &str,
    models: &[String],
) -> Result<String, String> {
    endpoint_url(base_url, "Enter a valid server URL.")?;
    if key.len() > 16 * 1024 || key.trim() != key {
        return Err("Enter a valid API key.".into());
    }
    let provider = match kind {
        "lmstudio" => LM_STUDIO_PROVIDER.to_owned(),
        "custom" if valid_identifier(name, 80) => custom_provider_id(name),
        _ => return Err("Name the endpoint.".into()),
    };
    let models_file = models_file(agent);
    let saved = if key.is_empty() {
        saved_endpoint_key(&models_file, &provider)
    } else {
        None
    };
    let key = saved.as_deref().unwrap_or(key);
    let mut models: Vec<String> = models
        .iter()
        .filter(|model| valid_identifier(model, 512))
        .cloned()
        .collect();
    let mut decisions = None;
    let mut windows = saved_endpoint_windows(&models_file, &provider);
    windows.retain(|id, _| models.contains(id));
    if !models.is_empty() && models.iter().any(|id| !windows.contains_key(id)) {
        // A named list keeps the limits it holds and asks the server for the
        // limits of every model it names for the first time.
        if let Some(found) = discover_endpoint_models(
            base_url,
            resolve_key(key).as_deref().or(Some(key)),
            DISCOVERY_TIMEOUT,
        ) {
            for (id, limits) in found.windows {
                if models.contains(&id) {
                    windows.entry(id).or_insert(limits);
                }
            }
        }
    }
    if models.is_empty() {
        let found = discover_endpoint_models(
            base_url,
            resolve_key(key).as_deref().or(Some(key)),
            DISCOVERY_TIMEOUT,
        )
        .unwrap_or_default();
        models = found.chat;
        windows = found.windows;
        decisions = Some(found.decisions);
    }
    if models.is_empty() {
        return Err("The server named no models. Start it, or list its models here.".into());
    }
    fs::create_dir_all(agent).map_err(|_| SAVE_ERROR.to_string())?;
    let _models_lock = lock(&models_file).map_err(|_| SAVE_ERROR.to_string())?;
    let mut root = read_for_update(&models_file)?;
    let providers = root
        .entry("providers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| SAVE_ERROR.to_string())?;
    providers.insert(
        provider.clone(),
        json!({
            "baseUrl": base_url,
            "api": "openai-completions",
            "apiKey": if key.is_empty() { "local" } else { key },
            "models": model_entries(&models, &windows)
        }),
    );
    write_for_update(&models_file, &root)?;
    let mut record = read_record(agent)?;
    if kind == "custom" {
        let names = record
            .entry(NAMES)
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or_else(|| SAVE_ERROR.to_string())?;
        names.insert(provider.clone(), name.into());
    }
    let mut named: Vec<String> = record_list(&record, NAMED_MODELS)
        .into_iter()
        .filter(|id| *id != provider)
        .collect();
    if decisions.is_none() {
        named.push(provider.clone());
    }
    set_record_list(&mut record, NAMED_MODELS, named);
    write_record(agent, &record)?;
    // A named list names no decision model.
    store_decision_models(agent, &provider, &decisions.unwrap_or_default())?;
    Ok(provider)
}

/// Each server Pi reaches through `models.json`, a local server or a custom
/// endpoint, as its provider id, the name the record gives it and its base URL.
pub fn endpoints(agent: &Path) -> Result<Vec<(String, String, String)>, String> {
    let root = match read_json(&models_file(agent)) {
        Ok(root) => root,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(READ_ERROR.into()),
    };
    let record = read_record(agent)?;
    Ok(root
        .get("providers")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(id, _)| {
            id.as_str() != ROUTER_PROVIDER && !key_providers().any(|(known, _)| known == *id)
        })
        .filter_map(|(id, entry)| {
            let base_url = entry.get("baseUrl").and_then(Value::as_str)?;
            let name = record
                .get("names")
                .and_then(|names| names.get(id))
                .and_then(Value::as_str)
                .unwrap_or(id);
            Some((id.clone(), name.to_owned(), base_url.to_owned()))
        })
        .collect())
}

/// Rewrites each endpoint's model list from what its server serves now, with
/// each model's window, and answers whether any list changed. A model the
/// server still serves keeps the settings the user gave it. A server that
/// lists no chat model keeps its saved list. An endpoint whose models the
/// person named keeps exactly those models, and the server only adds the
/// limits a model lacks.
pub fn refresh_endpoints(agent: &Path) -> Result<bool, String> {
    refresh_endpoints_with(agent, &resolve_key)
}

/// [`refresh_endpoints`], with `key_for` answering the key each server's list
/// read sends for its `apiKey` value. A host whose keys sit behind a prompt
/// answers only a key it already holds, since a list read is no request
/// that needs the key.
pub fn refresh_endpoints_with(
    agent: &Path,
    key_for: &(dyn Fn(&str) -> Option<String> + Sync),
) -> Result<bool, String> {
    let models_file = models_file(agent);
    if !models_file.is_file() {
        return Ok(false);
    }
    let _lock = lock(&models_file).map_err(|_| SAVE_ERROR.to_string())?;
    let mut root = read_for_update(&models_file)?;
    let record = read_record(agent)?;
    let mut changed = false;
    let mut decisions = Vec::new();
    if let Some(providers) = root.get_mut("providers").and_then(Value::as_object_mut) {
        for (id, entry) in providers
            .iter_mut()
            .filter(|(id, _)| {
                id.as_str() != ROUTER_PROVIDER && !key_providers().any(|(known, _)| known == *id)
            })
            .filter_map(|(id, value)| value.as_object_mut().map(|entry| (id, entry)))
        {
            let Some(base_url) = entry.get("baseUrl").and_then(Value::as_str) else {
                continue;
            };
            // The stand-in keys Pi sends to a server with no key are not keys.
            let key = entry
                .get("apiKey")
                .and_then(Value::as_str)
                .filter(|key| !matches!(*key, "ollama" | "local"))
                .and_then(key_for);
            // A server that answers with no chat model is still loading, as
            // a local server does for a while after it starts, so the saved
            // list stays until it serves one.
            let Some(found) = discover_endpoint_models(base_url, key.as_deref(), DISCOVERY_TIMEOUT)
                .filter(|found| !found.chat.is_empty())
            else {
                continue;
            };
            let current = entry
                .get("models")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if named_models(&record, id) {
                // The person named these models, so the list stays as it
                // is and the server only adds the limits a model lacks.
                let next: Vec<Value> = current
                    .iter()
                    .map(|old| {
                        let mut next = old.clone();
                        let window = old
                            .get("id")
                            .and_then(Value::as_str)
                            .and_then(|model| found.windows.get(model).map(|w| (model, w)));
                        if let (Some((model, window)), Some(fields)) =
                            (window, next.as_object_mut())
                        {
                            let reported = model_entry(model, Some(window));
                            for field in ["contextWindow", "maxTokens"] {
                                if let Some(value) = reported.get(field) {
                                    fields.entry(field).or_insert_with(|| value.clone());
                                }
                            }
                        }
                        next
                    })
                    .collect();
                if current != next {
                    entry.insert("models".into(), next.into());
                    changed = true;
                }
                continue;
            }
            decisions.push((id.clone(), found.decisions));
            let next: Vec<Value> = found
                .chat
                .iter()
                .map(|model| {
                    let mut next = current
                        .iter()
                        .find(|old| old.get("id").and_then(Value::as_str) == Some(model))
                        .filter(|old| old.is_object())
                        .cloned()
                        .unwrap_or_else(|| json!({ "id": model }));
                    if let Some(window) = found.windows.get(model) {
                        let reported = model_entry(model, Some(window));
                        for field in ["contextWindow", "maxTokens"] {
                            if let Some(value) = reported.get(field) {
                                next[field] = value.clone();
                            }
                        }
                    }
                    next
                })
                .collect();
            if current != next {
                entry.insert("models".into(), next.into());
                changed = true;
            }
        }
    }
    if changed {
        write_for_update(&models_file, &root)?;
    }
    for (provider, ids) in decisions {
        let before = decision_models(&read_record(agent)?, &provider);
        if before != ids {
            store_decision_models(agent, &provider, &ids)?;
            changed = true;
        }
    }
    Ok(changed)
}

/// Adds the models each provider's discovery found to Pi's model file without
/// replacing a model entry already there, then lets `register` change the file
/// under the same lock. Answers whether the file changed.
pub fn merge_discovered(
    agent: &Path,
    discovered: &[(String, Vec<Value>)],
    register: impl FnOnce(&mut Map<String, Value>) -> bool,
) -> Result<bool, String> {
    let models_file = models_file(agent);
    let _lock = lock(&models_file).map_err(|_| SAVE_ERROR.to_string())?;
    let mut root = read_for_update(&models_file)?;
    let mut changed = false;
    for (provider, models) in discovered {
        if !models.is_empty() {
            changed |= crate::provider_models::merge_models(&mut root, provider, models);
        }
    }
    changed |= register(&mut root);
    if changed {
        write_for_update(&models_file, &root)?;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_server::serve_routes;

    fn temporary_directory() -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("muniment-pi-models-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn write_json_leaves_an_unchanged_file_alone() {
        use std::os::unix::fs::MetadataExt;
        let directory = temporary_directory();
        let path = directory.join("auth.json");
        let mut root = Map::new();
        root.insert("provider".into(), json!({"key": "value"}));
        write_json(&path, &root).unwrap();
        let first = fs::metadata(&path).unwrap().ino();
        write_json(&path, &root).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().ino(), first);
        root.insert("other".into(), json!({}));
        write_json(&path, &root).unwrap();
        assert_ne!(fs::metadata(&path).unwrap().ino(), first);
        assert_eq!(read_json(&path).unwrap(), root);
        fs::remove_dir_all(directory).unwrap();
    }

    fn models(agent: &Path) -> Value {
        serde_json::from_slice(&fs::read(models_file(agent)).unwrap()).unwrap()
    }

    fn ollama_server() -> String {
        serve_routes(vec![
            (
                "GET /api/tags",
                json!({"models": [{"name": "qwen3:8b"}, {"name": "clef"}]}),
            ),
            (
                "POST /api/show qwen3:8b",
                json!({"model_info": {"general.architecture": "qwen3", "qwen3.context_length": 40960}}),
            ),
            (
                "POST /api/show clef",
                json!({"capabilities": ["completion"]}),
            ),
            ("GET /api/ps", json!({"models": []})),
        ])
    }

    #[test]
    fn an_ollama_server_lands_with_each_models_window_after_its_route() {
        let agent = temporary_directory();
        let url = ollama_server();
        let mut routed = None;
        store_ollama(&agent, &url, "", |model| {
            routed = Some(model.to_owned());
            Ok(())
        })
        .unwrap();
        assert_eq!(routed.as_deref(), Some("qwen3:8b"));
        assert_eq!(
            models(&agent)["providers"]["ollama"]["models"],
            json!([{"id": "qwen3:8b", "contextWindow": 40960}, {"id": "clef"}])
        );
        assert_eq!(models(&agent)["providers"]["ollama"]["apiKey"], "ollama");
        // A route that fails leaves the model file as it was.
        let before = fs::read(models_file(&agent)).unwrap();
        assert!(store_ollama(&agent, &url, "sk-new", |_| Err(SAVE_ERROR.into())).is_err());
        assert_eq!(fs::read(models_file(&agent)).unwrap(), before);
        assert!(!lock_path(&models_file(&agent)).exists());
        fs::remove_dir_all(agent).unwrap();
    }

    #[test]
    fn an_ollama_server_routes_to_a_chat_model_and_keeps_its_decision_models_apart() {
        let agent = temporary_directory();
        let url = serve_routes(vec![
            (
                "GET /api/tags",
                json!({"models": [{"name": "clef-flash:latest"}, {"name": "nimble:latest"}, {"name": "gpt-oss:latest"}]}),
            ),
            (
                "POST /api/show clef-flash:latest",
                json!({"capabilities": ["decisions"]}),
            ),
            ("POST /api/show nimble:latest", json!({})),
            (
                "POST /api/show gpt-oss:latest",
                json!({"capabilities": ["completion", "tools"]}),
            ),
            ("GET /api/ps", json!({"models": []})),
        ]);
        let mut routed = None;
        store_ollama(&agent, &url, "", |model| {
            routed = Some(model.to_owned());
            Ok(())
        })
        .unwrap();
        assert_eq!(routed.as_deref(), Some("gpt-oss:latest"));
        assert_eq!(
            models(&agent)["providers"]["ollama"]["models"],
            json!([{"id": "nimble:latest"}, {"id": "gpt-oss:latest"}])
        );
        assert_eq!(
            decision_models(&read_record(&agent).unwrap(), OLLAMA_PROVIDER),
            ["clef-flash:latest"]
        );
        fs::remove_dir_all(agent).unwrap();
    }

    #[test]
    fn a_discovered_endpoint_carries_its_windows_and_a_named_one_none() {
        let agent = temporary_directory();
        let url = serve_routes(vec![(
            "GET /v1/models",
            json!({"data": [{"id": "qwen3", "max_model_len": 32768}]}),
        )]);
        let id = store_endpoint(&agent, "custom", "vLLM", &url, "", &[]).unwrap();
        assert_eq!(id, "custom-vllm");
        assert_eq!(
            models(&agent)["providers"][&id]["models"],
            json!([{"id": "qwen3", "contextWindow": 32768}])
        );
        assert_eq!(read_record(&agent).unwrap()["names"][&id], "vLLM");
        store_endpoint(&agent, "lmstudio", "", &url, "", &["named".into()]).unwrap();
        assert_eq!(
            models(&agent)["providers"]["lmstudio"]["models"],
            json!([{"id": "named"}])
        );
        fs::remove_dir_all(agent).unwrap();
    }

    #[test]
    fn a_named_list_keeps_the_saved_limits_and_asks_the_server_for_new_ones() {
        let agent = temporary_directory();
        let url = serve_routes(vec![(
            "GET /v1/models",
            json!({"data": [
                {"id": "qwen3", "max_model_len": 32768},
                {"id": "gemma", "max_model_len": 8192}
            ]}),
        )]);
        let id = store_endpoint(&agent, "custom", "oMLX", &url, "", &[]).unwrap();
        // A limit the user set stays over the one the server reports.
        let mut root = models(&agent);
        root["providers"][&id]["models"][0]["contextWindow"] = 16384.into();
        root["providers"][&id]["models"][0]["maxTokens"] = 4096.into();
        fs::write(models_file(&agent), root.to_string()).unwrap();
        store_endpoint(
            &agent,
            "custom",
            "oMLX",
            &url,
            "",
            &["qwen3".into(), "gemma".into(), "unlisted".into()],
        )
        .unwrap();
        assert_eq!(
            models(&agent)["providers"][&id]["models"],
            json!([
                {"id": "qwen3", "contextWindow": 16384, "maxTokens": 4096},
                {"id": "gemma", "contextWindow": 8192},
                {"id": "unlisted"}
            ])
        );
        fs::remove_dir_all(agent).unwrap();
    }

    #[test]
    fn a_refresh_follows_the_server_and_keeps_what_the_user_set() {
        let agent = temporary_directory();
        let url = ollama_server();
        fs::write(
            models_file(&agent),
            json!({"providers": {
                "ollama": {"baseUrl": url, "apiKey": "ollama", "models": [
                    {"id": "qwen3:8b", "reasoning": true, "contextWindow": 8192},
                    {"id": "gone"}
                ]},
                "openai": {"baseUrl": "http://127.0.0.1:9/v1", "models": [{"id": "kept"}]}
            }})
            .to_string(),
        )
        .unwrap();
        assert!(refresh_endpoints(&agent).unwrap());
        let root = models(&agent);
        assert_eq!(
            root["providers"]["ollama"]["models"],
            json!([{"id": "qwen3:8b", "reasoning": true, "contextWindow": 40960}, {"id": "clef"}])
        );
        assert_eq!(root["providers"]["openai"]["models"][0]["id"], "kept");
        assert!(!refresh_endpoints(&agent).unwrap());
        fs::remove_dir_all(agent).unwrap();
    }

    #[test]
    fn a_refresh_keeps_exactly_the_models_the_person_named() {
        let agent = temporary_directory();
        let url = serve_routes(vec![(
            "GET /v1/models",
            json!({"data": [
                {"id": "served", "max_model_len": 32768},
                {"id": "other", "max_model_len": 8192}
            ]}),
        )]);
        // A one-id Connect keeps one id, and an id the server does not list stays.
        let id = store_endpoint(
            &agent,
            "custom",
            "Live29",
            &url,
            "",
            &["served".into(), "unlisted".into()],
        )
        .unwrap();
        let mut root = models(&agent);
        root["providers"][&id]["models"][0]["contextWindow"] = 16384.into();
        fs::write(models_file(&agent), root.to_string()).unwrap();
        refresh_endpoints(&agent).unwrap();
        assert_eq!(
            models(&agent)["providers"][&id]["models"],
            json!([{"id": "served", "contextWindow": 16384}, {"id": "unlisted"}])
        );
        // The server adds a limit a named model lacks.
        let mut root = models(&agent);
        root["providers"][&id]["models"][0]
            .as_object_mut()
            .unwrap()
            .remove("contextWindow");
        fs::write(models_file(&agent), root.to_string()).unwrap();
        assert!(refresh_endpoints(&agent).unwrap());
        assert_eq!(
            models(&agent)["providers"][&id]["models"],
            json!([{"id": "served", "contextWindow": 32768}, {"id": "unlisted"}])
        );
        assert!(!refresh_endpoints(&agent).unwrap());
        assert!(decision_models(&read_record(&agent).unwrap(), &id).is_empty());
        // An empty list takes the server's list, and its refresh follows the server.
        store_endpoint(&agent, "custom", "Live29", &url, "", &[]).unwrap();
        assert!(!named_models(&read_record(&agent).unwrap(), &id));
        let ids = |agent: &Path| {
            models(agent)["providers"][&id]["models"]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| entry["id"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&agent), ["served", "other"]);
        fs::remove_dir_all(agent).unwrap();
    }

    #[test]
    fn a_disconnected_endpoint_leaves_the_record_and_a_sweep_keeps_live_ones() {
        let agent = temporary_directory();
        fs::write(
            models_file(&agent),
            json!({"providers": {
                "custom-live30": {"baseUrl": "http://127.0.0.1:9/v1", "models": [{"id": "m"}]}
            }})
            .to_string(),
        )
        .unwrap();
        write_record(
            &agent,
            json!({
                "names": {"custom-live27": "Live27", "custom-live30": "Live30"},
                "namedModels": ["custom-live27", "custom-live30"],
                "decisionModels": {"custom-live28": ["kev"]},
                "hidden": ["custom-live30/m"]
            })
            .as_object()
            .unwrap(),
        )
        .unwrap();
        assert_eq!(sweep_endpoints(&agent).unwrap(), 2);
        assert_eq!(
            Value::Object(read_record(&agent).unwrap()),
            json!({
                "names": {"custom-live30": "Live30"},
                "namedModels": ["custom-live30"],
                "hidden": ["custom-live30/m"]
            })
        );
        assert_eq!(sweep_endpoints(&agent).unwrap(), 0);
        forget_endpoint(&agent, "custom-live30").unwrap();
        assert_eq!(
            Value::Object(read_record(&agent).unwrap()),
            json!({"hidden": ["custom-live30/m"]})
        );
        // A missing model file drops no name.
        write_record(
            &agent,
            json!({"names": {"custom-live31": "Live31"}})
                .as_object()
                .unwrap(),
        )
        .unwrap();
        fs::remove_file(models_file(&agent)).unwrap();
        assert_eq!(sweep_endpoints(&agent).unwrap(), 0);
        assert_eq!(
            read_record(&agent).unwrap()["names"]["custom-live31"],
            "Live31"
        );
        fs::remove_dir_all(agent).unwrap();
    }

    #[test]
    fn a_server_that_lists_no_model_yet_keeps_the_saved_list() {
        let agent = temporary_directory();
        let url = serve_routes(vec![(
            "GET /v1/models",
            json!({"object": "list", "data": []}),
        )]);
        let saved = json!({"providers": {
            "custom-omlx": {"baseUrl": url, "apiKey": "k", "models": [{"id": "qwen3"}]}
        }});
        fs::write(models_file(&agent), saved.to_string()).unwrap();
        assert!(!refresh_endpoints(&agent).unwrap());
        assert_eq!(models(&agent), saved);
        fs::remove_dir_all(agent).unwrap();
    }

    #[test]
    fn a_merge_adds_discoveries_and_registers_under_one_lock() {
        let agent = temporary_directory();
        fs::write(
            models_file(&agent),
            json!({"providers": {"xai": {"models": [{"id": "grok-old", "contextWindow": 42}]}}})
                .to_string(),
        )
        .unwrap();
        let discovered = vec![(
            "xai".to_owned(),
            vec![
                json!({"id": "grok-old", "contextWindow": 100}),
                json!({"id": "grok-new"}),
            ],
        )];
        let changed = merge_discovered(&agent, &discovered, |root| {
            assert!(lock_path(&models_file(&agent)).exists());
            root.insert("registered".into(), true.into());
            true
        })
        .unwrap();
        assert!(changed);
        let root = models(&agent);
        assert_eq!(root["providers"]["xai"]["models"][0]["contextWindow"], 42);
        assert_eq!(root["providers"]["xai"]["models"][1]["id"], "grok-new");
        assert_eq!(root["registered"], true);
        assert!(!merge_discovered(&agent, &discovered, |_| false).unwrap());
        fs::remove_dir_all(agent).unwrap();
    }

    #[test]
    fn a_decision_model_never_reads_as_a_chat_model() {
        let agent = temporary_directory();
        store_decision_models(&agent, "llama", &["kev-4b".to_owned()]).unwrap();
        let record = read_record(&agent).unwrap();
        assert!(!chat_model(&record, "llama", "kev-4b"));
        assert!(chat_model(&record, "llama", "qwen3"));
        assert!(!chat_model(&record, OLLAMA_PROVIDER, "clef-flash:latest"));
        assert!(!chat_model(&record, OLLAMA_PROVIDER, "nimble"));
        assert!(chat_model(&record, OLLAMA_PROVIDER, "gpt-oss:latest"));
        // Another server may name a chat model like an Ollama decision model.
        assert!(chat_model(&record, "custom-x", "clef-flash"));
        fs::remove_dir_all(agent).unwrap();
    }

    #[test]
    fn decision_models_are_kept_by_endpoint_and_cleared_when_none_remain() {
        let agent = temporary_directory();
        store_decision_models(&agent, "llama", &["kev-4b".to_owned()]).unwrap();
        assert_eq!(
            decision_models(&read_record(&agent).unwrap(), "llama"),
            ["kev-4b"]
        );
        store_decision_models(&agent, "llama", &[]).unwrap();
        assert!(decision_models(&read_record(&agent).unwrap(), "llama").is_empty());
        fs::remove_dir_all(agent).unwrap();
    }
}
