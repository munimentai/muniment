//! Phone pairing and the session relay against a running cloud API.
//!
//! The desktop side is the desktop's own pairing code and the core relay
//! publisher with a scripted runtime. A headless phone in this file signs in
//! as the mobile client, scans the rendered QR code, pairs, opens the sealed
//! events, sends sealed commands, and revokes the pair.
//!
//! The test is ignored. Run it against a local API with a seeded account:
//!
//! ```sh
//! MUNIMENT_RELAY_LIVE_API=http://127.0.0.1:58787 \
//! MUNIMENT_RELAY_LIVE_EMAIL=owner@example.test \
//! MUNIMENT_RELAY_LIVE_PASSWORD=... \
//! cargo test --manifest-path src-tauri/integration/Cargo.toml --test relay_live -- --ignored --nocapture
//! ```
//!
//! The API must register the `muniment-desktop` native client with the
//! redirect `http://127.0.0.1:49152/callback` through
//! `NATIVE_CLIENT_REGISTRATIONS`, and it needs Postgres and Redis.

use std::path::PathBuf;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ed25519_dalek::{Signer, SigningKey};
use muniment_core::active_run::ChatDelivery;
use muniment_core::chat_view::ChatPendingPermission;
use muniment_core::journal::reducer::PermissionRequest;
use muniment_core::permission_gate::ChatPermissionAnswer;
use muniment_core::relay_encryption::{
    agreement_proof, canonical_json, derive_relay_keys, open_event, proof_transcript, seal_command,
    timestamp, AgreementKeyPair, AnswerDecision, CommandContent, CommandHeader, CommandKind,
    DesktopAgreement, EventContent, EventHeader, GateDecision, PairingSecret, Phase, RelayKeys,
    SealedEvent, StopReason, CONTRACT_VERSION,
};
use muniment_core::relay_publisher::{
    relay_state_path, RelayAccount, RelayConnection, RelayPublisher, RelayRuntime,
};
use muniment_core::run_events::{ChatEvent, ChatEventSubscription};
use muniment_desktop_integration::auth::{
    create_pairing_challenge, read_pairing, stored_relay_keys, DesktopIdentity,
    NativeCredentialBackend, PendingAgreement, RemoteControlState, UreqPairingTransport,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tungstenite::Message;

const RELAY_PROTOCOL: &str = "muniment.session-relay/1";

struct Live {
    api: String,
    email: String,
    password: String,
}

fn live() -> Option<Live> {
    Some(Live {
        api: std::env::var("MUNIMENT_RELAY_LIVE_API").ok()?,
        email: std::env::var("MUNIMENT_RELAY_LIVE_EMAIL").ok()?,
        password: std::env::var("MUNIMENT_RELAY_LIVE_PASSWORD").ok()?,
    })
}

fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).unwrap();
    bytes
}

fn now() -> String {
    timestamp(chrono::Utc::now())
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(Duration::from_secs(20))
        .build()
}

