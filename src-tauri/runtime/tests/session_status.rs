mod common;

use std::net::TcpListener;
use std::time::{SystemTime, UNIX_EPOCH};

use common::credentials_with_expiry;
use muniment_core::auth::{KeyringNativeCredentialStore, NativeCredentialStore};
use muniment_runtime::session_status;

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[test]
fn session_status_reads_the_keyring_without_a_server() {
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let store = KeyringNativeCredentialStore::new();
    store.clear_session().unwrap();
    let expires_at = unix_time() + 3_600;
    store
        .save_credentials(&credentials_with_expiry(expires_at))
        .unwrap();

    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let closed_address = listener.local_addr().unwrap();
    drop(listener);
    std::env::set_var("MUNIMENT_API_BASE_URL", format!("http://{closed_address}"));

    let status = session_status().unwrap();
    assert!(status.signed_in);
    assert_eq!(status.subject.as_deref(), Some("user"));
    assert_eq!(status.expires_at, Some(expires_at));

    store.clear_session().unwrap();
    let status = session_status().unwrap();
    assert!(!status.signed_in);
    assert_eq!(status.subject, None);
    assert_eq!(status.expires_at, None);

    std::env::remove_var("MUNIMENT_API_BASE_URL");
}
