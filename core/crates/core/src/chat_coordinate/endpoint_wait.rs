//! A server the reply's model lives on that does not answer. The run names
//! the server while Pi retries, probes it with a short connect bound, and
//! ends the reply with the server's name and URL when it stays silent.

use std::net::TcpStream;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

/// How long the probe waits for the server to accept a connection.
pub(super) const CONNECT_BOUND: Duration = Duration::from_secs(10);
/// Pi tries a call that never reached the server this many times again.
pub(super) const CONNECTION_RETRIES: u32 = 1;

/// A server Pi reaches through `models.json`: its name and base URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Endpoint {
    pub name: String,
    pub url: String,
}

impl Endpoint {
    /// The server of the model `provider/model`, when the model lives on a
    /// local server or a custom endpoint.
    pub(super) fn of_model(agent: &Path, model: &str) -> Option<Self> {
        let (provider, _) = model.split_once('/')?;
        crate::models::pi_models::endpoints(agent)
            .ok()?
            .into_iter()
            .find(|(id, ..)| id == provider)
            .map(|(_, name, url)| Self { name, url })
    }

    /// The run stage that names the server while Pi tries `attempt` of
    /// `attempts` calls.
    pub(super) fn retry_stage(&self, attempt: u32, attempts: u32) -> String {
        format!("retrying:{attempt}:{attempts}:{}", self.name)
    }

    /// What a reply shows when the server never answered.
    pub(super) fn silent_reason(&self) -> String {
        format!(
            "{} did not answer at {}. Check that the server is running, then send again.",
            self.name, self.url
        )
    }
}

/// The stage of a retry with no server to name.
pub(super) fn retry_stage(attempt: u32, attempts: u32) -> String {
    format!("retrying:{attempt}:{attempts}")
}

/// One connect to a server, on its own thread.
pub(super) struct Probe(mpsc::Receiver<bool>);

impl Probe {
    /// Starts a connect to the host and port of `url` within `bound`. A proxy
    /// in the environment reaches servers a direct connect cannot, so no
    /// probe starts then.
    pub(super) fn start(url: &str, bound: Duration) -> Option<Self> {
        if [
            "HTTPS_PROXY",
            "https_proxy",
            "HTTP_PROXY",
            "http_proxy",
            "ALL_PROXY",
            "all_proxy",
        ]
        .iter()
        .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
        {
            return None;
        }
        let url = url::Url::parse(url).ok()?;
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("endpoint-probe".into())
            .spawn(move || {
                let answered = url.socket_addrs(|| None).is_ok_and(|addresses| {
                    addresses
                        .iter()
                        .any(|address| TcpStream::connect_timeout(address, bound).is_ok())
                });
                let _ = sender.send(answered);
            })
            .ok()?;
        Some(Self(receiver))
    }

    /// Whether the probe ended within `wait` with no server that accepted
    /// the connect. A probe answers once.
    pub(super) fn silent(&self, wait: Duration) -> bool {
        matches!(self.0.recv_timeout(wait), Ok(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn a_closed_port_is_silent_at_once() {
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let probe = Probe::start(&format!("http://127.0.0.1:{port}/v1"), CONNECT_BOUND).unwrap();
        let started = std::time::Instant::now();
        assert!(probe.silent(Duration::from_secs(5)));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_server_that_never_accepts_is_silent_after_the_bound() {
        // 192.0.2.0/24 is reserved for documentation, so nothing answers it.
        let bound = Duration::from_millis(300);
        let probe = Probe::start("http://192.0.2.1:8000/v1", bound).unwrap();
        assert!(probe.silent(Duration::from_secs(5)));
    }

    #[test]
    fn a_listening_server_answers() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let probe = Probe::start(&url, CONNECT_BOUND).unwrap();
        assert!(!probe.silent(Duration::from_secs(5)));
        assert!(!probe.silent(Duration::ZERO));
    }

    #[test]
    fn the_server_is_named_in_the_stage_and_the_reason() {
        let endpoint = Endpoint {
            name: "DO Test".into(),
            url: "http://192.0.2.1:8000/v1".into(),
        };
        assert_eq!(endpoint.retry_stage(2, 2), "retrying:2:2:DO Test");
        assert_eq!(retry_stage(2, 4), "retrying:2:4");
        assert_eq!(
            endpoint.silent_reason(),
            "DO Test did not answer at http://192.0.2.1:8000/v1. Check that the server is running, then send again."
        );
    }

    #[test]
    fn a_saved_endpoint_is_found_by_its_model() {
        let agent = std::env::temp_dir().join(format!("endpoint-wait-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&agent).unwrap();
        std::fs::write(
            crate::models::pi_models::models_file(&agent),
            r#"{"providers":{"custom-do-test":{"baseUrl":"http://192.0.2.1:8000/v1","api":"openai-completions","models":[{"id":"qwen"}]}}}"#,
        )
        .unwrap();
        let found = Endpoint::of_model(&agent, "custom-do-test/qwen").unwrap();
        assert_eq!(found.url, "http://192.0.2.1:8000/v1");
        assert_eq!(Endpoint::of_model(&agent, "openai/gpt-5"), None);
        let _ = std::fs::remove_dir_all(&agent);
    }
}
