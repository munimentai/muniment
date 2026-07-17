#![cfg(target_os = "linux")]

use muniment_core::browser_control::{
    AuthorizationError, BrowserControlAcceptError, BrowserControlBindError,
    BrowserControlConnectionError, BrowserControlListener, BrowserControlProcessAuthorizer,
    WebSocketHandshakeLimits,
};
use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

const VALID_REQUEST: &str = "GET /control HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n";

fn handshake_error(
    request: &str,
    limits: WebSocketHandshakeLimits,
) -> BrowserControlConnectionError {
    let listener = BrowserControlListener::bind("127.0.0.1:0", "/browser").unwrap();
    let injected = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut client = TcpStream::connect(injected.local_addr().unwrap()).unwrap();
    client.write_all(request.as_bytes()).unwrap();
    client.shutdown(std::net::Shutdown::Write).unwrap();
    let authorizer = RecordingAuthorizer {
        calls: RefCell::new(Vec::new()),
        result: Ok(()),
    };
    listener
        .accept_websocket_with(&injected, &authorizer, "/control", limits)
        .unwrap_err()
}

struct RecordingAuthorizer {
    calls: RefCell<Vec<(SocketAddr, SocketAddr, PathBuf)>>,
    result: Result<(), AuthorizationError>,
}

impl BrowserControlProcessAuthorizer for RecordingAuthorizer {
    fn authorize(
        &self,
        local: SocketAddr,
        peer: SocketAddr,
        expected_executable: &Path,
    ) -> Result<(), AuthorizationError> {
        self.calls
            .borrow_mut()
            .push((local, peer, expected_executable.to_owned()));
        self.result
    }
}

#[test]
fn accepts_a_real_loopback_stream_only_after_authorization() {
    let expected = PathBuf::from("/desktop/selected-browser");
    let listener = BrowserControlListener::bind("127.0.0.1:0", &expected).unwrap();
    let injected = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut client = TcpStream::connect(injected.local_addr().unwrap()).unwrap();
    let authorizer = RecordingAuthorizer {
        calls: RefCell::new(Vec::new()),
        result: Ok(()),
    };

    client.write_all(b"GET /control HTTP/1.1\r\nconnection: keep-alive, Upgrade\r\nSec-WebSocket-Version: 13\r\nHOST: localhost\r\nsec-websocket-key: dGhlIHNhbXBsZSBub25jZQ==\r\nUpGrAdE: WebSocket\r\n\r\n").unwrap();
    let accepted = listener
        .accept_websocket_with(
            &injected,
            &authorizer,
            "/control",
            WebSocketHandshakeLimits::default(),
        )
        .unwrap();
    let calls = authorizer.calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, client.peer_addr().unwrap());
    assert_eq!(calls[0].1, client.local_addr().unwrap());
    assert_eq!(calls[0].2, expected);
    assert_eq!(accepted.local_addr().unwrap(), calls[0].0);
    drop(accepted);
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));
    assert!(response.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n"));
}

#[test]
fn drops_an_unauthorized_accepted_peer() {
    let listener = BrowserControlListener::bind("127.0.0.1:0", "/secret/browser").unwrap();
    let injected = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let client = TcpStream::connect(injected.local_addr().unwrap()).unwrap();
    let authorizer = RecordingAuthorizer {
        calls: RefCell::new(Vec::new()),
        result: Err(AuthorizationError::ExecutableVerificationFailed),
    };

    assert!(matches!(
        listener.accept_websocket_with(
            &injected,
            &authorizer,
            "/control",
            WebSocketHandshakeLimits::default(),
        ),
        Err(BrowserControlConnectionError::Accept(
            BrowserControlAcceptError::Unauthorized
        ))
    ));
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(1)))
        .unwrap();
    assert_eq!(client.peek(&mut [0]).unwrap(), 0);
}

#[test]
fn rejects_invalid_bind_configuration_before_listening() {
    for address in [
        "0.0.0.0:0",
        "[::]:0",
        "localhost:0",
        "192.0.2.1:0",
        "127.0.0.1:12345",
        "127.0.0.1",
        "127.0.0.1:not-a-port",
    ] {
        assert_eq!(
            BrowserControlListener::bind(address, "/browser").err(),
            Some(BrowserControlBindError::InvalidConfiguration),
            "{address}"
        );
    }
}

