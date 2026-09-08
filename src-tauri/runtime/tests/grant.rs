use std::time::{SystemTime, UNIX_EPOCH};

use muniment_core::chat_grant::FetchGrantError;
use muniment_runtime::{configure_run, ConfigureRunError};

mod common;
use common::{credentials, grant_body, spawn_server};

#[test]
fn configure_run_checks_the_requested_workspace() {
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let credentials = credentials();
    common::save_credentials(&credentials);
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
        common::clear_credentials();
    });
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    assert_eq!(
        configure_run(access_token, None).unwrap_err(),
        ConfigureRunError::Grant(FetchGrantError::Unauthorized)
    );
    server.join().unwrap();
    near_expiry_session_refreshes_before_grant_issuance();
    common::clear_credentials();
    std::env::remove_var("MUNIMENT_API_BASE_URL");
}

fn near_expiry_session_refreshes_before_grant_issuance() {
    for seconds in [80, 0] {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut credentials = common::credentials_with_expiry(now + seconds);
        credentials.installation.device_challenge =
            "AwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwM".into();
        common::save_credentials(&credentials);
        let device_id = credentials.installation.device_id;
        let response = format!(
            r#"{{
                "access_token": "refreshed-access", "token_type": "Bearer", "expires_in": 900,
                "refresh_token": "refreshed-refresh", "refresh_expires_in": 86400,
                "session": {{"org_id": "20000000-0000-4000-8000-000000000002",
                    "user_id": "30000000-0000-4000-8000-000000000003", "role": "user",
                    "device_id": "{device_id}", "client_role": "desktop"}},
                "entitlement_snapshot": {{"payload": {{}}, "signature": "signature", "algorithm": "hmac-sha256"}},
                "device_challenge": "BQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQU"
            }}"#
        );
        let (base, server) =
            common::spawn_server_sequence(vec![(200, response), (201, grant_body())]);
        std::env::set_var("MUNIMENT_API_BASE_URL", base);
        let grant = configure_run(&credentials.tokens.access_token, None).unwrap();
        assert!(!grant.needs_renewal());
        assert_eq!(
            grant.native_access_token.as_deref(),
            Some("refreshed-access")
        );
        assert_eq!(
            common::load_credentials().tokens.access_token,
            "refreshed-access"
        );
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].starts_with("POST /v1/auth/native/token HTTP/1.1\r\n"));
        let refresh = requests[0].split_once("\r\n\r\n").unwrap().1;
        let proof = refresh
            .strip_prefix(&format!(
                r#"{{"grant_type":"refresh_token","refresh_token":"refresh-secret","client_id":"muniment-desktop","device_id":"{device_id}","device_proof":{{"challenge":"{}","issued_at":""#,
                credentials.installation.device_challenge,
            ))
            .unwrap()
            .strip_suffix(r#""}}"#)
            .unwrap();
        let (issued_at, proof) = proof.split_once(r#"","jti":""#).unwrap();
        let (jti, signature) = proof.split_once(r#"","signature":""#).unwrap();
        for value in [issued_at, jti, signature] {
            assert!(!value.is_empty());
            assert!(value
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && byte != b'"' && byte != b'\\'));
        }
        assert!(requests[1].starts_with("POST /v1/chat/grants HTTP/1.1\r\n"));
        assert!(requests[1].contains("Authorization: Bearer refreshed-access\r\n"));
        let body = requests[1].split_once("\r\n\r\n").unwrap().1;
        assert_eq!(body, r#"{"protocol":"muniment.desktop-access/1"}"#);
    }
}
