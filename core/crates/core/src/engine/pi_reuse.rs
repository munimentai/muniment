//! Keeps a thread's idle Pi process for the thread's next reply.
//!
//! A Pi process reads its arguments, its environment and a set of files once,
//! when it starts. A reply that ends cleanly leaves its process idle in the
//! thread's slot, marked with what it started from. The thread's next reply
//! takes that process only when the launch it would start now is the same
//! launch, the files Pi read are unchanged, the process still answers, holds
//! the thread's saved conversation and runs no turn. A thread folder renamed
//! after its title keeps its process: the process reopens the conversation in
//! the renamed folder, so its tools work there. Anything else starts a new
//! process.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::pi_execution::PiRuntime;
use crate::sidecar::{
    PiRpcTransport, PiSessionLocator, SidecarConfig, SidecarStatus, SECRET_VARIABLE_PREFIX,
};

/// The bound on the state read that checks an idle process before reuse.
const REUSE_READ_TIMEOUT: Duration = Duration::from_secs(1);
/// How long an idle process may wait for its thread's next reply.
pub const REUSE_IDLE_WINDOW: Duration = Duration::from_secs(15 * 60);
/// The agent directory files Pi reads when it starts, compared by content.
const AGENT_FILES: [&str; 11] = [
    "settings.json",
    "models.json",
    "auth.json",
    "mcp.json",
    "mcp-auth.json",
    "web-search.json",
    "trust.json",
    "AGENTS.md",
    "CLAUDE.md",
    "SYSTEM.md",
    "APPEND_SYSTEM.md",
];
/// The agent directory folders Pi lists when it starts, compared by entry.
const AGENT_TREES: [&str; 4] = ["skills", "prompts", "extensions", "themes"];
/// The project files and folders Pi reads in its working folder.
const PROJECT_TREES: [&str; 2] = [".pi", ".agents"];
/// The context files Pi reads in its working folder and each folder above it.
const CONTEXT_FILES: [&str; 2] = ["AGENTS.md", "CLAUDE.md"];
/// The bound on the entries one tree contributes.
const TREE_ENTRIES: usize = 512;

/// What one Pi launch starts from: its program, arguments, environment and
/// working folder, and the files Pi reads at start. The conversation it reopens is left out,
/// because reuse checks the conversation the process holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaunchKey {
    config: u64,
    files: u64,
}

impl LaunchKey {
    pub fn of(config: &SidecarConfig) -> Self {
        Self {
            config: config_hash(config, None),
            files: files_hash(config),
        }
    }
}

/// The launch one running process started from, kept for the length of a run.
pub struct PiLaunch {
    config: SidecarConfig,
    key: LaunchKey,
    place: Option<Place>,
    moved: bool,
}

impl PiLaunch {
    /// The launch of a process that just became ready. The files are read
    /// after Pi's own start, so a file Pi writes as it starts is part of it.
    pub fn ready(config: SidecarConfig) -> Self {
        let key = LaunchKey::of(&config);
        let place = Place::of(&config);
        Self {
            config,
            key,
            place,
            moved: false,
        }
    }

    /// Whether the process followed its thread's renamed folder to take
    /// this reply.
    pub fn moved(&self) -> bool {
        self.moved
    }
}

/// The folder a process works in, by identity, so a rename of the folder
/// is told apart from a move to another folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FolderId {
    device: u64,
    inode: u64,
}

