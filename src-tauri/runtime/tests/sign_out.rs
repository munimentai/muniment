use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Mutex;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use muniment_core::auth::{
    EntitlementSnapshotTracker, InstallationRecord, KeyringNativeCredentialStore,
    NativeCredentialStore, NativeCredentials, TokenSet,
};
use muniment_runtime::sign_out;

const DEVICE_ID: &str = "10000000-0000-4000-8000-000000000001";
static TEST_LOCK: Mutex<()> = Mutex::new(());

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn credentials() -> NativeCredentials {
    NativeCredentials {
        installation: InstallationRecord {
            private_key: [1; 32],
            device_id: DEVICE_ID.parse().unwrap(),
            registration_token: "registration-secret".into(),
            device_challenge: "challenge-secret".into(),
            registration_expires_at: unix_time() + 7_200,
        },
        tokens: TokenSet {
            access_token: "access-secret".into(),
            refresh_token: Some("refresh-secret".into()),
            expires_at: Some(unix_time() + 3_600),
            subject: Some("user".into()),
        },
        refresh_expires_at: unix_time() + 7_200,
    }
}

fn spawn_server(status: &str, body: &str) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_request(&mut stream);
        stream.write_all(response.as_bytes()).unwrap();
        request
    });
    (base_url, handle)
}

fn read_request(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    loop {
        let mut buffer = [0; 1024];
        let read = stream.read(&mut buffer).unwrap();
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8(bytes).unwrap()
}

fn save_session() -> KeyringNativeCredentialStore {
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let store = KeyringNativeCredentialStore::new();
    store.clear_session().unwrap();
    store.save_credentials(&credentials()).unwrap();
    store
}

#[test]
fn sign_out_revokes_the_server_session_and_clears_the_local_session() {
    let _guard = TEST_LOCK.lock().unwrap();
    let store = save_session();
    let tracker = EntitlementSnapshotTracker::new();
    tracker.observe(Some(7));
    let (base_url, server) = spawn_server("200 OK", r#"{"ok":true}"#);
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);

    let status = sign_out(&tracker).unwrap();

    assert!(!status.signed_in);
    assert!(store.load_credentials().unwrap().is_none());
    assert_eq!(tracker.observe(Some(8)), None);
    let request = server.join().unwrap().to_ascii_lowercase();
    assert!(request.starts_with("post /v1/auth/native/revoke http/1.1\r\n"));
    assert!(request.contains("authorization: bearer access-secret\r\n"));
    std::env::remove_var("MUNIMENT_API_BASE_URL");
}

#[test]
fn sign_out_clears_the_local_session_when_revocation_is_rejected() {
    let _guard = TEST_LOCK.lock().unwrap();
    let store = save_session();
    let tracker = EntitlementSnapshotTracker::new();
    let (base_url, server) = spawn_server("401 Unauthorized", r#"{"ok":false}"#);
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);

    let status = sign_out(&tracker).unwrap();

    assert!(!status.signed_in);
    assert!(store.load_credentials().unwrap().is_none());
    server.join().unwrap();
    std::env::remove_var("MUNIMENT_API_BASE_URL");
}
