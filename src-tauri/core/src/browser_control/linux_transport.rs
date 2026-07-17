//! Fail-closed Linux transport for browser-control connections.

use super::{authorize_browser_process, AuthorizationError};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::fmt;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sha1::{Digest, Sha1};

const WEBSOCKET_GUID: &[u8] = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Resource and time bounds for the opening handshake.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebSocketHandshakeLimits {
    pub read_timeout: Duration,
    pub max_header_bytes: usize,
    pub max_headers: usize,
    pub max_request_line_bytes: usize,
}

impl Default for WebSocketHandshakeLimits {
    fn default() -> Self {
        Self {
            read_timeout: Duration::from_secs(5),
            max_header_bytes: 16 * 1024,
            max_headers: 64,
            max_request_line_bytes: 2048,
        }
    }
}

/// A process-authorized stream after a successful RFC 6455 opening handshake.
#[derive(Debug)]
pub struct BrowserControlWebSocketStream(TcpStream);

impl BrowserControlWebSocketStream {
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0.local_addr()
    }
}

impl Read for BrowserControlWebSocketStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.0.read(buffer)
    }
}

impl Write for BrowserControlWebSocketStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.0.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

/// A stream released only after the Linux browser-process authorization boundary.
///
/// Its inner stream is intentionally opaque. A raw [`TcpStream`] cannot be
/// converted into this type by callers.
///
/// ```compile_fail
/// use muniment_core::browser_control::{ProcessAuthorizedBrowserControlStream, WebSocketHandshakeLimits};
/// use std::net::TcpStream;
///
/// let raw: TcpStream = unimplemented!();
/// let _ = ProcessAuthorizedBrowserControlStream(raw);
/// ```
///
/// ```compile_fail
/// use muniment_core::browser_control::BrowserControlProcessAuthorizer;
///
/// struct AllowAll;
/// impl BrowserControlProcessAuthorizer for AllowAll {}
/// ```
pub struct ProcessAuthorizedBrowserControlStream(TcpStream);

impl ProcessAuthorizedBrowserControlStream {
    /// Performs a bounded RFC 6455 opening handshake on this authorized stream.
    pub fn open_websocket(
        self,
        request_target: &str,
        limits: WebSocketHandshakeLimits,
    ) -> Result<BrowserControlWebSocketStream, BrowserControlConnectionError> {
        validate_handshake_configuration(request_target, limits)?;
        perform_websocket_handshake(self.0, request_target, limits)
            .map_err(BrowserControlConnectionError::Handshake)
    }
}

/// A browser-control listener bound to a numeric loopback address.
pub struct BrowserControlListener {
    listener: TcpListener,
    expected_executable: PathBuf,
}

impl BrowserControlListener {
    /// Binds an OS-assigned port. Hostnames and explicit ports are not accepted.
    pub fn bind(
        bind_address: &str,
        expected_executable: impl Into<PathBuf>,
    ) -> Result<Self, BrowserControlBindError> {
        let address: SocketAddr = bind_address
            .parse()
            .map_err(|_| BrowserControlBindError::InvalidConfiguration)?;
        if !address.ip().is_loopback() || address.port() != 0 {
            return Err(BrowserControlBindError::InvalidConfiguration);
        }

        let listener = TcpListener::bind(address).map_err(|_| BrowserControlBindError::Bind)?;
        let bound = listener
            .local_addr()
            .map_err(|_| BrowserControlBindError::Bind)?;
        if !bound.ip().is_loopback() || bound.port() == 0 {
            return Err(BrowserControlBindError::Bind);
        }
        Ok(Self {
            listener,
            expected_executable: expected_executable.into(),
        })
    }

    /// Returns the numeric loopback endpoint selected by the operating system.
    pub fn local_addr(&self) -> Result<SocketAddr, BrowserControlBindError> {
        self.listener
            .local_addr()
            .map_err(|_| BrowserControlBindError::Bind)
    }

