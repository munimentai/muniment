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
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use serde_json::{json, Map, Value};

use crate::endpoint_models::{
    discover_endpoint_models, discover_endpoint_models_checked, DiscoveryFailure, EndpointModels,
    CONNECT_TIMEOUT,
};
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
/// The record key that keeps the last name of each disconnected endpoint by
/// provider, so its usage history keeps a name a person reads.
pub const FORMER_NAMES: &str = "formerNames";
/// The record key that lists the endpoints whose models the person named,
/// so a refresh keeps exactly those models.
const NAMED_MODELS: &str = "namedModels";
/// The record key that lists the endpoints a chat reaches without tools,
/// for a server that refuses tool calls.
const TOOLS_OFF: &str = "toolsOff";
/// The sentence a save answers when the server refuses the key it sent.
pub const KEY_REFUSED: &str = "The server refused the API key. Check the key and try again.";
/// The sentence a save answers when the server asks for a key and none was sent.
pub const KEY_NEEDED: &str = "The server asks for an API key. Enter its key and try again.";
/// Pi breaks a lock on `auth.json` after 30 seconds without a refresh, the
/// longest threshold it uses, so a lock any live holder refreshes stays.
pub const LOCK_STALE: Duration = Duration::from_secs(30);
/// The wait outlasts the stale threshold, so a lock its holder left as it
/// died is broken before the wait ends.
const LOCK_TIMEOUT: Duration = Duration::from_secs(40);
// proper-lockfile refreshes its lock every 5 seconds by default.
const LOCK_UPDATE: Duration = Duration::from_secs(5);
const LOCK_RETRY: Duration = Duration::from_millis(20);
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(4);
/// How many endpoints a refresh reads at once.
const DISCOVERY_WORKERS: usize = 4;

/// Pi's model file in an agent directory.
pub fn models_file(agent: &Path) -> PathBuf {
    agent.join("models.json")
}

/// The lock Pi and Muniment share on one of Pi's files: proper-lockfile's
/// directory beside it. The holder refreshes the directory's modified time
/// while it holds the lock, so a lock whose time stops is abandoned: its
/// holder died without a release, and the next writer takes it.
pub struct FileLock {
    path: PathBuf,
    modified: Arc<Mutex<SystemTime>>,
    stop: mpsc::Sender<()>,
    heartbeat: Option<std::thread::JoinHandle<()>>,
}

impl FileLock {
    /// The lock directory.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Fails when another writer broke this lock and took the file.
    pub fn check(&self) -> io::Result<()> {
        let modified = self.modified.lock().unwrap();
        if fs::metadata(&self.path)?.modified()? != *modified {
            return Err(io::Error::other("The file lock changed owners."));
        }
        Ok(())
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(heartbeat) = self.heartbeat.take() {
            let _ = heartbeat.join();
        }
        if self.check().is_ok() {
            let _ = fs::remove_dir(&self.path);
        }
    }
}

pub fn lock_path(file: &Path) -> PathBuf {
    let mut name = OsString::from(file.as_os_str());
    name.push(".lock");
    PathBuf::from(name)
}

/// A handle that sets a directory's modified time.
pub fn directory_handle(path: &Path) -> io::Result<fs::File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Windows needs read/write attribute access and backup semantics to open a directory.
        options.access_mode(0x180).custom_flags(0x02000000);
    }
    options.open(path)
}

/// Takes the lock on `file`, waiting while a live writer holds it.
pub fn lock(file: &Path) -> io::Result<FileLock> {
    lock_with(file, LOCK_TIMEOUT, LOCK_STALE)
}

