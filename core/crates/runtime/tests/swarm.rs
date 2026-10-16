#![cfg(unix)]
//! Runtimes linked into a swarm over loopback: links and their revocation,
//! the agent directory, thread replication on the journal, moves, agent
//! messaging, group chats, approvals across members and the absence pause.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use muniment_core::agent_runs::{NewRun, Run, RunStatus, RunStore, RunTrigger, TriggerKind};
use muniment_core::agents::{self, Agent, AgentMessaging, MessagingAllow, Routine};
use muniment_core::attach::{
    connect_remote_runtime, pair_with_link, DesktopClient, ErrorCode, InstallKind, Operation,
    PairingLinkRequest, RemoteAccess, RemoteAccessOptions, RemoteClientOptions, RemoteListener,
    RemoteListenerConfig, RemoteListenerError, RemoteRuntimeRecord, SubscriptionEvent,
};
use muniment_core::serde_json::{json, Value};
use muniment_core::sidecar::pi_install::PiArtifactDescriptor;
use muniment_core::swarm::Swarm;
use muniment_runtime_core::{RuntimeAttachState, RuntimeHost};

mod common;
use common::TemporaryProfile;

/// The tests share process-wide stub settings, so they run one at a time.
static ENVIRONMENT: Mutex<()> = Mutex::new(());

const STUB_SETTINGS: [&str; 4] = [
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
}

impl Member {
    fn new(name: &'static str) -> Self {
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
        };
        member.open();
        member
    }

    fn open(&mut self) {
        let profile = &self.temporary.profile;
        let state =
            Arc::new(RuntimeAttachState::open(RuntimeHost::default(), profile, profile).unwrap());
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
        self.store().get(&run.id).unwrap().unwrap()
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
        .run_submit("Write here.", &[], Some(&thread), None)
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
    // Five replies and the end note reach the meeting thread.
    let posts = group_posts(&mut reader, &meeting_thread);
    assert_eq!(
        posts
            .iter()
            .filter(|post| post["author"]["kind"] == "agent")
            .count(),
        5,
        "{posts:#?}"
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
