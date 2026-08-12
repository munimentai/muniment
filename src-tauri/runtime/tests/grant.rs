use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

use muniment_core::chat_grant::FetchGrantError;
use muniment_runtime::fetch_chat_grant;

fn spawn_server(body: &'static str) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_request(&mut stream);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
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
fn grant_entry_fetches_and_validates_the_response() {
    let valid_grant = r#"{"workspace":"/work","gatewayUrl":"https://gateway.example.com","virtualKey":"key","minimumCacheablePrefixCharacters":8192,"receiptUrl":"https://receipts.example.com"}"#;
    let (base_url, server) = spawn_server(valid_grant);
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);

    let grant = fetch_chat_grant("access-secret").unwrap();
    assert_eq!(grant.workspace, "/work");
    assert_eq!(grant.gateway_url, "https://gateway.example.com");
    let request = server.join().unwrap().to_ascii_lowercase();
    assert!(request.starts_with("post /v1/desktop/chat/config http/1.1\r\n"));
    assert!(request.contains("authorization: bearer access-secret\r\n"));

    let invalid_grant = r#"{"workspace":"/work","gatewayUrl":"http://gateway.example.com","virtualKey":"key","minimumCacheablePrefixCharacters":8192,"receiptUrl":"https://receipts.example.com"}"#;
    let (base_url, server) = spawn_server(invalid_grant);
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    assert_eq!(
        fetch_chat_grant("access-secret").unwrap_err(),
        FetchGrantError::InvalidResponse
    );
    server.join().unwrap();

    std::env::remove_var("MUNIMENT_API_BASE_URL");
}
