//! The loopback page a provider's OAuth redirect lands on. The runtime holds
//! it for a sign-in it runs on this computer. A client holds it on its own
//! computer for a sign-in a paired runtime runs, because the browser's
//! `localhost` redirect reaches only the computer the browser runs on, and
//! the client hands the redirect to the runtime's `login_callback` action.

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::host::SignInPage;

/// The fixed local port the OpenAI account redirect lands on.
pub const CALLBACK_PORT: u16 = 1455;
/// The path of that redirect.
pub const CALLBACK_PATH: &str = "/auth/callback";

/// The name the callback page gives a provider.
pub fn provider_display_name(provider: &str) -> &str {
    match provider {
        "openai-codex" => "OpenAI",
        "xai" => "xAI",
        "openrouter" => "OpenRouter",
        "github-copilot" => "GitHub Copilot",
        "anthropic" => "Anthropic",
        other => other,
    }
}

/// The redirect URL for a request line that hits the OpenAI callback path
/// with a code, else nothing.
pub fn callback_redirect(request_line: &str) -> Option<String> {
    callback_redirect_on(request_line, CALLBACK_PORT, CALLBACK_PATH)
}

/// The redirect URL for a request line that hits `path` with a code, as the
/// browser sent it to `port`, else nothing.
pub fn callback_redirect_on(request_line: &str, port: u16, path: &str) -> Option<String> {
    let mut parts = request_line.split_whitespace();
    if parts.next()? != "GET" {
        return None;
    }
    let target = parts.next()?;
    let (target_path, query) = target.split_once('?')?;
    if target_path != path
        || !query
            .split('&')
            .any(|pair| pair.starts_with("code=") && pair.len() > "code=".len())
    {
        return None;
    }
    Some(format!("http://localhost:{port}{target}"))
}

/// The redirect a client forwards, checked the way the runtime's own page
/// checks it: a loopback origin on the callback port, the callback path and
/// a code. Anything else answers nothing.
pub fn forwarded_redirect(url: &str) -> Option<String> {
    forwarded_redirect_on(url, CALLBACK_PORT, CALLBACK_PATH)
}

/// [`forwarded_redirect`] for a sign-in whose redirect lands on `port` and `path`.
pub fn forwarded_redirect_on(url: &str, port: u16, path: &str) -> Option<String> {
    let rest = ["http://localhost", "http://127.0.0.1", "http://[::1]"]
        .into_iter()
        .find_map(|origin| url.strip_prefix(origin))?;
    let target = rest.strip_prefix(&format!(":{port}"))?;
    if !target.starts_with('/') || target.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    callback_redirect_on(&format!("GET {target} HTTP/1.1"), port, path)
}

pub(crate) fn write_response(stream: &mut TcpStream, status: &str, body: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
}

/// Holds the callback port on both loopback addresses, so the browser's
/// `localhost` redirect lands here whichever one it resolves first. A wildcard
/// listener of another program yields to the specific bind, a program on the
/// loopback itself is named, and a host without an IPv6 loopback keeps the
/// IPv4 one alone.
pub fn bind(port: u16) -> Result<Vec<TcpListener>, String> {
    let mut listeners = Vec::new();
    for address in [
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(Ipv6Addr::LOCALHOST),
    ] {
        match TcpListener::bind((address, port)) {
            Ok(listener) => listeners.push(listener),
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                return Err(port_error(port));
            }
            Err(_) => {}
        }
    }
    if listeners.is_empty() {
        return Err(port_error(port));
    }
    Ok(listeners)
}

fn port_error(port: u16) -> String {
    format!("Port {port} is in use by another program. Stop it and try again, or use an API key.")
}

/// Hands a redirect on and answers whether it arrived, which picks the page
/// the browser shows.
pub type Deliver = dyn Fn(String) -> bool + Send + Sync;

/// The servers on the callback listeners, one thread per listener. Stopping
/// or dropping it closes the listeners, so the next sign-in binds the port at
/// once.
pub struct CallbackServer {
    stop: Arc<AtomicBool>,
    servers: Vec<JoinHandle<()>>,
}

impl CallbackServer {
    /// Serves `listeners` until stopped. Each redirect with a code goes to
    /// `deliver`, and the browser gets `page` for `provider_name` when it
    /// arrived, the failure page when it did not, and a 404 for any other path.
    pub fn start(
        listeners: Vec<TcpListener>,
        page: Arc<SignInPage>,
        provider_name: String,
        deliver: Arc<Deliver>,
    ) -> Self {
        Self::start_on(
            listeners,
            CALLBACK_PORT,
            CALLBACK_PATH.to_owned(),
            page,
            provider_name,
            deliver,
        )
    }

    /// [`CallbackServer::start`] for a redirect that lands on `port` and `path`.
    pub fn start_on(
        listeners: Vec<TcpListener>,
        port: u16,
        path: String,
        page: Arc<SignInPage>,
        provider_name: String,
        deliver: Arc<Deliver>,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let servers = listeners
            .into_iter()
            .map(|listener| {
                let stop = Arc::clone(&stop);
                let page = Arc::clone(&page);
                let deliver = Arc::clone(&deliver);
                let provider_name = provider_name.clone();
                let path = path.clone();
                std::thread::spawn(move || {
                    serve(
                        listener,
                        (port, &path),
                        &stop,
                        page.as_ref(),
                        &provider_name,
                        deliver.as_ref(),
                    )
                })
            })
            .collect();
        Self { stop, servers }
    }

