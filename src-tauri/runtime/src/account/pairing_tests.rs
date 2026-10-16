use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;

use std::collections::HashMap;

use muniment_desktop_integration::auth::{
    DesktopIdentity, NativeCredentialBackend, RemoteControlState,
};

use super::{pairing_status, request_pairing_challenge, revoke_pairing_pair, PairId, PairingError};

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

fn identity() -> DesktopIdentity {
    DesktopIdentity {
        device_id: PairId::parse_str("11111111-1111-4111-8111-111111111111").unwrap(),
        public_key: "ERERERERERERERERERERERERERERERERERERERERERE".into(),
    }
}

struct TemporaryProfile {
    profile: PathBuf,
}

impl TemporaryProfile {
    fn new(test_name: &str) -> Self {
        let profile = std::env::temp_dir().join(format!(
            "muniment-runtime-{test_name}-{}-{}",
            std::process::id(),
            PairId::new_v4()
        ));
        std::fs::create_dir_all(&profile).unwrap();
        Self { profile }
    }
}

impl Drop for TemporaryProfile {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}

fn spawn_server(status: u16, body: String) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_request(&mut stream);
        write!(
            stream,
            "HTTP/1.1 {status} Result\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        request
    });
    (base_url, handle)
}

fn read_request(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    loop {
        let mut buffer = [0; 1024];
        let read = stream.read(&mut buffer).unwrap();
        assert_ne!(read, 0, "the client sends the complete request");
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(head_length) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&bytes[..head_length]).to_ascii_lowercase();
            let length: usize = head
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length:")
                        .map(|value| value.trim().parse().unwrap())
                })
                .unwrap_or(0);
            if bytes.len() >= head_length + 4 + length {
                break;
            }
        }
    }
    String::from_utf8(bytes).unwrap()
}

static TEST_LOCK: Mutex<()> = Mutex::new(());

const PAIR_BODY: &str = r#"{"contract_version":"muniment.remote-control-pairing/2","pair":{"pair_id":"22222222-2222-4222-8222-222222222222","desktop_device_id":"11111111-1111-4111-8111-111111111111","mobile_device_id":"33333333-3333-4333-8333-333333333333","desktop_public_key":"ERERERERERERERERERERERERERERERERERERERERERE","mobile_public_key":"MzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzM","desktop_agreement_key":"6KrsLX6QfdRVWdJAmhtDtUF_o9ZCMXGw4V1IVaNEXG0","mobile_agreement_key":"CdcaPhKUvIKW5NLb7ms7FFgzWQ9_5Y14ypJ6dcPPYVM","mobile_agreement_proof":"psjrU57Tq1xCx8kwS9fjBJvn333oelNmkcw5HBY7FQ8","created_at":"2026-01-01T00:01:00.000Z"}}"#;
const EMPTY_PAIR_BODY: &str =
    r#"{"contract_version":"muniment.remote-control-pairing/2","pair":null}"#;
const REVOKE_BODY: &str =
    r#"{"contract_version":"muniment.remote-control-pairing/2","revoked":true}"#;

/// A revision 2 challenge that echoes this desktop and the agreement key the
/// request carried.
fn challenge_body(request: &str) -> String {
    let body = request.split("\r\n\r\n").nth(1).unwrap();
    let request: muniment_core::serde_json::Value =
        muniment_core::serde_json::from_str(body).unwrap();
    format!(
        r#"{{"contract_version":"muniment.remote-control-pairing/2","expires_at":"2026-01-01T00:02:00.000Z","qr":{{"contract_version":"muniment.remote-control-pairing/2","desktop_device_id":"11111111-1111-4111-8111-111111111111","desktop_public_key":"ERERERERERERERERERERERERERERERERERERERERERE","desktop_agreement_key":"{}","challenge":"IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiI"}}}}"#,
        request["desktop_agreement_key"].as_str().unwrap()
    )
}

fn spawn_challenge_server() -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_request(&mut stream);
        let body = challenge_body(&request);
        write!(
            stream,
            "HTTP/1.1 201 Result\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        request
    });
    (base_url, handle)
}

