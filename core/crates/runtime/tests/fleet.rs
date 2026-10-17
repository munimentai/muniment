#![cfg(unix)]
//! Runtimes linked into a fleet over loopback: sealed model connections, a
//! forged member on a raw link, a member that leaves, and a sign-in whose
//! refresher dies mid-refresh and moves with `fleet.hold_sign_in`.

use std::collections::{BTreeSet, HashMap};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::SocketAddr;
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use muniment_core::attach::{
    pair_with_link, InstallKind, Operation, PairingLinkRequest, RemoteAccess, RemoteAccessOptions,
    RemoteClientOptions, RemoteListener, RemoteListenerConfig, RemoteListenerError,
    SCOPE_SWARM_PEER,
};
use muniment_core::model_router::config::{self, Account, Credential};
use muniment_core::serde_json::{json, Value};
use muniment_core::swarm::Swarm;
use muniment_runtime_core::host::FileCredentialStore;
use muniment_runtime_core::{RuntimeAttachState, RuntimeHost};

mod common;
use common::TemporaryProfile;

/// The tests share the process-wide refresh endpoint, so they run one at a
/// time.
static ENVIRONMENT: Mutex<()> = Mutex::new(());

struct Member {
    name: &'static str,
    temporary: TemporaryProfile,
    state: Option<Arc<RuntimeAttachState>>,
    access: Option<RemoteAccess>,
    address: Option<SocketAddr>,
    stop: Option<mpsc::Sender<()>>,
    server: Option<std::thread::JoinHandle<Result<(), RemoteListenerError>>>,
}

impl Member {
    fn new(name: &'static str) -> Self {
        let temporary = TemporaryProfile::new(&format!("fleet-{name}"), false);
        let profile = temporary.profile.clone();
        muniment_core::home::confirm_home(&profile, &temporary.root.join("home")).unwrap();
        std::fs::write(
            profile.join(muniment_core::local_mode::LOCAL_MODE_MARKER),
            "1",
        )
        .unwrap();
        let mut member = Self {
            name,
            temporary,
            state: None,
            access: None,
            address: None,
            stop: None,
            server: None,
        };
        member.open();
        member
    }

