use super::*;
use std::net::TcpListener;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::Mutex;

use crate::relay_encryption::{
    open_control, open_event, seal_command, seal_request, CommandHeader, CommandKind, DirectionKey,
    OpenedControl, OpenedEvent, RequestHeader, SealedControl, SealedEvent,
};

const VECTORS: &str = include_str!(
    "../../../../protocol-fixtures/muniment.relay-encryption/1/relay-encryption-v1.json"
);

fn keys() -> RelayKeys {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let pairing = &vectors["pairing"];
    RelayKeys {
        pair_id: pairing["pair_id"].as_str().unwrap().to_owned(),
        desktop_to_phone: DirectionKey::decode(pairing["desktop_to_phone_key"].as_str().unwrap())
            .unwrap(),
        phone_to_desktop: DirectionKey::decode(pairing["phone_to_desktop_key"].as_str().unwrap())
            .unwrap(),
    }
}

struct FakeAccount {
    api_base: String,
    keys: Mutex<Option<RelayKeys>>,
}

impl RelayAccount for FakeAccount {
    fn keys(&self) -> Option<RelayKeys> {
        self.keys.lock().unwrap().clone()
    }

    fn connection(&self, pair_id: &str) -> Option<RelayConnection> {
        (self.keys()?.pair_id == pair_id).then(|| RelayConnection {
            api_base: self.api_base.clone(),
            access_token: "test-access-token".into(),
        })
    }
}

#[derive(Default)]
struct FakeRuntime {
    events: Mutex<Option<SyncSender<ChatEvent>>>,
    calls: Mutex<Vec<String>>,
}

