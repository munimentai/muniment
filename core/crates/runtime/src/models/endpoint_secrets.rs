//! Endpoint keys outside Pi's `models.json`. A key an endpoint provider holds
//! lives in the host's credential store, one entry per provider, and its
//! `apiKey` in `models.json` is a command Pi runs for the key:
//! `!"<runtime executable>" secret get <provider>`. The runtime executable
//! answers that command from the same store, so the key never sits in a file
//! Pi or a tool reads. Stand-in keys, environment references and commands the
//! user wrote stay as they are.
//!
//! Pi kills the command after 10 seconds, sooner than a person answers a
//! keychain prompt. So the runtime, which lives on, asks the store through
//! its [`KeyBroker`], and the command asks the runtime over a socket under
//! the state root. The runtime asks the keychain once per entry at a time,
//! only when a request needs the key, and records a late answer for the next
//! request.
//!
//! A key the store refuses lives in the runtime's memory alone, and the
//! store may still hold the key it replaced. A mark under the state root
//! names each such provider, so after a restart the runtime never answers
//! the old key and the endpoint card asks for the key again.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use muniment_core::model_router::config::ROUTER_PROVIDER;
use muniment_core::models::pi_models;
use muniment_core::serde_json::Value;

use crate::host::CredentialStore;

/// The credential store service that holds endpoint keys.
pub const SERVICE: &str = "ai.muniment.endpoints";

/// The arguments the runtime executable answers a key on.
const COMMAND: [&str; 2] = ["secret", "get"];

/// The values Pi sends to a server with no key. They are not keys.
const STAND_INS: [&str; 2] = ["ollama", "local"];

/// The `apiKey` value that has Pi read `provider`'s key through `executable`.
pub fn reference(executable: &Path, provider: &str) -> Option<String> {
    let path = executable.to_str()?;
    (safe_provider(provider) && !path.contains('"') && !path.is_empty())
        .then(|| format!("!\"{path}\" {} {} {provider}", COMMAND[0], COMMAND[1]))
}

/// The provider a reference this module wrote names, with the executable it
/// runs, or none for any other value.
fn parse_reference(value: &str) -> Option<(&str, &str)> {
    let rest = value.strip_prefix("!\"")?;
    let (path, rest) = rest.split_once('"')?;
    let provider = rest.strip_prefix(&format!(" {} {} ", COMMAND[0], COMMAND[1]))?;
    safe_provider(provider).then_some((path, provider))
}

/// The provider whose key a reference this module wrote reads, or none for
/// any other `apiKey` value.
pub fn referenced_provider(value: &str) -> Option<&str> {
    parse_reference(value).map(|(_, provider)| provider)
}

