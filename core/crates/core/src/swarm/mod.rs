//! The swarm: the runtimes of one owner, linked over the remote attach route,
//! that share agents.
//!
//! - **Links.** A runtime joins another by redeeming a `swarm.peer` pairing
//!   link the other issued. The joining side keeps the link record and dials
//!   with backoff; the other sees the link as a paired device. Data flows
//!   both ways in each `swarm.exchange`, so a link needs one reachable end.
//! - **Relay.** Members gossip members, the agent directory, groups, thread
//!   events, messages, acknowledgements, notices and approvals to every
//!   linked member, which passes them on. A member reaches every other member
//!   through any chain of links, with no central server.
//! - **Threads.** Agent and group threads replicate as journal events, each
//!   with its `(origin, seq)`. The agent's primary runtime alone writes run
//!   output; a user message posted on any member is a `thread.message.posted`
//!   event with that member as origin, which the primary consumes in causal
//!   order.
//! - **Forwarded actions.** A client acting on an item another member owns,
//!   such as a post to its thread or a run of its agent, gets
//!   `{forwarded: true, op_id}`. The action travels to the owner as an `op`
//!   message, and the owner runs the same operation for it.
//! - **Records.** The directory, groups and thread registrations, and the
//!   classes other parts of core keep through [`Records`], replicate as
//!   field-level last-writer-wins records ordered by a hybrid logical clock.
//!   Members that name `store/1` trade only the records and thread clocks a
//!   peer lacks; an older member gets the full state.
//!
//! The runtime supplies what only it can do through [`SwarmHost`].

mod absence;
mod accounts;
mod approvals;
mod credentials;
mod directory;
mod fleet;
mod groups;
mod hlc;
mod link;
mod messaging;
mod moves;
mod ops;
mod records;
mod store;
mod sync;
pub mod wire;

#[cfg(test)]
mod class_tests;
#[cfg(test)]
mod exchange_tests;
#[cfg(test)]
mod fleet_tests;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};
use muniment_attach::{EventName, Operation};
use serde_json::{json, Value};

use crate::attach::RemoteAccess;
use crate::permission_gate::ChatPermissionAnswer;
use crate::run_events::SharedStorage;

pub use fleet::{RecipientSet, Role};
pub use hlc::Hlc;
pub use link::PEER_KIND;
pub use ops::{
    is_forwardable, FleetActor, Forwarded, Op, OpItem, OpReason, FORWARDABLE, MOVE_SUGGEST_HOURS,
    OPS_CAPABILITY, OP_DAYS,
};
pub use records::{
    ClassCount, Field, Record, RecordChange, Tombstone, STORE_CAPABILITY, TOMBSTONE_DAYS,
};
pub use sync::{posted_messages, SWARM_ORIGIN};
pub use wire::*;

/// One event the swarm sends to this runtime's clients.
#[derive(Clone, Debug, PartialEq)]
pub struct ClientEvent {
    pub name: EventName,
    pub body: Value,
}

/// Where replica threads live on this runtime: the workspace and attach
/// profile its threads use and the owner subject its runs record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplicaScope {
    pub workspace: String,
    pub attach_profile: String,
    pub subject: Option<String>,
}

/// What the runtime does for the swarm.
pub trait SwarmHost: Send + Sync {
    /// The scope replica threads take, once the runtime has a workspace.
    fn replica_scope(&self) -> Option<ReplicaScope>;
    /// Queues `text` as steering input of the run live in `thread_id` and
    /// answers that run's id, or `None` when no run is live there.
    fn steer(&self, thread_id: &str, text: &str) -> Option<String>;
    /// Answers a live run's permission gate.
    fn answer_permission(
        &self,
        run_id: &str,
        gate_id: &str,
        answer: ChatPermissionAnswer,
    ) -> Result<(), String>;
    /// Starts queued agent runs soon, without waiting for the next pass.
    fn kick_runs(&self);
    /// The secret of this runtime's webhook `hook_id`, which a move hands to
    /// the agent's new primary.
    fn hook_secret(&self, _hook_id: &str) -> Option<String> {
        None
    }
    /// Keeps the secret of a webhook another member handed over.
    fn store_hook_secret(&self, _hook_id: &str, _secret: &str) -> Result<(), String> {
        Err("This runtime keeps no webhook secrets.".into())
    }
    /// The store this runtime keeps its fleet key in. `None` keeps it in
    /// owner-only files under the profile.
    fn credentials(&self) -> Option<Arc<dyn crate::model_router::credentials::CredentialStore>> {
        None
    }
    /// The signed-in account's cloud session, with which a desktop issues
    /// and revokes the members' model vault tokens.
    fn cloud_session(&self) -> Option<crate::shared_models::CloudSession> {
        None
    }
    /// Runs an action another member forwarded through the same operation a
    /// client of this runtime reaches, and answers its body.
    fn run_forwarded(&self, _action: &Forwarded) -> Result<Value, String> {
        Err("This runtime cannot run actions from other machines.".into())
    }
}

/// How long a member counts as online after its last heartbeat.
pub const ONLINE_SECONDS: i64 = 60;
/// How long notices and acknowledgements stay in the gossip.
const GOSSIP_TTL_SECONDS: i64 = 24 * 60 * 60;
/// The bytes of events, messages and blobs one exchange carries at most.
const EXCHANGE_BUDGET: usize = 448 * 1024;
/// The most bytes of one exchange body, under the protocol's frame limit.
const FRAME_BUDGET: usize = muniment_attach::MAX_FRAME_LENGTH - 16 * 1024;
/// The most bytes of thread clocks one exchange carries, so events still
/// flow while a new peer pages through many threads.
const CLOCK_BUDGET: usize = 128 * 1024;
/// How often a member sends each peer a hash of all its thread clocks.
const DIGEST_SECONDS: i64 = 10 * 60;
/// An exchange this large, or one that left items out, gets a log line.
const LOG_EXCHANGE_BYTES: usize = 64 * 1024;
/// The record classes the swarm keeps itself, which [`Records`] does not
/// write.
const SWARM_CLASSES: [&str; 3] = [directory::CLASS, groups::CLASS, sync::CLASS];