    /// Accepts one stream and releases it only after browser-process authorization.
    pub fn accept_websocket(
        &self,
        request_target: &str,
        limits: WebSocketHandshakeLimits,
    ) -> Result<BrowserControlWebSocketStream, BrowserControlConnectionError> {
        validate_handshake_configuration(request_target, limits)?;
        self.accept()?.open_websocket(request_target, limits)
    }

    /// Accepts one stream and releases it only after browser-process authorization.
    pub fn accept(
        &self,
    ) -> Result<ProcessAuthorizedBrowserControlStream, BrowserControlAcceptError> {
        self.accept_with(&self.listener, &LinuxBrowserProcessAuthorizer)
    }

    fn accept_with(
        &self,
        listener: &impl BrowserControlStreamListener,
        authorizer: &impl BrowserControlProcessAuthorizer,
    ) -> Result<ProcessAuthorizedBrowserControlStream, BrowserControlAcceptError> {
        let stream = listener
            .accept_stream()
            .map_err(|_| BrowserControlAcceptError::Accept)?;
        let local = stream
            .local_addr()
            .map_err(|_| BrowserControlAcceptError::EndpointUnavailable)?;
        let peer = stream
            .peer_addr()
            .map_err(|_| BrowserControlAcceptError::EndpointUnavailable)?;
        authorizer
            .authorize(local, peer, &self.expected_executable)
            .map_err(|_| BrowserControlAcceptError::Unauthorized)?;
        Ok(ProcessAuthorizedBrowserControlStream(stream))
    }
}

fn validate_handshake_configuration(
    target: &str,
    limits: WebSocketHandshakeLimits,
) -> Result<(), BrowserControlConnectionError> {
    if !target.starts_with('/')
        || target.contains('#')
        || target
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b' ')
        || limits.read_timeout.is_zero()
        || limits.max_header_bytes < 4
        || limits.max_headers == 0
        || limits.max_request_line_bytes == 0
    {
        return Err(BrowserControlConnectionError::InvalidConfiguration);
    }
    Ok(())
}