    fn open(&mut self) {
        let profile = &self.temporary.profile;
        // Each member keeps its own fleet key, as separate machines do.
        let host = RuntimeHost {
            credentials: Arc::new(FileCredentialStore::new(
                self.temporary.root.join("credentials"),
            )),
            ..RuntimeHost::default()
        };
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

    fn offline(&mut self) {
        drop(self.stop.take());
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
    }

    /// Stops the runtime as a crash does: its listener and swarm end, and
    /// nothing it had in flight finishes on its behalf.
    fn kill(&mut self) {
        self.offline();
        if let Some(state) = self.state.take() {
            state.swarm().stop();
        }
        self.access = None;
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

    fn agent(&self) -> std::path::PathBuf {
        muniment_core::state_root::agent_directory(&self.temporary.profile)
    }

    fn runtime_id(&self) -> String {
        self.access().runtime_info().runtime_id
    }

    fn op(&self, operation: Operation, body: Value) -> Result<Value, String> {
        self.swarm().operation(operation, body, None)
    }

    fn join(&self, other: &Member) {
        let invite = other.op(Operation::SwarmInvite, json!({})).unwrap();
        assert_eq!(invite["role"], "full");
        let joined = self
            .op(Operation::SwarmJoin, json!({"link": invite["link"]}))
            .unwrap();
        assert_eq!(joined["member"]["runtime_id"], other.runtime_id());
    }

    fn trusts(&self, other: &Member) -> bool {
        self.op(Operation::SwarmMembers, json!({})).unwrap()["members"]
            .as_array()
            .unwrap()
            .iter()
            .any(|member| {
                member["runtime_id"] == other.runtime_id() && member["fleet"]["trusted"] == true
            })
    }

    fn share(&self) {
        let answer = self
            .op(Operation::FleetCredentials, json!({"share_models": true}))
            .unwrap();
        assert_eq!(answer["share_models"], true);
    }

    fn auth(&self) -> muniment_core::serde_json::Map<String, Value> {
        muniment_core::models::pi_models::read_json(&self.agent().join("auth.json")).unwrap()
    }

    fn write_auth(&self, auth: Value) {
        std::fs::create_dir_all(self.agent()).unwrap();
        muniment_core::models::pi_models::write_json(
            &self.agent().join("auth.json"),
            auth.as_object().unwrap(),
        )
        .unwrap();
    }

    fn accounts(&self) -> Vec<Account> {
        config::load(&self.agent()).unwrap().accounts
    }
}

impl Drop for Member {
    fn drop(&mut self) {
        if let Some(state) = &self.state {
            state.swarm().stop();
        }
        self.offline();
    }
}

fn wait(mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn environment() -> std::sync::MutexGuard<'static, ()> {
    ENVIRONMENT
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

fn api_key(key: &str) -> Value {
    json!({"type": "api_key", "key": key})
}

fn peer_options() -> RemoteClientOptions {
    RemoteClientOptions {
        kind: "swarm-peer".into(),
        version: "1.0.0".into(),
        device_name: Some("Intruder".into()),
        scopes: Some(BTreeSet::from([SCOPE_SWARM_PEER.to_owned()])),
        io_timeout: Duration::from_secs(10),
    }
}

/// A device paired to `member` with only `swarm.peer`, as a stolen or
/// hand-made link gives one: it skips `swarm.invite`, so no member vouches
/// for what it claims.
fn raw_peer(member: &Member) -> muniment_core::attach::DesktopClient {
    let link = member
        .access()
        .create_pairing_link(PairingLinkRequest {
            scopes: Some(BTreeSet::from([SCOPE_SWARM_PEER.to_owned()])),
            endpoints: None,
        })
        .unwrap()
        .link;
    pair_with_link(&link, &peer_options()).unwrap().0
}

fn exchange(client: &mut muniment_core::attach::DesktopClient, body: Value) -> Value {
    client
        .request(Operation::SwarmExchange, None, body)
        .unwrap()
        .body
}

#[test]
fn a_forged_member_on_a_raw_link_gets_no_ciphertext() {
    let _environment = environment();
    let a = Member::new("forge-a");
    let b = Member::new("forge-b");
    let c = Member::new("forge-c");
    b.join(&a);
    c.join(&a);
    assert!(wait(|| a.trusts(&b) && a.trusts(&c) && b.trusts(&c)));
    a.write_auth(json!({"openai": api_key("sk-from-a")}));
    a.share();
    a.swarm().fleet_pass().unwrap();
    assert!(wait(
        || b.auth().contains_key("openai") && c.auth().contains_key("openai")
    ));

    // The fourth member speaks the exchange itself under a fresh id. It
    // asks for every sealed secret and gets none.
    let mut intruder = raw_peer(&a);
    let now = "2026-10-17T00:00:00Z";
    let own = json!({
        "runtime_id": uuid::Uuid::new_v4().to_string(),
        "runtime_name": "Intruder",
        "heartbeat_at": now,
        "updated_at": now,
        "capabilities": ["store/1", "fleet/1"],
    });
    let answer = exchange(
        &mut intruder,
        json!({"from": own, "sealed_have": {}, "roster_digest": "none"}),
    );
    assert!(answer.get("sealed").is_none(), "{answer}");
    // It learns C's card from the member records and replays C's record as
    // its own from a second raw device.
    let claimed = answer["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|member| member["runtime_id"] == c.runtime_id())
        .cloned()
        .unwrap();
    assert!(claimed["fleet"].is_object());
    let mut forged = claimed.clone();
    forged["updated_at"] = json!("2999-01-01T00:00:00Z");
    let mut impostor = raw_peer(&a);
    // C's own link exchanges every 100 ms meanwhile, on another thread.
    for _ in 0..40 {
        let answer = exchange(
            &mut impostor,
            json!({"from": forged, "members": [forged], "sealed_have": {}, "roster_digest": "none"}),
        );
        assert_eq!(answer["from"]["runtime_id"], a.runtime_id());
        assert!(answer.get("sealed").is_none(), "{answer}");
        std::thread::sleep(Duration::from_millis(25));
    }
    // A's next seal still goes to B and C alone, under their own keys.
    a.write_auth(json!({"openai": api_key("sk-rotated")}));
    a.swarm().fleet_pass().unwrap();
    assert!(wait(|| c.auth()["openai"]["key"] == "sk-rotated"));
    let answer = exchange(
        &mut impostor,
        json!({"from": forged, "sealed_have": {}, "roster_digest": "none"}),
    );
    assert!(answer.get("sealed").is_none(), "{answer}");
}

#[test]
fn model_connections_apply_on_members_and_drop_when_a_member_leaves() {
    let _environment = environment();
    let a = Member::new("leave-a");
    let b = Member::new("leave-b");
    let c = Member::new("leave-c");
    b.join(&a);
    c.join(&a);
    c.join(&b);
    assert!(wait(|| a.trusts(&c) && b.trusts(&c) && c.trusts(&a)));
    a.write_auth(json!({"openai": api_key("sk-from-a")}));
    c.write_auth(json!({"xai": api_key("sk-from-c")}));
    a.share();
    c.share();
    a.swarm().fleet_pass().unwrap();
    c.swarm().fleet_pass().unwrap();
    assert!(wait(|| {
        b.auth().contains_key("openai")
            && b.auth().contains_key("xai")
            && c.auth().contains_key("openai")
            && a.auth().contains_key("xai")
    }));
    let record = std::fs::read_to_string(b.agent().join("muniment-shared.json")).unwrap();
    assert!(record.contains(&format!("swarm:{}", a.runtime_id())));

    // A ends C's membership: every member drops C's connections, and C
    // drops what the others shared once it hears of its removal.
    let left = a
        .op(Operation::SwarmLeave, json!({"runtime_id": c.runtime_id()}))
        .unwrap();
    assert_eq!(left["removed"], true);
    assert!(wait(|| !a.auth().contains_key("xai")), "A kept C's key");
    assert!(wait(|| !b.auth().contains_key("xai")), "B kept C's key");
    assert!(wait(|| !c.auth().contains_key("openai")), "C kept A's key");
    assert_eq!(c.auth()["xai"]["key"], "sk-from-c");
    assert_eq!(b.auth()["openai"]["key"], "sk-from-a");
    // A lists what C held, for rotation at the provider.
    let log = a.op(Operation::FleetCredentials, json!({})).unwrap()["audit"].clone();
    let revoked = log
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["kind"] == "revoked")
        .cloned()
        .unwrap();
    assert_eq!(revoked["runtime_id"], c.runtime_id());
    assert!(revoked["record"]["keys"]
        .as_array()
        .unwrap()
        .contains(&json!("pi:openai")));
}

/// A token endpoint that rotates refresh tokens and refuses a reused one,
/// as the rotating providers do. It can hold requests to stand in for a
/// refresh that never completes.
#[derive(Default)]
struct Provider {
    state: Mutex<ProviderState>,
    changed: Condvar,
}

#[derive(Default)]
struct ProviderState {
    current: String,
    issued: u32,
    /// How many times each refresh token was spent.
    spent: HashMap<String, u32>,
    reused: u32,
    holding: bool,
    held: u32,
    drop_held: bool,
}

impl Provider {
    fn start(first: &str) -> (Arc<Self>, String) {
        let provider = Arc::new(Self::default());
        provider.state.lock().unwrap().current = first.into();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/oauth/token", listener.local_addr().unwrap());
        let serving = Arc::clone(&provider);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let provider = Arc::clone(&serving);
                std::thread::spawn(move || provider.answer(stream));
            }
        });
        (provider, url)
    }