/// The swarm of one runtime.
#[derive(Clone)]
pub struct Swarm {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Swarm {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Swarm")
            .field("profile", &self.inner.profile)
            .finish()
    }
}

struct Identity {
    /// `None` only for in-process exchange tests.
    access: Option<RemoteAccess>,
    runtime_id: String,
    runtime_name: String,
    runtime_version: String,
    form_factor: Option<String>,
}

pub(crate) struct Inner {
    profile: PathBuf,
    storage: SharedStorage,
    host: Arc<dyn SwarmHost>,
    store: store::Store,
    identity: Mutex<Option<Identity>>,
    /// Serializes merges, scans and local processing.
    merge: Mutex<()>,
    links: Mutex<BTreeMap<String, link::LinkHandle>>,
    /// The replicated records.
    records: records::RecordStore,
    /// What this runtime knows of each peer it exchanges with.
    peers: Mutex<HashMap<String, PeerSync>>,
    /// Speaks only the full-state exchange of members before `store/1`.
    legacy_wire: AtomicBool,
    subscribers: Mutex<Vec<Sender<ClientEvent>>>,
    wake: Mutex<u64>,
    woken: Condvar,
    worker: AtomicBool,
    stopped: AtomicBool,
    offset: Mutex<chrono::Duration>,
    poll: Mutex<Duration>,
    /// The pending gate each live agent run showed at its last read.
    gates: Mutex<HashMap<String, (u64, Option<Value>)>>,
    /// Local agent run changes, which become notices for other members.
    run_changes: Mutex<Option<Receiver<Value>>>,
    /// The activity of each agent this runtime runs, as last sent to clients.
    activities: Mutex<HashMap<String, crate::agent_runs::Activity>>,
    /// Fleet keys, the roster and sealed secrets.
    fleet: fleet::FleetState,
    /// Held while forwarded actions run, so each runs once.
    ops_running: Mutex<()>,
}

/// What one member knows of a peer it exchanges with.
#[derive(Default)]
struct PeerSync {
    /// Whether the peer names `store/1`, once it said.
    store: Option<bool>,
    /// The peer's thread clocks, which pick the events to send it.
    clocks: Clocks,
    /// The peer's thread version up to which this runtime holds its clocks.
    clocks_mark: u64,
    /// The peer's clocks reach its newest thread version.
    clocks_complete: bool,
    /// The peer's clocks were complete once since this runtime last reset
    /// them, so events sent from them are mostly ones the peer lacks.
    clocks_synced: bool,
    /// This runtime's thread version the peer holds, as it last said.
    acked: u64,
    /// The peer's record version vector.
    record_vv: VersionVector,
    /// When this runtime last sent the peer a clock digest.
    digest_at: Option<DateTime<Utc>>,
    /// The content-store objects the peer asked for in its last exchange.
    wants: Vec<BlobWant>,
    stats: PeerStats,
}

/// What a member sent one peer and took in from it, for `swarm.members`
/// and the logs.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct PeerStats {
    pub exchanges: u64,
    /// The bytes of the last exchange this runtime sent the peer.
    pub sent_bytes: u64,
    pub max_sent_bytes: u64,
    pub records_sent: u64,
    pub records_received: u64,
    pub clocks_sent: u64,
    pub clocks_received: u64,
    pub events_sent: u64,
    pub events_received: u64,
    /// The peer trades records and clock deltas.
    pub store: bool,
}

/// How an exchange speaks to a peer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wire {
    /// Records and thread clock deltas.
    Store,
    /// The full state an older member reads.
    Full,
    /// Both, until the peer says which it speaks.
    Both,
}

static REGISTRY: Mutex<Vec<(PathBuf, Weak<Inner>)>> = Mutex::new(Vec::new());

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// An RFC 3339 time in seconds.
pub(crate) fn timestamp(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Secs, true)
}

pub(crate) fn parse_time(time: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(time)
        .ok()
        .map(|time| time.with_timezone(&Utc))
}

impl Swarm {
    /// Opens the swarm of the profile at `profile` and registers it, so the
    /// agent tools of this profile's runs reach it.
    pub fn open(
        profile: &Path,
        storage: SharedStorage,
        host: Arc<dyn SwarmHost>,
    ) -> Result<Self, String> {
        let store = store::Store::open(profile)?;
        let records = records::RecordStore::open(store.clone())?;
        let inner = Arc::new(Inner {
            profile: profile.to_path_buf(),
            storage,
            host,
            store,
            identity: Mutex::new(None),
            merge: Mutex::new(()),
            links: Mutex::new(BTreeMap::new()),
            records,
            peers: Mutex::new(HashMap::new()),
            legacy_wire: AtomicBool::new(false),
            subscribers: Mutex::new(Vec::new()),
            wake: Mutex::new(0),
            woken: Condvar::new(),
            worker: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            offset: Mutex::new(chrono::Duration::zero()),
            poll: Mutex::new(Duration::from_secs(2)),
            gates: Mutex::new(HashMap::new()),
            run_changes: Mutex::new(None),
            activities: Mutex::new(HashMap::new()),
            fleet: fleet::FleetState::default(),
            ops_running: Mutex::new(()),
        });
        absence::initialize(&inner)?;
        fleet::initialize(&inner)?;
        let mut registry = lock(&REGISTRY);
        registry.retain(|(path, swarm)| swarm.strong_count() > 0 && path != profile);
        registry.push((profile.to_path_buf(), Arc::downgrade(&inner)));
        Ok(Self { inner })
    }