/// Sends a JSON request and answers the status and body, error statuses included.
fn send_json(request: ureq::Request, body: Option<&Value>) -> (u16, Value) {
    let result = match body {
        Some(body) => request
            .set("Content-Type", "application/json")
            .send_string(&body.to_string()),
        None => request.call(),
    };
    let response = match result {
        Ok(response) => response,
        Err(ureq::Error::Status(_, response)) => response,
        Err(error) => panic!("the API did not answer: {error}"),
    };
    let status = response.status();
    let text = response.into_string().unwrap();
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

/// A native installation signed in to the account.
struct Installation {
    device_id: String,
    public_key: String,
    token: String,
}

/// Adds the installation's signed device proof to a native auth body.
fn signed(route: &str, body: Value, challenge: &str, key: &SigningKey) -> Value {
    let issued_at = now();
    let jti = URL_SAFE_NO_PAD.encode(random::<16>());
    let digest: String = Sha256::digest(canonical_json(&body).as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let transcript = [
        "MUNIMENT-NATIVE-V1",
        "POST",
        route,
        &digest,
        challenge,
        &issued_at,
        &jti,
    ]
    .join("\n");
    let signature = URL_SAFE_NO_PAD.encode(key.sign(transcript.as_bytes()).to_bytes());
    let mut body = body;
    body["device_proof"] =
        json!({"challenge": challenge, "issued_at": issued_at, "jti": jti, "signature": signature});
    body
}

fn sign_in(live: &Live, desktop: bool) -> Installation {
    let (client_id, role, platform, redirect) = if desktop {
        (
            "muniment-desktop",
            "desktop",
            "desktop",
            "http://127.0.0.1:49152/callback",
        )
    } else {
        (
            "muniment-mobile",
            "mobile",
            "ios",
            "muniment://oauth/callback",
        )
    };
    let key = SigningKey::from_bytes(&random::<32>());
    let public_key = URL_SAFE_NO_PAD.encode(key.verifying_key().as_bytes());
    let (status, registered) = send_json(
        agent().post(&format!("{}/v1/auth/native/devices", live.api)),
        Some(
            &json!({"client_id": client_id, "client_role": role, "platform": platform,
            "installation_public_key": public_key}),
        ),
    );
    assert!(status < 300, "registration: {registered}");
    let device_id = registered["device_id"].as_str().unwrap().to_owned();
    let verifier = URL_SAFE_NO_PAD.encode(random::<32>());
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = URL_SAFE_NO_PAD.encode(random::<32>());
    let body = json!({
        "client_id": client_id, "client_role": role, "redirect_uri": redirect,
        "response_type": "code", "code_challenge": challenge, "code_challenge_method": "S256",
        "state": state, "device_id": device_id,
        "registration_token": registered["registration_token"],
    });
    let (status, authorized) = send_json(
        agent().post(&format!("{}/v1/auth/native/authorize", live.api)),
        Some(&signed(
            "/v1/auth/native/authorize",
            body,
            registered["device_challenge"].as_str().unwrap(),
            &key,
        )),
    );
    assert!(status < 300, "authorize: {authorized}");
    let page = url::Url::parse(authorized["authorization_url"].as_str().unwrap()).unwrap();
    let page = format!("{}{}", live.api, page.path());
    let login = agent()
        .post(&page)
        .send_form(&[
            ("action", "login"),
            ("email", &live.email),
            ("password", &live.password),
        ])
        .unwrap();
    assert_eq!(login.status(), 303, "the account signs in");
    let cookie = login
        .all("set-cookie")
        .iter()
        .map(|value| value.split(';').next().unwrap())
        .collect::<Vec<_>>()
        .join("; ");
    let approval = agent()
        .post(&page)
        .set("Cookie", &cookie)
        .send_form(&[("action", "approve")])
        .unwrap();
    assert_eq!(approval.status(), 302, "the account approves the client");
    let location = url::Url::parse(approval.header("location").unwrap()).unwrap();
    let code = location
        .query_pairs()
        .find(|(name, _)| name == "code")
        .unwrap()
        .1
        .into_owned();
    let body = json!({
        "grant_type": "authorization_code", "code": code, "redirect_uri": redirect,
        "client_id": client_id, "code_verifier": verifier, "device_id": device_id,
    });
    let (status, token) = send_json(
        agent().post(&format!("{}/v1/auth/native/token", live.api)),
        Some(&signed(
            "/v1/auth/native/token",
            body,
            authorized["device_challenge"].as_str().unwrap(),
            &key,
        )),
    );
    assert_eq!(status, 200, "token: {}", token["error"]);
    Installation {
        device_id,
        public_key,
        token: token["access_token"].as_str().unwrap().to_owned(),
    }
}

/// Reads the QR code the desktop rendered, as a phone camera would.
fn scan(svg: &str) -> Value {
    let tree = resvg::usvg::Tree::from_str(svg, &Default::default()).unwrap();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(400, 400).unwrap();
    let scale = 400.0 / tree.size().width();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let gray: Vec<u8> = pixmap.pixels().iter().map(|pixel| pixel.red()).collect();
    let mut decoder = quircs::Quirc::default();
    let code = decoder.identify(400, 400, &gray).next().unwrap().unwrap();
    serde_json::from_slice(&code.decode().unwrap().payload).unwrap()
}

/// Keychain entries in memory.
#[derive(Default)]
struct MemoryKeys(Mutex<std::collections::HashMap<String, String>>);

impl NativeCredentialBackend for MemoryKeys {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        self.0.lock().unwrap().insert(key.into(), value.into());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

/// The desktop account as the runtime host answers it: stored keys, and a
/// pair status read before each connection.
struct DesktopAccount {
    api: String,
    token: String,
    store: PathBuf,
    keys: Arc<MemoryKeys>,
    pending: Arc<Mutex<Option<PendingAgreement>>>,
}

impl RelayAccount for DesktopAccount {
    fn keys(&self) -> Option<RelayKeys> {
        stored_relay_keys(&self.store, self.keys.as_ref())
    }

    fn connection(&self, pair_id: &str) -> Option<RelayConnection> {
        let status = read_pairing(
            &UreqPairingTransport::new(Duration::from_secs(20)),
            &self.api,
            &self.token,
            &self.store,
            &self.pending,
            self.keys.as_ref(),
            chrono::Utc::now(),
        )
        .ok()?;
        (status.pair?.pair_id.to_string() == pair_id).then(|| RelayConnection {
            api_base: self.api.clone(),
            access_token: self.token.clone(),
        })
    }
}

/// A runtime whose runs the test scripts. It records every operation and
/// refuses a message that asks to be refused.
#[derive(Default)]
struct ScriptedRuntime {
    events: Mutex<Option<SyncSender<ChatEvent>>>,
    calls: Mutex<Vec<String>>,
}

impl RelayRuntime for ScriptedRuntime {
    fn subscribe(&self) -> Option<ChatEventSubscription> {
        let (sender, receiver) = sync_channel(64);
        *self.events.lock().unwrap() = Some(sender);
        Some(ChatEventSubscription::detached(receiver))
    }

    fn stop(&self, run_id: &str) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("stop {run_id}"));
        Ok(())
    }

    fn queue(&self, run_id: &str, delivery: ChatDelivery, text: &str) -> Result<(), String> {
        if text.contains("refuse") {
            return Err("That reply is no longer active.".into());
        }
        self.calls
            .lock()
            .unwrap()
            .push(format!("{delivery:?} {run_id} {text}"));
        Ok(())
    }

    fn answer(
        &self,
        run_id: &str,
        gate_id: &str,
        answer: ChatPermissionAnswer,
    ) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("answer {run_id} {gate_id} {}", answer.decision()));
        Ok(())
    }
}

