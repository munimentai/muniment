//! Persistent agent definitions, thread membership, routines and run state.
//!
//! An agent is `<Home>/agents/<slug>/agent.md`: a JSON header in a fence and
//! the instructions below it. Every v2 field is optional, so a v1 file loads
//! unchanged, and its `schedule` reads as one cron routine.
pub use crate::agent_routines::{
    Condition, ConditionAll, ConditionAny, ConditionLeaf, ConditionOp, CreatedFrom, CronSchedule,
    McpTrigger, PollRecord, Routine, RoutineResult, Trigger, WebhookPreset, WebhookTrigger,
};
use crate::agent_runs::{NewRun, Run, RunStatus, RunStore, RunTrigger, TriggerKind};
pub use crate::permission_gate::{PermissionMode, PolicyRequest};
use chrono::{Datelike, TimeZone, Timelike};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    pub enabled: bool,
    /// daily, weekdays, or weekly. Times follow this computer's time zone.
    pub cadence: String,
    pub time: String,
    pub weekday: u32,
}
/// How a client draws an agent: a style id and the seed it draws from. Core keeps
/// no drawing; every style a client knows draws from the same seed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Avatar {
    pub style: String,
    pub seed: String,
}
/// The avatar styles an agent may carry. New agents get the first.
pub const AVATAR_STYLES: [&str; 3] = ["muniment", "muniment-v1", "muniment-v2"];
impl Avatar {
    pub fn is_valid(&self) -> bool {
        AVATAR_STYLES.contains(&self.style.as_str())
            && !self.seed.is_empty()
            && self.seed.len() <= 128
    }
}
/// The model an agent's runs use. Without one, the agent uses the default.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentModel {
    pub provider: String,
    pub id: String,
    /// A Pi thinking level, such as `medium`. None keeps the model's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
}
/// Which agents an agent may message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessagingAllow {
    Any,
    Agents(Vec<String>),
}
impl Serialize for MessagingAllow {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Any => serializer.serialize_str("any"),
            Self::Agents(ids) => ids.serialize(serializer),
        }
    }
}
impl<'de> Deserialize<'de> for MessagingAllow {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Name(String),
            Agents(Vec<String>),
        }
        match Wire::deserialize(deserializer)? {
            Wire::Name(name) if name == "any" => Ok(Self::Any),
            Wire::Name(_) => Err(serde::de::Error::custom(
                "messaging.allow is \"any\" or a list of agent ids",
            )),
            Wire::Agents(ids) => Ok(Self::Agents(ids)),
        }
    }
}
/// An agent's permission profile: the permission mode every thread the
/// agent owns runs in. Without a mode the agent runs in the runtime's
/// default agent mode. The rule lists an older profile holds are ignored.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentPermissions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<PermissionMode>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMessaging {
    pub allow: MessagingAllow,
}
/// An agent definition. Its runs get every tool its machine offers, and its
/// permission mode gates what runs. The `tools` list an older file holds is
/// ignored, and a save drops it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub label: String,
    #[serde(alias = "description")]
    pub instructions: String,
    #[serde(default)]
    pub avatar: Option<Avatar>,
    /// Reviewed source template data. External integrations remain setup requirements.
    #[serde(default)]
    pub template: Option<serde_json::Value>,
    pub project_id: Option<String>,
    /// The v1 schedule. It reads as the cron routine with id `schedule`, and
    /// a save turns it into that routine.
    pub schedule: Option<Schedule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<AgentModel>,
    /// Installed skill ids the agent's runs load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<AgentPermissions>,
    /// The runtime id of the agent's primary machine. None is this runtime.
    #[serde(default, rename = "primary_runtime")]
    pub primary_runtime: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messaging: Option<AgentMessaging>,
    #[serde(default)]
    pub routines: Vec<Routine>,
    /// Why the agent's routines are paused, such as `absence`. None runs them.
    #[serde(default, rename = "paused_reason")]
    pub paused_reason: Option<String>,
    /// Counts the moves of the agent between primary runtimes. A runtime
    /// that sees a higher epoch for an agent it runs steps down.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub epoch: u64,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunState {
    pub next_run: Option<i64>,
    #[serde(default)]
    pub schedule_key: Option<String>,
    pub last_run: Option<i64>,
    pub thread_id: Option<String>,
    pub run_id: Option<String>,
    pub status: String,
    pub error: Option<String>,
    /// What started the run: `manual` or a trigger kind such as `cron`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
}
/// The runtime's record of one routine: when it fires next and how its
/// last run went.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineState {
    /// Changes when the routine's trigger changes.
    #[serde(default)]
    pub key: String,
    pub next_fire_at: Option<i64>,
    pub last_fired_at: Option<String>,
    pub last_result: Option<RoutineResult>,
    /// The last poll of an MCP routine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_poll: Option<PollRecord>,
    /// The value at each `changed` condition path on the last poll.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub condition_values: BTreeMap<String, serde_json::Value>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    #[serde(default)]
    pub primary_threads: BTreeMap<String, String>,
    #[serde(default)]
    pub history: BTreeMap<String, Vec<RunState>>,
    #[serde(default)]
    pub threads: BTreeMap<String, String>,
    #[serde(default)]
    pub runs: BTreeMap<String, RunState>,
    /// Routine state by agent id, then routine id.
    #[serde(default)]
    pub routines: BTreeMap<String, BTreeMap<String, RoutineState>>,
    /// The ids of routine proposals the user dismissed, by agent id, so a
    /// later extraction does not propose them again.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dismissed_proposals: BTreeMap<String, Vec<String>>,
    /// The SHA-256 of the instructions a model last read for proposals, by
    /// agent id, so one version is read once.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub refined_instructions: BTreeMap<String, String>,
}
/// An agent folder whose `agent.md` could not be loaded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentLoadError {
    pub folder: String,
    pub error: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentList {
    pub agents: Vec<Agent>,
    pub state: State,
    /// Agent files that did not load. The other agents still list.
    #[serde(default)]
    pub errors: Vec<AgentLoadError>,
}
/// The most runs each agent's state history keeps.
pub const MAX_STATE_HISTORY: usize = crate::agent_runs::MAX_RUN_HISTORY;

