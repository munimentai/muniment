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
//!
//! The runtime supplies what only it can do through [`SwarmHost`].

mod absence;
mod approvals;
mod directory;
mod groups;
mod link;
mod messaging;
mod moves;
mod store;
mod sync;
pub mod wire;

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
}

/// How long a member counts as online after its last heartbeat.
pub const ONLINE_SECONDS: i64 = 60;
/// How long notices and acknowledgements stay in the gossip.
const GOSSIP_TTL_SECONDS: i64 = 24 * 60 * 60;
/// The bytes of events, messages and blobs one exchange carries at most.
const EXCHANGE_BUDGET: usize = 448 * 1024;
/// The most bytes of one exchange body, under the protocol's frame limit.
const FRAME_BUDGET: usize = muniment_attach::MAX_FRAME_LENGTH - 16 * 1024;

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
    access: RemoteAccess,
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
    /// Each peer's clocks from its last exchange, which pick what to send it.
    peer_clocks: Mutex<HashMap<String, Clocks>>,
    /// The content-store objects each peer asked for in its last exchange.
    peer_wants: Mutex<HashMap<String, Vec<BlobWant>>>,
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
        let inner = Arc::new(Inner {
            profile: profile.to_path_buf(),
            storage,
            host,
            store,
            identity: Mutex::new(None),
            merge: Mutex::new(()),
            links: Mutex::new(BTreeMap::new()),
            peer_clocks: Mutex::new(HashMap::new()),
            peer_wants: Mutex::new(HashMap::new()),
            subscribers: Mutex::new(Vec::new()),
            wake: Mutex::new(0),
            woken: Condvar::new(),
            worker: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            offset: Mutex::new(chrono::Duration::zero()),
            poll: Mutex::new(Duration::from_secs(2)),
            gates: Mutex::new(HashMap::new()),
            run_changes: Mutex::new(None),
        });
        absence::initialize(&inner)?;
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
            access: access.clone(),
        });
        access.set_outbound_links(Arc::new(link::OutboundHook(Arc::downgrade(&self.inner))));
        link::start_saved(&self.inner)?;
        self.ensure_worker();
        Ok(())
    }

    /// This runtime's id, once remote access gave it one.
    pub fn runtime_id(&self) -> Option<String> {
        self.inner.runtime_id()
    }

    /// Sets how long an idle link waits between exchanges.
    pub fn set_poll_interval(&self, poll: Duration) {
        *lock(&self.inner.poll) = poll;
        self.inner.nudge();
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
                Ok(json!({"agents": directory::entries(inner, false)?}))
            }
            Operation::SwarmResume => {
                empty(&body)?;
                absence::resume(inner)
            }
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
            _ => Err("The swarm operation is unknown.".into()),
        }
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
        self.operation(
            Operation::AgentApprovalAnswer,
            json!({
                "approval_id": format!("{run_id}:{gate_id}"),
                "answer": serde_json::to_value(answer).map_err(|error| error.to_string())?,
            }),
            None,
        )
    }

    /// Runs one exchange on every connected outbound link now, for tests.
    #[doc(hidden)]
    pub fn sync_now(&self) {
        link::exchange_all_now(&self.inner);
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

    fn access(&self) -> Option<RemoteAccess> {
        lock(&self.identity)
            .as_ref()
            .map(|identity| identity.access.clone())
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
            let steps: [Step<'_>; 9] = [
                ("directory", &|| directory::refresh_local(self)),
                ("threads", &|| sync::register_local(self).map(|()| false)),
                ("index", &|| sync::scan_local(self).map(|()| false)),
                ("posts", &|| sync::consume_posts(self)),
                ("messages", &|| messaging::deliver_pending(self)),
                ("results", &|| messaging::send_results(self)),
                ("approvals", &|| approvals::scan(self).map(|()| false)),
                ("moves", &|| moves::activate_pending(self)),
                ("absence", &|| absence::check(self).map(|()| false)),
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
        if kick {
            self.host.kick_runs();
            self.nudge();
        }
        Ok(())
    }

    /// The exchange this runtime sends `peer`, whose clocks it last saw.
    fn build_exchange(&self, peer: &str, peer_clocks: Option<&Clocks>) -> Result<Exchange, String> {
        let from = link::self_record(self)?;
        let mut budget = EXCHANGE_BUDGET;
        let (events, more_events) =
            sync::export(self, peer_clocks.cloned().unwrap_or_default(), &mut budget)?;
        let (messages, more_messages) = messaging::outgoing(self, peer, &mut budget)?;
        let serve = lock(&self.peer_wants).remove(peer).unwrap_or_default();
        let (blobs, wants) = sync::blob_traffic(self, &serve, &mut budget)?;
        let mut exchange = Exchange {
            members: link::member_records(self)?,
            directory: directory::entries(self, true)?,
            groups: groups::all(self)?,
            threads: sync::registrations(self)?,
            clocks: sync::clocks(self)?,
            events,
            messages,
            acks: messaging::recent_acks(self)?,
            notices: approvals::recent_notices(self)?,
            approvals: approvals::recent(self)?,
            wants,
            blobs,
            client_seen_at: absence::swarm_last_client(self)?,
            resumed_at: self.store.meta("resumed_at")?,
            more: more_events || more_messages,
            from,
        };
        // A frame never grows past the protocol's limit: what does not fit
        // waits for the next exchange.
        let fits = |exchange: &Exchange| {
            serde_json::to_vec(exchange).is_ok_and(|bytes| bytes.len() <= FRAME_BUDGET)
        };
        if !fits(&exchange) {
            exchange.blobs.clear();
            exchange.events.clear();
            exchange.more = true;
        }
        if !fits(&exchange) {
            exchange.messages.truncate(8);
            exchange.notices.clear();
        }
        Ok(exchange)
    }

    /// Takes in what `peer` sent.
    fn merge_exchange(&self, peer: &str, exchange: Exchange) -> Result<bool, String> {
        let _merge = lock(&self.merge);
        let mut kick = false;
        link::merge_members(self, &exchange.from, &exchange.members)?;
        sync::merge_registrations(self, &exchange.threads)?;
        if directory::merge(self, exchange.directory)? {
            self.changed("agents");
        }
        if groups::merge(self, exchange.groups)? {
            self.changed("groups");
        }
        lock(&self.peer_clocks).insert(peer.to_owned(), exchange.clocks);
        sync::import(self, exchange.events)?;
        messaging::merge_acks(self, exchange.acks)?;
        kick |= messaging::receive(self, exchange.messages)?;
        approvals::merge_notices(self, exchange.notices)?;
        approvals::merge(self, exchange.approvals)?;
        sync::receive_blobs(self, exchange.blobs)?;
        lock(&self.peer_wants).insert(peer.to_owned(), exchange.wants);
        absence::merge(self, exchange.client_seen_at, exchange.resumed_at)?;
        kick |= sync::consume_posts(self)?;
        Ok(kick)
    }
}