    fn answer(&self, mut stream: std::net::TcpStream) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let (mut line, mut length) = (String::new(), 0);
        reader.read_line(&mut line).unwrap();
        loop {
            line.clear();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse::<usize>().unwrap();
            }
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        let body = String::from_utf8(body).unwrap();
        let token = body
            .split('&')
            .find_map(|pair| pair.strip_prefix("refresh_token="))
            .unwrap_or_default()
            .to_owned();
        let mut state = self.state.lock().unwrap();
        if state.holding {
            state.held += 1;
            self.changed.notify_all();
            while state.holding {
                state = self.changed.wait(state).unwrap();
            }
            if state.drop_held {
                // The caller died before the request reached the provider.
                return;
            }
        }
        *state.spent.entry(token.clone()).or_default() += 1;
        let (status, answer) = if token == state.current {
            state.issued += 1;
            state.current = format!("r{}", state.issued);
            (
                "200 OK",
                json!({
                    "access_token": format!("a{}", state.issued),
                    "refresh_token": state.current,
                    "expires_in": 20 * 60,
                }),
            )
        } else {
            state.reused += 1;
            ("400 Bad Request", json!({"error": "invalid_grant"}))
        };
        drop(state);
        let text = answer.to_string();
        let _ = write!(
            stream,
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
            text.len()
        );
    }

    fn hold(&self) {
        self.state.lock().unwrap().holding = true;
    }

    fn held(&self) -> u32 {
        self.state.lock().unwrap().held
    }

    /// Lets held requests go, dropping each one unanswered and unspent.
    fn drop_held(&self) {
        let mut state = self.state.lock().unwrap();
        state.holding = false;
        state.drop_held = true;
        self.changed.notify_all();
    }

    fn snapshot(&self) -> (String, u32, HashMap<String, u32>) {
        let state = self.state.lock().unwrap();
        (state.current.clone(), state.reused, state.spent.clone())
    }
}

