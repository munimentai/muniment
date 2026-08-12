use std::sync::Mutex;

use muniment_core::auth::{NativeDeviceListError, NativeDevicePlatform};
use muniment_runtime::list_devices;

mod common;
use common::spawn_server;

static TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn list_devices_sends_the_caller_token_and_returns_rows() {
    let _guard = TEST_LOCK.lock().unwrap();
    let body = r#"{"devices":[{"device_id":"10000000-0000-4000-8000-000000000001","client_id":"muniment-desktop","client_role":"desktop","platform":"desktop","created_at":"2026-08-01T10:00:00Z","revoked_at":null,"last_active_at":"2026-08-12T12:00:00Z","current":true}]}"#;
    let (base_url, server) = spawn_server(200, body.into());
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
    let (base_url, server) = spawn_server(401, r#"{"error":"unauthorized"}"#.into());
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);

    let result = list_devices("rejected-access-token");

    assert_eq!(result.unwrap_err(), NativeDeviceListError::HttpStatus(401));
    server.join().unwrap();
    std::env::remove_var("MUNIMENT_API_BASE_URL");
}