/// Takes the lock on `file`, waiting up to `timeout` while a live writer
/// holds it, and breaks a lock whose time is older than `stale`.
pub fn lock_with(file: &Path, timeout: Duration, stale: Duration) -> io::Result<FileLock> {
    let path = lock_path(file);
    let deadline = Instant::now() + timeout;
    loop {
        match fs::create_dir(&path) {
            Ok(()) => break,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                match fs::metadata(&path).and_then(|metadata| metadata.modified()) {
                    Ok(modified) if modified.elapsed().is_ok_and(|age| age > stale) => {
                        // Recheck the time before removing an abandoned directory.
                        if fs::metadata(&path)?.modified()? == modified {
                            match fs::remove_dir(&path) {
                                Ok(()) => continue,
                                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                                Err(error) => return Err(error),
                            }
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(error),
                    _ => {}
                }
                if Instant::now() >= deadline {
                    return Err(error);
                }
                std::thread::sleep(LOCK_RETRY);
            }
            Err(error) => return Err(error),
        }
    }
    let setup = (|| {
        let directory = directory_handle(&path)?;
        let modified = Arc::new(Mutex::new(directory.metadata()?.modified()?));
        let shared = modified.clone();
        let heartbeat_path = path.clone();
        let (stop, wait) = mpsc::channel();
        let update = (stale / 2).min(LOCK_UPDATE);
        let heartbeat = std::thread::Builder::new().spawn(move || {
            while wait.recv_timeout(update) == Err(mpsc::RecvTimeoutError::Timeout) {
                let mut modified = shared.lock().unwrap();
                if fs::metadata(&heartbeat_path)
                    .and_then(|m| m.modified())
                    .ok()
                    != Some(*modified)
                {
                    break;
                }
                if directory.set_modified(SystemTime::now()).is_err() {
                    break;
                }
                match directory.metadata().and_then(|m| m.modified()) {
                    Ok(time) => *modified = time,
                    Err(_) => break,
                }
            }
        })?;
        Ok(FileLock {
            path: path.clone(),
            modified,
            stop,
            heartbeat: Some(heartbeat),
        })
    })();
    if setup.is_err() {
        let _ = fs::remove_dir(&path);
    }
    setup
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

/// Drops what the record keeps for the endpoint `provider`: its named list
/// and its decision models, and moves its name to the former names. Answers
/// whether the record changed.
fn drop_endpoint(record: &mut Map<String, Value>, provider: &str) -> bool {
    let mut changed = false;
    for key in [NAMES, DECISION_MODELS] {
        if let Some(entries) = record.get_mut(key).and_then(Value::as_object_mut) {
            let dropped = entries.remove(provider);
            if entries.is_empty() {
                record.remove(key);
            }
            let Some(dropped) = dropped else {
                continue;
            };
            changed = true;
            if key == NAMES {
                if let Some(former) = record
                    .entry(FORMER_NAMES)
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
                {
                    former.insert(provider.to_owned(), dropped);
                }
            }
        }
    }
    for key in [NAMED_MODELS, TOOLS_OFF] {
        let listed = record_list(record, key);
        if listed.iter().any(|id| id == provider) {
            let rest = listed.into_iter().filter(|id| id != provider).collect();
            set_record_list(record, key, rest);
            changed = true;
        }
    }
    changed
}

/// Whether a chat on the endpoint `provider` runs without tools. Pi's
/// `models.json` has no setting that keeps tools out of a request, so the
/// record keeps it and a launch on the endpoint starts Pi with no tools.
pub fn tools_off(record: &Map<String, Value>, provider: &str) -> bool {
    record_list(record, TOOLS_OFF)
        .iter()
        .any(|id| id == provider)
}

/// Turns tools off or on for the endpoint `provider`.
pub fn set_tools_off(agent: &Path, provider: &str, off: bool) -> Result<(), String> {
    let mut record = read_record(agent)?;
    if tools_off(&record, provider) == off {
        return Ok(());
    }
    let mut listed: Vec<String> = record_list(&record, TOOLS_OFF)
        .into_iter()
        .filter(|id| id != provider)
        .collect();
    if off {
        listed.push(provider.to_owned());
    }
    set_record_list(&mut record, TOOLS_OFF, listed);
    write_record(agent, &record)
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

/// The API routes a pasted server URL may end in. The longest comes first,
/// so `/chat/completions` goes whole.
const API_ROUTES: [&str; 4] = ["/chat/completions", "/completions", "/models", "/responses"];

/// The base URL of a server URL pasted with an API route on it, such as
/// `http://host:8000/v1/chat/completions`, which Pi and discovery both
/// extend with a route of their own. Any other URL stays as it is.
pub fn endpoint_base(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    for route in API_ROUTES {
        let Some(at) = trimmed.len().checked_sub(route.len()) else {
            continue;
        };
        if trimmed.is_char_boundary(at) && trimmed[at..].eq_ignore_ascii_case(route) {
            let base = trimmed[..at].trim_end_matches('/');
            if base
                .split_once("://")
                .is_some_and(|(_, host)| !host.is_empty())
            {
                return base.to_owned();
            }
        }
    }
    base_url.to_owned()
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

/// What a save says when the model list refused the key `key`, or asked for
/// one when `key` is blank.
fn key_refusal(key: &str) -> &'static str {
    if key.is_empty() {
        KEY_NEEDED
    } else {
        KEY_REFUSED
    }
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
    let base_url = &endpoint_base(base_url);
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

    // The server names its own models; the pinned id stands in when it does
    // not answer. Discovery runs before the lock, so no save waits on it.
    let discovered = discover_endpoint_models_checked(
        base_url,
        resolve_key(key).as_deref().or(Some(key)),
        DISCOVERY_TIMEOUT,
    );
    let (model_ids, decisions, windows) = match discovered {
        Ok(found) => (found.chat, found.decisions, found.windows),
        Err(DiscoveryFailure::KeyRefused) => return Err(key_refusal(key).into()),
        Err(DiscoveryFailure::NoList | DiscoveryFailure::Unreachable) => {
            (vec![OLLAMA_MODEL.to_owned()], Vec::new(), BTreeMap::new())
        }
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
    fs::create_dir_all(agent).map_err(|_| SAVE_ERROR.to_string())?;
    let _models_lock = lock(&models_file).map_err(|_| SAVE_ERROR.to_string())?;
    let mut models = read_for_update(&models_file)?;
    let providers = models
        .entry("providers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| SAVE_ERROR.to_string())?;
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
    let base_url = &endpoint_base(base_url);
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
        let found = match discover_endpoint_models_checked(
            base_url,
            resolve_key(key).as_deref().or(Some(key)),
            DISCOVERY_TIMEOUT,
        ) {
            Ok(found) => found,
            Err(DiscoveryFailure::KeyRefused) => return Err(key_refusal(key).into()),
            Err(DiscoveryFailure::NoList | DiscoveryFailure::Unreachable) => Default::default(),
        };
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
    // A live name replaces the former one.
    if let Some(former) = record.get_mut(FORMER_NAMES).and_then(Value::as_object_mut) {
        former.remove(&provider);
        if former.is_empty() {
            record.remove(FORMER_NAMES);
        }
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
    refresh_endpoints_checked(agent, key_for).map(|refresh| refresh.changed)
}

/// What one endpoint refresh found.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EndpointRefresh {
    /// Whether any endpoint's list changed.
    pub changed: bool,
    /// The provider ids whose server never answered within the discovery
    /// timeout. Each keeps its saved list.
    pub unreachable: Vec<String>,
}

/// [`refresh_endpoints_with`], with the endpoints whose server never answered.
pub fn refresh_endpoints_checked(
    agent: &Path,
    key_for: &(dyn Fn(&str) -> Option<String> + Sync),
) -> Result<EndpointRefresh, String> {
    refresh_endpoints_within(agent, key_for, DISCOVERY_TIMEOUT)
}

/// Reads every endpoint at once, at most [`DISCOVERY_WORKERS`] at a time, so
/// one server that never answers delays no other. Each read ends within
/// `timeout`. A read still running at the refresh deadline, such as one held
/// in a name lookup, counts as unreachable, and its thread ends on its own.
fn discover_all(
    jobs: Vec<(String, Option<String>)>,
    timeout: Duration,
) -> Vec<Result<EndpointModels, DiscoveryFailure>> {
    let count = jobs.len();
    let mut results = vec![Err(DiscoveryFailure::Unreachable); count];
    if count == 0 {
        return results;
    }
    let workers = count.min(DISCOVERY_WORKERS);
    let queue = Arc::new(Mutex::new(
        jobs.into_iter()
            .enumerate()
            .collect::<std::collections::VecDeque<_>>(),
    ));
    let (send, receive) = mpsc::channel();
    for _ in 0..workers {
        let queue = Arc::clone(&queue);
        let send = send.clone();
        std::thread::spawn(move || loop {
            let job = queue
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pop_front();
            let Some((index, (base_url, key))) = job else {
                break;
            };
            let found = discover_endpoint_models_checked(&base_url, key.as_deref(), timeout);
            if send.send((index, found)).is_err() {
                break;
            }
        });
    }
    drop(send);
    // A read ends within its timeout plus one connection wait, and each
    // worker reads its share of the endpoints in turn.
    let rounds = u32::try_from(count.div_ceil(workers)).unwrap_or(u32::MAX);
    let deadline = Instant::now() + (timeout + timeout.min(CONNECT_TIMEOUT)).saturating_mul(rounds);
    for _ in 0..count {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match receive.recv_timeout(remaining) {
            Ok((index, found)) => results[index] = found,
            Err(_) => break,
        }
    }
    results
}

fn refresh_endpoints_within(
    agent: &Path,
    key_for: &(dyn Fn(&str) -> Option<String> + Sync),
    timeout: Duration,
) -> Result<EndpointRefresh, String> {
    let models_file = models_file(agent);
    if !models_file.is_file() {
        return Ok(EndpointRefresh::default());
    }
    // Discovery reads each server over the network, and a key command may
    // run, so it reads the file without the lock. A save never waits on it.
    let seen = read_for_update(&models_file)?;
    let mut endpoints = Vec::new();
    let mut jobs = Vec::new();
    for (id, entry) in seen
        .get("providers")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(id, _)| {
            id.as_str() != ROUTER_PROVIDER && !key_providers().any(|(known, _)| known == *id)
        })
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
        endpoints.push((id.clone(), entry.clone()));
        jobs.push((base_url.to_owned(), key));
    }
    let mut discovered = Vec::new();
    let mut unreachable = Vec::new();
    for ((id, entry), found) in endpoints.into_iter().zip(discover_all(jobs, timeout)) {
        match found {
            // A server that answers with no chat model is still loading, as
            // a local server does for a while after it starts, so the saved
            // list stays until it serves one.
            Ok(found) if !found.chat.is_empty() => discovered.push((id, entry, found)),
            Err(DiscoveryFailure::Unreachable) => unreachable.push(id),
            _ => {}
        }
    }
    if discovered.is_empty() {
        return Ok(EndpointRefresh {
            changed: false,
            unreachable,
        });
    }
    // The lock covers only the merge. An entry a save changed while
    // discovery ran keeps what the save wrote.
    let _lock = lock(&models_file).map_err(|_| SAVE_ERROR.to_string())?;
    let mut root = read_for_update(&models_file)?;
    let record = read_record(agent)?;
    let mut changed = false;
    let mut decisions = Vec::new();
    if let Some(providers) = root.get_mut("providers").and_then(Value::as_object_mut) {
        for (id, before, found) in discovered {
            let Some(entry) = providers.get_mut(&id).filter(|entry| **entry == before) else {
                continue;
            };
            let Some(entry) = entry.as_object_mut() else {
                continue;
            };
            let named = named_models(&record, &id);
            changed |= apply_discovered(entry, named, &found);
            if !named {
                decisions.push((id, found.decisions));
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
    Ok(EndpointRefresh {
        changed,
        unreachable,
    })
}

/// Rewrites one endpoint entry's model list from what its server serves, as
/// [`refresh_endpoints`] describes, and answers whether the list changed.
fn apply_discovered(entry: &mut Map<String, Value>, named: bool, found: &EndpointModels) -> bool {
    let current = entry
        .get("models")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let next: Vec<Value> = if named {
        // The person named these models, so the list stays as it is and
        // the server only adds the limits a model lacks.
        current
            .iter()
            .map(|old| {
                let mut next = old.clone();
                let window = old
                    .get("id")
                    .and_then(Value::as_str)
                    .and_then(|model| found.windows.get(model).map(|w| (model, w)));
                if let (Some((model, window)), Some(fields)) = (window, next.as_object_mut()) {
                    let reported = model_entry(model, Some(window));
                    for field in ["contextWindow", "maxTokens"] {
                        if let Some(value) = reported.get(field) {
                            fields.entry(field).or_insert_with(|| value.clone());
                        }
                    }
                }
                next
            })
            .collect()
    } else {
        found
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
            .collect()
    };
    if current == next {
        return false;
    }
    entry.insert("models".into(), next.into());
    true
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
    use crate::test_server::{serve_routes, serve_status};

    fn temporary_directory() -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("muniment-pi-models-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn takes_at_once_a_lock_a_dead_writer_left() {
        let directory = temporary_directory();
        let file = models_file(&directory);
        let held = lock_path(&file);
        // A writer killed before its release leaves the directory, and its
        // time stops.
        fs::create_dir(&held).unwrap();
        directory_handle(&held)
            .unwrap()
            .set_modified(SystemTime::now() - LOCK_STALE - Duration::from_secs(1))
            .unwrap();
        let started = Instant::now();
        let lock = lock(&file).unwrap();
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(lock.check().is_ok());
        drop(lock);
        assert!(!held.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn waits_on_a_live_writer_and_keeps_its_lock() {
        let directory = temporary_directory();
        let file = models_file(&directory);
        let holder = lock(&file).unwrap();
        let modified = fs::metadata(holder.path()).unwrap().modified().unwrap();
        assert!(matches!(
            lock_with(&file, Duration::from_millis(60), LOCK_STALE),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists
        ));
        assert_eq!(
            fs::metadata(holder.path()).unwrap().modified().unwrap(),
            modified
        );
        assert!(holder.check().is_ok());
        drop(holder);
        assert!(!lock_path(&file).exists());
        lock(&file).unwrap();
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_live_writer_refreshes_its_lock_past_the_stale_threshold() {
        let directory = temporary_directory();
        let file = models_file(&directory);
        let stale = Duration::from_millis(300);
        let holder = lock_with(&file, Duration::ZERO, stale).unwrap();
        let initial = fs::metadata(holder.path()).unwrap().modified().unwrap();
        std::thread::sleep(stale * 2);
        assert!(fs::metadata(holder.path()).unwrap().modified().unwrap() > initial);
        assert!(matches!(
            lock_with(&file, Duration::ZERO, stale),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists
        ));
        drop(holder);
        assert!(!lock_path(&file).exists());
        fs::remove_dir_all(directory).unwrap();
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
    fn a_pasted_api_route_saves_as_the_servers_base_url() {
        for (pasted, base) in [
            (
                "http://host:8000/v1/chat/completions",
                "http://host:8000/v1",
            ),
            (
                "http://host:8000/v1/chat/completions/",
                "http://host:8000/v1",
            ),
            ("https://host/v1/completions", "https://host/v1"),
            ("http://host:8000/v1/models", "http://host:8000/v1"),
            ("http://host:8000/V1/Responses", "http://host:8000/V1"),
            ("http://host:8000/models", "http://host:8000"),
            ("http://host:8000/v1", "http://host:8000/v1"),
            ("http://host:8000/v1/", "http://host:8000/v1/"),
            (
                "http://localhost:4000/v1/my-models",
                "http://localhost:4000/v1/my-models",
            ),
            ("http:///models", "http:///models"),
        ] {
            assert_eq!(endpoint_base(pasted), base, "{pasted}");
        }
        let agent = temporary_directory();
        let url = serve_routes(vec![(
            "GET /v1/models",
            json!({"data": [{"id": "qwen2.5-1.5b", "max_model_len": 32768}]}),
        )]);
        let pasted = format!("{url}/chat/completions");
        let id = store_endpoint(&agent, "custom", "DO Test", &pasted, "", &[]).unwrap();
        let saved = &models(&agent)["providers"][&id];
        assert_eq!(saved["baseUrl"], url.as_str());
        assert_eq!(
            saved["models"],
            json!([{"id": "qwen2.5-1.5b", "contextWindow": 32768}])
        );
        fs::remove_dir_all(agent).unwrap();
    }

    #[test]
    fn a_refused_model_list_says_the_server_refused_the_key() {
        let agent = temporary_directory();
        let url = serve_status("401 Unauthorized");
        assert_eq!(
            store_endpoint(&agent, "custom", "DO Test", &url, "sk-wrong", &[]),
            Err(KEY_REFUSED.to_owned())
        );
        assert_eq!(
            store_endpoint(&agent, "custom", "DO Test", &url, "", &[]),
            Err(KEY_NEEDED.to_owned())
        );
        let forbidden = serve_status("403 Forbidden");
        assert_eq!(
            store_ollama(&agent, &forbidden, "sk-wrong", |_| Ok(())),
            Err(KEY_REFUSED.to_owned())
        );
        // A server with no list keeps its own sentence.
        let missing = serve_routes(vec![]);
        assert_eq!(
            store_endpoint(&agent, "custom", "DO Test", &missing, "", &[]),
            Err("The server named no models. Start it, or list its models here.".to_owned())
        );
        assert!(!models_file(&agent).exists());
        fs::remove_dir_all(agent).unwrap();
    }

    #[test]
    fn tools_off_is_kept_by_endpoint_and_dropped_with_it() {
        let agent = temporary_directory();
        set_tools_off(&agent, "custom-do-test", true).unwrap();
        set_tools_off(&agent, "custom-other", true).unwrap();
        let record = read_record(&agent).unwrap();
        assert!(tools_off(&record, "custom-do-test") && tools_off(&record, "custom-other"));
        set_tools_off(&agent, "custom-other", false).unwrap();
        let mut record = read_record(&agent).unwrap();
        assert!(!tools_off(&record, "custom-other"));
        assert!(drop_endpoint(&mut record, "custom-do-test"));
        assert!(!tools_off(&record, "custom-do-test"));
        assert!(record.get(TOOLS_OFF).is_none());
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
    fn a_dead_endpoint_neither_delays_a_live_one_nor_loses_its_list() {
        let agent = temporary_directory();
        let live = serve_routes(vec![(
            "GET /v1/models",
            json!({"data": [{"id": "served"}]}),
        )]);
        // 192.0.2.1 is TEST-NET-1, which no host answers.
        let mut providers = serde_json::Map::new();
        for index in 0..5 {
            providers.insert(
                format!("custom-dead{index}"),
                json!({"baseUrl": "http://192.0.2.1:8000/v1", "apiKey": "local", "models": [{"id": "kept"}]}),
            );
        }
        providers.insert(
            "custom-live".into(),
            json!({"baseUrl": live, "apiKey": "local", "models": [{"id": "old"}]}),
        );
        fs::write(
            models_file(&agent),
            json!({"providers": providers}).to_string(),
        )
        .unwrap();
        let started = Instant::now();
        let refresh =
            refresh_endpoints_within(&agent, &|_| None, Duration::from_millis(300)).unwrap();
        // Six endpoints on four workers take two rounds of at most 600 ms.
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "refresh took {:?}",
            started.elapsed()
        );
        assert!(refresh.changed);
        assert_eq!(
            refresh.unreachable,
            (0..5)
                .map(|index| format!("custom-dead{index}"))
                .collect::<Vec<_>>()
        );
        let root = models(&agent);
        assert_eq!(
            root["providers"]["custom-live"]["models"],
            json!([{"id": "served"}])
        );
        assert_eq!(
            root["providers"]["custom-dead0"]["models"],
            json!([{"id": "kept"}])
        );
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
                "formerNames": {"custom-live27": "Live27"},
                "namedModels": ["custom-live30"],
                "hidden": ["custom-live30/m"]
            })
        );
        assert_eq!(sweep_endpoints(&agent).unwrap(), 0);
        // A disconnect keeps the endpoint's last name for its usage history.
        forget_endpoint(&agent, "custom-live30").unwrap();
        assert_eq!(
            Value::Object(read_record(&agent).unwrap()),
            json!({
                "formerNames": {"custom-live27": "Live27", "custom-live30": "Live30"},
                "hidden": ["custom-live30/m"]
            })
        );
        // The same endpoint connected again takes its live name back.
        let url = serve_routes(vec![("GET /v1/models", json!({"data": [{"id": "m"}]}))]);
        let id = store_endpoint(&agent, "custom", "Live30", &url, "", &["m".into()]).unwrap();
        assert_eq!(id, "custom-live30");
        let record = read_record(&agent).unwrap();
        assert_eq!(record["names"]["custom-live30"], "Live30");
        assert_eq!(record["formerNames"], json!({"custom-live27": "Live27"}));
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
    fn a_refresh_leaves_the_lock_free_while_a_server_is_slow() {
        use std::io::{BufRead, BufReader, Write};
        let agent = temporary_directory();
        // The slow server holds its first answer until the test releases it.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let slow = format!("http://{}/v1", listener.local_addr().unwrap());
        let (arrived, asked) = mpsc::channel();
        let (release, released) = mpsc::channel::<()>();
        std::thread::spawn(move || {
            for (index, stream) in listener.incoming().enumerate() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let (mut route, mut line) = (String::new(), String::new());
                reader.read_line(&mut route).unwrap();
                while reader.read_line(&mut line).unwrap_or(0) > 0 && line != "\r\n" {
                    line.clear();
                }
                if index == 0 {
                    let _ = arrived.send(());
                    let _ = released.recv_timeout(Duration::from_secs(3));
                }
                let body = if route.starts_with("GET /v1/models") {
                    json!({"data": [{"id": "fresh"}]}).to_string()
                } else {
                    String::new()
                };
                let status = if body.is_empty() {
                    "404 Not Found"
                } else {
                    "200 OK"
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        let fast = serve_routes(vec![("GET /v1/models", json!({"data": [{"id": "quick"}]}))]);
        fs::write(
            models_file(&agent),
            json!({"providers": {
                "custom-slow": {"baseUrl": slow, "apiKey": "local", "models": [{"id": "old"}]},
                "custom-edited": {"baseUrl": slow, "apiKey": "local", "models": [{"id": "old"}]}
            }})
            .to_string(),
        )
        .unwrap();
        let refresh = std::thread::spawn({
            let agent = agent.clone();
            move || refresh_endpoints(&agent)
        });
        asked.recv_timeout(Duration::from_secs(3)).unwrap();

        // Discovery waits on the server, and a save takes the lock at once.
        let started = Instant::now();
        store_endpoint(&agent, "lmstudio", "", &fast, "", &[]).unwrap();
        {
            let _save = lock_with(&models_file(&agent), Duration::ZERO, LOCK_STALE).unwrap();
            let mut root = read_json(&models_file(&agent)).unwrap();
            root["providers"]["custom-edited"]["models"] = json!([{"id": "saved"}]);
            write_json(&models_file(&agent), &root).unwrap();
        }
        assert!(started.elapsed() < Duration::from_secs(1));
        release.send(()).unwrap();

        assert!(refresh.join().unwrap().unwrap());
        let root = models(&agent);
        assert_eq!(
            root["providers"]["custom-slow"]["models"],
            json!([{"id": "fresh"}])
        );
        // The save that ran during discovery wins over what discovery found.
        assert_eq!(
            root["providers"]["custom-edited"]["models"],
            json!([{"id": "saved"}])
        );
        assert_eq!(
            root["providers"]["lmstudio"]["models"],
            json!([{"id": "quick"}])
        );
        assert!(!lock_path(&models_file(&agent)).exists());
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