    /// The running swarm of a profile, if its runtime opened one.
    pub fn for_profile(profile: &Path) -> Option<Self> {
        lock(&REGISTRY)
            .iter()
            .find(|(path, _)| path == profile)
            .and_then(|(_, swarm)| swarm.upgrade())
            .map(|inner| Self { inner })
    }

    /// Gives the swarm this runtime's identity, starts the links it keeps
    /// and its worker. A swarm without remote access delivers messages
    /// between this runtime's own agents only.
    pub fn attach_access(&self, access: RemoteAccess) -> Result<(), String> {
        let info = access.runtime_info();
        self.inner
            .store
            .set_meta("runtime_id", Some(&info.runtime_id))?;
        *lock(&self.inner.identity) = Some(Identity {
            runtime_id: info.runtime_id,
            runtime_name: info.runtime_name,
            runtime_version: info.runtime_version,
            form_factor: info.form_factor.map(|form| form.as_str().to_owned()),
            access: Some(access.clone()),
        });
        access.set_outbound_links(Arc::new(link::OutboundHook(Arc::downgrade(&self.inner))));
        match access.identity_signer() {
            Ok(signer) => fleet::set_signer(&self.inner, Some(Arc::new(signer))),
            Err(error) => {
                crate::runtime_eprintln!("muniment-runtime: swarm holds no fleet keys: {error}")
            }
        }
        self.inner.migrate_records()?;
        link::start_saved(&self.inner)?;
        self.ensure_worker();
        Ok(())
    }

    /// This runtime's id, once remote access gave it one.
    pub fn runtime_id(&self) -> Option<String> {
        self.inner.runtime_id()
    }

    /// Gives the swarm an identity signer without remote access, as tests
    /// of members without a listener do.
    #[doc(hidden)]
    pub fn set_identity_signer(&self, signer: crate::attach::IdentitySigner) {
        fleet::set_signer(&self.inner, Some(Arc::new(signer)));
    }

    /// Reseals this runtime's own secrets now: its model connections and
    /// MCP tokens to the fleet's full members, and the model vault tokens
    /// it issues. The worker does this about once a minute.
    pub fn fleet_pass(&self) -> Result<(), String> {
        credentials::pass(&self.inner)
    }

    /// Sets how long the worker waits between reseal passes.
    #[doc(hidden)]
    pub fn set_fleet_interval(&self, interval: Duration) {
        credentials::set_interval(&self.inner, interval);
    }

    /// The members a secret that serves `agents` may reach.
    pub fn recipient_set(&self, agents: &[String]) -> Result<RecipientSet, String> {
        fleet::recipient_set(&self.inner, agents)
    }

    /// Sets how long an idle link waits between exchanges.
    pub fn set_poll_interval(&self, poll: Duration) {
        *lock(&self.inner.poll) = poll;
        self.inner.nudge();
    }

    /// The records every member replicates.
    pub fn records(&self) -> Records {
        Records {
            inner: Arc::clone(&self.inner),
        }
    }

    /// Makes this runtime speak only the full-state exchange of members
    /// before `store/1`, as a test of the fallback does.
    #[doc(hidden)]
    pub fn set_legacy_wire(&self, legacy: bool) {
        self.inner.legacy_wire.store(legacy, Ordering::SeqCst);
        lock(&self.inner.peers).clear();
    }

    /// Moves the swarm's clock forward, as a test of time-based behavior does.
    #[doc(hidden)]
    pub fn advance_clock(&self, by: chrono::Duration) {
        *lock(&self.inner.offset) += by;
    }

    /// Starts the worker that runs [`Self::tick`] about once a second.
    pub fn ensure_worker(&self) {
        if self.inner.worker.swap(true, Ordering::SeqCst) {
            return;
        }
        *lock(&self.inner.run_changes) = Some(crate::agent_runs::subscribe(&self.inner.profile));
        let weak = Arc::downgrade(&self.inner);
        std::thread::spawn(move || loop {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            if inner.stopped.load(Ordering::SeqCst) {
                return;
            }
            if let Err(error) = inner.tick() {
                crate::runtime_eprintln!("muniment-runtime: swarm tick failed: {error}");
            }
            credentials::maybe_pass(&inner);
            let guard = lock(&inner.wake);
            let _ = inner.woken.wait_timeout(guard, Duration::from_millis(500));
        });
    }

    /// Stops the worker and every link.
    pub fn stop(&self) {
        self.inner.stopped.store(true, Ordering::SeqCst);
        link::stop_all(&self.inner);
        self.inner.nudge();
    }

    /// One pass of local work: the directory, thread registrations and the
    /// replication index, posted messages the primary consumes, message
    /// delivery and automatic results, approvals, notices, moves and the
    /// absence pause.
    pub fn tick(&self) -> Result<(), String> {
        self.inner.tick()
    }

    /// Subscribes to the swarm's client events.
    pub fn subscribe(&self) -> Receiver<ClientEvent> {
        let (sender, receiver) = mpsc::channel();
        lock(&self.inner.subscribers).push(sender);
        receiver
    }

    /// Records that a client used this runtime.
    pub fn note_client_activity(&self) {
        if let Err(error) = absence::note_client(&self.inner) {
            crate::runtime_eprintln!("muniment-runtime: swarm client activity: {error}");
        }
    }

