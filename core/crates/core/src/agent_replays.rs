//! The failure replay library: the inputs of an agent's failed runs, kept in
//! the agent's folder under `replays/`, so a person can run them again
//! against the agent's current definition.
//!
//! A replay holds the run's prompt, its trigger, the agent definition
//! version and the model reference the run started with. A webhook or MCP
//! payload and an agent message stay inside the prompt's untrusted block, as
//! the run received them. Text the secret scanner flags is removed before the
//! file is written, so a replay never holds a credential.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::agent_runs::{NewRun, Run, RunStore, RunTrigger, REPLAY_SOURCE};
use crate::agents::{self, Agent, AgentModel};

/// The replays each agent keeps, newest first.
pub const MAX_REPLAYS: usize = 20;
const PREVIEW_CHARS: usize = 280;
const REMOVED: &str = "[secret removed]";

/// The saved inputs of one failed run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Replay {
    /// The failed run's id.
    pub replay_id: String,
    pub agent_id: String,
    #[serde(default)]
    pub thread_id: Option<String>,
    pub trigger: RunTrigger,
    #[serde(default)]
    pub hop: u32,
    pub prompt: String,
    #[serde(default)]
    pub error: Option<String>,
    /// The SHA-256 of the agent definition the run started with.
    pub agent_version: String,
    /// The model the agent named. None is the default model.
    #[serde(default)]
    pub model: Option<AgentModel>,
    pub failed_at: String,
    /// Whether text the secret scanner flagged was removed from the prompt.
    #[serde(default)]
    pub redacted: bool,
    /// The runs that replayed these inputs, oldest first.
    #[serde(default)]
    pub replayed_by: Vec<String>,
}

/// The version of an agent definition: the SHA-256 of its saved JSON.
pub fn agent_version(agent: &Agent) -> String {
    let mut definition = agent.clone();
    for routine in &mut definition.routines {
        routine.last_fired_at = None;
        routine.last_result = None;
        routine.last_poll = None;
    }
    hex::encode(Sha256::digest(
        serde_json::to_vec(&definition).unwrap_or_default(),
    ))
}

/// The text with every credential the secret scanner flags removed, and
/// whether it removed any.
pub fn without_secrets(text: &str) -> (String, bool) {
    let scan = crate::assistant_text::scan(text, true);
    let end = scan.withhold_from.unwrap_or(text.len());
    let mut kept = String::with_capacity(text.len());
    let mut at = 0;
    for found in scan.matches.iter().filter(|found| found.range.start < end) {
        kept.push_str(&text[at..found.range.start]);
        kept.push_str(REMOVED);
        at = found.range.end.min(end);
    }
    kept.push_str(&text[at.min(end)..end]);
    if scan.withhold_from.is_some() {
        kept.push_str(REMOVED);
    }
    let removed = !scan.matches.is_empty() || scan.withhold_from.is_some();
    (kept, removed)
}

fn folder(profile: &Path, agent_id: &str) -> Result<PathBuf, String> {
    let folder = agents::folder(profile, agent_id)?.join("replays");
    if folder.is_symlink() {
        return Err("The replays folder must not be a symbolic link.".into());
    }
    Ok(folder)
}

fn file(profile: &Path, agent_id: &str, replay_id: &str) -> Result<PathBuf, String> {
    uuid::Uuid::parse_str(replay_id).map_err(|_| "The replay identifier is invalid.")?;
    Ok(folder(profile, agent_id)?.join(format!("{replay_id}.json")))
}

fn write(path: &Path, replay: &Replay) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(replay).map_err(|_| "The replay is invalid.")?;
    agents::write(path, &bytes)
}

/// Saves a failed run's inputs in its agent's folder and keeps the newest
/// [`MAX_REPLAYS`].
pub fn record(profile: &Path, run: &Run) -> Result<Replay, String> {
    let agent = agents::get(profile, &run.agent_id)?;
    let prompt = if run.prompt.is_empty() {
        RunStore::open(profile)?
            .get(&run.id)?
            .map(|stored| stored.prompt)
            .unwrap_or_default()
    } else {
        run.prompt.clone()
    };
    let (prompt, redacted) = without_secrets(&prompt);
    let error = run.error.as_deref().map(|error| without_secrets(error).0);
    let replay = Replay {
        replay_id: run.id.clone(),
        agent_id: run.agent_id.clone(),
        thread_id: run.thread_id.clone(),
        trigger: run.trigger.clone(),
        hop: run.hop,
        prompt,
        error,
        agent_version: agent_version(&agent),
        model: agent.model.clone(),
        failed_at: run.updated_at.clone(),
        redacted,
        replayed_by: Vec::new(),
    };
    let folder = folder(profile, &run.agent_id)?;
    fs::create_dir_all(&folder).map_err(|_| "The replays folder could not be created.")?;
    write(&file(profile, &run.agent_id, &run.id)?, &replay)?;
    let mut saved = list_files(&folder)?;
    saved.sort();
    let excess = saved.len().saturating_sub(MAX_REPLAYS);
    for old in &saved[..excess] {
        let _ = fs::remove_file(old);
    }
    Ok(replay)
}

