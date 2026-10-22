#![cfg(unix)]
//! Runtimes linked into a swarm over loopback: links and their revocation,
//! the agent directory and groups on replicated records, thread replication
//! on the journal, moves, agent messaging, group chats, approvals across
//! members, the absence pause and actions forwarded to an item's owner.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use muniment_core::agent_hooks::HookEndpoints;
use muniment_core::agent_runs::{NewRun, Run, RunStatus, RunStore, RunTrigger, TriggerKind};
use muniment_core::agents::{self, Agent, AgentMessaging, MessagingAllow, Routine};
use muniment_core::attach::{
    connect_remote_runtime, pair_with_link, DesktopClient, ErrorCode, InstallKind, Operation,
    PairingLinkRequest, RemoteAccess, RemoteAccessOptions, RemoteClientOptions, RemoteListener,
    RemoteListenerConfig, RemoteListenerError, RemoteRuntimeRecord, SubscriptionEvent,
};
use muniment_core::run_start::RunAttachBoundaries;
use muniment_core::serde_json::{json, Value};
use muniment_core::sidecar::pi_install::PiArtifactDescriptor;
use muniment_core::swarm::{FleetActor, Swarm};
use muniment_runtime_core::host::{
    AccountService, AccountSession, CredentialStore, FileCredentialStore,
};
use muniment_runtime_core::{RuntimeAttachState, RuntimeHost};

mod common;
use common::TemporaryProfile;

/// The tests share process-wide stub settings, so they run one at a time.
static ENVIRONMENT: Mutex<()> = Mutex::new(());

const STUB_SETTINGS: [&str; 5] = [
    "PI_RESUME_STUB_AGENT_GATE",
    "PI_RESUME_STUB_STEER_CAPTURE",
    "PI_RESUME_STUB_PERMISSION_CAPTURE",
    "PI_RESUME_STUB_REPLY",
    "PI_RESUME_STUB_PROMPTS",
];

struct Member {
    name: &'static str,
    temporary: TemporaryProfile,
    state: Option<Arc<RuntimeAttachState>>,
    access: Option<RemoteAccess>,
    address: Option<SocketAddr>,
    stop: Option<mpsc::Sender<()>>,
    server: Option<std::thread::JoinHandle<Result<(), RemoteListenerError>>>,
    artifact: PiArtifactDescriptor,
    /// A client that reads threads, kept so reads do not pair a device each.
    reader: Mutex<Option<DesktopClient>>,
    account: Option<Arc<dyn AccountService>>,
}

impl Member {
    fn new(name: &'static str) -> Self {
        Self::with_account(name, None)
    }

    /// A member whose host signs in with `account`, as a desktop host does.
    fn with_account(name: &'static str, account: Option<Arc<dyn AccountService>>) -> Self {
        let temporary = TemporaryProfile::new(&format!("swarm-{name}"), false);
        let profile = temporary.profile.clone();
        muniment_core::home::confirm_home(&profile, &temporary.root.join("home")).unwrap();
        std::fs::write(
            profile.join(muniment_core::local_mode::LOCAL_MODE_MARKER),
            "1",
        )
        .unwrap();
        common::use_test_prompt_store();
        let artifact = common::stage_pi_stub(&profile);
        let mut member = Self {
            name,
            temporary,
            state: None,
            access: None,
            address: None,
            stop: None,
            server: None,
            artifact,
            reader: Mutex::new(None),
            account,
        };
        member.open();
        member
    }

    fn open(&mut self) {
        let profile = &self.temporary.profile;
        // Each member keeps its own secrets, as separate machines do.
        let mut host = RuntimeHost {
            credentials: Arc::new(FileCredentialStore::new(
                self.temporary.root.join("credentials"),
            )),
            ..RuntimeHost::default()
        };
        if let Some(account) = &self.account {
            host.account = Arc::clone(account);
        }
        let state = Arc::new(RuntimeAttachState::open(host, profile, profile).unwrap());
        let access = state
            .open_remote_access(RemoteAccessOptions {
                runtime_name: Some(self.name.into()),
                runtime_version: state.runtime_version().into(),
                install_kind: InstallKind::Server,
            })
            .unwrap();
        state.swarm().set_poll_interval(Duration::from_millis(100));
        self.state = Some(state);
        self.access = Some(access);
        self.listen();
    }

    fn listen(&mut self) {
        let address = self
            .address
            .unwrap_or_else(|| "127.0.0.1:0".parse().unwrap());
        let listener = RemoteListener::bind(RemoteListenerConfig::new(address)).unwrap();
        let bound = listener.local_addr().unwrap();
        self.address = Some(bound);
        let access = self.access().clone();
        access
            .set_advertised_endpoints(vec![bound.to_string()])
            .unwrap();
        let (stop, stopped) = mpsc::channel();
        let state = Arc::clone(self.state());
        self.server = Some(std::thread::spawn(move || {
            state.serve_remote_listener(listener, access, stopped)
        }));
        self.stop = Some(stop);
    }

    /// Takes the member off the network: its listener stops.
    fn offline(&mut self) {
        drop(self.stop.take());
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
    }

    /// Stops the runtime and opens it again on the same state.
    fn restart(&mut self) {
        self.offline();
        let state = self.state.take().unwrap();
        state.swarm().stop();
        drop(state);
        self.access = None;
        std::thread::sleep(Duration::from_millis(300));
        self.open();
    }

    fn state(&self) -> &Arc<RuntimeAttachState> {
        self.state.as_ref().unwrap()
    }

    fn access(&self) -> &RemoteAccess {
        self.access.as_ref().unwrap()
    }

    fn swarm(&self) -> &Swarm {
        self.state().swarm()
    }

    fn profile(&self) -> &std::path::Path {
        &self.temporary.profile
    }

    fn runtime_id(&self) -> String {
        self.access().runtime_info().runtime_id
    }

    fn op(&self, operation: Operation, body: Value) -> Result<Value, String> {
        self.swarm().operation(operation, body, None)
    }

    /// One pass of the agent run queue with the Pi stub, then the swarm.
    fn tick(&self) {
        self.state()
            .check_agent_schedules_with(self.state().boundaries().with_pi_artifact(self.artifact))
            .unwrap();
        self.swarm().tick().unwrap();
    }

    fn agent(&self, name: &str) -> Agent {
        agents::save(
            self.profile(),
            Agent {
                name: name.into(),
                instructions: format!("{name} does its work."),
                ..Agent::default()
            },
        )
        .unwrap()
    }

    fn store(&self) -> RunStore {
        RunStore::open(self.profile()).unwrap()
    }

    fn runs(&self, agent_id: &str) -> Vec<Run> {
        let mut runs = self.store().page(Some(agent_id), 100, None).unwrap().runs;
        runs.reverse();
        runs
    }

    /// Runs one manual run of an agent to its end and answers it.
    fn run_to_end(&self, agent_id: &str, prompt: &str) -> Run {
        let run = agents::run_now(self.profile(), agent_id, Some(prompt), Some("test")).unwrap();
        assert!(
            wait(|| {
                self.tick();
                !self
                    .store()
                    .get(&run.id)
                    .unwrap()
                    .unwrap()
                    .status
                    .is_active()
            }),
            "{} never finished run {}",
            self.name,
            run.id
        );
        let run = self.store().get(&run.id).unwrap().unwrap();
        assert_eq!(
            run.status,
            RunStatus::Completed,
            "{} run {} ended with {:?}",
            self.name,
            run.id,
            run.error
        );
        run
    }

    fn primary_thread(&self, agent_id: &str) -> String {
        agents::state(self.profile())
            .unwrap()
            .primary_threads
            .get(agent_id)
            .cloned()
            .unwrap()
    }

    /// A device paired to this runtime with the default scopes.
    fn client(&self, scopes: Option<&[&str]>) -> DesktopClient {
        let link = self
            .access()
            .create_pairing_link(PairingLinkRequest {
                scopes: scopes
                    .map(|scopes| scopes.iter().map(|scope| (*scope).to_owned()).collect()),
                endpoints: None,
            })
            .unwrap()
            .link;
        pair_with_link(&link, &options()).unwrap().0
    }

    /// Links this runtime to `other` through `other`'s invitation.
    fn join(&self, other: &Member) {
        let invite = other.op(Operation::SwarmInvite, json!({})).unwrap();
        let joined = self
            .op(Operation::SwarmJoin, json!({"link": invite["link"]}))
            .unwrap();
        assert_eq!(joined["member"]["runtime_id"], other.runtime_id());
    }

    fn members(&self) -> Vec<Value> {
        self.op(Operation::SwarmMembers, json!({})).unwrap()["members"]
            .as_array()
            .unwrap()
            .clone()
    }

    fn directory(&self) -> Vec<Value> {
        self.op(Operation::SwarmAgents, json!({})).unwrap()["agents"]
            .as_array()
            .unwrap()
            .clone()
    }
}

impl Drop for Member {
    fn drop(&mut self) {
        if let Some(state) = &self.state {
            let registry = state.run_registry();
            for identity in registry.identities() {
                let _ =
                    muniment_core::active_run::cancel_active_run(&registry, &identity.run_id, None);
            }
            wait(|| registry.is_empty());
            state.swarm().stop();
        }
        self.offline();
    }
}

fn options() -> RemoteClientOptions {
    RemoteClientOptions {
        kind: "desktop-client".into(),
        version: "1.0.0".into(),
        device_name: None,
        scopes: None,
        io_timeout: Duration::from_secs(10),
    }
}

fn wait(mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(40);
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn environment() -> std::sync::MutexGuard<'static, ()> {
    let guard = ENVIRONMENT
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    for name in STUB_SETTINGS {
        std::env::remove_var(name);
    }
    guard
}

fn has_member(member: &Member, runtime_id: &str) -> bool {
    member
        .members()
        .iter()
        .any(|entry| entry["runtime_id"] == runtime_id && entry["self"] == false)
}

#[test]
fn a_link_is_introduced_persists_across_a_restart_and_is_revoked() {
    let _environment = environment();
    let a = Member::new("link-a");
    let mut b = Member::new("link-b");
    b.join(&a);
    assert!(
        wait(|| has_member(&a, &b.runtime_id())),
        "A never heard of B"
    );
    assert!(has_member(&b, &a.runtime_id()));
    let link = b
        .members()
        .into_iter()
        .find(|member| member["runtime_id"] == a.runtime_id())
        .unwrap()["link"]
        .clone();
    assert_eq!(link["direction"], "outbound");
    assert_eq!(link["state"], "connected");

    // A lists B as a paired device that holds only swarm.peer; B lists its
    // link to A as an outbound device.
    let on_a = a.access().list_devices().unwrap();
    let peer = on_a
        .iter()
        .find(|device| device.kind == "swarm-peer")
        .unwrap();
    assert_eq!(peer.scopes, BTreeSet::from(["swarm.peer".to_owned()]));
    let on_b = b.access().list_devices().unwrap();
    let outbound = on_b
        .iter()
        .find(|device| device.direction.as_deref() == Some("outbound"))
        .unwrap();
    assert_eq!(
        outbound.runtime_id.as_deref(),
        Some(a.runtime_id().as_str())
    );

    // The swarm.peer device reaches nothing but the exchange.
    let record = RemoteRuntimeRecord::load(
        &b.profile()
            .join("swarm/links")
            .join(format!("{}.json", a.runtime_id())),
    )
    .unwrap();
    let mut peer_client = connect_remote_runtime(&record, &options()).unwrap();
    assert!(peer_client
        .request(Operation::ThreadList, None, json!({"limit": 10}))
        .is_err());
    assert_eq!(
        peer_client.last_request_error().map(|error| error.code()),
        Some(ErrorCode::Unauthorized)
    );
    drop(peer_client);
    // A device without swarm.peer cannot trade swarm state.
    let mut reader = a.client(None);
    assert!(reader
        .request(Operation::SwarmExchange, None, json!({}))
        .is_err());
    assert_eq!(
        reader.last_request_error().map(|error| error.code()),
        Some(ErrorCode::Unauthorized)
    );
    drop(reader);

    // The link survives a restart of the dialing runtime.
    b.restart();
    assert!(
        wait(|| b
            .members()
            .iter()
            .any(|member| member["runtime_id"] == a.runtime_id()
                && member["link"]["state"] == "connected"
                && member["online"] == true)),
        "B did not reconnect after its restart"
    );

    // A revokes B's device: B's link reads revoked and stops dialing.
    assert!(a.access().revoke_device(&peer.device_id).unwrap());
    assert!(
        wait(|| b
            .members()
            .iter()
            .any(|member| member["runtime_id"] == a.runtime_id()
                && member["link"]["state"] == "revoked")),
        "B never saw the revocation"
    );
    // B removes the dead link through the device revoke path.
    assert!(b.access().revoke_device(&outbound.device_id).unwrap());
    assert!(!b
        .profile()
        .join("swarm/links")
        .join(format!("{}.json", a.runtime_id()))
        .exists());
}

