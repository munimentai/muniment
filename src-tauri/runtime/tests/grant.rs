use muniment_core::auth::{KeyringNativeCredentialStore, NativeCredentialStore};
use muniment_core::chat_grant::FetchGrantError;
use muniment_runtime::{configure_run, ConfigureRunError};

mod common;
use common::{credentials, grant_body, spawn_server};

#[test]
fn configure_run_checks_the_requested_workspace() {
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let store = KeyringNativeCredentialStore::new();
    let credentials = credentials();
    store.save_credentials(&credentials).unwrap();
    let access_token = &credentials.tokens.access_token;
    let valid_grant = grant_body();
    let (base_url, server) = spawn_server(201, valid_grant.clone());
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);

    let grant = configure_run(access_token, Some("local")).unwrap();
    assert_eq!(grant.workspace, "local");
    assert_eq!(grant.gateway_url, "https://gateway.example.com");
    assert_eq!(grant.model.as_deref(), Some("muniment-stub-chat"));
    let request = server.join().unwrap().to_ascii_lowercase();
    assert!(request.starts_with("post /v1/chat/grants http/1.1\r\n"));
    assert!(request.contains(&format!("authorization: bearer {access_token}\r\n")));

    let (base_url, server) = spawn_server(201, valid_grant.clone());
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    assert_eq!(
        configure_run(access_token, Some("/other")).unwrap_err(),
        ConfigureRunError::Unauthorized
    );
    server.join().unwrap();

    let (base_url, server) = spawn_server(201, valid_grant.clone());
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    assert_eq!(
        configure_run(access_token, None).unwrap().workspace,
        "local"
    );
    server.join().unwrap();

    for invalid_grant in [
        valid_grant.replace("https://gateway.example.com", "http://gateway.example.com"),
        valid_grant.replace(
            &credentials.installation.device_id.to_string(),
            "another-device",
        ),
    ] {
        let (base_url, server) = spawn_server(201, invalid_grant);
        std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
        assert_eq!(
            configure_run(access_token, Some("local")).unwrap_err(),
            ConfigureRunError::Grant(FetchGrantError::InvalidResponse)
        );
        server.join().unwrap();
    }
    assert_eq!(
        configure_run("stale-access-token", None).unwrap_err(),
        ConfigureRunError::Grant(FetchGrantError::Unauthorized)
    );
    let (base_url, server) = common::spawn_server_with(201, valid_grant, || {
        KeyringNativeCredentialStore::new().clear_session().unwrap();
    });
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    assert_eq!(
        configure_run(access_token, None).unwrap_err(),
        ConfigureRunError::Grant(FetchGrantError::Unauthorized)
    );
    server.join().unwrap();
    store.clear_session().unwrap();
    std::env::remove_var("MUNIMENT_API_BASE_URL");
}
