use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Mutex;
use std::thread;

use muniment_core::auth::{NativeDeviceListError, NativeDevicePlatform};
use muniment_runtime::list_devices;

static TEST_LOCK: Mutex<()> = Mutex::new(());

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

#[test]
fn list_devices_sends_the_caller_token_and_returns_rows() {
    let _guard = TEST_LOCK.lock().unwrap();
    let body = r#"{"devices":[{"device_id":"10000000-0000-4000-8000-000000000001","client_id":"muniment-desktop","client_role":"desktop","platform":"desktop","created_at":"2026-08-01T10:00:00Z","revoked_at":null,"last_active_at":"2026-08-12T12:00:00Z","current":true}]}"#;
    let (base_url, server) = spawn_server("200 OK", body);
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);

    let result = list_devices("caller-access-token").unwrap();

    assert_eq!(result.devices.len(), 1);
    let device = &result.devices[0];
    assert_eq!(device.client_id, "muniment-desktop");
    assert_eq!(device.platform, NativeDevicePlatform::Desktop);
    assert!(device.current);
    let request = server.join().unwrap().to_ascii_lowercase();
    assert!(request.starts_with("get /v1/auth/native/devices http/1.1\r\n"));
    assert!(request.contains("authorization: bearer caller-access-token\r\n"));
    std::env::remove_var("MUNIMENT_API_BASE_URL");
}

#[test]
fn list_devices_returns_an_error_for_a_rejected_response() {
    let _guard = TEST_LOCK.lock().unwrap();
    let (base_url, server) = spawn_server("401 Unauthorized", r#"{"error":"unauthorized"}"#);
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);

    let result = list_devices("rejected-access-token");

    assert_eq!(result.unwrap_err(), NativeDeviceListError::HttpStatus(401));
    server.join().unwrap();
    std::env::remove_var("MUNIMENT_API_BASE_URL");
}
