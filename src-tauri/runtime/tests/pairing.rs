use std::sync::Mutex;

use muniment_core::auth::PairingError;
use muniment_runtime::{pairing_status, request_pairing_challenge, revoke_pairing_pair};
use uuid::Uuid;

mod common;
use common::{spawn_server, TemporaryProfile};

static TEST_LOCK: Mutex<()> = Mutex::new(());

const CHALLENGE_BODY: &str = r#"{"contract_version":"muniment.remote-control-pairing/1","expires_at":"2026-01-01T00:02:00.000Z","qr":{"contract_version":"muniment.remote-control-pairing/1","desktop_device_id":"11111111-1111-4111-8111-111111111111","desktop_public_key":"ERERERERERERERERERERERERERERERERERERERERERE","challenge":"IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiI"}}"#;
const PAIR_BODY: &str = r#"{"contract_version":"muniment.remote-control-pairing/1","pair":{"pair_id":"22222222-2222-4222-8222-222222222222","desktop_device_id":"11111111-1111-4111-8111-111111111111","mobile_device_id":"33333333-3333-4333-8333-333333333333","desktop_public_key":"ERERERERERERERERERERERERERERERERERERERERERE","mobile_public_key":"MzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzM","created_at":"2026-01-01T00:01:00.000Z"}}"#;
const EMPTY_PAIR_BODY: &str =
    r#"{"contract_version":"muniment.remote-control-pairing/1","pair":null}"#;
const REVOKE_BODY: &str =
    r#"{"contract_version":"muniment.remote-control-pairing/1","revoked":true}"#;

#[test]
fn request_pairing_challenge_sends_the_caller_token() {
    let _guard = TEST_LOCK.lock().unwrap();
    let (base_url, server) = spawn_server(201, CHALLENGE_BODY.into());
    std::env::set_var("MUNIMENT_API_BASE_URL", &base_url);

    let result = request_pairing_challenge("caller-access-token").unwrap();
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
    let profile = TemporaryProfile::new("pairing-status", false);
    let (base_url, pair_server) = spawn_server(200, PAIR_BODY.into());
    std::env::set_var("MUNIMENT_API_BASE_URL", &base_url);
    let paired = pairing_status("caller-access-token", &profile.profile).unwrap();
    assert_eq!(
        paired.pair.unwrap().mobile_device_id.to_string(),
        "33333333-3333-4333-8333-333333333333"
    );
    pair_server.join().unwrap();

    let (base_url, empty_server) = spawn_server(200, EMPTY_PAIR_BODY.into());
    std::env::set_var("MUNIMENT_API_BASE_URL", &base_url);
    let cleared = pairing_status("caller-access-token", &profile.profile).unwrap();
    assert!(cleared.pair.is_none());
    empty_server.join().unwrap();
    std::env::remove_var("MUNIMENT_API_BASE_URL");
}

#[test]
fn revoke_pairing_sends_the_pair_id() {
    let _guard = TEST_LOCK.lock().unwrap();
    let profile = TemporaryProfile::new("pairing-revoke", false);
    let (base_url, seed) = spawn_server(200, PAIR_BODY.into());
    std::env::set_var("MUNIMENT_API_BASE_URL", &base_url);
    pairing_status("caller-access-token", &profile.profile).unwrap();
    seed.join().unwrap();

    let (base_url, server) = spawn_server(200, REVOKE_BODY.into());
    std::env::set_var("MUNIMENT_API_BASE_URL", &base_url);
    let pair_id = Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap();
    let result = revoke_pairing_pair("caller-access-token", pair_id, &profile.profile).unwrap();
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
        request_pairing_challenge("caller-access-token").unwrap_err(),
        PairingError::HttpStatus(429)
    );
    limited.join().unwrap();

    let (base_url, conflict) = spawn_server(
        409,
        r#"{"error":{"code":"pairing_conflict","message":"The pairing request failed."}}"#.into(),
    );
    std::env::set_var("MUNIMENT_API_BASE_URL", &base_url);
    assert_eq!(
        request_pairing_challenge("caller-access-token").unwrap_err(),
        PairingError::HttpStatus(409)
    );
    conflict.join().unwrap();
    std::env::remove_var("MUNIMENT_API_BASE_URL");
}
