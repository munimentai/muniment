//! The durable queue of agent runs.
//!
//! Every run an agent makes without a user message (a manual "Run now", a
//! routine, an event) is a `Run` record here before it starts. The runtime
//! leases a queued run, starts it in the agent's thread under the same id the
//! run journal uses, renews the lease while the run is live, and records how
//! it ended. A lease that expires is reconciled against the journal: a run
//! that never started is queued again, and one that started without ending is
//! `interrupted`.
//!
//! Each change reaches every subscriber of the store as the run's JSON, the
//! body of an `agent.run_updated` event.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;
use std::time::Duration;

use chrono::{SecondsFormat, TimeZone, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The environment variable that caps concurrent runs.
pub const MAX_PARALLEL_RUNS_ENV: &str = "MUNIMENT_MAX_PARALLEL_RUNS";
/// The concurrent runs a runtime allows when the environment names none.
pub const DEFAULT_MAX_PARALLEL_RUNS: usize = 3;
const MAX_PARALLEL_RUNS_LIMIT: usize = 16;
/// How long a lease holds without a renewal.
pub const LEASE_SECONDS: i64 = 90;
/// The most times a run that never started is leased.
pub const MAX_ATTEMPTS: u32 = 3;
/// How long a run waits for input before the runtime reminds the user.
pub const REMIND_AFTER_SECONDS: i64 = 4 * 60 * 60;
/// How long a run waits for input before the runtime cancels it.
pub const CANCEL_AFTER_SECONDS: i64 = 24 * 60 * 60;
/// The ended runs each agent keeps.
pub const MAX_RUN_HISTORY: usize = 200;
/// The most agent-to-agent steps one chain of runs takes.
pub const MAX_HOPS: u32 = 6;
const MAX_PAGE: usize = 100;
/// How long a webhook delivery id is remembered, so a replay is refused.
pub const DELIVERY_MEMORY_SECONDS: i64 = 7 * 24 * 60 * 60;
const MAX_SUMMARY_CHARS: usize = 500;
const STORE_FILE: &str = "agent-runs.sqlite3";

/// The concurrent runs this process allows: `MUNIMENT_MAX_PARALLEL_RUNS`,
/// between 1 and 16, else 3.
pub fn max_parallel_runs() -> usize {
    std::env::var(MAX_PARALLEL_RUNS_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value >= 1)
        .map_or(DEFAULT_MAX_PARALLEL_RUNS, |value| {
            value.min(MAX_PARALLEL_RUNS_LIMIT)
        })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Leased,
    Running,
    WaitingInput,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Leased => "leased",
            Self::Running => "running",
            Self::WaitingInput => "waiting_input",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        [
            Self::Queued,
            Self::Leased,
            Self::Running,
            Self::WaitingInput,
            Self::Completed,
            Self::Failed,
            Self::Cancelled,
            Self::Interrupted,
        ]
        .into_iter()
        .find(|status| status.as_str() == text)
    }

    /// Whether the run still holds or waits for a place to run.
    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::Queued | Self::Leased | Self::Running | Self::WaitingInput
        )
    }
}

/// What started a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerKind {
    /// A person asked the agent to run now.
    Manual,
    Cron,
    Webhook,
    Mcp,
    /// Another agent's message woke the agent.
    AgentMessage,
}