/// A provider id the shell passes through untouched.
fn safe_provider(provider: &str) -> bool {
    !provider.is_empty()
        && provider.len() <= 128
        && provider
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// Moves every plain endpoint key in `models.json` into `store` and writes a
/// reference through `executable` in its place, and points each reference
/// this module wrote at `executable`, so a moved runtime keeps its keys.
/// Answers how many entries changed. No plain key stays in the file: a key
/// the store refuses leaves it too, and only [`seal_with`] hands it to the
/// caller.
pub fn seal(agent: &Path, store: &dyn CredentialStore, executable: &Path) -> Result<usize, String> {
    seal_with(agent, store, executable, &mut |_, _| {})
}

/// What became of one plain key [`seal_with`] moved out of `models.json`.
pub enum Sealed<'a> {
    /// The store holds the key.
    Stored,
    /// The store refused the key, and the caller holds it.
    Refused(&'a str),
}

/// [`seal`], which also names each provider whose plain key it moved and
/// what became of the key. A key the store refuses goes to `moved` as
/// [`Sealed::Refused`] and leaves the file all the same, so a plain key never
/// stays on disk. The file then holds a reference, so a later pass finds no
/// key to move and the store hears one write per key the person saves.
pub fn seal_with(
    agent: &Path,
    store: &dyn CredentialStore,
    executable: &Path,
    moved: &mut dyn FnMut(&str, Sealed<'_>),
) -> Result<usize, String> {
    let models_file = pi_models::models_file(agent);
    if !models_file.is_file() {
        return Ok(0);
    }
    // The store may be a keychain that prompts, so the moves read the file
    // and write the store without the file's lock. A save never waits on it.
    let seen = pi_models::read_json(&models_file).map_err(|_| pi_models::SAVE_ERROR.to_string())?;
    let mut planned: Vec<(String, String, String, Option<bool>)> = Vec::new();
    for (provider, entry) in seen
        .get("providers")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(provider, _)| provider.as_str() != ROUTER_PROVIDER)
    {
        let Some(key) = entry.get("apiKey").and_then(Value::as_str) else {
            continue;
        };
        let Some(next) = reference(executable, provider) else {
            continue;
        };
        if key == next {
            continue;
        }
        let stored = if let Some((_, named)) = parse_reference(key) {
            if named != provider {
                continue;
            }
            None
        } else {
            if key.is_empty()
                || STAND_INS.contains(&key)
                || key.starts_with('!')
                || key.starts_with('$')
            {
                continue;
            }
            match store.write(SERVICE, provider, key) {
                Ok(()) => Some(true),
                Err(error) => {
                    muniment_core::runtime_eprintln!(
                        "muniment-runtime: endpoint_key provider={provider} outcome=memory error={error}"
                    );
                    Some(false)
                }
            }
        };
        planned.push((provider.clone(), key.to_owned(), next, stored));
    }
    if planned.is_empty() {
        return Ok(0);
    }
    // The lock covers only the rewrite. A key a save changed meanwhile stays
    // for the pass after that save.
    let _lock = pi_models::lock(&models_file).map_err(|_| pi_models::SAVE_ERROR.to_string())?;
    let mut root =
        pi_models::read_json(&models_file).map_err(|_| pi_models::SAVE_ERROR.to_string())?;
    let mut changed = 0;
    if let Some(providers) = root.get_mut("providers").and_then(Value::as_object_mut) {
        for (provider, key, next, stored) in &planned {
            let Some(entry) = providers.get_mut(provider) else {
                continue;
            };
            if entry.get("apiKey").and_then(Value::as_str) != Some(key.as_str()) {
                continue;
            }
            match stored {
                Some(true) => moved(provider, Sealed::Stored),
                Some(false) => moved(provider, Sealed::Refused(key)),
                None => {}
            }
            entry["apiKey"] = next.as_str().into();
            changed += 1;
        }
    }
    if changed > 0 {
        pi_models::write_json(&models_file, &root)
            .map_err(|_| pi_models::SAVE_ERROR.to_string())?;
    }
    Ok(changed)
}

/// The sentence an endpoint card shows while the runtime holds its key in
/// memory alone, because the store refused it.
pub const MEMORY_KEY_NOTICE: &str =
    "The key is kept until Muniment quits; the keychain refused it.";

/// The sentence an endpoint card shows after a restart when the store
/// refused the key the person saved last, so the store holds an older key or
/// none: the person enters the key again, and how to remove the old one.
pub fn stale_key_notice(provider: &str) -> String {
    if cfg!(target_os = "macos") {
        format!(
            "The keychain holds an older key for this endpoint. Enter the key again. To remove the old key, open Keychain Access, search for {SERVICE}, and delete the item for {provider}."
        )
    } else {
        format!(
            "The system credential store holds an older key for this endpoint. Enter the key again. To remove the old key, delete the {SERVICE} entry for {provider} there."
        )
    }
}

/// The sentence a disconnect shows when the store kept `provider`'s key:
/// where the key stays and how the person removes it.
pub fn kept_key_notice(provider: &str) -> String {
    if cfg!(target_os = "macos") {
        format!(
            "Its key stays in the keychain. To remove it, open Keychain Access, search for {SERVICE}, and delete the item for {provider}."
        )
    } else {
        format!(
            "Its key stays in the system credential store. To remove it, delete the {SERVICE} entry for {provider} there."
        )
    }
}

/// Removes `provider`'s key from `store`.
pub fn forget(store: &dyn CredentialStore, provider: &str) -> Result<(), String> {
    if !safe_provider(provider) {
        return Ok(());
    }
    store.remove(SERVICE, provider)
}

/// The exit status of `secret get` when the store refused the key, as after
/// a declined keychain prompt. The command prints nothing then.
pub const REFUSED_STATUS: i32 = 2;

/// The exit status of `secret get` while the keychain waits for the
/// person's answer. The command prints nothing then, and the runtime keeps
/// the answer for the next request.
pub const PENDING_STATUS: i32 = 3;

/// The exit status of `secret get` for a provider whose stored key is
/// stale: the store refused the key that replaced it, so the person enters
/// the key again. The command prints nothing then, and never reads the store.
pub const STALE_STATUS: i32 = 4;

/// The folder under the state root that holds the socket the runtime
/// answers `secret get` on. Only the owner may open it.
pub const SOCKET_DIRECTORY_NAME: &str = muniment_core::state_root::ENDPOINT_KEYS_DIRECTORY_NAME;

/// The socket the runtime answers `secret get` on, under `state`.
pub fn socket_path(state: &Path) -> PathBuf {
    state.join(SOCKET_DIRECTORY_NAME).join("socket")
}

/// The folder of stale-key marks under `state`, beside the socket: one
/// empty mark per provider whose stored key is stale, because the store
/// refused the key that replaced it.
pub fn stale_marks_path(state: &Path) -> PathBuf {
    muniment_core::state_root::stale_endpoint_keys(state)
}

/// Whether `marks` holds the stale-key mark of `provider`.
fn marked_stale(marks: &Path, provider: &str) -> bool {
    safe_provider(provider) && marks.join(provider).is_file()
}

/// How long the runtime holds a `secret get` for the keychain's answer,
/// below the 10 seconds after which Pi kills the command.
pub const ANSWER_WAIT: Duration = Duration::from_secs(8);

/// How long `secret get` waits for the runtime's answer before it reports
/// the key as pending, still below Pi's 10 seconds.
#[cfg(unix)]
const COMMAND_WAIT: Duration = Duration::from_millis(9_500);

/// What the runtime answers for one endpoint key.
#[derive(Clone, PartialEq, Eq)]
pub enum KeyAnswer {
    /// The key.
    Key(String),
    /// The store holds no key for the provider.
    Missing,
    /// The store refused the key, as after a declined keychain prompt.
    Refused,
    /// The keychain still waits for the person's answer.
    Pending,
    /// The store's key is stale, so the person enters the key again.
    Stale,
}

impl std::fmt::Debug for KeyAnswer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Key(_) => "Key(<redacted>)",
            Self::Missing => "Missing",
            Self::Refused => "Refused",
            Self::Pending => "Pending",
            Self::Stale => "Stale",
        })
    }
}

impl KeyAnswer {
    /// The exit status of `secret get` for this answer.
    pub fn status(&self) -> i32 {
        match self {
            Self::Key(_) => 0,
            Self::Missing => 1,
            Self::Refused => REFUSED_STATUS,
            Self::Pending => PENDING_STATUS,
            Self::Stale => STALE_STATUS,
        }
    }