#[test]
fn the_directory_converges_through_a_relay() {
    let _environment = environment();
    let a = Member::new("directory-a");
    let b = Member::new("directory-b");
    let c = Member::new("directory-c");
    // A chain: B dials A and C dials B. A and C never link directly.
    b.join(&a);
    c.join(&b);
    let scout = a.agent("Scout");
    let writer = b.agent("Writer");
    let critic = c.agent("Critic");
    let names = |member: &Member| -> BTreeSet<String> {
        member
            .directory()
            .iter()
            .map(|entry| entry["name"].as_str().unwrap().to_owned())
            .collect()
    };
    let all = BTreeSet::from(["Scout".into(), "Writer".into(), "Critic".into()]);
    for member in [&a, &b, &c] {
        assert!(
            wait(|| names(member) == all),
            "{} saw {:?}",
            member.name,
            names(member)
        );
    }
    // Every entry names the runtime that runs the agent.
    let entry = |member: &Member, id: &str| {
        member
            .directory()
            .into_iter()
            .find(|entry| entry["agent_id"] == id)
            .unwrap()
    };
    assert_eq!(entry(&c, &scout.id)["primary_runtime"], a.runtime_id());
    assert_eq!(entry(&a, &critic.id)["primary_runtime"], c.runtime_id());
    assert_eq!(entry(&a, &writer.id)["primary_runtime"], b.runtime_id());
    assert!(entry(&c, &scout.id).get("definition").is_none());
    // A and C know each other through B.
    assert!(wait(
        || has_member(&a, &c.runtime_id()) && has_member(&c, &a.runtime_id())
    ));

    // An edit on A wins everywhere; a deletion leaves the directory.
    let mut renamed = agents::get(a.profile(), &scout.id).unwrap();
    renamed.name = "Lead scout".into();
    agents::save(a.profile(), renamed).unwrap();
    assert!(wait(|| entry(&c, &scout.id)["name"] == "Lead scout"));
    agents::delete(c.profile(), &critic.id).unwrap();
    assert!(wait(|| a
        .directory()
        .iter()
        .all(|entry| entry["agent_id"] != critic.id.as_str())));

    // Groups ride the same records: one created on C, renamed on A and
    // given a new lead on B, converges everywhere with both edits.
    let group = c
        .op(
            Operation::GroupCreate,
            json!({"name": "Field team", "members": ["Lead scout", writer.id]}),
        )
        .unwrap()["group"]
        .clone();
    let group_id = group["group_id"].as_str().unwrap().to_owned();
    let listed = |member: &Member| -> Option<Value> {
        member.op(Operation::GroupList, json!({})).unwrap()["groups"]
            .as_array()
            .unwrap()
            .iter()
            .find(|listed| listed["group_id"] == group_id.as_str())
            .cloned()
    };
    for member in [&a, &b] {
        assert!(
            wait(|| listed(member).is_some()),
            "{} never saw the group",
            member.name
        );
    }
    a.op(
        Operation::GroupUpdate,
        json!({"group_id": group_id, "name": "Scouting party"}),
    )
    .unwrap();
    b.op(
        Operation::GroupUpdate,
        json!({"group_id": group_id, "lead": writer.id}),
    )
    .unwrap();
    for member in [&a, &b, &c] {
        assert!(
            wait(|| listed(member)
                .is_some_and(|group| group["name"] == "Scouting party"
                    && group["lead"] == writer.id.as_str())),
            "{} saw {:?}",
            member.name,
            listed(member)
        );
    }
    a.op(
        Operation::GroupUpdate,
        json!({"group_id": group_id, "deleted": true}),
    )
    .unwrap();
    for member in [&a, &b, &c] {
        assert!(wait(|| listed(member).is_none()));
    }
    // Every member keeps the directory and groups as records and trades
    // them as deltas with its links.
    let sync = c.op(Operation::SwarmMembers, json!({})).unwrap()["sync"].clone();
    assert!(sync["records"]["agent"]["rows"].as_u64().unwrap() >= 6);
    assert!(sync["records"]["group"]["rows"].as_u64().unwrap() >= 8);
    assert_eq!(sync["peers"][b.runtime_id()]["store"], true);
}

fn routine(name: &str) -> Routine {
    muniment_core::serde_json::from_value(json!({
        "name": name,
        "enabled": true,
        "trigger": {"kind": "cron", "expr": "0 9 * * *", "tz": "UTC"},
        "prompt": "Report.",
    }))
    .unwrap()
}

fn history_entries(reader: &mut DesktopClient, thread: &str) -> Vec<Value> {
    reader
        .thread_history(thread, 50, None)
        .ok()
        .and_then(|page| page["entries"].as_array().cloned())
        .unwrap_or_default()
}

fn posted(reader: &mut DesktopClient, thread: &str) -> Vec<Value> {
    reader
        .thread_history(thread, 50, None)
        .ok()
        .and_then(|page| page["posted"].as_array().cloned())
        .unwrap_or_default()
}

#[test]
fn threads_replicate_with_causal_posts_a_replica_read_and_catch_up() {
    let _environment = environment();
    let mut a = Member::new("sync-a");
    let b = Member::new("sync-b");
    b.join(&a);
    let scout = a.agent("Scout");
    std::env::set_var("PI_RESUME_STUB_REPLY", " first answer");
    a.run_to_end(&scout.id, "Find leads.");
    let thread = a.primary_thread(&scout.id);

    // B reads its replica through its own thread.history, under A's ids.
    let mut reader = b.client(None);
    assert!(
        wait(|| history_entries(&mut reader, &thread)
            .iter()
            .any(|entry| entry["text"]
                .as_str()
                .unwrap_or("")
                .contains("first answer"))),
        "B never read A's run"
    );
    let entry = history_entries(&mut reader, &thread)[0].clone();
    assert_eq!(entry["prompt"], "Find leads.");
    assert_eq!(b.swarm().is_replica_thread(&thread), Some(a.runtime_id()));
    // Only the primary writes runs into the thread.
    let mut writer = b.client(None);
    assert!(writer
        .run_submit("Write here.", &[], Some(&thread), None, None)
        .is_err());

    // Causal order: B posts only after it holds A's post, so A consumes
    // A's post first.
    let west = a
        .op(
            Operation::ThreadPost,
            json!({"thread_id": thread, "text": "Check the west region first."}),
        )
        .unwrap();
    assert_eq!(west["origin_runtime"], a.runtime_id());
    assert!(wait(|| posted(&mut reader, &thread)
        .iter()
        .any(|post| post["message_id"] == west["message_id"])));
    let east = b
        .op(
            Operation::ThreadPost,
            json!({"thread_id": thread, "text": "Then the east region."}),
        )
        .unwrap();
    assert_eq!(east["origin_runtime"], b.runtime_id());
    // Concurrent posts on both members.
    a.op(
        Operation::ThreadPost,
        json!({"thread_id": thread, "text": "Concurrent from A."}),
    )
    .unwrap();
    b.op(
        Operation::ThreadPost,
        json!({"thread_id": thread, "text": "Concurrent from B."}),
    )
    .unwrap();
    let posted_prompts = |member: &Member| -> Vec<String> {
        member
            .runs(&scout.id)
            .into_iter()
            .filter(|run| {
                run.trigger
                    .source
                    .as_deref()
                    .is_some_and(|source| source.starts_with("swarm:"))
            })
            .map(|run| run.prompt)
            .collect()
    };
    assert!(
        wait(|| {
            a.swarm().tick().unwrap();
            posted_prompts(&a).len() == 4
        }),
        "A consumed {:?}",
        posted_prompts(&a)
    );
    let prompts = posted_prompts(&a);
    let position = |text: &str| prompts.iter().position(|prompt| prompt == text).unwrap();
    assert!(position("Check the west region first.") < position("Then the east region."));
    assert!(prompts.contains(&"Concurrent from A.".to_owned()));
    assert!(prompts.contains(&"Concurrent from B.".to_owned()));
    // The consumed marks replicate: B sees every post answered by a run.
    assert!(
        wait(|| {
            let posts = posted(&mut reader, &thread);
            posts.len() == 4 && posts.iter().all(|post| post["consumed"] == true)
        }),
        "{:?}",
        posted(&mut reader, &thread)
    );

    // Catch-up after a reconnect: A runs while B cannot reach it.
    assert!(wait(|| {
        a.tick();
        a.runs(&scout.id).iter().all(|run| !run.status.is_active())
    }));
    a.offline();
    std::env::set_var("PI_RESUME_STUB_REPLY", " while apart");
    a.run_to_end(&scout.id, "Work while B is away.");
    std::thread::sleep(Duration::from_millis(500));
    assert!(!history_entries(&mut reader, &thread)
        .iter()
        .any(|entry| entry["text"].as_str().unwrap_or("").contains("while apart")));
    a.listen();
    assert!(
        wait(|| history_entries(&mut reader, &thread)
            .iter()
            .any(|entry| entry["text"].as_str().unwrap_or("").contains("while apart"))),
        "B never caught up"
    );
    assert_eq!(
        history_entries(&mut reader, &thread).len(),
        a.runs(&scout.id).len()
    );
}

fn listed(reader: &mut DesktopClient, thread: &str) -> Option<Value> {
    reader
        .thread_summaries(100, None)
        .ok()?
        .get("summaries")?
        .as_array()?
        .iter()
        .find(|summary| summary["threadId"] == thread)
        .cloned()
}

fn thread_mode(reader: &mut DesktopClient, thread: &str) -> Value {
    reader
        .agent_request(
            Operation::ThreadPermissionMode,
            json!({"thread_id": thread}),
        )
        .map(|answer| answer["mode"].clone())
        .unwrap_or(Value::Null)
}

#[test]
fn an_ordinary_thread_reaches_every_member_and_a_post_runs_on_its_owner() {
    let _environment = environment();
    let a = Member::new("plain-a");
    let b = Member::new("plain-b");
    let c = Member::new("plain-c");
    for member in [&a, &b, &c] {
        member.state().use_pi_artifact(member.artifact);
    }
    // C reaches B only through A.
    b.join(&a);
    c.join(&a);
    std::env::set_var("PI_RESUME_STUB_REPLY", " trip ready");
    let mut writer = a.client(None);
    let thread = writer
        .run_submit("Plan the trip.", &[], None, None, None)
        .unwrap()
        .thread_id;
    let ours = |summary: &Value| {
        summary["owner_runtime"] == a.runtime_id().as_str() && summary["replica"] == true
    };
    for member in [&b, &c] {
        let mut reader = member.client(None);
        assert!(
            wait(|| listed(&mut reader, &thread).is_some_and(|summary| ours(&summary))),
            "{} never listed the thread: {:?}",
            member.name,
            listed(&mut reader, &thread)
        );
        assert!(
            wait(|| history_entries(&mut reader, &thread)
                .iter()
                .any(|entry| entry["text"].as_str().unwrap_or("").contains("trip ready"))),
            "{} never read A's run",
            member.name
        );
        assert_eq!(
            member.swarm().is_replica_thread(&thread),
            Some(a.runtime_id())
        );
    }
    let own = listed(&mut writer, &thread).unwrap();
    assert_eq!(own["owner_runtime"], a.runtime_id().as_str());
    assert!(own.get("replica").is_none());

    // A post on C reaches A, which runs it; C reads the answer.
    std::env::set_var("PI_RESUME_STUB_REPLY", " train booked");
    let post = c
        .op(
            Operation::ThreadPost,
            json!({"thread_id": thread, "text": "Book the train."}),
        )
        .unwrap();
    assert_eq!(post["origin_runtime"], c.runtime_id().as_str());
    let mut reader = c.client(None);
    assert!(
        wait(|| posted(&mut reader, &thread).iter().any(|entry| {
            entry["message_id"] == post["message_id"]
                && entry["consumed"] == true
                && entry["run_id"].is_string()
        })),
        "A never consumed C's post: {:?}",
        posted(&mut reader, &thread)
    );
    assert!(
        wait(|| history_entries(&mut reader, &thread)
            .iter()
            .any(|entry| entry["text"]
                .as_str()
                .unwrap_or("")
                .contains("train booked"))),
        "C never read the run its post started"
    );
    assert_eq!(history_entries(&mut writer, &thread).len(), 2);
    // Only the owner writes runs into the thread, and the refusal names no
    // operation a person could not act on.
    let refused = reader
        .run_submit("Write here.", &[], Some(&thread), None, None)
        .unwrap_err();
    let reason = format!("{:?} {refused:?}", reader.last_request_error());
    assert!(
        reason.contains("Another machine runs this thread") && !reason.contains("thread.post"),
        "{reason}"
    );

    // A mode set on C goes to A, which sets it, and it replicates back.
    let answer = reader
        .set_thread_permission_mode(&thread, "edit_auto")
        .unwrap();
    assert_eq!(answer["forwarded"], true, "{answer}");
    assert!(
        wait(|| thread_mode(&mut writer, &thread) == "edit_auto"),
        "A never set the mode"
    );
    let mut b_reader = b.client(None);
    for reader in [&mut reader, &mut b_reader] {
        assert!(
            wait(|| thread_mode(reader, &thread) == "edit_auto"),
            "the mode never replicated"
        );
    }

    // Every member reports itself synced once the events of the mode change
    // reach it: a peer can name them in its clocks a moment before.
    let sync_status = || b.op(Operation::FleetSyncStatus, json!({})).unwrap();
    wait(|| sync_status()["state"] == "synced");
    let status = sync_status();
    assert_eq!(status["state"], "synced", "{status}");
}