fn perform_websocket_handshake(
    mut stream: TcpStream,
    target: &str,
    limits: WebSocketHandshakeLimits,
) -> Result<BrowserControlWebSocketStream, WebSocketHandshakeError> {
    let deadline = Instant::now()
        .checked_add(limits.read_timeout)
        .ok_or(WebSocketHandshakeError::Timeout)?;
    let mut request = Vec::new();
    let mut request_line_complete = false;
    loop {
        if request.len() == limits.max_header_bytes {
            return Err(WebSocketHandshakeError::TooLarge);
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or(WebSocketHandshakeError::Timeout)?;
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|_| WebSocketHandshakeError::Io)?;
        let mut byte = [0];
        match stream.read(&mut byte) {
            Ok(0) => return Err(WebSocketHandshakeError::Incomplete),
            Ok(_) => request.push(byte[0]),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                return Err(WebSocketHandshakeError::Timeout)
            }
            Err(_) => return Err(WebSocketHandshakeError::Io),
        }
        if !request_line_complete {
            if request.ends_with(b"\r\n") {
                request_line_complete = true;
            } else if request.len() > limits.max_request_line_bytes
                && !(request.len() == limits.max_request_line_bytes + 1
                    && request.last() == Some(&b'\r'))
            {
                return Err(WebSocketHandshakeError::RequestLineTooLong);
            }
        }
        if request.ends_with(b"\r\n\r\n") {
            break;
        }
    }

    let text = std::str::from_utf8(&request).map_err(|_| WebSocketHandshakeError::Malformed)?;
    let mut lines = text[..text.len() - 4].split("\r\n");
    let request_line = lines.next().ok_or(WebSocketHandshakeError::Malformed)?;
    if request_line.len() > limits.max_request_line_bytes {
        return Err(WebSocketHandshakeError::RequestLineTooLong);
    }
    let mut request_parts = request_line.split(' ');
    if request_parts.next() != Some("GET")
        || request_parts.next() != Some(target)
        || request_parts.next() != Some("HTTP/1.1")
        || request_parts.next().is_some()
    {
        return Err(WebSocketHandshakeError::Malformed);
    }

    let mut host = None;
    let mut upgrade = None;
    let mut connection = None;
    let mut key = None;
    let mut version = None;
    for (index, line) in lines.enumerate() {
        if index >= limits.max_headers {
            return Err(WebSocketHandshakeError::TooManyHeaders);
        }
        let (name, value) = line
            .split_once(':')
            .ok_or(WebSocketHandshakeError::Malformed)?;
        if name.is_empty()
            || name
                .bytes()
                .any(|b| !b.is_ascii_alphanumeric() && b != b'-')
        {
            return Err(WebSocketHandshakeError::Malformed);
        }
        let value = value.trim_matches([' ', '\t']);
        if value
            .bytes()
            .any(|byte| byte.is_ascii_control() && byte != b'\t')
        {
            return Err(WebSocketHandshakeError::Malformed);
        }
        let slot = if name.eq_ignore_ascii_case("host") {
            Some(&mut host)
        } else if name.eq_ignore_ascii_case("upgrade") {
            Some(&mut upgrade)
        } else if name.eq_ignore_ascii_case("connection") {
            Some(&mut connection)
        } else if name.eq_ignore_ascii_case("sec-websocket-key") {
            Some(&mut key)
        } else if name.eq_ignore_ascii_case("sec-websocket-version") {
            Some(&mut version)
        } else {
            None
        };
        if let Some(slot) = slot {
            if slot.replace(value).is_some() {
                return Err(WebSocketHandshakeError::DuplicateHeader);
            }
        }
    }

    if host.filter(|value| !value.is_empty()).is_none()
        || upgrade
            .filter(|value| value.eq_ignore_ascii_case("websocket"))
            .is_none()
        || connection
            .filter(|value| {
                value
                    .split(',')
                    .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
            })
            .is_none()
        || version != Some("13")
    {
        return Err(WebSocketHandshakeError::UnsupportedRequest);
    }
    let key = key.ok_or(WebSocketHandshakeError::UnsupportedRequest)?;
    let decoded = STANDARD
        .decode(key)
        .map_err(|_| WebSocketHandshakeError::Malformed)?;
    if decoded.len() != 16 {
        return Err(WebSocketHandshakeError::Malformed);
    }
    let accept = STANDARD.encode(
        Sha1::new()
            .chain_update(key.as_bytes())
            .chain_update(WEBSOCKET_GUID)
            .finalize(),
    );
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|_| WebSocketHandshakeError::Io)?;
    stream.flush().map_err(|_| WebSocketHandshakeError::Io)?;
    let _ = stream.set_read_timeout(None);
    Ok(BrowserControlWebSocketStream(stream))
}

trait BrowserControlStreamListener {
    fn accept_stream(&self) -> io::Result<TcpStream>;
}

impl BrowserControlStreamListener for TcpListener {
    fn accept_stream(&self) -> io::Result<TcpStream> {
        self.accept().map(|(stream, _)| stream)
    }
}

trait BrowserControlProcessAuthorizer {
    fn authorize(
        &self,
        local: SocketAddr,
        peer: SocketAddr,
        expected_executable: &Path,
    ) -> Result<(), AuthorizationError>;
}

#[derive(Clone, Copy, Debug, Default)]
struct LinuxBrowserProcessAuthorizer;

impl BrowserControlProcessAuthorizer for LinuxBrowserProcessAuthorizer {
    fn authorize(
        &self,
        local: SocketAddr,
        peer: SocketAddr,
        expected_executable: &Path,
    ) -> Result<(), AuthorizationError> {
        authorize_browser_process(local, peer, expected_executable).map(|_| ())
    }
}

/// A bounded, redacted listener-creation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrowserControlBindError {
    InvalidConfiguration,
    Bind,
}

impl fmt::Display for BrowserControlBindError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidConfiguration => "browser-control bind configuration is invalid",
            Self::Bind => "browser-control listener could not be opened",
        })
    }
}

impl std::error::Error for BrowserControlBindError {}

/// A bounded, redacted accepted-stream failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrowserControlAcceptError {
    Accept,
    EndpointUnavailable,
    Unauthorized,
}

