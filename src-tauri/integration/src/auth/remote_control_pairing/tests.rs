use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use muniment_core::relay_encryption::{
    AgreementKeyPair, DesktopAgreement, DirectionKey, PairingSecret, RelayKeys,
};
use muniment_core::relay_publisher::{relay_state_path, RELAY_STATE_FILE_NAME};

use super::{
    challenge_expired, create_pairing_challenge, displayed_qr_json, qr_json, read_pairing,
    relay_key_entry, revoke_pairing, stored_relay_keys, DesktopIdentity, PairingError, PairingQr,
    PendingAgreement, RemoteControlState, UreqPairingTransport, CHALLENGE_PATH, CONTRACT_VERSION,
    LEGACY_CONTRACT_VERSION, PAIRING_PATH, REPLACEMENT_INTERVAL,
};
use crate::auth::NativeCredentialBackend;
use base64::Engine as _;
use serde_json::Value;
use uuid::Uuid;

const TOKEN: &str = "native-access-secret";
const PAIR_ID: &str = "22222222-2222-4222-8222-222222222222";
const LEGACY_PAIR_BODY: &str = r#"{"contract_version":"muniment.remote-control-pairing/1","pair":{"pair_id":"22222222-2222-4222-8222-222222222222","desktop_device_id":"11111111-1111-4111-8111-111111111111","mobile_device_id":"33333333-3333-4333-8333-333333333333","desktop_public_key":"ERERERERERERERERERERERERERERERERERERERERERE","mobile_public_key":"MzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzM","created_at":"2026-01-01T00:01:00.000Z"}}"#;
const EMPTY_PAIR_BODY: &str =
    r#"{"contract_version":"muniment.remote-control-pairing/2","pair":null}"#;
const REVOKE_BODY: &str =
    r#"{"contract_version":"muniment.remote-control-pairing/2","revoked":true}"#;

/// A loopback server that answers each connection with the next response
/// and keeps every request it read.
struct Server {
    base_url: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Server {
    fn spawn(status: u16, body: String) -> Self {
        Self::sequence(vec![(status, body)])
    }

    fn sequence(responses: Vec<(u16, String)>) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        std::thread::spawn(move || {
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                captured.lock().unwrap().push(read_request(&mut stream));
                write!(stream, "HTTP/1.1 {status} Result\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        Self { base_url, requests }
    }

    fn request(&self, index: usize) -> Option<String> {
        self.requests.lock().unwrap().get(index).cloned()
    }
}

fn read_request(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    loop {
        let mut buffer = [0; 1024];
        let read = stream.read(&mut buffer).unwrap();
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            let header = bytes
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .unwrap();
            let headers = String::from_utf8_lossy(&bytes[..header]);
            let length = headers.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                (name.eq_ignore_ascii_case("content-length"))
                    .then(|| value.trim().parse::<usize>().ok())?
            });
            if let Some(length) = length {
                let body_start = header + 4;
                while bytes.len() < body_start + length {
                    let read = stream.read(&mut buffer).unwrap();
                    bytes.extend_from_slice(&buffer[..read]);
                }
            }
            break;
        }
    }
    String::from_utf8(bytes).unwrap()
}

/// Keychain entries in memory.
#[derive(Default)]
struct MemoryKeys(Mutex<HashMap<String, String>>);

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

fn transport() -> UreqPairingTransport {
    UreqPairingTransport::new(Duration::from_secs(2))
}

fn store_path(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "muniment-pairing-{name}-{}-{}",
        std::process::id(),
        Uuid::new_v4()
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path.join("remote-control-pair.json")
}

/// The relay encryption contract vectors, `muniment.relay-encryption/1` with
/// pairing revision `muniment.remote-control-pairing/2`, copied unchanged
/// from the cloud's conformance package.
fn vectors() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/remote-control/relay-encryption-v1.json"
    ))
    .unwrap()
}

fn legacy_fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/remote-control/pairing-v1.json"
    ))
    .unwrap()
}

fn text(value: &Value, field: &str) -> String {
    value[field].as_str().unwrap().to_owned()
}

fn identity() -> DesktopIdentity {
    let qr = &vectors()["pairing_wire"]["challenge_response"]["qr"];
    DesktopIdentity {
        device_id: Uuid::parse_str(&text(qr, "desktop_device_id")).unwrap(),
        public_key: text(qr, "desktop_public_key"),
    }
}

