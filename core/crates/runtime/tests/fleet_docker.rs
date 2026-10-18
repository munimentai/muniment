#![cfg(unix)]
//! A fleet of three runtime containers and this process's runtime, linked
//! as full members over a Docker network: a host with a long history that
//! joins a container as the app does, offloaded threads with stop and steer
//! from the host, settings,
//! preferences and memory with a conflict copy, sealed model connections
//! and a revocation, a partition, delay and loss, a queued action for an
//! owner that stopped, and the phone's control session.
//!
//! `scripts/fleet-e2e.sh` builds the images, starts the containers and runs
//! this test with the variables below. Run alone, it does nothing.
//!
//! - `MUNIMENT_FLEET_E2E=1` turns it on.
//! - `MUNIMENT_FLEET_E2E_CONTAINERS`: the three container names, comma
//!   separated. Each serves remote access with the stub in place of Pi.
//! - `MUNIMENT_FLEET_E2E_NETWORK`: the Docker network that links them.
//! - `MUNIMENT_FLEET_E2E_NETEM_IMAGE`: an image whose entrypoint is `tc`.
//! - `MUNIMENT_FLEET_E2E_HOST_LISTEN`: where this runtime listens
//!   (`127.0.0.1:0`), and `MUNIMENT_FLEET_E2E_HOST_ENDPOINT` the host name
//!   containers reach it by (`host.docker.internal`).
//! - `MUNIMENT_FLEET_E2E_TIMEOUT`: seconds each convergence may take (240).
//! - The phone step runs when `MUNIMENT_FLEET_E2E_CLOUD_API`, `..._EMAIL`,
//!   `..._PASSWORD` and `MUNIMENT_FLEET_E2E_PHONE_PROBE` (the Node probe)
//!   are set.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use muniment_core::attach::{
    connect_remote_runtime, pair_with_link, ClientError, DesktopClient, Id, InstallKind,
    ModelAction, Operation, PairingLink, PairingLinkRequest, RemoteAccess, RemoteAccessOptions,
    RemoteClientOptions, RemoteListener, RemoteListenerConfig, RemoteListenerError,
    RemoteRuntimeRecord,
};
use muniment_core::serde_json::{self, json, Value};
use muniment_runtime_core::host::{AccountService, AccountSession, FileCredentialStore};
use muniment_runtime_core::{RetentionScheduleCommand, RuntimeAttachState, RuntimeHost};

mod common;
use common::TemporaryProfile;

const SCOPES: [&str; 3] = ["thread.read", "run.write", "runtime.admin"];
const HOST_NAME: &str = "fleet-host";
const CONTAINER_MEMORY: &str = "/data/home/memory";
const CONTAINER_AUTH: &str = "/data/agent/auth.json";
const API_KEY: &str = "sk-fleet-e2e-0000";
/// The files a member writes when it applies shared model connections.
const SHARED_FILES: [&str; 3] = [
    "/data/agent/muniment-shared.json",
    "/data/agent/muniment-router.json",
    "/data/agent/muniment-router-endpoint.json",
];
/// A prompt the stub holds open until a steer or a stop.
const HOLD_MARKER: &str = "[hold the run]";

fn started() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

fn log(message: impl AsRef<str>) {
    eprintln!(
        "fleet-e2e {:>7.1}s {}",
        started().elapsed().as_secs_f64(),
        message.as_ref()
    );
}

fn env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn timeout() -> Duration {
    Duration::from_secs(
        env("MUNIMENT_FLEET_E2E_TIMEOUT")
            .and_then(|value| value.parse().ok())
            .unwrap_or(240),
    )
}

/// Polls `check` until it answers `Ok`, and answers how long that took. On
/// timeout it fails with the last answer.
fn until(what: &str, check: impl FnMut() -> Result<(), String>) -> Duration {
    until_within(what, timeout(), check)
}

fn until_within(
    what: &str,
    limit: Duration,
    mut check: impl FnMut() -> Result<(), String>,
) -> Duration {
    let begin = Instant::now();
    let mut last = String::from("never checked");
    while begin.elapsed() < limit {
        match check() {
            Ok(()) => {
                let took = begin.elapsed();
                log(format!("{what} ({:.1}s)", took.as_secs_f64()));
                return took;
            }
            Err(state) => last = state,
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    panic!("{what} did not happen within {limit:?}: {last}");
}

/// Runs `check` on every node and answers every failure, not only the first.
fn every(
    nodes: &[&Node],
    mut check: impl FnMut(&Node) -> Result<(), String>,
) -> Result<(), String> {
    let failures: Vec<String> = nodes.iter().filter_map(|node| check(node).err()).collect();
    ensure(failures.is_empty(), || failures.join("; "))
}

fn ensure(condition: bool, state: impl FnOnce() -> String) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(state())
    }
}