impl RelayRuntime for FakeRuntime {
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

impl FakeRuntime {
    fn emit(&self, event: ChatEvent) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            // A publisher that resubscribes drops the earlier stream.
            if let Some(sender) = self.events.lock().unwrap().as_ref() {
                if sender.send(event.clone()).is_ok() {
                    return;
                }
            }
            assert!(Instant::now() < deadline, "the publisher never subscribed");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_for_call(&self, count: usize) -> Vec<String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let calls = self.calls.lock().unwrap().clone();
            if calls.len() >= count {
                return calls;
            }
            assert!(Instant::now() < deadline, "the runtime saw {calls:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

const RUN: &str = "0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";

fn run_event(phase: &str, text: &str) -> ChatEvent {
    ChatEvent {
        run_id: RUN.into(),
        thread_id: Some("thread_local_0001".into()),
        phase: phase.into(),
        text: text.into(),
        prompt_accepted: true,
        turn_started: true,
        routing_stage: None,
        prompt_storage_notice: None,
        resume_notice: None,
        failure_reason: None,
        failure_detail: None,
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

/// The cloud side of one connection, as a test drives it.
struct StubRelay {
    socket: WebSocket<TcpStream>,
    authorization: String,
}

impl StubRelay {
    // The handshake callback's error type is tungstenite's own response.
    #[allow(clippy::result_large_err)]
    fn accept(listener: &TcpListener) -> Self {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut authorization = String::new();
        let socket = tungstenite::accept_hdr(
            stream,
            |request: &tungstenite::handshake::server::Request,
             response: tungstenite::handshake::server::Response| {
                assert_eq!(request.uri().path(), EVENTS_PATH);
                authorization = request
                    .headers()
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default()
                    .to_owned();
                Ok(response)
            },
        )
        .unwrap();
        let mut relay = Self {
            socket,
            authorization,
        };
        relay.send(json!({
            "type": "connection.ready",
            "service": "api",
            "version": "v1",
            "time": "2026-07-10T16:00:00.000Z",
            "data": {
                "protocol": RELAY_PROTOCOL,
                "session_relay_protocols": [RELAY_PROTOCOL],
                "connection_id": "con_desktop_0001",
                "org_id": "org_synthetic_0001",
                "user_id": "usr_synthetic_0001",
                "device_id": "11111111-1111-4111-8111-111111111111",
                "client_role": "desktop",
                "expires_at": "2099-01-01T00:00:00.000Z",
                "actor": {
                    "user_id": "usr_synthetic_0001",
                    "device_id": "11111111-1111-4111-8111-111111111111",
                    "role": "desktop",
                },
            },
        }));
        relay
    }

    fn send(&mut self, message: Value) {
        self.socket
            .send(Message::text(message.to_string()))
            .unwrap();
    }

    /// The next relay message of type `kind`, skipping heartbeats.
    fn next(&mut self, kind: &str) -> Value {
        loop {
            let message = match self.socket.read().unwrap() {
                Message::Text(text) => serde_json::from_str::<Value>(text.as_str()).unwrap(),
                _ => continue,
            };
            assert_eq!(message["protocol"], RELAY_PROTOCOL);
            assert_eq!(message["connection_id"], "con_desktop_0001");
            assert_eq!(message["actor"]["role"], "desktop");
            assert_eq!(message["actor"]["user_id"], "usr_synthetic_0001");
            assert_eq!(message["sequence"], 0);
            if message["type"] == kind {
                return message;
            }
            assert_eq!(message["type"], "session.event", "unexpected {message}");
        }
    }

    /// The next sealed event, opened with the phone's view of the keys.
    fn next_event(&mut self, keys: &RelayKeys) -> (EventHeader, OpenedEvent) {
        let message = self.next("session.event");
        let sealed: SealedEvent = serde_json::from_value(message["payload"].clone()).unwrap();
        let opened = open_event(
            &keys.desktop_to_phone,
            &sealed,
            message["session_id"].as_str().unwrap(),
        )
        .unwrap();
        (sealed.header, opened)
    }

    fn command(
        &mut self,
        keys: &RelayKeys,
        header: &CommandHeader,
        counter: u64,
        content: &CommandContent,
    ) {
        let sealed = seal_command(&keys.phone_to_desktop, header, counter, content).unwrap();
        self.forward(header, serde_json::to_value(sealed).unwrap());
    }

    fn forward(&mut self, header: &CommandHeader, payload: Value) {
        self.send(json!({
            "protocol": RELAY_PROTOCOL,
            "type": header.command_kind.envelope_type(),
            "message_id": header.command_id,
            "session_id": header.session_id,
            "connection_id": "con_mobile_0001",
            "device_id": "33333333-3333-4333-8333-333333333333",
            "actor": {
                "user_id": "usr_synthetic_0001",
                "device_id": "33333333-3333-4333-8333-333333333333",
                "role": "mobile",
            },
            "sequence": 0,
            "timestamp": relay_encryption::timestamp(Utc::now()),
            "payload": payload,
        }));
    }
}

fn command_header(
    kind: CommandKind,
    command_id: &str,
    session_id: &str,
    run_id: &str,
    gate_id: Option<&str>,
) -> CommandHeader {
    let now = Utc::now();
    CommandHeader {
        contract_version: relay_encryption::CONTRACT_VERSION.into(),
        header_type: "command".into(),
        command_kind: kind,
        command_id: command_id.into(),
        session_id: session_id.into(),
        run_id: run_id.into(),
        gate_id: gate_id.map(str::to_owned),
        created_at: relay_encryption::timestamp(now),
        expires_at: relay_encryption::timestamp(now + chrono::Duration::seconds(30)),
    }
}

fn temporary_state(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "muniment-relay-{name}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

fn timing() -> RelayTiming {
    RelayTiming {
        idle_poll: Duration::from_millis(20),
        backoff_min: Duration::from_millis(20),
        backoff_max: Duration::from_millis(100),
        read_tick: Duration::from_millis(10),
        silence_limit: Duration::from_secs(30),
        pair_check: Duration::from_millis(50),
    }
}

#[test]
fn the_publisher_registers_seals_applies_and_receipts() {
    let keys = keys();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let account = Arc::new(FakeAccount {
        api_base: format!("http://{}", listener.local_addr().unwrap()),
        keys: Mutex::new(Some(keys.clone())),
    });
    let runtime = Arc::new(FakeRuntime::default());
    let profile = temporary_state("publisher");
    let publisher = RelayPublisher::start_with_timing(
        account.clone(),
        runtime.clone(),
        relay_state_path(&profile),
        timing(),
    );
    let mut relay = StubRelay::accept(&listener);
    assert_eq!(relay.authorization, "Bearer test-access-token");

    // The relay's application heartbeat gets its pong.
    relay.send(json!({
        "protocol": RELAY_PROTOCOL, "type": "control.heartbeat", "message_id": "msg_ping_0001",
        "session_id": null, "connection_id": "con_desktop_0001",
        "device_id": "11111111-1111-4111-8111-111111111111",
        "actor": {"user_id": "usr_synthetic_0001", "device_id": "11111111-1111-4111-8111-111111111111", "role": "desktop"},
        "sequence": 0, "timestamp": "2026-07-10T16:00:01.000Z", "payload": {"kind": "ping", "nonce": "n-301"},
    }));
    let pong = relay.next("control.heartbeat");
    assert_eq!(pong["payload"], json!({"kind": "pong", "nonce": "n-301"}));

    // A device-wide notice is no run, and a live run registers a session and
    // publishes sealed events.
    let mut notice = run_event("sign-in-link", "https://example.test/sign-in");
    notice.run_id = "sign-in".into();
    runtime.emit(notice);
    runtime.emit(run_event("streaming", "Hello"));
    let register = relay.next("session.register");
    let session_id = register["session_id"].as_str().unwrap().to_owned();
    assert!(relay_encryption::valid_id(&session_id));
    assert_eq!(register["payload"]["run_id"], RUN);
    assert_eq!(register["payload"]["state"], "running");
    assert_eq!(
        register["payload"]["client_protocols"],
        json!([RELAY_PROTOCOL])
    );
    let (header, opened) = relay.next_event(&keys);
    assert_eq!(header.sequence, "1");
    assert_eq!(header.phase, Phase::Running);
    assert_eq!(opened.content, EventContent::RunStatus { detail: None });
    let (header, opened) = relay.next_event(&keys);
    assert_eq!(header.sequence, "2");
    assert_eq!(header.run_id, RUN);
    assert_eq!(
        opened.content,
        EventContent::AssistantDelta {
            text: "Hello".into()
        }
    );
    let first_counter = opened.counter;

    // A streamed delta extends the text.
    let mut delta = run_event("streaming", ", phone");
    delta.delta = Some(crate::run_events::ChatTextDelta::new(5));
    runtime.emit(delta);
    let (header, opened) = relay.next_event(&keys);
    assert_eq!(header.sequence, "3");
    assert!(opened.counter > first_counter);
    assert_eq!(
        opened.content,
        EventContent::AssistantDelta {
            text: ", phone".into()
        }
    );

    // A sealed steer reaches the runtime and earns an applied receipt.
    let steer = command_header(
        CommandKind::Steer,
        "cmd_test_steer_0001",
        &session_id,
        RUN,
        None,
    );
    let content = CommandContent::Steer {
        text: "Focus on the tests".into(),
    };
    relay.command(&keys, &steer, 1, &content);
    let receipt = relay.next("command.receipt");
    assert_eq!(receipt["session_id"], session_id.as_str());
    assert_eq!(receipt["payload"]["command_id"], "cmd_test_steer_0001");
    assert_eq!(receipt["payload"]["stage"], "applied");
    assert_eq!(receipt["payload"]["result"], "steered");
    assert_eq!(
        runtime.wait_for_call(1),
        [format!("Steer {RUN} Focus on the tests")]
    );

    // The same sealed command again is a replay: the receipt repeats and
    // the runtime applies nothing more.
    let sealed = seal_command(&keys.phone_to_desktop, &steer, 1, &content).unwrap();
    relay.forward(&steer, serde_json::to_value(sealed).unwrap());
    let repeated = relay.next("command.receipt");
    assert_eq!(repeated["payload"], receipt["payload"]);
    assert_eq!(runtime.calls.lock().unwrap().len(), 1);

    // A permission gate opens in the header and the ask travels sealed.
    let mut asking = run_event("pending-permission", "Hello, phone");
    asking.pending_permission = Some(ChatPendingPermission {
        gate_id: "gate_local_0001".into(),
        request: PermissionRequest::Confirm {
            title: "Run tests".into(),
            message: "cargo test".into(),
            timeout: None,
        },
        diff: None,
    });
    runtime.emit(asking);
    let (header, opened) = relay.next_event(&keys);
    assert_eq!(header.phase, Phase::WaitingForPermission);
    let window = header.permission.clone().unwrap();
    assert_eq!(window.gate_id, "gate_local_0001");
    let EventContent::PermissionAsk {
        gate_id,
        kind,
        summary,
        request_digest,
    } = opened.content
    else {
        panic!("expected an ask");
    };
    assert_eq!(
        (gate_id.as_str(), kind.as_str(), summary.as_str()),
        ("gate_local_0001", "confirm", "Run tests: cargo test")
    );

    // A changed header never opens: the desktop rejects it and asks for a new pair.
    let answer = command_header(
        CommandKind::PermissionAnswer,
        "cmd_test_answer_0001",
        &session_id,
        RUN,
        Some("gate_local_0001"),
    );
    let answer_content = CommandContent::PermissionAnswer {
        gate_id: "gate_local_0001".into(),
        decision: AnswerDecision::Allow,
        request_digest: request_digest.clone(),
    };
    let mut tampered = seal_command(&keys.phone_to_desktop, &answer, 2, &answer_content).unwrap();
    tampered.header.expires_at =
        relay_encryption::timestamp(Utc::now() + chrono::Duration::seconds(29));
    relay.forward(&answer, serde_json::to_value(&tampered).unwrap());
    let failure = relay.next("command.failure");
    assert_eq!(failure["payload"]["command_id"], "cmd_test_answer_0001");
    assert_eq!(failure["payload"]["terminal_status"], "rejected");
    assert_eq!(failure["payload"]["code"], "invalid_command");
    assert!(relay_key_mismatch(&profile, &keys.pair_id));

    // The phone's sealed allow answers the gate with a null result.
    relay.command(&keys, &answer, 3, &answer_content);
    let receipt = relay.next("command.receipt");
    assert_eq!(receipt["payload"]["gate_id"], "gate_local_0001");
    assert_eq!(receipt["payload"]["result"], Value::Null);
    let calls = runtime.wait_for_call(2);
    assert_eq!(
        calls[1],
        format!(
            "answer {RUN} gate_local_0001 {}",
            json!({"type": "confirm", "value": true})
        )
    );

    // The gate closes with the phone's decision, and the run ends.
    runtime.emit(run_event("streaming", "Hello, phone"));
    let (header, opened) = relay.next_event(&keys);
    assert_eq!(header.phase, Phase::Running);
    assert_eq!(header.permission, None);
    assert_eq!(
        opened.content,
        EventContent::PermissionResolved {
            gate_id: "gate_local_0001".into(),
            decision: GateDecision::Allow
        }
    );
    runtime.emit(run_event("complete", "Hello, phone"));
    let (header, opened) = relay.next_event(&keys);
    assert_eq!(header.phase, Phase::Completed);
    assert_eq!(opened.content, EventContent::RunStatus { detail: None });

    // A command for an ended run is rejected.
    let stop = command_header(
        CommandKind::Stop,
        "cmd_test_stop_0001",
        &session_id,
        RUN,
        None,
    );
    relay.command(
        &keys,
        &stop,
        4,
        &CommandContent::Stop {
            reason: relay_encryption::StopReason::UserRequested,
        },
    );
    let failure = relay.next("command.failure");
    assert_eq!(failure["payload"]["command_id"], "cmd_test_stop_0001");
    assert_eq!(runtime.calls.lock().unwrap().len(), 2);

    // Revoking the pair closes the connection.
    *account.keys.lock().unwrap() = None;
    let closed = loop {
        match relay.socket.read() {
            Ok(Message::Close(_)) | Err(_) => break true,
            Ok(_) => continue,
        }
    };
    assert!(closed);
    publisher.stop();
    let state = load_state(&relay_state_path(&profile), &keys.pair_id);
    assert!(state.send_reserved >= 1);
    assert_eq!(state.commands.highest, 4);
    std::fs::remove_dir_all(profile).unwrap();
}

fn gate_event(phase: &str, text: &str, gate_id: &str) -> ChatEvent {
    let mut event = run_event(phase, text);
    event.pending_permission = Some(ChatPendingPermission {
        gate_id: gate_id.into(),
        request: PermissionRequest::Confirm {
            title: "Run tests".into(),
            message: String::new(),
            timeout: None,
        },
        diff: None,
    });
    event
}

#[test]
fn a_gate_closes_before_later_text_and_after_a_run_s_last_text() {
    let keys = keys();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let account = Arc::new(FakeAccount {
        api_base: format!("http://{}", listener.local_addr().unwrap()),
        keys: Mutex::new(Some(keys.clone())),
    });
    let runtime = Arc::new(FakeRuntime::default());
    let profile = temporary_state("gate-order");
    let publisher = RelayPublisher::start_with_timing(
        account,
        runtime.clone(),
        relay_state_path(&profile),
        timing(),
    );
    let mut relay = StubRelay::accept(&listener);
    runtime.emit(run_event("streaming", "Hi"));
    relay.next("session.register");
    relay.next_event(&keys);
    relay.next_event(&keys);
    runtime.emit(gate_event("pending-permission", "Hi", "gate_local_0001"));
    let (header, _) = relay.next_event(&keys);
    assert_eq!(header.phase, Phase::WaitingForPermission);

    // The desktop answered the gate, and the run goes on with more text.
    runtime.emit(run_event("streaming", "Hi there"));
    let (header, OpenedEvent { content, .. }) = relay.next_event(&keys);
    assert_eq!((header.phase, header.permission), (Phase::Running, None));
    assert!(matches!(content, EventContent::PermissionResolved { .. }));
    let (_, OpenedEvent { content, .. }) = relay.next_event(&keys);
    assert_eq!(
        content,
        EventContent::AssistantDelta {
            text: " there".into()
        }
    );

    // A run stopped at its gate sends its last text, then closes the gate.
    runtime.emit(gate_event(
        "pending-permission",
        "Hi there",
        "gate_local_0002",
    ));
    relay.next_event(&keys);
    runtime.emit(gate_event("cancelled", "Hi there!", "gate_local_0002"));
    let (header, OpenedEvent { content, .. }) = relay.next_event(&keys);
    assert_eq!(header.phase, Phase::WaitingForPermission);
    assert_eq!(content, EventContent::AssistantDelta { text: "!".into() });
    let (header, OpenedEvent { content, .. }) = relay.next_event(&keys);
    assert_eq!((header.phase, header.permission), (Phase::Cancelled, None));
    assert_eq!(
        content,
        EventContent::PermissionResolved {
            gate_id: "gate_local_0002".into(),
            decision: GateDecision::Cancelled
        }
    );
    let (header, OpenedEvent { content, .. }) = relay.next_event(&keys);
    assert_eq!(header.phase, Phase::Cancelled);
    assert_eq!(content, EventContent::RunStatus { detail: None });
    publisher.stop();
    std::fs::remove_dir_all(profile).unwrap();
}

#[test]
fn a_pair_the_cloud_no_longer_holds_ends_the_connection() {
    let keys = keys();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let account = Arc::new(FakeAccount {
        api_base: format!("http://{}", listener.local_addr().unwrap()),
        keys: Mutex::new(Some(keys.clone())),
    });
    let runtime = Arc::new(FakeRuntime::default());
    let profile = temporary_state("pair-unavailable");
    // A slow pair check leaves the relay's refusal as the only way out.
    let publisher = RelayPublisher::start_with_timing(
        account.clone(),
        runtime.clone(),
        relay_state_path(&profile),
        RelayTiming {
            pair_check: Duration::from_secs(60),
            ..timing()
        },
    );
    let mut relay = StubRelay::accept(&listener);
    runtime.emit(run_event("streaming", "Hello"));
    let register = relay.next("session.register");
    let event = relay.next("session.event");
    *account.keys.lock().unwrap() = None;
    relay.send(json!({
        "protocol": RELAY_PROTOCOL, "type": "protocol.error", "message_id": event["message_id"],
        "session_id": register["session_id"], "connection_id": "con_desktop_0001",
        "device_id": "11111111-1111-4111-8111-111111111111",
        "actor": {"user_id": "usr_synthetic_0001", "device_id": "11111111-1111-4111-8111-111111111111", "role": "desktop"},
        "sequence": 0, "timestamp": "2026-07-10T16:00:01.000Z", "payload": {"code": "pair_unavailable"},
    }));
    let closed = loop {
        match relay.socket.read() {
            Ok(Message::Close(_)) | Err(_) => break true,
            Ok(_) => continue,
        }
    };
    assert!(closed);
    listener.set_nonblocking(true).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert!(listener.accept().is_err(), "no reconnect without keys");
    publisher.stop();
    std::fs::remove_dir_all(profile).unwrap();
}

#[test]
fn a_publisher_without_keys_never_connects() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let account = Arc::new(FakeAccount {
        api_base: format!("http://{}", listener.local_addr().unwrap()),
        keys: Mutex::new(None),
    });
    let profile = temporary_state("idle");
    let publisher = RelayPublisher::start_with_timing(
        account,
        Arc::new(FakeRuntime::default()),
        relay_state_path(&profile),
        timing(),
    );
    std::thread::sleep(Duration::from_millis(200));
    assert!(listener.accept().is_err(), "no connection without keys");
    publisher.stop();
    std::fs::remove_dir_all(profile).unwrap();
}

#[test]
fn the_publisher_reconnects_and_registers_its_live_runs_again() {
    let keys = keys();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let account = Arc::new(FakeAccount {
        api_base: format!("http://{}", listener.local_addr().unwrap()),
        keys: Mutex::new(Some(keys.clone())),
    });
    let runtime = Arc::new(FakeRuntime::default());
    let profile = temporary_state("reconnect");
    let publisher = RelayPublisher::start_with_timing(
        account,
        runtime.clone(),
        relay_state_path(&profile),
        timing(),
    );
    let mut relay = StubRelay::accept(&listener);
    runtime.emit(run_event("streaming", "One"));
    let first = relay.next("session.register");
    let (header, _) = relay.next_event(&keys);
    assert_eq!(header.sequence, "1");
    relay.next_event(&keys);
    drop(relay);

    // The next connection re-registers the same session, and its sequence
    // and the sealed text continue where they stopped.
    let mut relay = StubRelay::accept(&listener);
    runtime.emit(run_event("streaming", "One two"));
    let second = relay.next("session.register");
    assert_eq!(second["session_id"], first["session_id"]);
    let (header, opened) = relay.next_event(&keys);
    assert_eq!(header.sequence, "3");
    assert_eq!(
        opened.content,
        EventContent::RunStatus { detail: None },
        "a registration restates the run's status"
    );
    let (header, opened) = relay.next_event(&keys);
    assert_eq!(header.sequence, "4");
    assert_eq!(
        opened.content,
        EventContent::AssistantDelta {
            text: " two".into()
        }
    );
    publisher.stop();
    std::fs::remove_dir_all(profile).unwrap();
}

#[test]
fn deltas_split_long_text_and_redact_credentials() {
    let long = "é".repeat(MAX_DELTA_BYTES);
    let pieces = deltas(&long);
    assert_eq!(pieces.len(), 2);
    let joined: String = pieces
        .iter()
        .map(|piece| match piece {
            EventContent::AssistantDelta { text } => text.as_str(),
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(joined, long);
    let secret = format!("token: sk-ant-api03-{}", "a".repeat(90));
    let redacted = redact(&secret);
    assert!(redacted.contains("[redacted]"), "{redacted}");
    assert!(!redacted.contains(&"a".repeat(90)));
}

#[test]
fn events_url_follows_the_api_scheme() {
    assert_eq!(
        events_url("https://api.example.test").unwrap().as_str(),
        "wss://api.example.test/v1/events"
    );
    assert_eq!(
        events_url("http://127.0.0.1:8080/").unwrap().as_str(),
        "ws://127.0.0.1:8080/v1/events"
    );
    assert!(events_url("ftp://example.test").is_err());
}

#[test]
fn a_deny_cancels_and_an_allow_needs_a_yes_or_no_gate() {
    let now = Utc::now();
    let gate = |request| {
        open_gate(
            &ChatPendingPermission {
                gate_id: "gate with spaces".into(),
                request,
                diff: None,
            },
            now,
        )
    };
    let select = gate(PermissionRequest::Select {
        title: "Pick".into(),
        options: vec!["A".into()],
        timeout: Some(60_000),
    });
    assert!(relay_encryption::valid_id(&select.relay_id));
    assert_eq!(select.expires - now, chrono::Duration::seconds(60));
    assert!(local_answer(&select, AnswerDecision::Allow).is_none());
    assert!(matches!(
        local_answer(&select, AnswerDecision::Deny),
        Some(ChatPermissionAnswer::Cancelled)
    ));
    let code = gate(PermissionRequest::CodeDiff {
        effect_id: "effect".into(),
        code_diff_id: "diff".into(),
        diff_sha256: "a".into(),
        write_plan_sha256: "b".into(),
    });
    assert_eq!(code.expires - now, chrono::Duration::minutes(5));
    assert!(matches!(
        local_answer(&code, AnswerDecision::Allow),
        Some(ChatPermissionAnswer::CodeDiff { gate_id, .. }) if gate_id == "gate with spaces"
    ));
}

/// A runtime that serves the control session with canned answers and a
/// fleet event stream the test feeds.
#[derive(Default)]
struct ControlRuntime {
    inner: FakeRuntime,
    fleet: Mutex<Option<std::sync::mpsc::Sender<ClientEvent>>>,
    requests: Mutex<Vec<(String, Value, String)>>,
}

impl RelayRuntime for ControlRuntime {
    fn subscribe(&self) -> Option<ChatEventSubscription> {
        self.inner.subscribe()
    }
    fn stop(&self, run_id: &str) -> Result<(), String> {
        self.inner.stop(run_id)
    }
    fn queue(&self, run_id: &str, delivery: ChatDelivery, text: &str) -> Result<(), String> {
        self.inner.queue(run_id, delivery, text)
    }
    fn answer(
        &self,
        run_id: &str,
        gate_id: &str,
        answer: ChatPermissionAnswer,
    ) -> Result<(), String> {
        self.inner.answer(run_id, gate_id, answer)
    }
    fn serves_control(&self) -> bool {
        true
    }
    fn request(
        &self,
        operation: muniment_attach::Operation,
        body: Value,
        request_id: &str,
    ) -> Result<Value, RequestError> {
        let name = serde_json::to_value(operation).unwrap();
        let name = name.as_str().unwrap().to_owned();
        self.requests
            .lock()
            .unwrap()
            .push((name.clone(), body, request_id.to_owned()));
        match name.as_str() {
            "swarm.members" => Ok(json!({"self": "rt_studio", "members": [
                {"runtime_id": "rt_studio", "runtime_name": "Studio", "self": true, "online": true},
                {"runtime_id": "rt_box", "runtime_name": "Box", "self": false, "online": true},
            ]})),
            "swarm.agents" => Ok(
                json!({"agents": [{"agent_id": "agent_scout", "name": "Scout",
                "primary_runtime": "rt_box"}]}),
            ),
            "thread.summaries" => Ok(json!({"summaries": [{"threadId": "thread_one",
                "title": "One", "updatedAt": "2026-10-17T12:00:00.000Z", "owner_runtime": "rt_box"}],
                "nextCursor": null})),
            "thread.post" => Ok(json!({"forwarded": true, "op_id": "op_1", "state": "sent",
                "message_id": "op_1"})),
            _ => Ok(json!({})),
        }
    }
    fn fleet_events(&self) -> Option<std::sync::mpsc::Receiver<ClientEvent>> {
        let (sender, receiver) = std::sync::mpsc::channel();
        *self.fleet.lock().unwrap() = Some(sender);
        Some(receiver)
    }
}

const CONTROL: &str = "ctl_000000000000400080000000000000a1";

fn request_header(request_id: &str, lifetime: i64) -> RequestHeader {
    let now = Utc::now();
    RequestHeader::new(
        request_id,
        CONTROL,
        &relay_encryption::timestamp(now),
        &relay_encryption::timestamp(now + chrono::Duration::seconds(lifetime)),
    )
}

impl StubRelay {
    fn request(&mut self, keys: &RelayKeys, header: &RequestHeader, counter: u64, content: Value) {
        let content: RequestContent = serde_json::from_value(content).unwrap();
        let sealed = seal_request(&keys.phone_to_desktop, header, counter, &content).unwrap();
        self.send(json!({
            "protocol": RELAY_PROTOCOL, "type": "control.request", "message_id": header.request_id,
            "session_id": header.session_id, "connection_id": "con_mobile_0001",
            "device_id": "33333333-3333-4333-8333-333333333333",
            "actor": {"user_id": "usr_synthetic_0001",
                "device_id": "33333333-3333-4333-8333-333333333333", "role": "mobile"},
            "sequence": 0, "timestamp": relay_encryption::timestamp(Utc::now()),
            "payload": serde_json::to_value(sealed).unwrap(),
        }));
    }

    /// The next control session event, opened as the phone opens it.
    fn next_control(&mut self, keys: &RelayKeys) -> (ControlHeader, OpenedControl) {
        loop {
            let message = self.next("session.event");
            if message["session_id"] != CONTROL {
                continue;
            }
            let sealed: SealedControl = serde_json::from_value(message["payload"].clone()).unwrap();
            let opened = open_control(&keys.desktop_to_phone, &sealed, CONTROL).unwrap();
            return (sealed.header, opened);
        }
    }

    /// The joined answer of the next response.
    fn response(&mut self, keys: &RelayKeys) -> (String, Value) {
        let mut text = String::new();
        loop {
            let (_, opened) = self.next_control(keys);
            let ControlContent::Response {
                request_id,
                page,
                pages,
                body,
            } = opened.content
            else {
                continue;
            };
            text.push_str(&body);
            if page + 1 == pages {
                return (request_id, serde_json::from_str(&text).unwrap());
            }
        }
    }
}

fn control_keys() -> RelayKeys {
    RelayKeys {
        pair_id: "00000000-0000-4000-8000-0000000000a1".into(),
        ..keys()
    }
}

#[test]
fn the_control_session_answers_requests_and_forwards_fleet_events() {
    let keys = control_keys();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let account = Arc::new(FakeAccount {
        api_base: format!("http://{}", listener.local_addr().unwrap()),
        keys: Mutex::new(Some(keys.clone())),
    });
    let runtime = Arc::new(ControlRuntime::default());
    let profile = temporary_state("control");
    let publisher = RelayPublisher::start_with_timing(
        account,
        runtime.clone(),
        relay_state_path(&profile),
        timing(),
    );
    let mut relay = StubRelay::accept(&listener);
    let register = relay.next("session.register");
    assert_eq!(register["session_id"], CONTROL);
    assert_eq!(
        register["payload"],
        json!({"channel": "control", "client_protocols": [RELAY_PROTOCOL]})
    );

    relay.request(
        &keys,
        &request_header("req_snapshot_0001", 60),
        1,
        json!({"type": "request", "request_id": "req_snapshot_0001",
            "operation": "fleet.snapshot", "body": {}}),
    );
    let (request_id, answer) = relay.response(&keys);
    assert_eq!(request_id, "req_snapshot_0001");
    assert_eq!(answer["ok"], true);
    let snapshot = &answer["result"];
    assert_eq!(snapshot["members"][1]["runtime_name"], "Box");
    assert_eq!(snapshot["agents"][0]["primary_runtime"], "rt_box");
    assert_eq!(snapshot["threads"][0]["owner_runtime"], "rt_box");

    // A post goes to the runtime with the request id as its key.
    relay.request(
        &keys,
        &request_header("req_post_00000001", 60),
        2,
        json!({"type": "request", "request_id": "req_post_00000001",
            "operation": "thread.post", "body": {"thread_id": "thread_one", "text": "Hello"}}),
    );
    let (_, answer) = relay.response(&keys);
    assert_eq!(answer["result"]["op_id"], "op_1");
    assert!(runtime.requests.lock().unwrap().contains(&(
        "thread.post".to_owned(),
        json!({"thread_id": "thread_one", "text": "Hello"}),
        "req_post_00000001".to_owned()
    )));

    // A replayed counter is dropped; an operation off the phone list and an
    // expired request are refused in a sealed answer.
    relay.request(
        &keys,
        &request_header("req_replay_000001", 60),
        2,
        json!({"type": "request", "request_id": "req_replay_000001",
            "operation": "thread.post", "body": {"thread_id": "thread_one", "text": "Again"}}),
    );
    relay.request(
        &keys,
        &request_header("req_admin_0000001", 60),
        3,
        json!({"type": "request", "request_id": "req_admin_0000001",
            "operation": "agent.save", "body": {}}),
    );
    let (request_id, answer) = relay.response(&keys);
    assert_eq!(request_id, "req_admin_0000001");
    assert_eq!(answer["error"]["code"], "unsupported_operation");
    relay.request(
        &keys,
        &request_header("req_late_00000001", -1),
        4,
        json!({"type": "request", "request_id": "req_late_00000001",
            "operation": "fleet.sync_status", "body": {}}),
    );
    let (_, answer) = relay.response(&keys);
    assert_eq!(answer["error"]["code"], "invalid_request");
    let made = Utc::now() - chrono::Duration::seconds(61);
    let stale = RequestHeader::new(
        "req_stale_0000001",
        CONTROL,
        &relay_encryption::timestamp(made),
        &relay_encryption::timestamp(made + chrono::Duration::seconds(60)),
    );
    relay.request(
        &keys,
        &stale,
        5,
        json!({"type": "request", "request_id": "req_stale_0000001",
            "operation": "fleet.sync_status", "body": {}}),
    );
    let (_, answer) = relay.response(&keys);
    assert_eq!(answer["error"]["code"], "expired");
    relay.request(
        &keys,
        &request_header("req_long_00000001", 120),
        6,
        json!({"type": "request", "request_id": "req_long_00000001",
            "operation": "fleet.sync_status", "body": {}}),
    );
    let (_, answer) = relay.response(&keys);
    assert_eq!(answer["error"]["code"], "invalid_request");

    // Fleet events travel; an approval wakes the phone, others do not.
    let sender = runtime.fleet.lock().unwrap().clone().unwrap();
    sender
        .send(ClientEvent {
            name: muniment_attach::EventName::ModelsLogin,
            body: json!({}),
        })
        .unwrap();
    sender
        .send(ClientEvent {
            name: muniment_attach::EventName::SwarmChanged,
            body: json!({"kind": "agents"}),
        })
        .unwrap();
    sender
        .send(ClientEvent {
            name: muniment_attach::EventName::AgentApprovalPending,
            body: json!({"approval_id": "run_1:gate_1"}),
        })
        .unwrap();
    let (header, opened) = relay.next_control(&keys);
    assert!(!header.wake);
    assert_eq!(
        opened.content,
        ControlContent::Event {
            event: "swarm.changed".into(),
            body: Some(json!({"kind": "agents"}))
        }
    );
    let (header, opened) = relay.next_control(&keys);
    assert!(header.wake);
    assert!(
        matches!(opened.content, ControlContent::Event { event, .. } if event == "agent.approval_pending")
    );
    let last_sequence: u64 = header.sequence.parse().unwrap();
    assert!(runtime
        .requests
        .lock()
        .unwrap()
        .iter()
        .all(|(_, body, _)| body["text"] != "Again"));
    drop(relay);

    // A reconnect registers the session again and its sequence goes on.
    let mut relay = StubRelay::accept(&listener);
    assert_eq!(relay.next("session.register")["session_id"], CONTROL);
    relay.request(
        &keys,
        &request_header("req_status_000001", 60),
        7,
        json!({"type": "request", "request_id": "req_status_000001",
            "operation": "fleet.sync_status", "body": {}}),
    );
    let (header, _) = relay.next_control(&keys);
    assert!(header.sequence.parse::<u64>().unwrap() > last_sequence);
    publisher.stop();
    std::fs::remove_dir_all(profile).unwrap();
}

#[test]
fn a_request_sealed_with_another_key_marks_the_pair_for_pairing_again() {
    let keys = control_keys();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let account = Arc::new(FakeAccount {
        api_base: format!("http://{}", listener.local_addr().unwrap()),
        keys: Mutex::new(Some(keys.clone())),
    });
    let runtime = Arc::new(ControlRuntime::default());
    let profile = temporary_state("control-mismatch");
    let publisher = RelayPublisher::start_with_timing(
        account,
        runtime.clone(),
        relay_state_path(&profile),
        timing(),
    );
    let mut relay = StubRelay::accept(&listener);
    relay.next("session.register");
    let wrong = RelayKeys {
        phone_to_desktop: relay_encryption::DirectionKey::from_bytes([9; 32]),
        ..keys.clone()
    };
    relay.request(
        &wrong,
        &request_header("req_wrong_0000001", 60),
        1,
        json!({"type": "request", "request_id": "req_wrong_0000001",
            "operation": "fleet.sync_status", "body": {}}),
    );
    let (_, answer) = relay.response(&keys);
    assert_eq!(answer["error"]["code"], "invalid_request");
    assert!(relay_key_mismatch(&profile, &keys.pair_id));
    assert!(runtime.requests.lock().unwrap().is_empty());
    publisher.stop();
    std::fs::remove_dir_all(profile).unwrap();
}
