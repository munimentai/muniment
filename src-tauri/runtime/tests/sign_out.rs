use std::sync::Mutex;

use muniment_core::auth::{
    EntitlementSnapshotTracker, KeyringNativeCredentialStore, NativeCredentialStore,
};
use muniment_runtime::sign_out;

mod common;
use common::{credentials, spawn_server};

static TEST_LOCK: Mutex<()> = Mutex::new(());

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
    let (base_url, server) = spawn_server(200, r#"{"ok":true}"#.into());
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
    let (base_url, server) = spawn_server(401, r#"{"ok":false}"#.into());
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);

    let status = sign_out(&tracker).unwrap();

    assert!(!status.signed_in);
    assert!(store.load_credentials().unwrap().is_none());
    server.join().unwrap();
    std::env::remove_var("MUNIMENT_API_BASE_URL");
}