#[test]
fn a_move_fences_the_old_primary_and_a_forced_move_steps_it_down() {
    let _environment = environment();
    let mut a = Member::new("move-a");
    let b = Member::new("move-b");
    b.join(&a);
    let mut scout = a.agent("Scout");
    scout.routines.push(routine("Morning"));
    let scout = agents::save(a.profile(), scout).unwrap();
    let memory = a.profile().join("agent-memory").join(&scout.id);
    std::fs::create_dir_all(memory.join("facts")).unwrap();
    std::fs::write(memory.join("facts/style.md"), "Prefers short answers.").unwrap();
    a.run_to_end(&scout.id, "Say hello.");
    let thread = a.primary_thread(&scout.id);
    assert!(wait(|| b
        .directory()
        .iter()
        .any(|entry| entry["agent_id"] == scout.id.as_str())));

    // B asks for the move; A fences the agent and hands it over.
    let requested = b
        .op(
            Operation::AgentMove,
            json!({"id": scout.id, "to": b.runtime_id()}),
        )
        .unwrap();
    assert_eq!(requested["status"], "requested");
    assert!(
        wait(|| agents::get(b.profile(), &scout.id).is_ok_and(|agent| {
            agent.primary_runtime.as_deref() == Some(b.runtime_id().as_str()) && agent.epoch == 1
        })),
        "B never took the agent"
    );
    let old = agents::get(a.profile(), &scout.id).unwrap();
    assert_eq!(
        old.primary_runtime.as_deref(),
        Some(b.runtime_id().as_str())
    );
    assert_eq!(old.epoch, 1);
    assert_eq!(
        std::fs::read_to_string(
            b.profile()
                .join("agent-memory")
                .join(&scout.id)
                .join("facts/style.md")
        )
        .unwrap(),
        "Prefers short answers."
    );
    assert_eq!(b.primary_thread(&scout.id), thread);
    assert_eq!(
        agents::get(b.profile(), &scout.id).unwrap().routines.len(),
        1
    );
    // A's copy is fenced: its thread is a replica there and B runs it.
    assert_eq!(a.swarm().is_replica_thread(&thread), Some(b.runtime_id()));
    assert_eq!(b.swarm().is_replica_thread(&thread), None);
    let on_b = b.run_to_end(&scout.id, "Now on B.");
    assert_eq!(on_b.status, RunStatus::Completed, "{:?}", on_b.error);
    assert_eq!(on_b.thread_id.as_deref(), Some(thread.as_str()));
    assert!(wait(|| a
        .directory()
        .iter()
        .any(|entry| entry["agent_id"] == scout.id.as_str()
            && entry["primary_runtime"] == b.runtime_id()
            && entry["epoch"] == 1)));

    // A forced move while the primary is offline.
    let critic = a.agent("Critic");
    a.run_to_end(&critic.id, "Hello.");
    assert!(wait(|| b
        .directory()
        .iter()
        .any(|entry| entry["agent_id"] == critic.id.as_str())));
    a.offline();
    b.swarm()
        .advance_clock(muniment_core::chrono::Duration::minutes(5));
    let forced = b
        .op(
            Operation::AgentMove,
            json!({"id": critic.id, "to": b.runtime_id(), "force": true}),
        )
        .unwrap();
    assert_eq!(forced["status"], "forced");
    assert_eq!(forced["agent"]["epoch"], 1);
    let taken = agents::get(b.profile(), &critic.id).unwrap();
    assert_eq!(
        taken.primary_runtime.as_deref(),
        Some(b.runtime_id().as_str())
    );
    // Without the old primary's saved conversation, the thread goes on in a
    // new one.
    assert_eq!(
        b.run_to_end(&critic.id, "Carry on.").status,
        RunStatus::Completed
    );
    // The old primary steps down when it sees the higher epoch.
    a.listen();
    assert!(
        wait(|| agents::get(a.profile(), &critic.id).is_ok_and(|agent| {
            agent.primary_runtime.as_deref() == Some(b.runtime_id().as_str()) && agent.epoch == 1
        })),
        "A never stepped down"
    );
}