    fn from_lookup(lookup: Result<Option<String>, crate::host::CredentialRefusal>) -> Self {
        match lookup {
            Ok(Some(key)) => Self::Key(key),
            Ok(None) => Self::Missing,
            Err(_) => Self::Refused,
        }
    }
}

/// The runtime's one door to the endpoint keys in its store. It asks the
/// store for an entry once at a time, keeps a key it got for later requests,
/// and leaves refusals to the store's memory, so a refused key answers at
/// once until the person retries.
pub struct KeyBroker {
    store: Arc<dyn CredentialStore>,
    slots: Mutex<BTreeMap<String, Slot>>,
    /// The providers whose key the store refused, so the broker alone holds it.
    memory: Mutex<std::collections::BTreeSet<String>>,
    /// The folder of stale-key marks, which outlive the runtime.
    marks: Option<PathBuf>,
    wait: Duration,
}

enum Slot {
    /// A store read runs for the entry.
    Asking(Arc<Ask>),
    /// The key the store answered.
    Held(String),
}

/// One store read and the answer it reached.
#[derive(Default)]
struct Ask {
    answer: Mutex<Option<KeyAnswer>>,
    done: Condvar,
}

impl Ask {
    fn finish(&self, answer: KeyAnswer) {
        *self
            .answer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(answer);
        self.done.notify_all();
    }

    fn wait(&self, wait: Duration) -> KeyAnswer {
        let answer = self
            .answer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (answer, _) = self
            .done
            .wait_timeout_while(answer, wait, |answer| answer.is_none())
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        answer.clone().unwrap_or(KeyAnswer::Pending)
    }
}

impl KeyBroker {
    /// A broker over `store` that holds a request [`ANSWER_WAIT`] for the
    /// store's answer.
    pub fn new(store: Arc<dyn CredentialStore>) -> Arc<Self> {
        Self::waiting(store, ANSWER_WAIT)
    }

    /// A broker that holds a request `wait` for the store's answer.
    pub fn waiting(store: Arc<dyn CredentialStore>, wait: Duration) -> Arc<Self> {
        Self::build(store, wait, None)
    }

    /// A broker over `store` that keeps its stale-key marks in `marks`.
    pub fn marking(store: Arc<dyn CredentialStore>, marks: PathBuf) -> Arc<Self> {
        Self::build(store, ANSWER_WAIT, Some(marks))
    }

    fn build(store: Arc<dyn CredentialStore>, wait: Duration, marks: Option<PathBuf>) -> Arc<Self> {
        Arc::new(Self {
            store,
            slots: Mutex::new(BTreeMap::new()),
            memory: Mutex::new(Default::default()),
            marks,
            wait,
        })
    }

    fn slots(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Slot>> {
        self.slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The key for `provider`, for a request that uses it. A key the broker
    /// holds answers at once. Otherwise the broker asks the store, unless a
    /// read for the entry already runs, and waits for the answer up to its
    /// bound. A read that outlasts the bound answers [`KeyAnswer::Pending`]
    /// and keeps running, so its answer serves the next request. A stale
    /// stored key answers [`KeyAnswer::Stale`] without a store read.
    pub fn answer(self: &Arc<Self>, provider: &str) -> KeyAnswer {
        if !safe_provider(provider) {
            return KeyAnswer::Missing;
        }
        let ask = {
            let mut slots = self.slots();
            match slots.get(provider) {
                Some(Slot::Held(key)) => {
                    // A key that answers ends a failure the store holds for it.
                    self.store.answered(SERVICE, provider);
                    return KeyAnswer::Key(key.clone());
                }
                Some(Slot::Asking(ask)) => Arc::clone(ask),
                None if self.stale(provider) => {
                    muniment_core::runtime_eprintln!(
                        "muniment-runtime: endpoint_key provider={provider} read=stale"
                    );
                    return KeyAnswer::Stale;
                }
                None => {
                    let ask = Arc::new(Ask::default());
                    slots.insert(provider.to_owned(), Slot::Asking(Arc::clone(&ask)));
                    let broker = Arc::clone(self);
                    let name = provider.to_owned();
                    let read = Arc::clone(&ask);
                    let spawned = std::thread::Builder::new()
                        .name("endpoint-key".into())
                        .spawn(move || broker.read(&name, &read));
                    if spawned.is_err() {
                        slots.remove(provider);
                        return KeyAnswer::Pending;
                    }
                    ask
                }
            }
        };
        ask.wait(self.wait)
    }

    /// Reads the store for `provider` and records the answer.
    fn read(&self, provider: &str, ask: &Arc<Ask>) {
        let answer = KeyAnswer::from_lookup(self.store.lookup(SERVICE, provider));
        let outcome = match answer {
            KeyAnswer::Key(_) => "answered",
            KeyAnswer::Missing => "missing",
            KeyAnswer::Refused | KeyAnswer::Pending | KeyAnswer::Stale => "refused",
        };
        muniment_core::runtime_eprintln!(
            "muniment-runtime: endpoint_key provider={provider} read={outcome}"
        );
        {
            let mut slots = self.slots();
            let current =
                matches!(slots.get(provider), Some(Slot::Asking(held)) if Arc::ptr_eq(held, ask));
            if current {
                match &answer {
                    KeyAnswer::Key(key) => {
                        slots.insert(provider.to_owned(), Slot::Held(key.clone()));
                    }
                    _ => {
                        slots.remove(provider);
                    }
                }
            }
        }
        ask.finish(answer);
    }

    /// The key the broker holds for `provider`, without asking the store.
    pub fn held(&self, provider: &str) -> Option<String> {
        match self.slots().get(provider) {
            Some(Slot::Held(key)) => Some(key.clone()),
            _ => None,
        }
    }

    /// Whether a store read for `provider` runs now.
    pub fn asking(&self, provider: &str) -> bool {
        matches!(self.slots().get(provider), Some(Slot::Asking(_)))
    }

    /// The key for `provider` changed or left the store: the broker drops
    /// what it holds, and a read that runs now answers its waiters only.
    pub fn forget(&self, provider: &str) {
        self.slots().remove(provider);
        self.memory().remove(provider);
        if let (Some(marks), true) = (&self.marks, safe_provider(provider)) {
            match std::fs::remove_file(marks.join(provider)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => muniment_core::runtime_eprintln!(
                    "muniment-runtime: endpoint_key provider={provider} stale_mark=kept error={error}"
                ),
            }
        }
    }

    /// The store refused `key`, the new key for `provider`: the broker holds
    /// it for the requests of this runtime, which never ask the store for it.
    pub fn keep(&self, provider: &str, key: &str) {
        if !safe_provider(provider) {
            return;
        }
        self.slots()
            .insert(provider.to_owned(), Slot::Held(key.to_owned()));
        self.memory().insert(provider.to_owned());
        // The endpoint card names the key in memory, and nothing retries the
        // write, so the store's failure for it no longer holds.
        self.store.answered(SERVICE, provider);
        // The store may still hold the key this one replaced, so a later
        // runtime must not answer it.
        if let Some(marks) = &self.marks {
            let marked = std::fs::create_dir_all(marks)
                .and_then(|()| std::fs::write(marks.join(provider), b""));
            if let Err(error) = marked {
                muniment_core::runtime_eprintln!(
                    "muniment-runtime: endpoint_key provider={provider} stale_mark=failed error={error}"
                );
            }
        }
    }

    /// Whether the broker alone holds `provider`'s key, because the store
    /// refused it.
    pub fn kept_in_memory(&self, provider: &str) -> bool {
        self.memory().contains(provider)
    }

    /// Whether the store's key for `provider` is stale: the store refused
    /// the key that replaced it, and this runtime holds no key in its place.
    pub fn stale(&self, provider: &str) -> bool {
        !self.kept_in_memory(provider)
            && self
                .marks
                .as_deref()
                .is_some_and(|marks| marked_stale(marks, provider))
    }

    fn memory(&self) -> std::sync::MutexGuard<'_, std::collections::BTreeSet<String>> {
        self.memory
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Answers `secret get` on `socket` from `broker`, on a thread of its own.
/// The socket's folder admits only the owner.
#[cfg(unix)]
pub fn serve(broker: Arc<KeyBroker>, socket: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;
    let directory = socket
        .parent()
        .ok_or_else(|| std::io::Error::other("the socket has no folder"))?;
    std::fs::create_dir_all(directory)?;
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
    // A socket an earlier runtime left answers nothing.
    match std::fs::remove_file(socket) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(socket)?;
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600))?;
    std::thread::Builder::new()
        .name("endpoint-keys".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let broker = Arc::clone(&broker);
                let _ = std::thread::Builder::new()
                    .name("endpoint-key-request".into())
                    .spawn(move || answer_request(&broker, stream));
            }
        })?;
    Ok(())
}