impl TriggerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Cron => "cron",
            Self::Webhook => "webhook",
            Self::Mcp => "mcp",
            Self::AgentMessage => "agent_message",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        [
            Self::Manual,
            Self::Cron,
            Self::Webhook,
            Self::Mcp,
            Self::AgentMessage,
        ]
        .into_iter()
        .find(|kind| kind.as_str() == text)
    }

    /// Whether an outside event started the run, so it asks before writes.
    pub fn event_triggered(self) -> bool {
        matches!(self, Self::Webhook | Self::Mcp | Self::AgentMessage)
    }

    /// Whether a routine started the run, so a silent reply posts nothing.
    pub fn routine(self) -> bool {
        matches!(self, Self::Cron | Self::Webhook | Self::Mcp)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunTrigger {
    pub kind: TriggerKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routine_id: Option<String>,
    /// Who or what fired it, such as `desktop` or a webhook's sender.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// One queued, live or ended agent run. Its `id` is the run journal's run id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub agent_id: String,
    pub thread_id: Option<String>,
    pub trigger: RunTrigger,
    pub status: RunStatus,
    /// How many times the run was leased.
    pub attempt: u32,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<String>,
    /// Agent-to-agent steps before this run. A user message starts at 0.
    pub hop: u32,
    pub origin_run_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub result_summary: Option<String>,
    /// A routine run that replied with the silent reply and posted nothing.
    #[serde(default)]
    pub silent: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// When the run started waiting for an answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting_since: Option<String>,
    /// When the runtime reminded the user about the wait.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reminded_at: Option<String>,
    /// What the run asks the agent. It stays in the store.
    #[serde(skip)]
    pub prompt: String,
}

/// A run to queue.
#[derive(Clone, Debug)]
pub struct NewRun {
    pub agent_id: String,
    pub thread_id: Option<String>,
    pub trigger: RunTrigger,
    pub prompt: String,
    pub hop: u32,
    pub origin_run_id: Option<String>,
}

/// One page of run history, newest first.
#[derive(Clone, Debug, Serialize)]
pub struct RunPage {
    pub runs: Vec<Run>,
    pub next_cursor: Option<String>,
}

/// An RFC 3339 time from seconds since the epoch.
pub fn timestamp(seconds: i64) -> String {
    Utc.timestamp_opt(seconds, 0)
        .single()
        .unwrap_or_else(Utc::now)
        .to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Seconds since the epoch from an RFC 3339 time.
pub fn seconds(time: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(time)
        .ok()
        .map(|time| time.timestamp())
}

pub fn now() -> i64 {
    Utc::now().timestamp()
}

static SUBSCRIBERS: Mutex<Vec<(PathBuf, Sender<Value>)>> = Mutex::new(Vec::new());

/// Receives the JSON of each run the store in `profile` changes.
pub fn subscribe(profile: &Path) -> Receiver<Value> {
    let (sender, receiver) = mpsc::channel();
    SUBSCRIBERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push((profile.join(STORE_FILE), sender));
    receiver
}

/// Whether a run's change warrants a notification: it just completed with
/// a reply, failed, or started to wait for an answer.
pub fn warrants_notification(previous: Option<RunStatus>, run: &Run) -> bool {
    previous != Some(run.status)
        && match run.status {
            RunStatus::Completed => !run.silent,
            RunStatus::Failed | RunStatus::WaitingInput => true,
            _ => false,
        }
}

fn publish(path: &Path, run: &Run, notify: bool) {
    let Ok(mut body) = serde_json::to_value(run) else {
        return;
    };
    body["notify"] = Value::Bool(notify);
    publish_body(path, body);
}

fn publish_body(path: &Path, body: Value) {
    SUBSCRIBERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .retain(|(store, sender)| store != path || sender.send(body.clone()).is_ok());
}

/// Sends a run another swarm member reported to this profile's subscribers,
/// as the body of an `agent.run_updated` event.
pub fn publish_remote(profile: &Path, body: Value) {
    publish_body(&profile.join(STORE_FILE), body);
}

/// What an agent's avatar shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityState {
    Idle,
    Running,
    Waiting,
    Error,
}

/// What an agent is doing now, derived from its runs in the queue and the
/// gate a run waits at.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    pub state: ActivityState,
    /// A short text of the current action, such as the approval a run waits
    /// for or why the last run failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// When the agent entered this state. None for an agent that never ran.
    pub since: Option<String>,
}

const MAX_ACTION_CHARS: usize = 120;

fn short_action(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= MAX_ACTION_CHARS {
        return text.to_owned();
    }
    let mut short: String = text.chars().take(MAX_ACTION_CHARS - 1).collect();
    short.push('…');
    short
}