impl ScriptedRuntime {
    fn emit(&self, event: ChatEvent) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(sender) = self.events.lock().unwrap().as_ref() {
                if sender.send(event.clone()).is_ok() {
                    return;
                }
            }
            assert!(Instant::now() < deadline, "the publisher never subscribed");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

fn run_event(run_id: &str, phase: &str, text: &str) -> ChatEvent {
    ChatEvent {
        run_id: run_id.into(),
        thread_id: Some(format!("thread-{run_id}")),
        phase: phase.into(),
        text: text.into(),
        prompt_accepted: true,
        turn_started: true,
        routing_stage: None,
        prompt_storage_notice: None,
        resume_notice: None,
        failure_reason: None,
        receipt: None,
        tool_activity: Vec::new(),
        attachments: Vec::new(),
        recalls: Vec::new(),
        applied_diffs: Vec::new(),
        pending_permission: None,
        delta: None,
        seq: None,
    }
}

/// The phone's relay connection.
struct Phone {
    socket: tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
    ready: Value,
    keys: RelayKeys,
    counter: u64,
    /// Frames read while waiting for another, in arrival order.
    held: std::collections::VecDeque<Value>,
}

impl Phone {
    fn connect(live: &Live, token: &str, keys: RelayKeys) -> Self {
        use tungstenite::client::IntoClientRequest;
        let url = format!("{}/v1/events", live.api.replacen("http", "ws", 1));
        let mut request = url.into_client_request().unwrap();
        request
            .headers_mut()
            .insert("Authorization", format!("Bearer {token}").parse().unwrap());
        let (mut socket, _) = tungstenite::connect(request).unwrap();
        if let tungstenite::stream::MaybeTlsStream::Plain(stream) = socket.get_mut() {
            stream
                .set_read_timeout(Some(Duration::from_secs(20)))
                .unwrap();
        }
        let mut phone = Self {
            socket,
            ready: Value::Null,
            keys,
            counter: 0,
            held: std::collections::VecDeque::new(),
        };
        phone.ready = phone.next(|frame| frame["type"] == "connection.ready")["data"].clone();
        assert_eq!(phone.ready["client_role"], "mobile");
        phone
    }

