//! Moving an agent to another primary runtime.
//!
//! The old primary raises the agent's epoch and names the new primary
//! before anything else, so it fires no routine after that. It hands over
//! the definition, the memory folder, the routine state and the clocks of
//! the agent's threads; the new primary takes over once its replicas hold
//! every event those clocks name. When the old primary is offline, the
//! target may force the move from the directory's copy of the definition;
//! the old primary steps down when it sees the higher epoch.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{directory, link, lock, messaging, sync, DirectoryEntry, Inner, Message};
use crate::agent_routines::Trigger;
use crate::agents::{self, Agent};

/// The bytes of memory files one handover carries at most.
const MAX_MEMORY_BYTES: usize = 384 * 1024;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct HandoverThread {
    thread_id: String,
    clock: std::collections::BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct MemoryFile {
    path: String,
    data: String,
}

/// A thread's saved agent conversation, which travels through the content
/// store.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct SessionFile {
    locator: String,
    hash: String,
    size: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Handover {
    agent: Value,
    #[serde(default)]
    threads: Vec<HandoverThread>,
    #[serde(default)]
    primary_thread: Option<String>,
    #[serde(default)]
    memory: Vec<MemoryFile>,
    /// Memory files that did not fit and stay on the old primary.
    #[serde(default)]
    left_behind: Vec<String>,
    #[serde(default)]
    routine_state: Option<Value>,
    #[serde(default)]
    sessions: Vec<SessionFile>,
    /// The secrets of the agent's webhook routines by hook id, so a sender
    /// that reaches the new primary still authenticates.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    hook_secrets: BTreeMap<String, String>,
    /// The saved MCP servers the agent's MCP routines call, as extension
    /// inventory items, and their saved bearer tokens.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    mcp_servers: Vec<Value>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    mcp_tokens: Value,
}

/// The webhook secrets and MCP servers the agent's routines need.
fn routine_needs(inner: &Inner, agent: &Agent, handover: &mut Handover) {
    let mut servers = Vec::new();
    for routine in &agent.routines {
        match &routine.trigger {
            Trigger::Webhook(hook) if !hook.hook_id.is_empty() => {
                if let Some(secret) = inner.host.hook_secret(&hook.hook_id) {
                    handover.hook_secrets.insert(hook.hook_id.clone(), secret);
                }
            }
            Trigger::Mcp(call) => servers.push(call.server.clone()),
            _ => {}
        }
    }
    if !servers.is_empty() {
        let (items, tokens) = crate::extend::export_mcp_servers(&inner.profile, &servers);
        handover.mcp_servers = items;
        handover.mcp_tokens = tokens;
    }
}

fn session_root(inner: &Inner) -> PathBuf {
    crate::chat_profile::ChatProfile::new(&inner.profile).pi_session_root()
}

/// Puts each thread's latest saved conversation in the content store.
fn session_files(inner: &Inner, threads: &[HandoverThread]) -> Result<Vec<SessionFile>, String> {
    let root = session_root(inner);
    let mut files = Vec::new();
    let mut storage = inner
        .storage
        .lock()
        .map_err(|_| "The journal is unavailable.".to_string())?;
    for thread in threads {
        let Some(binding) = storage
            .journal
            .thread_session_binding(&thread.thread_id)
            .map_err(|error| error.to_string())?
        else {
            continue;
        };
        let crate::journal::EventPayload::Inline { payload_json } = &binding.payload else {
            continue;
        };
        let Some(locator) = payload_json["locator"].as_str() else {
            continue;
        };
        let Ok((_, path)) = crate::sidecar::validate_pi_session(&root, locator) else {
            continue;
        };
        let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
        let hash = storage.cas.put(&bytes).map_err(|error| error.to_string())?;
        files.push(SessionFile {
            locator: locator.to_owned(),
            hash: hash.as_str().to_owned(),
            size: bytes.len() as u64,
        });
    }
    Ok(files)
}

fn has_object(inner: &Inner, hash: &str) -> bool {
    hash.parse::<crate::cas::ContentHash>()
        .ok()
        .and_then(|hash| {
            inner
                .storage
                .lock()
                .ok()
                .and_then(|storage| storage.cas.has(&hash).ok())
        })
        .unwrap_or(false)
}

fn memory_root(inner: &Inner, agent_id: &str) -> PathBuf {
    inner.profile.join("agent-memory").join(agent_id)
}

fn collect_memory(
    root: &Path,
    base: &Path,
    files: &mut Vec<MemoryFile>,
    left: &mut Vec<String>,
    budget: &mut usize,
) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_symlink() {
            continue;
        }
        let relative = path
            .strip_prefix(base)
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        if path.is_dir() {
            collect_memory(&path, base, files, left, budget);
        } else if let Ok(bytes) = std::fs::read(&path) {
            if bytes.len() > *budget {
                left.push(relative);
                continue;
            }
            *budget -= bytes.len();
            files.push(MemoryFile {
                path: relative,
                data: STANDARD.encode(bytes),
            });
        }
    }
}