/// The desktop agreement the vectors start from.
fn vector_agreement() -> DesktopAgreement {
    let pairing = &vectors()["pairing"];
    let private = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(text(pairing, "desktop_agreement_private_key"))
        .unwrap();
    DesktopAgreement::from_parts(
        AgreementKeyPair::from_private_bytes(private.try_into().unwrap()),
        PairingSecret::decode(&text(pairing, "pairing_secret")).unwrap(),
    )
}

fn vector_keys() -> RelayKeys {
    let pairing = &vectors()["pairing"];
    RelayKeys {
        pair_id: text(pairing, "pair_id"),
        desktop_to_phone: DirectionKey::decode(&text(pairing, "desktop_to_phone_key")).unwrap(),
        phone_to_desktop: DirectionKey::decode(&text(pairing, "phone_to_desktop_key")).unwrap(),
    }
}

fn challenge_body() -> String {
    vectors()["pairing_wire"]["challenge_response"].to_string()
}

fn pair_body() -> String {
    vectors()["pairing_wire"]["pair_response"].to_string()
}

/// Shows the vector challenge and returns the pending agreement.
fn pending_challenge() -> Mutex<Option<PendingAgreement>> {
    let server = Server::spawn(201, challenge_body());
    let (_, pending) = create_pairing_challenge(
        &transport(),
        &server.base_url,
        TOKEN,
        &identity(),
        vector_agreement(),
    )
    .unwrap();
    Mutex::new(Some(pending))
}

fn now() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339("2026-01-01T00:01:30.000Z")
        .unwrap()
        .with_timezone(&chrono::Utc)
}

#[test]
fn challenge_posts_the_agreement_key_and_shows_the_secret_only_in_the_qr() {
    let vectors = vectors();
    let wire = &vectors["pairing_wire"];
    let server = Server::spawn(201, challenge_body());
    let (view, _) = create_pairing_challenge(
        &transport(),
        &server.base_url,
        TOKEN,
        &identity(),
        vector_agreement(),
    )
    .unwrap();
    assert_eq!(
        view.expires_at
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "2026-01-01T00:02:00.000Z"
    );
    assert!(view.qr_svg.contains("Pairing code"));
    let secret = text(&vectors["pairing"], "pairing_secret");
    assert!(!view.qr_svg.contains(&secret));

    let request = server.request(0).unwrap();
    assert!(request.starts_with(&format!("POST {CHALLENGE_PATH} HTTP/1.1\r\n")));
    assert!(request
        .to_ascii_lowercase()
        .contains("authorization: bearer native-access-secret\r\n"));
    let body: Value = serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(body, wire["challenge_request"]);
    assert!(
        !request.contains(&secret),
        "the cloud never receives the secret"
    );

    let qr: PairingQr = serde_json::from_value(wire["challenge_response"]["qr"].clone()).unwrap();
    let payload = displayed_qr_json(&qr, &secret).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        wire["displayed_qr"]
    );
    for forbidden in [
        "http://",
        "https://",
        "Bearer",
        "access_token",
        "private_key",
    ] {
        assert!(!payload.contains(forbidden));
    }
    assert!(!format!("{qr:?}").contains("IiIi"));
}

#[test]
fn rendered_qr_decodes_the_exact_displayed_object_in_both_themes() {
    let expected = vectors()["pairing_wire"]["displayed_qr"].clone();
    let server = Server::spawn(201, challenge_body());
    let (view, _) = create_pairing_challenge(
        &transport(),
        &server.base_url,
        TOKEN,
        &identity(),
        vector_agreement(),
    )
    .unwrap();
    for (ink, surface) in [("#1A1D1C", "#FFFFFF"), ("#ECEFED", "#0E1110")] {
        let themed = view.qr_svg.replacen(
            "<svg ",
            &format!("<svg color=\"{ink}\" style=\"--surface:{surface}\" "),
            1,
        );
        let tree = resvg::usvg::Tree::from_str(&themed, &Default::default()).unwrap();
        let mut pixmap = resvg::tiny_skia::Pixmap::new(300, 300).unwrap();
        let scale = 300.0 / tree.size().width();
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        let pixels = pixmap.pixels();
        let gray: Vec<u8> = pixels.iter().map(|pixel| pixel.red()).collect();
        let quiet = (4.0 * scale).floor() as usize;
        for y in 0..300 {
            for x in 0..300 {
                if x < quiet || y < quiet || x >= 300 - quiet || y >= 300 - quiet {
                    let pixel = pixels[y * 300 + x];
                    assert_eq!(
                        (pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()),
                        (255, 255, 255, 255)
                    );
                }
            }
        }
        let mut decoder = quircs::Quirc::default();
        let codes: Vec<_> = decoder.identify(300, 300, &gray).collect();
        assert_eq!(codes.len(), 1);
        let decoded = codes.into_iter().next().unwrap().unwrap().decode().unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&decoded.payload).unwrap(),
            expected
        );
    }
}