    fn next(&mut self, matches: impl Fn(&Value) -> bool) -> Value {
        if let Some(index) = self.held.iter().position(&matches) {
            return self.held.remove(index).unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            let text = match self
                .socket
                .read()
                .expect("the relay keeps the phone connected")
            {
                Message::Text(text) => text,
                _ => continue,
            };
            let frame: Value = serde_json::from_str(text.as_str()).unwrap();
            if std::env::var_os("MUNIMENT_RELAY_LIVE_TRACE").is_some() {
                eprintln!("phone <- {} {}", frame["type"], frame["payload"]);
            }
            if frame["type"] == "control.heartbeat" && frame["payload"]["kind"] == "ping" {
                let nonce = frame["payload"]["nonce"].clone();
                self.send(
                    "control.heartbeat",
                    Value::Null,
                    None,
                    json!({"kind": "pong", "nonce": nonce}),
                );
                continue;
            }
            if matches(&frame) {
                return frame;
            }
            self.held.push_back(frame);
        }
        panic!("the phone received no matching frame");
    }

    fn send(&mut self, kind: &str, session_id: Value, message_id: Option<&str>, payload: Value) {
        let message_id = message_id
            .map(str::to_owned)
            .unwrap_or_else(|| format!("msg_{}", URL_SAFE_NO_PAD.encode(random::<12>())));
        let envelope = json!({
            "protocol": RELAY_PROTOCOL, "type": kind, "message_id": message_id,
            "session_id": session_id, "connection_id": self.ready["connection_id"],
            "device_id": self.ready["device_id"], "actor": self.ready["actor"], "sequence": 0,
            "timestamp": now(), "payload": payload,
        });
        self.socket
            .send(Message::text(envelope.to_string()))
            .unwrap();
    }