fn lock(profile: &Path) -> Result<fs::File, String> {
    fs::create_dir_all(profile).map_err(|_| "Agent settings are unavailable.")?;
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(profile.join("agents.lock"))
        .map_err(|_| "Agent settings are unavailable.")?;
    file.lock_exclusive()
        .map_err(|_| "Agent settings are busy.")?;
    Ok(file)
}
fn root(profile: &Path) -> Result<PathBuf, String> {
    let root = crate::memory_files::home(profile)?.join("agents");
    if root.is_symlink() {
        return Err("The agents folder must not be a symbolic link.".into());
    }
    fs::create_dir_all(&root).map_err(|_| "The agents folder is unavailable.")?;
    Ok(root)
}
/// Where an agent's folder is, found without a write.
enum Location {
    /// The folder the name index records.
    Recorded(PathBuf),
    /// A folder whose `agent.md` holds the agent, which no save recorded
    /// yet, with the agent's name.
    Unrecorded(PathBuf, String),
    Missing,
}
fn locate(profile: &Path, id: &str) -> Result<Location, String> {
    let root = crate::memory_files::home(profile)?.join("agents");
    if let Some(name) = crate::workspace_names::recorded(profile, &root, id)? {
        return Ok(Location::Recorded(root.join(name)));
    }
    let Ok(entries) = fs::read_dir(&root) else {
        return Ok(Location::Missing);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_symlink() {
            continue;
        }
        if let Ok(agent) = read_file(&path.join("agent.md"), Some(id)) {
            return Ok(Location::Unrecorded(path, agent.name));
        }
    }
    Ok(Location::Missing)
}
/// An agent's folder for a write. A folder no save recorded yet is recorded
/// under its agent's name. An agent no folder holds, such as one a delete
/// removed, has no folder: a write for it would make an `Untitled-` folder
/// that no agent owns.
pub fn folder(profile: &Path, id: &str) -> Result<PathBuf, String> {
    uuid::Uuid::parse_str(id).map_err(|_| "The agent identifier is invalid.")?;
    let (name, legacy) = match locate(profile, id)? {
        Location::Unrecorded(folder, name) => (Some(name), Some(folder)),
        Location::Recorded(_) => (None, None),
        Location::Missing => return Err("The agent is unavailable.".into()),
    };
    let folder = crate::workspace_names::resolve(
        profile,
        &root(profile)?,
        id,
        name.as_deref(),
        legacy.as_deref(),
    )?;
    if folder.is_symlink() {
        return Err("The agent folder must not be a symbolic link.".into());
    }
    Ok(folder)
}
pub(crate) fn write(path: &Path, content: &[u8]) -> Result<(), String> {
    use std::io::Write;
    write_with(path, content, fs::File::write_all, fs::File::sync_all)
}
fn write_with(
    path: &Path,
    content: &[u8],
    write_temp: impl FnOnce(&mut fs::File, &[u8]) -> std::io::Result<()>,
    sync_temp: impl FnOnce(&fs::File) -> std::io::Result<()>,
) -> Result<(), String> {
    if path.is_symlink() {
        return Err("The agent file must not be a symbolic link.".into());
    }
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| write_error("create_temp", error))?;
    let result = write_temp(&mut file, content)
        .map_err(|error| write_error("write_temp", error))
        .and_then(|()| sync_temp(&file).map_err(|error| write_error("sync_temp", error)));
    // Close the handle before replacement or cleanup, including write and sync failures.
    drop(file);
    let result = result.and_then(|()| {
        crate::home::replace_file(&temporary, path)
            .map_err(|error| write_error("replace_file", error))
    });
    let _ = fs::remove_file(temporary);
    result
}
fn write_error(step: &str, error: std::io::Error) -> String {
    use crate::disk_full::DiskFull;
    if error.is_disk_full() {
        return crate::disk_full::DISK_FULL_MESSAGE.into();
    }
    let code = error
        .raw_os_error()
        .map(|code| code.to_string())
        .unwrap_or_else(|| "unavailable".into());
    // Error text can contain paths or content. Report only the step and error codes.
    format!(
        "The agent save failed at {step} (OS error {code}, kind {:?}).",
        error.kind()
    )
}
pub fn state(profile: &Path) -> Result<State, String> {
    match fs::read(profile.join("agent-state.json")) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|_| "Agent state cannot be read.".into())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
        Err(_) => Err("Agent state cannot be read.".into()),
    }
}
pub fn persist_state(profile: &Path, state: &State) -> Result<(), String> {
    write(
        &profile.join("agent-state.json"),
        &serde_json::to_vec(state).map_err(|_| "Agent state cannot be saved.")?,
    )
}
/// Reads one agent. A read writes nothing: it finds a folder no save
/// recorded yet and records none.
pub fn get(profile: &Path, id: &str) -> Result<Agent, String> {
    uuid::Uuid::parse_str(id).map_err(|_| "The agent identifier is invalid.")?;
    let folder = match locate(profile, id)? {
        Location::Recorded(folder) | Location::Unrecorded(folder, _) => folder,
        Location::Missing => return Err("The agent is unavailable.".into()),
    };
    if folder.is_symlink() {
        return Err("The agent folder must not be a symbolic link.".into());
    }
    read_file(&folder.join("agent.md"), Some(id))
}
/// Reads one `agent.md`. With `id`, the file must hold that agent.
fn read_file(path: &Path, id: Option<&str>) -> Result<Agent, String> {
    if path.is_symlink() {
        return Err("The agent file must not be a symbolic link.".into());
    }
    if fs::metadata(path)
        .map_err(|_| "The agent is unavailable.")?
        .len()
        > 128 * 1024
    {
        return Err("The agent file is too large.".into());
    }
    let text = fs::read_to_string(path).map_err(|_| "The agent could not be read.")?;
    // JSON metadata is bounded by fences. Everything below is editable Markdown.
    let text = text
        .strip_prefix("```json\n")
        .ok_or("The agent format is invalid.")?;
    let (metadata, instructions) = text
        .split_once("\n```\n\n")
        .ok_or("The agent format is invalid.")?;
    let mut agent: Agent =
        serde_json::from_str(metadata).map_err(|_| "The agent settings are invalid.")?;
    if id.is_some_and(|id| agent.id != id) {
        return Err("The agent identifier does not match its folder.".into());
    }
    uuid::Uuid::parse_str(&agent.id).map_err(|_| "The agent identifier is invalid.")?;
    agent.instructions = instructions.trim_end().into();
    migrate(&mut agent)?;
    validate(&agent)?;
    crate::tool_presentation::remember_agent(&agent.id, &agent.name);
    Ok(agent)
}
/// Reads a v1 `schedule` as the cron routine it becomes, and fills the
/// `schedule` a v1 client reads from that routine. A routine an older
/// extraction cut at its name limit gets its full prompt back when the
/// instructions still hold it.
fn migrate(agent: &mut Agent) -> Result<(), String> {
    for routine in &mut agent.routines {
        crate::agent_extract::mend_cut_routine(routine, &agent.instructions);
    }
    let existing = agent
        .routines
        .iter()
        .position(|routine| routine.id == crate::agent_routines::SCHEDULE_ROUTINE_ID);
    match (&agent.schedule, existing) {
        (Some(schedule), None) => {
            let routine = crate::agent_routines::schedule_routine(
                schedule,
                &crate::agent_routines::local_time_zone(),
            )?;
            agent.routines.insert(0, routine);
        }
        (None, Some(index)) => {
            agent.schedule = crate::agent_routines::routine_schedule(&agent.routines[index]);
        }
        _ => {}
    }
    Ok(())
}
/// Applies a client's v1 `schedule` to the `schedule` routine: a schedule
/// that differs from the routine's replaces its trigger. A missing schedule
/// changes nothing, so a client that edits only routines keeps them.
fn apply_schedule(agent: &mut Agent) -> Result<(), String> {
    let id = crate::agent_routines::SCHEDULE_ROUTINE_ID;
    let routine_schedule = agent
        .routines
        .iter()
        .find(|routine| routine.id == id)
        .and_then(crate::agent_routines::routine_schedule);
    let schedule_changed = |schedule: &Option<Schedule>| {
        serde_json::to_value(schedule).ok() != serde_json::to_value(&routine_schedule).ok()
    };
    match agent.schedule.clone() {
        Some(schedule) if schedule_changed(&Some(schedule.clone())) => {
            let tz = agent
                .routines
                .iter()
                .find(|routine| routine.id == id)
                .and_then(|routine| match &routine.trigger {
                    Trigger::Cron(cron) => Some(cron.tz.clone()),
                    _ => None,
                })
                .unwrap_or_else(crate::agent_routines::local_time_zone);
            let mut routine = crate::agent_routines::schedule_routine(&schedule, &tz)?;
            if let Some(existing) = agent.routines.iter_mut().find(|routine| routine.id == id) {
                routine.name = existing.name.clone();
                routine.prompt = existing.prompt.clone();
                *existing = routine;
            } else {
                agent.routines.insert(0, routine);
            }
        }
        _ => {}
    }
    agent.schedule = agent
        .routines
        .iter()
        .find(|routine| routine.id == id)
        .and_then(crate::agent_routines::routine_schedule);
    Ok(())
}
/// Lists every agent. A folder whose `agent.md` does not load is left out
/// and named in `errors`. A list only reads: it renames no folder and saves
/// no state, so it answers on a full or read-only disk. [`mend`] makes the
/// repairs a list would need.
pub fn list(profile: &Path) -> Result<AgentList, String> {
    let mut agents = Vec::new();
    let mut errors = Vec::new();
    let root = crate::memory_files::home(profile)?.join("agents");
    if root.is_symlink() {
        return Err("The agents folder must not be a symbolic link.".into());
    }
    if root.is_dir() {
        // A save renames the folder first, so the scan waits for it and
        // never sees an agent in neither folder.
        let _lock = read_lock(profile)?;
        for entry in fs::read_dir(&root).map_err(|_| "The agents could not be read.")? {
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path().join("agent.md");
            if entry.path().is_symlink() || !path.is_file() {
                continue;
            }
            let folder = entry.file_name().to_string_lossy().into_owned();
            match read_file(&path, None) {
                Ok(agent) => agents.push(agent),
                Err(error) => errors.push(AgentLoadError { folder, error }),
            }
        }
    }
    agents.sort_by(|a, b| a.name.cmp(&b.name));
    errors.sort_by(|a, b| a.folder.cmp(&b.folder));
    let mut state = state(profile)?;
    for agent in &agents {
        ensure_primary(&mut state, &agent.id);
    }
    for agent in &mut agents {
        apply_routine_state(agent, &state);
    }
    Ok(AgentList {
        agents,
        state,
        errors,
    })
}
/// The agents lock for a reader: the lock a save holds, when one exists. A
/// profile no save has touched has no lock file, and a read creates none.
fn read_lock(profile: &Path) -> Result<Option<fs::File>, String> {
    let file = match fs::File::open(profile.join("agents.lock")) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Agent settings are unavailable.".into()),
    };
    file.lock_shared().map_err(|_| "Agent settings are busy.")?;
    Ok(Some(file))
}
/// Makes the repairs a list leaves undone: names each agent's folder after
/// the name in its file, records a folder the name index lacks, and saves
/// each agent's primary thread once. A consistent profile writes nothing.
/// The schedule pass runs it and logs a failure, so a full disk delays a
/// repair and fails no read.
pub fn mend(profile: &Path) -> Result<(), String> {
    let root = crate::memory_files::home(profile)?.join("agents");
    if root.is_symlink() || !root.is_dir() {
        return Ok(());
    }
    let mut ids = Vec::new();
    {
        // Each entry names its folder after the name in its file. A save
        // renames the folder first, so the pass waits for it and never reads
        // the old name back into the folder.
        let _lock = lock(profile)?;
        for entry in fs::read_dir(&root).map_err(|_| "The agents could not be read.")? {
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path().join("agent.md");
            if entry.path().is_symlink() || !path.is_file() {
                continue;
            }
            let Ok(agent) = read_file(&path, None) else {
                continue;
            };
            crate::workspace_names::resolve(
                profile,
                &root,
                &agent.id,
                Some(&agent.name),
                Some(&entry.path()),
            )?;
            ids.push(agent.id);
        }
    }
    let mut pinned = state(profile)?;
    let unpinned = ids.iter().any(|id| {
        !pinned.primary_threads.contains_key(id) && ensure_primary(&mut pinned, id).is_some()
    });
    if unpinned {
        with_state(profile, |state| {
            for id in &ids {
                ensure_primary(state, id);
            }
            Ok(())
        })?;
    }
    Ok(())
}
/// Fills each routine's last fire time, result, poll and next fire time
/// from the runtime's state.
fn apply_routine_state(agent: &mut Agent, state: &State) {
    let routines = state.routines.get(&agent.id);
    for routine in &mut agent.routines {
        let saved = routines.and_then(|routines| routines.get(&routine.id));
        routine.last_fired_at = saved.and_then(|saved| saved.last_fired_at.clone());
        routine.last_result = saved.and_then(|saved| saved.last_result.clone());
        routine.last_poll = saved.and_then(|saved| saved.last_poll.clone());
        routine.next_fire_at = saved.and_then(|saved| saved.next_fire_at);
    }
}
/// Reads one agent with its routines' runtime state.
pub fn read(profile: &Path, id: &str) -> Result<Agent, String> {
    let mut agent = get(profile, id)?;
    apply_routine_state(&mut agent, &state(profile)?);
    Ok(agent)
}
fn validate(agent: &Agent) -> Result<(), String> {
    if agent.label.len() > 120 {
        return Err("Keep the job title under 120 bytes.".into());
    }
    if let Some(avatar) = &agent.avatar {
        if !avatar.is_valid() {
            return Err("Choose a valid agent avatar.".into());
        }
    }
    if let Some(template) = &agent.template {
        let text = serde_json::to_string(template).map_err(|_| "The template is invalid.")?;
        if !template.is_object() || text.len() > 128 * 1024 {
            return Err("The template exceeds 128 KB.".into());
        }
        crate::memory_secret::reject_memory_secret(&text)
            .map_err(|_| "Remove credentials from the template.")?;
    }
    if agent.name.trim().is_empty()
        || agent.name.len() > 100
        || agent.instructions.trim().is_empty()
        || agent.instructions.len() > 64 * 1024
    {
        return Err("Give the agent a name and instructions under 64 KB.".into());
    }
    crate::memory_secret::reject_memory_secret(&agent.instructions)
        .map_err(|_| "Remove credentials from the agent instructions.")?;
    if let Some(schedule) = &agent.schedule {
        validate_schedule(schedule)?;
    }
    validate_v2(agent)
}
const MAX_SKILLS: usize = 32;
const MAX_MESSAGING_AGENTS: usize = 64;
fn validate_v2(agent: &Agent) -> Result<(), String> {
    let short = |text: &str, limit: usize| {
        !text.trim().is_empty() && text.len() <= limit && !text.chars().any(char::is_control)
    };
    if let Some(model) = &agent.model {
        // The same bounds the default model's provider and model take.
        if !short(&model.provider, 128) || !short(&model.id, 512) {
            return Err("Choose a model by its provider and id.".into());
        }
        if model
            .thinking
            .as_deref()
            .is_some_and(|level| crate::engine::thinking_level(level).is_none())
        {
            return Err(
                "Choose a thinking level: off, minimal, low, medium, high, xhigh or max.".into(),
            );
        }
    }
    if let Some(skills) = &agent.skills {
        if skills.len() > MAX_SKILLS || !skills.iter().all(|skill| short(skill, 256)) {
            return Err(format!("Select at most {MAX_SKILLS} installed skills."));
        }
    }
    if let Some(runtime) = &agent.primary_runtime {
        if runtime.is_empty()
            || runtime.len() > 64
            || !runtime
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err("The primary machine is not a runtime id.".into());
        }
    }
    if let Some(AgentMessaging {
        allow: MessagingAllow::Agents(ids),
    }) = &agent.messaging
    {
        if ids.len() > MAX_MESSAGING_AGENTS
            || ids.iter().any(|id| uuid::Uuid::parse_str(id).is_err())
        {
            return Err(format!(
                "List at most {MAX_MESSAGING_AGENTS} agents by id that this agent may message."
            ));
        }
    }
    if let Some(reason) = &agent.paused_reason {
        if !short(reason, 64) {
            return Err("The pause reason is a short word such as absence.".into());
        }
    }
    crate::agent_routines::validate_routines(&agent.routines)
}
fn validate_schedule(schedule: &Schedule) -> Result<(u32, u32), String> {
    let parsed = chrono::NaiveTime::parse_from_str(&schedule.time, "%H:%M")
        .map_err(|_| "Choose a valid schedule time.")?;
    if !["daily", "weekdays", "weekly"].contains(&schedule.cadence.as_str()) || schedule.weekday > 6
    {
        return Err("Choose a valid schedule.".into());
    }
    Ok((parsed.hour(), parsed.minute()))
}
/// The first future local occurrence. A skipped DST time advances to the next day.
pub fn next_after<T: TimeZone>(
    schedule: &Schedule,
    after: chrono::DateTime<T>,
) -> Result<i64, String> {
    let (hour, minute) = validate_schedule(schedule)?;
    for offset in 0..9 {
        let date = after.date_naive() + chrono::Duration::days(offset);
        let day = date.weekday().num_days_from_monday();
        if schedule.cadence == "weekdays" && day > 4
            || schedule.cadence == "weekly" && day != schedule.weekday
        {
            continue;
        }
        let naive = date
            .and_hms_opt(hour, minute, 0)
            .ok_or("Choose a valid schedule time.")?;
        let local = after.timezone().from_local_datetime(&naive);
        if let Some(time) = local
            .clone()
            .earliest()
            .filter(|t| t.timestamp() > after.timestamp())
            .or_else(|| local.latest().filter(|t| t.timestamp() > after.timestamp()))
        {
            return Ok(time.timestamp());
        }
    }
    Err("The next schedule time could not be found.".into())
}
pub fn save(profile: &Path, mut agent: Agent) -> Result<Agent, String> {
    // A new agent starts in the runtime's default agent mode, saved as its
    // own choice.
    if agent.id.is_empty() {
        let rules = agent
            .permissions
            .get_or_insert_with(AgentPermissions::default);
        if rules.mode.is_none() {
            rules.mode = Some(crate::permission_gate::settings::new_agent_mode(profile));
        }
    }
    if let Some(mode) = agent.permissions.as_ref().and_then(|rules| rules.mode) {
        let previous_mode = get(profile, &agent.id)
            .ok()
            .and_then(|previous| previous.permissions)
            .and_then(|rules| rules.mode);
        // A saved agent keeps a mode the settings turned off since; a new
        // choice must be available.
        if previous_mode != Some(mode) {
            if let Some(reason) = crate::permission_gate::settings::load(profile).unavailable(mode)
            {
                return Err(reason.into());
            }
        }
    }
    let previous = (!agent.id.is_empty())
        .then(|| get(profile, &agent.id).ok())
        .flatten();
    let extract = previous
        .as_ref()
        .is_none_or(|previous| previous.instructions.trim() != agent.instructions.trim());
    let mut saved = save_definition(profile, agent, previous.as_ref())?;
    if extract {
        let proposals = crate::agent_extract::propose(
            &saved.instructions,
            &crate::agent_routines::local_time_zone(),
            &crate::agent_extract::installed_tools(),
        );
        if let Some(applied) = crate::agent_extract::apply(profile, &saved.id, proposals)? {
            saved = applied;
        }
    }
    Ok(saved)
}
/// Saves an agent without proposing routines from its instructions.
pub(crate) fn save_definition(
    profile: &Path,
    mut agent: Agent,
    previous: Option<&Agent>,
) -> Result<Agent, String> {
    apply_schedule(&mut agent)?;
    for routine in &mut agent.routines {
        if routine.id.is_empty() {
            routine.id = crate::agent_routines::new_id();
        }
        if let Trigger::Webhook(hook) = &mut routine.trigger {
            if hook.hook_id.is_empty() {
                hook.hook_id = crate::agent_routines::new_id();
            }
            // A client that edits a routine keeps the secret it never saw.
            if hook.secret_sha256.is_none() {
                hook.secret_sha256 = previous
                    .into_iter()
                    .flat_map(|previous| &previous.routines)
                    .find_map(|existing| match &existing.trigger {
                        Trigger::Webhook(old) if old.hook_id == hook.hook_id => {
                            old.secret_sha256.clone()
                        }
                        _ => None,
                    });
            }
        }
        // A cut prompt stays marked until a save changes it. The mark comes
        // from the saved routine, never from the client.
        routine.cut_short = previous
            .into_iter()
            .flat_map(|previous| &previous.routines)
            .any(|saved| {
                saved.id == routine.id && saved.cut_short && saved.prompt == routine.prompt
            });
        // An enabled routine is not a proposal.
        if routine.enabled {
            routine.proposed = false;
        }
        // A picked time stays marked while the routine stays the same
        // proposal. The mark comes from the saved routine, never from the
        // client, so accepting or editing a proposal clears it.
        routine.default_time = routine.proposed
            && previous
                .into_iter()
                .flat_map(|previous| &previous.routines)
                .any(|saved| {
                    saved.id == routine.id && saved.default_time && saved.trigger == routine.trigger
                });
        // The runtime keeps fire times, results and polls; a save never sets them.
        routine.last_fired_at = None;
        routine.last_result = None;
        routine.last_poll = None;
        routine.next_fire_at = None;
    }
    validate(&agent)?;
    let hooks: Vec<&str> = agent
        .routines
        .iter()
        .filter_map(|routine| match &routine.trigger {
            Trigger::Webhook(hook) => Some(hook.hook_id.as_str()),
            _ => None,
        })
        .collect();
    if !hooks.is_empty()
        && list(profile)?.agents.iter().any(|other| {
            other.id != agent.id
                && other.routines.iter().any(|routine| {
                    matches!(&routine.trigger, Trigger::Webhook(hook) if hooks.contains(&hook.hook_id.as_str()))
                })
        })
    {
        return Err("Another agent uses this webhook.".into());
    }
    if let Some(project) = &agent.project_id {
        crate::projects::folder(profile, project)?;
    }
    let _lock = lock(profile)?;
    let creating = agent.id.is_empty();
    if creating {
        agent.id = uuid::Uuid::new_v4().to_string();
    }
    if agent.avatar.is_none() {
        agent.avatar = Some(Avatar {
            style: if creating {
                AVATAR_STYLES[0]
            } else {
                "muniment-v1"
            }
            .into(),
            seed: format!(
                "legacy-{:x}",
                agent
                    .id
                    .encode_utf16()
                    .fold(2166136261_u32, |hash, c| (hash ^ u32::from(c))
                        .wrapping_mul(16777619))
            ),
        });
    }
    // A folder no save recorded yet, such as a hand-written one, becomes
    // this agent's folder instead of a second one.
    let legacy = if creating {
        None
    } else {
        match locate(profile, &agent.id)? {
            Location::Unrecorded(folder, _) => Some(folder),
            _ => None,
        }
    };
    let directory = crate::workspace_names::resolve(
        profile,
        &root(profile)?,
        &agent.id,
        Some(&agent.name),
        legacy.as_deref(),
    )?;
    fs::create_dir_all(&directory).map_err(|_| "The agent folder could not be created.")?;
    let mut metadata = agent.clone();
    metadata.instructions.clear();
    // The `schedule` routine holds the schedule. v1 readers rebuild it.
    metadata.schedule = None;
    let text = format!(
        "```json\n{}\n```\n\n{}\n",
        serde_json::to_string_pretty(&metadata).map_err(|_| "The agent settings are invalid.")?,
        agent.instructions.trim()
    );
    write(&directory.join("agent.md"), text.as_bytes())?;
    let mut state = state(profile)?;
    if let Some(thread) = ensure_primary(&mut state, &agent.id) {
        match &agent.project_id {
            Some(project) => crate::projects::assign(profile, &thread, project)?,
            None => crate::projects::unassign(profile, &thread)?,
        }
    }
    let run = state.runs.entry(agent.id.clone()).or_default();
    run.schedule_key = Some(schedule_key(&agent));
    run.next_run = next_routine_fire(&agent, chrono::Utc::now());
    let saved = state.routines.entry(agent.id.clone()).or_default();
    saved.retain(|id, _| agent.routines.iter().any(|routine| &routine.id == id));
    plan_cron_fires(&agent, saved, chrono::Utc::now());
    persist_state(profile, &state)?;
    apply_routine_state(&mut agent, &state);
    Ok(agent)
}
/// Sets each cron routine's next fire time as the run queue's pass does, so
/// the save and the next list name it at once. A routine turned off has
/// none. A new or changed schedule fires next after `now`, and an unchanged
/// one keeps its due time. A paused agent or one with a primary machine
/// waits for the pass, which knows the machine it runs on.
fn plan_cron_fires(
    agent: &Agent,
    saved: &mut BTreeMap<String, RoutineState>,
    now: chrono::DateTime<chrono::Utc>,
) {
    let here = agent.paused_reason.is_none() && agent.primary_runtime.is_none();
    for routine in &agent.routines {
        let Trigger::Cron(schedule) = &routine.trigger else {
            continue;
        };
        let key = crate::agent_routines::schedule_key(routine);
        let fire = saved.entry(routine.id.clone()).or_default();
        if !routine.enabled {
            fire.next_fire_at = None;
            fire.key = key;
        } else if here && (fire.key != key || fire.next_fire_at.is_none()) {
            fire.next_fire_at = crate::agent_routines::next_cron_after(schedule, now)
                .ok()
                .map(|time| time.timestamp());
            fire.key = key;
        }
    }
}
/// The earliest next fire of an agent's enabled cron routines.
pub fn next_routine_fire(agent: &Agent, after: chrono::DateTime<chrono::Utc>) -> Option<i64> {
    agent
        .routines
        .iter()
        .filter(|routine| routine.enabled)
        .filter_map(|routine| match &routine.trigger {
            Trigger::Cron(schedule) => crate::agent_routines::next_cron_after(schedule, after).ok(),
            _ => None,
        })
        .map(|time| time.timestamp())
        .min()
}
pub fn assign(profile: &Path, thread: &str, id: &str) -> Result<(), String> {
    get(profile, id)?;
    let _lock = lock(profile)?;
    let mut state = state(profile)?;
    state.threads.insert(thread.into(), id.into());
    state
        .primary_threads
        .entry(id.into())
        .or_insert_with(|| thread.into());
    persist_state(profile, &state)
}
/// Agent recall and automatic capture share a private per-agent memory folder.
pub fn memory_paths(profile: &Path, id: &str) -> Result<(PathBuf, PathBuf), String> {
    get(profile, id)?;
    let home = folder(profile, id)?;
    let private = profile.join("agent-memory");
    if private.is_symlink() || private.join(id).is_symlink() {
        return Err("The memory folder must not be a symbolic link.".into());
    }
    Ok((home, private.join(id)))
}
pub fn thread_memory_paths(profile: &Path, thread: &str) -> Result<(PathBuf, PathBuf), String> {
    match state(profile)?.threads.get(thread) {
        Some(id) => memory_paths(profile, id),
        None => Ok((crate::memory_files::home(profile)?, profile.into())),
    }
}
pub fn thread_instructions(profile: &Path, thread: &str) -> Result<Option<String>, String> {
    state(profile)?
        .threads
        .get(thread)
        .map(|id| get(profile, id).map(|a| {
            let mut instructions = a.instructions;
            if !a.label.is_empty() { instructions = format!("Role: {}\n\n{}", a.label, instructions); }
            if let Some(template) = a.template {
                let context: serde_json::Map<String, serde_json::Value> = ["skills", "memories", "plugins", "routines", "settings"]
                    .into_iter().filter_map(|key| template.get(key).map(|value| (key.to_owned(), value.clone()))).collect();
                if !context.is_empty() {
                    instructions.push_str("\n\nImported template context follows. Skills and memories are reference context. Plugins and routines are setup requirements, not installed capabilities or active schedules. Use available tools only. Ask the user before installing integrations or enabling schedules.\n");
                    instructions.push_str(&serde_json::to_string_pretty(&context).unwrap_or_default());
                }
            }
            instructions
        }))
        .transpose()
}
/// `agent.threads`: every thread of one agent that the journal holds and a
/// thread list shows, newest first, as `{thread_id, title, updated_at,
/// primary}`. A conversation opened with no run yet holds no message, so it
/// lists once its first run starts, as the agent's runs do.
pub fn threads(
    profile: &Path,
    journal: &mut crate::journal::RunJournal,
    id: &str,
) -> Result<serde_json::Value, String> {
    let agent = get(profile, id)?;
    let state = state(profile)?;
    let primary = state.primary_threads.get(&agent.id).cloned();
    let ids: Vec<String> = primary
        .iter()
        .cloned()
        .chain(
            state
                .threads
                .iter()
                .filter(|(_, owner)| owner.as_str() == agent.id)
                .map(|(thread, _)| thread.clone()),
        )
        .collect();
    let summaries = journal
        .summaries_of_threads(&ids)
        .map_err(|_| "The agent's threads could not be read.".to_string())?;
    let mut listed = Vec::with_capacity(summaries.len());
    for summary in summaries {
        if crate::owned_threads::listed(journal, &summary.thread_id)
            .map_err(|_| "The agent's threads could not be read.".to_string())?
        {
            listed.push(summary);
        }
    }
    let threads: Vec<serde_json::Value> = listed
        .into_iter()
        .map(|summary| {
            serde_json::json!({
                "primary": primary.as_deref() == Some(summary.thread_id.as_str()),
                "thread_id": summary.thread_id,
                "title": summary.title,
                "updated_at": summary.updated_at,
            })
        })
        .collect();
    Ok(serde_json::json!({"threads": threads}))
}
/// Select an existing conversation once. Routine dispatch never replaces it.
pub fn ensure_primary(state: &mut State, id: &str) -> Option<String> {
    if let Some(thread) = state.primary_threads.get(id) {
        return Some(thread.clone());
    }
    let thread = state
        .runs
        .get(id)
        .and_then(|run| run.thread_id.clone())
        .filter(|thread| state.threads.get(thread).is_some_and(|agent| agent == id))
        .or_else(|| {
            state
                .threads
                .iter()
                .find(|(_, agent)| *agent == id)
                .map(|(thread, _)| thread.clone())
        });
    if let Some(thread) = &thread {
        state.primary_threads.insert(id.into(), thread.clone());
    }
    thread
}
/// Drops a thread the journal no longer holds from every agent, so the
/// agent's next run starts a new primary thread. Answers the agents whose
/// primary thread it was.
pub fn forget_thread(state: &mut State, thread: &str) -> Vec<String> {
    state.threads.remove(thread);
    let agents: Vec<String> = state
        .primary_threads
        .iter()
        .filter(|(_, primary)| primary.as_str() == thread)
        .map(|(agent, _)| agent.clone())
        .collect();
    for agent in &agents {
        state.primary_threads.remove(agent);
    }
    agents
}
/// Runs `action` on the agent state under the agents lock and saves the
/// state only when its bytes changed.
pub fn with_state<T>(
    profile: &Path,
    action: impl FnOnce(&mut State) -> Result<T, String>,
) -> Result<T, String> {
    let _lock = lock(profile)?;
    let mut current = state(profile)?;
    let before = serde_json::to_vec(&current).map_err(|_| "Agent state cannot be saved.")?;
    let result = action(&mut current)?;
    for (id, run) in &current.runs {
        if let Some(run_id) = &run.run_id {
            let history = current.history.entry(id.clone()).or_default();
            if let Some(previous) = history
                .iter_mut()
                .find(|item| item.run_id.as_ref() == Some(run_id))
            {
                *previous = run.clone();
            } else {
                history.push(run.clone());
            }
        }
    }
    for history in current.history.values_mut() {
        let excess = history.len().saturating_sub(MAX_STATE_HISTORY);
        history.drain(..excess);
    }
    // A pass that changes nothing writes nothing, so an idle schedule pass
    // leaves the file and a full disk alone.
    let after = serde_json::to_vec(&current).map_err(|_| "Agent state cannot be saved.")?;
    if after != before {
        write(&profile.join("agent-state.json"), &after)?;
    }
    Ok(result)
}
/// Queues a manual run of an agent: a person asked it to run now. An agent
/// whose manual run still waits to start refuses a second one.
pub fn queue_run(profile: &Path, id: &str) -> Result<(), String> {
    run_now(profile, id, None, Some("desktop")).map(drop)
}
/// The trigger source of a test run that an agent chat starts.
pub const TEST_RUN_SOURCE: &str = "test";