    /// When a client last used this runtime, as an RFC 3339 time.
    pub fn last_client_activity(&self) -> Option<String> {
        self.inner.store.meta("last_client_at").ok().flatten()
    }

    /// Answers a linked runtime's `swarm.exchange`.
    pub fn exchange(&self, device_id: &str, body: Value) -> Result<Value, String> {
        link::answer_exchange(&self.inner, device_id, body)
    }

    /// Runs one swarm operation for a client and answers its body.
    pub fn operation(
        &self,
        operation: Operation,
        body: Value,
        client: Option<&str>,
    ) -> Result<Value, String> {
        let inner = &self.inner;
        match operation {
            Operation::SwarmInvite => link::invite(inner, body),
            Operation::SwarmJoin => link::join(inner, body),
            Operation::SwarmLeave => link::leave(inner, body),
            Operation::SwarmMembers => {
                empty(&body)?;
                link::members_view(inner)
            }
            Operation::SwarmAgents => {
                empty(&body)?;
                let mut agents = directory::entries(inner, false)?;
                for agent in &mut agents {
                    agent.move_suggested = ops::move_suggested(inner, &agent.primary_runtime);
                }
                Ok(json!({"agents": agents}))
            }
            Operation::SwarmResume => {
                empty(&body)?;
                absence::resume(inner)
            }
            Operation::FleetHoldSignIn => credentials::hold_operation(inner, body),
            Operation::FleetCredentials => credentials::credentials_operation(inner, body),
            Operation::ThreadPost => sync::post_operation(inner, body),
            Operation::AgentMove => moves::move_operation(inner, body),
            Operation::AgentApprovals => {
                empty(&body)?;
                approvals::list(inner)
            }
            Operation::AgentApprovalAnswer => approvals::answer_operation(inner, body, client),
            Operation::GroupCreate => groups::create(inner, body),
            Operation::GroupList => {
                empty(&body)?;
                groups::list(inner)
            }
            Operation::GroupUpdate => groups::update(inner, body),
            Operation::FleetOpStatus => ops::status(inner, body),
            _ => Err("The swarm operation is unknown.".into()),
        }
    }

    /// Where a client's call goes: `None` runs it here. When the item it
    /// acts on belongs to another member and the operation is forwardable,
    /// the action goes there and the answer carries `forwarded` and `op_id`.
    pub fn route(
        &self,
        operation: Operation,
        body: &Value,
        actor: &FleetActor,
        idempotency_key: Option<&str>,
    ) -> Result<Option<Value>, String> {
        ops::route(&self.inner, operation, body, actor, idempotency_key)
    }

    /// Runs the `agent-message` or `agent-handoff` tool for the run `run_id`
    /// in `thread_id`.
    pub fn agent_tool(
        &self,
        run_id: &str,
        thread_id: &str,
        tool: &str,
        arguments: Value,
    ) -> Result<Value, String> {
        messaging::tool(&self.inner, run_id, thread_id, tool, arguments)
    }

    /// Whether `thread_id` replicates an agent or group thread that another
    /// member runs, so this runtime writes no run output into it.
    pub fn is_replica_thread(&self, thread_id: &str) -> Option<String> {
        sync::replica_primary(&self.inner, thread_id)
    }

    /// Asks the members for the content-store objects a thread's events name
    /// and this runtime lacks, as a client opens the thread.
    pub fn want_thread_blobs(&self, thread_id: &str) {
        if let Err(error) = sync::want_thread_blobs(&self.inner, thread_id) {
            crate::runtime_eprintln!("muniment-runtime: swarm blob request: {error}");
        }
    }

    /// The approvals other members hold, as chat events the phone relay
    /// publishes: a waiting gate, then a finished run once it is answered.
    pub fn phone_events(&self) -> Receiver<crate::run_events::ChatEvent> {
        let events = self.subscribe();
        let own = self.inner.runtime_id();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(event) = events.recv() {
                if !matches!(
                    event.name,
                    EventName::AgentApprovalPending | EventName::AgentApprovalResolved
                ) {
                    continue;
                }
                let Ok(approval) = serde_json::from_value::<Approval>(event.body) else {
                    continue;
                };
                if Some(&approval.runtime_id) == own.as_ref() {
                    continue;
                }
                if let Some(chat) = approvals::phone_event(&approval) {
                    if sender.send(chat).is_err() {
                        return;
                    }
                }
            }
        });
        receiver
    }

    /// Answers a gate another member holds, as the phone relay answers one.
    pub fn answer_gate(
        &self,
        run_id: &str,
        gate_id: &str,
        answer: &ChatPermissionAnswer,
    ) -> Result<Value, String> {
        let body = json!({
            "approval_id": format!("{run_id}:{gate_id}"),
            "answer": serde_json::to_value(answer).map_err(|error| error.to_string())?,
        });
        if let Some(answer) = self.route(
            Operation::AgentApprovalAnswer,
            &body,
            &FleetActor::phone(),
            None,
        )? {
            return Ok(answer);
        }
        self.operation(Operation::AgentApprovalAnswer, body, None)
    }

    /// The summary of the approval the run `run_id` waits at, if any.
    pub fn approval_summary(&self, run_id: &str) -> Option<String> {
        approvals::pending_summary(&self.inner, run_id)
    }

    /// Runs one exchange on every connected outbound link now, for tests.
    #[doc(hidden)]
    pub fn sync_now(&self) {
        link::exchange_all_now(&self.inner);
    }
}