impl fmt::Display for BrowserControlAcceptError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Accept => "browser-control connection could not be accepted",
            Self::EndpointUnavailable => "browser-control connection could not be inspected",
            Self::Unauthorized => "browser-control connection was not authorized",
        })
    }
}

impl std::error::Error for BrowserControlAcceptError {}

/// A bounded, redacted connection failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrowserControlConnectionError {
    InvalidConfiguration,
    Accept(BrowserControlAcceptError),
    Handshake(WebSocketHandshakeError),
}

impl From<BrowserControlAcceptError> for BrowserControlConnectionError {
    fn from(error: BrowserControlAcceptError) -> Self {
        Self::Accept(error)
    }
}

impl fmt::Display for BrowserControlConnectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidConfiguration => "browser-control handshake configuration is invalid",
            Self::Accept(_) => "browser-control connection was not authorized",
            Self::Handshake(_) => "browser-control WebSocket handshake failed",
        })
    }
}

impl std::error::Error for BrowserControlConnectionError {}

/// Bounded reasons for rejecting an opening handshake.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebSocketHandshakeError {
    Timeout,
    Incomplete,
    TooLarge,
    TooManyHeaders,
    RequestLineTooLong,
    Malformed,
    DuplicateHeader,
    UnsupportedRequest,
    Io,
}

impl fmt::Display for WebSocketHandshakeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("browser-control WebSocket handshake was rejected")
    }
}