fn docker(args: &[&str]) -> Result<String, String> {
    let output = Command::new("docker")
        .args(args)
        .output()
        .map_err(|error| format!("docker did not start: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(format!(
            "docker {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn options() -> RemoteClientOptions {
    RemoteClientOptions {
        kind: "desktop-client".into(),
        version: "1.0.0".into(),
        device_name: Some("fleet-e2e".into()),
        scopes: Some(SCOPES.iter().map(|scope| (*scope).to_owned()).collect()),
        io_timeout: Duration::from_secs(20),
    }
}

fn key() -> Id {
    Id::new(uuid::Uuid::new_v4().to_string()).unwrap()
}

/// One member as a paired device sees it: the host's own runtime or a
/// container. The device reconnects after a transport failure.
struct Node {
    name: String,
    container: Option<String>,
    runtime_id: String,
    record: RemoteRuntimeRecord,
    client: Mutex<Option<DesktopClient>>,
}

impl Node {
    fn paired(name: &str, container: Option<&str>, link: &PairingLink) -> Result<Self, String> {
        let (client, record) =
            pair_with_link(link, &options()).map_err(|error| format!("{name}: {error:?}"))?;
        Ok(Self {
            name: name.to_owned(),
            container: container.map(str::to_owned),
            runtime_id: link.runtime_id.as_str().to_owned(),
            record,
            client: Mutex::new(Some(client)),
        })
    }

    /// Pairs with a container through `muniment-server pair`, as an
    /// operator does, once its runtime serves.
    fn container(container: &str) -> Self {
        let scopes = SCOPES.join(",");
        let mut node = None;
        until_within(
            &format!("{container} paired"),
            Duration::from_secs(180),
            || {
                let printed = docker(&[
                    "exec",
                    container,
                    "muniment-server",
                    "pair",
                    "--scopes",
                    &scopes,
                    "--json",
                ])?;
                let line = printed
                    .lines()
                    .find(|line| line.starts_with('{'))
                    .ok_or_else(|| format!("pair printed {printed}"))?;
                let answer: Value =
                    serde_json::from_str(line).map_err(|error| error.to_string())?;
                let link = PairingLink::parse(answer["link"].as_str().unwrap_or_default())
                    .map_err(|error| format!("{error:?}"))?;
                let name = answer["runtime_name"].as_str().unwrap_or(container);
                node = Some(Self::paired(name, Some(container), &link)?);
                Ok(())
            },
        );
        node.unwrap()
    }

    fn with<T>(
        &self,
        action: impl FnOnce(&mut DesktopClient) -> Result<T, ClientError>,
    ) -> Result<T, String> {
        let mut slot = self
            .client
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if slot.is_none() {
            *slot = Some(
                connect_remote_runtime(&self.record, &options())
                    .map_err(|error| format!("{} unreachable: {error:?}", self.name))?,
            );
        }
        let client = slot.as_mut().unwrap();
        match action(client) {
            Ok(value) => Ok(value),
            Err(error) => {
                if let Some(refused) = client.last_request_error() {
                    return Err(format!("{} refused: {refused}", self.name));
                }
                *slot = None;
                Err(format!("{}: {error:?}", self.name))
            }
        }
    }

    fn call(&self, operation: Operation, key: Option<Id>, body: Value) -> Result<Value, String> {
        self.with(|client| {
            client
                .request(operation, key, body)
                .map(|response| response.body)
        })
    }

    fn members(&self) -> Result<Vec<Value>, String> {
        list(
            self.call(Operation::SwarmMembers, None, json!({}))?,
            "members",
        )
    }

    fn agents(&self) -> Result<Vec<Value>, String> {
        list(
            self.call(Operation::SwarmAgents, None, json!({}))?,
            "agents",
        )
    }

    fn agent_named(&self, name: &str) -> Result<Option<Value>, String> {
        Ok(self
            .agents()?
            .into_iter()
            .find(|agent| agent["name"] == name))
    }

    fn summary(&self, thread: &str) -> Result<Option<Value>, String> {
        let page = self.with(|client| client.thread_summaries(100, None))?;
        Ok(page["summaries"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|summary| summary["threadId"] == thread)
            .cloned())
    }

    fn history(&self, thread: &str) -> Result<Vec<Value>, String> {
        let page = self.with(|client| client.thread_history(thread, 50, None))?;
        Ok(page["entries"].as_array().cloned().unwrap_or_default())
    }

    fn preference(&self, name: &str) -> Result<Value, String> {
        Ok(self.call(Operation::PreferencesGet, None, json!({}))?["values"][name].clone())
    }

    fn op_status(&self, op_id: &str) -> Result<Value, String> {
        Ok(self.call(Operation::FleetOpStatus, None, json!({"op_id": op_id}))?["op"].clone())
    }

    fn credentials(&self) -> Result<Value, String> {
        self.call(Operation::FleetCredentials, None, json!({}))
    }

    fn audit(&self) -> Result<Vec<Value>, String> {
        list(
            self.call(Operation::FleetAudit, None, json!({"limit": 500}))?,
            "entries",
        )
    }

    fn container_name(&self) -> &str {
        self.container.as_deref().expect("a container member")
    }
}

fn list(mut body: Value, field: &str) -> Result<Vec<Value>, String> {
    match body.get_mut(field).map(Value::take) {
        Some(Value::Array(items)) => Ok(items),
        _ => Err(format!("no {field} in {body}")),
    }
}

/// The desktop account of the phone step: the relay keys, access token and
/// pair the phone probe writes after it paired both sides with the cloud.
struct ProbeAccount {
    api: String,
    file: PathBuf,
}

impl ProbeAccount {
    fn read(&self) -> Option<Value> {
        serde_json::from_slice(&std::fs::read(&self.file).ok()?).ok()
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
        _profile_directory: &Path,
    ) -> Result<muniment_core::account::PairingStatusView, muniment_core::attach::ProtocolError>
    {
        let paired = self
            .read()
            .ok_or_else(muniment_core::attach::ProtocolError::unauthorized)?;
        serde_json::from_value(json!({"pair": {
            "pair_id": paired["pair_id"],
            "mobile_device_id": paired["mobile_device_id"],
            "created_at": paired["created_at"],
        }}))
        .map_err(|_| muniment_core::attach::ProtocolError::invalid_request())
    }

    fn relay_keys(
        &self,
        _profile_directory: &Path,
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

/// The phone step's settings, when every one is set.
struct Phone {
    api: String,
    email: String,
    password: String,
    probe: PathBuf,
    directory: PathBuf,
}

impl Phone {
    fn from_env(root: &Path) -> Option<Self> {
        let phone = Self {
            api: env("MUNIMENT_FLEET_E2E_CLOUD_API")?,
            email: env("MUNIMENT_FLEET_E2E_CLOUD_EMAIL")?,
            password: env("MUNIMENT_FLEET_E2E_CLOUD_PASSWORD")?,
            probe: PathBuf::from(env("MUNIMENT_FLEET_E2E_PHONE_PROBE")?),
            directory: root.join("phone"),
        };
        std::fs::create_dir_all(&phone.directory).unwrap();
        Some(phone)
    }
}

/// This process's runtime, run the way a desktop host runs it: its remote
/// listener, the swarm worker and the schedule that starts runs and the
/// relay publisher.
struct HostRuntime {
    temporary: TemporaryProfile,
    state: Arc<RuntimeAttachState>,
    access: RemoteAccess,
    port: u16,
    stop: Option<mpsc::Sender<()>>,
    server: Option<std::thread::JoinHandle<Result<(), RemoteListenerError>>>,
    schedule_stop: mpsc::Sender<RetentionScheduleCommand>,
    schedule: Option<std::thread::JoinHandle<()>>,
}

impl HostRuntime {
    fn open(temporary: TemporaryProfile, account: Option<Arc<dyn AccountService>>) -> Self {
        let profile = temporary.profile.clone();
        muniment_core::home::confirm_home(&profile, &temporary.root.join("home")).unwrap();
        std::fs::write(
            profile.join(muniment_core::local_mode::LOCAL_MODE_MARKER),
            "1",
        )
        .unwrap();
        common::use_test_prompt_store();
        let artifact = common::stage_pi_stub(&profile);
        let mut host = RuntimeHost {
            credentials: Arc::new(FileCredentialStore::new(temporary.root.join("credentials"))),
            ..RuntimeHost::default()
        };
        if let Some(account) = account {
            host.account = account;
        }
        let state = Arc::new(RuntimeAttachState::open(host, &profile, &profile).unwrap());
        state.use_pi_artifact(artifact);
        let access = state
            .open_remote_access(RemoteAccessOptions {
                runtime_name: Some(HOST_NAME.into()),
                runtime_version: state.runtime_version().into(),
                install_kind: InstallKind::Server,
            })
            .unwrap();
        let listen = env("MUNIMENT_FLEET_E2E_HOST_LISTEN").unwrap_or_else(|| "127.0.0.1:0".into());
        let listener =
            RemoteListener::bind(RemoteListenerConfig::new(listen.parse().unwrap())).unwrap();
        let port = listener.local_addr().unwrap().port();
        let reach = env("MUNIMENT_FLEET_E2E_HOST_ENDPOINT")
            .unwrap_or_else(|| "host.docker.internal".into());
        access
            .set_advertised_endpoints(vec![format!("{reach}:{port}"), format!("127.0.0.1:{port}")])
            .unwrap();
        let (stop, stopped) = mpsc::channel();
        let serving = Arc::clone(&state);
        let serving_access = access.clone();
        let server = std::thread::spawn(move || {
            serving.serve_remote_listener(listener, serving_access, stopped)
        });
        let (schedule_stop, commands) = mpsc::channel();
        let schedule = muniment_runtime_core::start_retention_schedule(
            Arc::clone(&state),
            Duration::from_secs(3600),
            commands,
            None,
        );
        Self {
            temporary,
            state,
            access,
            port,
            stop: Some(stop),
            server: Some(server),
            schedule_stop,
            schedule: Some(schedule),
        }
    }

    /// A device paired with this runtime over loopback.
    fn device(&self) -> Node {
        let link = self
            .access
            .create_pairing_link(PairingLinkRequest {
                scopes: Some(SCOPES.iter().map(|scope| (*scope).to_owned()).collect()),
                endpoints: Some(vec![format!("127.0.0.1:{}", self.port)]),
            })
            .unwrap()
            .link;
        Node::paired(HOST_NAME, None, &link).unwrap()
    }

    fn memory(&self) -> PathBuf {
        self.temporary.root.join("home").join("memory")
    }

    /// Gives this runtime `threads` threads of `posts` messages each: more
    /// items than one exchange frame holds, as a desktop holds after some
    /// days of chats.
    fn seed_history(&self, threads: usize, posts: usize) {
        use muniment_core::journal::{
            replica::THREAD_MESSAGE_POSTED, Provenance, ThreadEventEnvelope,
        };
        let storage = muniment_runtime_core::open_profile_storage(&self.temporary.profile).unwrap();
        let mut storage = storage.lock().unwrap();
        let now = muniment_core::chrono::Utc::now()
            .to_rfc3339_opts(muniment_core::chrono::SecondsFormat::AutoSi, true);
        let provenance = Provenance {
            source: "fleet-e2e".into(),
            source_version: "1".into(),
            actor_id: None,
            device_id: None,
            rpc_request_id: None,
            capability_versions: None,
            extra: [("attach_profile".to_owned(), json!("desktop-owner"))]
                .into_iter()
                .collect(),
        };
        for thread in 0..threads {
            let id = storage
                .journal
                .create_thread("local", &now, provenance.clone())
                .unwrap();
            for post in 0..posts {
                let message = uuid::Uuid::now_v7().to_string();
                storage
                    .journal
                    .append_thread_event_with_id(ThreadEventEnvelope::new(
                        &message,
                        &id,
                        THREAD_MESSAGE_POSTED,
                        &now,
                        json!({"message_id": message, "author": {"kind": "agent"}, "text": format!("Note {post} of thread {thread}."), "deps": {}}),
                        provenance.clone(),
                    ))
                    .unwrap();
            }
        }
    }

    fn write_auth(&self, auth: &Value) {
        let directory = muniment_core::state_root::agent_directory(&self.temporary.profile);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("auth.json"), auth.to_string()).unwrap();
    }

    /// A ChatGPT subscription in the router pool, signed in for a day.
    fn write_subscription(&self) {
        use muniment_core::model_router::config::{self, Account, Credential};
        let agent = muniment_core::state_root::agent_directory(&self.temporary.profile);
        let mut settings = config::load(&agent).unwrap();
        settings.enabled = true;
        settings.accounts.push(Account {
            id: "chatgpt".into(),
            family: "openai".into(),
            label: "ChatGPT".into(),
            credential: Credential::Subscription {
                provider: "openai-codex".into(),
                access: "fleet-e2e-access".into(),
                refresh: Some("fleet-e2e-refresh".into()),
                expires_ms: Some(
                    muniment_core::chrono::Utc::now().timestamp_millis() + 24 * 60 * 60_000,
                ),
                account_id: None,
                email: None,
                plan: None,
                renews_at_ms: None,
            },
            base_url: None,
            models: Vec::new(),
            enabled: true,
            weight: 1,
            overflow: false,
        });
        config::save(&agent, &settings).unwrap();
    }
}

impl Drop for HostRuntime {
    fn drop(&mut self) {
        let _ = self.schedule_stop.send(RetentionScheduleCommand::Stop);
        if let Some(schedule) = self.schedule.take() {
            let _ = schedule.join();
        }
        let registry = self.state.run_registry();
        for identity in registry.identities() {
            let _ = muniment_core::active_run::cancel_active_run(&registry, &identity.run_id, None);
        }
        self.state.swarm().stop();
        drop(self.stop.take());
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
    }
}

/// The text of a memory file on `node`, or `None` when it has none.
fn memory_file(host: &HostRuntime, node: &Node, name: &str) -> Option<String> {
    match &node.container {
        None => std::fs::read_to_string(host.memory().join(name)).ok(),
        Some(container) => docker(&[
            "exec",
            container,
            "cat",
            &format!("{CONTAINER_MEMORY}/{name}"),
        ])
        .ok(),
    }
}

/// The names of the memory files on `node` that start with `stem`.
fn memory_names(host: &HostRuntime, node: &Node, stem: &str) -> Result<BTreeSet<String>, String> {
    let names: Vec<String> = match &node.container {
        None => std::fs::read_dir(host.memory())
            .map_err(|error| error.to_string())?
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect(),
        Some(container) => docker(&["exec", container, "ls", "-1", CONTAINER_MEMORY])?
            .lines()
            .map(str::to_owned)
            .collect(),
    };
    Ok(names
        .into_iter()
        .filter(|name| name.starts_with(stem))
        .collect())
}

fn write_memory(host: &HostRuntime, node: &Node, name: &str, text: &str) {
    match &node.container {
        None => std::fs::write(host.memory().join(name), text).unwrap(),
        Some(container) => {
            let mut child = Command::new("docker")
                .args(["exec", "-i", container, "sh", "-c", "cat > \"$1\"", "sh"])
                .arg(format!("{CONTAINER_MEMORY}/{name}"))
                .stdin(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(text.as_bytes())
                .unwrap();
            assert!(child.wait().unwrap().success(), "{container}: write {name}");
        }
    }
}

fn remove_memory(host: &HostRuntime, name: &str) {
    std::fs::remove_file(host.memory().join(name)).unwrap();
}

fn container_auth(node: &Node) -> Value {
    docker(&["exec", node.container_name(), "cat", CONTAINER_AUTH])
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

fn save_agent(device: &Node, name: &str) -> String {
    let saved = device
        .call(
            Operation::AgentSave,
            None,
            json!({"agent": {"name": name, "instructions": format!("{name} does its work.")}}),
        )
        .unwrap();
    saved["agent"]["id"].as_str().unwrap().to_owned()
}

fn network(action: &str, container: &str) {
    let network = env("MUNIMENT_FLEET_E2E_NETWORK").unwrap();
    let mut args = vec!["network", action];
    if action == "connect" {
        args.extend(["--alias", container]);
    }
    args.extend([network.as_str(), container]);
    docker(&args).unwrap();
    log(format!("network {action} {container}"));
}

fn netem(container: &str, args: &[&str]) -> Result<String, String> {
    let image = env("MUNIMENT_FLEET_E2E_NETEM_IMAGE").ok_or("no netem image")?;
    let target = format!("container:{container}");
    let mut command = vec![
        "run",
        "--rm",
        "--network",
        &target,
        "--cap-add",
        "NET_ADMIN",
        &image,
    ];
    command.extend(args);
    docker(&command)
}

fn online(node: &Node, runtime_id: &str) -> Result<bool, String> {
    Ok(node
        .members()?
        .iter()
        .any(|member| member["runtime_id"] == runtime_id && member["online"] == true))
}

fn thread_ran_on(owner: &Node, thread: &str, prompt: Option<&str>) -> Result<(), String> {
    let entries = owner.history(thread)?;
    let reply = format!("reply from {}", owner.name);
    let ran = entries.iter().any(|entry| {
        prompt.is_none_or(|prompt| entry["prompt"] == prompt)
            && entry["text"].as_str().unwrap_or("").contains(&reply)
    });
    ensure(ran, || format!("{} history: {entries:?}", owner.name))
}

#[test]
#[ignore = "needs the Docker fleet that scripts/fleet-e2e.sh starts"]
fn a_docker_fleet_converges_through_partitions_loss_and_a_stopped_owner() {
    if env("MUNIMENT_FLEET_E2E").as_deref() != Some("1") {
        return;
    }
    started();
    let containers: Vec<String> = env("MUNIMENT_FLEET_E2E_CONTAINERS")
        .expect("MUNIMENT_FLEET_E2E_CONTAINERS names three containers")
        .split(',')
        .map(|name| name.trim().to_owned())
        .collect();
    assert_eq!(containers.len(), 3, "three containers");
    std::env::set_var("PI_RESUME_STUB_REPLY", format!(" reply from {HOST_NAME}"));
    let temporary = TemporaryProfile::new("fleet-docker", false);
    let phone = Phone::from_env(&temporary.root);
    let account = phone.as_ref().map(|phone| {
        Arc::new(ProbeAccount {
            api: phone.api.clone(),
            file: phone.directory.join("desktop.json"),
        }) as Arc<dyn AccountService>
    });
    let host = HostRuntime::open(temporary, account);
    host.seed_history(40, 12);
    let h = host.device();
    let nodes: Vec<Node> = containers
        .iter()
        .map(|name| Node::container(name))
        .collect();
    let [c1, c2, c3] = [&nodes[0], &nodes[1], &nodes[2]];
    let all: Vec<&Node> = vec![&h, c1, c2, c3];
    log(format!(
        "host {} on port {}, containers {}",
        h.runtime_id,
        host.port,
        nodes
            .iter()
            .map(|node| format!("{}={}", node.name, node.runtime_id))
            .collect::<Vec<_>>()
            .join(" ")
    ));

    // Links: the host joins c1 as the app links a machine it paired, with
    // its whole history in the first exchanges; c2 and c3 dial the host
    // through the host gateway, c2 dials c1 and c3 dials c2 by their Docker
    // names.
    for (joiner, inviter) in [(&h, c1), (c2, &h), (c3, &h), (c2, c1), (c3, c2)] {
        let invite = inviter
            .call(Operation::SwarmInvite, None, json!({}))
            .unwrap();
        let joined = joiner
            .call(Operation::SwarmJoin, None, json!({"link": invite["link"]}))
            .unwrap();
        assert_eq!(joined["member"]["runtime_id"], inviter.runtime_id.as_str());
        log(format!("{} joined {}", joiner.name, inviter.name));
    }
    let ids: BTreeSet<String> = all.iter().map(|node| node.runtime_id.clone()).collect();
    until("every member lists the four as online full members", || {
        for node in &all {
            let members = node.members()?;
            let full: BTreeSet<String> = members
                .iter()
                .filter(|member| {
                    (member["online"] == true || member["self"] == true)
                        && member["fleet"]["role"] == "full"
                })
                .filter_map(|member| member["runtime_id"].as_str().map(str::to_owned))
                .collect();
            ensure(full == ids, || format!("{}: {members:?}", node.name))?;
        }
        Ok(())
    });

    // 1. An agent made on the host reaches every container, and a thread
    // offloaded to c2 runs there and lists everywhere with c2 as its owner.
    let scout = save_agent(&h, "Scout");
    until("Scout is in every container's directory", || {
        for node in &nodes {
            let agent = node.agent_named("Scout")?;
            ensure(
                agent.as_ref().is_some_and(|agent| {
                    agent["agent_id"] == scout.as_str() && agent["primary_runtime"] == h.runtime_id
                }),
                || format!("{}: {agent:?}", node.name),
            )?;
        }
        Ok(())
    });
    let offload_text = "Summarize the logs on c2.";
    let created = h
        .call(
            Operation::ThreadCreate,
            Some(key()),
            json!({"runtime_id": c2.runtime_id, "text": offload_text, "permission_mode": "bypass"}),
        )
        .unwrap();
    assert_eq!(created["forwarded"], true, "{created}");
    let offloaded = created["thread_id"].as_str().unwrap().to_owned();
    let op_id = created["op_id"].as_str().unwrap().to_owned();
    until("the offload op is applied", || {
        let op = h.op_status(&op_id)?;
        ensure(op["state"] == "applied", || op.to_string())
    });
    let op = h.op_status(&op_id).unwrap();
    assert_eq!(op["owner_runtime"], c2.runtime_id.as_str(), "{op}");
    assert!(op["result"]["run_id"].is_string(), "{op}");
    until("the offloaded run completes on c2", || {
        thread_ran_on(c2, &offloaded, Some(offload_text))
    });
    until("every member lists the offloaded thread as c2's", || {
        for node in &all {
            let summary = node.summary(&offloaded)?;
            let replica = node.runtime_id != c2.runtime_id;
            ensure(
                summary.as_ref().is_some_and(|summary| {
                    summary["owner_runtime"] == c2.runtime_id
                        && (summary["replica"] == true) == replica
                }),
                || format!("{}: {summary:?}", node.name),
            )?;
        }
        Ok(())
    });
    until("the host and c1 read c2's reply", || {
        for node in [&h, c1] {
            let entries = node.history(&offloaded)?;
            ensure(
                entries.iter().any(|entry| {
                    entry["text"]
                        .as_str()
                        .unwrap_or("")
                        .contains(&format!("reply from {}", c2.name))
                }),
                || format!("{}: {entries:?}", node.name),
            )?;
        }
        Ok(())
    });

    // A follow-up from the host runs on c2. A run started here in place of
    // a post is refused in words that name no operation.
    let refused =
        h.with(|client| client.run_submit("Again here.", &[], Some(&offloaded), None, None));
    let refused = format!("{refused:?}");
    assert!(!refused.contains("thread.post"), "{refused}");
    let follow_up = "Follow up on c2.";
    let posted = h
        .call(
            Operation::ThreadPost,
            None,
            json!({"thread_id": offloaded, "text": follow_up}),
        )
        .unwrap();
    assert_eq!(posted["forwarded"], true, "{posted}");
    until("the follow-up runs on c2", || {
        thread_ran_on(c2, &offloaded, Some(follow_up))
    });

    // The app's path to a machine it paired: while a run waits on the host,
    // a new thread on c1 takes a first and a second message, and every
    // member lists the thread as c1's.
    let waiting = h
        .with(|client| {
            client.run_submit(
                &format!("Wait on the host. {HOLD_MARKER}"),
                &[],
                None,
                None,
                Some("bypass"),
            )
        })
        .unwrap();
    let fresh = c1.with(|client| client.create_thread()).unwrap();
    for text in ["First message on c1.", "Second message on c1."] {
        // A refusal records nothing, so the send repeats while the last
        // run's slot frees.
        until(&format!("c1 takes {text:?}"), || {
            c1.with(|client| client.run_submit(text, &[], Some(&fresh), None, Some("bypass")))
                .map(drop)
        });
        until(&format!("{text:?} runs on c1"), || {
            thread_ran_on(c1, &fresh, Some(text))
        });
    }
    h.with(|client| client.run_cancel(&waiting.run_id)).unwrap();
    until("every member lists c1's new thread as c1's", || {
        every(&all, |node| {
            let summary = node.summary(&fresh)?;
            ensure(
                summary
                    .as_ref()
                    .is_some_and(|summary| summary["owner_runtime"] == c1.runtime_id),
                || format!("{}: {summary:?}", node.name),
            )
        })
    });

    // Stop and steer from the host reach a run on c2, which only the fleet
    // links the host to: the host's runtime forwards both to the owner.
    let held = |text: &str| -> (String, String) {
        let created = h
            .call(
                Operation::ThreadCreate,
                Some(key()),
                json!({"runtime_id": c2.runtime_id, "text": format!("{text} {HOLD_MARKER}"), "permission_mode": "bypass"}),
            )
            .unwrap();
        let thread = created["thread_id"].as_str().unwrap().to_owned();
        let op_id = created["op_id"].as_str().unwrap().to_owned();
        let mut run = String::new();
        until("the held run starts on c2", || {
            let op = h.op_status(&op_id)?;
            run = op["result"]["run_id"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            ensure(op["state"] == "applied" && !run.is_empty(), || {
                op.to_string()
            })
        });
        (thread, run)
    };
    let (steered, run) = held("Wait for a steer.");
    until("the host's steer reaches the run on c2", || {
        h.with(|client| client.run_steer(&run, "Steered from the host."))
            .map(drop)
    });
    until("the steered run finishes on c2", || {
        let entries = c2.history(&steered)?;
        ensure(
            entries.iter().any(|entry| {
                // The stub answers a steer with its name and ends the turn.
                entry["runId"] == run.as_str()
                    && entry["phase"] == "complete"
                    && entry["text"].as_str().unwrap_or("").contains("steer")
            }),
            || format!("c2 history: {entries:?}"),
        )
    });
    let (stopped, run) = held("Wait for a stop.");
    until("the host's stop reaches the run on c2", || {
        h.with(|client| client.run_cancel(&run)).map(drop)
    });
    until("the stopped run ends cancelled on c2", || {
        let entries = c2.history(&stopped)?;
        ensure(
            entries
                .iter()
                .any(|entry| entry["runId"] == run.as_str() && entry["phase"] == "cancelled"),
            || format!("c2 history: {entries:?}"),
        )
    });

    // 2. Settings, preferences and memory edits converge.
    c1.call(
        Operation::PermissionSettingsSave,
        None,
        json!({"unattended_timeout_seconds": 7200}),
    )
    .unwrap();
    h.call(
        Operation::PreferencesSet,
        None,
        json!({"values": {"theme": "dark"}}),
    )
    .unwrap();
    write_memory(
        &host,
        &h,
        "fleet-notes.md",
        "# Fleet notes\n\nFrom the host.\n",
    );
    until(
        "permission settings, preferences and memory converge",
        || {
            every(&all, |node| {
                let settings = node.call(Operation::PermissionSettings, None, json!({}))?;
                ensure(settings["unattended_timeout_seconds"] == 7200, || {
                    format!("{} settings: {settings}", node.name)
                })?;
                let theme = node.preference("theme")?;
                ensure(theme == "dark", || format!("{} theme: {theme}", node.name))?;
                let notes = memory_file(&host, node, "fleet-notes.md");
                ensure(
                    notes.as_deref() == Some("# Fleet notes\n\nFrom the host.\n"),
                    || format!("{} notes: {notes:?}", node.name),
                )
            })
        },
    );
    write_memory(
        &host,
        c1,
        "fleet-notes.md",
        "# Fleet notes\n\nEdited on c1.\n",
    );
    until("an edit on c1 reaches every member", || {
        for node in &all {
            let notes = memory_file(&host, node, "fleet-notes.md");
            ensure(
                notes.as_deref() == Some("# Fleet notes\n\nEdited on c1.\n"),
                || format!("{} notes: {notes:?}", node.name),
            )?;
        }
        Ok(())
    });

    // 3. The host shares a model connection and a ChatGPT subscription its
    // router pool serves: every container holds both and offers the
    // subscription's models.
    host.write_auth(&json!({"openai": {"type": "api_key", "key": API_KEY}}));
    host.write_subscription();
    let shared = h
        .call(
            Operation::FleetCredentials,
            None,
            json!({"share_models": true}),
        )
        .unwrap();
    assert_eq!(shared["share_models"], true, "{shared}");
    let models = format!("models:{}", h.runtime_id);
    until(
        "the sealed model connection reaches every container",
        || {
            for node in &nodes {
                let auth = container_auth(node);
                ensure(auth["openai"]["key"] == API_KEY, || {
                    format!("{} auth: {auth}", node.name)
                })?;
                let inventory = node.with(|client| {
                    client.models_request(ModelAction::ProviderInventory, json!({}))
                })?;
                ensure(
                    inventory["providers"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|provider| provider["source"] == "router")
                        && inventory["router_models"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .any(|model| {
                                model["family"] == "openai" && model["accounts"].as_u64() >= Some(1)
                            }),
                    || format!("{} inventory: {inventory}", node.name),
                )?;
                // A default the router names is a model the catalog knows
                // and some plan behind it offers.
                if inventory["default_provider"] == "muniment-router" {
                    let default = inventory["default_model"].as_str().unwrap_or_default();
                    ensure(
                        inventory["router_models"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .any(|model| {
                                model["id"] == default
                                    && model["name"].is_string()
                                    && model["unavailable"].is_null()
                            }),
                        || format!("{} default: {inventory}", node.name),
                    )?;
                }
                // The member writes the files its runs read the shared
                // connections from.
                for file in SHARED_FILES {
                    docker(&["exec", node.container_name(), "test", "-s", file])
                        .map_err(|error| format!("{} lacks {file}: {error}", node.name))?;
                }
                let held = node.credentials()?;
                ensure(
                    held["secrets"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|secret| {
                            secret["secret_id"] == models.as_str()
                                && secret["for_this_runtime"] == true
                        }),
                    || format!("{} secrets: {}", node.name, held["secrets"]),
                )?;
            }
            Ok(())
        },
    );

    // A thread offloaded after the switch runs on the shared connections.
    let shared_text = "Answer with the shared connection on c1.";
    let created = h
        .call(
            Operation::ThreadCreate,
            Some(key()),
            json!({"runtime_id": c1.runtime_id, "text": shared_text, "permission_mode": "bypass"}),
        )
        .unwrap();
    let on_shared = created["thread_id"].as_str().unwrap().to_owned();
    until(
        "the offloaded run on the shared connections completes on c1",
        || thread_ran_on(c1, &on_shared, Some(shared_text)),
    );

    // 4. c3 leaves the network while the others keep editing, and edits
    // the same memory file as the host meanwhile.
    let doomed = save_agent(&h, "Doomed");
    write_memory(&host, &h, "doomed.md", "# Doomed\n\nThis file goes.\n");
    write_memory(
        &host,
        &h,
        "shared.md",
        "# Shared\n\nBefore the partition.\n",
    );
    until("c3 holds the agent and files the partition changes", || {
        ensure(c3.agent_named("Doomed")?.is_some(), || "no Doomed".into())?;
        for name in ["doomed.md", "shared.md"] {
            ensure(memory_file(&host, c3, name).is_some(), || {
                format!("c3 lacks {name}")
            })?;
        }
        Ok(())
    });
    network("disconnect", c3.container_name());
    let partitioned = Instant::now();
    let c3_text = "# Shared\n\nEdited on c3 during the partition.\n";
    let host_text = "# Shared\n\nEdited on the host during the partition.\n";
    write_memory(&host, c3, "shared.md", c3_text);
    // c3's memory pass runs every 5 seconds, so its edit is a record before
    // the host edits the same file.
    std::thread::sleep(Duration::from_secs(8));
    write_memory(&host, &h, "shared.md", host_text);
    let late = save_agent(&h, "Late");
    h.call(
        Operation::AgentDelete,
        None,
        json!({"id": doomed, "remove_memory": false}),
    )
    .unwrap();
    h.call(
        Operation::PreferencesSet,
        None,
        json!({"values": {"theme": "light"}}),
    )
    .unwrap();
    write_memory(
        &host,
        &h,
        "fleet-notes.md",
        "# Fleet notes\n\nPartition edit.\n",
    );
    remove_memory(&host, "doomed.md");
    let partition_state = |node: &Node| -> Result<(), String> {
        let agents = node.agents()?;
        ensure(
            agents
                .iter()
                .any(|agent| agent["agent_id"] == late.as_str()),
            || format!("{} lacks Late", node.name),
        )?;
        ensure(
            agents
                .iter()
                .all(|agent| agent["agent_id"] != doomed.as_str()),
            || format!("{} still lists Doomed", node.name),
        )?;
        let theme = node.preference("theme")?;
        ensure(theme == "light", || format!("{} theme: {theme}", node.name))?;
        let notes = memory_file(&host, node, "fleet-notes.md");
        ensure(
            notes.as_deref() == Some("# Fleet notes\n\nPartition edit.\n"),
            || format!("{} notes: {notes:?}", node.name),
        )?;
        ensure(memory_file(&host, node, "doomed.md").is_none(), || {
            format!("{} still holds doomed.md", node.name)
        })
    };
    until("c1 and c2 take the partition edits", || {
        for node in [c1, c2] {
            partition_state(node)?;
        }
        Ok(())
    });
    std::thread::sleep(Duration::from_secs(6));
    network("connect", c3.container_name());
    log(format!(
        "c3 was partitioned for {:.1}s",
        partitioned.elapsed().as_secs_f64()
    ));
    until("c3 catches up after the partition", || partition_state(c3));
    let mut conflict = None;
    until("every member keeps both shared.md edits", || {
        let mut seen = None;
        for node in &all {
            let names = memory_names(&host, node, "shared")?;
            let copies: Vec<&String> = names
                .iter()
                .filter(|name| name.contains("(from "))
                .collect();
            ensure(names.contains("shared.md") && copies.len() == 1, || {
                format!("{}: {names:?}", node.name)
            })?;
            let copy = copies[0].clone();
            let texts: BTreeSet<String> = ["shared.md", copy.as_str()]
                .iter()
                .filter_map(|name| memory_file(&host, node, name))
                .collect();
            let expected: BTreeSet<String> = [c3_text, host_text].map(str::to_owned).into();
            ensure(texts == expected, || format!("{}: {texts:?}", node.name))?;
            let winner = memory_file(&host, node, "shared.md");
            let current = (copy, winner);
            if seen.as_ref().is_some_and(|seen| seen != &current) {
                return Err(format!("{} differs: {current:?} vs {seen:?}", node.name));
            }
            seen = Some(current);
        }
        conflict = seen;
        Ok(())
    });
    log(format!("conflict copy and winner: {conflict:?}"));
    // Nothing deleted during the partition comes back, on any member.
    std::thread::sleep(Duration::from_secs(20));
    for node in &all {
        partition_state(node).unwrap_or_else(|state| panic!("resurrection: {state}"));
    }
    log("no deleted agent or memory file came back on any member after 20s");

    // 5. Delay and loss on every container; changes still converge.
    for node in &nodes {
        netem(
            node.container_name(),
            &[
                "qdisc", "add", "dev", "eth0", "root", "netem", "delay", "150ms", "50ms", "loss",
                "10%",
            ],
        )
        .unwrap();
    }
    log(format!(
        "netem on every container: {}",
        netem(c1.container_name(), &["qdisc", "show", "dev", "eth0"])
            .unwrap()
            .trim()
    ));
    h.call(
        Operation::PreferencesSet,
        None,
        json!({"values": {"theme": "dark"}}),
    )
    .unwrap();
    let steady = save_agent(&h, "Steady");
    write_memory(
        &host,
        c2,
        "netem.md",
        "# Netem\n\nWritten on c2 under loss.\n",
    );
    c3.call(
        Operation::PermissionSettingsSave,
        None,
        json!({"unattended_timeout_seconds": 3600}),
    )
    .unwrap();
    until("every member converges under delay and loss", || {
        for node in &all {
            ensure(
                node.agent_named("Steady")?
                    .is_some_and(|agent| agent["agent_id"] == steady.as_str()),
                || format!("{} lacks Steady", node.name),
            )?;
            let theme = node.preference("theme")?;
            ensure(theme == "dark", || format!("{} theme: {theme}", node.name))?;
            let netem_file = memory_file(&host, node, "netem.md");
            ensure(
                netem_file.as_deref() == Some("# Netem\n\nWritten on c2 under loss.\n"),
                || format!("{} netem.md: {netem_file:?}", node.name),
            )?;
            let settings = node.call(Operation::PermissionSettings, None, json!({}))?;
            ensure(settings["unattended_timeout_seconds"] == 3600, || {
                format!("{} settings: {settings}", node.name)
            })?;
        }
        Ok(())
    });
    for node in &nodes {
        netem(
            node.container_name(),
            &["qdisc", "del", "dev", "eth0", "root"],
        )
        .unwrap();
    }

    // 6. c2 stops: a post into its thread queues, then applies and runs
    // there once c2 is back.
    docker(&["stop", "--time", "30", c2.container_name()]).unwrap();
    log("c2 stopped");
    until("the host sees c2 offline", || {
        ensure(!online(&h, &c2.runtime_id)?, || "c2 still online".into())
    });
    let queued_text = "Queued while c2 is stopped.";
    let posted = h
        .call(
            Operation::ThreadPost,
            None,
            json!({"thread_id": offloaded, "text": queued_text}),
        )
        .unwrap();
    assert_eq!(posted["forwarded"], true, "{posted}");
    let queued_op = posted["op_id"].as_str().unwrap().to_owned();
    let queued = h.op_status(&queued_op).unwrap();
    assert_eq!(queued["state"], "queued", "{queued}");
    assert_eq!(queued["reason"]["code"], "owner_offline", "{queued}");
    log(format!("queued: {}", queued["reason"]["text"]));
    std::thread::sleep(Duration::from_secs(5));
    assert_eq!(h.op_status(&queued_op).unwrap()["state"], "queued");
    let open_ops = h
        .call(Operation::FleetOps, None, json!({"state": "open"}))
        .unwrap();
    assert!(
        open_ops["ops"]
            .as_array()
            .unwrap()
            .iter()
            .any(|op| op["op_id"] == queued_op.as_str()),
        "{open_ops}"
    );
    docker(&["start", c2.container_name()]).unwrap();
    log("c2 started");
    until("the queued post applies once c2 is back", || {
        let op = h.op_status(&queued_op)?;
        ensure(op["state"] == "applied", || op.to_string())
    });
    until("the queued post ran on c2", || {
        thread_ran_on(c2, &offloaded, Some(queued_text))
    });

    // 7. The phone reads the fleet through the host and posts into a
    // container's thread.
    if let Some(phone) = &phone {
        phone_step(phone, &all, c2, &offloaded, &scout);
    } else {
        log("phone step skipped: no cloud API configured");
    }

    // 3, continued. The host removes c3 from the fleet: it reseals to the
    // rest, c3 drops the key, and the log lists what c3 held.
    let version_before = h.credentials().unwrap()["secrets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|secret| secret["secret_id"] == models.as_str())
        .map(|secret| secret["version"].as_u64().unwrap())
        .unwrap();
    let left = h
        .call(
            Operation::SwarmLeave,
            None,
            json!({"runtime_id": c3.runtime_id}),
        )
        .unwrap();
    assert_eq!(left["removed"], true, "{left}");
    until("the host reseals its models without c3", || {
        let audit = h.audit()?;
        let last = audit
            .iter()
            .filter(|entry| {
                entry["kind"] == "sealed" && entry["record"]["secret_id"] == models.as_str()
            })
            .max_by_key(|entry| entry["record"]["version"].as_u64().unwrap_or(0))
            .cloned()
            .unwrap_or(Value::Null);
        let recipients: BTreeSet<String> = last["record"]["recipients"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|id| id.as_str().map(str::to_owned))
            .collect();
        ensure(
            last["record"]["version"].as_u64().unwrap_or(0) > version_before
                && !recipients.contains(&c3.runtime_id)
                && recipients.contains(&c1.runtime_id)
                && recipients.contains(&c2.runtime_id),
            || format!("last seal: {last}"),
        )
    });
    until("c3 drops the host's key and the others keep it", || {
        let auth = container_auth(c3);
        ensure(auth.get("openai").is_none(), || format!("c3 auth: {auth}"))?;
        for node in [c1, c2] {
            let auth = container_auth(node);
            ensure(auth["openai"]["key"] == API_KEY, || {
                format!("{} auth: {auth}", node.name)
            })?;
        }
        Ok(())
    });
    until("the revocation lists the keys c3 held", || {
        let audit = h.audit()?;
        let revoked = audit
            .iter()
            .find(|entry| entry["kind"] == "revoked" && entry["runtime_id"] == c3.runtime_id)
            .cloned()
            .unwrap_or(Value::Null);
        ensure(
            revoked["record"]["keys"]
                .as_array()
                .is_some_and(|keys| keys.contains(&json!("pi:openai"))),
            || format!("revoked: {revoked}"),
        )?;
        log(format!("revoked entry: {}", revoked["record"]));
        Ok(())
    });
    let version_after = h.credentials().unwrap()["secrets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|secret| secret["secret_id"] == models.as_str())
        .map(|secret| secret["version"].as_u64().unwrap())
        .unwrap();
    log(format!(
        "models secret version {version_before} -> {version_after}"
    ));
    log("every scenario passed");
}

/// Runs the phone probe against the local cloud: it pairs the host's
/// account with a phone, reads `fleet.snapshot` and posts into `thread`,
/// which `owner` runs.
fn phone_step(phone: &Phone, all: &[&Node], owner: &Node, thread: &str, agent: &str) {
    let text = format!("Phone post {}.", uuid::Uuid::new_v4().simple());
    std::fs::write(
        phone.directory.join("fleet.json"),
        json!({"thread_id": thread, "owner_runtime": owner.runtime_id, "text": text}).to_string(),
    )
    .unwrap();
    let mut probe = Command::new("node")
        .arg(&phone.probe)
        .env("API", &phone.api)
        .env("EMAIL", &phone.email)
        .env("PASSWORD", &phone.password)
        .env("DIR", &phone.directory)
        .spawn()
        .expect("node runs the phone probe");
    let begin = Instant::now();
    let status = loop {
        if let Some(status) = probe.try_wait().unwrap() {
            break status;
        }
        if begin.elapsed() > Duration::from_secs(600) {
            let _ = probe.kill();
            panic!("the phone probe did not finish within 600s");
        }
        std::thread::sleep(Duration::from_millis(500));
    };
    assert!(status.success(), "the phone probe failed: {status}");
    let read = |name: &str| -> Value {
        serde_json::from_slice(&std::fs::read(phone.directory.join(name)).unwrap()).unwrap()
    };
    let snapshot = read("snapshot.json");
    let members: BTreeSet<String> = snapshot["members"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|member| member["runtime_id"].as_str().map(str::to_owned))
        .collect();
    let ids: BTreeSet<String> = all.iter().map(|node| node.runtime_id.clone()).collect();
    assert_eq!(members, ids, "{snapshot}");
    let agents = snapshot["agents"].as_array().unwrap();
    assert!(
        agents.iter().any(|entry| entry["agent_id"] == agent),
        "{snapshot}"
    );
    let threads = snapshot["threads"].as_array().unwrap();
    assert!(
        threads.iter().any(|entry| {
            (entry["threadId"] == thread || entry["thread_id"] == thread)
                && entry["owner_runtime"] == owner.runtime_id
        }),
        "{snapshot}"
    );
    let done = read("done.json");
    assert_eq!(done["op_state"], "applied", "{done}");
    assert_eq!(done["owner_runtime"], owner.runtime_id.as_str(), "{done}");
    log(format!(
        "phone: snapshot lists {} members, {} agents, {} threads; its post {} was {}",
        members.len(),
        agents.len(),
        threads.len(),
        done["op_id"],
        done["op_state"]
    ));
    until("the phone's post ran on its owner", || {
        thread_ran_on(owner, thread, Some(&text))
    });
}