/// The records every member replicates, for the classes other parts of core
/// keep on them. Each field is a last-writer-wins register ordered by a
/// hybrid logical clock; a set holds elements that a concurrent add keeps.
/// The swarm's own classes (`agent`, `group`, `thread`) are read-only here.
#[derive(Clone)]
pub struct Records {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Records {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Records").finish_non_exhaustive()
    }
}

impl Records {
    fn writable(&self, class: &str) -> Result<String, String> {
        if SWARM_CLASSES.contains(&class) {
            return Err("The swarm keeps this record class itself.".into());
        }
        self.inner.origin()
    }

    fn written(&self, change: Option<RecordChange>) -> bool {
        if change.is_some() {
            self.inner.nudge();
        }
        change.is_some()
    }

    /// Sets a field. Answers whether the record changed.
    pub fn put(&self, class: &str, key: &str, field: &str, value: Value) -> Result<bool, String> {
        let origin = self.writable(class)?;
        let change =
            self.inner
                .records
                .put(&origin, self.inner.now(), class, key, field, value, 0)?;
        Ok(self.written(change))
    }

    /// Removes a field.
    pub fn remove(&self, class: &str, key: &str, field: &str) -> Result<bool, String> {
        let origin = self.writable(class)?;
        let change =
            self.inner
                .records
                .remove_field(&origin, self.inner.now(), class, key, field, 0)?;
        Ok(self.written(change))
    }

    /// Deletes a record. Its tombstone wins over every older write and lasts
    /// [`TOMBSTONE_DAYS`].
    pub fn delete(&self, class: &str, key: &str) -> Result<bool, String> {
        let origin = self.writable(class)?;
        let change = self
            .inner
            .records
            .delete(&origin, self.inner.now(), class, key, 0, None)?;
        Ok(self.written(change))
    }

    /// Adds an element to a set field.
    pub fn set_add(
        &self,
        class: &str,
        key: &str,
        set: &str,
        element: &str,
        value: Value,
    ) -> Result<bool, String> {
        let origin = self.writable(class)?;
        let change = self.inner.records.set_add(
            &origin,
            self.inner.now(),
            class,
            key,
            set,
            element,
            value,
        )?;
        Ok(self.written(change))
    }

    /// Removes an element from a set field. An add another member made at
    /// the same time keeps it.
    pub fn set_remove(
        &self,
        class: &str,
        key: &str,
        set: &str,
        element: &str,
    ) -> Result<bool, String> {
        let origin = self.writable(class)?;
        let change =
            self.inner
                .records
                .set_remove(&origin, self.inner.now(), class, key, set, element)?;
        Ok(self.written(change))
    }

    pub fn get(&self, class: &str, key: &str) -> Result<Option<Record>, String> {
        self.inner.records.get(class, key)
    }

    /// The records of a class in key order after `after`, at most `limit`.
    pub fn scan(
        &self,
        class: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Record>, String> {
        self.inner.records.scan(class, after, limit)
    }

    /// Every change to a record from now on, local or from another member.
    pub fn subscribe(&self) -> Receiver<RecordChange> {
        self.inner.records.subscribe()
    }

    /// When this runtime found it was away past [`TOMBSTONE_DAYS`] and so
    /// needs a fresh snapshot of the records.
    pub fn snapshot_needed_at(&self) -> Option<String> {
        self.inner.store.meta("snapshot_needed_at").ok().flatten()
    }
}

fn empty(body: &Value) -> Result<(), String> {
    if body.as_object().is_some_and(serde_json::Map::is_empty) {
        Ok(())
    } else {
        Err("The request is invalid.".into())
    }
}

impl Inner {
    pub(crate) fn now(&self) -> DateTime<Utc> {
        Utc::now() + *lock(&self.offset)
    }

    pub(crate) fn now_text(&self) -> String {
        timestamp(self.now())
    }

    pub(crate) fn runtime_id(&self) -> Option<String> {
        lock(&self.identity)
            .as_ref()
            .map(|identity| identity.runtime_id.clone())
    }

    /// This runtime's machine name, once remote access gave it one.
    pub(crate) fn runtime_name(&self) -> Option<String> {
        lock(&self.identity)
            .as_ref()
            .map(|identity| identity.runtime_name.clone())
            .filter(|name| !name.is_empty())
    }

    /// The member id this runtime writes records under.
    pub(crate) fn origin(&self) -> Result<String, String> {
        match self.runtime_id() {
            Some(id) => Ok(id),
            None => self
                .store
                .meta("runtime_id")?
                .ok_or_else(|| "Turn on remote access to share records.".to_owned()),
        }
    }

    fn legacy(&self) -> bool {
        self.legacy_wire.load(Ordering::SeqCst)
    }

    /// Moves the directory, groups and thread registrations a store held
    /// before records onto records.
    fn migrate_records(&self) -> Result<(), String> {
        let _merge = lock(&self.merge);
        directory::migrate(self)?;
        groups::migrate(self)?;
        sync::migrate(self)
    }

    /// Lets the swarm's own classes react to records another member wrote.
    fn dispatch(&self, changes: &[RecordChange]) -> Result<(), String> {
        if changes.is_empty() {
            return Ok(());
        }
        if sync::on_changes(self, changes)? {
            self.changed("threads");
        }
        if directory::on_changes(self, changes) {
            self.changed("agents");
        }
        if groups::on_changes(changes) {
            self.changed("groups");
        }
        Ok(())
    }