#[test]
fn public_errors_are_bounded_and_redacted() {
    let secrets = [
        "127.0.0.1:42424",
        "987654321",
        "/secret/selected-browser",
        "pairing-token",
    ];
    for rendered in [
        format!(
            "{:?}: {}",
            BrowserControlBindError::InvalidConfiguration,
            BrowserControlBindError::InvalidConfiguration
        ),
        format!(
            "{:?}: {}",
            BrowserControlAcceptError::Unauthorized,
            BrowserControlAcceptError::Unauthorized
        ),
        format!(
            "{:?}: {}",
            BrowserControlConnectionError::InvalidConfiguration,
            BrowserControlConnectionError::InvalidConfiguration
        ),
    ] {
        assert!(rendered.len() < 100);
        for secret in secrets {
            assert!(!rendered.contains(secret));
        }
    }
}

#[test]
fn rejects_each_configured_handshake_limit() {
    let defaults = WebSocketHandshakeLimits::default();
    assert!(matches!(
        handshake_error(
            VALID_REQUEST,
            WebSocketHandshakeLimits {
                max_header_bytes: VALID_REQUEST.len() - 1,
                ..defaults
            }
        ),
        BrowserControlConnectionError::Handshake(
            muniment_core::browser_control::WebSocketHandshakeError::TooLarge
        )
    ));
    assert!(matches!(
        handshake_error(
            VALID_REQUEST,
            WebSocketHandshakeLimits {
                max_headers: 4,
                ..defaults
            }
        ),
        BrowserControlConnectionError::Handshake(
            muniment_core::browser_control::WebSocketHandshakeError::TooManyHeaders
        )
    ));
    assert!(matches!(
        handshake_error(
            VALID_REQUEST,
            WebSocketHandshakeLimits {
                max_request_line_bytes: 10,
                ..defaults
            }
        ),
        BrowserControlConnectionError::Handshake(
            muniment_core::browser_control::WebSocketHandshakeError::RequestLineTooLong
        )
    ));

    let listener = BrowserControlListener::bind("127.0.0.1:0", "/browser").unwrap();
    let injected = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let _client = TcpStream::connect(injected.local_addr().unwrap()).unwrap();
    let authorizer = RecordingAuthorizer {
        calls: RefCell::new(Vec::new()),
        result: Ok(()),
    };
    assert!(matches!(
        listener.accept_websocket_with(
            &injected,
            &authorizer,
            "/control",
            WebSocketHandshakeLimits {
                read_timeout: Duration::from_millis(10),
                ..defaults
            },
        ),
        Err(BrowserControlConnectionError::Handshake(
            muniment_core::browser_control::WebSocketHandshakeError::Timeout
        ))
    ));
}

#[test]
fn rejects_malformed_duplicate_wrong_target_and_unsupported_version() {
    for request in [
        VALID_REQUEST.replace(
            "Host: localhost\r\n",
            "Host: localhost\r\nHost: attacker\r\n",
        ),
        VALID_REQUEST.replace(
            "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==",
            "Sec-WebSocket-Key: short",
        ),
        VALID_REQUEST.replace("GET /control", "GET /wrong"),
        VALID_REQUEST.replace("Version: 13", "Version: 12"),
        VALID_REQUEST.replace("\r\n\r\n", "\r\nBroken\r\n\r\n"),
        VALID_REQUEST.replace("Host: localhost", "Host: local\0host"),
    ] {
        assert!(matches!(
            handshake_error(&request, WebSocketHandshakeLimits::default()),
            BrowserControlConnectionError::Handshake(_)
        ));
    }
    assert!(matches!(
        handshake_error(
            VALID_REQUEST.trim_end_matches("\r\n"),
            WebSocketHandshakeLimits::default()
        ),
        BrowserControlConnectionError::Handshake(
            muniment_core::browser_control::WebSocketHandshakeError::Incomplete
        )
    ));
}

#[test]
fn fragmented_request_bytes_are_accepted() {
    let listener = BrowserControlListener::bind("127.0.0.1:0", "/browser").unwrap();
    let injected = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut client = TcpStream::connect(injected.local_addr().unwrap()).unwrap();
    for byte in VALID_REQUEST.bytes() {
        client.write_all(&[byte]).unwrap();
    }
    let authorizer = RecordingAuthorizer {
        calls: RefCell::new(Vec::new()),
        result: Ok(()),
    };
    assert!(listener
        .accept_websocket_with(
            &injected,
            &authorizer,
            "/control",
            WebSocketHandshakeLimits::default()
        )
        .is_ok());
}