/// Reads one provider id from `stream` and writes the broker's answer: its
/// exit status, then the key after a space when there is one.
#[cfg(unix)]
fn answer_request(broker: &Arc<KeyBroker>, stream: std::os::unix::net::UnixStream) {
    use std::io::{BufRead, Write};
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let mut line = String::new();
    let mut reader = std::io::BufReader::new(std::io::Read::take(&stream, 256));
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let answer = broker.answer(line.trim_end_matches(['\n', '\r']));
    let reply = match &answer {
        KeyAnswer::Key(key) => format!("0 {key}\n"),
        other => format!("{}\n", other.status()),
    };
    let mut stream = stream;
    let _ = stream.write_all(reply.as_bytes());
}

/// Asks the runtime on `socket` for `provider`'s key. Answers none when no
/// runtime listens there, so the caller reads the store itself.
#[cfg(unix)]
fn ask_runtime(socket: &Path, provider: &str) -> Option<KeyAnswer> {
    use std::io::{Read, Write};
    let mut stream = std::os::unix::net::UnixStream::connect(socket).ok()?;
    let _ = stream.set_read_timeout(Some(COMMAND_WAIT));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    if stream
        .write_all(format!("{provider}\n").as_bytes())
        .is_err()
    {
        return Some(KeyAnswer::Pending);
    }
    let mut reply = String::new();
    if (&mut stream)
        .take(64 * 1024)
        .read_to_string(&mut reply)
        .is_err()
    {
        return Some(KeyAnswer::Pending);
    }
    let reply = reply.trim_end_matches(['\n', '\r']);
    Some(match reply.split_once(' ') {
        Some(("0", key)) if !key.is_empty() => KeyAnswer::Key(key.to_owned()),
        _ => match reply.parse::<i32>() {
            Ok(1) => KeyAnswer::Missing,
            Ok(REFUSED_STATUS) => KeyAnswer::Refused,
            Ok(STALE_STATUS) => KeyAnswer::Stale,
            _ => KeyAnswer::Pending,
        },
    })
}

/// Answers `secret get <provider>`, the command a reference runs: the key on
/// standard output. Answers none for other arguments, so the caller handles
/// them, and the exit status otherwise: 0 with the key, 1 without one,
/// [`REFUSED_STATUS`] when the store refused it, [`PENDING_STATUS`] while
/// the keychain waits for the person, and [`STALE_STATUS`] for a stale key. The running runtime answers through
/// its socket under the state root. Only without one does the command read
/// `store` itself, and never for a provider whose stored key is stale.
pub fn run_command(
    arguments: &[String],
    store: impl FnOnce() -> Arc<dyn CredentialStore>,
) -> Option<i32> {
    let socket = muniment_core::state_root::state_directory().map(|state| socket_path(&state));
    run_command_through(arguments, socket.as_deref(), store)
}