/// The replay files of one folder. Replay ids are v7 UUIDs, so their names
/// sort oldest first.
fn list_files(folder: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("The replays could not be read.".into()),
    };
    Ok(entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            !path.is_symlink() && path.extension().and_then(|ext| ext.to_str()) == Some("json")
        })
        .collect())
}

fn read(path: &Path) -> Result<Replay, String> {
    let bytes = fs::read(path).map_err(|_| "The replay was not found.")?;
    serde_json::from_slice(&bytes).map_err(|_| "The replay is invalid.".into())
}

/// One replay as `agent.replays` lists it: the prompt shortened to a preview.
fn listed(replay: Replay, current_version: &str) -> Value {
    let prompt_bytes = replay.prompt.len();
    let mut preview: String = replay.prompt.chars().take(PREVIEW_CHARS).collect();
    if preview.len() < prompt_bytes {
        preview.push('…');
    }
    let mut value = serde_json::to_value(&replay).unwrap_or(Value::Null);
    if let Some(object) = value.as_object_mut() {
        object.remove("prompt");
        object.insert("prompt_preview".into(), Value::from(preview));
        object.insert("prompt_bytes".into(), Value::from(prompt_bytes));
        object.insert(
            "definition_changed".into(),
            Value::Bool(replay.agent_version != current_version),
        );
    }
    value
}

/// `agent.replays`: an agent's saved failed runs, newest first.
pub fn list(profile: &Path, agent_id: &str) -> Result<Value, String> {
    let agent = agents::get(profile, agent_id)?;
    let current = agent_version(&agent);
    let mut files = list_files(&folder(profile, agent_id)?)?;
    files.sort();
    files.reverse();
    let replays: Vec<Value> = files
        .iter()
        .filter_map(|path| read(path).ok())
        .filter(|replay| replay.agent_id == agent_id)
        .map(|replay| listed(replay, &current))
        .collect();
    Ok(json!({"replays": replays, "agent_version": current}))
}

/// `agent.replay`: queues the saved inputs again as a run of the agent's
/// current definition. The new run names the failed run as its origin and
/// keeps its trigger kind, so an event's payload still runs behind the gate
/// an event's run gets.
pub fn replay(profile: &Path, agent_id: &str, replay_id: &str) -> Result<Value, String> {
    let agent = agents::get(profile, agent_id)?;
    let path = file(profile, agent_id, replay_id)?;
    let mut replay = read(&path)?;
    if replay.agent_id != agent_id {
        return Err("The replay was not found.".into());
    }
    let run = RunStore::open(profile)?.enqueue(NewRun {
        agent_id: agent_id.to_owned(),
        thread_id: None,
        trigger: RunTrigger {
            kind: replay.trigger.kind,
            routine_id: replay.trigger.routine_id.clone(),
            source: Some(REPLAY_SOURCE.into()),
        },
        prompt: replay.prompt.clone(),
        hop: replay.hop,
        origin_run_id: Some(replay.replay_id.clone()),
    })?;
    agents::mirror_run(profile, &run)?;
    replay.replayed_by.push(run.id.clone());
    write(&path, &replay)?;
    Ok(json!({
        "run": run,
        "replay": listed(replay, &agent_version(&agent)),
    }))
}

/// Removes an agent's replays, as deleting the agent does.
pub fn remove_all(profile: &Path, agent_id: &str) -> Result<(), String> {
    match fs::remove_dir_all(folder(profile, agent_id)?) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            Err("The agent's replays could not be removed.".into())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_never_reach_a_replay() {
        let (kept, removed) = without_secrets(
            "Check the build.\nkey sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789 then report.",
        );
        assert!(removed);
        assert!(!kept.contains("sk-ant-api03"), "{kept}");
        assert!(kept.contains(REMOVED));
        assert!(kept.starts_with("Check the build."));
        let (plain, removed) =
            without_secrets("<webhook_payload>\n{\"ok\":true}\n</webhook_payload>");
        assert!(!removed);
        assert_eq!(
            plain,
            "<webhook_payload>\n{\"ok\":true}\n</webhook_payload>"
        );
    }

    #[test]
    fn the_agent_version_ignores_runtime_filled_routine_state() {
        let mut agent = Agent {
            name: "Scout".into(),
            instructions: "Look around.".into(),
            ..Agent::default()
        };
        let version = agent_version(&agent);
        assert_eq!(version.len(), 64);
        agent.instructions = "Look closer.".into();
        assert_ne!(agent_version(&agent), version);
    }
}
