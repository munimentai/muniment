use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use muniment_core::auth::{
    FreshNativeSessionError, InstallationRecord, KeyringNativeCredentialStore,
    NativeCredentialStore, NativeCredentials, TokenSet,
};
use muniment_runtime::ensure_native_session;

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

fn session_body() -> String {
    format!(
        r#"{{"session":{{"org_id":"20000000-0000-4000-8000-000000000002","user_id":"30000000-0000-4000-8000-000000000003","role":"owner","device_id":"{DEVICE_ID}","client_role":"desktop"}},"entitlement_snapshot":{{"payload":{{"version":7,"user_display_name":"User","organization_display_name":"Muniment","groups":[]}},"signature":"signature-secret","algorithm":"hmac-sha256"}}}}"#
    )
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
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8(bytes).unwrap()
}

#[test]
fn session_entry_reads_the_keyring_for_success_and_failure() {
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let store = KeyringNativeCredentialStore::new();
    store.clear_session().unwrap();
    store.save_credentials(&credentials()).unwrap();
    assert!(store.load_credentials().unwrap().is_some());

    let (base_url, server) = spawn_server(200, session_body());
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    let session = ensure_native_session().unwrap();
    assert!(session.status.signed_in);
    assert_eq!(
        session.status.subject.as_deref(),
        Some("30000000-0000-4000-8000-000000000003")
    );
    let request = server.join().unwrap().to_ascii_lowercase();
    assert!(request.starts_with("get /v1/auth/native/session http/1.1\r\n"));
    assert!(request.contains("authorization: bearer access-secret\r\n"));

    let (base_url, server) = spawn_server(401, "denied".into());
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    assert_eq!(
        ensure_native_session().unwrap_err(),
        FreshNativeSessionError::SessionInspection
    );
    server.join().unwrap();

    store.clear_session().unwrap();
    std::env::remove_var("MUNIMENT_API_BASE_URL");
}