#[test]
fn challenge_refuses_a_qr_that_names_another_desktop_or_key() {
    let vectors = vectors();
    let qr = &vectors["pairing_wire"]["challenge_response"]["qr"];
    let other_key = text(&vectors["repaired_pairing"], "desktop_agreement_key");
    let cases = [
        challenge_body().replace(
            &text(qr, "desktop_device_id"),
            "44444444-4444-4444-8444-444444444444",
        ),
        challenge_body().replace(
            &text(qr, "desktop_public_key"),
            "MzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzM",
        ),
        challenge_body().replace(&text(qr, "desktop_agreement_key"), &other_key),
    ];
    for body in cases {
        let server = Server::spawn(201, body);
        let error = create_pairing_challenge(
            &transport(),
            &server.base_url,
            TOKEN,
            &identity(),
            vector_agreement(),
        )
        .unwrap_err();
        assert!(matches!(error, PairingError::MalformedResponse(_)));
    }
}

#[test]
fn challenge_rejects_unknown_fields_tokens_and_caller_origins() {
    let challenge = "\"challenge\":\"IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiI\"";
    let cases = [
        challenge_body().replace(
            challenge,
            &format!("{challenge},\"origin\":\"https://evil.example\""),
        ),
        challenge_body().replace(
            challenge,
            &format!("{challenge},\"access_token\":\"secret\""),
        ),
        challenge_body().replace(CONTRACT_VERSION, LEGACY_CONTRACT_VERSION),
        format!("{} trailing-secret", challenge_body()),
        challenge_body().replace("\"qr\":{", "\"qr\":{\"private_key\":\"secret\","),
    ];
    for body in cases {
        let server = Server::spawn(201, body);
        let error = create_pairing_challenge(
            &transport(),
            &server.base_url,
            TOKEN,
            &identity(),
            vector_agreement(),
        )
        .unwrap_err();
        assert!(matches!(error, PairingError::MalformedResponse(_)));
        assert!(!format!("{error:?} {error}").contains("secret"));
        assert!(!format!("{error:?}").contains(TOKEN));
    }
}

#[test]
fn replacement_within_ten_seconds_is_rate_limited() {
    assert_eq!(REPLACEMENT_INTERVAL, Duration::from_secs(10));
    let server = Server::spawn(
        429,
        r#"{"error":{"code":"pairing_rate_limited","message":"The pairing request failed."}}"#
            .into(),
    );
    let error = create_pairing_challenge(
        &transport(),
        &server.base_url,
        TOKEN,
        &identity(),
        vector_agreement(),
    )
    .unwrap_err();
    assert_eq!(error, PairingError::HttpStatus(429));
    assert_eq!(
        error.to_string(),
        "Wait ten seconds before replacing the code."
    );
}

#[test]
fn already_paired_desktop_cannot_create_a_challenge() {
    let server = Server::spawn(
        409,
        r#"{"error":{"code":"pairing_conflict","message":"The pairing request failed."}}"#.into(),
    );
    let error = create_pairing_challenge(
        &transport(),
        &server.base_url,
        TOKEN,
        &identity(),
        vector_agreement(),
    )
    .unwrap_err();
    assert_eq!(error, PairingError::HttpStatus(409));
    assert_eq!(error.to_string(), "A phone is already paired.");
}