/// [`run_command`] with the runtime's socket at `socket`.
pub fn run_command_through(
    arguments: &[String],
    socket: Option<&Path>,
    store: impl FnOnce() -> Arc<dyn CredentialStore>,
) -> Option<i32> {
    let [first, second, provider] = arguments else {
        return None;
    };
    if first != COMMAND[0] || second != COMMAND[1] {
        return None;
    }
    if !safe_provider(provider) {
        return Some(1);
    }
    #[cfg(unix)]
    let answer = socket.and_then(|socket| ask_runtime(socket, provider));
    #[cfg(not(unix))]
    let answer = {
        let _ = socket;
        None
    };
    let stale = socket
        .and_then(Path::parent)
        .and_then(Path::parent)
        .is_some_and(|state| marked_stale(&stale_marks_path(state), provider));
    let answer = answer.unwrap_or_else(|| {
        if stale {
            KeyAnswer::Stale
        } else {
            KeyAnswer::from_lookup(store().lookup(SERVICE, provider))
        }
    });
    if let KeyAnswer::Key(key) = &answer {
        println!("{key}");
    }
    Some(answer.status())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{CredentialRefusal, FileCredentialStore};
    use muniment_core::serde_json::{self, json};
    use std::path::PathBuf;

    fn temporary(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "muniment-endpoint-secrets-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn models(agent: &Path) -> Value {
        serde_json::from_slice(&std::fs::read(agent.join("models.json")).unwrap()).unwrap()
    }

    #[test]
    fn a_plain_key_moves_to_the_store_and_pi_reads_it_through_the_runtime() {
        let root = temporary("seal");
        let agent = root.join("agent");
        std::fs::create_dir_all(&agent).unwrap();
        std::fs::write(
            agent.join("models.json"),
            serde_json::to_vec(&json!({"providers": {
                "custom-proxy": {"baseUrl": "https://proxy.example/v1", "apiKey": "sk-secret"},
                "ollama": {"baseUrl": "http://127.0.0.1:11434/v1", "apiKey": "ollama"},
                "lmstudio": {"baseUrl": "http://127.0.0.1:1234/v1", "apiKey": "local"},
                "custom-env": {"baseUrl": "https://env.example/v1", "apiKey": "$PROXY_KEY"},
                "custom-own": {"baseUrl": "https://own.example/v1", "apiKey": "!op read key"},
                ROUTER_PROVIDER: {"baseUrl": "http://127.0.0.1:1/v1", "apiKey": "router-token"}
            }}))
            .unwrap(),
        )
        .unwrap();
        let store = FileCredentialStore::new(root.join("credentials"));
        let executable = Path::new("/opt/muniment/muniment-runtime");

        assert_eq!(seal(&agent, &store, executable).unwrap(), 1);
        let written = models(&agent);
        let providers = &written["providers"];
        assert_eq!(
            providers["custom-proxy"]["apiKey"],
            "!\"/opt/muniment/muniment-runtime\" secret get custom-proxy"
        );
        assert!(!std::fs::read_to_string(agent.join("models.json"))
            .unwrap()
            .contains("sk-secret"));
        assert_eq!(
            store.read(SERVICE, "custom-proxy").as_deref(),
            Some("sk-secret")
        );
        for (provider, kept) in [
            ("ollama", "ollama"),
            ("lmstudio", "local"),
            ("custom-env", "$PROXY_KEY"),
            ("custom-own", "!op read key"),
            (ROUTER_PROVIDER, "router-token"),
        ] {
            assert_eq!(providers[provider]["apiKey"], kept, "{provider}");
        }

        // A second pass changes nothing, and a moved runtime takes its references along.
        assert_eq!(seal(&agent, &store, executable).unwrap(), 0);
        let moved = Path::new("/Applications/Muniment.app/Contents/MacOS/muniment-runtime");
        assert_eq!(seal(&agent, &store, moved).unwrap(), 1);
        assert_eq!(
            models(&agent)["providers"]["custom-proxy"]["apiKey"],
            "!\"/Applications/Muniment.app/Contents/MacOS/muniment-runtime\" secret get custom-proxy"
        );
        assert_eq!(
            store.read(SERVICE, "custom-proxy").as_deref(),
            Some("sk-secret")
        );

        forget(&store, "custom-proxy").unwrap();
        assert_eq!(store.read(SERVICE, "custom-proxy"), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_key_the_store_refuses_leaves_the_file_for_the_caller_and_asks_once() {
        let root = temporary("refused");
        let agent = root.join("agent");
        std::fs::create_dir_all(&agent).unwrap();
        std::fs::write(
            agent.join("models.json"),
            r#"{"providers":{"custom-proxy":{"baseUrl":"https://proxy.example/v1","apiKey":"sk-kept"}}}"#,
        )
        .unwrap();
        let store = Refusing::default();
        let executable = Path::new("/opt/runtime");
        let mut held = Vec::new();
        let mut moved = |provider: &str, sealed: Sealed<'_>| match sealed {
            Sealed::Refused(key) => held.push((provider.to_owned(), key.to_owned())),
            Sealed::Stored => panic!("the store refused the key"),
        };
        assert_eq!(
            seal_with(&agent, &store, executable, &mut moved).unwrap(),
            1
        );
        // Every later pass finds the reference and asks the store nothing.
        for _ in 0..7 {
            assert_eq!(
                seal_with(&agent, &store, executable, &mut moved).unwrap(),
                0
            );
        }
        assert_eq!(held, [("custom-proxy".to_owned(), "sk-kept".to_owned())]);
        assert_eq!(store.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(!std::fs::read_to_string(agent.join("models.json"))
            .unwrap()
            .contains("sk-kept"));
        assert_eq!(
            models(&agent)["providers"]["custom-proxy"]["apiKey"],
            "!\"/opt/runtime\" secret get custom-proxy"
        );

        // The broker answers the kept key without a store read, until a
        // disconnect or a stored key drops it.
        let broker = KeyBroker::new(Arc::new(Refusing::default()));
        broker.keep("custom-proxy", "sk-kept");
        assert!(broker.kept_in_memory("custom-proxy"));
        assert_eq!(
            broker.answer("custom-proxy"),
            KeyAnswer::Key("sk-kept".into())
        );
        broker.forget("custom-proxy");
        assert!(!broker.kept_in_memory("custom-proxy"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_key_the_store_refused_never_brings_back_the_old_key_after_a_restart() {
        let root = temporary("stale");
        let marks = stale_marks_path(&root);
        let credentials = root.join("credentials");
        // The keychain holds the old key and refuses to replace it.
        let store = Arc::new(FileCredentialStore::new(&credentials));
        store.write(SERVICE, "custom-live29", "sk-old").unwrap();
        let broker = KeyBroker::marking(
            Arc::clone(&store) as Arc<dyn CredentialStore>,
            marks.clone(),
        );
        broker.keep("custom-live29", "sk-new");
        assert_eq!(
            broker.answer("custom-live29"),
            KeyAnswer::Key("sk-new".into())
        );
        assert!(!broker.stale("custom-live29"));

        // After a restart the runtime holds no key, and the old one stays out.
        let restarted = KeyBroker::marking(
            Arc::clone(&store) as Arc<dyn CredentialStore>,
            marks.clone(),
        );
        assert!(restarted.stale("custom-live29"));
        assert_eq!(restarted.answer("custom-live29"), KeyAnswer::Stale);
        assert_eq!(KeyAnswer::Stale.status(), STALE_STATUS);
        assert_ne!(STALE_STATUS, REFUSED_STATUS);
        assert_eq!(restarted.held("custom-live29"), None);
        // A command that finds no runtime reads no stale key either, and
        // its status says the key is stale, not refused.
        let open = || Arc::new(FileCredentialStore::new(&credentials)) as Arc<dyn CredentialStore>;
        let socket = socket_path(&root);
        assert_eq!(
            run_command_through(&secret_get("custom-live29"), Some(&socket), open),
            Some(STALE_STATUS)
        );
        assert_eq!(
            run_command_through(&secret_get("custom-other"), Some(&socket), open),
            Some(1)
        );
        store.write(SERVICE, "custom-other", "sk-other").unwrap();
        assert_eq!(
            run_command_through(&secret_get("custom-other"), Some(&socket), open),
            Some(0)
        );

        // A key the store takes, or a disconnect, clears the mark.
        restarted.forget("custom-live29");
        assert!(!restarted.stale("custom-live29"));
        assert_eq!(
            restarted.answer("custom-live29"),
            KeyAnswer::Key("sk-old".into())
        );
        assert!(stale_key_notice("custom-live29").contains("Enter the key again"));
        let _ = std::fs::remove_dir_all(root);
    }

    /// A store whose every read and write the keychain declined.
    #[derive(Default)]
    struct Refusing {
        writes: std::sync::atomic::AtomicUsize,
    }

    impl CredentialStore for Refusing {
        fn read(&self, _: &str, _: &str) -> Option<String> {
            None
        }

        fn lookup(&self, _: &str, _: &str) -> Result<Option<String>, CredentialRefusal> {
            Err(CredentialRefusal::Declined(
                "User canceled the operation.".into(),
            ))
        }

        fn write(&self, _: &str, _: &str, _: &str) -> Result<(), String> {
            self.writes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err("The keychain refused the secret: User canceled the operation.".into())
        }

        fn remove(&self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
    }

    /// A keychain whose write lands while a save edits `models.json`, as a
    /// save does while a prompt waits for the person.
    struct SavingDuringWrite {
        models_file: PathBuf,
        free: std::sync::atomic::AtomicBool,
    }

    impl CredentialStore for SavingDuringWrite {
        fn read(&self, _: &str, _: &str) -> Option<String> {
            None
        }

        fn lookup(&self, _: &str, _: &str) -> Result<Option<String>, CredentialRefusal> {
            Ok(None)
        }

        fn write(&self, _: &str, provider: &str, _: &str) -> Result<(), String> {
            let started = std::time::Instant::now();
            let save =
                pi_models::lock_with(&self.models_file, Duration::ZERO, pi_models::LOCK_STALE);
            self.free.store(
                save.is_ok() && started.elapsed() < Duration::from_secs(1),
                std::sync::atomic::Ordering::SeqCst,
            );
            if provider == "custom-edited" {
                let mut root = pi_models::read_json(&self.models_file).unwrap();
                root["providers"]["custom-edited"]["apiKey"] = "sk-newer".into();
                pi_models::write_json(&self.models_file, &root).unwrap();
            }
            Ok(())
        }

        fn remove(&self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn a_seal_writes_the_store_without_the_lock_and_keeps_a_newer_save() {
        let root = temporary("unlocked");
        let agent = root.join("agent");
        std::fs::create_dir_all(&agent).unwrap();
        std::fs::write(
            agent.join("models.json"),
            r#"{"providers":{"custom-edited":{"baseUrl":"https://a.example/v1","apiKey":"sk-old"},"custom-proxy":{"baseUrl":"https://b.example/v1","apiKey":"sk-plain"}}}"#,
        )
        .unwrap();
        let store = SavingDuringWrite {
            models_file: pi_models::models_file(&agent),
            free: Default::default(),
        };
        let executable = Path::new("/opt/runtime");
        let mut sealed = Vec::new();
        let mut moved = |provider: &str, _: Sealed<'_>| sealed.push(provider.to_owned());
        assert_eq!(
            seal_with(&agent, &store, executable, &mut moved).unwrap(),
            1
        );
        // A save took the lock while the store wrote.
        assert!(store.free.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(sealed, ["custom-proxy"]);
        let saved = models(&agent);
        assert_eq!(
            saved["providers"]["custom-proxy"]["apiKey"],
            "!\"/opt/runtime\" secret get custom-proxy"
        );
        // The key the save wrote stays for the pass after that save.
        assert_eq!(saved["providers"]["custom-edited"]["apiKey"], "sk-newer");
        assert!(!pi_models::lock_path(&pi_models::models_file(&agent)).exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_runtime_answers_only_the_secret_command() {
        let root = temporary("command");
        let store = FileCredentialStore::new(root.join("credentials"));
        store.write(SERVICE, "custom-proxy", "sk-secret").unwrap();
        let open = || {
            Arc::new(FileCredentialStore::new(root.join("credentials"))) as Arc<dyn CredentialStore>
        };
        let arguments = |values: &[&str]| {
            values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            run_command_through(&arguments(&["secret", "get", "custom-proxy"]), None, open),
            Some(0)
        );
        assert_eq!(
            run_command_through(&arguments(&["secret", "get", "custom-missing"]), None, open),
            Some(1)
        );
        assert_eq!(
            run_command_through(&arguments(&["secret", "get", "../escape"]), None, open),
            Some(1)
        );
        assert_eq!(
            run_command_through(&arguments(&["--version"]), None, open),
            None
        );
        let refusing = || Arc::new(Refusing::default()) as Arc<dyn CredentialStore>;
        assert_eq!(
            run_command_through(
                &arguments(&["secret", "get", "custom-proxy"]),
                None,
                refusing
            ),
            Some(REFUSED_STATUS)
        );
        assert_eq!(
            run_command_through(&arguments(&["secret", "list", "x"]), None, open),
            None
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// A keychain stand-in: each read the store has not refused waits
    /// `delay` for the person, then answers what `next` holds. It remembers
    /// refusals as the keychain store does.
    struct SlowKeychain {
        delay: Mutex<Duration>,
        next: Mutex<Result<Option<String>, CredentialRefusal>>,
        asks: std::sync::atomic::AtomicUsize,
        refusals: muniment_core::model_router::credentials::RefusalMemory,
    }

    impl SlowKeychain {
        fn new(delay: Duration, next: Result<Option<String>, CredentialRefusal>) -> Arc<Self> {
            Arc::new(Self {
                delay: Mutex::new(delay),
                next: Mutex::new(next),
                asks: Default::default(),
                refusals: Default::default(),
            })
        }

        fn asks(&self) -> usize {
            self.asks.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl CredentialStore for SlowKeychain {
        fn read(&self, service: &str, name: &str) -> Option<String> {
            self.lookup(service, name).ok().flatten()
        }

        fn lookup(&self, service: &str, name: &str) -> Result<Option<String>, CredentialRefusal> {
            if let Some(refusal) = self.refusals.held(service, name, std::time::Instant::now()) {
                return Err(refusal);
            }
            self.asks.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let delay = *self.delay.lock().unwrap();
            std::thread::sleep(delay);
            let answer = self.next.lock().unwrap().clone();
            match &answer {
                Err(refusal) => {
                    self.refusals
                        .note(service, name, refusal, std::time::Instant::now())
                }
                Ok(_) => self.refusals.answered(service, name),
            }
            answer
        }

        fn write(&self, _: &str, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }

        fn remove(&self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }

        fn refusals(&self) -> Vec<muniment_core::model_router::credentials::RefusedEntry> {
            self.refusals.list()
        }

        fn retry_refused(&self) {
            self.refusals.clear();
        }

        fn answered(&self, service: &str, name: &str) {
            self.refusals.settled(service, name);
        }
    }

    #[test]
    fn a_key_that_works_ends_the_failure_the_store_holds_for_it() {
        let store = SlowKeychain::new(Duration::ZERO, Ok(None));
        let failed = CredentialRefusal::Failed("The specified item already exists.".into());
        let now = std::time::Instant::now();
        store.refusals.note(SERVICE, "custom-live30", &failed, now);
        store.refusals.note(SERVICE, "custom-other", &failed, now);
        let broker = KeyBroker::new(Arc::clone(&store) as Arc<dyn CredentialStore>);
        // The store refused the new key, so the broker holds it, and the
        // Machines notice no longer names the failure.
        broker.keep("custom-live30", "sk-new");
        let names = |store: &SlowKeychain| {
            store
                .refusals()
                .into_iter()
                .map(|entry| entry.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&store), ["custom-other"]);
        // A key the broker answers ends a failure noted since.
        store.refusals.note(SERVICE, "custom-live30", &failed, now);
        assert_eq!(
            broker.answer("custom-live30"),
            KeyAnswer::Key("sk-new".into())
        );
        assert_eq!(names(&store), ["custom-other"]);
        // A declined prompt stays until the person retries.
        let declined = CredentialRefusal::Declined("User canceled the operation.".into());
        store
            .refusals
            .note(SERVICE, "custom-live30", &declined, now);
        broker.answer("custom-live30");
        assert_eq!(names(&store), ["custom-live30", "custom-other"]);
    }

    fn secret_get(provider: &str) -> Vec<String> {
        ["secret", "get", provider]
            .iter()
            .map(|value| value.to_string())
            .collect()
    }

    #[test]
    fn the_runtime_answers_within_pis_command_timeout() {
        // Pi kills a key command after 10 seconds.
        assert!(ANSWER_WAIT < Duration::from_secs(10));
        #[cfg(unix)]
        assert!(ANSWER_WAIT < COMMAND_WAIT && COMMAND_WAIT < Duration::from_secs(10));
    }

    #[cfg(unix)]
    #[test]
    fn a_keychain_answer_after_pis_timeout_reaches_the_next_request_and_asks_once() {
        let root = std::env::temp_dir().join(format!(
            "mek-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ));
        let socket = socket_path(&root);
        // The person declines the prompt 15 seconds after it shows.
        let keychain = SlowKeychain::new(
            Duration::from_secs(15),
            Err(CredentialRefusal::Declined(
                "User canceled the operation.".into(),
            )),
        );
        let broker = KeyBroker::waiting(
            Arc::clone(&keychain) as Arc<dyn CredentialStore>,
            Duration::from_secs(1),
        );
        serve(Arc::clone(&broker), &socket).unwrap();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(socket.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        let never = || -> Arc<dyn CredentialStore> { panic!("the runtime answers") };
        let started = std::time::Instant::now();

        // Requests while the prompt is open answer pending within Pi's
        // timeout, and raise no second prompt.
        for _ in 0..3 {
            let asked = std::time::Instant::now();
            assert_eq!(
                run_command_through(&secret_get("custom-live27"), Some(&socket), never),
                Some(PENDING_STATUS)
            );
            assert!(asked.elapsed() < Duration::from_secs(10));
        }
        assert!(broker.asking("custom-live27"));
        assert_eq!(keychain.asks(), 1);

        // The late decline is recorded, and every later request answers at
        // once without a prompt.
        while broker.asking("custom-live27") {
            assert!(started.elapsed() < Duration::from_secs(30));
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(keychain.refusals().len(), 1);
        for _ in 0..3 {
            let asked = std::time::Instant::now();
            assert_eq!(
                run_command_through(&secret_get("custom-live27"), Some(&socket), never),
                Some(REFUSED_STATUS)
            );
            assert!(asked.elapsed() < Duration::from_millis(500));
        }
        assert_eq!(keychain.asks(), 1);

        // After the person's retry the next request asks once more, and the
        // key it gets serves the requests after it from the runtime.
        *keychain.delay.lock().unwrap() = Duration::from_millis(50);
        *keychain.next.lock().unwrap() = Ok(Some("sk-live".into()));
        keychain.retry_refused();
        assert_eq!(
            broker.answer("custom-live27"),
            KeyAnswer::Key("sk-live".into())
        );
        assert_eq!(
            run_command_through(&secret_get("custom-live27"), Some(&socket), never),
            Some(0)
        );
        assert_eq!(keychain.asks(), 2);
        assert_eq!(
            format!("{:?}", broker.answer("custom-live27")),
            "Key(<redacted>)"
        );

        // A new key drops the held one.
        broker.forget("custom-live27");
        assert_eq!(broker.held("custom-live27"), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_quick_answer_serves_the_request_that_asked() {
        let keychain = SlowKeychain::new(Duration::from_millis(50), Ok(Some("sk-key".into())));
        let broker = KeyBroker::new(Arc::clone(&keychain) as Arc<dyn CredentialStore>);
        assert_eq!(broker.held("custom-proxy"), None);
        assert_eq!(
            broker.answer("custom-proxy"),
            KeyAnswer::Key("sk-key".into())
        );
        assert_eq!(
            broker.answer("custom-proxy"),
            KeyAnswer::Key("sk-key".into())
        );
        assert_eq!(broker.held("custom-proxy").as_deref(), Some("sk-key"));
        assert_eq!(keychain.asks(), 1);
        assert_eq!(broker.answer("../escape"), KeyAnswer::Missing);
        *keychain.next.lock().unwrap() = Ok(None);
        assert_eq!(broker.answer("custom-gone"), KeyAnswer::Missing);
    }

    #[cfg(unix)]
    #[test]
    fn a_command_without_a_runtime_reads_the_store_itself() {
        let root = temporary("no-runtime");
        let store = FileCredentialStore::new(root.join("credentials"));
        store.write(SERVICE, "custom-proxy", "sk-secret").unwrap();
        let open = || {
            Arc::new(FileCredentialStore::new(root.join("credentials"))) as Arc<dyn CredentialStore>
        };
        assert_eq!(
            run_command_through(
                &secret_get("custom-proxy"),
                Some(&root.join("endpoint-keys").join("socket")),
                open
            ),
            Some(0)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn a_reference_resolves_through_the_command_it_names() {
        use std::os::unix::fs::PermissionsExt;
        let root = temporary("resolve");
        let credentials = root.join("credentials");
        FileCredentialStore::new(&credentials)
            .write(SERVICE, "custom-proxy", "sk-secret")
            .unwrap();
        // A stand-in for the runtime executable that answers from the same store.
        let executable = root.join("runtime");
        std::fs::write(
            &executable,
            format!(
                "#!/bin/sh\n[ \"$1 $2\" = \"secret get\" ] && cat \"{}/{SERVICE}/$3\"\n",
                credentials.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        let value = reference(&executable, "custom-proxy").unwrap();
        assert_eq!(pi_models::resolve_key(&value).as_deref(), Some("sk-secret"));
        assert_eq!(
            pi_models::resolve_key("sk-plain").as_deref(),
            Some("sk-plain")
        );
        assert_eq!(pi_models::resolve_key("!exit 3"), None);
        let _ = std::fs::remove_dir_all(root);
    }
}