fn folder_id(path: &Path) -> Option<FolderId> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::symlink_metadata(path).ok()?;
        metadata.is_dir().then(|| FolderId {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
    // Windows refuses to rename the folder a process works in.
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

/// Where a process works: its folder by path and identity, and its launch
/// with the folder's path left out.
#[derive(Clone, Debug)]
struct Place {
    folder: PathBuf,
    id: FolderId,
    unplaced: u64,
}

impl Place {
    fn of(config: &SidecarConfig) -> Option<Self> {
        let folder = config.working_directory.as_deref()?;
        Some(Self {
            folder: folder.to_path_buf(),
            id: folder_id(folder)?,
            unplaced: config_hash(config, Some(folder)),
        })
    }

    /// The folder `config` works in when it is this folder under a new
    /// name: the old name is gone, the new name is the same folder, and the
    /// launch differs only in the folder's path.
    fn renamed<'a>(&self, config: &'a SidecarConfig) -> Option<&'a Path> {
        let folder = config.working_directory.as_deref()?;
        let gone = std::fs::symlink_metadata(&self.folder)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound);
        (folder != self.folder
            && gone
            && folder_id(folder) == Some(self.id)
            && config_hash(config, Some(folder)) == self.unplaced)
            .then_some(folder)
    }
}

/// The mark an idle process carries while its thread's next reply may take it.
pub struct PiReuse {
    key: LaunchKey,
    place: Option<Place>,
    transport: Arc<PiRpcTransport>,
    parked: Instant,
}

/// Why an idle process did not take a reply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReuseMiss {
    /// The last reply did not end cleanly, or the process never ran one.
    Unmarked,
    /// The process waited longer than [`REUSE_IDLE_WINDOW`].
    Expired,
    /// The process stopped, restarted or does not answer.
    Stopped,
    /// The launch differs: arguments, environment, prompt or working folder.
    Launch,
    /// The process could not reopen the conversation in its renamed folder.
    Moved,
    /// A file Pi reads at start changed.
    Files,
    /// The thread has no saved conversation, or the process holds another.
    Session,
    /// The process still runs a turn or holds queued messages.
    Busy,
}

impl ReuseMiss {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unmarked => "unmarked",
            Self::Expired => "expired",
            Self::Stopped => "stopped",
            Self::Launch => "launch",
            Self::Moved => "moved",
            Self::Files => "files",
            Self::Session => "session",
            Self::Busy => "busy",
        }
    }
}

impl super::PiRpcEngine {
    /// Marks a process idle for its thread's next reply after a reply that
    /// ended cleanly. A process that stopped, or whose start files changed
    /// while it ran, stays unmarked, so the next reply starts a new one.
    pub fn park(&self, process: &mut PiRuntime, launch: PiLaunch) {
        park(process, launch);
    }

    /// Hands an idle process the next reply in its thread when it can take
    /// it. `config` is the launch the reply would start now and `locator`
    /// the conversation the thread's last reply bound. A process that cannot
    /// take the reply stops, and the answer names why.
    pub fn reuse(
        &self,
        process: PiRuntime,
        config: &SidecarConfig,
        locator: Option<&PiSessionLocator>,
    ) -> Result<(PiRuntime, Arc<PiRpcTransport>, PiLaunch), ReuseMiss> {
        match self.session_root.as_deref() {
            Some(root) => take(process, config, root, locator),
            None => Err(ReuseMiss::Session),
        }
    }
}

fn park(process: &mut PiRuntime, launch: PiLaunch) {
    process.reuse = None;
    if process.supervisor.status() != SidecarStatus::Healthy {
        return;
    }
    let Some(transport) = process.wiring.transport() else {
        return;
    };
    if LaunchKey::of(&launch.config) != launch.key {
        return;
    }
    process.reuse = Some(PiReuse {
        key: launch.key,
        place: launch.place,
        transport,
        parked: Instant::now(),
    });
}