#[test]
fn a_completed_pair_derives_and_stores_the_relay_keys() {
    let store = store_path("complete");
    let keys = MemoryKeys::default();
    let pending = pending_challenge();
    let server = Server::spawn(200, pair_body());
    let view = read_pairing(
        &transport(),
        &server.base_url,
        TOKEN,
        &store,
        &pending,
        &keys,
        now(),
    )
    .unwrap();
    let phone = view.pair.unwrap();
    assert_eq!(phone.pair_id, Uuid::parse_str(PAIR_ID).unwrap());
    assert_eq!(phone.remote_control, RemoteControlState::Ready);
    assert!(
        pending.lock().unwrap().is_none(),
        "the agreement material is erased"
    );
    let entry = relay_key_entry(phone.pair_id);
    let stored = RelayKeys::from_secret(&keys.get(&entry).unwrap().unwrap()).unwrap();
    assert_eq!(stored, vector_keys());
    assert_eq!(stored_relay_keys(&store, &keys), Some(vector_keys()));

    let request = server.request(0).unwrap();
    assert!(request.starts_with(&format!("GET {PAIRING_PATH} HTTP/1.1\r\n")));
    let on_disk = std::fs::read_to_string(&store).unwrap();
    assert!(on_disk.contains("33333333-3333-4333-8333-333333333333"));
    assert!(!on_disk.contains(&vector_keys().desktop_to_phone.encode()));
    assert!(!on_disk.contains(TOKEN));

    // A later read keeps the stored keys.
    let again = Server::spawn(200, pair_body());
    let view = read_pairing(
        &transport(),
        &again.base_url,
        TOKEN,
        &store,
        &pending,
        &keys,
        now(),
    )
    .unwrap();
    assert_eq!(view.pair.unwrap().remote_control, RemoteControlState::Ready);
}