/// The title of the thread an agent run opens: `<name> · test run` for a
/// test run from an agent chat, and `<name> · <date and time>` as `time`
/// shows times for any other run, so the thread names the agent and the
/// run rather than the run's standing prompt.
pub fn run_thread_title(
    name: &str,
    run: &Run,
    now: chrono::DateTime<chrono::Utc>,
    time: &crate::time_display::TimeDisplay,
) -> String {
    let name: String = name.trim().chars().take(60).collect();
    let name = if name.is_empty() {
        "Agent".to_owned()
    } else {
        name
    };
    if run.trigger.kind == TriggerKind::Manual
        && run.trigger.source.as_deref() == Some(TEST_RUN_SOURCE)
    {
        return format!("{name} · test run");
    }
    format!("{name} · {}", time.day_and_time(now))
}

/// Queues a manual run with an optional prompt and answers the run.
pub fn run_now(
    profile: &Path,
    id: &str,
    prompt: Option<&str>,
    source: Option<&str>,
) -> Result<Run, String> {
    get(profile, id)?;
    let prompt = prompt.map(str::trim).filter(|prompt| !prompt.is_empty());
    if prompt.is_some_and(|prompt| prompt.len() > crate::agent_routines::MAX_ROUTINE_PROMPT_BYTES) {
        return Err("Keep the run prompt under 16 KB.".into());
    }
    let store = RunStore::open(profile)?;
    if store.active()?.iter().any(|run| {
        run.agent_id == id
            && run.trigger.kind == TriggerKind::Manual
            && matches!(run.status, RunStatus::Queued | RunStatus::Leased)
    }) {
        return Err("This agent already has a pending run.".into());
    }
    let run = store.enqueue(NewRun {
        agent_id: id.into(),
        thread_id: None,
        trigger: RunTrigger {
            kind: TriggerKind::Manual,
            routine_id: None,
            source: source.map(str::to_owned),
        },
        prompt: prompt
            .unwrap_or(crate::agent_routines::DEFAULT_ROUTINE_PROMPT)
            .to_owned(),
        hop: 0,
        origin_run_id: None,
    })?;
    mirror_run(profile, &run)?;
    Ok(run)
}
/// Records a run in the agent's v1 state, which the desktop's agent list reads.
pub fn mirror_run(profile: &Path, run: &Run) -> Result<(), String> {
    with_state(profile, |state| {
        let entry = state.runs.entry(run.agent_id.clone()).or_default();
        entry.run_id = Some(run.id.clone());
        entry.thread_id = run.thread_id.clone().or(entry.thread_id.take());
        entry.last_run = crate::agent_runs::seconds(&run.created_at);
        entry.status = match run.status {
            RunStatus::Leased | RunStatus::Running => "running",
            RunStatus::WaitingInput => "waiting",
            status => status.as_str(),
        }
        .into();
        entry.error = run.error.clone();
        entry.trigger = Some(run.trigger.kind.as_str().into());
        if let Some(routine_id) = &run.trigger.routine_id {
            let saved = state
                .routines
                .entry(run.agent_id.clone())
                .or_default()
                .entry(routine_id.clone())
                .or_default();
            saved.last_result = Some(RoutineResult {
                run_id: run.id.clone(),
                status: run.status.as_str().into(),
                silent: run.silent,
            });
        }
        Ok(())
    })
}
pub fn schedule_key(agent: &Agent) -> String {
    serde_json::to_string(&agent.schedule).expect("schedule serializes")
}
pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
pub fn next(schedule: &Schedule) -> Result<i64, String> {
    next_after(schedule, chrono::Local::now())
}