fn take(
    mut process: PiRuntime,
    config: &SidecarConfig,
    session_root: &Path,
    locator: Option<&PiSessionLocator>,
) -> Result<(PiRuntime, Arc<PiRpcTransport>, PiLaunch), ReuseMiss> {
    let Some(reuse) = process.reuse.take() else {
        return Err(ReuseMiss::Unmarked);
    };
    if reuse.parked.elapsed() > REUSE_IDLE_WINDOW {
        return Err(ReuseMiss::Expired);
    }
    let current = process.wiring.transport();
    if process.supervisor.status() != SidecarStatus::Healthy
        || !current.is_some_and(|current| Arc::ptr_eq(&current, &reuse.transport))
    {
        return Err(ReuseMiss::Stopped);
    }
    let key = LaunchKey::of(config);
    // A launch that differs only because the thread's folder took its
    // title is the same launch in the renamed folder.
    let moved = if key.config == reuse.key.config {
        None
    } else {
        Some(
            reuse
                .place
                .as_ref()
                .and_then(|place| place.renamed(config))
                .ok_or(ReuseMiss::Launch)?,
        )
    };
    if key.files != reuse.key.files {
        return Err(ReuseMiss::Files);
    }
    let Some(locator) = locator else {
        return Err(ReuseMiss::Session);
    };
    let state = match reuse
        .transport
        .call(json!({"type": "get_state"}), REUSE_READ_TIMEOUT)
    {
        Ok(response)
            if response.get("type").and_then(Value::as_str) == Some("response")
                && response.get("command").and_then(Value::as_str) == Some("get_state")
                && response.get("success").and_then(Value::as_bool) == Some(true) =>
        {
            response.get("data").cloned().unwrap_or(Value::Null)
        }
        _ => return Err(ReuseMiss::Stopped),
    };
    if busy(&state) {
        return Err(ReuseMiss::Busy);
    }
    if !holds(&state, session_root, locator) {
        return Err(ReuseMiss::Session);
    }
    if let Some(folder) = moved {
        follow(
            &reuse.transport,
            session_root,
            locator,
            folder,
            config.startup_timeout,
        )?;
    }
    let transport = Arc::clone(&reuse.transport);
    Ok((
        process,
        transport,
        PiLaunch {
            config: config.clone(),
            key,
            place: Place::of(config),
            moved: moved.is_some(),
        },
    ))
}

/// Moves an idle process to its thread's renamed folder. Pi rebuilds its
/// tools, extensions and context files for the working folder its saved
/// conversation names, which the launch already pointed at `folder`, when
/// it reopens that conversation.
fn follow(
    transport: &PiRpcTransport,
    session_root: &Path,
    locator: &PiSessionLocator,
    folder: &Path,
    timeout: Duration,
) -> Result<(), ReuseMiss> {
    let (_, file) = crate::sidecar::validate_pi_session(session_root, locator.as_str())
        .map_err(|_| ReuseMiss::Session)?;
    let stored = std::fs::read_to_string(&file).ok().and_then(|text| {
        let header: Value = serde_json::from_str(text.lines().next()?).ok()?;
        header.get("cwd")?.as_str().map(PathBuf::from)
    });
    if stored.as_deref() != Some(folder) {
        return Err(ReuseMiss::Launch);
    }
    let switched = transport
        .call(
            json!({"type": "switch_session", "sessionPath": file}),
            timeout,
        )
        .is_ok_and(|response| {
            response.get("type").and_then(Value::as_str) == Some("response")
                && response.get("command").and_then(Value::as_str) == Some("switch_session")
                && response.get("success").and_then(Value::as_bool) == Some(true)
                && response.pointer("/data/cancelled").and_then(Value::as_bool) != Some(true)
        });
    if !switched {
        return Err(ReuseMiss::Moved);
    }
    match transport.call(json!({"type": "get_state"}), REUSE_READ_TIMEOUT) {
        Ok(response)
            if response.get("success").and_then(Value::as_bool) == Some(true)
                && response
                    .get("data")
                    .is_some_and(|state| !busy(state) && holds(state, session_root, locator)) =>
        {
            Ok(())
        }
        _ => Err(ReuseMiss::Moved),
    }
}

/// Whether Pi's state names a turn, a compaction or queued messages.
fn busy(state: &Value) -> bool {
    let flag = |name: &str| state.get(name).and_then(Value::as_bool) == Some(true);
    flag("isStreaming")
        || flag("isCompacting")
        || flag("isRetrying")
        || state
            .get("pendingMessageCount")
            .and_then(Value::as_u64)
            .is_some_and(|count| count > 0)
}

