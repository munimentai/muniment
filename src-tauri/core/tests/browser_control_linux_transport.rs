#![cfg(target_os = "linux")]

use muniment_core::browser_control::{
    BrowserControlAcceptError, BrowserControlBindError, BrowserControlConnectionError,
    BrowserControlListener, WebSocketHandshakeError,
};

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
fn every_public_error_variant_is_bounded_and_redacted() {
    let bind = [
        BrowserControlBindError::InvalidConfiguration,
        BrowserControlBindError::Bind,
    ];
    let accept = [
        BrowserControlAcceptError::Accept,
        BrowserControlAcceptError::EndpointUnavailable,
        BrowserControlAcceptError::Unauthorized,
    ];
    let handshake = [
        WebSocketHandshakeError::Timeout,
        WebSocketHandshakeError::Incomplete,
        WebSocketHandshakeError::TooLarge,
        WebSocketHandshakeError::TooManyHeaders,
        WebSocketHandshakeError::RequestLineTooLong,
        WebSocketHandshakeError::Malformed,
        WebSocketHandshakeError::DuplicateHeader,
        WebSocketHandshakeError::UnsupportedRequest,
        WebSocketHandshakeError::Io,
    ];
    let mut rendered: Vec<String> = bind
        .iter()
        .map(|error| format!("{error:?}: {error}"))
        .chain(accept.iter().map(|error| format!("{error:?}: {error}")))
        .collect();
    rendered.push(format!(
        "{:?}: {}",
        BrowserControlConnectionError::InvalidConfiguration,
        BrowserControlConnectionError::InvalidConfiguration
    ));
    for error in accept {
        let error = BrowserControlConnectionError::Accept(error);
        rendered.push(format!("{error:?}: {error}"));
    }
    for error in handshake {
        rendered.push(format!("{error:?}: {error}"));
        let error = BrowserControlConnectionError::Handshake(error);
        rendered.push(format!("{error:?}: {error}"));
    }

    for value in rendered {
        assert!(value.len() < 100);
        for secret in [
            "GET /secret?pairing-token HTTP/1.1",
            "Sec-WebSocket-Key",
            "dGhlIHNhbXBsZSBub25jZQ==",
            "/secret/selected-browser",
        ] {
            assert!(!value.contains(secret));
        }
    }
}
