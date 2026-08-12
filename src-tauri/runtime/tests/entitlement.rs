use muniment_core::auth::{
    EntitlementSnapshotTracker, KeyringNativeCredentialStore, NativeCredentialStore,
};
use muniment_runtime::{entitlement_snapshot, EntitlementSnapshotError};

mod common;
use common::{credentials, spawn_server};

const DEVICE_ID: &str = "10000000-0000-4000-8000-000000000001";

fn session_body(version: u64) -> String {
    format!(
        r#"{{"session":{{"org_id":"20000000-0000-4000-8000-000000000002","user_id":"30000000-0000-4000-8000-000000000003","role":"owner","device_id":"{DEVICE_ID}","client_role":"desktop"}},"entitlement_snapshot":{{"payload":{{"version":{version},"user_display_name":"User","organization_display_name":"Muniment","groups":[]}},"signature":"signature-secret","algorithm":"hmac-sha256"}}}}"#
    )
}

#[test]
fn entitlement_entry_projects_snapshots_and_reports_version_changes() {
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let store = KeyringNativeCredentialStore::new();
    store.clear_session().unwrap();
    store.save_credentials(&credentials()).unwrap();
    let tracker = EntitlementSnapshotTracker::new();

    let (base_url, server) = spawn_server(200, session_body(7));
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    let first = entitlement_snapshot(&tracker).unwrap();
    assert_eq!(first.snapshot.snapshot_version, 7);
    assert_eq!(first.changed_snapshot_version, None);
    let request = server.join().unwrap().to_ascii_lowercase();
    assert!(request.starts_with("get /v1/auth/native/session http/1.1\r\n"));
    assert!(request.contains("authorization: bearer access-secret\r\n"));

    let (base_url, server) = spawn_server(200, session_body(8));
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
    let (base_url, server) = spawn_server(200, session_body(9));
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    let after_missing = entitlement_snapshot(&tracker).unwrap();
    assert_eq!(after_missing.changed_snapshot_version, None);
    server.join().unwrap();

    store.clear_session().unwrap();
    std::env::remove_var("MUNIMENT_API_BASE_URL");
}
