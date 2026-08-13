use muniment_core::attach::RuntimeActivityRegistry;
use muniment_core::auth::{
    FreshNativeSessionError, KeyringNativeCredentialStore, NativeCredentialStore,
};
use muniment_runtime::ensure_native_session;

mod common;
use common::{credentials, spawn_server, spawn_server_with};

const DEVICE_ID: &str = "10000000-0000-4000-8000-000000000001";

fn session_body() -> String {
    format!(
        r#"{{"session":{{"org_id":"20000000-0000-4000-8000-000000000002","user_id":"30000000-0000-4000-8000-000000000003","role":"owner","device_id":"{DEVICE_ID}","client_role":"desktop"}},"entitlement_snapshot":{{"payload":{{"version":7,"user_display_name":"User","organization_display_name":"Muniment","groups":[]}},"signature":"signature-secret","algorithm":"hmac-sha256"}}}}"#
    )
}

#[test]
fn session_entry_reads_the_keyring_for_success_and_failure() {
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let store = KeyringNativeCredentialStore::new();
    store.clear_session().unwrap();
    store.save_credentials(&credentials()).unwrap();
    assert!(store.load_credentials().unwrap().is_some());
    let runtime_activity = RuntimeActivityRegistry::new();

    let activity_during_request = runtime_activity.clone();
    let (base_url, server) = spawn_server_with(200, session_body(), move || {
        assert!(activity_during_request.snapshot().session_refresh);
    });
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    let session = ensure_native_session(&runtime_activity).unwrap();
    assert!(!runtime_activity.snapshot().session_refresh);
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
        ensure_native_session(&runtime_activity).unwrap_err(),
        FreshNativeSessionError::SessionInspection
    );
    assert!(!runtime_activity.snapshot().session_refresh);
    server.join().unwrap();

    store.clear_session().unwrap();
    std::env::remove_var("MUNIMENT_API_BASE_URL");
}