fn running_action(run: &Run) -> String {
    if run.trigger.source.as_deref() == Some(REPLAY_SOURCE) {
        return "Replays a failed run".into();
    }
    match run.trigger.kind {
        TriggerKind::Manual => "Runs on request",
        TriggerKind::Cron => "Runs a scheduled routine",
        TriggerKind::Webhook => "Handles a webhook",
        TriggerKind::Mcp => "Handles an MCP trigger",
        TriggerKind::AgentMessage => "Answers an agent message",
    }
    .into()
}

/// An agent's activity from its active runs, oldest first, and its newest
/// ended run. A run that waits for an answer outranks a running one, which
/// outranks a queued one; with none, a failed last run shows an error and
/// anything else is idle. `gate` names the approval a run waits at.
pub fn derive_activity(
    active: &[Run],
    latest: Option<&Run>,
    gate: impl Fn(&str) -> Option<String>,
) -> Activity {
    if let Some(run) = active
        .iter()
        .find(|run| run.status == RunStatus::WaitingInput)
    {
        return Activity {
            state: ActivityState::Waiting,
            action: Some(short_action(
                &gate(&run.id).unwrap_or_else(|| "Waits for an answer".into()),
            )),
            since: Some(
                run.waiting_since
                    .clone()
                    .unwrap_or_else(|| run.updated_at.clone()),
            ),
        };
    }
    if let Some(run) = active
        .iter()
        .find(|run| matches!(run.status, RunStatus::Running | RunStatus::Leased))
    {
        return Activity {
            state: ActivityState::Running,
            action: Some(running_action(run)),
            since: Some(run.created_at.clone()),
        };
    }
    if let Some(run) = active.first() {
        return Activity {
            state: ActivityState::Running,
            action: Some("Waits to start".into()),
            since: Some(run.created_at.clone()),
        };
    }
    match latest {
        Some(run) if run.status == RunStatus::Failed => Activity {
            state: ActivityState::Error,
            action: Some(short_action(
                run.error.as_deref().unwrap_or("The last run failed."),
            )),
            since: Some(run.updated_at.clone()),
        },
        latest => Activity {
            state: ActivityState::Idle,
            action: None,
            since: latest.map(|run| run.updated_at.clone()),
        },
    }
}

/// The activity of each agent in `agent_ids`, from one read of the queue.
pub fn activities(
    profile: &Path,
    agent_ids: &[String],
    gate: &dyn Fn(&str) -> Option<String>,
) -> Result<std::collections::BTreeMap<String, Activity>, String> {
    let store = RunStore::open(profile)?;
    let active = store.active()?;
    let mut activities = std::collections::BTreeMap::new();
    for agent_id in agent_ids {
        let mine: Vec<Run> = active
            .iter()
            .filter(|run| &run.agent_id == agent_id)
            .cloned()
            .collect();
        let latest = if mine.is_empty() {
            store.latest(agent_id)?
        } else {
            None
        };
        activities.insert(
            agent_id.clone(),
            derive_activity(&mine, latest.as_ref(), gate),
        );
    }
    Ok(activities)
}

/// The `source` of a run that replays a failed run's inputs.
pub const REPLAY_SOURCE: &str = "replay";

/// The run store of one profile.
#[derive(Clone, Debug)]
pub struct RunStore {
    path: PathBuf,
}

const COLUMNS: &str =
    "id, agent_id, thread_id, trigger_kind, routine_id, source, status, attempt, \
    lease_owner, lease_expires_at, hop, origin_run_id, created_at, updated_at, result_summary, \
    silent, error, waiting_since, reminded_at, prompt";

impl RunStore {
    /// Opens the store under `profile`, creating it on first use.
    pub fn open(profile: &Path) -> Result<Self, String> {
        let store = Self {
            path: profile.join(STORE_FILE),
        };
        let connection = store.connect()?;
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS runs (
                    id TEXT PRIMARY KEY,
                    agent_id TEXT NOT NULL,
                    thread_id TEXT,
                    trigger_kind TEXT NOT NULL,
                    routine_id TEXT,
                    source TEXT,
                    status TEXT NOT NULL,
                    attempt INTEGER NOT NULL,
                    lease_owner TEXT,
                    lease_expires_at INTEGER,
                    hop INTEGER NOT NULL,
                    origin_run_id TEXT,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    result_summary TEXT,
                    silent INTEGER NOT NULL DEFAULT 0,
                    error TEXT,
                    waiting_since INTEGER,
                    reminded_at INTEGER,
                    prompt TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS runs_agent ON runs(agent_id, id);
                CREATE INDEX IF NOT EXISTS runs_status ON runs(status);
                CREATE TABLE IF NOT EXISTS webhook_deliveries (
                    hook_id TEXT NOT NULL,
                    delivery_id TEXT NOT NULL,
                    received_at INTEGER NOT NULL,
                    PRIMARY KEY (hook_id, delivery_id)
                );",
            )
            .map_err(|_| "Agent run history could not be opened.".to_string())?;
        Ok(store)
    }