    /// How this runtime speaks to `peer`.
    fn wire_for(&self, peer: &str) -> Result<Wire, String> {
        if self.legacy() {
            return Ok(Wire::Full);
        }
        let known = lock(&self.peers).get(peer).and_then(|sync| sync.store);
        let known = match known {
            Some(known) => Some(known),
            None => self
                .store
                .meta(&format!("peer_store:{peer}"))?
                .map(|saved| saved == "1"),
        };
        Ok(match known {
            Some(true) => Wire::Store,
            Some(false) => Wire::Full,
            None => Wire::Both,
        })
    }

    fn access(&self) -> Option<RemoteAccess> {
        lock(&self.identity)
            .as_ref()
            .and_then(|identity| identity.access.clone())
    }

    /// Wakes the worker and every idle link.
    pub(crate) fn nudge(&self) {
        *lock(&self.wake) += 1;
        self.woken.notify_all();
    }

    pub(crate) fn emit(&self, name: EventName, body: Value) {
        lock(&self.subscribers).retain(|subscriber| {
            subscriber
                .send(ClientEvent {
                    name: name.clone(),
                    body: body.clone(),
                })
                .is_ok()
        });
    }

    pub(crate) fn changed(&self, kind: &str) {
        self.emit(EventName::SwarmChanged, json!({"kind": kind}));
    }

    fn tick(&self) -> Result<(), String> {
        let mut kick = false;
        {
            let _merge = lock(&self.merge);
            type Step<'a> = (&'a str, &'a dyn Fn() -> Result<bool, String>);
            // Approvals come before activity, so a waiting agent's activity
            // names its approval rather than a bare wait.
            let steps: [Step<'_>; 11] = [
                ("approvals", &|| approvals::scan(self).map(|()| false)),
                ("activity", &|| directory::refresh_activity(self)),
                ("directory", &|| directory::refresh_local(self)),
                ("threads", &|| sync::register_local(self).map(|()| false)),
                ("index", &|| sync::scan_local(self).map(|()| false)),
                ("posts", &|| sync::consume_posts(self)),
                ("messages", &|| messaging::deliver_pending(self)),
                ("results", &|| messaging::send_results(self)),
                ("moves", &|| moves::activate_pending(self)),
                ("absence", &|| absence::check(self).map(|()| false)),
                ("fleet", &|| credentials::settle(self).map(|()| false)),
            ];
            for (name, step) in steps {
                match step() {
                    Ok(started) => kick |= started,
                    Err(error) => {
                        crate::runtime_eprintln!("muniment-runtime: swarm {name} failed: {error}")
                    }
                }
            }
            if let Err(error) = approvals::forward_run_changes(self) {
                crate::runtime_eprintln!("muniment-runtime: swarm notices failed: {error}");
            }
        }
        // Forwarded actions run outside the merge: the operations they reach
        // take it themselves.
        for (name, step) in [
            (
                "ops",
                ops::execute_pending as fn(&Self) -> Result<(), String>,
            ),
            ("op states", ops::refresh),
            ("op results", ops::forget_old_results),
        ] {
            if let Err(error) = step(self) {
                crate::runtime_eprintln!("muniment-runtime: swarm {name} failed: {error}");
            }
        }
        if kick {
            self.host.kick_runs();
            self.nudge();
        }
        Ok(())
    }