fn subscription(access: &str, refresh: &str, expires_ms: i64) -> Account {
    Account {
        id: "work".into(),
        family: "openai".into(),
        label: "Work".into(),
        credential: Credential::Subscription {
            provider: "openai-codex".into(),
            access: access.into(),
            refresh: Some(refresh.into()),
            expires_ms: Some(expires_ms),
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
    }
}

fn refresh_of(account: &Account) -> Option<String> {
    match &account.credential {
        Credential::Subscription { refresh, .. } => refresh.clone(),
        Credential::ApiKey { .. } => None,
    }
}

/// Clears the refresh endpoint when a test ends, however it ends.
struct Endpoint;

impl Drop for Endpoint {
    fn drop(&mut self) {
        muniment_core::model_router::native_auth::set_refresh_endpoint_for_tests(None);
    }
}

#[test]
fn a_holder_killed_mid_refresh_never_double_spends_and_hold_sign_in_recovers() {
    let _environment = environment();
    let (provider, url) = Provider::start("r0");
    muniment_core::model_router::native_auth::set_refresh_endpoint_for_tests(Some(url));
    let _endpoint = Endpoint;
    let mut holder = Member::new("race-holder");
    let b = Member::new("race-b");
    let c = Member::new("race-c");
    b.join(&holder);
    c.join(&holder);
    assert!(wait(|| holder.trusts(&b)
        && holder.trusts(&c)
        && b.trusts(&c)));
    let holder_id = holder.runtime_id();

    // The holder signed in; its access token has ten minutes left, inside
    // the lease margin, so the next seal refreshes it first.
    let expires = muniment_core::chrono::Utc::now().timestamp_millis() + 10 * 60_000;
    let mut settings = config::load(&holder.agent()).unwrap();
    settings.accounts.push(subscription("a0", "r0", expires));
    config::save(&holder.agent(), &settings).unwrap();

    // The holder dies while its refresh waits at the provider.
    provider.hold();
    holder.share();
    let swarm = holder.swarm().clone();
    let pass = std::thread::spawn(move || swarm.fleet_pass());
    assert!(wait(|| provider.held() > 0), "the holder never refreshed");
    holder.kill();
    provider.drop_held();
    let _ = pass.join().unwrap();

    // The members hold leases; no member but the holder ever had r0.
    holder.open();
    assert!(
        wait(|| {
            [&b, &c].iter().all(|member| {
                member
                    .accounts()
                    .iter()
                    .any(|account| account.id.ends_with(":work"))
            })
        }),
        "the members never got a lease"
    );
    for member in [&b, &c] {
        assert!(member
            .accounts()
            .iter()
            .all(|account| refresh_of(account).is_none()));
    }

    // B holds the sign-in from now on.
    let held = b
        .op(
            Operation::FleetHoldSignIn,
            json!({"account": "router:work"}),
        )
        .unwrap();
    assert_eq!(held["status"], "requested");
    assert_eq!(held["holder"], holder_id);
    assert!(
        wait(|| b
            .accounts()
            .iter()
            .any(|account| account.id == "work" && refresh_of(account).is_some())),
        "B never received the sign-in"
    );
    // Every member keeps sealing while B refreshes on each pass.
    for _ in 0..3 {
        holder.swarm().fleet_pass().unwrap();
        b.swarm().fleet_pass().unwrap();
        c.swarm().fleet_pass().unwrap();
    }
    let (current, reused, spent) = provider.snapshot();
    assert_eq!(reused, 0, "a refresh token was spent twice: {spent:?}");
    assert!(spent.values().all(|count| *count == 1), "{spent:?}");
    assert!(spent.len() >= 2, "B never refreshed: {spent:?}");
    let own = b
        .accounts()
        .into_iter()
        .find(|account| account.id == "work")
        .unwrap();
    assert_eq!(refresh_of(&own).as_deref(), Some(current.as_str()));
    // The old holder and C keep B's lease, without a refresh token.
    let b_id = b.runtime_id();
    for member in [&holder, &c] {
        assert!(
            wait(|| {
                let accounts = member.accounts();
                accounts.len() == 1
                    && accounts[0].id.contains(&b_id)
                    && refresh_of(&accounts[0]).is_none()
            }),
            "{} holds {:?}",
            member.name,
            member.accounts()
        );
    }
}