#[test]
fn a_substituted_key_or_proof_revokes_the_pair() {
    let vectors = vectors();
    let pair = &vectors["pairing_wire"]["pair_response"]["pair"];
    let repaired = &vectors["repaired_pairing"];
    let cases = [
        pair_body().replace(
            &text(pair, "mobile_agreement_key"),
            &text(repaired, "mobile_agreement_key"),
        ),
        pair_body().replace(
            &text(pair, "mobile_agreement_proof"),
            &text(repaired, "mobile_agreement_proof"),
        ),
        pair_body().replace(
            &text(pair, "desktop_agreement_key"),
            &text(repaired, "desktop_agreement_key"),
        ),
    ];
    for body in cases {
        let store = store_path("substituted");
        let keys = MemoryKeys::default();
        let pending = pending_challenge();
        let server = Server::sequence(vec![(200, body), (200, REVOKE_BODY.into())]);
        let error = read_pairing(
            &transport(),
            &server.base_url,
            TOKEN,
            &store,
            &pending,
            &keys,
            now(),
        )
        .unwrap_err();
        assert_eq!(error, PairingError::Agreement);
        assert!(pending.lock().unwrap().is_none());
        assert!(keys.0.lock().unwrap().is_empty());
        assert!(!store.exists());
        let revoke = server.request(1).unwrap();
        assert!(revoke.starts_with(&format!("DELETE {PAIRING_PATH} HTTP/1.1\r\n")));
        assert!(revoke.contains(&format!(r#""pair_id":"{PAIR_ID}""#)));
    }
}

#[test]
fn a_pair_without_keys_reads_as_pair_again() {
    for body in [pair_body(), LEGACY_PAIR_BODY.to_owned()] {
        let store = store_path("no-keys");
        let keys = MemoryKeys::default();
        let server = Server::spawn(200, body);
        let view = read_pairing(
            &transport(),
            &server.base_url,
            TOKEN,
            &store,
            &Mutex::new(None),
            &keys,
            now(),
        )
        .unwrap();
        assert_eq!(
            view.pair.unwrap().remote_control,
            RemoteControlState::PairAgain
        );
        assert!(store.exists());
    }
}

#[test]
fn a_failed_command_turns_the_pair_to_pair_again() {
    let store = store_path("mismatch");
    let keys = MemoryKeys::default();
    let pending = pending_challenge();
    let server = Server::spawn(200, pair_body());
    read_pairing(
        &transport(),
        &server.base_url,
        TOKEN,
        &store,
        &pending,
        &keys,
        now(),
    )
    .unwrap();
    let profile = store.parent().unwrap();
    std::fs::write(
        relay_state_path(profile),
        format!(
            r#"{{"pair_id":"{PAIR_ID}","send_reserved":0,"commands":{{"highest":0,"accepted":0}},"key_mismatch":true}}"#
        ),
    )
    .unwrap();
    let again = Server::spawn(200, pair_body());
    let view = read_pairing(
        &transport(),
        &again.base_url,
        TOKEN,
        &store,
        &pending,
        &keys,
        now(),
    )
    .unwrap();
    assert_eq!(
        view.pair.unwrap().remote_control,
        RemoteControlState::PairAgain
    );
}

#[test]
fn a_gone_pair_erases_its_keys_and_an_expired_code_its_agreement() {
    let store = store_path("gone");
    let keys = MemoryKeys::default();
    let pending = pending_challenge();
    let server = Server::spawn(200, pair_body());
    read_pairing(
        &transport(),
        &server.base_url,
        TOKEN,
        &store,
        &pending,
        &keys,
        now(),
    )
    .unwrap();
    let profile = store.parent().unwrap().to_path_buf();
    std::fs::write(profile.join(RELAY_STATE_FILE_NAME), "{}").unwrap();

    let empty = Server::spawn(200, EMPTY_PAIR_BODY.into());
    let view = read_pairing(
        &transport(),
        &empty.base_url,
        TOKEN,
        &store,
        &pending,
        &keys,
        now(),
    )
    .unwrap();
    assert!(view.pair.is_none());
    assert!(!store.exists());
    assert!(keys.0.lock().unwrap().is_empty());
    assert!(!profile.join(RELAY_STATE_FILE_NAME).exists());

    // A code on screen keeps its agreement until it expires.
    let pending = pending_challenge();
    let empty = Server::spawn(200, EMPTY_PAIR_BODY.into());
    read_pairing(
        &transport(),
        &empty.base_url,
        TOKEN,
        &store,
        &pending,
        &keys,
        now(),
    )
    .unwrap();
    assert!(pending.lock().unwrap().is_some());
    let expired = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:02:00.000Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let empty = Server::spawn(200, EMPTY_PAIR_BODY.into());
    read_pairing(
        &transport(),
        &empty.base_url,
        TOKEN,
        &store,
        &pending,
        &keys,
        expired,
    )
    .unwrap();
    assert!(pending.lock().unwrap().is_none());
}

#[test]
fn revoke_erases_the_relay_keys_and_counters() {
    let store = store_path("revoke");
    let keys = MemoryKeys::default();
    let pending = pending_challenge();
    let seed = Server::spawn(200, pair_body());
    read_pairing(
        &transport(),
        &seed.base_url,
        TOKEN,
        &store,
        &pending,
        &keys,
        now(),
    )
    .unwrap();
    let profile = store.parent().unwrap().to_path_buf();
    std::fs::write(profile.join(RELAY_STATE_FILE_NAME), "{}").unwrap();
    assert!(!keys.0.lock().unwrap().is_empty());

    let pair_id = Uuid::parse_str(PAIR_ID).unwrap();
    let server = Server::spawn(200, REVOKE_BODY.into());
    let view = revoke_pairing(
        &transport(),
        &server.base_url,
        TOKEN,
        pair_id,
        &store,
        &keys,
    )
    .unwrap();
    assert!(view.revoked);
    assert!(!store.exists());
    assert!(keys.0.lock().unwrap().is_empty());
    assert!(!profile.join(RELAY_STATE_FILE_NAME).exists());

    let request = server.request(0).unwrap();
    assert!(request.starts_with(&format!("DELETE {PAIRING_PATH} HTTP/1.1\r\n")));
    assert!(request.contains(&format!(r#""pair_id":"{pair_id}""#)));
    assert!(request.contains(&format!(r#""contract_version":"{CONTRACT_VERSION}""#)));
    assert!(!request.contains("mobile_public_key"));
}

#[test]
fn revoke_keeps_a_replacement_pair_when_the_deleted_id_differs() {
    let store = store_path("stale-revoke");
    let keys = MemoryKeys::default();
    let pending = pending_challenge();
    let seed = Server::spawn(200, pair_body());
    read_pairing(
        &transport(),
        &seed.base_url,
        TOKEN,
        &store,
        &pending,
        &keys,
        now(),
    )
    .unwrap();
    let other = Uuid::parse_str("44444444-4444-4444-8444-444444444444").unwrap();
    let server = Server::spawn(200, REVOKE_BODY.into());
    revoke_pairing(&transport(), &server.base_url, TOKEN, other, &store, &keys).unwrap();
    assert!(store.exists());
    assert_eq!(stored_relay_keys(&store, &keys), Some(vector_keys()));
}

#[test]
fn expiry_at_the_deadline_removes_the_code() {
    let deadline = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:02:00.000Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    assert!(!challenge_expired(
        deadline,
        deadline - chrono::TimeDelta::seconds(1)
    ));
    assert!(challenge_expired(deadline, deadline));
    assert!(challenge_expired(
        deadline,
        deadline + chrono::TimeDelta::seconds(1)
    ));
}

#[test]
fn strict_schema_rejects_invalid_pair_responses() {
    let store = store_path("invalid");
    let keys = MemoryKeys::default();
    let pending = pending_challenge();
    let seed = Server::spawn(200, pair_body());
    read_pairing(
        &transport(),
        &seed.base_url,
        TOKEN,
        &store,
        &pending,
        &keys,
        now(),
    )
    .unwrap();
    let stored = std::fs::read(&store).unwrap();
    let cases = [
        format!(r#"{{"contract_version":"{CONTRACT_VERSION}"}}"#),
        pair_body().replace(PAIR_ID, "not-a-uuid"),
        pair_body().replace(
            "\"mobile_public_key\":\"REREREREREREREREREREREREREREREREREREREREREQ\"",
            "\"mobile_public_key\":\"short\"",
        ),
        pair_body().replace(
            "\"created_at\":\"2026-01-01T00:01:00.000Z\"",
            "\"created_at\":\"2026-01-01T00:01:00.000Z\",\"origin\":\"https://evil.example\"",
        ),
        format!("{} trailing-secret", pair_body()),
    ];
    for body in cases {
        let server = Server::spawn(200, body);
        assert!(matches!(
            read_pairing(
                &transport(),
                &server.base_url,
                TOKEN,
                &store,
                &Mutex::new(None),
                &keys,
                now()
            ),
            Err(PairingError::MalformedResponse(_))
        ));
        assert_eq!(std::fs::read(&store).unwrap(), stored);
    }
}

#[test]
fn status_malformed_and_transport_errors_are_redacted() {
    let store = store_path("errors");
    let keys = MemoryKeys::default();
    let oversized = format!(
        r#"{{"contract_version":"{CONTRACT_VERSION}","pair":null,"padding":"{}"}}"#,
        "x".repeat(20_000)
    );
    let servers = [
        Server::spawn(401, "response-secret".into()),
        Server::spawn(200, "response-secret".into()),
        Server::spawn(200, oversized),
    ];
    for server in &servers {
        let error = read_pairing(
            &transport(),
            &server.base_url,
            TOKEN,
            &store,
            &Mutex::new(None),
            &keys,
            now(),
        )
        .unwrap_err();
        let rendered = format!("{error:?} {error}");
        assert!(!rendered.contains(TOKEN));
        assert!(!rendered.contains("response-secret"));
    }
}

#[test]
fn invalid_bases_and_empty_credentials_fail_before_a_request() {
    for base in [
        "http://example.com",
        "https://api.muniment.ai/path",
        "not a URL",
    ] {
        assert!(matches!(
            create_pairing_challenge(&transport(), base, TOKEN, &identity(), vector_agreement()),
            Err(PairingError::Config(_))
        ));
    }
    let server = Server::spawn(201, challenge_body());
    assert_eq!(
        create_pairing_challenge(
            &transport(),
            &server.base_url,
            "",
            &identity(),
            vector_agreement()
        )
        .unwrap_err(),
        PairingError::CredentialsMissing
    );
    assert!(server.request(0).is_none());
}

#[test]
fn a_revision_one_qr_no_longer_encodes() {
    let legacy: Result<PairingQr, _> =
        serde_json::from_value(legacy_fixture()["responses"][0]["qr"].clone());
    assert!(legacy.is_err(), "a revision 1 QR has no agreement key");
    let mut qr = vectors()["pairing_wire"]["challenge_response"]["qr"].clone();
    qr["contract_version"] = LEGACY_CONTRACT_VERSION.into();
    let qr: PairingQr = serde_json::from_value(qr).unwrap();
    assert!(matches!(
        qr_json(&qr),
        Err(PairingError::MalformedResponse(_))
    ));
}