    /// The exchange this runtime sends `peer`. Approvals, acknowledgements,
    /// notices and messages go first, then records, then thread clocks and
    /// events, then content-store objects, within the exchange budget.
    fn build_exchange(&self, peer: &str) -> Result<Exchange, String> {
        let from = link::self_record(self)?;
        let wire = self.wire_for(peer)?;
        let now = self.now();
        let (peer_vv, acked, held_mark, peer_clocks, clocks_ready, digest_due, serve) = {
            let mut peers = lock(&self.peers);
            let sync = peers.entry(peer.to_owned()).or_default();
            (
                sync.record_vv.clone(),
                sync.acked,
                sync.clocks_mark,
                sync.clocks.clone(),
                wire != Wire::Store || sync.clocks_synced,
                sync.digest_at
                    .is_none_or(|at| (now - at).num_seconds() >= DIGEST_SECONDS),
                std::mem::take(&mut sync.wants),
            )
        };
        let acks = messaging::recent_acks(self)?;
        let notices = approvals::recent_notices(self)?;
        let approvals = approvals::recent(self)?;
        let mut budget = EXCHANGE_BUDGET
            .saturating_sub(json_bytes(&acks) + json_bytes(&notices) + json_bytes(&approvals));
        let (messages, more_messages) = messaging::outgoing(self, peer, &mut budget)?;
        let (records, record_vv, more_records) = if wire == Wire::Full {
            (Vec::new(), None, false)
        } else {
            let (rows, more) = self.records.delta(&peer_vv, &mut budget)?;
            (rows, Some(self.records.version_vector()?), more)
        };
        let (clocks, watermarks, more_clocks) = match wire {
            Wire::Full => (sync::clocks(self)?, None, false),
            Wire::Both => {
                let clocks = sync::clocks(self)?;
                let watermarks = Watermarks {
                    base: 0,
                    mark: sync::clock_version(self)?,
                    complete: true,
                    ack: held_mark,
                    digest: None,
                };
                (clocks, Some(watermarks), false)
            }
            Wire::Store => {
                let mut clock_budget = budget.min(CLOCK_BUDGET);
                let before = clock_budget;
                let (delta, mark, complete) = sync::clock_delta(self, acked, &mut clock_budget)?;
                budget -= before - clock_budget;
                let digest = if complete && digest_due {
                    lock(&self.peers)
                        .entry(peer.to_owned())
                        .or_default()
                        .digest_at = Some(now);
                    Some(sync::clock_digest(&sync::clocks(self)?))
                } else {
                    None
                };
                let watermarks = Watermarks {
                    base: acked,
                    mark,
                    complete,
                    ack: held_mark,
                    digest,
                };
                (delta, Some(watermarks), !complete)
            }
        };
        let (events, more_events) = if clocks_ready {
            sync::export(self, &peer_clocks, &mut budget)?
        } else {
            (Vec::new(), true)
        };
        let (blobs, wants) = sync::blob_traffic(self, &serve, &mut budget)?;
        let fleet = fleet::outgoing(self, peer, &mut budget).unwrap_or_else(|error| {
            crate::runtime_eprintln!("muniment-runtime: swarm fleet exchange failed: {error}");
            fleet::Outgoing::default()
        });
        let full = wire != Wire::Store;
        let mut exchange = Exchange {
            members: link::member_records(self)?,
            directory: if full {
                directory::entries(self, true)?
            } else {
                Vec::new()
            },
            groups: if full { groups::all(self)? } else { Vec::new() },
            threads: if full {
                sync::registrations(self)?
            } else {
                Vec::new()
            },
            clocks,
            events,
            messages,
            acks,
            notices,
            approvals,
            wants,
            blobs,
            client_seen_at: absence::swarm_last_client(self)?,
            resumed_at: self.store.meta("resumed_at")?,
            more: more_events || more_messages || more_records || more_clocks || fleet.more,
            records,
            record_vv,
            watermarks,
            roster: fleet.roster,
            roster_digest: fleet.roster_digest,
            sealed: fleet.sealed,
            sealed_have: fleet.sealed_have,
            admission: fleet.admission,
            from,
        };
        // A frame never grows past the protocol's limit: what does not fit
        // waits for the next exchange.
        let length = |exchange: &Exchange| {
            serde_json::to_vec(exchange).map_or(usize::MAX, |bytes| bytes.len())
        };
        let mut bytes = length(&exchange);
        if bytes > FRAME_BUDGET {
            exchange.blobs.clear();
            exchange.events.clear();
            exchange.sealed.clear();
            exchange.more = true;
            bytes = length(&exchange);
        }
        if bytes > FRAME_BUDGET {
            exchange.messages.truncate(8);
            exchange.notices.clear();
            bytes = length(&exchange);
        }
        if bytes > FRAME_BUDGET && full {
            // An older member's full state outgrew the frame; it gets the
            // directory and groups now and threads later.
            exchange.threads.clear();
            exchange.clocks.clear();
            if let Some(watermarks) = &mut exchange.watermarks {
                watermarks.mark = 0;
                watermarks.complete = false;
            }
            exchange.more = true;
            bytes = length(&exchange);
        }
        self.note_sent(peer, wire, &exchange, bytes);
        Ok(exchange)
    }

    /// Counts what an exchange carried and logs a large one.
    fn note_sent(&self, peer: &str, wire: Wire, exchange: &Exchange, bytes: usize) {
        let summary = {
            let mut peers = lock(&self.peers);
            let stats = &mut peers.entry(peer.to_owned()).or_default().stats;
            stats.exchanges += 1;
            stats.sent_bytes = bytes as u64;
            stats.max_sent_bytes = stats.max_sent_bytes.max(bytes as u64);
            stats.records_sent += exchange.records.len() as u64;
            stats.clocks_sent += exchange.clocks.len() as u64;
            stats.events_sent += exchange.events.len() as u64;
            stats.store = wire == Wire::Store;
            exchange
                .watermarks
                .as_ref()
                .and_then(|watermarks| watermarks.digest.as_ref())
                .map(|_| stats.clone())
        };
        if bytes >= LOG_EXCHANGE_BYTES || exchange.more {
            crate::runtime_eprintln!(
                "muniment-runtime: swarm exchange peer={peer} wire={wire:?} bytes={bytes} records={} clocks={} events={} messages={} more={}",
                exchange.records.len(),
                exchange.clocks.len(),
                exchange.events.len(),
                exchange.messages.len(),
                exchange.more
            );
        }
        if let Some(stats) = summary {
            let counts = self.records.counts().unwrap_or_default();
            let records: Vec<String> = counts
                .iter()
                .map(|(class, count)| format!("{class}:{}/{}", count.rows, count.tombstones))
                .collect();
            crate::runtime_eprintln!(
                "muniment-runtime: swarm sync peer={peer} exchanges={} last_bytes={} max_bytes={} records_sent={} records_received={} clocks_sent={} events_sent={} records={}",
                stats.exchanges,
                stats.sent_bytes,
                stats.max_sent_bytes,
                stats.records_sent,
                stats.records_received,
                stats.clocks_sent,
                stats.events_sent,
                records.join(",")
            );
        }
    }

    /// What this runtime sent and took in per peer, and its record counts.
    fn sync_view(&self) -> Result<Value, String> {
        let peers: BTreeMap<String, PeerStats> = lock(&self.peers)
            .iter()
            .map(|(peer, sync)| (peer.clone(), sync.stats.clone()))
            .collect();
        Ok(json!({
            "records": self.records.counts()?,
            "clock": self.records.clock(),
            "peers": peers,
            "snapshot_needed_at": self.store.meta("snapshot_needed_at")?,
        }))
    }

    /// Notes that this runtime heard from the swarm, and marks it as needing
    /// a fresh snapshot when it was away past the tombstone lifetime.
    fn note_synced(&self) -> Result<(), String> {
        let now = self.now();
        let last = self
            .store
            .meta("last_sync_at")?
            .as_deref()
            .and_then(parse_time);
        if last.is_some_and(|last| (now - last).num_days() >= TOMBSTONE_DAYS) {
            self.store
                .set_meta("snapshot_needed_at", Some(&timestamp(now)))?;
            crate::runtime_eprintln!(
                "muniment-runtime: swarm member was away past {TOMBSTONE_DAYS} days and needs a fresh snapshot"
            );
        }
        if last.is_none_or(|last| (now - last).num_seconds() >= ONLINE_SECONDS) {
            self.store.set_meta("last_sync_at", Some(&timestamp(now)))?;
        }
        Ok(())
    }