fn later(inner: &Inner, earlier: &str) -> String {
    let now = inner.now_text();
    if now.as_str() > earlier {
        return now;
    }
    super::parse_time(earlier)
        .map(|time| super::timestamp(time + chrono::Duration::seconds(1)))
        .unwrap_or(now)
}

/// Fences the agent here and hands it to `to`.
fn hand_over(inner: &Inner, agent_id: &str, to: &str) -> Result<DirectoryEntry, String> {
    let mut agent = agents::get(&inner.profile, agent_id)?;
    agent.epoch += 1;
    agent.primary_runtime = Some(to.to_owned());
    let agent = agents::save(&inner.profile, agent)?;
    let state = agents::state(&inner.profile)?;
    let threads = state
        .threads
        .iter()
        .filter(|(_, owner)| owner.as_str() == agent_id)
        .map(|(thread, _)| {
            sync::register_agent_thread(inner, thread, agent_id)?;
            Ok(HandoverThread {
                thread_id: thread.clone(),
                clock: sync::thread_clock(inner, thread)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut memory = Vec::new();
    let mut left_behind = Vec::new();
    let mut budget = MAX_MEMORY_BYTES;
    let root = memory_root(inner, agent_id);
    collect_memory(&root, &root, &mut memory, &mut left_behind, &mut budget);
    let handover = Handover {
        agent: serde_json::to_value(&agent).map_err(|error| error.to_string())?,
        threads,
        primary_thread: state.primary_threads.get(agent_id).cloned(),
        memory,
        left_behind,
        routine_state: state
            .routines
            .get(agent_id)
            .and_then(|routines| serde_json::to_value(routines).ok()),
        sessions: Vec::new(),
        ..Handover::default()
    };
    let mut handover = handover;
    handover.sessions = session_files(inner, &handover.threads)?;
    routine_needs(inner, &agent, &mut handover);
    let mut entry = directory::entry(inner, agent_id).unwrap_or(DirectoryEntry {
        agent_id: agent.id.clone(),
        name: agent.name.clone(),
        label: agent.label.clone(),
        avatar: agent
            .avatar
            .as_ref()
            .and_then(|avatar| serde_json::to_value(avatar).ok()),
        primary_runtime: to.to_owned(),
        epoch: 0,
        updated_at: String::new(),
        deleted: false,
        primary_thread: None,
        move_suggested: false,
        definition: None,
        activity: None,
    });
    entry.primary_runtime = to.to_owned();
    entry.activity = None;
    entry.epoch = agent.epoch;
    entry.updated_at = later(inner, &entry.updated_at);
    entry.primary_thread = handover.primary_thread.clone();
    entry.definition = Some(handover.agent.clone());
    directory::put_entry(inner, &entry, None)?;
    messaging::send(
        inner,
        messaging::new_message(
            inner,
            "handover",
            to,
            None,
            Some(agent_id),
            0,
            serde_json::to_value(&handover).map_err(|error| error.to_string())?,
        ),
    )?;
    inner.changed("agents");
    entry.definition = None;
    Ok(entry)
}

/// Makes this runtime the agent's primary.
fn install(inner: &Inner, handover: &Handover, epoch: u64) -> Result<DirectoryEntry, String> {
    let self_id = inner
        .runtime_id()
        .ok_or("This runtime is not part of a swarm.")?;
    let mut agent: Agent = serde_json::from_value(handover.agent.clone())
        .map_err(|_| "The agent definition is invalid.".to_string())?;
    agent.primary_runtime = Some(self_id.clone());
    agent.epoch = epoch;
    // Projects are folders of one machine.
    agent.project_id = None;
    agent.schedule = None;
    if uuid::Uuid::parse_str(&agent.id).is_err() {
        return Err("The agent definition is invalid.".into());
    }
    // The agent's files, conversations and threads land first, so a reader
    // that finds the agent here finds everything it hands over.
    let sessions = session_root(inner);
    for session in &handover.sessions {
        if session.locator.contains(['/', '\\']) || !session.locator.ends_with(".jsonl") {
            continue;
        }
        let Ok(hash) = session.hash.parse::<crate::cas::ContentHash>() else {
            continue;
        };
        let bytes = inner
            .storage
            .lock()
            .map_err(|_| "The journal is unavailable.".to_string())?
            .cas
            .get(&hash)
            .map_err(|error| error.to_string())?;
        if let Some(bytes) = bytes {
            std::fs::create_dir_all(&sessions).map_err(|error| error.to_string())?;
            std::fs::write(sessions.join(&session.locator), bytes)
                .map_err(|error| error.to_string())?;
        }
    }
    for (hook_id, secret) in &handover.hook_secrets {
        if let Err(error) = inner.host.store_hook_secret(hook_id, secret) {
            crate::runtime_eprintln!("swarm: webhook secret not kept: {error}");
        }
    }
    if let Err(error) = crate::extend::import_mcp_servers(
        &inner.profile,
        &handover.mcp_servers,
        &handover.mcp_tokens,
    ) {
        crate::runtime_eprintln!("swarm: MCP servers not added: {error}");
    }
    let root = memory_root(inner, &agent.id);
    for file in &handover.memory {
        let relative = Path::new(&file.path);
        if relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        {
            continue;
        }
        let Ok(bytes) = STANDARD.decode(file.data.as_bytes()) else {
            continue;
        };
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(&path, bytes).map_err(|error| error.to_string())?;
    }
    agents::with_state(&inner.profile, |state| {
        for thread in &handover.threads {
            state
                .threads
                .insert(thread.thread_id.clone(), agent.id.clone());
        }
        if let Some(primary) = &handover.primary_thread {
            state.threads.insert(primary.clone(), agent.id.clone());
            state
                .primary_threads
                .insert(agent.id.clone(), primary.clone());
        }
        if let Some(routines) = handover
            .routine_state
            .clone()
            .and_then(|routines| serde_json::from_value(routines).ok())
        {
            state.routines.insert(agent.id.clone(), routines);
        }
        Ok(())
    })?;
    for thread in &handover.threads {
        sync::register_agent_thread(inner, &thread.thread_id, &agent.id)?;
    }
    let agent = agents::save(&inner.profile, agent)?;
    let previous = directory::entry(inner, &agent.id);
    let entry = DirectoryEntry {
        agent_id: agent.id.clone(),
        name: agent.name.clone(),
        label: agent.label.clone(),
        avatar: agent
            .avatar
            .as_ref()
            .and_then(|avatar| serde_json::to_value(avatar).ok()),
        primary_runtime: self_id,
        epoch,
        updated_at: later(
            inner,
            previous
                .as_ref()
                .map_or("", |previous| previous.updated_at.as_str()),
        ),
        deleted: false,
        primary_thread: handover.primary_thread.clone(),
        move_suggested: false,
        definition: serde_json::to_value(&agent).ok(),
        activity: None,
    };
    directory::put_entry(inner, &entry, None)?;
    if !handover.left_behind.is_empty() {
        messaging::note_in_agent_thread(
            inner,
            &agent.id,
            &format!(
                "Some memory files stayed on the old machine: {}",
                handover.left_behind.join(", ")
            ),
        );
    }
    inner.changed("agents");
    Ok(entry)
}

/// `agent.move`.
pub(super) fn move_operation(inner: &Inner, body: Value) -> Result<Value, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Body {
        id: String,
        to: String,
        #[serde(default)]
        force: bool,
    }
    let body: Body = serde_json::from_value(body).map_err(|_| "The request is invalid.")?;
    let _merge = lock(&inner.merge);
    let self_id = inner
        .runtime_id()
        .ok_or("Turn on remote access to move agents.")?;
    let agent_id = directory::resolve(inner, &body.id)?.agent_id;
    if !link::known_member(inner, &body.to) {
        return Err("The target runtime is not a swarm member.".into());
    }
    let primary = directory::primary_of(inner, &agent_id).ok_or("The agent was not found.")?;
    if primary == body.to {
        return Err("The agent already runs on that machine.".into());
    }
    if primary == self_id {
        let entry = hand_over(inner, &agent_id, &body.to)?;
        return Ok(json!({"status": "moved", "agent": entry}));
    }
    if body.force {
        if body.to != self_id {
            return Err("Force a move on the machine the agent moves to.".into());
        }
        if link::online(inner, &primary) {
            return Err(
                "The agent's primary machine is online. Move the agent without force.".into(),
            );
        }
        let entry = directory::entry(inner, &agent_id).ok_or("The agent was not found.")?;
        let definition = entry
            .definition
            .clone()
            .ok_or("This machine has no copy of the agent's definition yet.")?;
        let threads = sync::registrations(inner)?
            .into_iter()
            .filter(|registration| registration.agent_id.as_deref() == Some(agent_id.as_str()))
            .map(|registration| HandoverThread {
                thread_id: registration.thread_id,
                clock: Default::default(),
            })
            .collect();
        let mut installed = install(
            inner,
            &Handover {
                agent: definition,
                threads,
                primary_thread: entry.primary_thread.clone(),
                ..Handover::default()
            },
            entry.epoch + 1,
        )?;
        installed.definition = None;
        return Ok(json!({"status": "forced", "agent": installed}));
    }
    messaging::send(
        inner,
        messaging::new_message(
            inner,
            "move_request",
            &primary,
            None,
            Some(&agent_id),
            0,
            json!({"agent_id": agent_id, "to": body.to}),
        ),
    )?;
    let mut entry = directory::entry(inner, &agent_id).ok_or("The agent was not found.")?;
    entry.definition = None;
    Ok(json!({"status": "requested", "agent": entry}))
}

/// Keeps a handover until this runtime's replicas caught up.
pub(super) fn on_handover(inner: &Inner, message: &Message) -> Result<(), String> {
    let agent_id = message
        .to_agent
        .clone()
        .ok_or("The handover names no agent.")?;
    let handover: Handover = serde_json::from_value(message.body.clone())
        .map_err(|_| "The handover is invalid.".to_string())?;
    let connection = inner.store.connect()?;
    for session in &handover.sessions {
        if has_object(inner, &session.hash) {
            continue;
        }
        connection
            .execute(
                "INSERT INTO blobs(hash, thread_id, total, requested) VALUES(?1, NULL, ?2, 1) \
                 ON CONFLICT(hash) DO UPDATE SET requested=1",
                rusqlite::params![session.hash, session.size],
            )
            .map_err(super::store::sql)?;
    }
    inner
        .store
        .put_record("moves", "agent_id", "bundle", &agent_id, &message.body)?;
    inner.nudge();
    Ok(())
}

/// Moves an agent this runtime runs at another member's request.
pub(super) fn on_move_request(inner: &Inner, message: &Message) -> Result<(), String> {
    let agent_id = message.body["agent_id"].as_str().unwrap_or_default();
    let to = message.body["to"].as_str().unwrap_or_default();
    if directory::primary_of(inner, agent_id) != inner.runtime_id() {
        return Err("This machine no longer runs the agent.".into());
    }
    hand_over(inner, agent_id, to).map(drop)
}

/// Installs each handover whose threads this runtime holds in full.
pub(super) fn activate_pending(inner: &Inner) -> Result<bool, String> {
    let pending: Vec<(String, Handover)> = {
        let connection = inner.store.connect()?;
        let mut statement = connection
            .prepare("SELECT agent_id, bundle FROM moves")
            .map_err(super::store::sql)?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(super::store::sql)?;
        rows.filter_map(Result::ok)
            .filter_map(|(id, raw)| Some((id, serde_json::from_str(&raw).ok()?)))
            .collect()
    };
    for (agent_id, handover) in pending {
        let mut ready = true;
        for thread in &handover.threads {
            let held = sync::thread_clock(inner, &thread.thread_id)?;
            if thread
                .clock
                .iter()
                .any(|(origin, seq)| held.get(origin).copied().unwrap_or(0) < *seq)
            {
                ready = false;
            }
        }
        if !ready
            || !handover
                .sessions
                .iter()
                .all(|session| has_object(inner, &session.hash))
        {
            continue;
        }
        let epoch = handover.agent["epoch"].as_u64().unwrap_or(0);
        install(inner, &handover, epoch)?;
        inner.store.delete_record("moves", "agent_id", &agent_id)?;
    }
    Ok(false)
}