#[test]
fn request_pairing_challenge_sends_the_caller_token() {
    let _guard = TEST_LOCK.lock().unwrap();
    let (base_url, server) = spawn_challenge_server();
    std::env::set_var("MUNIMENT_API_BASE_URL", &base_url);

    let (result, _pending) = request_pairing_challenge("caller-access-token", &identity()).unwrap();
    assert!(result.qr_svg.contains("Pairing code"));
    assert!(!result
        .qr_svg
        .contains("IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiI"));
    let request = server.join().unwrap().to_ascii_lowercase();
    assert!(request.starts_with("post /v1/remote-control/pairing/challenges http/1.1\r\n"));
    assert!(request.contains("authorization: bearer caller-access-token\r\n"));
    std::env::remove_var("MUNIMENT_API_BASE_URL");
}

#[test]
fn pairing_status_clears_stored_identity_when_the_pair_is_absent() {
    let _guard = TEST_LOCK.lock().unwrap();
    let profile = TemporaryProfile::new("pairing-status");
    let keys = MemoryKeys::default();
    let pending = Mutex::new(None);
    let (base_url, pair_server) = spawn_server(200, PAIR_BODY.into());
    std::env::set_var("MUNIMENT_API_BASE_URL", &base_url);
    let paired = pairing_status("caller-access-token", &profile.profile, &pending, &keys).unwrap();
    let phone = paired.pair.unwrap();
    assert_eq!(
        phone.mobile_device_id.to_string(),
        "33333333-3333-4333-8333-333333333333"
    );
    assert_eq!(phone.remote_control, RemoteControlState::PairAgain);
    pair_server.join().unwrap();

    let (base_url, empty_server) = spawn_server(200, EMPTY_PAIR_BODY.into());
    std::env::set_var("MUNIMENT_API_BASE_URL", &base_url);
    let cleared = pairing_status("caller-access-token", &profile.profile, &pending, &keys).unwrap();
    assert!(cleared.pair.is_none());
    empty_server.join().unwrap();
    std::env::remove_var("MUNIMENT_API_BASE_URL");
}

#[test]
fn revoke_pairing_sends_the_pair_id() {
    let _guard = TEST_LOCK.lock().unwrap();
    let profile = TemporaryProfile::new("pairing-revoke");
    let keys = MemoryKeys::default();
    let (base_url, seed) = spawn_server(200, PAIR_BODY.into());
    std::env::set_var("MUNIMENT_API_BASE_URL", &base_url);
    pairing_status(
        "caller-access-token",
        &profile.profile,
        &Mutex::new(None),
        &keys,
    )
    .unwrap();
    seed.join().unwrap();

    let (base_url, server) = spawn_server(200, REVOKE_BODY.into());
    std::env::set_var("MUNIMENT_API_BASE_URL", &base_url);
    let pair_id = PairId::parse_str("22222222-2222-4222-8222-222222222222").unwrap();
    let result =
        revoke_pairing_pair("caller-access-token", pair_id, &profile.profile, &keys).unwrap();
    assert!(result.revoked);
    let request = server.join().unwrap().to_ascii_lowercase();
    assert!(request.starts_with("delete /v1/remote-control/pairing http/1.1\r\n"));
    assert!(request.contains("authorization: bearer caller-access-token\r\n"));
    assert!(request.contains("22222222-2222-4222-8222-222222222222"));
    std::env::remove_var("MUNIMENT_API_BASE_URL");
}

#[test]
fn replacement_conflict_and_rate_limit_are_distinct() {
    let _guard = TEST_LOCK.lock().unwrap();
    let (base_url, limited) = spawn_server(
        429,
        r#"{"error":{"code":"pairing_rate_limited","message":"The pairing request failed."}}"#
            .into(),
    );
    std::env::set_var("MUNIMENT_API_BASE_URL", &base_url);
    assert_eq!(
        request_pairing_challenge("caller-access-token", &identity()).unwrap_err(),
        PairingError::HttpStatus(429)
    );
    limited.join().unwrap();

    let (base_url, conflict) = spawn_server(
        409,
        r#"{"error":{"code":"pairing_conflict","message":"The pairing request failed."}}"#.into(),
    );
    std::env::set_var("MUNIMENT_API_BASE_URL", &base_url);
    assert_eq!(
        request_pairing_challenge("caller-access-token", &identity()).unwrap_err(),
        PairingError::HttpStatus(409)
    );
    conflict.join().unwrap();
    std::env::remove_var("MUNIMENT_API_BASE_URL");
}