    /// Takes in what `peer` sent.
    fn merge_exchange(&self, peer: &str, mut exchange: Exchange) -> Result<bool, String> {
        let _merge = lock(&self.merge);
        let mut kick = false;
        let store = !self.legacy()
            && exchange
                .from
                .capabilities
                .iter()
                .any(|capability| capability == STORE_CAPABILITY);
        self.note_peer_wire(peer, store)?;
        self.note_synced()?;
        let fleet = fleet::Incoming {
            roster: std::mem::take(&mut exchange.roster),
            roster_digest: exchange.roster_digest.take(),
            sealed: std::mem::take(&mut exchange.sealed),
            sealed_have: exchange.sealed_have.take(),
            admission: exchange.admission.take(),
        };
        match fleet::merge(self, peer, &exchange.from, fleet) {
            Ok(true) => {
                if let Err(error) = credentials::settle(self) {
                    crate::runtime_eprintln!("muniment-runtime: swarm fleet failed: {error}");
                }
            }
            Ok(false) => {}
            Err(error) => {
                crate::runtime_eprintln!("muniment-runtime: swarm fleet merge failed: {error}")
            }
        }
        link::merge_members(self, &exchange.from, &exchange.members)?;
        let records_received = exchange.records.len() as u64;
        if store {
            let changes = self.records.merge(self.now(), exchange.records)?;
            self.dispatch(&changes)?;
        } else {
            sync::merge_registrations(self, &exchange.threads)?;
            if directory::merge_full(self, exchange.directory)? {
                self.changed("agents");
            }
            if groups::merge_full(self, exchange.groups)? {
                self.changed("groups");
            }
        }
        let watermarks = exchange.watermarks.filter(|_| store);
        let clocks_received = exchange.clocks.len() as u64;
        let digest_failed = {
            let mut peers = lock(&self.peers);
            let sync = peers.entry(peer.to_owned()).or_default();
            sync.stats.records_received += records_received;
            sync.stats.clocks_received += clocks_received;
            sync.stats.events_received += exchange.events.len() as u64;
            if let Some(record_vv) = exchange.record_vv.filter(|_| store) {
                sync.record_vv = record_vv;
            }
            sync.merge_clocks(exchange.clocks, watermarks.as_ref())
        };
        if digest_failed {
            crate::runtime_eprintln!(
                "muniment-runtime: swarm clock digest differs peer={peer}; asking for every thread clock again"
            );
        }
        sync::import(self, exchange.events)?;
        messaging::merge_acks(self, exchange.acks)?;
        kick |= messaging::receive(self, exchange.messages)?;
        approvals::merge_notices(self, exchange.notices)?;
        approvals::merge(self, exchange.approvals)?;
        sync::receive_blobs(self, exchange.blobs)?;
        lock(&self.peers).entry(peer.to_owned()).or_default().wants = exchange.wants;
        absence::merge(self, exchange.client_seen_at, exchange.resumed_at)?;
        kick |= sync::consume_posts(self)?;
        Ok(kick)
    }

    /// Keeps whether `peer` names `store/1`, so the next exchange after a
    /// restart speaks the same way.
    fn note_peer_wire(&self, peer: &str, store: bool) -> Result<(), String> {
        let changed = {
            let mut peers = lock(&self.peers);
            let sync = peers.entry(peer.to_owned()).or_default();
            let changed = sync.store != Some(store);
            sync.store = Some(store);
            changed
        };
        if changed && !self.legacy() {
            self.store.set_meta(
                &format!("peer_store:{peer}"),
                Some(if store { "1" } else { "0" }),
            )?;
        }
        Ok(())
    }
}

impl PeerSync {
    /// Takes in a peer's thread clocks: the full map from an older member,
    /// or the threads changed since this runtime's watermark. Answers
    /// whether a digest showed the held copy drifted, which resets it.
    fn merge_clocks(&mut self, clocks: Clocks, watermarks: Option<&Watermarks>) -> bool {
        let Some(watermarks) = watermarks else {
            self.clocks = clocks;
            self.clocks_complete = true;
            self.clocks_synced = true;
            return false;
        };
        self.acked = watermarks.ack;
        for (thread, origins) in clocks {
            let held = self.clocks.entry(thread).or_default();
            for (origin, seq) in origins {
                let entry = held.entry(origin).or_default();
                *entry = (*entry).max(seq);
            }
        }
        if watermarks.base <= self.clocks_mark {
            self.clocks_mark = self.clocks_mark.max(watermarks.mark);
            self.clocks_complete = watermarks.complete && self.clocks_mark == watermarks.mark;
        } else {
            self.clocks_complete = false;
        }
        let drifted = self.clocks_complete
            && watermarks
                .digest
                .as_ref()
                .is_some_and(|digest| *digest != sync::clock_digest(&self.clocks));
        if drifted {
            self.clocks.clear();
            self.clocks_mark = 0;
            self.clocks_complete = false;
            self.clocks_synced = false;
        }
        self.clocks_synced |= self.clocks_complete;
        drifted
    }
}

/// The serialized size of a value, for the exchange budget.
fn json_bytes<T: serde::Serialize>(value: &T) -> usize {
    serde_json::to_vec(value).map_or(0, |bytes| bytes.len())
}
