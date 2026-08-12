use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use muniment_core::auth::{
    EntitlementSnapshotTracker, InstallationRecord, KeyringNativeCredentialStore,
    NativeCredentialStore, NativeCredentials, TokenSet,
};
use muniment_runtime::{entitlement_snapshot, EntitlementSnapshotError};

const DEVICE_ID: &str = "10000000-0000-4000-8000-000000000001";

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

fn session_body(version: u64) -> String {
    format!(
        r#"{{"session":{{"org_id":"20000000-0000-4000-8000-000000000002","user_id":"30000000-0000-4000-8000-000000000003","role":"owner","device_id":"{DEVICE_ID}","client_role":"desktop"}},"entitlement_snapshot":{{"payload":{{"version":{version},"user_display_name":"User","organization_display_name":"Muniment","groups":[]}},"signature":"signature-secret","algorithm":"hmac-sha256"}}}}"#
    )
}

fn spawn_server(body: String) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_request(&mut stream);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
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
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8(bytes).unwrap()
}

#[test]
fn entitlement_entry_projects_snapshots_and_reports_version_changes() {
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let store = KeyringNativeCredentialStore::new();
    store.clear_session().unwrap();
    store.save_credentials(&credentials()).unwrap();
    let tracker = EntitlementSnapshotTracker::new();

    let (base_url, server) = spawn_server(session_body(7));
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    let first = entitlement_snapshot(&tracker).unwrap();
    assert_eq!(first.snapshot.snapshot_version, 7);
    assert_eq!(first.changed_snapshot_version, None);
    let request = server.join().unwrap().to_ascii_lowercase();
    assert!(request.starts_with("get /v1/auth/native/session http/1.1\r\n"));
    assert!(request.contains("authorization: bearer access-secret\r\n"));

    let (base_url, server) = spawn_server(session_body(8));
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    let second = entitlement_snapshot(&tracker).unwrap();
    assert_eq!(second.snapshot.snapshot_version, 8);
    assert_eq!(second.changed_snapshot_version, Some(8));
    server.join().unwrap();

    store.clear_session().unwrap();
    assert_eq!(
        entitlement_snapshot(&tracker).unwrap_err(),
        EntitlementSnapshotError::Missing
    );

    store.save_credentials(&credentials()).unwrap();
    let (base_url, server) = spawn_server(session_body(9));
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    let after_missing = entitlement_snapshot(&tracker).unwrap();
    assert_eq!(after_missing.changed_snapshot_version, None);
    server.join().unwrap();

    store.clear_session().unwrap();
    std::env::remove_var("MUNIMENT_API_BASE_URL");
}