pub fn update_run(profile: &Path, id: &str, run: RunState) -> Result<(), String> {
    let _lock = lock(profile)?;
    let mut state = state(profile)?;
    state.runs.insert(id.into(), run);
    persist_state(profile, &state)
}
/// Removing an agent keeps its memory, the user's files in its folder and
/// existing thread history.
pub fn delete(profile: &Path, id: &str) -> Result<(), String> {
    delete_agent(profile, id, false).map(drop)
}
/// What a delete left in place.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Deleted {
    /// The agent's folder, kept because it holds the user's files.
    pub folder_kept: Option<PathBuf>,
}
/// The folder Muniment keeps its own files in, inside an agent's folder.
const INTERNAL_FOLDER: &str = ".muniment";
/// The folder at an agent's top that the workspace onboarding writes its
/// memory scaffold in.
const SCAFFOLD_FOLDER: &str = "memory";
/// Whether `folder` holds only what Muniment and the system write there: the
/// `.muniment` folder at its top, the onboarding's memory scaffold files with
/// the bytes it wrote, Finder's `.DS_Store` and empty folders. A link, a
/// scaffold file the user changed or any other file is the user's.
fn holds_only_internals(folder: &Path, top: bool) -> bool {
    holds_only(folder, top, false)
}
fn is_unchanged_scaffold_file(path: &Path, name: &std::ffi::OsStr) -> bool {
    muniment_attach::WORKSPACE_MEMORY_SCAFFOLD
        .iter()
        .any(|(scaffold, contents)| {
            name == *scaffold
                && fs::symlink_metadata(path)
                    .is_ok_and(|metadata| metadata.len() == contents.len() as u64)
                && fs::read(path).is_ok_and(|bytes| bytes == *contents)
        })
}
fn holds_only(folder: &Path, top: bool, scaffold: bool) -> bool {
    let Ok(entries) = fs::read_dir(folder) else {
        return false;
    };
    entries.into_iter().all(|entry| {
        let Ok(entry) = entry else {
            return false;
        };
        let Ok(kind) = entry.file_type() else {
            return false;
        };
        let name = entry.file_name();
        if kind.is_symlink() {
            false
        } else if kind.is_dir() {
            (top && name == INTERNAL_FOLDER)
                || holds_only(&entry.path(), false, top && name == SCAFFOLD_FOLDER)
        } else {
            kind.is_file()
                && (name == ".DS_Store"
                    || (scaffold && is_unchanged_scaffold_file(&entry.path(), &name)))
        }
    })
}
/// Removes an agent's or a thread's folder that holds only Muniment's own
/// files. Answers the folder when it stays.
pub(crate) fn remove_folder(folder: &Path) -> Option<PathBuf> {
    if folder.is_symlink() || !folder.exists() {
        return None;
    }
    if holds_only_internals(folder, true) && fs::remove_dir_all(folder).is_ok() {
        return None;
    }
    Some(folder.to_path_buf())
}
/// Removes each folder under the agents folder that the name index records,
/// that no agent holds, and that holds only Muniment's own files, as an
/// agent delete leaves when the folder held run output. A folder with any
/// other file stays. Answers the names of the removed folders.
pub fn remove_deleted_folders(profile: &Path) -> Result<Vec<String>, String> {
    let Some(home) = crate::home::configured_home(profile).map_err(|e| e.to_string())? else {
        return Ok(Vec::new());
    };
    let root = home.join("agents");
    if !root.is_dir() || root.is_symlink() {
        return Ok(Vec::new());
    }
    let recorded = crate::workspace_names::recorded_names(profile, &root)?;
    // A save writes `agent.md` under this lock, so a folder without one
    // holds no agent while the sweep runs.
    let _lock = lock(profile)?;
    let mut removed = Vec::new();
    for entry in fs::read_dir(&root).map_err(|_| "The agents could not be read.")? {
        let Ok(entry) = entry else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if !entry.file_type().is_ok_and(|kind| kind.is_dir())
            || !recorded.contains(&name)
            || path.join("agent.md").exists()
            || path.join("agent.md").is_symlink()
        {
            continue;
        }
        if remove_folder(&path).is_none() {
            removed.push(name);
        }
    }
    Ok(removed)
}
/// The files an agent's private memory folder holds that its facts rebuild:
/// the search index and the record of chats already read for facts.
const MEMORY_INDEX_FILES: [&str; 5] = [
    "memory-index.sqlite3",
    "memory-index.sqlite3-wal",
    "memory-index.sqlite3-shm",
    "memory-index.sqlite3-journal",
    "memory-extracted.json",
];
/// Removes the index files from the private memory folder of agent `id`
/// under `memory`, then the folder when nothing else stays in it. Deleted
/// facts under it stay restorable. Answers whether the folder went.
fn remove_memory_index(memory: &Path, id: &str) -> bool {
    let folder = memory.join(id);
    if memory.is_symlink() || folder.is_symlink() || !folder.is_dir() {
        return false;
    }
    for name in MEMORY_INDEX_FILES {
        let file = folder.join(name);
        if file.is_file() && !file.is_symlink() {
            let _ = fs::remove_file(file);
        }
    }
    fs::remove_dir(&folder).is_ok()
}
/// Removes the index files, and the folder when nothing else stays, of each
/// private memory folder under `agent-memory` whose agent no longer exists,
/// as a delete that kept the memory or an older version leaves. A folder
/// whose agent's folder or name cannot be read stays. Deleted facts stay
/// restorable, and the shared memory outside `agent-memory` is never read.
/// Answers the agent ids whose folders went.
pub fn remove_deleted_memory(profile: &Path) -> Result<Vec<String>, String> {
    let memory = profile.join("agent-memory");
    if memory.is_symlink() || !memory.is_dir() {
        return Ok(Vec::new());
    }
    // A save writes `agent.md` under this lock, so an agent cannot appear
    // while the sweep runs.
    let _lock = lock(profile)?;
    let mut removed = Vec::new();
    for entry in fs::read_dir(&memory).map_err(|_| "The agent memory could not be read.")? {
        let Ok(entry) = entry else {
            continue;
        };
        let id = entry.file_name().to_string_lossy().into_owned();
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) || uuid::Uuid::parse_str(&id).is_err()
        {
            continue;
        }
        let gone = match locate(profile, &id) {
            Ok(Location::Missing) => true,
            Ok(Location::Recorded(folder) | Location::Unrecorded(folder, _)) => {
                let file = folder.join("agent.md");
                !file.exists() && !file.is_symlink()
            }
            Err(_) => false,
        };
        if gone && remove_memory_index(&memory, &id) {
            removed.push(id);
        }
    }
    Ok(removed)
}
/// Removes an agent, its routines and its run history, and its folder when
/// that holds only Muniment's own files. A folder with the user's files
/// stays, and the answer names it. With `remove_memory`, its private memory
/// folder and the facts its memory holds in its folder go too; the user
/// confirms that first, since the memory cannot be recovered.
pub fn delete_agent(profile: &Path, id: &str, remove_memory: bool) -> Result<Deleted, String> {
    let deleted = remove_agent(profile, id, remove_memory)?;
    forget_in_messaging(profile, id);
    Ok(deleted)
}
fn remove_agent(profile: &Path, id: &str, remove_memory: bool) -> Result<Deleted, String> {
    let _lock = lock(profile)?;
    let store = RunStore::open(profile)?;
    if store.active()?.iter().any(|run| run.agent_id == id) {
        return Err("Finish or cancel the agent run before deleting it.".into());
    }
    let directory = folder(profile, id)?;
    let memory = profile.join("agent-memory");
    if remove_memory && (memory.is_symlink() || memory.join(id).is_symlink()) {
        return Err("The memory folder must not be a symbolic link.".into());
    }
    fs::remove_file(directory.join("agent.md")).map_err(|_| "The agent could not be deleted.")?;
    crate::agent_replays::remove_all(profile, id)?;
    if remove_memory && !directory.is_symlink() {
        // The facts the memory store wrote are the agent's memory, so they
        // go before the folder rule reads the folder.
        crate::memory_files::remove_store_written_in(&directory);
    }
    // The folder's name record goes with the folder, so the index never
    // names a folder that is gone. A kept folder keeps its record.
    let folder_kept =
        crate::workspace_names::remove_recorded(profile, &root(profile)?, id, remove_folder)?;
    if remove_memory {
        match fs::remove_dir_all(memory.join(id)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err("The agent's memory could not be removed.".into())
            }
            _ => {}
        }
    } else {
        // The agent's index goes with it. Its deleted facts stay restorable.
        remove_memory_index(&memory, id);
    }
    store.delete_agent(id)?;
    let mut state = state(profile)?;
    state.threads.retain(|_, agent| agent != id);
    state.runs.remove(id);
    state.primary_threads.remove(id);
    state.history.remove(id);
    state.routines.remove(id);
    state.dismissed_proposals.remove(id);
    state.refined_instructions.remove(id);
    persist_state(profile, &state)?;
    // The chat that created the agent goes with it. The agent is gone by
    // now, so a record that stays is one the user can still delete.
    for creation in crate::creations::list(profile).unwrap_or_default() {
        if creation.kind == "agent" && creation.result_id.as_deref() == Some(id) {
            let _ = crate::creations::remove(profile, &creation.thread_id);
        }
    }
    Ok(Deleted { folder_kept })
}
/// The creation chats of one agent: the threads whose creation record
/// made it, each with the time it became one. Read them before
/// [`delete_agent`], which removes the records.
pub fn creation_threads(profile: &Path, id: &str) -> Vec<crate::creations::CreationChat> {
    crate::creations::list(profile)
        .unwrap_or_default()
        .into_iter()
        .filter(|creation| creation.kind == "agent" && creation.result_id.as_deref() == Some(id))
        .map(|creation| crate::creations::CreationChat {
            started_at: creation.started_at(),
            thread_id: creation.thread_id,
        })
        .collect()
}
/// Whether an agent exists: its folder holds its `agent.md`. An agent this
/// cannot tell exists.
pub fn exists(profile: &Path, id: &str) -> bool {
    match locate(profile, id) {
        Ok(Location::Missing) => false,
        Ok(Location::Recorded(folder) | Location::Unrecorded(folder, _)) => {
            let file = folder.join("agent.md");
            file.exists() || file.is_symlink()
        }
        Err(_) => true,
    }
}
/// Drops a deleted agent from the agents that named it in their messaging
/// list, so a list naming only that agent does not read as empty. An agent
/// that fails to save keeps the stale id, which allows nothing.
fn forget_in_messaging(profile: &Path, id: &str) {
    let Ok(listed) = list(profile) else {
        return;
    };
    for mut agent in listed.agents {
        let Some(AgentMessaging {
            allow: MessagingAllow::Agents(ids),
            ..
        }) = agent.messaging.as_mut()
        else {
            continue;
        };
        if !ids.iter().any(|named| named == id) {
            continue;
        }
        ids.retain(|named| named != id);
        agent.schedule = None;
        let _ = save(profile, agent);
    }
}
/// The agent a thread belongs to, if any.
pub fn thread_agent(profile: &Path, thread: &str) -> Result<Option<Agent>, String> {
    state(profile)?
        .threads
        .get(thread)
        .map(|id| get(profile, id))
        .transpose()
}
/// Whether the runtime `runtime_id` runs an agent's routines: the agent
/// names no primary machine, or names this one, and its routines are not
/// paused.
pub fn runs_routines_here(agent: &Agent, runtime_id: Option<&str>) -> bool {
    agent.paused_reason.is_none()
        && agent
            .primary_runtime
            .as_deref()
            .is_none_or(|primary| runtime_id == Some(primary))
}
/// What one run of an agent's thread launches with.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AgentLaunch {
    pub agent_id: String,
    pub model: Option<AgentModel>,
    pub skills: Vec<String>,
    /// The run's permission gate. Every agent run loads it.
    pub permissions: Option<crate::permission_gate::RunPermissions>,
    /// The agent started the run itself: a routine, another agent's message
    /// or Run now. Such a run answers with the agent's model, not the model
    /// a user picked in the thread.
    pub agent_run: bool,
}
/// The launch settings of the agent that owns `thread`. `trigger` is what
/// started the run, when the run queue started it.
pub fn thread_launch(
    profile: &Path,
    thread: &str,
    trigger: Option<&RunTrigger>,
) -> Result<Option<AgentLaunch>, String> {
    let Some(agent) = thread_agent(profile, thread)? else {
        return Ok(None);
    };
    let rules = agent.permissions.clone().unwrap_or_default();
    let settings = crate::permission_gate::settings::load(profile);
    let permissions = crate::permission_gate::RunPermissions {
        mode: settings.effective(rules.mode.unwrap_or(settings.default_agent_mode)),
        event_triggered: trigger.is_some_and(|trigger| trigger.kind.event_triggered()),
        bypass_enabled: settings.bypass_enabled,
        ..Default::default()
    };
    Ok(Some(AgentLaunch {
        agent_id: agent.id,
        model: agent.model,
        skills: agent.skills.unwrap_or_default(),
        permissions: Some(permissions),
        agent_run: trigger.is_some_and(RunTrigger::agent_started),
    }))
}
/// Adds or replaces one routine. An empty id adds a routine with a new id.
pub fn save_routine(profile: &Path, agent_id: &str, routine: Routine) -> Result<Routine, String> {
    crate::agent_routines::validate_routine(&routine)?;
    let mut agent = get(profile, agent_id)?;
    let id = if routine.id.is_empty() {
        crate::agent_routines::new_id()
    } else {
        routine.id.clone()
    };
    let routine = Routine {
        id: id.clone(),
        ..routine
    };
    match agent.routines.iter_mut().find(|existing| existing.id == id) {
        Some(existing) => *existing = routine,
        None => agent.routines.push(routine),
    }
    // A routine edit wins over the v1 schedule the read filled in.
    agent.schedule = None;
    let agent = save(profile, agent)?;
    agent
        .routines
        .into_iter()
        .find(|routine| routine.id == id)
        .ok_or_else(|| "The routine could not be saved.".into())
}
/// Removes one routine.
pub fn delete_routine(profile: &Path, agent_id: &str, routine_id: &str) -> Result<(), String> {
    let mut agent = get(profile, agent_id)?;
    let before = agent.routines.len();
    agent.routines.retain(|routine| routine.id != routine_id);
    if agent.routines.len() == before {
        return Err("The routine was not found.".into());
    }
    agent.schedule = None;
    save(profile, agent).map(drop)
}
/// Accepts a routine proposal: the routine is turned on and is a proposal no
/// more. A webhook routine still needs a secret before deliveries count.
pub fn accept_proposal(
    profile: &Path,
    agent_id: &str,
    routine_id: &str,
) -> Result<Routine, String> {
    let mut agent = get(profile, agent_id)?;
    let routine = agent
        .routines
        .iter_mut()
        .find(|routine| routine.id == routine_id && routine.proposed)
        .ok_or("The routine proposal was not found.")?;
    routine.proposed = false;
    routine.enabled = true;
    let routine = routine.clone();
    save_routine(profile, agent_id, routine)
}
/// Dismisses a routine proposal. A later extraction does not propose it again.
pub fn dismiss_proposal(profile: &Path, agent_id: &str, routine_id: &str) -> Result<(), String> {
    let agent = get(profile, agent_id)?;
    if !agent
        .routines
        .iter()
        .any(|routine| routine.id == routine_id && routine.proposed)
    {
        return Err("The routine proposal was not found.".into());
    }
    with_state(profile, |state| {
        let dismissed = state
            .dismissed_proposals
            .entry(agent_id.to_owned())
            .or_default();
        if !dismissed.iter().any(|id| id == routine_id) {
            dismissed.push(routine_id.to_owned());
        }
        // The list stays bounded; the oldest dismissals go first.
        let excess = dismissed.len().saturating_sub(200);
        dismissed.drain(..excess);
        Ok(())
    })?;
    delete_routine(profile, agent_id, routine_id)
}
/// One agent's routines with their runtime state.
pub fn routines(profile: &Path, agent_id: &str) -> Result<Vec<Routine>, String> {
    read(profile, agent_id).map(|agent| agent.routines)
}

/// Whether the swarm offers to move `agent` here: its primary is another
/// member that has been away a day or more.
fn move_suggested(swarm: Option<&crate::swarm::Swarm>, agent: &Agent) -> bool {
    match (swarm, agent.primary_runtime.as_deref()) {
        (Some(swarm), Some(primary)) => swarm.move_suggested(primary),
        _ => false,
    }
}