/// Whether Pi's state names the saved conversation `locator` under the root.
fn holds(state: &Value, session_root: &Path, locator: &PiSessionLocator) -> bool {
    let Some(file) = state.get("sessionFile").and_then(Value::as_str) else {
        return false;
    };
    let Some(name) = Path::new(file).file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Ok((held, canonical)) = crate::sidecar::validate_pi_session(session_root, name) else {
        return false;
    };
    held == *locator
        && Path::new(file)
            .canonicalize()
            .is_ok_and(|file| file == canonical)
}

/// The launch without the conversation it reopens. A reply's first process
/// starts a new conversation and the next reply reopens it by name, so the
/// two launches differ only there. With `place`, the launch also leaves out
/// that working folder's path, wherever an argument or a variable names it.
fn config_hash(config: &SidecarConfig, place: Option<&Path>) -> u64 {
    let place = place.map(|folder| folder.to_string_lossy().into_owned());
    let unplaced = |text: &str| match place.as_deref() {
        Some(folder) if !folder.is_empty() => text.replace(folder, "\u{0}folder\u{0}"),
        _ => text.to_owned(),
    };
    let mut hasher = DefaultHasher::new();
    config.program.hash(&mut hasher);
    let mut args = config.args.iter();
    while let Some(arg) = args.next() {
        if arg == "--session" {
            args.next();
            continue;
        }
        unplaced(arg).hash(&mut hasher);
    }
    let mut env: Vec<_> = config
        .env
        .iter()
        .map(|(name, value)| (name, unplaced(value)))
        .collect();
    env.sort();
    env.hash(&mut hasher);
    let mut removed: Vec<_> = config.env_remove.iter().collect();
    removed.sort();
    removed.hash(&mut hasher);
    if place.is_none() {
        config.working_directory.hash(&mut hasher);
    }
    // The process inherits the runtime's own environment, apart from the
    // names the launch sets or removes and the runtime's secrets.
    let mut inherited: Vec<_> = std::env::vars_os()
        .filter(|(name, _)| {
            name.to_str().is_none_or(|name| {
                !name.starts_with(SECRET_VARIABLE_PREFIX)
                    && !config.env.contains_key(name)
                    && !config.env_remove.iter().any(|removed| removed == name)
            })
        })
        .collect();
    inherited.sort();
    inherited.hash(&mut hasher);
    hasher.finish()
}

/// The files Pi reads when it starts: its executable, each extension by
/// content, each skill, the agent directory's settings and context files,
/// and the project and context files of its working folder. A path inside
/// the working folder counts from the folder, so the folder's own name
/// leaves the files unchanged.
fn files_hash(config: &SidecarConfig) -> u64 {
    let mut files = Files {
        hasher: DefaultHasher::new(),
        base: config.working_directory.as_deref(),
    };
    stat(Path::new(&config.program), &mut files);
    for pair in config.args.windows(2) {
        match pair[0].as_str() {
            "--extension" => content(Path::new(&pair[1]), &mut files),
            "--skill" => tree(Path::new(&pair[1]), &mut files),
            _ => {}
        }
    }
    if let Some(file) = config.env.get("MUNIMENT_EXTEND_MCP") {
        content(Path::new(file), &mut files);
    }
    if let Some(agent) = config.env.get("PI_CODING_AGENT_DIR").map(Path::new) {
        for name in AGENT_FILES {
            content(&agent.join(name), &mut files);
        }
        for name in AGENT_TREES {
            tree(&agent.join(name), &mut files);
        }
    }
    if let Some(directory) = config.working_directory.as_deref() {
        for name in PROJECT_TREES {
            tree(&directory.join(name), &mut files);
        }
        for folder in directory.ancestors() {
            for name in CONTEXT_FILES {
                stat(&folder.join(name), &mut files);
            }
        }
    }
    files.hasher.finish()
}

