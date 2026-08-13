use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

use muniment_core::attach::{evaluate_quiesce, QuiesceError, RuntimeActivityRegistry};
use muniment_core::auth::{
    BrowserOpenError, EntitlementSnapshotTracker, KeyringNativeCredentialStore,
    NativeCredentialStore,
};
use muniment_runtime::sign_in;

mod common;
use common::read_request;

static TEST_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone)]
struct AuthorizationAttempt {
    redirect_uri: String,
    state: String,
}

fn json_string(body: &str, field: &str) -> String {
    let prefix = format!(r#""{field}":""#);
    let value = body.split_once(&prefix).unwrap().1;
    value.split_once('"').unwrap().0.to_string()
}

fn response_for(request: &str, attempt: &Arc<Mutex<Option<AuthorizationAttempt>>>) -> String {
    let body = request.split_once("\r\n\r\n").unwrap().1;
    if request.starts_with("POST /v1/auth/native/devices ") {
        return r#"{"device_id":"10000000-0000-4000-8000-000000000001","registration_token":"AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI","device_challenge":"AwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwM","expires_in":600}"#.into();
    }
    if request.starts_with("POST /v1/auth/native/authorize ") {
        *attempt.lock().unwrap() = Some(AuthorizationAttempt {
            redirect_uri: json_string(body, "redirect_uri"),
            state: json_string(body, "state"),
        });
        return r#"{"authorization_url":"https://login.muniment.test/continue","device_challenge":"BAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQ"}"#.into();
    }
    assert!(request.starts_with("POST /v1/auth/native/token "));
    r#"{"access_token":"access-secret","token_type":"Bearer","expires_in":900,"refresh_token":"refresh-secret","refresh_expires_in":86400,"session":{"org_id":"20000000-0000-4000-8000-000000000002","user_id":"30000000-0000-4000-8000-000000000003","role":"user","device_id":"10000000-0000-4000-8000-000000000001","client_role":"desktop"},"entitlement_snapshot":{"payload":{"version":1},"signature":"snapshot-secret","algorithm":"hmac-sha256"},"device_challenge":"BQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQU"}"#.into()
}

fn spawn_sign_in_server(
    activity: RuntimeActivityRegistry,
    attempt: Arc<Mutex<Option<AuthorizationAttempt>>>,
) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let mut requests = Vec::new();
        for _ in 0..3 {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request_with_body(&mut stream);
            assert_eq!(
                evaluate_quiesce(activity.snapshot()),
                Err(QuiesceError::AuthenticationOperation)
            );
            let body = response_for(&request, &attempt);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            requests.push(request);
        }
        requests
    });
    (base_url, server)
}

fn read_request_with_body(stream: &mut TcpStream) -> String {
    let headers = read_request(stream);
    let content_length = headers
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length: ")
                .and_then(|value| value.parse::<usize>().ok())
        })
        .unwrap_or(0);
    let body_start = headers.split_once("\r\n\r\n").unwrap().1.len();
    let mut request = headers;
    let remaining = content_length.saturating_sub(body_start);
    if remaining != 0 {
        let mut body = vec![0; remaining];
        stream.read_exact(&mut body).unwrap();
        request.push_str(std::str::from_utf8(&body).unwrap());
    }
    request
}

fn send_callback(attempt: AuthorizationAttempt) {
    let authority = attempt
        .redirect_uri
        .strip_prefix("http://")
        .unwrap()
        .split_once('/')
        .unwrap();
    let mut stream = TcpStream::connect(authority.0).unwrap();
    write!(
        stream,
        "GET /{}?code=CQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQk&state={} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        authority.1, attempt.state, authority.0
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"));
}

#[test]
fn sign_in_returns_status_stores_credentials_and_marks_activity() {
    let _guard = TEST_LOCK.lock().unwrap();
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let tracker = EntitlementSnapshotTracker::new();
    tracker.observe(Some(7));
    let activity = RuntimeActivityRegistry::new();
    let attempt = Arc::new(Mutex::new(None));
    let (base_url, server) = spawn_sign_in_server(activity.clone(), attempt.clone());
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    let browser = move |url: &str| -> Result<(), BrowserOpenError> {
        assert_eq!(url, "https://login.muniment.test/continue");
        let attempt = attempt.lock().unwrap().clone().unwrap();
        thread::spawn(move || send_callback(attempt));
        Ok(())
    };

    let status = sign_in(&browser, &tracker, &activity).unwrap();

    assert!(status.signed_in);
    assert_eq!(
        status.subject.as_deref(),
        Some("30000000-0000-4000-8000-000000000003")
    );
    assert!(status.expires_at.is_some());
    let credentials = KeyringNativeCredentialStore::new()
        .load_credentials()
        .unwrap()
        .unwrap();
    assert_eq!(credentials.tokens.access_token, "access-secret");
    assert_eq!(
        credentials.tokens.refresh_token.as_deref(),
        Some("refresh-secret")
    );
    assert_eq!(status.expires_at, credentials.tokens.expires_at);
    assert_eq!(tracker.observe(Some(8)), None);
    assert!(evaluate_quiesce(activity.snapshot()).is_ok());
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 3);
    std::env::remove_var("MUNIMENT_API_BASE_URL");
}