    /// Stops the servers and waits for their threads.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for server in self.servers.drain(..) {
            let _ = server.join();
        }
    }
}

impl Drop for CallbackServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn serve(
    listener: TcpListener,
    (port, path): (u16, &str),
    stop: &AtomicBool,
    page: &SignInPage,
    provider_name: &str,
    deliver: &Deliver,
) {
    let _ = listener.set_nonblocking(true);
    while !stop.load(Ordering::SeqCst) {
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(_) => break,
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let line = request_line(&mut stream);
        match callback_redirect_on(&line, port, path).map(deliver) {
            Some(true) => write_response(&mut stream, "200 OK", &page(Some(provider_name))),
            Some(false) => write_response(&mut stream, "502 Bad Gateway", &page(None)),
            None => write_response(&mut stream, "404 Not Found", &page(None)),
        }
    }
}

/// The first line of the request on `stream`, read up to the header's end.
fn request_line(stream: &mut TcpStream) -> String {
    let mut request = Vec::new();
    let mut buffer = [0u8; 1024];
    while request.len() < 16 * 1024 {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&request)
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn the_redirect_parses_only_on_the_callback_path_with_a_code() {
        assert_eq!(
            callback_redirect("GET /auth/callback?code=ac_1&state=s HTTP/1.1"),
            Some("http://localhost:1455/auth/callback?code=ac_1&state=s".to_owned())
        );
        assert_eq!(
            callback_redirect("GET /auth/callback?state=s HTTP/1.1"),
            None
        );
        assert_eq!(callback_redirect("GET /favicon.ico HTTP/1.1"), None);
        assert_eq!(
            callback_redirect("POST /auth/callback?code=x HTTP/1.1"),
            None
        );
    }

    #[test]
    fn a_forwarded_redirect_needs_a_loopback_origin_on_the_callback_port() {
        let expected = Some("http://localhost:1455/auth/callback?code=c&state=s".to_owned());
        for url in [
            "http://localhost:1455/auth/callback?code=c&state=s",
            "http://127.0.0.1:1455/auth/callback?code=c&state=s",
            "http://[::1]:1455/auth/callback?code=c&state=s",
        ] {
            assert_eq!(forwarded_redirect(url), expected, "{url}");
        }
        assert_eq!(
            forwarded_redirect_on(
                "http://127.0.0.1:40259/callback?code=c&state=s",
                40259,
                "/callback"
            ),
            Some("http://localhost:40259/callback?code=c&state=s".to_owned())
        );
        assert_eq!(
            forwarded_redirect_on("http://localhost:1455/callback?code=c", 40259, "/callback"),
            None
        );
        for url in [
            "https://localhost:1455/auth/callback?code=c",
            "http://example.com:1455/auth/callback?code=c",
            "http://localhost:1456/auth/callback?code=c",
            "http://localhost:14550/auth/callback?code=c",
            "http://localhost:1455/auth/callback?state=s",
            "http://localhost:1455/other?code=c",
            "http://localhost:1455/auth/callback?code=c HTTP/1.1\r\nHost: x",
            "http://localhost:1455@example.com/auth/callback?code=c",
        ] {
            assert_eq!(forwarded_redirect(url), None, "{url}");
        }
    }

    #[test]
    fn the_callback_port_is_held_on_both_loopbacks_and_a_holder_is_named() {
        let probe = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let listeners = bind(port).unwrap();
        let addresses: Vec<IpAddr> = listeners
            .iter()
            .map(|listener| listener.local_addr().unwrap().ip())
            .collect();
        assert_eq!(addresses[0], IpAddr::V4(Ipv4Addr::LOCALHOST));
        if let Some(second) = addresses.get(1) {
            assert_eq!(*second, IpAddr::V6(Ipv6Addr::LOCALHOST));
        }
        assert!(listeners
            .iter()
            .all(|listener| listener.local_addr().unwrap().port() == port));
        drop(listeners);
        let holder = TcpListener::bind(("127.0.0.1", port)).unwrap();
        let error = bind(port).unwrap_err();
        assert!(error.contains(&port.to_string()), "{error}");
        drop(holder);
    }

    #[test]
    fn the_server_hands_a_redirect_on_and_shows_the_page_for_its_outcome() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let received = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&received);
        let accept = Arc::new(AtomicBool::new(true));
        let answer = Arc::clone(&accept);
        let page: Arc<SignInPage> = Arc::new(|provider: Option<&str>| match provider {
            Some(name) => format!("ok {name}"),
            None => "failed".to_owned(),
        });
        let mut server = CallbackServer::start(
            vec![listener],
            page,
            "OpenAI".into(),
            Arc::new(move |url| {
                sink.lock().unwrap().push(url);
                answer.load(Ordering::SeqCst)
            }),
        );
        let get = |target: &str| {
            let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
            write!(stream, "GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            response
        };
        let response = get("/auth/callback?code=c1");
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
        assert!(response.ends_with("ok OpenAI"), "{response}");
        accept.store(false, Ordering::SeqCst);
        let response = get("/auth/callback?code=c2");
        assert!(response.starts_with("HTTP/1.1 502"), "{response}");
        assert!(get("/favicon.ico").starts_with("HTTP/1.1 404"));
        assert_eq!(
            *received.lock().unwrap(),
            vec![
                "http://localhost:1455/auth/callback?code=c1".to_owned(),
                "http://localhost:1455/auth/callback?code=c2".to_owned()
            ]
        );
        server.stop();
        assert!(TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok());
    }
}