impl std::error::Error for WebSocketHandshakeError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::net::Shutdown;

    const VALID_REQUEST: &str = "GET /control HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n";

    struct Authorizer {
        calls: RefCell<usize>,
        result: Result<(), AuthorizationError>,
    }

    impl BrowserControlProcessAuthorizer for Authorizer {
        fn authorize(
            &self,
            _local: SocketAddr,
            _peer: SocketAddr,
            _expected_executable: &Path,
        ) -> Result<(), AuthorizationError> {
            *self.calls.borrow_mut() += 1;
            self.result
        }
    }

    fn connected() -> (TcpListener, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        (listener, client)
    }

    fn handshake(
        request: &[u8],
        limits: WebSocketHandshakeLimits,
    ) -> Result<BrowserControlWebSocketStream, WebSocketHandshakeError> {
        let (listener, mut client) = connected();
        client.write_all(request).unwrap();
        client.shutdown(Shutdown::Write).unwrap();
        let (server, _) = listener.accept().unwrap();
        perform_websocket_handshake(server, "/control", limits)
    }

    #[test]
    fn authorization_failure_never_invokes_handshake_parsing() {
        let browser_listener = BrowserControlListener::bind("127.0.0.1:0", "/browser").unwrap();
        let (injected, mut client) = connected();
        client.write_all(VALID_REQUEST.as_bytes()).unwrap();
        let authorizer = Authorizer {
            calls: RefCell::new(0),
            result: Err(AuthorizationError::ExecutableVerificationFailed),
        };
        assert_eq!(
            browser_listener.accept_with(&injected, &authorizer).err(),
            Some(BrowserControlAcceptError::Unauthorized)
        );
        assert_eq!(*authorizer.calls.borrow(), 1);
        client
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        match client.peek(&mut [0]) {
            Ok(0) => {}
            Err(error) if error.kind() == io::ErrorKind::ConnectionReset => {}
            result => panic!("unauthorized stream remained open: {result:?}"),
        }
    }

    #[test]
    fn accepts_fragmented_mixed_case_headers_and_rfc_key_vector() {
        let (listener, mut client) = connected();
        let request = b"GET /control HTTP/1.1\r\nconnection: keep-alive, Upgrade\r\nSec-WebSocket-Version: 13\r\nHOST: localhost\r\nsec-websocket-key: dGhlIHNhbXBsZSBub25jZQ==\r\nUpGrAdE: WebSocket\r\n\r\n";
        for byte in request {
            client.write_all(&[*byte]).unwrap();
        }
        let (server, _) = listener.accept().unwrap();
        drop(
            perform_websocket_handshake(server, "/control", WebSocketHandshakeLimits::default())
                .unwrap(),
        );
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n"));
        assert!(!response.contains("Sec-WebSocket-Protocol"));
        assert!(!response.contains("Sec-WebSocket-Extensions"));
    }

    #[test]
    fn request_line_limit_is_inclusive_and_independent() {
        let line = "GET /control HTTP/1.1";
        let defaults = WebSocketHandshakeLimits::default();
        assert!(handshake(
            VALID_REQUEST.as_bytes(),
            WebSocketHandshakeLimits {
                max_request_line_bytes: line.len(),
                ..defaults
            }
        )
        .is_ok());
        assert_eq!(
            handshake(
                VALID_REQUEST.as_bytes(),
                WebSocketHandshakeLimits {
                    max_request_line_bytes: line.len() - 1,
                    ..defaults
                }
            )
            .unwrap_err(),
            WebSocketHandshakeError::RequestLineTooLong
        );
        let overlong = vec![b'G'; 4096];
        assert_eq!(
            handshake(
                &overlong,
                WebSocketHandshakeLimits {
                    read_timeout: Duration::from_secs(10),
                    max_header_bytes: 8192,
                    max_request_line_bytes: 10,
                    ..defaults
                }
            )
            .unwrap_err(),
            WebSocketHandshakeError::RequestLineTooLong
        );
    }

    #[test]
    fn rejects_duplicate_security_critical_headers_with_mixed_casing() {
        for header in [
            "hOsT: attacker\r\n",
            "uPgRaDe: websocket\r\n",
            "cOnNeCtIoN: Upgrade\r\n",
            "sEc-WeBsOcKeT-kEy: dGhlIHNhbXBsZSBub25jZQ==\r\n",
            "sEc-WeBsOcKeT-vErSiOn: 13\r\n",
        ] {
            let request = VALID_REQUEST.replacen("\r\n\r\n", &format!("\r\n{header}\r\n"), 1);
            assert_eq!(
                handshake(request.as_bytes(), WebSocketHandshakeLimits::default()).unwrap_err(),
                WebSocketHandshakeError::DuplicateHeader,
                "{header}"
            );
        }
    }

    #[test]
    fn rejects_header_byte_and_count_limits() {
        let defaults = WebSocketHandshakeLimits::default();
        assert_eq!(
            handshake(
                VALID_REQUEST.as_bytes(),
                WebSocketHandshakeLimits {
                    max_header_bytes: VALID_REQUEST.len() - 1,
                    ..defaults
                }
            )
            .unwrap_err(),
            WebSocketHandshakeError::TooLarge
        );
        assert_eq!(
            handshake(
                VALID_REQUEST.as_bytes(),
                WebSocketHandshakeLimits {
                    max_headers: 4,
                    ..defaults
                }
            )
            .unwrap_err(),
            WebSocketHandshakeError::TooManyHeaders
        );

        let (listener, _client) = connected();
        let (server, _) = listener.accept().unwrap();
        assert_eq!(
            perform_websocket_handshake(
                server,
                "/control",
                WebSocketHandshakeLimits {
                    read_timeout: Duration::from_millis(10),
                    ..defaults
                }
            )
            .unwrap_err(),
            WebSocketHandshakeError::Timeout
        );
    }

    #[test]
    fn malformed_incomplete_wrong_target_and_version_fail_closed() {
        for request in [
            VALID_REQUEST.replace(
                "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==",
                "Sec-WebSocket-Key: short",
            ),
            VALID_REQUEST.replace("GET /control", "GET /wrong"),
            VALID_REQUEST.replace("Version: 13", "Version: 12"),
            VALID_REQUEST.replace("\r\n\r\n", "\r\nBroken\r\n\r\n"),
            VALID_REQUEST.replace("Host: localhost", "Host: local\0host"),
        ] {
            assert!(handshake(request.as_bytes(), WebSocketHandshakeLimits::default()).is_err());
        }
        assert_eq!(
            handshake(
                VALID_REQUEST.trim_end_matches("\r\n").as_bytes(),
                WebSocketHandshakeLimits::default()
            )
            .unwrap_err(),
            WebSocketHandshakeError::Incomplete
        );
    }
}