/// Answers one agent operation of the attach protocol, named by its wire
/// name such as `agent.list`, and its body.
pub fn operation(
    profile: &Path,
    operation: &str,
    body: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use serde_json::json;
    fn parse<T: serde::de::DeserializeOwned>(body: serde_json::Value) -> Result<T, String> {
        serde_json::from_value(body).map_err(|_| "The request is invalid.".to_string())
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Empty {}
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ById {
        id: String,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Save {
        agent: Agent,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Delete {
        id: String,
        #[serde(default)]
        remove_memory: bool,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct RunNow {
        id: String,
        #[serde(default)]
        prompt: Option<String>,
        /// A test run from an agent chat, which titles its thread so.
        #[serde(default)]
        test: bool,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Runs {
        #[serde(default)]
        agent_id: Option<String>,
        #[serde(default)]
        limit: Option<usize>,
        #[serde(default)]
        cursor: Option<String>,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ForAgent {
        agent_id: String,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct SaveRoutine {
        agent_id: String,
        routine: Routine,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct DeleteRoutine {
        agent_id: String,
        routine_id: String,
    }
    match operation {
        "agent.list" => {
            parse::<Empty>(body)?;
            let listing = list(profile)?;
            // The latest runs are a view of the history. A history that
            // cannot be read, such as on a full disk, fails no list.
            let mut latest = serde_json::Map::new();
            let read = RunStore::read(profile).and_then(|store| {
                let Some(store) = store else {
                    return Ok(());
                };
                for agent in &listing.agents {
                    if let Some(run) = store.latest(&agent.id)? {
                        latest.insert(agent.id.clone(), json!(run));
                    }
                }
                Ok(())
            });
            if let Err(error) = read {
                crate::runtime_eprintln!("agent.list: run history unread error={error}");
            }
            let ids: Vec<String> = listing
                .agents
                .iter()
                .map(|agent| agent.id.clone())
                .collect();
            let swarm = crate::swarm::Swarm::for_profile(profile);
            let mut activities = crate::agent_runs::activities(profile, &ids, &|run_id| {
                swarm
                    .as_ref()
                    .and_then(|swarm| swarm.approval_summary(run_id))
            })?;
            let agents: Vec<serde_json::Value> = listing
                .agents
                .iter()
                .map(|agent| {
                    let mut value = json!(agent);
                    value["activity"] = json!(activities.remove(&agent.id));
                    value["move_suggested"] = json!(move_suggested(swarm.as_ref(), agent));
                    value
                })
                .collect();
            Ok(json!({
                "agents": agents,
                "primary_threads": listing.state.primary_threads,
                "latest_runs": latest,
                "errors": listing.errors,
            }))
        }
        "agent.tool_catalog" => {
            parse::<Empty>(body)?;
            // A runtime's profile is the state root a launch reads extensions from.
            Ok(crate::extend::tool_catalog(profile))
        }
        "agent.read" => {
            let request: ById = parse(body)?;
            let agent = read(profile, &request.id)?;
            let primary = state(profile)?.primary_threads.get(&agent.id).cloned();
            let swarm = crate::swarm::Swarm::for_profile(profile);
            Ok(json!({
                "agent": agent,
                "primary_thread_id": primary,
                "move_suggested": move_suggested(swarm.as_ref(), &agent),
            }))
        }
        "agent.save" => {
            let request: Save = parse(body)?;
            Ok(json!({"agent": save(profile, request.agent)?}))
        }
        "agent.delete" => {
            let request: Delete = parse(body)?;
            let deleted = delete_agent(profile, &request.id, request.remove_memory)?;
            Ok(match deleted.folder_kept {
                Some(folder) => json!({
                    "folder_kept": folder,
                    "message": "The agent's folder holds your files, so it stays.",
                }),
                None => json!({}),
            })
        }
        "agent.run_now" => {
            let request: RunNow = parse(body)?;
            let run = run_now(
                profile,
                &request.id,
                request.prompt.as_deref(),
                Some(if request.test {
                    TEST_RUN_SOURCE
                } else {
                    "client"
                }),
            )?;
            Ok(json!({"run": run}))
        }
        "agent.runs" => {
            let request: Runs = parse(body)?;
            let page = RunStore::open(profile)?.page(
                request.agent_id.as_deref(),
                request.limit.unwrap_or(50),
                request.cursor.as_deref(),
            )?;
            Ok(json!(page))
        }
        "agent.replays" => {
            let request: ById = parse(body)?;
            crate::agent_replays::list(profile, &request.id)
        }
        "agent.replay" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Replay {
                id: String,
                replay_id: String,
            }
            let request: Replay = parse(body)?;
            crate::agent_replays::replay(profile, &request.id, &request.replay_id)
        }
        "routine.list" => {
            let request: ForAgent = parse(body)?;
            Ok(json!({"routines": routines(profile, &request.agent_id)?}))
        }
        "routine.save" => {
            let request: SaveRoutine = parse(body)?;
            Ok(json!({"routine": save_routine(profile, &request.agent_id, request.routine)?}))
        }
        "routine.delete" => {
            let request: DeleteRoutine = parse(body)?;
            delete_routine(profile, &request.agent_id, &request.routine_id)?;
            Ok(json!({}))
        }
        "agent.routine_proposals" => {
            let request: ForAgent = parse(body)?;
            let proposals: Vec<Routine> = routines(profile, &request.agent_id)?
                .into_iter()
                .filter(|routine| routine.proposed)
                .collect();
            Ok(json!({"proposals": proposals}))
        }
        "agent.routine_proposal_accept" => {
            let request: DeleteRoutine = parse(body)?;
            Ok(
                json!({"routine": accept_proposal(profile, &request.agent_id, &request.routine_id)?}),
            )
        }
        "agent.routine_proposal_dismiss" => {
            let request: DeleteRoutine = parse(body)?;
            dismiss_proposal(profile, &request.agent_id, &request.routine_id)?;
            Ok(json!({}))
        }
        "agent.import" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Import {
                source: String,
            }
            let request: Import = parse(body)?;
            Ok(json!(crate::agent_templates::import(&request.source)?))
        }
        "agent.import_commit" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Commit {
                agent: Agent,
                #[serde(default)]
                memories: Vec<crate::agent_templates::ImportedMemory>,
            }
            let request: Commit = parse(body)?;
            let agent = crate::agent_templates::commit(profile, request.agent, &request.memories)?;
            Ok(json!({"agent": agent}))
        }
        "agent.export" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Export {
                id: String,
                format: String,
            }
            let request: Export = parse(body)?;
            crate::agent_templates::export(
                profile,
                &request.id,
                &request.format,
                &crate::agent_templates::Installed::local(),
            )
        }
        "permission.modes" | "permission.settings" | "permission.settings_save" => {
            crate::permission_gate::settings::operation(profile, operation, body)
        }
        "settings.scope" => crate::settings_scope::operation(body),
        _ => Err("The agent operation is unknown.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instructions_in_prose_save_and_a_credential_does_not() {
        let mut agent = Agent {
            name: "Weekly digest".into(),
            instructions: "Highlight key developments, accomplishments, and blockers. \
                Ask for a password: never store it. Keep the token: placeholder until the owner adds one."
                .into(),
            ..Agent::default()
        };
        assert_eq!(validate(&agent), Ok(()));

        agent.instructions = "Post the digest with api_key=Zx8qL2mN4vB7cR1tY6wP.".into();
        assert_eq!(
            validate(&agent),
            Err("Remove credentials from the agent instructions.".into())
        );
    }

    #[test]
    fn write_diagnostics_report_steps_and_codes_without_private_text() {
        for step in ["create_temp", "write_temp", "sync_temp", "replace_file"] {
            let error = std::io::Error::from_raw_os_error(32);
            assert_eq!(
                write_error(step, error),
                format!(
                    "The agent save failed at {step} (OS error 32, kind {:?}).",
                    std::io::Error::from_raw_os_error(32).kind()
                )
            );
            let error =
                std::io::Error::other("C:\\private\\agent.md contains private instructions");
            assert_eq!(
                write_error(step, error),
                format!("The agent save failed at {step} (OS error unavailable, kind Other).")
            );
        }
    }

    #[test]
    fn write_failures_report_the_step_and_remove_only_the_temporary_file() {
        let root = crate::state_root::test_root().join(uuid::Uuid::new_v4().to_string());
        let path = root.join("private-agent.md");
        let error = write(&path, b"private instructions").unwrap_err();
        assert!(error.starts_with("The agent save failed at create_temp (OS error "));
        assert!(!error.contains("unavailable"));
        assert!(!error.contains("private"));
        assert!(!root.exists());

        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("keep"), b"original").unwrap();
        let error = write(&path, b"private instructions").unwrap_err();
        assert!(error.starts_with("The agent save failed at replace_file (OS error "));
        assert!(!error.contains("unavailable"));
        assert!(!error.contains("private"));
        assert_eq!(fs::read(path.join("keep")).unwrap(), b"original");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn write_and_sync_failures_remove_private_temporary_content() {
        use std::io::Write;

        for step in ["write_temp", "sync_temp"] {
            for existing in [false, true] {
                let root = crate::state_root::test_root().join(uuid::Uuid::new_v4().to_string());
                fs::create_dir_all(&root).unwrap();
                let path = root.join("agent.md");
                if existing {
                    write(&path, b"original").unwrap();
                }
                let error = write_with(
                    &path,
                    b"private instructions",
                    |file, content| {
                        #[cfg(windows)]
                        {
                            use std::os::windows::fs::OpenOptionsExt;
                            use windows_sys::Win32::Storage::FileSystem::{
                                FILE_SHARE_READ, FILE_SHARE_WRITE,
                            };

                            // Deny deletion until the writer closes its handle.
                            let temporary = fs::read_dir(&root)
                                .unwrap()
                                .map(|entry| entry.unwrap().path())
                                .find(|entry| entry != &path)
                                .unwrap();
                            *file = fs::OpenOptions::new()
                                .write(true)
                                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                                .open(temporary)?;
                        }
                        if step == "write_temp" {
                            file.write_all(&content[..7])?;
                            return Err(std::io::Error::from_raw_os_error(112));
                        }
                        file.write_all(content)
                    },
                    |_| {
                        assert_eq!(step, "sync_temp");
                        Err(std::io::Error::from_raw_os_error(112))
                    },
                )
                .unwrap_err();
                assert_eq!(
                    error,
                    write_error(step, std::io::Error::from_raw_os_error(112))
                );
                assert_eq!(fs::read_dir(&root).unwrap().count(), usize::from(existing));
                if existing {
                    assert_eq!(fs::read(&path).unwrap(), b"original");
                } else {
                    assert!(!path.exists());
                }
                write(&path, b"retry").unwrap();
                assert_eq!(fs::read(&path).unwrap(), b"retry");
                assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
                fs::remove_dir_all(root).unwrap();
            }
        }
    }

    #[test]
    fn with_state_saves_only_a_changed_state() {
        let profile = crate::state_root::test_root().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&profile).unwrap();
        let path = profile.join("agent-state.json");
        // No state and no change: no file.
        with_state(&profile, |_| Ok(())).unwrap();
        assert!(!path.exists());
        // A state the action leaves alone keeps its bytes. The pretty
        // form differs from a save, so a save would show.
        let mut saved = State::default();
        saved.threads.insert("thread".into(), "agent".into());
        let pretty = serde_json::to_vec_pretty(&saved).unwrap();
        fs::write(&path, &pretty).unwrap();
        with_state(&profile, |state| {
            state.threads.insert("thread".into(), "agent".into());
            Ok(())
        })
        .unwrap();
        assert_eq!(fs::read(&path).unwrap(), pretty);
        // A change saves.
        with_state(&profile, |state| {
            state.threads.insert("other".into(), "agent".into());
            Ok(())
        })
        .unwrap();
        assert_ne!(fs::read(&path).unwrap(), pretty);
        assert_eq!(state(&profile).unwrap().threads.len(), 2);
        fs::remove_dir_all(profile).unwrap();
    }

    #[test]
    fn write_creates_and_replaces_agent_files_without_temporary_files() {
        let root = crate::state_root::test_root().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&root).unwrap();
        for name in [
            "agent.md",
            "agent-state.json",
            "agent.backup.md",
            "agent",
            "agént.md",
        ] {
            let path = root.join(name);
            for content in [b"first".as_slice(), b"replacement", b""] {
                write(&path, content).unwrap();
                assert_eq!(fs::read(&path).unwrap(), content);
            }
            fs::remove_file(path).unwrap();
            assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_replacement_requires_a_closed_temporary_file() {
        use std::io::Write;
        use windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION;

        let root = crate::state_root::test_root().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&root).unwrap();
        let path = root.join("agent-state.json");
        fs::write(&path, b"original").unwrap();
        let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .unwrap();
        file.write_all(b"replacement").unwrap();
        file.sync_all().unwrap();
        let error = crate::home::replace_file(&temporary, &path).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(ERROR_SHARING_VIOLATION as i32));
        assert_eq!(fs::read(&path).unwrap(), b"original");
        drop(file);
        crate::home::replace_file(&temporary, &path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"replacement");
        assert!(!temporary.exists());

        let reader = fs::File::open(&path).unwrap();
        write(&path, b"saved with an open reader").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"saved with an open reader");
        drop(reader);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_locked_target_reports_the_error_and_preserves_the_saved_file() {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

        let root = crate::state_root::test_root().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&root).unwrap();
        let path = root.join("agent.md");
        write(&path, b"original").unwrap();
        let locked = fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(&path)
            .unwrap();
        let error = write(&path, b"replacement").unwrap_err();
        assert!(error.starts_with("The agent save failed at replace_file (OS error 32,"));
        assert_eq!(fs::read(&path).unwrap(), b"original");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        drop(locked);
        write(&path, b"replacement").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"replacement");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_full_disk_reads_as_plain_words_in_an_agent_save() {
        #[cfg(unix)]
        let full = std::io::Error::from_raw_os_error(28);
        #[cfg(windows)]
        let full = std::io::Error::from_raw_os_error(112);
        assert_eq!(
            write_error("create_temp", full),
            "The disk is full. Free some space, then try again."
        );
    }

    /// Every file and folder under `root`, a file with its size, inode and
    /// change time, so a comparison catches a write, a replacement and a
    /// rename. A folder keeps its times out: SQLite opens and removes its
    /// shared-memory file beside a database it reads.
    #[cfg(unix)]
    fn tree(root: &Path) -> Vec<(PathBuf, u64, u64, i64, i64)> {
        use std::os::unix::fs::MetadataExt;
        let mut entries = Vec::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(&directory).unwrap().flatten() {
                let metadata = entry.metadata().unwrap();
                if metadata.is_dir() {
                    pending.push(entry.path());
                    entries.push((entry.path(), 0, metadata.ino(), 0, 0));
                    continue;
                }
                entries.push((
                    entry.path(),
                    metadata.len(),
                    metadata.ino(),
                    metadata.mtime_nsec(),
                    metadata.ctime_nsec(),
                ));
            }
        }
        entries.sort();
        entries
    }

    #[cfg(unix)]
    #[test]
    fn a_list_writes_nothing_and_answers_when_no_write_can_land() {
        use std::os::unix::fs::PermissionsExt;
        let root = crate::state_root::test_root().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&root).unwrap();
        let profile = root.join("private");
        crate::home::confirm_home(&profile, &root.join("muniment")).unwrap();
        let agent = save(
            &profile,
            Agent {
                name: "Scout".into(),
                instructions: "Find sources.".into(),
                ..Default::default()
            },
        )
        .unwrap();
        RunStore::open(&profile).unwrap();
        // A thread with no saved primary: a list that pinned it would save.
        with_state(&profile, |state| {
            state.threads.insert("first-chat".into(), agent.id.clone());
            Ok(())
        })
        .unwrap();
        // The folder name no longer matches the name: a list that renamed
        // the folder would write.
        let file = folder(&profile, &agent.id).unwrap().join("agent.md");
        let text = fs::read_to_string(&file).unwrap();
        fs::write(&file, text.replace("\"Scout\"", "\"Scout Prime\"")).unwrap();

        let before = tree(&root);
        let listing = operation(&profile, "agent.list", serde_json::json!({})).unwrap();
        assert_eq!(tree(&root), before);
        assert_eq!(listing["agents"][0]["name"], "Scout Prime");
        assert_eq!(listing["primary_threads"][&agent.id], "first-chat");

        // With every folder read-only no write can land, as on a full disk,
        // and the list still answers.
        let folders: Vec<PathBuf> = before
            .iter()
            .map(|entry| entry.0.clone())
            .filter(|path| path.is_dir())
            .chain([root.clone()])
            .collect();
        let set_mode = |mode| {
            for path in &folders {
                fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
            }
        };
        set_mode(0o555);
        let listing = operation(&profile, "agent.list", serde_json::json!({}));
        set_mode(0o755);
        let listing = listing.unwrap();
        assert_eq!(listing["agents"][0]["id"], agent.id.as_str());
        assert!(listing["errors"].as_array().unwrap().is_empty());

        // The mend pass makes the repairs the list left undone.
        mend(&profile).unwrap();
        assert!(folder(&profile, &agent.id)
            .unwrap()
            .ends_with(crate::workspace_names::basename("Scout Prime", &agent.id)));
        assert_eq!(
            state(&profile).unwrap().primary_threads[&agent.id],
            "first-chat"
        );
        let mended = tree(&root);
        mend(&profile).unwrap();
        assert_eq!(tree(&root), mended);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_rename_saves_while_another_thread_lists_the_agents() {
        let root = crate::state_root::test_root().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&root).unwrap();
        let profile = root.join("private");
        crate::home::confirm_home(&profile, &root.join("muniment")).unwrap();
        let mut agent = save(
            &profile,
            Agent {
                name: "Scout".into(),
                instructions: "Find sources.".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let lister = {
            let (profile, done) = (profile.clone(), std::sync::Arc::clone(&done));
            std::thread::spawn(move || {
                while !done.load(std::sync::atomic::Ordering::SeqCst) {
                    list(&profile).unwrap();
                }
            })
        };
        for round in 0..100 {
            agent.name = format!("Scout {round}");
            agent = save(&profile, agent).unwrap();
        }
        done.store(true, std::sync::atomic::Ordering::SeqCst);
        lister.join().unwrap();
        assert_eq!(get(&profile, &agent.id).unwrap().name, "Scout 99");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn primary_selection_survives_later_runs_and_project_changes() {
        let root = crate::state_root::test_root().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&root).unwrap();
        let profile = root.join("private");
        crate::home::confirm_home(&profile, &root.join("muniment")).unwrap();
        let mut agent = save(
            &profile,
            Agent {
                name: "Scout".into(),
                instructions: "Find sources.".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assign(&profile, "first-chat", &agent.id).unwrap();
        assign(&profile, "older-run", &agent.id).unwrap();
        with_state(&profile, |state| {
            state.runs.get_mut(&agent.id).unwrap().thread_id = Some("older-run".into());
            Ok(())
        })
        .unwrap();
        assert_eq!(
            list(&profile).unwrap().state.primary_threads[&agent.id],
            "first-chat"
        );
        let project = crate::projects::create(&profile, "Reports").unwrap();
        agent.project_id = Some(project.clone());
        save(&profile, agent.clone()).unwrap();
        assert_eq!(
            crate::projects::list(&profile).unwrap().threads["first-chat"],
            project
        );
        agent.project_id = None;
        save(&profile, agent).unwrap();
        assert!(!crate::projects::list(&profile)
            .unwrap()
            .threads
            .contains_key("first-chat"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn weekday_and_weekly_schedules_use_local_calendar() {
        let friday = chrono::FixedOffset::west_opt(4 * 3600)
            .unwrap()
            .with_ymd_and_hms(2026, 9, 18, 10, 0, 0)
            .unwrap();
        let mut s = Schedule {
            enabled: true,
            cadence: "weekdays".into(),
            time: "09:00".into(),
            weekday: 0,
        };
        assert_eq!(
            next_after(&s, friday).unwrap(),
            friday
                .timezone()
                .with_ymd_and_hms(2026, 9, 21, 9, 0, 0)
                .unwrap()
                .timestamp()
        );
        s.cadence = "weekly".into();
        s.weekday = 4;
        assert_eq!(
            next_after(&s, friday).unwrap(),
            friday
                .timezone()
                .with_ymd_and_hms(2026, 9, 25, 9, 0, 0)
                .unwrap()
                .timestamp()
        );
    }
    #[test]
    fn grok_profile_and_template_context_survive_save_and_enter_the_agent_chat() {
        let root = crate::state_root::test_root().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&root).unwrap();
        let profile = root.join("private");
        crate::home::confirm_home(&profile, &root.join("muniment")).unwrap();
        let agent: Agent = serde_json::from_value(serde_json::json!({
            "name":"Scout", "label":"Researcher", "description":"Find sources.",
            "template":{"skills":[{"name":"Check","description":"Check dates."}],"plugins":["github"],"custom":"keep"},
            "avatar":{"style":"muniment-v1","seed":"happy"}
        })).unwrap();
        let mut saved = save(&profile, agent).unwrap();
        let avatar = saved.avatar.as_ref().unwrap().seed.clone();
        saved.name = "Renamed".into();
        save(&profile, saved.clone()).unwrap();
        let read = get(&profile, &saved.id).unwrap();
        assert_eq!(read.label, "Researcher");
        assert_eq!(read.avatar.unwrap().seed, avatar);
        assert_eq!(read.template.unwrap()["custom"], "keep");
        assign(&profile, "chat", &saved.id).unwrap();
        let context = thread_instructions(&profile, "chat").unwrap().unwrap();
        assert!(context.contains("Role: Researcher"));
        assert!(context.contains("Check dates."));
        assert!(context.contains("not installed capabilities or active schedules"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn definition_and_thread_assignment_survive_edits() {
        let root = crate::state_root::test_root().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&root).unwrap();
        let profile = root.join("private");
        crate::home::confirm_home(&profile, &root.join("muniment")).unwrap();
        let mut agent = save(
            &profile,
            Agent {
                name: "Scout".into(),
                instructions: "Read sources.".into(),
                ..Agent::default()
            },
        )
        .unwrap();
        assign(&profile, "thread-a", &agent.id).unwrap();
        agent.name = "Researcher".into();
        agent.instructions = "Cite sources.".into();
        save(&profile, agent.clone()).unwrap();
        assert_eq!(
            thread_instructions(&profile, "thread-a").unwrap(),
            Some("Cite sources.".into())
        );
        assert_eq!(list(&profile).unwrap().agents.len(), 1);
        fs::write(
            folder(&profile, &agent.id).unwrap().join("reference.txt"),
            "keep",
        )
        .unwrap();
        delete(&profile, &agent.id).unwrap();
        assert_eq!(
            fs::read_to_string(folder(&profile, &agent.id).unwrap().join("reference.txt")).unwrap(),
            "keep"
        );
        assert!(list(&profile).unwrap().agents.is_empty());
        assert!(thread_instructions(&profile, "thread-a").unwrap().is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn new_agents_get_the_muniment_style_and_saved_avatar_styles_keep_their_seed() {
        let (root_dir, profile) = profile();
        let created = save(
            &profile,
            Agent {
                name: "Scout".into(),
                instructions: "Read sources.".into(),
                ..Agent::default()
            },
        )
        .unwrap();
        let avatar = created.avatar.clone().unwrap();
        assert_eq!(avatar.style, "muniment");
        assert!(avatar.seed.starts_with("legacy-"));
        for style in AVATAR_STYLES {
            let mut agent = created.clone();
            agent.avatar = Some(Avatar {
                style: style.into(),
                seed: "scout".into(),
            });
            let saved = save(&profile, agent).unwrap();
            assert_eq!(get(&profile, &saved.id).unwrap().avatar, saved.avatar);
        }
        let mut agent = created.clone();
        agent.avatar = Some(Avatar {
            style: "painted".into(),
            seed: "scout".into(),
        });
        assert_eq!(
            save(&profile, agent).unwrap_err(),
            "Choose a valid agent avatar."
        );
        fs::remove_dir_all(root_dir).unwrap();
    }

    fn profile() -> (PathBuf, PathBuf) {
        let root = crate::state_root::test_root().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&root).unwrap();
        let profile = root.join("private");
        crate::home::confirm_home(&profile, &root.join("muniment")).unwrap();
        (root, profile)
    }

    fn write_header(profile: &Path, id: &str, header: serde_json::Value, instructions: &str) {
        let folder = root(profile).unwrap().join(format!("Agent-{}", &id[..8]));
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("agent.md"),
            format!(
                "```json\n{}\n```\n\n{instructions}\n",
                serde_json::to_string_pretty(&header).unwrap()
            ),
        )
        .unwrap();
    }

    #[test]
    fn a_routine_an_older_extraction_cut_stays_runnable_and_marked_until_edited() {
        let (root_dir, profile) = profile();
        let id = uuid::Uuid::new_v4().to_string();
        let cut = "Search for recent Tauri news and deliver a three-bullet-poin";
        write_header(
            &profile,
            &id,
            serde_json::json!({
                "id": id, "name": "Tauri Morning", "label": "", "instructions": "",
                "avatar": null, "template": null, "projectId": null, "schedule": null,
                "routines": [{
                    "id": "extract-cc22420970616a65", "name": cut, "enabled": true,
                    "trigger": {"kind": "cron", "expr": "0 8 * * 1-5", "tz": "America/New_York"},
                    "prompt": format!("{cut}."), "created_from": "instructions",
                    "last_fired_at": null, "last_result": null
                }]
            }),
            "Search the web for the latest news about Tauri.",
        );
        let agent = list(&profile).unwrap().agents.remove(0);
        let routine = &agent.routines[0];
        assert_eq!(routine.id, "extract-cc22420970616a65");
        assert_eq!(routine.name, "Search for recent Tauri news and deliver a…");
        assert_eq!(routine.prompt, format!("{cut}."));
        assert!(routine.cut_short);
        // A save that keeps the prompt keeps the mark, and a toggle is no edit.
        let saved = save(&profile, agent.clone()).unwrap();
        assert!(saved.routines[0].cut_short);
        let toggled = save_routine(
            &profile,
            &id,
            Routine {
                enabled: false,
                cut_short: false,
                ..saved.routines[0].clone()
            },
        )
        .unwrap();
        assert!(toggled.cut_short);
        assert!(get(&profile, &id).unwrap().routines[0].cut_short);
        // The user's edit of the prompt clears it for good.
        let edited = save_routine(
            &profile,
            &id,
            Routine {
                prompt: "Search for recent Tauri news and deliver a three-bullet-point briefing."
                    .into(),
                ..toggled
            },
        )
        .unwrap();
        assert!(!edited.cut_short);
        let reloaded = get(&profile, &id).unwrap();
        assert!(!reloaded.routines[0].cut_short);
        assert!(
            !fs::read_to_string(folder(&profile, &id).unwrap().join("agent.md"))
                .unwrap()
                .contains("cut_short")
        );
        fs::remove_dir_all(root_dir).unwrap();
    }

    #[test]
    fn a_v1_agent_loads_unchanged_and_its_schedule_reads_as_a_cron_routine() {
        let (root_dir, profile) = profile();
        let id = uuid::Uuid::new_v4().to_string();
        write_header(
            &profile,
            &id,
            serde_json::json!({
                "id": id, "name": "Scout", "label": "", "instructions": "", "avatar": null,
                "template": null, "projectId": null,
                "schedule": {"enabled": true, "cadence": "weekly", "time": "07:45", "weekday": 2}
            }),
            "Find sources.",
        );
        // Listing finds the hand-written folder, as it finds a v1 one.
        let listing = list(&profile).unwrap();
        assert_eq!(listing.agents.len(), 1, "{:?}", listing.errors);
        let agent = get(&profile, &id).unwrap();
        assert_eq!(agent.instructions, "Find sources.");
        assert_eq!(agent.routines.len(), 1);
        let routine = &agent.routines[0];
        assert_eq!(routine.id, crate::agent_routines::SCHEDULE_ROUTINE_ID);
        let Trigger::Cron(cron) = &routine.trigger else {
            panic!("a v1 schedule is a cron routine");
        };
        // v1 Wednesday is 2; cron Wednesday is 3.
        assert_eq!(cron.expr, "45 7 * * 3");
        assert_eq!(cron.tz, crate::agent_routines::local_time_zone());
        assert!(routine.enabled);
        assert_eq!(agent.schedule.as_ref().unwrap().time, "07:45");
        assert!(agent.model.is_none() && agent.permissions.is_none());

        // A save writes the routine and leaves the v1 schedule out of the file.
        let saved = save(&profile, agent).unwrap();
        let text = fs::read_to_string(folder(&profile, &id).unwrap().join("agent.md")).unwrap();
        assert!(text.contains("\"routines\""));
        assert!(text.contains("\"schedule\": null"));
        assert_eq!(saved.schedule.as_ref().unwrap().weekday, 2);

        // A v1 client that edits the schedule edits the routine.
        let mut edited = get(&profile, &id).unwrap();
        edited.schedule.as_mut().unwrap().time = "08:00".into();
        edited.schedule.as_mut().unwrap().enabled = false;
        let edited = save(&profile, edited).unwrap();
        assert_eq!(edited.routines.len(), 1);
        assert!(!edited.routines[0].enabled);
        assert!(
            matches!(&edited.routines[0].trigger, Trigger::Cron(cron) if cron.expr == "0 8 * * 3")
        );
        // A client that names no schedule keeps the routines it sends.
        let mut v2 = get(&profile, &id).unwrap();
        v2.schedule = None;
        assert_eq!(save(&profile, v2).unwrap().routines.len(), 1);
        fs::remove_dir_all(root_dir).unwrap();
    }

    #[test]
    fn v2_fields_round_trip_and_are_checked() {
        let (root_dir, profile) = profile();
        let agent: Agent = serde_json::from_value(serde_json::json!({
            "name": "Closer", "instructions": "Follow up on deals.",
            "model": {"provider": "local", "id": "example-model", "thinking": "high"},
            "tools": {"disabled": ["bash", "mcp__crm__delete"]},
            "skills": ["review", "pack:skills/a/SKILL.md"],
            "permissions": {"mode": "strict",
                "always_allow": [{"request_kind": "path",
                    "resource": {"type": "path", "path": "/work/notes.md", "operation": "read"}}],
                "ask_first": [{"request_kind": "command",
                    "resource": {"type": "command", "arguments": ["bash", "git push"], "working_directory": "/work"}}]},
            "primary_runtime": "018f0000-0000-7000-8000-000000000095",
            "messaging": {"allow": ["018f0000-0000-4000-8000-0000000000a1"]},
            "routines": [{"name": "Hook", "trigger": {"kind": "webhook", "preset": "github"}}],
            "paused_reason": "absence"
        }))
        .unwrap();
        let saved = save(&profile, agent).unwrap();
        let read = read(&profile, &saved.id).unwrap();
        assert_eq!(
            read.model.as_ref().unwrap().thinking.as_deref(),
            Some("high")
        );
        // The old `strict` migrates to `dont_ask`, and a save writes the new id.
        assert_eq!(
            read.permissions.as_ref().unwrap().mode,
            Some(PermissionMode::DontAsk)
        );
        let file =
            fs::read_to_string(folder(&profile, &read.id).unwrap().join("agent.md")).unwrap();
        assert!(file.contains("\"mode\": \"dont_ask\""), "{file}");
        assert_eq!(read.paused_reason.as_deref(), Some("absence"));
        assert_eq!(
            read.messaging,
            Some(AgentMessaging {
                allow: MessagingAllow::Agents(vec!["018f0000-0000-4000-8000-0000000000a1".into()])
            })
        );
        let Trigger::Webhook(hook) = &read.routines[0].trigger else {
            panic!("a webhook routine");
        };
        assert_eq!(hook.hook_id.len(), 32, "a save assigns the hook id");
        assert!(!read.routines[0].id.is_empty());
        let value = serde_json::to_value(&read).unwrap();
        assert_eq!(
            value["primary_runtime"],
            "018f0000-0000-7000-8000-000000000095"
        );
        assert_eq!(value["paused_reason"], "absence");
        // The rule lists of an older profile load and a save drops them.
        assert_eq!(
            value["permissions"],
            serde_json::json!({"mode": "dont_ask"})
        );
        assert!(!file.contains("always_allow") && !file.contains("ask_first"));
        // The tool list of an older agent loads and a save drops it.
        assert!(value.get("tools").is_none() && !file.contains("mcp__crm__delete"));
        let any: AgentMessaging =
            serde_json::from_value(serde_json::json!({"allow": "any"})).unwrap();
        assert_eq!(any.allow, MessagingAllow::Any);
        assert!(
            serde_json::from_value::<AgentMessaging>(serde_json::json!({"allow": "some"})).is_err()
        );
        let invalid = |change: &dyn Fn(&mut Agent)| {
            let mut agent = read.clone();
            change(&mut agent);
            save(&profile, agent).unwrap_err()
        };
        assert!(
            invalid(&|a| a.model.as_mut().unwrap().thinking = Some("extreme".into()))
                .contains("thinking level")
        );
        assert!(invalid(&|a| a.skills = Some(vec!["x".into(); 33])).contains("skills"));
        assert!(invalid(&|a| a.primary_runtime = Some("../etc".into())).contains("runtime id"));
        // A new choice of bypass is refused while bypass is off.
        crate::permission_gate::settings::operation(
            &profile,
            "permission.settings_save",
            serde_json::json!({"bypass_enabled": false}),
        )
        .unwrap();
        assert!(
            invalid(&|a| a.permissions.as_mut().unwrap().mode = Some(PermissionMode::Bypass))
                .contains("Bypass is off")
        );
        assert!(invalid(&|a| {
            a.messaging = Some(AgentMessaging {
                allow: MessagingAllow::Agents(vec!["not-an-id".into()]),
            })
        })
        .contains("message"));
        assert!(
            invalid(&|a| a.routines[0].trigger = Trigger::Cron(CronSchedule {
                expr: "* * * * *".into(),
                tz: "UTC".into()
            }))
            .contains("five minutes")
        );
        fs::remove_dir_all(root_dir).unwrap();
    }

    #[test]
    fn one_bad_agent_file_is_reported_and_the_rest_still_list() {
        let (root_dir, profile) = profile();
        let good = save(
            &profile,
            Agent {
                name: "Good".into(),
                instructions: "Work.".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let broken = root(&profile).unwrap().join("Broken");
        fs::create_dir_all(&broken).unwrap();
        fs::write(broken.join("agent.md"), "```json\n{not json\n```\n\nText\n").unwrap();
        let unfenced = root(&profile).unwrap().join("Unfenced");
        fs::create_dir_all(&unfenced).unwrap();
        fs::write(unfenced.join("agent.md"), "no header\n").unwrap();
        let invalid_id = uuid::Uuid::new_v4().to_string();
        write_header(
            &profile,
            &invalid_id,
            serde_json::json!({"id": invalid_id, "name": "Bad cron", "instructions": "x",
                "projectId": null, "schedule": null,
                "routines": [{"name": "Fast", "trigger": {"kind": "cron", "expr": "* * * * *", "tz": "UTC"}}]}),
            "Runs too often.",
        );
        let listing = list(&profile).unwrap();
        assert_eq!(listing.agents.len(), 1);
        assert_eq!(listing.agents[0].id, good.id);
        assert_eq!(listing.errors.len(), 3, "{:?}", listing.errors);
        assert!(listing
            .errors
            .iter()
            .any(|error| error.folder == "Broken"
                && error.error == "The agent settings are invalid."));
        assert!(listing
            .errors
            .iter()
            .any(|error| error.error.contains("five minutes")));
        fs::remove_dir_all(root_dir).unwrap();
    }

    #[test]
    fn run_history_keeps_the_last_200_runs() {
        let (root_dir, profile) = profile();
        let agent = save(
            &profile,
            Agent {
                name: "Busy".into(),
                instructions: "Work.".into(),
                ..Default::default()
            },
        )
        .unwrap();
        for index in 0..(MAX_STATE_HISTORY + 20) {
            with_state(&profile, |state| {
                let run = state.runs.entry(agent.id.clone()).or_default();
                run.run_id = Some(format!("run-{index:04}"));
                run.status = "completed".into();
                Ok(())
            })
            .unwrap();
        }
        let history = &state(&profile).unwrap().history[&agent.id];
        assert_eq!(history.len(), MAX_STATE_HISTORY);
        assert_eq!(history[0].run_id.as_deref(), Some("run-0020"));
        assert_eq!(
            history.last().unwrap().run_id.as_deref(),
            Some(format!("run-{:04}", MAX_STATE_HISTORY + 19).as_str())
        );
        fs::remove_dir_all(root_dir).unwrap();
    }

    #[test]
    fn deleting_an_agent_drops_it_from_other_messaging_lists() {
        let (root_dir, profile) = profile();
        let make = |name: &str, allow: MessagingAllow| {
            save(
                &profile,
                Agent {
                    name: name.into(),
                    instructions: "Work.".into(),
                    messaging: Some(AgentMessaging { allow }),
                    ..Default::default()
                },
            )
            .unwrap()
        };
        let helper = make("Helper", MessagingAllow::Any);
        let other = make("Other", MessagingAllow::Any);
        let caller = make(
            "Caller",
            MessagingAllow::Agents(vec![helper.id.clone(), other.id.clone()]),
        );
        let open = make("Open", MessagingAllow::Any);
        delete(&profile, &helper.id).unwrap();
        assert_eq!(
            get(&profile, &caller.id).unwrap().messaging.unwrap().allow,
            MessagingAllow::Agents(vec![other.id.clone()])
        );
        assert_eq!(
            get(&profile, &open.id).unwrap().messaging.unwrap().allow,
            MessagingAllow::Any
        );
        fs::remove_dir_all(root_dir).unwrap();
    }

    #[test]
    fn delete_removes_agent_memory_only_when_confirmed() {
        let (root_dir, profile) = profile();
        let make = |name: &str| {
            let agent = save(
                &profile,
                Agent {
                    name: name.into(),
                    instructions: "Work.".into(),
                    ..Default::default()
                },
            )
            .unwrap();
            let (_, private) = memory_paths(&profile, &agent.id).unwrap();
            fs::create_dir_all(&private).unwrap();
            fs::write(private.join("fact.md"), "kept").unwrap();
            (agent, private)
        };
        let (kept, kept_memory) = make("Kept");
        delete(&profile, &kept.id).unwrap();
        assert!(kept_memory.join("fact.md").exists());
        let (removed, removed_memory) = make("Removed");
        let queued = run_now(&profile, &removed.id, None, None).unwrap();
        assert!(delete_agent(&profile, &removed.id, true)
            .unwrap_err()
            .contains("Finish or cancel"));
        let store = RunStore::open(&profile).unwrap();
        let mut cancelled = store.get(&queued.id).unwrap().unwrap();
        cancelled.status = RunStatus::Cancelled;
        store.save(&mut cancelled).unwrap();
        delete_agent(&profile, &removed.id, true).unwrap();
        assert!(!removed_memory.exists());
        assert!(store.latest(&removed.id).unwrap().is_none());
        assert!(list(&profile).unwrap().agents.is_empty());
        fs::remove_dir_all(root_dir).unwrap();
    }

    #[test]
    fn a_deleted_agent_leaves_no_untitled_folder_when_asked_for_again() {
        let (root_dir, profile) = profile();
        let (agent, folder_of_agent) = saved(&profile, "Morning brief");
        let root = folder_of_agent.parent().unwrap().to_path_buf();
        delete_agent(&profile, &agent.id, false).unwrap();
        assert!(!folder_of_agent.exists());
        // A second delete, a replay list or a run's write for the gone
        // agent answers that it is unavailable and makes no folder.
        assert!(delete_agent(&profile, &agent.id, false).is_err());
        assert!(crate::agent_replays::list(&profile, &agent.id).is_err());
        assert_eq!(
            folder(&profile, &agent.id).unwrap_err(),
            "The agent is unavailable."
        );
        let left: Vec<String> = fs::read_dir(&root)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("Untitled-"))
            .collect();
        assert!(left.is_empty(), "{left:?}");
        fs::remove_dir_all(root_dir).unwrap();
    }

    fn saved(profile: &Path, name: &str) -> (Agent, PathBuf) {
        let agent = save(
            profile,
            Agent {
                name: name.into(),
                instructions: "Work.".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let folder = folder(profile, &agent.id).unwrap();
        assert!(folder.join("agent.md").is_file());
        (agent, folder)
    }

    /// Writes what a run leaves in an agent's folder.
    fn run_output(folder: &Path) {
        let task = folder.join(".muniment/tasks/run-1");
        fs::create_dir_all(&task).unwrap();
        fs::write(task.join("output.txt"), "done").unwrap();
        fs::create_dir_all(folder.join("empty/inner")).unwrap();
        fs::write(folder.join(".DS_Store"), "finder").unwrap();
    }

    #[test]
    fn deleting_an_agent_removes_its_folder_when_it_holds_only_muniment_files() {
        let (root_dir, profile) = profile();
        let (agent, folder) = saved(&profile, "Failure Probe");
        run_output(&folder);
        let root = root(&profile).unwrap();
        assert!(crate::workspace_names::recorded(&profile, &root, &agent.id)
            .unwrap()
            .is_some());
        let deleted = delete_agent(&profile, &agent.id, false).unwrap();
        assert_eq!(deleted, Deleted::default());
        assert!(!folder.exists());
        // The delete forgets the folder's name record with the folder.
        assert_eq!(
            crate::workspace_names::recorded(&profile, &root, &agent.id).unwrap(),
            None
        );
        assert!(crate::workspace_names::recorded_names(&profile, &root)
            .unwrap()
            .is_empty());
        fs::remove_dir_all(root_dir).unwrap();
    }

    #[test]
    fn deleting_an_agent_keeps_a_folder_with_the_users_files_and_says_so() {
        let (root_dir, profile) = profile();
        let (agent, folder) = saved(&profile, "Writer");
        run_output(&folder);
        fs::create_dir_all(folder.join("drafts")).unwrap();
        fs::write(folder.join("drafts/chapter.md"), "mine").unwrap();
        let answer = operation(
            &profile,
            "agent.delete",
            serde_json::json!({"id": agent.id}),
        )
        .unwrap();
        assert_eq!(answer["folder_kept"], folder.to_string_lossy().as_ref());
        assert_eq!(
            answer["message"],
            "The agent's folder holds your files, so it stays."
        );
        assert!(!folder.join("agent.md").exists());
        assert_eq!(
            fs::read_to_string(folder.join("drafts/chapter.md")).unwrap(),
            "mine"
        );
        assert!(folder.join(".muniment/tasks/run-1/output.txt").exists());
        assert!(list(&profile).unwrap().agents.is_empty());
        // The kept folder keeps its name record, so the start sweep finds it.
        assert_eq!(
            crate::workspace_names::recorded(&profile, &root(&profile).unwrap(), &agent.id)
                .unwrap()
                .as_deref(),
            folder.file_name().and_then(|name| name.to_str())
        );
        fs::remove_dir_all(root_dir).unwrap();
    }

    #[test]
    fn deleting_an_agent_removes_its_folder_with_the_unchanged_memory_scaffold() {
        let (root_dir, profile) = profile();
        let (agent, folder) = saved(&profile, "Onboarded");
        crate::onboard_companion_workspace(&folder, &folder).unwrap();
        assert!(folder.join("memory/README.md").is_file());
        assert!(folder.join("memory/ONBOARDING.md").is_file());
        run_output(&folder);
        for run in ["run-2", "run-3", "run-4"] {
            fs::create_dir_all(folder.join(".muniment/tasks").join(run)).unwrap();
        }
        let deleted = delete_agent(&profile, &agent.id, true).unwrap();
        assert_eq!(deleted, Deleted::default());
        assert!(!folder.exists());
        fs::remove_dir_all(root_dir).unwrap();
    }

    #[test]
    fn deleting_an_agent_keeps_a_changed_or_added_memory_file() {
        for (name, contents) in [
            ("README.md", "# Memory\n\nMy own notes.\n"),
            ("ONBOARDING.md", ""),
            ("facts.md", "mine"),
        ] {
            let (root_dir, profile) = profile();
            let (agent, folder) = saved(&profile, "Onboarded");
            crate::onboard_companion_workspace(&folder, &folder).unwrap();
            fs::write(folder.join("memory").join(name), contents).unwrap();
            let deleted = delete_agent(&profile, &agent.id, true).unwrap();
            assert_eq!(deleted.folder_kept.as_deref(), Some(folder.as_path()));
            assert_eq!(
                fs::read_to_string(folder.join("memory").join(name)).unwrap(),
                contents
            );
            fs::remove_dir_all(root_dir).unwrap();
        }
    }

    /// Saves a fact into an agent's folder as its memory tool does in a run.
    fn tool_fact(profile: &Path, folder: &Path, agent: &str) -> PathBuf {
        let fact = crate::memory_files::fact_save_in(
            &profile.join("agent-memory").join(agent),
            folder,
            crate::memory_files::Fact {
                id: String::new(),
                title: "Prefers tea".into(),
                content: "The user prefers tea.".into(),
                source: "thread:thread-1 run:run-1".into(),
                trust: crate::memory_files::Trust::Conversation,
                origin: Some(crate::memory_origin::Origin::of_run(
                    profile, "thread-1", "run-1",
                )),
            },
        )
        .unwrap();
        folder.join(format!("memory/facts/{}.md", fact.id))
    }

    #[test]
    fn deleting_an_agent_with_its_memory_removes_the_facts_its_memory_holds() {
        let (root_dir, profile) = profile();
        let (agent, folder) = saved(&profile, "Learner");
        crate::onboard_companion_workspace(&folder, &folder).unwrap();
        let fact = tool_fact(&profile, &folder, &agent.id);
        // A fact the user added in the agent's memory dialog.
        let added = crate::memory_files::fact_save_in(
            &profile.join("agent-memory").join(&agent.id),
            &folder,
            crate::memory_files::Fact {
                id: String::new(),
                title: "Mine".into(),
                content: "My own fact.".into(),
                source: "Me".into(),
                trust: crate::memory_files::Trust::User,
                origin: Some(crate::memory_origin::Origin::user(Some(&agent.id))),
            },
        )
        .unwrap();
        assert!(fact.is_file());
        assert!(folder
            .join(format!("memory/facts/{}.md", added.id))
            .is_file());
        let deleted = delete_agent(&profile, &agent.id, true).unwrap();
        assert_eq!(deleted, Deleted::default());
        assert!(!folder.exists());
        fs::remove_dir_all(root_dir).unwrap();
    }

    #[test]
    fn deleting_an_agent_with_its_memory_keeps_the_users_files_in_memory() {
        for case in ["added", "edited"] {
            let (root_dir, profile) = profile();
            let (agent, folder) = saved(&profile, "Learner");
            let fact = tool_fact(&profile, &folder, &agent.id);
            let kept = match case {
                "added" => {
                    let notes = folder.join("memory/notes.md");
                    fs::write(&notes, "mine").unwrap();
                    notes
                }
                _ => {
                    let text = fs::read_to_string(&fact).unwrap();
                    fs::write(&fact, format!("{text}\nMy edit.")).unwrap();
                    fact.clone()
                }
            };
            let before = fs::read(&kept).unwrap();
            let deleted = delete_agent(&profile, &agent.id, true).unwrap();
            assert_eq!(
                deleted.folder_kept.as_deref(),
                Some(folder.as_path()),
                "{case}"
            );
            assert_eq!(fs::read(&kept).unwrap(), before, "{case}");
            if case != "edited" {
                assert!(!fact.exists(), "{case}");
            }
            fs::remove_dir_all(root_dir).unwrap();
        }
    }

    #[test]
    fn deleting_an_agent_and_keeping_its_memory_keeps_the_facts_its_memory_saved() {
        let (root_dir, profile) = profile();
        let (agent, folder) = saved(&profile, "Learner");
        let fact = tool_fact(&profile, &folder, &agent.id);
        let before = fs::read(&fact).unwrap();
        let deleted = delete_agent(&profile, &agent.id, false).unwrap();
        assert_eq!(deleted.folder_kept.as_deref(), Some(folder.as_path()));
        assert_eq!(fs::read(&fact).unwrap(), before);
        fs::remove_dir_all(root_dir).unwrap();
    }

    #[test]
    fn a_scaffold_named_file_outside_the_memory_folder_is_the_users() {
        let (root_dir, profile) = profile();
        let (agent, folder) = saved(&profile, "Readme");
        fs::write(
            folder.join("README.md"),
            muniment_attach::WORKSPACE_MEMORY_SCAFFOLD[0].1,
        )
        .unwrap();
        let deleted = delete_agent(&profile, &agent.id, false).unwrap();
        assert_eq!(deleted.folder_kept.as_deref(), Some(folder.as_path()));
        assert!(folder.join("README.md").is_file());
        fs::remove_dir_all(root_dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn deleting_an_agent_keeps_a_folder_that_holds_a_link() {
        let (root_dir, profile) = profile();
        let (agent, folder) = saved(&profile, "Linker");
        let outside = root_dir.join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("keep.txt"), "mine").unwrap();
        std::os::unix::fs::symlink(&outside, folder.join("link")).unwrap();
        let deleted = delete_agent(&profile, &agent.id, false).unwrap();
        assert_eq!(deleted.folder_kept.as_deref(), Some(folder.as_path()));
        assert!(folder.join("link").is_symlink());
        assert!(outside.join("keep.txt").exists());
        fs::remove_dir_all(root_dir).unwrap();
    }

    #[test]
    fn the_start_sweep_removes_only_folders_deleted_agents_left_with_muniment_files() {
        let (root_dir, profile) = profile();
        // A live agent whose folder holds only run output.
        let (_, live) = saved(&profile, "Live");
        run_output(&live);
        // Folders a delete left after it removed only `agent.md`.
        let (_, left) = saved(&profile, "Failure Probe");
        run_output(&left);
        fs::remove_file(left.join("agent.md")).unwrap();
        let (_, mine) = saved(&profile, "Kept");
        run_output(&mine);
        fs::write(mine.join("notes.md"), "mine").unwrap();
        fs::remove_file(mine.join("agent.md")).unwrap();
        // A folder Muniment never named.
        let unnamed = root(&profile).unwrap().join("Empty-12345678");
        fs::create_dir_all(unnamed.join(".muniment")).unwrap();
        let removed = remove_deleted_folders(&profile).unwrap();
        assert_eq!(
            removed,
            [left.file_name().unwrap().to_string_lossy().into_owned()]
        );
        assert!(!left.exists());
        assert!(live.join("agent.md").is_file());
        assert!(live.join(".muniment/tasks/run-1/output.txt").exists());
        assert_eq!(fs::read_to_string(mine.join("notes.md")).unwrap(), "mine");
        assert!(unnamed.is_dir());
        assert_eq!(list(&profile).unwrap().agents.len(), 1);
        assert!(remove_deleted_folders(&profile).unwrap().is_empty());
        fs::remove_dir_all(root_dir).unwrap();
    }

    /// Writes the index files and, with `trash`, one deleted fact into an
    /// agent's private memory folder.
    fn private_memory(profile: &Path, id: &str, trash: bool) -> PathBuf {
        let folder = profile.join("agent-memory").join(id);
        fs::create_dir_all(&folder).unwrap();
        for name in [
            "memory-index.sqlite3",
            "memory-index.sqlite3-wal",
            "memory-extracted.json",
        ] {
            fs::write(folder.join(name), "index").unwrap();
        }
        if trash {
            fs::create_dir_all(folder.join("memory-trash")).unwrap();
            fs::write(folder.join("memory-trash/fact-1.md"), "deleted fact").unwrap();
        }
        folder
    }

    #[test]
    fn deleting_an_agent_and_keeping_its_memory_removes_its_index_and_keeps_deleted_facts() {
        let (root_dir, profile) = profile();
        let (plain, _) = saved(&profile, "Plain");
        let plain_memory = private_memory(&profile, &plain.id, false);
        delete_agent(&profile, &plain.id, false).unwrap();
        assert!(!plain_memory.exists());
        let (kept, _) = saved(&profile, "Kept");
        let kept_memory = private_memory(&profile, &kept.id, true);
        delete_agent(&profile, &kept.id, false).unwrap();
        assert!(!kept_memory.join("memory-index.sqlite3").exists());
        assert!(!kept_memory.join("memory-extracted.json").exists());
        assert_eq!(
            fs::read_to_string(kept_memory.join("memory-trash/fact-1.md")).unwrap(),
            "deleted fact"
        );
        fs::remove_dir_all(root_dir).unwrap();
    }

    #[test]
    fn the_start_sweep_removes_only_memory_folders_deleted_agents_left() {
        let (root_dir, profile) = profile();
        let (live, _) = saved(&profile, "Live");
        let live_memory = private_memory(&profile, &live.id, false);
        // A delete that left the folder: the agent's folder and `agent.md` are gone.
        let (gone, gone_folder) = saved(&profile, "Gone");
        fs::remove_dir_all(&gone_folder).unwrap();
        let gone_memory = private_memory(&profile, &gone.id, false);
        // An agent no save ever recorded here.
        let never = uuid::Uuid::new_v4().to_string();
        let never_memory = private_memory(&profile, &never, false);
        // A deleted agent's deleted facts stay restorable.
        let trashed = uuid::Uuid::new_v4().to_string();
        let trashed_memory = private_memory(&profile, &trashed, true);
        // A folder that names no agent, and the shared memory beside it.
        let other = profile.join("agent-memory/shared");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("memory-index.sqlite3"), "index").unwrap();
        fs::write(profile.join("memory-index.sqlite3"), "shared").unwrap();
        let mut removed = remove_deleted_memory(&profile).unwrap();
        removed.sort();
        let mut expected = vec![gone.id.clone(), never.clone()];
        expected.sort();
        assert_eq!(removed, expected);
        assert!(!gone_memory.exists() && !never_memory.exists());
        assert!(live_memory.join("memory-index.sqlite3").is_file());
        assert!(live_memory.join("memory-extracted.json").is_file());
        assert!(!trashed_memory.join("memory-index.sqlite3").exists());
        assert!(trashed_memory.join("memory-trash/fact-1.md").is_file());
        assert!(other.join("memory-index.sqlite3").is_file());
        assert_eq!(
            fs::read_to_string(profile.join("memory-index.sqlite3")).unwrap(),
            "shared"
        );
        assert!(remove_deleted_memory(&profile).unwrap().is_empty());
        fs::remove_dir_all(root_dir).unwrap();
    }

    #[test]
    fn deleting_an_agent_deletes_the_chat_that_created_it() {
        let (root_dir, profile) = profile();
        let agent = save(
            &profile,
            Agent {
                name: "Scout".into(),
                instructions: "Work.".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let creation = |result_id: Option<String>| crate::creations::Creation {
            thread_id: uuid::Uuid::new_v4().to_string(),
            kind: "agent".into(),
            goal: "Find sources".into(),
            output: "A saved agent".into(),
            result_id,
            turns: Vec::new(),
            title: None,
            created_at: None,
        };
        let chat = crate::creations::save(&profile, creation(Some(agent.id.clone()))).unwrap();
        let other = crate::creations::save(&profile, creation(None)).unwrap();
        delete(&profile, &agent.id).unwrap();
        assert!(crate::creations::read(&profile, &chat.thread_id)
            .unwrap()
            .is_none());
        assert!(crate::creations::read(&profile, &other.thread_id)
            .unwrap()
            .is_some());
        fs::remove_dir_all(root_dir).unwrap();
    }

    #[test]
    fn a_run_thread_names_the_agent_and_the_test_or_the_time() {
        let (_root, profile) = profile();
        let agent = save(
            &profile,
            Agent {
                name: "HN AI Desktop".into(),
                instructions: "Work.".into(),
                ..Agent::default()
            },
        )
        .unwrap();
        let now = chrono::Utc
            .with_ymd_and_hms(2026, 10, 18, 13, 5, 0)
            .unwrap();
        let test = operation(
            &profile,
            "agent.run_now",
            serde_json::json!({"id": agent.id, "test": true}),
        )
        .unwrap();
        let test: Run = serde_json::from_value(test["run"].clone()).unwrap();
        let time = |zone: &str, clock| crate::time_display::TimeDisplay {
            zone: Some(zone.into()),
            clock: Some(clock),
            computer_clock: None,
        };
        let new_york = time("America/New_York", crate::time_display::Clock::TwentyFour);
        assert_eq!(
            run_thread_title(&agent.name, &test, now, &new_york),
            "HN AI Desktop · test run"
        );
        let mut scheduled = test.clone();
        scheduled.trigger = RunTrigger {
            kind: TriggerKind::Cron,
            routine_id: Some("schedule".into()),
            source: None,
        };
        assert_eq!(
            run_thread_title(&agent.name, &scheduled, now, &new_york),
            "HN AI Desktop · Oct 18, 09:05"
        );
        let tokyo = time("Asia/Tokyo", crate::time_display::Clock::Twelve);
        assert_eq!(
            run_thread_title(&agent.name, &scheduled, now, &tokyo),
            "HN AI Desktop · Oct 18, 10:05 PM"
        );
    }

    #[test]
    fn run_now_is_labeled_manual_and_refuses_a_second_pending_run() {
        let (root_dir, profile) = profile();
        let agent = save(
            &profile,
            Agent {
                name: "Runner".into(),
                instructions: "Work.".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let run = run_now(&profile, &agent.id, Some("Check now."), Some("client")).unwrap();
        assert_eq!(run.trigger.kind, TriggerKind::Manual);
        assert_eq!(run.trigger.source.as_deref(), Some("client"));
        assert_eq!(run.prompt, "Check now.");
        assert!(queue_run(&profile, &agent.id).is_err());
        let mirrored = &state(&profile).unwrap().runs[&agent.id];
        assert_eq!(mirrored.trigger.as_deref(), Some("manual"));
        assert_eq!(mirrored.status, "queued");
        fs::remove_dir_all(root_dir).unwrap();
    }

    #[test]
    fn a_save_names_each_cron_routines_next_fire_without_waiting_for_the_queue() {
        let (root_dir, profile) = profile();
        let saved = operation(
            &profile,
            "agent.save",
            serde_json::json!({"agent": {"name": "Morning", "instructions": "Work."}}),
        )
        .unwrap();
        let id = saved["agent"]["id"].as_str().unwrap().to_owned();
        let save = |expr: &str, enabled: bool, routine_id: Option<&str>| {
            let mut routine = serde_json::json!({"name": "Daily", "enabled": enabled,
                "trigger": {"kind": "cron", "expr": expr, "tz": "UTC"}});
            if let Some(routine_id) = routine_id {
                routine["id"] = routine_id.into();
            }
            operation(
                &profile,
                "routine.save",
                serde_json::json!({"agent_id": id, "routine": routine}),
            )
            .unwrap()["routine"]
                .clone()
        };
        let listed = || {
            operation(&profile, "agent.list", serde_json::json!({})).unwrap()["agents"][0]
                ["routines"][0]["next_fire_at"]
                .clone()
        };
        let expected = |expr: &str| {
            let schedule = CronSchedule {
                expr: expr.into(),
                tz: "UTC".into(),
            };
            crate::agent_routines::next_cron_after(&schedule, chrono::Utc::now())
                .unwrap()
                .timestamp()
        };
        // The save answer and the list right after it name the next fire.
        let routine = save("0 9 * * *", true, None);
        let routine_id = routine["id"].as_str().unwrap().to_owned();
        let first = routine["next_fire_at"].as_i64().unwrap();
        assert_eq!(first, expected("0 9 * * *"));
        assert_eq!(listed(), first);
        // A save that keeps the schedule keeps its due time.
        save("0 9 * * *", true, Some(&routine_id));
        assert_eq!(listed(), first);
        // A changed schedule fires on its new time.
        let changed = save("30 17 * * *", true, Some(&routine_id));
        assert_eq!(changed["next_fire_at"], expected("30 17 * * *"));
        assert_eq!(listed(), expected("30 17 * * *"));
        // A routine turned off fires never, and on again fires next.
        assert!(save("30 17 * * *", false, Some(&routine_id))["next_fire_at"].is_null());
        assert!(listed().is_null());
        assert_eq!(
            save("30 17 * * *", true, Some(&routine_id))["next_fire_at"],
            expected("30 17 * * *")
        );
        fs::remove_dir_all(root_dir).unwrap();
    }

    #[test]
    fn agent_operations_answer_the_wire_shapes() {
        let (root_dir, profile) = profile();
        let saved = operation(
            &profile,
            "agent.save",
            serde_json::json!({"agent": {"name": "Ops", "instructions": "Work.",
                "permissions": {"mode": "ask"}}}),
        )
        .unwrap();
        let id = saved["agent"]["id"].as_str().unwrap().to_owned();
        let listing = operation(&profile, "agent.list", serde_json::json!({})).unwrap();
        assert_eq!(listing["agents"][0]["id"], id.as_str());
        assert!(listing["errors"].as_array().unwrap().is_empty());
        let routine = operation(
            &profile,
            "routine.save",
            serde_json::json!({"agent_id": id, "routine": {"name": "Morning",
                "trigger": {"kind": "cron", "expr": "0 9 * * *", "tz": "UTC"}}}),
        )
        .unwrap();
        let routine_id = routine["routine"]["id"].as_str().unwrap().to_owned();
        assert_eq!(
            operation(
                &profile,
                "routine.list",
                serde_json::json!({"agent_id": id})
            )
            .unwrap()["routines"][0]["id"],
            routine_id.as_str()
        );
        // The listing carries when each routine fires next, and a save of the
        // listed agent keeps it out of the agent file.
        with_state(&profile, |state| {
            state
                .routines
                .entry(id.clone())
                .or_default()
                .entry(routine_id.clone())
                .or_default()
                .next_fire_at = Some(1_900_000_000);
            Ok(())
        })
        .unwrap();
        let listed = operation(&profile, "agent.list", serde_json::json!({})).unwrap();
        assert_eq!(
            listed["agents"][0]["routines"][0]["next_fire_at"],
            1_900_000_000
        );
        let read = operation(&profile, "agent.read", serde_json::json!({"id": id})).unwrap();
        assert_eq!(read["agent"]["routines"][0]["next_fire_at"], 1_900_000_000);
        operation(
            &profile,
            "agent.save",
            serde_json::json!({"agent": read["agent"].clone()}),
        )
        .unwrap();
        assert!(
            !fs::read_to_string(folder(&profile, &id).unwrap().join("agent.md"))
                .unwrap()
                .contains("next_fire_at")
        );
        let run = operation(&profile, "agent.run_now", serde_json::json!({"id": id})).unwrap();
        assert_eq!(run["run"]["trigger"]["kind"], "manual");
        assert_eq!(run["run"]["status"], "queued");
        let page = operation(
            &profile,
            "agent.runs",
            serde_json::json!({"agent_id": id, "limit": 10}),
        )
        .unwrap();
        assert_eq!(page["runs"][0]["id"], run["run"]["id"]);
        assert_eq!(page["next_cursor"], serde_json::json!(null));
        let read = operation(&profile, "agent.read", serde_json::json!({"id": id})).unwrap();
        assert_eq!(read["agent"]["routines"][0]["name"], "Morning");
        operation(
            &profile,
            "routine.delete",
            serde_json::json!({"agent_id": id, "routine_id": routine_id}),
        )
        .unwrap();
        assert!(operation(
            &profile,
            "agent.read",
            serde_json::json!({"id": id, "extra": 1})
        )
        .unwrap_err()
        .contains("invalid"));
        assert!(operation(&profile, "agent.forget", serde_json::json!({})).is_err());
        let catalog = operation(&profile, "agent.tool_catalog", serde_json::json!({})).unwrap();
        assert!(catalog["builtin"].as_array().unwrap().len() > 5);
        assert!(catalog["mcp"].is_array() && catalog["skills"].is_array());
        assert!(operation(&profile, "agent.tool_catalog", serde_json::json!({"x": 1})).is_err());
        fs::remove_dir_all(root_dir).unwrap();
    }

    #[test]
    fn agent_threads_lists_every_thread_of_the_agent_with_a_run_newest_first() {
        let (root_dir, profile) = profile();
        let agent = save(
            &profile,
            Agent {
                name: "Threads".into(),
                instructions: "Work.".into(),
                ..Agent::default()
            },
        )
        .unwrap();
        let mut journal = crate::journal::RunJournal::open(root_dir.join("runs.sqlite3")).unwrap();
        let provenance = crate::journal::Provenance {
            source: "desktop".into(),
            source_version: "1".into(),
            actor_id: None,
            device_id: None,
            rpc_request_id: None,
            capability_versions: None,
            extra: [("attach_profile".to_owned(), serde_json::json!("owner"))]
                .into_iter()
                .collect(),
        };
        let mut create = |at: &str| {
            journal
                .create_thread("local", at, provenance.clone())
                .unwrap()
        };
        let primary = create("2026-01-01T09:00:00Z");
        let older = create("2026-01-01T08:00:00Z");
        let deleted = create("2026-01-01T10:00:00Z");
        let other = create("2026-01-01T11:00:00Z");
        // A conversation opened with no run holds no message and does not list.
        let empty = create("2026-01-01T12:00:00Z");
        let runs = rusqlite::Connection::open(root_dir.join("runs.sqlite3")).unwrap();
        for (run, thread) in [("run-primary", &primary), ("run-older", &older)] {
            runs.execute(
                "INSERT INTO run_threads(run_id,thread_id,thread_run_ordinal) VALUES(?1,?2,1)",
                [run, thread.as_str()],
            )
            .unwrap();
        }
        let seq = journal.last_thread_seq(&older).unwrap();
        journal
            .append_thread_title_renamed(
                seq,
                &older,
                "Old report",
                "2026-01-01T08:30:00Z",
                &provenance,
            )
            .unwrap();
        let seq = journal.last_thread_seq(&deleted).unwrap();
        journal
            .append_thread_deleted(seq, &deleted, "2026-01-01T10:30:00Z", &provenance)
            .unwrap();
        with_state(&profile, |state| {
            state
                .primary_threads
                .insert(agent.id.clone(), primary.clone());
            for thread in [&primary, &older, &deleted, &empty] {
                state.threads.insert(thread.clone(), agent.id.clone());
            }
            state.threads.insert(other.clone(), "another-agent".into());
            state
                .threads
                .insert("missing-thread".into(), agent.id.clone());
            Ok(())
        })
        .unwrap();
        let listed = threads(&profile, &mut journal, &agent.id).unwrap();
        assert_eq!(
            listed,
            serde_json::json!({"threads": [
                {"thread_id": primary, "title": "Untitled run", "updated_at": "2026-01-01T09:00:00Z", "primary": true},
                {"thread_id": older, "title": "Old report", "updated_at": "2026-01-01T08:30:00Z", "primary": false}
            ]})
        );
        assert!(threads(&profile, &mut journal, "no-such-agent").is_err());
        drop(runs);
        drop(journal);
        fs::remove_dir_all(root_dir).unwrap();
    }

    #[test]
    fn a_picked_time_stays_marked_until_the_proposal_is_accepted_or_edited() {
        let (root_dir, profile) = profile();
        let saved = operation(
            &profile,
            "agent.save",
            serde_json::json!({"agent": {"name": "Keeper",
                "instructions": "On Fridays, back up the notes folder. Post a recap every Monday at 5pm."}}),
        )
        .unwrap();
        let id = saved["agent"]["id"].as_str().unwrap().to_owned();
        let proposals = operation(
            &profile,
            "agent.routine_proposals",
            serde_json::json!({"agent_id": id}),
        )
        .unwrap()["proposals"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(proposals.len(), 2);
        assert_eq!(proposals[0]["default_time"], true);
        assert!(proposals[1].get("default_time").is_none());
        // A save that keeps the proposal keeps the mark, and a client never sets it.
        let mut agent = get(&profile, &id).unwrap();
        agent.routines[1].default_time = true;
        let agent = save(&profile, agent).unwrap();
        assert!(agent.routines[0].default_time);
        assert!(!agent.routines[1].default_time);
        // An edit of the trigger clears it.
        let mut edited = agent.routines[0].clone();
        if let Trigger::Cron(cron) = &mut edited.trigger {
            cron.expr = "0 18 * * 5".into();
        }
        assert!(!save_routine(&profile, &id, edited).unwrap().default_time);
        // Accepting clears it too.
        let agent = save(
            &profile,
            Agent {
                instructions: "On Fridays, back up the notes folder. Be brief.".into(),
                ..get(&profile, &id).unwrap()
            },
        )
        .unwrap();
        let picked = agent
            .routines
            .iter()
            .find(|routine| routine.proposed && routine.default_time)
            .unwrap()
            .id
            .clone();
        let accepted = accept_proposal(&profile, &id, &picked).unwrap();
        assert!(!accepted.default_time);
        assert!(
            !fs::read_to_string(folder(&profile, &id).unwrap().join("agent.md"))
                .unwrap()
                .contains("default_time")
        );
        fs::remove_dir_all(root_dir).unwrap();
    }

    #[test]
    fn routine_proposals_are_listed_accepted_and_dismissed_for_good() {
        let (root_dir, profile) = profile();
        let saved = operation(
            &profile,
            "agent.save",
            serde_json::json!({"agent": {"name": "Scout",
                "instructions": "Check the inbox every morning at 8am. Post a recap every Friday at 5pm."}}),
        )
        .unwrap();
        let id = saved["agent"]["id"].as_str().unwrap().to_owned();
        let proposals = operation(
            &profile,
            "agent.routine_proposals",
            serde_json::json!({"agent_id": id}),
        )
        .unwrap();
        let proposals = proposals["proposals"].as_array().unwrap().clone();
        assert_eq!(proposals.len(), 2);
        assert!(proposals
            .iter()
            .all(|routine| routine["proposed"] == true && routine["enabled"] == false));
        let first = proposals[0]["id"].as_str().unwrap();
        let second = proposals[1]["id"].as_str().unwrap();
        let accepted = operation(
            &profile,
            "agent.routine_proposal_accept",
            serde_json::json!({"agent_id": id, "routine_id": first}),
        )
        .unwrap();
        assert_eq!(accepted["routine"]["enabled"], true);
        assert!(accepted["routine"].get("proposed").is_none());
        operation(
            &profile,
            "agent.routine_proposal_dismiss",
            serde_json::json!({"agent_id": id, "routine_id": second}),
        )
        .unwrap();
        // Saving the same instructions again proposes neither one again.
        let mut agent = get(&profile, &id).unwrap();
        agent.instructions.push_str(" Be brief.");
        let agent = save(&profile, agent).unwrap();
        assert_eq!(agent.routines.len(), 1);
        assert!(agent.routines[0].enabled && !agent.routines[0].proposed);
        assert!(operation(
            &profile,
            "agent.routine_proposal_accept",
            serde_json::json!({"agent_id": id, "routine_id": second}),
        )
        .is_err());
        fs::remove_dir_all(root_dir).unwrap();
    }
}