    fn connect(&self) -> Result<Connection, String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|_| "Agent run history is unavailable.".to_string())?;
        }
        let connection = Connection::open(&self.path)
            .map_err(|_| "Agent run history is unavailable.".to_string())?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|_| "Agent run history is unavailable.".to_string())?;
        let _ = connection.pragma_update(None, "journal_mode", "WAL");
        Ok(connection)
    }

    /// Queues a run. A chain of agent messages past [`MAX_HOPS`] is refused.
    pub fn enqueue(&self, new: NewRun) -> Result<Run, String> {
        if new.hop > MAX_HOPS {
            return Err(format!(
                "This message passed through {MAX_HOPS} agents already, so it was not delivered."
            ));
        }
        let now = now();
        let run = Run {
            id: crate::run_start::new_run_id(),
            agent_id: new.agent_id,
            thread_id: new.thread_id,
            trigger: new.trigger,
            status: RunStatus::Queued,
            attempt: 0,
            lease_owner: None,
            lease_expires_at: None,
            hop: new.hop,
            origin_run_id: new.origin_run_id,
            created_at: timestamp(now),
            updated_at: timestamp(now),
            result_summary: None,
            silent: false,
            error: None,
            waiting_since: None,
            reminded_at: None,
            prompt: new.prompt,
        };
        let connection = self.connect()?;
        connection
            .execute(
                &format!("INSERT INTO runs ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)"),
                params![
                    run.id,
                    run.agent_id,
                    run.thread_id,
                    run.trigger.kind.as_str(),
                    run.trigger.routine_id,
                    run.trigger.source,
                    run.status.as_str(),
                    run.attempt,
                    run.lease_owner,
                    Option::<i64>::None,
                    run.hop,
                    run.origin_run_id,
                    now,
                    now,
                    run.result_summary,
                    run.silent,
                    run.error,
                    Option::<i64>::None,
                    Option::<i64>::None,
                    run.prompt,
                ],
            )
            .map_err(|_| "The run could not be queued.".to_string())?;
        publish(&self.path, &run, false);
        Ok(run)
    }

    pub fn get(&self, id: &str) -> Result<Option<Run>, String> {
        self.connect()?
            .query_row(
                &format!("SELECT {COLUMNS} FROM runs WHERE id = ?1"),
                [id],
                read_run,
            )
            .optional()
            .map_err(|_| "Agent run history could not be read.".to_string())
    }

    /// Runs that are queued, leased, running or waiting, oldest first.
    pub fn active(&self) -> Result<Vec<Run>, String> {
        self.query(
            &format!(
                "SELECT {COLUMNS} FROM runs WHERE status IN ('queued', 'leased', 'running', 'waiting_input') ORDER BY id"
            ),
            [],
        )
    }

    /// Every run's id with its agent and thread, for usage by agent.
    pub fn agents_by_run(&self) -> Result<Vec<(String, String, Option<String>)>, String> {
        let connection = self.connect()?;
        let mut statement = connection
            .prepare("SELECT id, agent_id, thread_id FROM runs")
            .map_err(|_| "Agent run history could not be read.".to_string())?;
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|_| "Agent run history could not be read.".to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|_| "Agent run history could not be read.".to_string())
    }

    /// One agent's newest run, if it has one.
    pub fn latest(&self, agent_id: &str) -> Result<Option<Run>, String> {
        Ok(self.page(Some(agent_id), 1, None)?.runs.into_iter().next())
    }

    /// One page of history, newest first, for one agent or all of them.
    /// `cursor` is the `next_cursor` of the page before.
    pub fn page(
        &self,
        agent_id: Option<&str>,
        limit: usize,
        cursor: Option<&str>,
    ) -> Result<RunPage, String> {
        let limit = limit.clamp(1, MAX_PAGE);
        let mut runs = self.query(
            &format!(
                "SELECT {COLUMNS} FROM runs WHERE (?1 IS NULL OR agent_id = ?1) AND (?2 IS NULL OR id < ?2) ORDER BY id DESC LIMIT ?3"
            ),
            params![agent_id, cursor, (limit + 1) as i64],
        )?;
        let next_cursor = (runs.len() > limit).then(|| {
            runs.truncate(limit);
            runs.last().map(|run| run.id.clone()).unwrap_or_default()
        });
        Ok(RunPage { runs, next_cursor })
    }

    fn query(&self, sql: &str, params: impl rusqlite::Params) -> Result<Vec<Run>, String> {
        let connection = self.connect()?;
        let mut statement = connection
            .prepare(sql)
            .map_err(|_| "Agent run history could not be read.".to_string())?;
        let rows = statement
            .query_map(params, read_run)
            .map_err(|_| "Agent run history could not be read.".to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|_| "Agent run history could not be read.".to_string())
    }

    /// Records a webhook delivery id. A delivery the hook already received
    /// answers `false`. Ids older than [`DELIVERY_MEMORY_SECONDS`] are
    /// forgotten.
    pub fn record_delivery(&self, hook_id: &str, delivery_id: &str) -> Result<bool, String> {
        let now = now();
        let connection = self.connect()?;
        connection
            .execute(
                "DELETE FROM webhook_deliveries WHERE received_at < ?1",
                [now - DELIVERY_MEMORY_SECONDS],
            )
            .map_err(|_| "Webhook deliveries could not be read.".to_string())?;
        let inserted = connection
            .execute(
                "INSERT OR IGNORE INTO webhook_deliveries (hook_id, delivery_id, received_at) VALUES (?1, ?2, ?3)",
                params![hook_id, delivery_id, now],
            )
            .map_err(|_| "Webhook deliveries could not be saved.".to_string())?;
        Ok(inserted == 1)
    }

    /// Leases a queued run for `owner` and counts the attempt. Another owner's
    /// lease, or a run no longer queued, answers `None`.
    pub fn lease(&self, id: &str, owner: &str) -> Result<Option<Run>, String> {
        let now = now();
        let changed = self
            .connect()?
            .execute(
                "UPDATE runs SET status = 'leased', lease_owner = ?2, lease_expires_at = ?3, attempt = attempt + 1, updated_at = ?4 WHERE id = ?1 AND status = 'queued'",
                params![id, owner, now + LEASE_SECONDS, now],
            )
            .map_err(|_| "The run could not be leased.".to_string())?;
        if changed == 0 {
            return Ok(None);
        }
        let run = self.get(id)?;
        if let Some(run) = &run {
            publish(&self.path, run, false);
        }
        Ok(run)
    }

    /// Saves every changed field of `run`, stamps its update time and tells
    /// subscribers. An unchanged run is not written.
    pub fn save(&self, run: &mut Run) -> Result<(), String> {
        let previous = self.get(&run.id)?;
        if previous.as_ref() == Some(run) {
            return Ok(());
        }
        let previous_status = previous.map(|previous| previous.status);
        let notify = warrants_notification(previous_status, run);
        let now = now();
        run.updated_at = timestamp(now);
        if let Some(summary) = &run.result_summary {
            run.result_summary = Some(summary.chars().take(MAX_SUMMARY_CHARS).collect());
        }
        let connection = self.connect()?;
        connection
            .execute(
                "UPDATE runs SET thread_id = ?2, status = ?3, attempt = ?4, lease_owner = ?5, lease_expires_at = ?6, \
                 result_summary = ?7, silent = ?8, error = ?9, waiting_since = ?10, reminded_at = ?11, updated_at = ?12 WHERE id = ?1",
                params![
                    run.id,
                    run.thread_id,
                    run.status.as_str(),
                    run.attempt,
                    run.lease_owner,
                    run.lease_expires_at.as_deref().and_then(seconds),
                    run.result_summary,
                    run.silent,
                    run.error,
                    run.waiting_since.as_deref().and_then(seconds),
                    run.reminded_at.as_deref().and_then(seconds),
                    now,
                ],
            )
            .map_err(|_| "The run could not be saved.".to_string())?;
        if !run.status.is_active() {
            prune(&connection, &run.agent_id)?;
        }
        if run.status == RunStatus::Failed && previous_status != Some(RunStatus::Failed) {
            if let Some(profile) = self.path.parent() {
                if let Err(error) = crate::agent_replays::record(profile, run) {
                    crate::runtime_eprintln!(
                        "muniment-runtime: failed run inputs not saved run_id={}: {error}",
                        run.id
                    );
                }
            }
        }
        publish(&self.path, run, notify);
        Ok(())
    }

    /// Removes every run of one agent.
    pub fn delete_agent(&self, agent_id: &str) -> Result<(), String> {
        self.connect()?
            .execute("DELETE FROM runs WHERE agent_id = ?1", [agent_id])
            .map(drop)
            .map_err(|_| "The agent's runs could not be removed.".to_string())
    }
}

