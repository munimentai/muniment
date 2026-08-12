use muniment_core::chat_grant::FetchGrantError;
use muniment_runtime::fetch_chat_grant;

mod common;
use common::spawn_server;

#[test]
fn grant_entry_fetches_and_validates_the_response() {
    let valid_grant = r#"{"workspace":"/work","gatewayUrl":"https://gateway.example.com","virtualKey":"key","minimumCacheablePrefixCharacters":8192,"receiptUrl":"https://receipts.example.com"}"#;
    let (base_url, server) = spawn_server(200, valid_grant.into());
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);

    let grant = fetch_chat_grant("access-secret").unwrap();
    assert_eq!(grant.workspace, "/work");
    assert_eq!(grant.gateway_url, "https://gateway.example.com");
    let request = server.join().unwrap().to_ascii_lowercase();
    assert!(request.starts_with("post /v1/desktop/chat/config http/1.1\r\n"));
    assert!(request.contains("authorization: bearer access-secret\r\n"));

    let invalid_grant = r#"{"workspace":"/work","gatewayUrl":"http://gateway.example.com","virtualKey":"key","minimumCacheablePrefixCharacters":8192,"receiptUrl":"https://receipts.example.com"}"#;
    let (base_url, server) = spawn_server(200, invalid_grant.into());
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    assert_eq!(
        fetch_chat_grant("access-secret").unwrap_err(),
        FetchGrantError::InvalidResponse
    );
    server.join().unwrap();

    std::env::remove_var("MUNIMENT_API_BASE_URL");
}