/// The hash of the files one launch reads, and the working folder their
/// paths count from.
struct Files<'a> {
    hasher: DefaultHasher,
    base: Option<&'a Path>,
}

impl Files<'_> {
    fn path(&mut self, path: &Path) {
        match self.base.and_then(|base| path.strip_prefix(base).ok()) {
            Some(inside) => {
                true.hash(&mut self.hasher);
                inside.hash(&mut self.hasher);
            }
            None => {
                false.hash(&mut self.hasher);
                path.hash(&mut self.hasher);
            }
        }
    }
}

fn content(path: &Path, files: &mut Files) {
    files.path(path);
    let hasher = &mut files.hasher;
    match std::fs::read(path) {
        Ok(bytes) => bytes.hash(hasher),
        Err(error) => error.kind().hash(hasher),
    }
}

fn stat(path: &Path, files: &mut Files) {
    files.path(path);
    let hasher = &mut files.hasher;
    match std::fs::metadata(path) {
        Ok(metadata) => {
            metadata.is_dir().hash(hasher);
            metadata.len().hash(hasher);
            metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .hash(hasher);
        }
        Err(error) => error.kind().hash(hasher),
    }
}

/// A file, or a folder and the entries under it, by name, size and time.
fn tree(root: &Path, files: &mut Files) {
    stat(root, files);
    let mut pending = vec![(root.to_path_buf(), 0)];
    let mut seen = 0;
    while let Some((folder, depth)) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            seen += 1;
            if seen > TREE_ENTRIES {
                // A folder too large to compare is never taken as unchanged.
                Instant::now().hash(&mut files.hasher);
                return;
            }
            let path = entry.path();
            stat(&path, files);
            let file_type = entry.file_type();
            if depth < 3 && file_type.is_ok_and(|kind| kind.is_dir()) {
                pending.push((path, depth + 1));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("muniment-pi-reuse-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn launch(root: &Path) -> SidecarConfig {
        let agent = root.join("agent");
        let work = root.join("work");
        std::fs::create_dir_all(&agent).unwrap();
        std::fs::create_dir_all(&work).unwrap();
        std::fs::write(agent.join("settings.json"), "{}").unwrap();
        std::fs::write(root.join("memory.mjs"), "export default () => {}").unwrap();
        let mut config = SidecarConfig::new(root.join("pi").to_string_lossy());
        config.args = vec![
            "--mode".into(),
            "rpc".into(),
            "--system-prompt".into(),
            "prompt".into(),
            "--extension".into(),
            root.join("memory.mjs").to_string_lossy().into_owned(),
        ];
        config.env.insert(
            "PI_CODING_AGENT_DIR".into(),
            agent.to_string_lossy().into_owned(),
        );
        config.working_directory = Some(work);
        config
    }

    #[test]
    fn a_reopened_conversation_is_the_same_launch() {
        let root = folder();
        let first = launch(&root);
        let mut reopened = first.clone();
        reopened
            .args
            .extend(["--session".into(), "stub.jsonl".into()]);
        assert_eq!(LaunchKey::of(&first), LaunchKey::of(&reopened));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn every_input_pi_reads_at_start_changes_the_launch() {
        let root = folder();
        let base = launch(&root);
        let key = LaunchKey::of(&base);
        let differs = |config: &SidecarConfig| {
            let other = LaunchKey::of(config);
            assert_ne!(other, key);
        };

        let mut prompt = base.clone();
        prompt.args[3] = "another prompt".into();
        differs(&prompt);
        let mut model = base.clone();
        model.args.extend([
            "--provider".into(),
            "a".into(),
            "--model".into(),
            "b".into(),
        ]);
        differs(&model);
        let mut env = base.clone();
        env.env.insert("CLAUDE_CONFIG_DIR".into(), "other".into());
        differs(&env);
        let variable = format!("MUNIMENT_PI_REUSE_TEST_{}", std::process::id());
        std::env::set_var(&variable, "1");
        differs(&base);
        std::env::remove_var(&variable);
        assert_eq!(LaunchKey::of(&base), key);
        let mut folder_moved = base.clone();
        folder_moved.working_directory = Some(root.join("moved"));
        differs(&folder_moved);

        std::fs::write(root.join("memory.mjs"), "export default (pi) => pi").unwrap();
        assert_eq!(LaunchKey::of(&base).config, key.config);
        assert_ne!(LaunchKey::of(&base).files, key.files);
        std::fs::write(root.join("memory.mjs"), "export default () => {}").unwrap();
        assert_eq!(LaunchKey::of(&base), key);

        std::fs::write(root.join("agent/settings.json"), r#"{"a":1}"#).unwrap();
        assert_ne!(LaunchKey::of(&base).files, key.files);
        std::fs::write(root.join("agent/settings.json"), "{}").unwrap();
        assert_eq!(LaunchKey::of(&base), key);

        std::fs::write(root.join("agent/auth.json"), "{}").unwrap();
        assert_ne!(LaunchKey::of(&base).files, key.files);
        let with_auth = LaunchKey::of(&base);

        std::fs::create_dir_all(root.join("work/.pi/skills/a")).unwrap();
        assert_ne!(LaunchKey::of(&base).files, with_auth.files);
        let with_skill = LaunchKey::of(&base);

        std::fs::write(root.join("work/AGENTS.md"), "rules").unwrap();
        assert_ne!(LaunchKey::of(&base).files, with_skill.files);
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn a_renamed_folder_is_the_same_launch_in_its_new_place() {
        let root = folder();
        let work = root.join("work");
        let named = root.join("Named-work");
        let mut base = launch(&root);
        base.args[3] = format!("The working directory is {}.", work.display());
        std::fs::write(work.join("AGENTS.md"), "rules").unwrap();
        let key = LaunchKey::of(&base);
        let place = Place::of(&base).unwrap();
        let mut renamed = base.clone();
        renamed.working_directory = Some(named.clone());
        renamed.args[3] = format!("The working directory is {}.", named.display());
        // While the old name exists, the new name is another folder.
        assert!(place.renamed(&renamed).is_none());

        std::fs::rename(&work, &named).unwrap();
        assert_ne!(LaunchKey::of(&renamed).config, key.config);
        assert_eq!(LaunchKey::of(&renamed).files, key.files);
        assert_eq!(place.renamed(&renamed), Some(named.as_path()));
        // Any other change to the launch still differs.
        let mut model = renamed.clone();
        model.args.extend(["--model".into(), "b".into()]);
        assert!(place.renamed(&model).is_none());
        // A new folder under the new name is not the folder the process works in.
        std::fs::rename(&named, root.join("elsewhere")).unwrap();
        std::fs::create_dir_all(&named).unwrap();
        assert!(place.renamed(&renamed).is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn only_an_idle_state_on_the_threads_conversation_takes_a_reply() {
        let root = folder();
        let sessions = root.join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let file = sessions.join("thread.jsonl");
        std::fs::write(&file, "{}\n").unwrap();
        let (locator, _) = crate::sidecar::validate_pi_session(&sessions, "thread.jsonl").unwrap();
        let state = json!({"sessionFile": file, "isStreaming": false, "pendingMessageCount": 0});
        assert!(!busy(&state));
        assert!(holds(&state, &sessions, &locator));
        assert!(busy(&json!({"isStreaming": true})));
        assert!(busy(&json!({"isCompacting": true})));
        assert!(busy(&json!({"pendingMessageCount": 2})));
        std::fs::write(sessions.join("other.jsonl"), "{}\n").unwrap();
        let other = json!({"sessionFile": sessions.join("other.jsonl")});
        assert!(!holds(&other, &sessions, &locator));
        assert!(!holds(&json!({}), &sessions, &locator));
        let _ = std::fs::remove_dir_all(root);
    }
}