    /// Waits for the desktop to list the session of `run_id`.
    fn discover(&mut self, run_id: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            self.send("session.list", Value::Null, None, json!({"limit": 100}));
            let listed = self.next(|frame| frame["type"] == "session.list.result");
            let sessions = listed["payload"]["sessions"].as_array().unwrap().clone();
            // The phone lists only its paired desktop's live sessions.
            if let Some(session) = sessions.first() {
                let session_id = session["session_id"].as_str().unwrap().to_owned();
                self.send(
                    "session.subscribe",
                    json!(session_id),
                    None,
                    json!({"after_sequence": 0, "limit": 100}),
                );
                self.next(|frame| frame["type"] == "session.subscribed");
                return session_id;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        panic!("the phone never listed the session of {run_id}");
    }

    /// The next sealed event, opened.
    fn event(&mut self) -> (EventHeader, EventContent) {
        let frame = self.next(|frame| frame["type"] == "session.event");
        let sealed: SealedEvent = serde_json::from_value(frame["payload"].clone()).unwrap();
        let opened = open_event(
            &self.keys.desktop_to_phone,
            &sealed,
            frame["session_id"].as_str().unwrap(),
        )
        .unwrap();
        (sealed.header, opened.content)
    }

    /// Seals and sends one command, and answers its id.
    fn command(
        &mut self,
        session_id: &str,
        run_id: &str,
        kind: CommandKind,
        gate_id: Option<&str>,
        content: CommandContent,
    ) -> String {
        let command_id = format!("cmd_{}", URL_SAFE_NO_PAD.encode(random::<12>()));
        let created = chrono::Utc::now();
        let header = CommandHeader {
            contract_version: CONTRACT_VERSION.into(),
            header_type: "command".into(),
            command_kind: kind,
            command_id: command_id.clone(),
            session_id: session_id.into(),
            run_id: run_id.into(),
            gate_id: gate_id.map(str::to_owned),
            created_at: timestamp(created),
            expires_at: timestamp(created + chrono::Duration::seconds(30)),
        };
        self.counter += 1;
        let sealed =
            seal_command(&self.keys.phone_to_desktop, &header, self.counter, &content).unwrap();
        self.send(
            kind.envelope_type(),
            json!(session_id),
            Some(&command_id),
            serde_json::to_value(sealed).unwrap(),
        );
        command_id
    }

    /// The desktop's applied receipt or terminal failure for a command.
    fn outcome(&mut self, command_id: &str) -> Value {
        self.next(|frame| {
            frame["payload"]["command_id"] == command_id
                && ((frame["type"] == "command.receipt" && frame["payload"]["stage"] == "applied")
                    || frame["type"] == "command.failure")
        })
    }
}

#[test]
#[ignore = "needs a running cloud API; see the module documentation"]
fn a_paired_phone_follows_and_steers_a_desktop_run_through_the_relay() {
    let Some(live) = live() else {
        eprintln!("MUNIMENT_RELAY_LIVE_API, _EMAIL and _PASSWORD are unset; nothing ran.");
        return;
    };
    let desktop = sign_in(&live, true);
    let mobile = sign_in(&live, false);

    // The desktop asks for a code with its own agreement key and shows the QR.
    let profile = std::env::temp_dir().join(format!(
        "muniment-relay-live-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&profile).unwrap();
    let store = profile.join("remote-control-pair.json");
    let identity = DesktopIdentity {
        device_id: desktop.device_id.parse().unwrap(),
        public_key: desktop.public_key.clone(),
    };
    let (view, pending) = create_pairing_challenge(
        &UreqPairingTransport::new(Duration::from_secs(20)),
        &live.api,
        &desktop.token,
        &identity,
        DesktopAgreement::generate().unwrap(),
    )
    .unwrap();
    let pending = Arc::new(Mutex::new(Some(pending)));
    let qr = scan(&view.qr_svg);
    assert_eq!(qr["contract_version"], "muniment.remote-control-pairing/2");
    assert_eq!(qr["desktop_device_id"], desktop.device_id.as_str());

    // The phone scans it, proves its agreement key and derives its keys.
    let phone_keys = AgreementKeyPair::generate().unwrap();
    let secret = PairingSecret::decode(qr["pairing_secret"].as_str().unwrap()).unwrap();
    let transcript = proof_transcript(
        &desktop.device_id,
        &mobile.device_id,
        qr["challenge"].as_str().unwrap(),
        qr["desktop_agreement_key"].as_str().unwrap(),
        &phone_keys.public_key(),
    );
    let mut authorize = qr.clone();
    authorize.as_object_mut().unwrap().remove("pairing_secret");
    authorize["mobile_agreement_key"] = json!(phone_keys.public_key());
    authorize["mobile_agreement_proof"] = json!(agreement_proof(&secret, &transcript));
    let (status, paired) = send_json(
        agent()
            .post(&format!("{}/v1/remote-control/pairing", live.api))
            .set("Authorization", &format!("Bearer {}", mobile.token)),
        Some(&authorize),
    );
    assert_eq!(status, 201, "authorize the pair: {paired}");
    let pair_id = paired["pair"]["pair_id"].as_str().unwrap().to_owned();
    let shared = phone_keys
        .agree(qr["desktop_agreement_key"].as_str().unwrap())
        .unwrap();
    let phone_relay_keys = derive_relay_keys(
        &shared,
        &secret,
        &pair_id,
        qr["desktop_agreement_key"].as_str().unwrap(),
        &phone_keys.public_key(),
    )
    .unwrap();

    // The desktop reads the pair, verifies the proof and stores the same keys.
    let keychain = Arc::new(MemoryKeys::default());
    let status = read_pairing(
        &UreqPairingTransport::new(Duration::from_secs(20)),
        &live.api,
        &desktop.token,
        &store,
        &pending,
        keychain.as_ref(),
        chrono::Utc::now(),
    )
    .unwrap();
    let phone = status.pair.unwrap();
    assert_eq!(phone.pair_id.to_string(), pair_id);
    assert_eq!(phone.remote_control, RemoteControlState::Ready);
    assert!(pending.lock().unwrap().is_none(), "the agreement is erased");
    assert_eq!(
        stored_relay_keys(&store, keychain.as_ref()),
        Some(phone_relay_keys.clone())
    );

    // The runtime's publisher connects and publishes a live run.
    let runtime = Arc::new(ScriptedRuntime::default());
    let account = Arc::new(DesktopAccount {
        api: live.api.clone(),
        token: desktop.token.clone(),
        store: store.clone(),
        keys: keychain.clone(),
        pending: pending.clone(),
    });
    let publisher = RelayPublisher::start(account, runtime.clone(), relay_state_path(&profile));
    let run_id = uuid::Uuid::now_v7().to_string();
    runtime.emit(run_event(&run_id, "streaming", "Reading the repository."));

    let mut phone = Phone::connect(&live, &mobile.token, phone_relay_keys.clone());
    let session_id = phone.discover(&run_id);
    let (header, content) = phone.event();
    assert_eq!(
        (header.sequence.as_str(), header.phase),
        ("1", Phase::Running)
    );
    assert_eq!(header.run_id, run_id);
    assert_eq!(content, EventContent::RunStatus { detail: None });
    let (header, content) = phone.event();
    assert_eq!(header.sequence, "2");
    assert_eq!(
        content,
        EventContent::AssistantDelta {
            text: "Reading the repository.".into()
        }
    );

    // A steer reaches the runtime and the phone sees the applied receipt.
    let steer = phone.command(
        &session_id,
        &run_id,
        CommandKind::Steer,
        None,
        CommandContent::Steer {
            text: "Focus on the failing test.".into(),
        },
    );
    let receipt = phone.outcome(&steer);
    assert_eq!(receipt["type"], "command.receipt");
    assert_eq!(receipt["payload"]["result"], "steered");
    assert_eq!(
        runtime.calls(),
        [format!("Steer {run_id} Focus on the failing test.")]
    );

    // A follow-up the runtime refuses ends in the desktop's terminal failure.
    let refused = phone.command(
        &session_id,
        &run_id,
        CommandKind::FollowUp,
        None,
        CommandContent::FollowUp {
            text: "Please refuse this one.".into(),
        },
    );
    let failure = phone.outcome(&refused);
    assert_eq!(failure["type"], "command.failure", "{failure}");
    assert_eq!(failure["payload"]["terminal_status"], "rejected");
    assert_eq!(failure["payload"]["code"], "invalid_command");
    assert_eq!(runtime.calls().len(), 1);

    // A permission gate opens in the header, and the phone's allow answers it.
    let mut asking = run_event(&run_id, "pending-permission", "Reading the repository.");
    asking.pending_permission = Some(ChatPendingPermission {
        gate_id: "gate-live-0001".into(),
        request: PermissionRequest::Confirm {
            title: "Run tests".into(),
            message: "cargo test".into(),
            timeout: None,
        },
        diff: None,
    });
    runtime.emit(asking);
    let (header, content) = phone.event();
    assert_eq!(header.phase, Phase::WaitingForPermission);
    let gate = header.permission.unwrap();
    let EventContent::PermissionAsk {
        gate_id,
        kind,
        summary,
        request_digest,
    } = content
    else {
        panic!("the phone expected an ask");
    };
    assert_eq!(
        (gate_id.as_str(), kind.as_str(), summary.as_str()),
        (gate.gate_id.as_str(), "confirm", "Run tests: cargo test")
    );
    let answer = phone.command(
        &session_id,
        &run_id,
        CommandKind::PermissionAnswer,
        Some(&gate_id),
        CommandContent::PermissionAnswer {
            gate_id: gate_id.clone(),
            decision: AnswerDecision::Allow,
            request_digest,
        },
    );
    let receipt = phone.outcome(&answer);
    assert_eq!(receipt["type"], "command.receipt");
    assert_eq!(receipt["payload"]["result"], Value::Null);
    assert_eq!(receipt["payload"]["gate_id"], gate_id.as_str());
    assert_eq!(
        runtime.calls()[1],
        format!(
            "answer {run_id} gate-live-0001 {}",
            json!({"type": "confirm", "value": true})
        )
    );
    runtime.emit(run_event(
        &run_id,
        "streaming",
        "Reading the repository. Tests pass.",
    ));
    let (header, content) = phone.event();
    assert_eq!((header.phase, header.permission), (Phase::Running, None));
    assert_eq!(
        content,
        EventContent::PermissionResolved {
            gate_id: gate_id.clone(),
            decision: GateDecision::Allow
        }
    );
    let (_, content) = phone.event();
    assert_eq!(
        content,
        EventContent::AssistantDelta {
            text: " Tests pass.".into()
        }
    );

    // A stop ends the run.
    let stop = phone.command(
        &session_id,
        &run_id,
        CommandKind::Stop,
        None,
        CommandContent::Stop {
            reason: StopReason::UserRequested,
        },
    );
    assert_eq!(phone.outcome(&stop)["payload"]["result"], "interrupted");
    runtime.emit(run_event(
        &run_id,
        "cancelled",
        "Reading the repository. Tests pass.",
    ));
    let (header, content) = phone.event();
    assert_eq!(header.phase, Phase::Cancelled);
    assert_eq!(content, EventContent::RunStatus { detail: None });

    // The phone revokes the pair. The desktop's next publish meets the
    // revoked pair, and the desktop erases the keys.
    let (status, revoked) = send_json(
        agent()
            .request("DELETE", &format!("{}/v1/remote-control/pairing", live.api))
            .set("Authorization", &format!("Bearer {}", mobile.token)),
        Some(&json!({"contract_version": "muniment.remote-control-pairing/2", "pair_id": pair_id})),
    );
    assert_eq!((status, revoked["revoked"].clone()), (200, json!(true)));
    let next_run = uuid::Uuid::now_v7().to_string();
    runtime.emit(run_event(&next_run, "streaming", "Another run."));
    let deadline = Instant::now() + Duration::from_secs(30);
    while stored_relay_keys(&store, keychain.as_ref()).is_some() {
        assert!(
            Instant::now() < deadline,
            "the desktop kept the keys of a revoked pair"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(keychain.0.lock().unwrap().is_empty());
    assert!(!store.exists());
    publisher.stop();
    std::fs::remove_dir_all(profile).unwrap();
}

/// A runtime that plays one run and answers the phone's commands as a live
/// run would: a steer opens a permission gate, an answer closes it with more
/// text, and a stop cancels the run.
struct ReactiveRuntime {
    script: ScriptedRuntime,
    run_id: String,
}

impl RelayRuntime for ReactiveRuntime {
    fn subscribe(&self) -> Option<ChatEventSubscription> {
        self.script.subscribe()
    }

    fn stop(&self, run_id: &str) -> Result<(), String> {
        self.script.stop(run_id)?;
        self.react(run_event(
            &self.run_id,
            "cancelled",
            "Reading the repository. Tests pass.",
        ));
        Ok(())
    }

    fn queue(&self, run_id: &str, delivery: ChatDelivery, text: &str) -> Result<(), String> {
        self.script.queue(run_id, delivery, text)?;
        if matches!(delivery, ChatDelivery::Steer) {
            let mut asking = run_event(
                &self.run_id,
                "pending-permission",
                "Reading the repository.",
            );
            asking.pending_permission = Some(ChatPendingPermission {
                gate_id: "gate-live-0001".into(),
                request: PermissionRequest::Confirm {
                    title: "Run tests".into(),
                    message: "cargo test".into(),
                    timeout: None,
                },
                diff: None,
            });
            self.react(asking);
        }
        Ok(())
    }

    fn answer(
        &self,
        run_id: &str,
        gate_id: &str,
        answer: ChatPermissionAnswer,
    ) -> Result<(), String> {
        self.script.answer(run_id, gate_id, answer)?;
        self.react(run_event(
            &self.run_id,
            "streaming",
            "Reading the repository. Tests pass.",
        ));
        Ok(())
    }
}

impl ReactiveRuntime {
    /// Sends an event from the publisher's own thread without waiting on it.
    fn react(&self, event: ChatEvent) {
        if let Some(sender) = self.script.events.lock().unwrap().as_ref() {
            let _ = sender.try_send(event);
        }
    }
}

/// The desktop side alone, for a phone that runs elsewhere, such as the
/// phone app's own clients. The desktop writes the scanned QR object to
/// `$MUNIMENT_RELAY_LIVE_PHONE_DIR/qr.json`, waits up to two minutes for the
/// pair, serves one run, and ends when the phone revokes the pair. The phone
/// steers the run, sends a follow-up containing "refuse", answers the
/// permission gate with allow, stops the run, and revokes the pair.
#[test]
#[ignore = "needs a running cloud API and an external phone; see the documentation"]
fn a_desktop_serves_a_phone_that_runs_elsewhere() {
    let (Some(live), Some(directory)) = (live(), std::env::var_os("MUNIMENT_RELAY_LIVE_PHONE_DIR"))
    else {
        eprintln!("MUNIMENT_RELAY_LIVE_PHONE_DIR or the API settings are unset; nothing ran.");
        return;
    };
    let directory = PathBuf::from(directory);
    let desktop = sign_in(&live, true);
    let profile = std::env::temp_dir().join(format!(
        "muniment-relay-live-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&profile).unwrap();
    let store = profile.join("remote-control-pair.json");
    let identity = DesktopIdentity {
        device_id: desktop.device_id.parse().unwrap(),
        public_key: desktop.public_key.clone(),
    };
    let (view, pending) = create_pairing_challenge(
        &UreqPairingTransport::new(Duration::from_secs(20)),
        &live.api,
        &desktop.token,
        &identity,
        DesktopAgreement::generate().unwrap(),
    )
    .unwrap();
    let pending = Arc::new(Mutex::new(Some(pending)));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("qr.json"), scan(&view.qr_svg).to_string()).unwrap();

    let keychain = Arc::new(MemoryKeys::default());
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let status = read_pairing(
            &UreqPairingTransport::new(Duration::from_secs(20)),
            &live.api,
            &desktop.token,
            &store,
            &pending,
            keychain.as_ref(),
            chrono::Utc::now(),
        )
        .unwrap();
        if let Some(phone) = status.pair {
            assert_eq!(phone.remote_control, RemoteControlState::Ready);
            break;
        }
        assert!(Instant::now() < deadline, "no phone paired");
        std::thread::sleep(Duration::from_secs(2));
    }

    let run_id = uuid::Uuid::now_v7().to_string();
    let runtime = Arc::new(ReactiveRuntime {
        script: ScriptedRuntime::default(),
        run_id: run_id.clone(),
    });
    let account = Arc::new(DesktopAccount {
        api: live.api.clone(),
        token: desktop.token.clone(),
        store: store.clone(),
        keys: keychain.clone(),
        pending,
    });
    let publisher = RelayPublisher::start(account, runtime.clone(), relay_state_path(&profile));
    runtime
        .script
        .emit(run_event(&run_id, "streaming", "Reading the repository."));

    // The phone's revocation ends the pair. After the stop, a second run
    // publishes every few seconds until a publish meets the revoked pair.
    let deadline = Instant::now() + Duration::from_secs(180);
    let next_run = uuid::Uuid::now_v7().to_string();
    let mut text = String::from("Another run.");
    let mut nudged = Instant::now();
    while stored_relay_keys(&store, keychain.as_ref()).is_some() {
        assert!(
            Instant::now() < deadline,
            "the phone never revoked the pair"
        );
        let stopped = runtime
            .script
            .calls()
            .iter()
            .any(|call| call.starts_with("stop"));
        if stopped && nudged.elapsed() > Duration::from_secs(3) {
            text.push('.');
            runtime
                .script
                .emit(run_event(&next_run, "streaming", &text));
            nudged = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    publisher.stop();
    let calls = runtime.script.calls();
    assert_eq!(calls.len(), 3, "{calls:?}");
    assert!(calls[0].starts_with(&format!("Steer {run_id} ")));
    assert!(calls[1].starts_with(&format!("answer {run_id} gate-live-0001 ")));
    assert_eq!(calls[2], format!("stop {run_id}"));
    std::fs::remove_dir_all(profile).unwrap();
}