/// Delivers one webhook over the member's pinned TLS listener and answers
/// the HTTP status and the JSON body.
fn deliver(member: &Member, hook: &str, secret: &str, body: &[u8]) -> (u16, Value) {
    use std::io::{Read, Write};
    let mut stream = muniment_core::attach::connect_pinned(
        &[member.address.unwrap().to_string()],
        member.access().certificate_fingerprint(),
        Duration::from_secs(5),
    )
    .unwrap();
    let mut request = format!(
        "POST /hooks/{hook} HTTP/1.1\r\nHost: runtime\r\nAuthorization: Bearer {secret}\r\n\
         Content-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    request.extend_from_slice(body);
    stream.write_all(&request).unwrap();
    let mut answer = Vec::new();
    let _ = stream.read_to_end(&mut answer);
    let text = String::from_utf8(answer).unwrap();
    let status = text[9..12].parse().unwrap();
    let body = text.split("\r\n\r\n").nth(1).unwrap_or("{}");
    (
        status,
        muniment_core::serde_json::from_str(body).unwrap_or(Value::Null),
    )
}

#[test]
fn a_move_hands_over_webhook_secrets_and_the_mcp_servers_routines_call() {
    let _environment = environment();
    let a = Member::new("hand-a");
    let b = Member::new("hand-b");
    b.join(&a);
    let watcher = a.agent("Watcher");
    let created = a
        .state()
        .boundaries()
        .hook_operation(
            Operation::RoutineWebhookCreate,
            json!({"agent_id": watcher.id, "routine": {"name": "Build finished",
                "trigger": {"kind": "webhook", "preset": "generic"},
                "prompt": "Summarize the build."}}),
            HookEndpoints {
                endpoints: a.access().advertised_endpoints(),
                certificate_sha256: Some(a.access().certificate_fingerprint().to_hex()),
                listening: true,
            },
        )
        .unwrap();
    let hook = created["hook"]["hook_id"].as_str().unwrap().to_owned();
    let secret = created["secret"].as_str().unwrap().to_owned();
    std::fs::create_dir_all(a.profile().join("extensions")).unwrap();
    std::fs::write(
        a.profile().join("extensions/inventory.json"),
        json!({"items": [
            {"id": "crm-1", "kind": "mcp", "name": "crm", "definition": {"url": "https://crm.example/mcp", "auth": "bearer"},
             "lastCheck": {"status": "connected", "tools": 1, "toolNames": ["search_leads"]}},
            {"id": "other-1", "kind": "mcp", "name": "other", "definition": {"url": "https://other.example/mcp"}}
        ], "turns": {}})
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        a.profile().join("extensions/mcp-tokens.json"),
        json!({"extend-crm-1": "crm-token", "extend-other-1": "other-token"}).to_string(),
    )
    .unwrap();
    agents::save_routine(
        a.profile(),
        &watcher.id,
        muniment_core::serde_json::from_value(json!({"id": "leads", "name": "Leads",
            "prompt": "Summarize the leads.",
            "trigger": {"kind": "mcp", "server": "crm", "tool": "search_leads", "args": {},
                "every": {"expr": "*/5 * * * *", "tz": "UTC"},
                "condition": {"path": "/total", "op": "gt", "value": 0}}}))
        .unwrap(),
    )
    .unwrap();
    assert!(wait(|| b
        .directory()
        .iter()
        .any(|entry| entry["agent_id"] == watcher.id.as_str())));

    let moved = a
        .op(
            Operation::AgentMove,
            json!({"id": watcher.id, "to": b.runtime_id()}),
        )
        .unwrap();
    assert_eq!(moved["status"], "moved");
    assert!(
        wait(|| agents::get(b.profile(), &watcher.id).is_ok_and(|agent| {
            agent.primary_runtime.as_deref() == Some(b.runtime_id().as_str())
        })),
        "B never took the agent"
    );
    let b_secrets = FileCredentialStore::new(b.temporary.root.join("credentials"));
    assert_eq!(
        b_secrets.read(muniment_core::agent_hooks::SECRET_SERVICE, &hook),
        Some(secret.clone())
    );
    let inventory = muniment_core::extend::inventory(b.profile()).unwrap();
    let servers: Vec<&str> = inventory["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["name"].as_str())
        .collect();
    assert_eq!(servers, ["crm"]);
    assert_eq!(
        muniment_core::serde_json::from_slice::<Value>(
            &std::fs::read(b.profile().join("extensions/mcp-tokens.json")).unwrap()
        )
        .unwrap(),
        json!({"extend-crm-1": "crm-token"})
    );
    let catalog = b
        .client(Some(&["thread.read"]))
        .agent_tool_catalog()
        .unwrap();
    assert_eq!(
        catalog["mcp"],
        json!([{"id": "crm-1", "server": "crm", "tools": [{"name": "search_leads"}]}])
    );

    // The new primary takes the hook's deliveries with the same secret; the
    // old one refuses them.
    let (status, answer) = deliver(&b, &hook, &secret, br#"{"build":"failed"}"#);
    assert_eq!(status, 202, "{answer}");
    let run = b
        .store()
        .get(answer["run_id"].as_str().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(run.trigger.kind, TriggerKind::Webhook);
    assert_eq!(run.agent_id, watcher.id);
    let (status, _) = deliver(&a, &hook, &secret, br#"{"build":"failed"}"#);
    assert_ne!(status, 202);
}

fn has_prompt(runs: &[Run], text: &str) -> Option<Run> {
    runs.iter().find(|run| run.prompt.contains(text)).cloned()
}

#[test]
fn agents_message_across_runtimes_with_results_hops_allow_lists_and_handoffs() {
    let _environment = environment();
    let a = Member::new("message-a");
    let b = Member::new("message-b");
    b.join(&a);
    let planner = a.agent("Planner");
    let analyst = b.agent("Analyst");
    std::env::set_var("PI_RESUME_STUB_REPLY", " three deals");
    let start = a.run_to_end(&planner.id, "Plan the week.");
    let planner_thread = a.primary_thread(&planner.id);
    assert!(wait(|| a
        .directory()
        .iter()
        .any(|entry| entry["agent_id"] == analyst.id.as_str())));

    // A's clients hear B's runs that warrant a notification.
    let remote_runs = muniment_core::agent_runs::subscribe(a.profile());
    // A request crosses to B, runs there in an untrusted block, and its
    // final answer returns to the planner as a result.
    let sent = a
        .swarm()
        .agent_tool(
            &start.id,
            &planner_thread,
            "agent-message",
            json!({"to": "analyst", "intent": "request", "text": "List the three biggest open deals."}),
        )
        .unwrap();
    assert_eq!(sent["hop"], 1);
    assert_eq!(sent["to"]["runtime_id"], b.runtime_id());
    let mut woken = None;
    assert!(
        wait(|| {
            b.tick();
            woken = has_prompt(&b.runs(&analyst.id), "<agent_message");
            woken
                .as_ref()
                .is_some_and(|run| run.status == RunStatus::Completed)
        }),
        "the analyst never ran"
    );
    let woken = woken.unwrap();
    let notice_deadline = Instant::now() + Duration::from_secs(30);
    let notice = loop {
        assert!(Instant::now() < notice_deadline, "A never heard of B's run");
        match remote_runs.recv_timeout(Duration::from_millis(200)) {
            Ok(body) if body["id"] == woken.id.as_str() => break body,
            _ => {}
        }
    };
    assert_eq!(notice["runtime_id"], b.runtime_id());
    assert_eq!(notice["notify"], true);
    assert_eq!(notice["status"], "completed");
    assert_eq!(woken.trigger.kind, TriggerKind::AgentMessage);
    assert_eq!(woken.hop, 1);
    assert!(woken.prompt.contains("intent=\"request\""));
    assert!(woken.prompt.contains("untrusted"));
    let mut result = None;
    assert!(
        wait(|| {
            a.swarm().tick().unwrap();
            result = has_prompt(&a.runs(&planner.id), "intent=\"result\"");
            result.is_some()
        }),
        "the result never came back"
    );
    let result = result.unwrap();
    assert_eq!(result.hop, 2);
    assert!(result.prompt.contains("three deals"));
    assert!(result.prompt.contains(&format!(
        "reply_to=\"{}\"",
        sent["message_id"].as_str().unwrap()
    )));
    assert!(wait(|| {
        a.tick();
        a.runs(&planner.id)
            .iter()
            .all(|run| !run.status.is_active())
    }));

    // The hop cap refuses a seventh step.
    let deep = a
        .store()
        .enqueue(NewRun {
            agent_id: planner.id.clone(),
            thread_id: Some(planner_thread.clone()),
            trigger: RunTrigger {
                kind: TriggerKind::AgentMessage,
                routine_id: None,
                source: None,
            },
            prompt: "Deep in a chain.".into(),
            hop: 6,
            origin_run_id: None,
        })
        .unwrap();
    let refused = a
        .swarm()
        .agent_tool(
            &deep.id,
            &planner_thread,
            "agent-message",
            json!({"to": "Analyst", "intent": "fyi", "text": "One more."}),
        )
        .unwrap_err();
    assert!(refused.contains("6 steps"), "{refused}");
    let mut cancelled = a.store().get(&deep.id).unwrap().unwrap();
    cancelled.status = RunStatus::Cancelled;
    a.store().save(&mut cancelled).unwrap();

    // messaging.allow is enforced when the agent sends.
    let mut limited = agents::get(a.profile(), &planner.id).unwrap();
    limited.messaging = Some(AgentMessaging {
        allow: MessagingAllow::Agents(vec![uuid_string()]),
    });
    agents::save(a.profile(), limited).unwrap();
    let blocked = a
        .swarm()
        .agent_tool(
            &start.id,
            &planner_thread,
            "agent-message",
            json!({"to": "Analyst", "intent": "fyi", "text": "Hello."}),
        )
        .unwrap_err();
    assert!(blocked.contains("may not message"), "{blocked}");
    assert!(
        blocked.ends_with("Choose agents under Messages to other agents in its profile."),
        "{blocked}"
    );
    let mut open = agents::get(a.profile(), &planner.id).unwrap();
    open.messaging = Some(AgentMessaging {
        allow: MessagingAllow::Any,
    });
    agents::save(a.profile(), open).unwrap();

    // A handoff is idempotent under its run's key.
    let handoff =
        json!({"to": "Analyst", "task": "Draft the summary.", "context": "Use the deals list."});
    let first = a
        .swarm()
        .agent_tool(&start.id, &planner_thread, "agent-handoff", handoff.clone())
        .unwrap();
    assert_eq!(first["message_id"], format!("handoff:{}", start.id));
    let again = a
        .swarm()
        .agent_tool(&start.id, &planner_thread, "agent-handoff", handoff)
        .unwrap();
    assert_eq!(again["duplicate"], true);
    assert_eq!(again["message_id"], first["message_id"]);
    let mut handed = None;
    assert!(
        wait(|| {
            b.swarm().tick().unwrap();
            handed = has_prompt(&b.runs(&analyst.id), "<agent_handoff");
            handed.is_some()
        }),
        "the handoff never arrived"
    );
    assert_eq!(
        b.runs(&analyst.id)
            .iter()
            .filter(|run| run.prompt.contains("<agent_handoff"))
            .count(),
        1
    );
    // The analyst cannot hand the task straight back.
    let analyst_thread = b.primary_thread(&analyst.id);
    let back = b
        .swarm()
        .agent_tool(
            &handed.unwrap().id,
            &analyst_thread,
            "agent-handoff",
            json!({"to": "Planner", "task": "Back to you."}),
        )
        .unwrap_err();
    assert!(back.contains("straight back"), "{back}");
}

fn uuid_string() -> String {
    format!("018f0000-0000-4000-8000-{:012x}", std::process::id())
}

fn turns(member: &Member, agent: &str) -> Vec<Run> {
    member
        .runs(agent)
        .into_iter()
        .filter(|run| run.trigger.kind == TriggerKind::AgentMessage && run.prompt.contains("group"))
        .collect()
}

fn group_posts(reader: &mut DesktopClient, thread: &str) -> Vec<Value> {
    posted(reader, thread)
}

#[test]
fn group_chats_route_mentions_and_hold_meetings_across_members() {
    let _environment = environment();
    let a = Member::new("group-a");
    let b = Member::new("group-b");
    b.join(&a);
    let pilot = a.agent("Pilot");
    let navigator = b.agent("Navigator");
    let critic = a.agent("Critic");
    assert!(wait(|| a
        .directory()
        .iter()
        .any(|entry| entry["agent_id"] == navigator.id.as_str())));
    std::env::set_var("PI_RESUME_STUB_REPLY", " on it");
    let group = a
        .op(
            Operation::GroupCreate,
            json!({"name": "Flight deck", "members": [pilot.id, "Navigator", critic.id]}),
        )
        .unwrap()["group"]
        .clone();
    assert_eq!(group["owner_runtime"], a.runtime_id());
    assert_eq!(group["lead"], pilot.id.as_str());
    let thread = group["thread_id"].as_str().unwrap().to_owned();
    // A client of B hears each post to the group as it lands, without reading.
    let mut watcher = b.client(None);
    watcher.subscribe_chat_events_with_swarm_live(None).unwrap();
    let (heard, posts_heard) = mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(event) = watcher.read_subscription_event() {
            if let SubscriptionEvent::GroupPosted(body) = event {
                if heard.send(body).is_err() {
                    return;
                }
            }
        }
    });
    assert!(wait(|| b.op(Operation::GroupList, json!({})).unwrap()
        ["groups"]
        .as_array()
        .unwrap()
        .iter()
        .any(|listed| listed["group_id"] == group["group_id"])));
    assert!(a
        .op(
            Operation::GroupCreate,
            json!({"name": "Alone", "members": [pilot.id]}),
        )
        .unwrap_err()
        .contains("2 to 6"));
    // An update on the owner reaches every member.
    let renamed = a
        .op(
            Operation::GroupUpdate,
            json!({"group_id": group["group_id"], "name": "Cockpit"}),
        )
        .unwrap();
    assert_eq!(renamed["group"]["name"], "Cockpit");
    assert!(wait(|| b.op(Operation::GroupList, json!({})).unwrap()
        ["groups"]
        .as_array()
        .unwrap()
        .iter()
        .any(
            |listed| listed["group_id"] == group["group_id"] && listed["name"] == "Cockpit"
        )));

    // An @mention posted on B wakes only the named agent, on its machine.
    assert!(wait(|| b
        .op(
            Operation::ThreadPost,
            json!({"thread_id": thread, "text": "@Navigator plot a course"}),
        )
        .is_ok()));
    assert!(
        wait(|| turns(&b, &navigator.id).len() == 1),
        "the navigator never got its turn"
    );
    assert!(turns(&a, &pilot.id).is_empty() && turns(&a, &critic.id).is_empty());
    let mut reader = a.client(None);
    assert!(
        wait(|| {
            b.tick();
            group_posts(&mut reader, &thread)
                .iter()
                .any(|post| post["author"]["name"] == "Navigator" && post["text"] == "on it")
        }),
        "{:?}",
        group_posts(&mut reader, &thread)
    );
    let chat = group_posts(&mut reader, &thread)
        .into_iter()
        .find(|post| post["author"]["name"] == "Navigator")
        .unwrap();
    assert_eq!(chat["author"]["phase"], "chat");
    assert_eq!(chat["author"]["round"], 0);
    let heard = loop {
        let body = posts_heard
            .recv_timeout(Duration::from_secs(30))
            .expect("B's client never heard the group post");
        if body["post"]["author"]["name"] == "Navigator" {
            break body;
        }
    };
    assert_eq!(heard["group_id"], group["group_id"]);
    assert_eq!(heard["thread_id"], thread.as_str());
    assert_eq!(heard["post"]["text"], "on it");
    // No mention: the first member answers.
    a.op(
        Operation::ThreadPost,
        json!({"thread_id": thread, "text": "Status?"}),
    )
    .unwrap();
    assert!(wait(|| turns(&a, &pilot.id).len() == 1));
    assert!(turns(&a, &critic.id).is_empty());
    // @everyone wakes every member.
    a.op(
        Operation::ThreadPost,
        json!({"thread_id": thread, "text": "@everyone check in"}),
    )
    .unwrap();
    assert!(wait(|| turns(&a, &pilot.id).len() == 2
        && turns(&a, &critic.id).len() == 1
        && turns(&b, &navigator.id).len() == 2));
    assert!(wait(|| {
        a.tick();
        b.tick();
        [(&a, &pilot.id), (&a, &critic.id), (&b, &navigator.id)]
            .iter()
            .all(|(member, agent)| member.runs(agent).iter().all(|run| !run.status.is_active()))
    }));

    // A meeting: a blind first round, one debate round, the lead's summary.
    let meeting = a
        .op(
            Operation::GroupCreate,
            json!({"name": "Launch review", "members": [pilot.id, navigator.id],
                   "lead": pilot.id, "meeting_mode": true, "debate_rounds": 1}),
        )
        .unwrap()["group"]
        .clone();
    let meeting_thread = meeting["thread_id"].as_str().unwrap().to_owned();
    a.op(
        Operation::ThreadPost,
        json!({"thread_id": meeting_thread, "text": "Should we launch on Friday?"}),
    )
    .unwrap();
    assert!(
        wait(|| {
            a.tick();
            b.tick();
            group_posts(&mut reader, &meeting_thread)
                .iter()
                .any(|post| post["text"] == "The meeting ended with the lead's summary.")
        }),
        "{:?}",
        group_posts(&mut reader, &meeting_thread)
    );
    let meeting_prompts = |member: &Member, agent: &str| -> Vec<String> {
        turns(member, agent)
            .into_iter()
            .filter(|run| run.prompt.contains("Launch review"))
            .map(|run| run.prompt)
            .collect()
    };
    let pilot_prompts = meeting_prompts(&a, &pilot.id);
    let navigator_prompts = meeting_prompts(&b, &navigator.id);
    assert_eq!(pilot_prompts.len(), 3, "{pilot_prompts:#?}");
    assert_eq!(navigator_prompts.len(), 2, "{navigator_prompts:#?}");
    for prompts in [&pilot_prompts, &navigator_prompts] {
        assert!(prompts[0].contains("Round 1 of 3"));
        assert!(!prompts[0].contains("<meeting_answers>"));
        assert!(prompts[1].contains("Debate round 1 of 1"));
        assert!(prompts[1].contains("Round 1, Pilot: on it"));
        assert!(prompts[1].contains("Round 1, Navigator: on it"));
    }
    assert!(pilot_prompts[2].contains("the lead of the meeting"));
    assert!(pilot_prompts[2].contains("Round 2, Navigator: on it"));
    // Five replies and the end note reach the meeting thread, each reply
    // with its round and phase.
    let posts = group_posts(&mut reader, &meeting_thread);
    let mut phases: Vec<(u64, String)> = posts
        .iter()
        .filter(|post| post["author"]["kind"] == "agent")
        .map(|post| {
            (
                post["author"]["round"].as_u64().unwrap(),
                post["author"]["phase"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    phases.sort();
    assert_eq!(
        phases,
        [
            (1, "blind".to_owned()),
            (1, "blind".to_owned()),
            (2, "debate".to_owned()),
            (2, "debate".to_owned()),
            (3, "summary".to_owned()),
        ],
        "{posts:#?}"
    );
    // B's client heard the meeting's posts too, the summary included.
    assert!(
        posts_heard
            .iter()
            .take(40)
            .any(|body| body["thread_id"] == meeting_thread.as_str()
                && body["post"]["author"]["phase"] == "summary"),
        "B's client never heard the summary"
    );
}

#[test]
fn an_approval_reaches_every_member_and_the_first_answer_wins() {
    let _environment = environment();
    let a = Member::new("approval-a");
    let b = Member::new("approval-b");
    b.join(&a);
    let deployer = a.agent("Deployer");
    let mut watcher = b.client(None);
    watcher.subscribe_chat_events_with_swarm(None).unwrap();
    let (events, received) = mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(event) = watcher.read_subscription_event() {
            if events.send(event).is_err() {
                return;
            }
        }
    });
    let capture = a.temporary.root.join("permission.jsonl");
    std::env::set_var("PI_RESUME_STUB_PERMISSION_CAPTURE", &capture);
    let run = agents::run_now(a.profile(), &deployer.id, Some("Deploy."), Some("test")).unwrap();
    assert!(
        wait(|| {
            a.tick();
            b.op(Operation::AgentApprovals, json!({})).unwrap()["approvals"]
                .as_array()
                .unwrap()
                .len()
                == 1
        }),
        "B never heard of the approval"
    );
    let pending = b.op(Operation::AgentApprovals, json!({})).unwrap()["approvals"][0].clone();
    assert_eq!(pending["runtime_id"], a.runtime_id());
    assert_eq!(pending["run_id"], run.id.as_str());
    assert_eq!(pending["agent_name"], "Deployer");
    let approval_id = pending["approval_id"].as_str().unwrap().to_owned();
    let event_named = |wanted: &str| loop {
        match received.recv_timeout(Duration::from_secs(30)).unwrap() {
            SubscriptionEvent::ApprovalPending(body) if wanted == "pending" => return body,
            SubscriptionEvent::ApprovalResolved(body) if wanted == "resolved" => return body,
            _ => {}
        }
    };
    assert_eq!(event_named("pending")["approval_id"], approval_id.as_str());

    // B answers first and wins; A's later answer reads already answered.
    let answer = json!({"type": "confirm", "value": true});
    let first = b
        .op(
            Operation::AgentApprovalAnswer,
            json!({"approval_id": approval_id, "answer": answer}),
        )
        .unwrap();
    assert_eq!(first["status"], "answered", "{first}");
    assert_eq!(first["approval"]["answered_by"], b.runtime_id());
    let second = a
        .op(
            Operation::AgentApprovalAnswer,
            json!({"approval_id": approval_id, "answer": answer}),
        )
        .unwrap();
    assert_eq!(second["status"], "already_answered");
    assert_eq!(event_named("resolved")["state"], "answered");
    assert!(wait(|| {
        a.tick();
        a.store().get(&run.id).unwrap().unwrap().status == RunStatus::Completed
    }));
    assert!(std::fs::read_to_string(&capture)
        .unwrap()
        .contains("\"confirmed\":true"));
    std::env::remove_var("PI_RESUME_STUB_PERMISSION_CAPTURE");
}

#[test]
fn an_agent_gate_approval_names_its_request_and_allow_once_lets_the_run_go_on() {
    let _environment = environment();
    let a = Member::new("gate-a");
    let b = Member::new("gate-b");
    b.join(&a);
    let releaser: Agent = muniment_core::serde_json::from_value(json!({
        "name": "Releaser",
        "instructions": "Release carefully.",
        "permissions": {"mode": "ask"}
    }))
    .unwrap();
    let releaser = agents::save(a.profile(), releaser).unwrap();
    let mut watcher = b.client(None);
    watcher.subscribe_chat_events_with_swarm_live(None).unwrap();
    let (events, received) = mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(event) = watcher.read_subscription_event() {
            if events.send(event).is_err() {
                return;
            }
        }
    });
    std::env::set_var(
        "PI_RESUME_STUB_AGENT_GATE",
        json!({"tool": "bash", "input": {"command": "git push origin main"}}).to_string(),
    );
    let run = agents::run_now(a.profile(), &releaser.id, Some("Release."), Some("test")).unwrap();
    assert!(
        wait(|| {
            a.tick();
            b.op(Operation::AgentApprovals, json!({})).unwrap()["approvals"]
                .as_array()
                .unwrap()
                .len()
                == 1
        }),
        "B never heard of the approval"
    );
    let pending = b.op(Operation::AgentApprovals, json!({})).unwrap()["approvals"][0].clone();
    assert_eq!(pending["run_id"], run.id.as_str());
    assert_eq!(pending["rule"]["request_kind"], "command");
    assert_eq!(
        pending["rule"]["resource"]["arguments"],
        json!(["bash", "git push origin main"])
    );
    assert!(pending["summary"]
        .as_str()
        .unwrap()
        .starts_with("Run `git push origin main` in "));
    assert_eq!(pending["presentation"]["category"], "shell");
    assert_eq!(pending["answers"], json!(["allow_once", "deny"]));

    // B's client sees the agent on A wait, with the approval as its action.
    let waiting = loop {
        match received.recv_timeout(Duration::from_secs(30)).unwrap() {
            SubscriptionEvent::AgentActivity(body)
                if body["agent_id"] == releaser.id.as_str()
                    && body["activity"]["state"] == "waiting" =>
            {
                break body
            }
            _ => {}
        }
    };
    assert_eq!(waiting["runtime_id"], a.runtime_id());
    // The action is the approval's summary, shortened for an avatar.
    let action = waiting["activity"]["action"].as_str().unwrap();
    assert!(pending["summary"]
        .as_str()
        .unwrap()
        .starts_with(action.trim_end_matches('…')));

    // An answer the gate does not offer is refused; allow once lets the run
    // on A carry on.
    let approval_id = pending["approval_id"].as_str().unwrap().to_owned();
    assert!(b
        .op(
            Operation::AgentApprovalAnswer,
            json!({"approval_id": approval_id, "answer": "allow_forever"}),
        )
        .is_err());
    let answered = b
        .op(
            Operation::AgentApprovalAnswer,
            json!({"approval_id": approval_id, "answer": "allow_once"}),
        )
        .unwrap();
    assert_eq!(answered["status"], "answered", "{answered}");
    assert!(wait(|| {
        a.tick();
        a.store().get(&run.id).unwrap().unwrap().status == RunStatus::Completed
    }));
    // Once the run ends, B's client sees the agent go idle.
    assert!(wait(|| matches!(
        received.try_recv(),
        Ok(SubscriptionEvent::AgentActivity(body))
            if body["agent_id"] == releaser.id.as_str() && body["activity"]["state"] == "idle"
    )));
    std::env::remove_var("PI_RESUME_STUB_AGENT_GATE");
}

#[test]
fn a_swarm_without_clients_for_14_days_pauses_routines_until_resumed() {
    let _environment = environment();
    let a = Member::new("absence-a");
    let b = Member::new("absence-b");
    b.join(&a);
    let mut keeper = a.agent("Keeper");
    keeper.routines.push(routine("Morning"));
    let keeper = agents::save(a.profile(), keeper).unwrap();
    assert!(wait(|| has_member(&a, &b.runtime_id())));
    for member in [&a, &b] {
        member
            .swarm()
            .advance_clock(muniment_core::chrono::Duration::days(15));
    }
    assert!(
        wait(|| agents::get(a.profile(), &keeper.id)
            .unwrap()
            .paused_reason
            .as_deref()
            == Some("absence")),
        "the routines never paused"
    );
    assert_eq!(
        a.op(Operation::SwarmMembers, json!({})).unwrap()["absence"]["paused"],
        true
    );
    // A resume on any member clears the pause on every member.
    let resumed = b.op(Operation::SwarmResume, json!({})).unwrap();
    assert!(resumed["resumed"].is_array());
    assert!(
        wait(|| agents::get(a.profile(), &keeper.id)
            .unwrap()
            .paused_reason
            .is_none()),
        "the pause never cleared"
    );
    assert_eq!(
        a.op(Operation::SwarmMembers, json!({})).unwrap()["absence"]["paused"],
        false
    );
}

fn scopes(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// Whether `member` counts `runtime_id` as online.
fn online_on(member: &Member, runtime_id: &str) -> bool {
    member
        .members()
        .iter()
        .any(|entry| entry["runtime_id"] == runtime_id && entry["online"] == true)
}

/// How many of an agent's runs on `member` took `prompt`.
fn prompted(member: &Member, agent_id: &str, prompt: &str) -> usize {
    member
        .runs(agent_id)
        .iter()
        .filter(|run| run.prompt == prompt)
        .count()
}

/// The `fleet.op_updated` bodies a client of `member` receives.
fn fleet_events(member: &Member) -> Arc<Mutex<Vec<Value>>> {
    let mut watcher = member.client(None);
    watcher.subscribe_chat_events_with_fleet(None).unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    std::thread::spawn(move || {
        while let Ok(event) = watcher.read_subscription_event() {
            if let SubscriptionEvent::FleetOpUpdated(body) = event {
                sink.lock().unwrap().push(body);
            }
        }
    });
    events
}

fn op_states(events: &Mutex<Vec<Value>>, op_id: &str) -> Vec<String> {
    events
        .lock()
        .unwrap()
        .iter()
        .filter(|event| event["op_id"] == op_id)
        .map(|event| event["state"].as_str().unwrap().to_owned())
        .collect()
}

fn op_status(client: &mut DesktopClient, op_id: &str) -> Value {
    client
        .swarm_request(Operation::FleetOpStatus, json!({"op_id": op_id}))
        .unwrap()["op"]
        .clone()
}

/// Three members in a line, so C reaches A only through B, with an agent on
/// A that ran once. Answers the agent and its primary thread.
fn line(a: &Member, b: &Member, c: &Member) -> (Agent, String) {
    b.join(a);
    c.join(b);
    let scout = a.agent("Scout");
    a.run_to_end(&scout.id, "Find leads.");
    let thread = a.primary_thread(&scout.id);
    assert!(
        wait(
            || c.swarm().is_replica_thread(&thread) == Some(a.runtime_id())
                && online_on(c, &a.runtime_id())
        ),
        "C never learned of A's thread"
    );
    (scout, thread)
}

#[test]
fn the_phones_requests_read_the_fleet_and_reach_an_items_owner_with_its_scopes() {
    let _environment = environment();
    let a = Member::new("phone-a");
    let b = Member::new("phone-b");
    let c = Member::new("phone-c");
    let (scout, thread) = line(&a, &b, &c);

    // C serves the paired phone: its reads answer from C's replicas.
    let members = c
        .state()
        .phone_request(Operation::SwarmMembers, json!({}), "req_members_0001")
        .unwrap();
    assert_eq!(members["members"].as_array().unwrap().len(), 3);
    let agents = c
        .state()
        .phone_request(Operation::SwarmAgents, json!({}), "req_agents_00001")
        .unwrap();
    assert!(agents["agents"]
        .as_array()
        .unwrap()
        .iter()
        .any(|agent| agent["agent_id"] == scout.id.as_str()
            && agent["primary_runtime"] == a.runtime_id()));

    // A post to A's thread goes to A as a forwarded action and runs there.
    let text = "Sent from the phone.";
    let posted = c
        .state()
        .phone_request(
            Operation::ThreadPost,
            json!({"thread_id": thread, "text": text}),
            "req_post_0000001",
        )
        .unwrap();
    assert_eq!(posted["forwarded"], true, "{posted}");
    assert!(
        wait(|| {
            a.tick();
            prompted(&a, &scout.id, text) == 1
        }),
        "A never ran the phone's post"
    );

    // An action past the phone's scopes, or one only the runtime's owner
    // makes, is refused before it travels.
    let refused = c
        .state()
        .phone_request(
            Operation::AgentDelete,
            json!({"id": scout.id}),
            "req_delete_00001",
        )
        .unwrap_err();
    assert_eq!(refused.code(), ErrorCode::Unauthorized);
    let excluded = c
        .state()
        .phone_request(Operation::PairingStatus, json!({}), "req_pairing_0001")
        .unwrap_err();
    assert_eq!(excluded.code(), ErrorCode::UnsupportedOperation);
}

#[test]
fn an_action_on_another_members_item_reaches_its_owner_through_a_relay_once() {
    let _environment = environment();
    let a = Member::new("fleet-a");
    let b = Member::new("fleet-b");
    let c = Member::new("fleet-c");
    let (scout, thread) = line(&a, &b, &c);
    let events = fleet_events(&c);

    // A device paired to C posts to A's thread; C's core sends it on.
    let mut device = c.client(None);
    let west = "Check the west region.";
    let posted = device
        .swarm_request(
            Operation::ThreadPost,
            json!({"thread_id": thread, "text": west}),
        )
        .unwrap();
    assert_eq!(posted["forwarded"], true, "{posted}");
    let op_id = posted["op_id"].as_str().unwrap().to_owned();
    assert_eq!(posted["message_id"], op_id.as_str());
    assert!(
        wait(|| {
            a.tick();
            prompted(&a, &scout.id, west) == 1
        }),
        "A never applied the post: {}",
        op_status(&mut device, &op_id)
    );
    assert!(wait(|| op_status(&mut device, &op_id)["state"] == "applied"));
    let op = op_status(&mut device, &op_id);
    assert_eq!(op["operation"], "thread.post");
    assert_eq!(op["owner_runtime"], a.runtime_id());
    assert_eq!(op["item"], json!({"kind": "thread", "id": thread}));
    assert_eq!(op["result"]["origin_runtime"], a.runtime_id());
    // The outbox resent the op until A's acknowledgement came back; A ran
    // it once.
    for _ in 0..5 {
        a.tick();
        c.swarm().sync_now();
    }
    assert_eq!(prompted(&a, &scout.id, west), 1);

    // C's clients saw it queued, then sent, then applied.
    assert!(wait(|| op_states(&events, &op_id)
        .last()
        .map(String::as_str)
        == Some("applied")));
    let states = op_states(&events, &op_id);
    let at = |state: &str| states.iter().position(|seen| seen == state).unwrap();
    assert_eq!(states[0], "queued", "{states:?}");
    assert!(
        at("queued") < at("sent") && at("sent") < at("applied"),
        "{states:?}"
    );

    // A repeat of one idempotency key is one op, applied once.
    let east = json!({"thread_id": thread, "text": "Then the east region."});
    let first = c
        .swarm()
        .route(
            Operation::ThreadPost,
            &east,
            &FleetActor::phone(),
            Some("east-1"),
        )
        .unwrap()
        .unwrap();
    let again = c
        .swarm()
        .route(
            Operation::ThreadPost,
            &east,
            &FleetActor::phone(),
            Some("east-1"),
        )
        .unwrap()
        .unwrap();
    assert_eq!(first["op_id"], again["op_id"]);
    assert!(wait(|| {
        a.tick();
        prompted(&a, &scout.id, "Then the east region.") == 1
    }));

    // Running A's agent from C runs it on A.
    let ran = device
        .agent_request(
            Operation::AgentRunNow,
            json!({"id": scout.id, "prompt": "Run it from C."}),
        )
        .unwrap();
    assert_eq!(ran["forwarded"], true, "{ran}");
    assert!(wait(|| {
        a.tick();
        prompted(&a, &scout.id, "Run it from C.") == 1
    }));
    assert!(c.runs(&scout.id).is_empty());

    // The owner checks the scopes the actor and its member share: a
    // read-only actor's post is refused on A.
    let viewer = FleetActor::new("viewer", scopes(&["thread.read"]));
    let refused = c
        .swarm()
        .route(
            Operation::ThreadPost,
            &json!({"thread_id": thread, "text": "Read only."}),
            &viewer,
            None,
        )
        .unwrap()
        .unwrap();
    let refused_id = refused["op_id"].as_str().unwrap().to_owned();
    assert!(wait(
        || op_status(&mut device, &refused_id)["state"] == "failed"
    ));
    assert!(op_status(&mut device, &refused_id)["error"]
        .as_str()
        .unwrap()
        .contains("run.write"));
    assert_eq!(prompted(&a, &scout.id, "Read only."), 0);
    // A device paired without run access is refused on C itself.
    let mut reader = c.client(Some(&["thread.read"]));
    assert!(reader
        .swarm_request(
            Operation::ThreadPost,
            json!({"thread_id": thread, "text": "Read only."}),
        )
        .is_err());
    // An operation off the allowlist stays on C, which does not hold the
    // agent, so A's agent survives.
    let mut manager = c.client(Some(&["thread.read", "run.write", "agents.manage"]));
    assert!(manager
        .agent_request(
            Operation::AgentDelete,
            json!({"id": scout.id, "remove_memory": false}),
        )
        .is_err());
    assert!(agents::get(a.profile(), &scout.id).is_ok());
}

#[test]
fn an_action_for_an_offline_owner_queues_until_the_owner_returns() {
    let _environment = environment();
    let mut a = Member::new("fleet-off-a");
    let b = Member::new("fleet-off-b");
    let c = Member::new("fleet-off-c");
    let (scout, thread) = line(&a, &b, &c);
    let events = fleet_events(&c);
    a.offline();
    for member in [&a, &b, &c] {
        member
            .swarm()
            .advance_clock(muniment_core::chrono::Duration::minutes(2));
    }
    assert!(wait(|| !online_on(&c, &a.runtime_id())));

    let mut device = c.client(None);
    let text = "Post while A is away.";
    let posted = device
        .swarm_request(
            Operation::ThreadPost,
            json!({"thread_id": thread, "text": text}),
        )
        .unwrap();
    let op_id = posted["op_id"].as_str().unwrap().to_owned();
    let queued = op_status(&mut device, &op_id);
    assert_eq!(queued["state"], "queued", "{queued}");
    assert_eq!(queued["reason"]["code"], "owner_offline");
    let reason = queued["reason"]["text"].as_str().unwrap();
    assert!(reason.starts_with("fleet-off-a is offline."), "{reason}");
    let last_seen = queued["reason"]["last_seen_at"].as_str().unwrap();
    assert!(reason.contains(last_seen), "{reason}");
    assert!(wait(|| op_states(&events, &op_id)
        .first()
        .map(String::as_str)
        == Some("queued")));
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(op_status(&mut device, &op_id)["state"], "queued");
    assert_eq!(prompted(&a, &scout.id, text), 0);

    a.listen();
    assert!(
        wait(|| {
            a.tick();
            prompted(&a, &scout.id, text) == 1
        }),
        "A never applied the queued post"
    );
    assert!(wait(|| op_status(&mut device, &op_id)["state"] == "applied"));
    assert!(wait(|| op_states(&events, &op_id)
        .last()
        .map(String::as_str)
        == Some("applied")));
}

#[test]
fn a_queued_action_cancels_before_its_owner_returns() {
    let _environment = environment();
    let mut a = Member::new("fleet-cancel-a");
    let b = Member::new("fleet-cancel-b");
    b.join(&a);
    let scout = a.agent("Scout");
    a.run_to_end(&scout.id, "Find leads.");
    let thread = a.primary_thread(&scout.id);
    assert!(wait(|| b.swarm().is_replica_thread(&thread)
        == Some(a.runtime_id())
        && online_on(&b, &a.runtime_id())));
    let events = fleet_events(&b);
    a.offline();
    for member in [&a, &b] {
        member
            .swarm()
            .advance_clock(muniment_core::chrono::Duration::minutes(2));
    }
    assert!(wait(|| !online_on(&b, &a.runtime_id())));

    let mut device = b.client(None);
    let text = "Post that never runs.";
    let posted = device
        .swarm_request(
            Operation::ThreadPost,
            json!({"thread_id": thread, "text": text}),
        )
        .unwrap();
    let op_id = posted["op_id"].as_str().unwrap().to_owned();
    let open = device
        .swarm_request(Operation::FleetOps, json!({"state": "open"}))
        .unwrap();
    assert_eq!(open["ops"][0]["op_id"], op_id.as_str(), "{open}");
    assert_eq!(open["ops"][0]["state"], "queued");

    let cancelled = device
        .swarm_request(Operation::FleetOpCancel, json!({"op_id": op_id}))
        .unwrap();
    assert_eq!(cancelled["op"]["state"], "cancelled", "{cancelled}");
    assert!(wait(|| op_states(&events, &op_id)
        .last()
        .map(String::as_str)
        == Some("cancelled")));
    let open = device
        .swarm_request(Operation::FleetOps, json!({"state": "open"}))
        .unwrap();
    assert_eq!(open["ops"], json!([]));

    a.listen();
    assert!(wait(|| online_on(&b, &a.runtime_id())));
    for _ in 0..5 {
        a.tick();
        std::thread::sleep(Duration::from_millis(200));
    }
    assert_eq!(prompted(&a, &scout.id, text), 0);
    assert_eq!(op_status(&mut device, &op_id)["state"], "cancelled");
}

#[test]
fn an_agent_away_a_day_offers_a_move_and_its_queued_actions_expire_in_7_days() {
    let _environment = environment();
    let mut a = Member::new("away-a");
    let b = Member::new("away-b");
    b.join(&a);
    let scout = a.agent("Scout");
    a.run_to_end(&scout.id, "Find leads.");
    let thread = a.primary_thread(&scout.id);
    assert!(wait(|| b.swarm().is_replica_thread(&thread)
        == Some(a.runtime_id())
        && online_on(&b, &a.runtime_id())));
    let suggested = |member: &Member| {
        member
            .directory()
            .iter()
            .find(|entry| entry["agent_id"] == scout.id.as_str())
            .map(|entry| entry["move_suggested"] == true)
    };
    assert_eq!(suggested(&b), Some(false));
    a.offline();
    b.swarm()
        .advance_clock(muniment_core::chrono::Duration::hours(23));
    assert_eq!(suggested(&b), Some(false));
    let text = "Post that never arrives.";
    let op_id = b
        .swarm()
        .route(
            Operation::ThreadPost,
            &json!({"thread_id": thread, "text": text}),
            &FleetActor::phone(),
            None,
        )
        .unwrap()
        .unwrap()["op_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let status = |member: &Member| {
        member
            .op(Operation::FleetOpStatus, json!({"op_id": op_id}))
            .unwrap()["op"]
            .clone()
    };
    assert_eq!(status(&b)["state"], "queued");

    // A day without A: B offers to take the agent. A runs it, so A does not.
    b.swarm()
        .advance_clock(muniment_core::chrono::Duration::hours(2));
    assert_eq!(suggested(&b), Some(true));
    assert_eq!(suggested(&a), Some(false));

    // A week without A: the post fails and stops travelling.
    b.swarm()
        .advance_clock(muniment_core::chrono::Duration::days(7));
    assert!(wait(|| status(&b)["state"] == "failed"));
    assert_eq!(status(&b)["reason"]["code"], "expired");
    a.listen();
    assert!(wait(|| b
        .members()
        .iter()
        .any(|entry| entry["runtime_id"] == a.runtime_id().as_str()
            && entry["link"]["state"] == "connected")));
    b.swarm().sync_now();
    for _ in 0..3 {
        a.tick();
        b.swarm().sync_now();
    }
    assert_eq!(prompted(&a, &scout.id, text), 0);
}

#[test]
fn a_stop_from_another_member_cancels_the_owners_live_run() {
    let _environment = environment();
    let a = Member::new("stop-a");
    let b = Member::new("stop-b");
    let c = Member::new("stop-c");
    let (scout, _thread) = line(&a, &b, &c);
    let capture = a.temporary.root.join("permission.jsonl");
    std::env::set_var("PI_RESUME_STUB_PERMISSION_CAPTURE", &capture);
    let run = agents::run_now(a.profile(), &scout.id, Some("Deploy."), Some("test")).unwrap();
    assert!(
        wait(|| {
            a.tick();
            c.op(Operation::AgentApprovals, json!({})).unwrap()["approvals"]
                .as_array()
                .unwrap()
                .len()
                == 1
        }),
        "C never saw A's run wait"
    );
    // A device on C stops the run that waits on A.
    let mut device = c.client(None);
    assert!(
        wait(|| device.run_cancel(&run.id).is_ok()),
        "C never sent the stop"
    );
    assert!(
        wait(|| {
            a.tick();
            a.store().get(&run.id).unwrap().unwrap().status == RunStatus::Cancelled
        }),
        "A's run never stopped: {:?}",
        a.store().get(&run.id).unwrap().unwrap().status
    );
    std::env::remove_var("PI_RESUME_STUB_PERMISSION_CAPTURE");
}

impl Member {
    /// Links this runtime to `other` through an invitation with `role`.
    fn join_as(&self, other: &Member, role: &str) {
        let invite = other
            .op(Operation::SwarmInvite, json!({"role": role}))
            .unwrap();
        assert_eq!(invite["role"], role);
        let joined = self
            .op(Operation::SwarmJoin, json!({"link": invite["link"]}))
            .unwrap();
        assert_eq!(joined["member"]["runtime_id"], other.runtime_id());
    }

    fn role_of(&self, other: &Member) -> Option<String> {
        self.members()
            .iter()
            .find(|entry| entry["runtime_id"] == other.runtime_id().as_str())
            .and_then(|entry| entry["fleet"]["role"].as_str().map(str::to_owned))
    }

    fn agent_dir(&self) -> std::path::PathBuf {
        muniment_core::state_root::agent_directory(self.profile())
    }

    fn auth(&self) -> muniment_core::serde_json::Map<String, Value> {
        muniment_core::models::pi_models::read_json(&self.agent_dir().join("auth.json"))
            .unwrap_or_default()
    }

    fn write_auth(&self, auth: Value) {
        std::fs::create_dir_all(self.agent_dir()).unwrap();
        muniment_core::models::pi_models::write_json(
            &self.agent_dir().join("auth.json"),
            auth.as_object().unwrap(),
        )
        .unwrap();
    }

    /// The sealed secrets this runtime holds, by id.
    fn secrets(&self) -> Vec<Value> {
        self.op(Operation::FleetCredentials, json!({})).unwrap()["secrets"]
            .as_array()
            .unwrap()
            .clone()
    }

    fn audit(&self) -> Vec<Value> {
        self.op(Operation::FleetAudit, json!({"limit": 500}))
            .unwrap()["entries"]
            .as_array()
            .unwrap()
            .clone()
    }

    /// Whether this runtime's journal holds the thread.
    fn holds_thread(&self, thread: &str) -> bool {
        let mut reader = self.reader.lock().unwrap();
        let reader = reader.get_or_insert_with(|| self.client(None));
        !history_entries(reader, thread).is_empty()
    }
}

/// An agent on `member` that ran once, on the Pi provider `provider`.
fn agent_on(member: &Member, name: &str, provider: &str) -> (Agent, String) {
    let agent = member.agent(name);
    member.run_to_end(&agent.id, "Start.");
    let mut saved = agents::get(member.profile(), &agent.id).unwrap();
    saved.model = Some(agents::AgentModel {
        provider: provider.into(),
        id: "model-1".into(),
        thinking: None,
    });
    let agent = agents::save(member.profile(), saved).unwrap();
    let thread = member.primary_thread(&agent.id);
    (agent, thread)
}

fn api_key(key: &str) -> Value {
    json!({"type": "api_key", "key": key})
}

#[test]
fn a_worker_holds_only_its_own_agents_and_relays_nothing() {
    let _environment = environment();
    let a = Member::new("role-a");
    let r = Member::new("role-r");
    let w = Member::new("role-w");
    let b = Member::new("role-b");
    // A and B are full members, R relays between A and the worker W, and B
    // reaches the fleet only through W.
    r.join(&a);
    w.join_as(&r, "worker");
    w.join_as(&b, "worker");
    assert!(wait(|| a.role_of(&w).as_deref() == Some("worker")));
    assert_eq!(w.role_of(&w), Some("worker".into()));
    assert_eq!(r.role_of(&a).as_deref(), Some("full"));

    let (scout, scout_thread) = agent_on(&a, "Scout", "openai");
    let (builder, builder_thread) = agent_on(&w, "Builder", "xai");
    a.write_auth(json!({"openai": api_key("sk-openai"), "xai": api_key("sk-xai")}));
    a.op(Operation::FleetCredentials, json!({"share_models": true}))
        .unwrap();
    a.swarm().fleet_pass().unwrap();

    // The full members hold the worker's agent and thread.
    for member in [&a, &r] {
        assert!(
            wait(
                || member.swarm().is_replica_thread(&builder_thread) == Some(w.runtime_id())
                    && member.holds_thread(&builder_thread)
            ),
            "{} never held the worker's thread",
            member.name
        );
    }
    assert!(wait(|| r.holds_thread(&scout_thread)));
    // The worker sees the directory and the members, and no content of
    // another agent: not its thread, its definition or its title.
    assert!(wait(|| w.directory().iter().any(|entry| entry["agent_id"]
        == scout.id.as_str()
        && entry["name"] == "Scout")));
    assert!(wait(|| has_member(&w, &a.runtime_id())));
    assert_eq!(w.swarm().is_replica_thread(&scout_thread), None);
    assert!(!w.holds_thread(&scout_thread));
    // Its lease names only the connection its own agent uses, without a
    // refresh token, and it holds no other secret.
    assert!(
        wait(|| {
            a.swarm().fleet_pass().unwrap();
            w.auth().get("xai").is_some()
        }),
        "the worker never got its lease"
    );
    assert!(!w.auth().contains_key("openai"));
    let held: Vec<String> = w
        .secrets()
        .iter()
        .map(|secret| secret["secret_id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        held,
        vec![format!("lease:{}:{}", a.runtime_id(), w.runtime_id())]
    );
    assert!(wait(|| r.auth().contains_key("openai")));
    // The worker gets no audit record of another member.
    assert!(wait(|| r.audit().iter().any(|entry| entry["kind"]
        == "sealed"
        && entry["actor_runtime"] == a.runtime_id())));
    assert!(w
        .audit()
        .iter()
        .all(|entry| entry["actor_runtime"] == w.runtime_id()));
    let classes = w.op(Operation::SwarmMembers, json!({})).unwrap()["sync"]["records"].clone();
    assert!(classes.get("group").is_none(), "{classes}");

    // A leaf relays nothing: B, linked only to the worker, learns the
    // worker's agent and nothing of A's.
    assert!(wait(|| b.holds_thread(&builder_thread)));
    for _ in 0..5 {
        w.swarm().sync_now();
    }
    assert!(!has_member(&b, &a.runtime_id()));
    assert!(b
        .directory()
        .iter()
        .all(|entry| entry["agent_id"] != scout.id.as_str()));
    assert!(!b.holds_thread(&scout_thread));
    assert!(!b.auth().contains_key("openai"));

    // A worker sends no action that needs agents.manage, and the owner
    // refuses one that arrives from it anyway.
    let routine_save = json!({"agent_id": scout.id, "routine": routine("Nightly")});
    let refused = w
        .swarm()
        .route(
            Operation::RoutineSave,
            &routine_save,
            &FleetActor::owner(),
            None,
        )
        .unwrap_err();
    assert!(refused.contains("agents.manage"), "{refused}");
    // A post to the worker's own agent from a full member runs there.
    let posted = a
        .swarm()
        .route(
            Operation::ThreadPost,
            &json!({"thread_id": builder_thread, "text": "Ship it."}),
            &FleetActor::owner(),
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(posted["forwarded"], true);
    assert!(wait(|| {
        w.tick();
        prompted(&w, &builder.id, "Ship it.") == 1
    }));
    // The owner logged the action with its actor, origin and result.
    assert!(
        wait(|| w.audit().iter().any(|entry| entry["kind"] == "op"
            && entry["record"]["origin"] == a.runtime_id()
            && entry["record"]["result"] == "delivered")),
        "{:?}",
        w.audit()
    );
}

/// A device that pairs with `member` as a linked runtime would, with no
/// invitation behind it.
fn raw_peer(member: &Member) -> DesktopClient {
    let link = member
        .access()
        .create_pairing_link(PairingLinkRequest {
            scopes: Some(BTreeSet::from([
                muniment_core::attach::SCOPE_SWARM_PEER.to_owned()
            ])),
            endpoints: None,
        })
        .unwrap()
        .link;
    let options = RemoteClientOptions {
        kind: "swarm-peer".into(),
        version: "1.0.0".into(),
        device_name: Some("Impostor".into()),
        scopes: Some(BTreeSet::from([
            muniment_core::attach::SCOPE_SWARM_PEER.to_owned()
        ])),
        io_timeout: Duration::from_secs(10),
    };
    pair_with_link(&link, &options).unwrap().0
}

#[test]
fn a_device_replaying_a_workers_card_gets_nothing() {
    let _environment = environment();
    let a = Member::new("replay-a");
    let w = Member::new("replay-w");
    w.join_as(&a, "worker");
    let (_builder, builder_thread) = agent_on(&w, "Builder", "xai");
    assert!(wait(|| a.holds_thread(&builder_thread)));
    assert_eq!(a.role_of(&w).as_deref(), Some("worker"));

    // Another device claims the worker's id with the worker's card, as a
    // compromised worker replaying a card would, and asks for everything.
    let mut impostor = raw_peer(&a);
    let fresh = json!({
        "runtime_id": uuid_string(),
        "runtime_name": "Impostor",
        "heartbeat_at": "2026-10-17T00:00:00Z",
        "updated_at": "2026-10-17T00:00:00Z",
        "capabilities": ["store/1", "ops/1", "fleet/1"],
    });
    let first = impostor
        .request(
            Operation::SwarmExchange,
            None,
            json!({"from": fresh, "roster_digest": "none"}),
        )
        .unwrap()
        .body;
    let card = first["roster"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["runtime_id"] == w.runtime_id().as_str() && entry["card"].is_object())
        .map(|entry| entry["card"].clone())
        .unwrap();
    let mut impostor = raw_peer(&a);
    let forged = json!({
        "runtime_id": w.runtime_id(),
        "runtime_name": "replay-w",
        "heartbeat_at": "2999-01-01T00:00:00Z",
        "updated_at": "2999-01-01T00:00:00Z",
        "capabilities": ["store/1", "ops/1", "fleet/1"],
        "fleet": card,
    });
    for _ in 0..10 {
        let answer = impostor
            .request(
                Operation::SwarmExchange,
                None,
                json!({"from": forged, "sealed_have": {}, "record_vv": {}, "roster_digest": "none"}),
            )
            .unwrap()
            .body;
        for field in [
            "records",
            "events",
            "messages",
            "sealed",
            "approvals",
            "clocks",
        ] {
            assert!(
                answer
                    .get(field)
                    .is_none_or(|value| value.as_array().map_or(
                        value.as_object().is_some_and(|map| map.is_empty()),
                        Vec::is_empty
                    )),
                "{field}: {answer}"
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn a_promotion_bootstraps_and_a_demotion_reseals_and_drops() {
    let _environment = environment();
    let a = Member::new("promote-a");
    let b = Member::new("promote-b");
    let w = Member::new("promote-w");
    b.join(&a);
    w.join_as(&a, "worker");
    let (_scout, scout_thread) = agent_on(&a, "Scout", "openai");
    let (_builder, builder_thread) = agent_on(&w, "Builder", "xai");
    a.write_auth(json!({"openai": api_key("sk-openai"), "xai": api_key("sk-xai")}));
    a.op(Operation::FleetCredentials, json!({"share_models": true}))
        .unwrap();
    let leased = |member: &Member| {
        wait(|| {
            a.swarm().fleet_pass().unwrap();
            member.auth().contains_key("xai")
        })
    };
    assert!(leased(&w), "the worker never got its lease");
    assert!(wait(|| b.holds_thread(&builder_thread)));
    assert!(!w.auth().contains_key("openai"));
    assert!(!w.holds_thread(&scout_thread));

    // A promotes the worker: it takes the classes a full member holds.
    let promoted = a
        .op(
            Operation::SwarmSetRole,
            json!({"runtime_id": w.runtime_id(), "role": "full"}),
        )
        .unwrap();
    assert_eq!(promoted["previous"], "worker");
    assert!(wait(|| w.role_of(&w).as_deref() == Some("full")));
    assert!(
        wait(
            || w.swarm().is_replica_thread(&scout_thread) == Some(a.runtime_id())
                && w.holds_thread(&scout_thread)
        ),
        "the promoted member never took A's thread"
    );
    assert!(
        wait(|| {
            a.swarm().fleet_pass().unwrap();
            w.auth().contains_key("openai")
        }),
        "the promoted member never got A's connections"
    );
    // The role change is in the audit log of every full member, the
    // promoted one among them.
    for member in [&b, &w] {
        assert!(
            wait(|| member
                .audit()
                .iter()
                .any(|entry| entry["kind"] == "role_changed"
                    && entry["actor_runtime"] == a.runtime_id()
                    && entry["record"]["to"] == "full")),
            "{} never saw the role change",
            member.name
        );
    }

    // A demotes it again: A reseals without it, and it drops what a worker
    // may not hold.
    a.op(
        Operation::SwarmSetRole,
        json!({"runtime_id": w.runtime_id(), "role": "worker"}),
    )
    .unwrap();
    assert!(wait(|| w.role_of(&w).as_deref() == Some("worker")));
    assert!(
        wait(|| !w.holds_thread(&scout_thread)
            && w.swarm().is_replica_thread(&scout_thread).is_none()),
        "the demoted member kept A's thread"
    );
    assert!(
        wait(|| {
            a.swarm().fleet_pass().unwrap();
            !w.auth().contains_key("openai") && w.auth().contains_key("xai")
        }),
        "the demoted member kept A's connections: {:?}",
        w.auth()
    );
    let lease = format!("lease:{}:{}", a.runtime_id(), w.runtime_id());
    assert!(
        wait(|| w
            .secrets()
            .iter()
            .all(|secret| secret["secret_id"] == lease.as_str())),
        "{:?}",
        w.secrets()
    );
    assert!(wait(|| w
        .audit()
        .iter()
        .all(|entry| entry["actor_runtime"] == w.runtime_id())));
    // Its own agent and thread stay with it and keep replicating.
    assert!(w.holds_thread(&builder_thread));
    let models = format!("models:{}", a.runtime_id());
    assert!(wait(|| b
        .secrets()
        .iter()
        .any(
            |secret| secret["secret_id"] == models.as_str() && secret["for_this_runtime"] == true
        )));
    let removed = a.audit();
    assert_eq!(
        removed
            .iter()
            .filter(
                |entry| entry["kind"] == "role_changed" && entry["actor_runtime"] == a.runtime_id()
            )
            .count(),
        2
    );
}

#[test]
fn a_thread_created_from_one_member_onto_another_runs_there() {
    let _environment = environment();
    let b = Member::new("offload-b");
    let c = Member::new("offload-c");
    for member in [&b, &c] {
        member.state().use_pi_artifact(member.artifact);
    }
    c.join(&b);
    assert!(wait(|| online_on(&c, &b.runtime_id())));
    std::env::set_var("PI_RESUME_STUB_REPLY", " offloaded answer");
    // A device on C starts a thread on B with its first message.
    let mut device = c.client(None);
    let key = muniment_core::attach::Id::new(uuid_string()).unwrap();
    let created = device
        .request(
            Operation::ThreadCreate,
            Some(key.clone()),
            json!({
                "runtime_id": b.runtime_id(),
                "text": "Summarize the logs.",
                "permission_mode": "bypass",
            }),
        )
        .unwrap()
        .body;
    assert_eq!(created["forwarded"], true, "{created}");
    let thread = created["thread_id"].as_str().unwrap().to_owned();
    let op_id = created["op_id"].as_str().unwrap().to_owned();
    // A repeat of the key names the same thread and op.
    let again = device
        .request(
            Operation::ThreadCreate,
            Some(key),
            json!({
                "runtime_id": b.runtime_id(),
                "text": "Summarize the logs.",
                "permission_mode": "bypass",
            }),
        )
        .unwrap()
        .body;
    assert_eq!(again["thread_id"], thread.as_str());
    assert_eq!(again["op_id"], op_id.as_str());
    assert!(
        wait(|| op_status(&mut device, &op_id)["state"] == "applied"),
        "{}",
        op_status(&mut device, &op_id)
    );
    let op = op_status(&mut device, &op_id);
    assert_eq!(op["owner_runtime"], b.runtime_id());
    assert_eq!(op["result"]["thread_id"], thread.as_str());
    assert!(op["result"]["run_id"].is_string(), "{op}");
    // The thread and its run live on B, under the id C chose, and C reads
    // B's reply in its replica.
    assert!(wait(|| b.holds_thread(&thread)));
    assert!(
        wait(|| c.swarm().thread_owner(&thread) == Some(b.runtime_id())),
        "C never learned that B runs the thread"
    );
    let mut reader = c.client(None);
    assert!(
        wait(|| history_entries(&mut reader, &thread)
            .iter()
            .any(|entry| entry["text"]
                .as_str()
                .unwrap_or("")
                .contains("offloaded answer"))),
        "C never read B's reply"
    );
}

/// The content hash of the artifact at `path` once `member` holds its
/// manifest record.
fn artifact_hash(member: &Member, path: &str) -> Option<String> {
    member
        .swarm()
        .records()
        .get("artifact", path)
        .ok()??
        .value("hash")?
        .as_str()
        .map(str::to_owned)
}

/// Starts a thread on `member` whose first message attaches the file at
/// `path`, as the desktop on that machine does, and answers its id.
fn submit_with_file(member: &Member, text: &str, path: &std::path::Path) -> String {
    use muniment_core::attach::desktop_service_message::CompanionProvenance;
    use muniment_core::attach::{Id, Protocol, Request};
    let approval = member
        .state()
        .signed_workspace_approval()
        .approval()
        .unwrap();
    let mut service = member.state().attach_service().unwrap();
    let request = Request {
        protocol: Protocol,
        request_id: Id::new(uuid_string()).unwrap(),
        operation: Operation::RunSubmit,
        capability: String::new(),
        idempotency_key: Some(Id::new(uuid_string()).unwrap()),
        body: json!({"text": text, "files": [path]}),
    };
    let provenance = CompanionProvenance {
        profile: approval.profile,
        companion_kind: "desktop".into(),
        companion_version: "1.0.0".into(),
        peer_uid: 0,
        peer_pid: 0,
    };
    let accepted = muniment_core::attach::dispatch_forwarded(
        &mut service,
        request,
        &approval.workspace,
        provenance,
    )
    .unwrap();
    accepted["thread_id"].as_str().unwrap().to_owned()
}

/// Reads a whole blob through the phone's `blob.read` on `member`, chunk
/// by chunk, waiting while the member fetches it.
fn phone_blob(member: &Member, hash: &str) -> Vec<u8> {
    use base64::Engine as _;
    let mut bytes = Vec::new();
    let mut reads = 0;
    loop {
        reads += 1;
        let read = member
            .state()
            .phone_request(
                Operation::BlobRead,
                json!({"hash": hash, "offset": bytes.len()}),
                &format!("req_blob_{reads:08}"),
            )
            .unwrap();
        if read["status"] == "fetching" {
            assert!(reads < 400, "the blob never arrived");
            member.tick();
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        assert_eq!(read["status"], "held", "{read}");
        assert_eq!(read["offset"], bytes.len());
        bytes.extend(
            base64::engine::general_purpose::STANDARD
                .decode(read["data"].as_str().unwrap())
                .unwrap(),
        );
        if read["done"] == true {
            return bytes;
        }
    }
}

#[test]
fn the_phone_starts_a_thread_on_a_chosen_member_and_reads_another_members_artifact() {
    let _environment = environment();
    let studio = Member::new("phone-studio");
    let boxed = Member::new("phone-box");
    for member in [&studio, &boxed] {
        member.state().use_pi_artifact(member.artifact);
    }
    boxed.join(&studio);
    assert!(wait(|| online_on(&studio, &boxed.runtime_id())));
    std::env::set_var("PI_RESUME_STUB_REPLY", " answered on Box");

    // The phone, paired with Studio, starts a thread on Box. Studio picks
    // the thread id and forwards the action.
    let body = json!({
        "runtime_id": boxed.runtime_id(),
        "text": "Summarize the logs.",
        "permission_mode": "edit_auto",
    });
    let created = studio
        .state()
        .phone_request(Operation::ThreadCreate, body.clone(), "req_create_000001")
        .unwrap();
    assert_eq!(created["forwarded"], true, "{created}");
    let thread = created["thread_id"].as_str().unwrap().to_owned();
    let op_id = created["op_id"].as_str().unwrap().to_owned();
    // The request id is the idempotency key: a repeat names the same thread.
    let again = studio
        .state()
        .phone_request(Operation::ThreadCreate, body, "req_create_000001")
        .unwrap();
    assert_eq!(again["thread_id"], thread.as_str());
    assert_eq!(again["op_id"], op_id.as_str());
    let status = |attempt: usize| {
        studio
            .state()
            .phone_request(
                Operation::FleetOpStatus,
                json!({"op_id": op_id}),
                &format!("req_status_{attempt:06}"),
            )
            .unwrap()["op"]
            .clone()
    };
    let mut attempt = 0;
    assert!(
        wait(|| {
            attempt += 1;
            status(attempt)["state"] == "applied"
        }),
        "{}",
        status(0)
    );
    let op = status(0);
    assert_eq!(op["owner_runtime"], boxed.runtime_id());
    assert_eq!(op["result"]["thread_id"], thread.as_str());
    assert!(op["result"]["run_id"].is_string(), "{op}");

    // The thread and its run live on Box, in the mode the phone chose.
    // Studio holds a replica and reads Box's reply.
    assert!(wait(|| boxed.holds_thread(&thread)));
    assert_eq!(thread_mode(&mut boxed.client(None), &thread), "edit_auto");
    assert!(
        wait(|| studio.swarm().is_replica_thread(&thread) == Some(boxed.runtime_id())),
        "Studio never learned that Box runs the thread"
    );
    assert!(
        wait(|| {
            studio
                .state()
                .phone_request(
                    Operation::ThreadHistory,
                    json!({"thread_id": thread, "limit": 10}),
                    "req_history_00001",
                )
                .ok()
                .and_then(|history| history["entries"].as_array().cloned())
                .unwrap_or_default()
                .iter()
                .any(|entry| {
                    entry["text"]
                        .as_str()
                        .unwrap_or("")
                        .contains("answered on Box")
                })
        }),
        "the phone never read Box's reply through Studio"
    );

    // An artifact Box owns, larger than one chunk: Studio holds only its
    // manifest, fetches the bytes on the phone's first read and answers
    // them chunk by chunk.
    let bytes: Vec<u8> = (0..300_000u32)
        .map(|index| (index.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    let file = boxed
        .temporary
        .root
        .join("home/artifacts/report/summary.bin");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, &bytes).unwrap();
    let mut hash = None;
    assert!(
        wait(|| {
            boxed.tick();
            studio.tick();
            hash = artifact_hash(&studio, "report/summary.bin");
            hash.is_some()
        }),
        "Studio never held Box's artifact manifest"
    );
    let hash = hash.unwrap();
    assert!(!studio
        .temporary
        .root
        .join("home/artifacts/report/summary.bin")
        .exists());
    assert_eq!(phone_blob(&studio, &hash), bytes);

    // A thread on Box whose first message attaches a file: the phone finds
    // the attachment's hash in `thread.history` and reads its bytes through
    // Studio's replica, and through Box, which runs the thread.
    let attached: Vec<u8> = (0..100_000u32)
        .map(|index| b'a' + (index % 26) as u8)
        .collect();
    let path = boxed.temporary.root.join("notes.txt");
    std::fs::write(&path, &attached).unwrap();
    let thread = submit_with_file(&boxed, "Read the notes.", &path);
    let mut sha256 = None;
    assert!(
        wait(|| {
            sha256 = studio
                .state()
                .phone_request(
                    Operation::ThreadHistory,
                    json!({"thread_id": thread, "limit": 10}),
                    "req_history_00002",
                )
                .ok()
                .and_then(|history| {
                    history["entries"][0]["attachments"][0]["sha256"]
                        .as_str()
                        .map(str::to_owned)
                });
            sha256.is_some()
        }),
        "Studio never listed the attachment's hash"
    );
    let sha256 = sha256.unwrap();
    assert_eq!(phone_blob(&studio, &sha256), attached);
    assert_eq!(phone_blob(&boxed, &sha256), attached);
    // A hash no fleet item names is refused.
    let refused = studio
        .state()
        .phone_request(
            Operation::BlobRead,
            json!({"hash": "0".repeat(64)}),
            "req_blob_unknown",
        )
        .unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("No fleet item names that blob"),
        "{refused:?}"
    );
}

/// The desktop account of the live control session test: the relay keys,
/// access token and pair that the phone probe wrote after it paired both
/// sides with a local cloud.
struct ProbeAccount {
    api: String,
    file: std::path::PathBuf,
}

impl ProbeAccount {
    fn read(&self) -> Option<Value> {
        muniment_core::serde_json::from_slice(&std::fs::read(&self.file).ok()?).ok()
    }
}

impl AccountService for ProbeAccount {
    fn session(
        &self,
        _runtime_activity: &muniment_core::attach::RuntimeActivityRegistry,
    ) -> Result<AccountSession, String> {
        Ok(AccountSession {
            tokens: self.read().map(|paired| muniment_core::account::TokenSet {
                access_token: paired["access_token"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                refresh_token: None,
                expires_at: None,
                subject: None,
            }),
            entitlement_snapshot: None,
        })
    }

    fn cloud_api_base(&self) -> Option<String> {
        Some(self.api.clone())
    }

    fn pairing_status(
        &self,
        _access_token: &str,
        _profile_directory: &std::path::Path,
    ) -> Result<muniment_core::account::PairingStatusView, muniment_core::attach::ProtocolError>
    {
        let paired = self
            .read()
            .ok_or_else(muniment_core::attach::ProtocolError::unauthorized)?;
        muniment_core::serde_json::from_value(json!({"pair": {
            "pair_id": paired["pair_id"],
            "mobile_device_id": paired["mobile_device_id"],
            "created_at": paired["created_at"],
        }}))
        .map_err(|_| muniment_core::attach::ProtocolError::invalid_request())
    }

    fn relay_keys(
        &self,
        _profile_directory: &std::path::Path,
    ) -> Option<muniment_core::relay_encryption::RelayKeys> {
        use muniment_core::relay_encryption::{DirectionKey, RelayKeys};
        let paired = self.read()?;
        Some(RelayKeys {
            pair_id: paired["pair_id"].as_str()?.to_owned(),
            desktop_to_phone: DirectionKey::decode(paired["desktop_to_phone"].as_str()?).ok()?,
            phone_to_desktop: DirectionKey::decode(paired["phone_to_desktop"].as_str()?).ok()?,
        })
    }
}

/// The desktop side of the phone probe: two members, Studio and Box, with an
/// agent and a thread on each, and Studio's relay publisher on a local cloud.
/// Run it with the probe that pairs, reads the fleet and acts on it:
///
/// ```sh
/// MUNIMENT_RELAY_CONTROL_API=http://127.0.0.1:58787 \
/// MUNIMENT_RELAY_CONTROL_DIR=/tmp/control-probe \
/// cargo test -p muniment-runtime-core --test swarm a_phone_probe -- --ignored --nocapture
/// ```
///
/// The probe writes `desktop.json` with the pair, reads `fleet.json` (with
/// the hash and SHA-256 of an artifact on Box, and a Box thread with an
/// attachment), and writes `done.json` with
/// the text it posted to Box's thread and the thread it started on Box.
#[test]
#[ignore = "needs a local cloud API and the phone probe"]
fn a_phone_probe_reads_and_acts_on_the_fleet_through_the_relay_control_session() {
    let (Ok(api), Ok(directory)) = (
        std::env::var("MUNIMENT_RELAY_CONTROL_API"),
        std::env::var("MUNIMENT_RELAY_CONTROL_DIR"),
    ) else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    let _environment = environment();
    let studio = Member::with_account(
        "Studio",
        Some(Arc::new(ProbeAccount {
            api,
            file: directory.join("desktop.json"),
        })),
    );
    let boxed = Member::new("Box");
    boxed.join(&studio);
    let scout = boxed.agent("Scout");
    boxed.run_to_end(&scout.id, "Find leads.");
    let ledger = studio.agent("Ledger");
    studio.run_to_end(&ledger.id, "Balance the books.");
    let scout_thread = boxed.primary_thread(&scout.id);
    assert!(wait(|| {
        studio.tick();
        boxed.tick();
        studio.swarm().is_replica_thread(&scout_thread) == Some(boxed.runtime_id())
    }));
    for member in [&studio, &boxed] {
        member.state().use_pi_artifact(member.artifact);
    }
    // An artifact on Box, larger than one `blob.read` chunk, that the phone
    // reads through Studio.
    let bytes: Vec<u8> = (0..300_000u32)
        .map(|index| (index.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    let file = boxed.temporary.root.join("home/artifacts/probe/report.bin");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, &bytes).unwrap();
    let mut artifact = None;
    assert!(wait(|| {
        studio.tick();
        boxed.tick();
        artifact = artifact_hash(&studio, "probe/report.bin");
        artifact.is_some()
    }));
    let digest = |bytes: &[u8]| {
        use muniment_core::sha2::{Digest, Sha256};
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let sha256 = digest(&bytes);
    // A thread on Box whose first message attaches a file, which the phone
    // finds in the thread's history and reads through Studio.
    let attached: Vec<u8> = (0..100_000u32)
        .map(|index| b'a' + (index % 26) as u8)
        .collect();
    let notes = boxed.temporary.root.join("notes.txt");
    std::fs::write(&notes, &attached).unwrap();
    let attachment_thread = submit_with_file(&boxed, "Read the notes.", &notes);
    assert!(wait(|| {
        studio.tick();
        boxed.tick();
        studio.swarm().is_replica_thread(&attachment_thread) == Some(boxed.runtime_id())
    }));
    let (stop, commands) = mpsc::channel();
    let schedule = muniment_runtime_core::start_retention_schedule(
        Arc::clone(studio.state()),
        Duration::from_secs(3600),
        commands,
        None,
    );
    std::fs::write(
        directory.join("fleet.json"),
        json!({
            "studio": studio.runtime_id(), "box": boxed.runtime_id(),
            "scout": scout.id, "scout_thread": scout_thread,
            "ledger": ledger.id, "ledger_thread": studio.primary_thread(&ledger.id),
            "artifact_hash": artifact, "artifact_size": bytes.len(), "artifact_sha256": sha256,
            "attachment_thread": attachment_thread, "attachment_size": attached.len(),
            "attachment_sha256": digest(&attached),
        })
        .to_string(),
    )
    .unwrap();
    let done = directory.join("done.json");
    let deadline = Instant::now() + Duration::from_secs(600);
    while !done.exists() {
        assert!(Instant::now() < deadline, "the probe never finished");
        studio.tick();
        boxed.tick();
        std::thread::sleep(Duration::from_millis(100));
    }
    let report: Value =
        muniment_core::serde_json::from_slice(&std::fs::read(&done).unwrap()).unwrap();
    let posted = report["posted"].as_str().unwrap().to_owned();
    assert!(
        wait(|| {
            boxed.tick();
            prompted(&boxed, &scout.id, &posted) == 1
        }),
        "Box never ran the phone's post"
    );
    // The thread the phone started on Box runs there, and Studio holds it
    // as a replica.
    let created = report["created_thread"].as_str().unwrap().to_owned();
    assert!(wait(|| boxed.holds_thread(&created)));
    assert_eq!(
        studio.swarm().is_replica_thread(&created),
        Some(boxed.runtime_id())
    );
    stop.send(muniment_runtime_core::RetentionScheduleCommand::Stop)
        .unwrap();
    schedule.join().unwrap();
}

#[test]
fn a_message_that_mentions_an_agent_reaches_it_through_the_messaging_path() {
    let _environment = environment();
    let a = Member::new("mention-a");
    let planner = a.agent("Planner");
    let analyst = a.agent("Analyst");
    let start = a.run_to_end(&planner.id, "Plan the week.");
    let planner_thread = a.primary_thread(&planner.id);

    // In an agent's thread the agent sends it, so its messaging list applies.
    let mut limited = agents::get(a.profile(), &planner.id).unwrap();
    limited.messaging = Some(AgentMessaging {
        allow: MessagingAllow::Agents(vec![uuid_string()]),
    });
    agents::save(a.profile(), limited).unwrap();
    let refused = a.swarm().mention(
        &start.id,
        &planner_thread,
        "Check the deals.",
        std::slice::from_ref(&analyst.id),
    );
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0]["sent"], false);
    assert_eq!(refused[0]["name"], "Analyst");
    assert_eq!(
        refused[0]["error"],
        "Planner may not message Analyst. Choose agents under Messages to other agents in its profile."
    );
    let mut open = agents::get(a.profile(), &planner.id).unwrap();
    open.messaging = Some(AgentMessaging {
        allow: MessagingAllow::Any,
    });
    agents::save(a.profile(), open).unwrap();
    let sent = a.swarm().mention(
        &start.id,
        &planner_thread,
        "Check the deals.",
        &[analyst.id.clone(), analyst.id.clone()],
    );
    assert_eq!(
        sent,
        vec![json!({"agent_id": analyst.id, "name": "Analyst", "sent": true})]
    );
    let request = has_prompt(&a.runs(&analyst.id), "<agent_message").unwrap();
    assert_eq!(request.trigger.kind, TriggerKind::AgentMessage);
    assert!(request.prompt.contains("intent=\"request\""));
    assert!(request.prompt.contains("Check the deals."));

    // In any other thread the user sends it, as a message to the agent.
    let chat = uuid::Uuid::new_v4().to_string();
    let posted = a.swarm().mention(
        &start.id,
        &chat,
        "Summarize the week.",
        std::slice::from_ref(&analyst.id),
    );
    assert_eq!(posted[0]["sent"], true);
    assert!(a
        .runs(&analyst.id)
        .iter()
        .any(|run| run.prompt == "Summarize the week." && run.trigger.kind == TriggerKind::Manual));
}