/// Keeps the newest [`MAX_RUN_HISTORY`] ended runs of one agent.
fn prune(connection: &Connection, agent_id: &str) -> Result<(), String> {
    connection
        .execute(
            "DELETE FROM runs WHERE agent_id = ?1 AND status NOT IN ('queued', 'leased', 'running', 'waiting_input') \
             AND id NOT IN (SELECT id FROM runs WHERE agent_id = ?1 AND status NOT IN ('queued', 'leased', 'running', 'waiting_input') ORDER BY id DESC LIMIT ?2)",
            params![agent_id, MAX_RUN_HISTORY as i64],
        )
        .map(drop)
        .map_err(|_| "Agent run history could not be trimmed.".to_string())
}

fn read_run(row: &Row<'_>) -> rusqlite::Result<Run> {
    let invalid = |index: usize| {
        rusqlite::Error::InvalidColumnType(index, "run".into(), rusqlite::types::Type::Text)
    };
    let time = |value: Option<i64>| value.map(timestamp);
    Ok(Run {
        id: row.get(0)?,
        agent_id: row.get(1)?,
        thread_id: row.get(2)?,
        trigger: RunTrigger {
            kind: TriggerKind::parse(&row.get::<_, String>(3)?).ok_or_else(|| invalid(3))?,
            routine_id: row.get(4)?,
            source: row.get(5)?,
        },
        status: RunStatus::parse(&row.get::<_, String>(6)?).ok_or_else(|| invalid(6))?,
        attempt: row.get(7)?,
        lease_owner: row.get(8)?,
        lease_expires_at: time(row.get(9)?),
        hop: row.get(10)?,
        origin_run_id: row.get(11)?,
        created_at: timestamp(row.get(12)?),
        updated_at: timestamp(row.get(13)?),
        result_summary: row.get(14)?,
        silent: row.get(15)?,
        error: row.get(16)?,
        waiting_since: time(row.get(17)?),
        reminded_at: time(row.get(18)?),
        prompt: row.get(19)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (PathBuf, RunStore) {
        let root =
            std::env::temp_dir().join(format!("muniment-agent-runs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store = RunStore::open(&root).unwrap();
        (root, store)
    }

    fn new_run(agent: &str, kind: TriggerKind) -> NewRun {
        NewRun {
            agent_id: agent.into(),
            thread_id: None,
            trigger: RunTrigger {
                kind,
                routine_id: None,
                source: None,
            },
            prompt: "Do the work.".into(),
            hop: 0,
            origin_run_id: None,
        }
    }

    #[test]
    fn an_agents_activity_follows_its_runs_and_gate() {
        let (_root, store) = store();
        let none = derive_activity(&[], None, |_| None);
        assert_eq!(none.state, ActivityState::Idle);
        assert!(none.since.is_none() && none.action.is_none());
        let queued = store
            .enqueue(new_run("agent-a", TriggerKind::Webhook))
            .unwrap();
        let waiting_start = derive_activity(std::slice::from_ref(&queued), None, |_| None);
        assert_eq!(waiting_start.state, ActivityState::Running);
        assert_eq!(waiting_start.action.as_deref(), Some("Waits to start"));
        assert_eq!(waiting_start.since, Some(queued.created_at.clone()));
        let mut running = queued.clone();
        running.status = RunStatus::Running;
        let busy = derive_activity(&[running.clone()], None, |_| None);
        assert_eq!(busy.action.as_deref(), Some("Handles a webhook"));
        let mut waiting = running.clone();
        waiting.status = RunStatus::WaitingInput;
        waiting.waiting_since = Some("2026-01-01T00:00:05Z".into());
        // A waiting run outranks a running one and shows its approval.
        let asked = derive_activity(&[running.clone(), waiting.clone()], None, |run_id| {
            (run_id == waiting.id).then(|| "bash `git push` in /work".to_owned())
        });
        assert_eq!(asked.state, ActivityState::Waiting);
        assert_eq!(asked.action.as_deref(), Some("bash `git push` in /work"));
        assert_eq!(asked.since.as_deref(), Some("2026-01-01T00:00:05Z"));
        let mut failed = running.clone();
        failed.status = RunStatus::Failed;
        failed.error = Some("x".repeat(300));
        let error = derive_activity(&[], Some(&failed), |_| None);
        assert_eq!(error.state, ActivityState::Error);
        assert_eq!(
            error.action.as_ref().unwrap().chars().count(),
            MAX_ACTION_CHARS
        );
        let mut done = running;
        done.status = RunStatus::Completed;
        done.trigger.source = Some(REPLAY_SOURCE.into());
        let idle = derive_activity(&[], Some(&done), |_| None);
        assert_eq!(idle.state, ActivityState::Idle);
        assert_eq!(idle.since, Some(done.updated_at.clone()));
        done.status = RunStatus::Running;
        assert_eq!(
            derive_activity(&[done], None, |_| None).action.as_deref(),
            Some("Replays a failed run")
        );
    }

    #[test]
    fn a_queued_run_leases_once_and_its_changes_reach_subscribers() {
        let (root, store) = store();
        let events = subscribe(&root);
        let run = store
            .enqueue(new_run("agent-a", TriggerKind::Manual))
            .unwrap();
        assert_eq!(run.status, RunStatus::Queued);
        assert_eq!(events.try_recv().unwrap()["status"], "queued");
        let leased = store.lease(&run.id, "runtime-1").unwrap().unwrap();
        assert_eq!(leased.status, RunStatus::Leased);
        assert_eq!(leased.attempt, 1);
        assert_eq!(leased.lease_owner.as_deref(), Some("runtime-1"));
        assert!(store.lease(&run.id, "runtime-2").unwrap().is_none());
        let leased_event = events.try_recv().unwrap();
        assert_eq!(leased_event["status"], "leased");
        assert_eq!(leased_event["notify"], false);
        let mut done = leased;
        done.status = RunStatus::Completed;
        done.result_summary = Some("x".repeat(900));
        store.save(&mut done).unwrap();
        let read = store.get(&run.id).unwrap().unwrap();
        assert_eq!(read.status, RunStatus::Completed);
        assert_eq!(read.result_summary.unwrap().chars().count(), 500);
        assert_eq!(read.prompt, "Do the work.");
        let event = events.try_recv().unwrap();
        assert_eq!(event["status"], "completed");
        assert_eq!(event["notify"], true);
        assert!(event.get("prompt").is_none());
        // An unchanged save writes nothing and sends nothing.
        let mut same = store.get(&run.id).unwrap().unwrap();
        store.save(&mut same).unwrap();
        assert!(events.try_recv().is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn history_pages_newest_first_and_keeps_the_last_200_runs() {
        let (root, store) = store();
        for _ in 0..(MAX_RUN_HISTORY + 5) {
            let mut run = store
                .enqueue(new_run("agent-a", TriggerKind::Cron))
                .unwrap();
            run.status = RunStatus::Completed;
            store.save(&mut run).unwrap();
        }
        let queued = store
            .enqueue(new_run("agent-a", TriggerKind::Manual))
            .unwrap();
        store
            .enqueue(new_run("agent-b", TriggerKind::Manual))
            .unwrap();
        let mut seen = Vec::new();
        let mut cursor = None;
        loop {
            let page = store.page(Some("agent-a"), 50, cursor.as_deref()).unwrap();
            seen.extend(page.runs.iter().map(|run| run.id.clone()));
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        assert_eq!(seen.len(), MAX_RUN_HISTORY + 1);
        assert_eq!(seen[0], queued.id);
        let mut sorted = seen.clone();
        sorted.sort_by(|a, b| b.cmp(a));
        assert_eq!(seen, sorted);
        assert_eq!(store.page(None, 1000, None).unwrap().runs.len(), MAX_PAGE);
        assert_eq!(store.active().unwrap().len(), 2);
        assert_eq!(store.latest("agent-a").unwrap().unwrap().id, queued.id);
        store.delete_agent("agent-a").unwrap();
        assert!(store.latest("agent-a").unwrap().is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn only_a_reply_a_failure_or_a_question_warrants_a_notification() {
        let (root, store) = store();
        let mut run = store
            .enqueue(new_run("agent-a", TriggerKind::Cron))
            .unwrap();
        for (status, silent, expected) in [
            (RunStatus::Running, false, false),
            (RunStatus::WaitingInput, false, true),
            (RunStatus::Completed, true, false),
            (RunStatus::Failed, false, true),
            (RunStatus::Cancelled, false, false),
        ] {
            let previous = run.status;
            run.status = status;
            run.silent = silent;
            assert_eq!(
                warrants_notification(Some(previous), &run),
                expected,
                "{status:?}"
            );
        }
        run.status = RunStatus::Completed;
        run.silent = false;
        assert!(warrants_notification(Some(RunStatus::Running), &run));
        assert!(!warrants_notification(Some(RunStatus::Completed), &run));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_chain_past_the_hop_cap_is_refused() {
        let (root, store) = store();
        let mut run = new_run("agent-a", TriggerKind::AgentMessage);
        run.hop = MAX_HOPS;
        store.enqueue(run.clone()).unwrap();
        run.hop = MAX_HOPS + 1;
        assert!(store.enqueue(run).unwrap_err().contains("6 agents"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn run_json_matches_the_contract() {
        let (root, store) = store();
        let mut new = new_run("agent-a", TriggerKind::Cron);
        new.trigger.routine_id = Some("morning".into());
        let run = store.enqueue(new).unwrap();
        let value = serde_json::to_value(&run).unwrap();
        for key in [
            "id",
            "agent_id",
            "thread_id",
            "trigger",
            "status",
            "attempt",
            "lease_owner",
            "lease_expires_at",
            "hop",
            "origin_run_id",
            "created_at",
            "updated_at",
            "result_summary",
            "silent",
        ] {
            assert!(value.get(key).is_some(), "{key}");
        }
        assert_eq!(
            value["trigger"],
            serde_json::json!({"kind": "cron", "routine_id": "morning"})
        );
        assert!(TriggerKind::Webhook.event_triggered());
        assert!(!TriggerKind::Manual.event_triggered());
        assert!(TriggerKind::Cron.routine() && !TriggerKind::AgentMessage.routine());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_parallel_run_cap_reads_the_environment() {
        std::env::remove_var(MAX_PARALLEL_RUNS_ENV);
        assert_eq!(max_parallel_runs(), 3);
        std::env::set_var(MAX_PARALLEL_RUNS_ENV, "5");
        assert_eq!(max_parallel_runs(), 5);
        std::env::set_var(MAX_PARALLEL_RUNS_ENV, "0");
        assert_eq!(max_parallel_runs(), 3);
        std::env::set_var(MAX_PARALLEL_RUNS_ENV, "99");
        assert_eq!(max_parallel_runs(), 16);
        std::env::remove_var(MAX_PARALLEL_RUNS_ENV);
    }
}
