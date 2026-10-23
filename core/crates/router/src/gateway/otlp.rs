//! Traces over OTLP/HTTP in its JSON encoding, with the OpenTelemetry GenAI
//! semantic conventions.
//!
//! A request thread hands each span to a bounded queue and never waits. One
//! sender thread posts the queue in batches to the collector's traces endpoint
//! with the configured headers. A full queue drops the span and counts it, so
//! a slow or missing collector never slows a turn.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const QUEUE: usize = 10_000;
const BATCH: usize = 100;
const INTERVAL: Duration = Duration::from_secs(2);
const TIMEOUT: Duration = Duration::from_secs(10);
/// The longest trace id a request header may name.
const TRACE_ID_LIMIT: usize = 128;
/// The instrumentation scope and the default service name.
pub const SCOPE: &str = "muniment-router";
/// `SPAN_KIND_CLIENT`: the gateway calls a model on the caller's behalf.
const SPAN_KIND_CLIENT: u8 = 3;
/// `STATUS_CODE_ERROR`.
const STATUS_ERROR: u8 = 2;

/// Where the spans go.
#[derive(Clone, PartialEq, Eq)]
pub struct Settings {
    /// The full traces URL, such as `http://collector:4318/v1/traces`.
    pub endpoint: String,
    /// Sent with every export, such as an `authorization` header.
    pub headers: Vec<(String, String)>,
    /// The `service.name` resource attribute.
    pub service_name: String,
}

impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.headers.iter().map(|(name, _)| name.as_str()).collect();
        f.debug_struct("Settings")
            .field("endpoint", &self.endpoint)
            .field("headers", &names)
            .field("service_name", &self.service_name)
            .finish()
    }
}

/// The traces URL under an OTLP base endpoint, as the OpenTelemetry
/// environment variables define it.
pub fn traces_url(base: &str) -> String {
    format!("{}/v1/traces", base.trim_end_matches('/'))
}

/// Parses `name=value,name=value` as `OTEL_EXPORTER_OTLP_HEADERS` writes it,
/// with percent-encoded values.
pub fn parse_headers(text: &str) -> Result<Vec<(String, String)>, String> {
    text.split(',')
        .filter(|entry| !entry.trim().is_empty())
        .map(|entry| {
            let (name, value) = entry
                .split_once('=')
                .ok_or_else(|| "An OTLP header is not name=value.".to_owned())?;
            let name = percent_decode(name.trim());
            if name.is_empty() {
                return Err("An OTLP header has no name.".into());
            }
            Ok((name, percent_decode(value.trim())))
        })
        .collect()
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let decoded = (bytes[index] == b'%')
            .then(|| text.get(index + 1..index + 3))
            .flatten()
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match decoded {
            Some(byte) => {
                out.push(byte);
                index += 3;
            }
            None => {
                out.push(bytes[index]);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

enum Message {
    Span(Value),
    Flush(mpsc::Sender<()>),
}

/// The export queue. Cloning shares the queue and its sender thread.
#[derive(Clone)]
pub struct Exporter {
    queue: SyncSender<Message>,
    dropped: Arc<AtomicU64>,
    failed: Arc<AtomicU64>,
}

impl Exporter {
    /// Starts the sender thread.
    pub fn start(settings: Settings) -> Self {
        let (queue, receiver) = mpsc::sync_channel(QUEUE);
        let failed = Arc::new(AtomicU64::new(0));
        let sender_failed = Arc::clone(&failed);
        std::thread::spawn(move || send_loop(&settings, &receiver, &sender_failed));
        Self {
            queue,
            dropped: Arc::new(AtomicU64::new(0)),
            failed,
        }
    }

    /// Queues one span without waiting.
    pub fn send(&self, span: Value) {
        if let Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) =
            self.queue.try_send(Message::Span(span))
        {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Posts every queued span, waiting at most `deadline`. Answers whether
    /// the queue emptied in time.
    pub fn flush(&self, deadline: Duration) -> bool {
        let (done, wait) = mpsc::channel();
        let until = Instant::now() + deadline;
        loop {
            match self.queue.try_send(Message::Flush(done.clone())) {
                Ok(()) => break,
                Err(TrySendError::Disconnected(_)) => return false,
                Err(TrySendError::Full(_)) if Instant::now() >= until => return false,
                Err(TrySendError::Full(_)) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        wait.recv_timeout(until.saturating_duration_since(Instant::now()))
            .is_ok()
    }

    /// Spans the full queue dropped.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Spans the collector did not accept.
    pub fn failed(&self) -> u64 {
        self.failed.load(Ordering::Relaxed)
    }
}

/// One export request around `spans`.
pub fn request(service_name: &str, spans: Vec<Value>) -> Value {
    json!({"resourceSpans": [{
        "resource": {"attributes": [string("service.name", service_name)]},
        "scopeSpans": [{
            "scope": {"name": SCOPE, "version": env!("CARGO_PKG_VERSION")},
            "spans": spans,
        }],
    }]})
}

fn send_loop(settings: &Settings, receiver: &Receiver<Message>, failed: &AtomicU64) {
    let agent = crate::http::agent_builder().timeout(TIMEOUT).build();
    let mut batch: Vec<Value> = Vec::new();
    let mut due = Instant::now() + INTERVAL;
    loop {
        let message = receiver.recv_timeout(due.saturating_duration_since(Instant::now()));
        let mut flushed = None;
        match message {
            Ok(Message::Span(span)) => batch.push(span),
            Ok(Message::Flush(done)) => flushed = Some(done),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                post(&agent, settings, &mut batch, failed);
                return;
            }
        }
        if flushed.is_some() || batch.len() >= BATCH || Instant::now() >= due {
            post(&agent, settings, &mut batch, failed);
            due = Instant::now() + INTERVAL;
        }
        if let Some(done) = flushed {
            let _ = done.send(());
        }
    }
}

fn post(agent: &ureq::Agent, settings: &Settings, batch: &mut Vec<Value>, failed: &AtomicU64) {
    if batch.is_empty() {
        return;
    }
    let spans = std::mem::take(batch);
    let count = spans.len() as u64;
    let mut call = agent
        .post(&settings.endpoint)
        .set("content-type", "application/json");
    for (name, value) in &settings.headers {
        call = call.set(name, value);
    }
    match call.send_json(request(&settings.service_name, spans)) {
        // A collector may accept part of a batch and name what it rejected. A
        // rejected span is logged once and not sent again.
        Ok(response) => {
            let answer: Value = response.into_json().unwrap_or(Value::Null);
            let rejected = answer["partialSuccess"]["rejectedSpans"]
                .as_u64()
                .or_else(|| {
                    answer["partialSuccess"]["rejectedSpans"]
                        .as_str()
                        .and_then(|text| text.parse().ok())
                })
                .unwrap_or(0);
            if rejected > 0 {
                failed.fetch_add(rejected, Ordering::Relaxed);
                eprintln!(
                    "muniment-router: the OTLP collector rejected {rejected} of {count} spans: {}",
                    answer["partialSuccess"]["errorMessage"]
                );
            }
        }
        Err(error) => {
            failed.fetch_add(count, Ordering::Relaxed);
            eprintln!("muniment-router: OTLP export failed for {count} spans: {error}");
        }
    }
}

/// A string attribute.
pub fn string(key: &str, value: &str) -> Value {
    json!({"key": key, "value": {"stringValue": value}})
}

/// An integer attribute. OTLP JSON carries 64-bit integers as strings.
pub fn int(key: &str, value: i64) -> Value {
    json!({"key": key, "value": {"intValue": value.to_string()}})
}

/// A floating point attribute.
pub fn double(key: &str, value: f64) -> Value {
    json!({"key": key, "value": {"doubleValue": value}})
}

/// The trace a span joins, and the span it hangs under when the caller named
/// one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parent {
    /// 32 lowercase hex characters.
    pub trace_id: String,
    /// 16 lowercase hex characters.
    pub span_id: Option<String>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn is_hex(text: &str, length: usize) -> bool {
    text.len() == length && text.bytes().all(|c| c.is_ascii_hexdigit())
}

/// The parent of a turn's span: the W3C `traceparent` header when it is
/// valid, then the trace the `x-muniment-trace` header names, then the task's
/// own trace.
pub fn parent(traceparent: Option<&str>, trace: Option<&str>, task_id: &str) -> Parent {
    if let Some(parent) = traceparent.and_then(w3c_parent) {
        return parent;
    }
    let trace_id = trace_header(trace)
        .map(|trace| {
            if is_hex(&trace, 32) {
                trace.to_ascii_lowercase()
            } else {
                task_trace_id(&trace)
            }
        })
        .unwrap_or_else(|| task_trace_id(task_id));
    Parent {
        trace_id,
        span_id: None,
    }
}

fn w3c_parent(value: &str) -> Option<Parent> {
    let mut parts = value.trim().split('-');
    let (version, trace, span, flags) =
        (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    let valid = is_hex(version, 2)
        && version != "ff"
        && is_hex(trace, 32)
        && is_hex(span, 16)
        && is_hex(flags, 2)
        && trace.bytes().any(|c| c != b'0')
        && span.bytes().any(|c| c != b'0')
        && (version != "00" || parts.next().is_none());
    valid.then(|| Parent {
        trace_id: trace.to_ascii_lowercase(),
        span_id: Some(span.to_ascii_lowercase()),
    })
}

/// The trace a request names in `x-muniment-trace`, when it is a usable id.
pub fn trace_header(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    let usable = !value.is_empty()
        && value.len() <= TRACE_ID_LIMIT
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_');
    usable.then(|| value.to_owned())
}

/// The trace id for a seed, such as a task id: the first 16 bytes of its
/// SHA-256, in hex. A caller that derives its own trace id the same way
/// joins the gateway's spans without a header.
pub fn task_trace_id(seed: &str) -> String {
    hex(&Sha256::digest(seed.as_bytes())[..16])
}

/// A fresh span id.
pub fn span_id() -> String {
    loop {
        let mut bytes = [0_u8; 8];
        if getrandom::fill(&mut bytes).is_err() {
            bytes.copy_from_slice(&uuid::Uuid::new_v4().as_bytes()[..8]);
        }
        if bytes.iter().any(|byte| *byte != 0) {
            return hex(&bytes);
        }
    }
}

/// Nanoseconds since the Unix epoch, as OTLP JSON writes them.
pub fn nanos(at: chrono::DateTime<chrono::Utc>) -> String {
    at.timestamp_nanos_opt()
        .unwrap_or(at.timestamp_millis().saturating_mul(1_000_000))
        .to_string()
}

/// One client span. `error` sets the error status with its message.
pub fn span(
    parent: &Parent,
    name: &str,
    started: chrono::DateTime<chrono::Utc>,
    ended: chrono::DateTime<chrono::Utc>,
    attributes: Vec<Value>,
    error: Option<String>,
) -> Value {
    let mut span = json!({
        "traceId": parent.trace_id,
        "spanId": span_id(),
        "name": name,
        "kind": SPAN_KIND_CLIENT,
        "startTimeUnixNano": nanos(started),
        "endTimeUnixNano": nanos(ended),
        "attributes": attributes,
    });
    if let Some(parent) = &parent.span_id {
        span["parentSpanId"] = json!(parent);
    }
    if let Some(message) = error {
        span["status"] = json!({"code": STATUS_ERROR, "message": message});
    }
    span
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    /// One export request a stand-in collector received.
    pub(crate) struct Captured {
        pub path: String,
        pub headers: Vec<(String, String)>,
        pub body: Value,
    }

    impl Captured {
        pub fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.as_str())
        }

        /// Every span in the request.
        pub fn spans(&self) -> Vec<Value> {
            self.body["resourceSpans"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|resource| {
                    resource["scopeSpans"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                })
                .flat_map(|scope| scope["spans"].as_array().cloned().unwrap_or_default())
                .collect()
        }
    }

    /// The value of attribute `key` on `span`, whatever its type.
    pub(crate) fn attribute(span: &Value, key: &str) -> Value {
        span["attributes"]
            .as_array()
            .and_then(|attributes| attributes.iter().find(|a| a["key"] == key))
            .and_then(|a| a["value"].as_object())
            .and_then(|value| value.values().next().cloned())
            .unwrap_or(Value::Null)
    }

    /// A stand-in collector that answers every POST with `status` and `answer`
    /// and sends each request to the receiver.
    pub(crate) fn collector(
        status: u16,
        answer: &'static str,
    ) -> (String, mpsc::Receiver<Captured>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (sender, seen) = mpsc::channel();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                let _ = reader.read_line(&mut request);
                let path = request.split_whitespace().nth(1).unwrap_or("").to_owned();
                let (mut length, mut headers) = (0, Vec::new());
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    let line = line.trim_end();
                    if line.is_empty() {
                        break;
                    }
                    let (name, value) = line.split_once(':').unwrap_or((line, ""));
                    if name.eq_ignore_ascii_case("content-length") {
                        length = value.trim().parse().unwrap_or(0);
                    }
                    headers.push((name.to_owned(), value.trim().to_owned()));
                }
                let mut body = vec![0; length];
                let _ = reader.read_exact(&mut body);
                let _ = sender.send(Captured {
                    path,
                    headers,
                    body: serde_json::from_slice(&body).unwrap_or(Value::Null),
                });
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{answer}",
                    answer.len()
                );
            }
        });
        (format!("http://127.0.0.1:{port}"), seen)
    }

    pub(crate) fn settings(base: &str) -> Settings {
        Settings {
            endpoint: traces_url(base),
            headers: parse_headers("authorization=Basic%20cGstMTpzay0x,x-scope=a=b").unwrap(),
            service_name: "gateway-test".into(),
        }
    }

    #[test]
    fn spans_post_in_one_request_with_the_headers_on_flush() {
        let (base, seen) = collector(200, "{}");
        let exporter = Exporter::start(settings(&format!("{base}/")));
        let parent = parent(None, None, "task-1");
        let at = chrono::DateTime::from_timestamp_millis(1_700_000_000_123).unwrap();
        exporter.send(span(&parent, "one", at, at, vec![int("n", 1)], None));
        exporter.send(span(
            &parent,
            "two",
            at,
            at,
            Vec::new(),
            Some("429 rate_limited".into()),
        ));
        assert!(exporter.flush(Duration::from_secs(5)));
        let captured = seen.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(captured.path, "/v1/traces");
        assert_eq!(captured.header("authorization"), Some("Basic cGstMTpzay0x"));
        assert_eq!(captured.header("x-scope"), Some("a=b"));
        assert_eq!(captured.header("content-type"), Some("application/json"));
        let resource = &captured.body["resourceSpans"][0];
        assert_eq!(resource["resource"]["attributes"][0]["key"], "service.name");
        assert_eq!(
            resource["resource"]["attributes"][0]["value"]["stringValue"],
            "gateway-test"
        );
        assert_eq!(resource["scopeSpans"][0]["scope"]["name"], SCOPE);
        let spans = captured.spans();
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0]["traceId"], task_trace_id("task-1"));
        assert!(is_hex(spans[0]["spanId"].as_str().unwrap(), 16));
        assert_eq!(spans[0]["kind"], 3);
        assert_eq!(spans[0]["startTimeUnixNano"], "1700000000123000000");
        assert_eq!(attribute(&spans[0], "n"), "1");
        assert!(spans[0].get("status").is_none());
        assert_eq!(spans[1]["status"]["code"], 2);
        assert_eq!(spans[1]["status"]["message"], "429 rate_limited");
        assert_eq!(exporter.failed(), 0);
    }

    #[test]
    fn a_failing_collector_never_blocks_a_send() {
        let exporter = Exporter::start(settings("http://127.0.0.1:9"));
        let started = Instant::now();
        for index in 0..(QUEUE + 50) {
            exporter.send(json!({ "index": index }));
        }
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(exporter.flush(Duration::from_secs(30)));
        assert!(exporter.failed() > 0);
    }

    #[test]
    fn rejected_spans_count_as_failed() {
        let (base, _seen) = collector(
            200,
            r#"{"partialSuccess":{"rejectedSpans":"1","errorMessage":"bad span"}}"#,
        );
        let exporter = Exporter::start(settings(&base));
        exporter.send(json!({"name": "x"}));
        assert!(exporter.flush(Duration::from_secs(5)));
        assert_eq!(exporter.failed(), 1);
    }

    #[test]
    fn traces_come_from_traceparent_then_the_header_then_the_task() {
        let w3c = "00-4BF92F3577B34DA6A3CE929D0E0E4736-00F067AA0BA902B7-01";
        assert_eq!(
            parent(Some(w3c), Some("trace-9"), "task-1"),
            Parent {
                trace_id: "4bf92f3577b34da6a3ce929d0e0e4736".into(),
                span_id: Some("00f067aa0ba902b7".into()),
            }
        );
        let zero = "00-00000000000000000000000000000000-00f067aa0ba902b7-01";
        let hex_trace = "7AFAA346B4BF92BF9DC21E9AE8098874";
        assert_eq!(
            parent(Some(zero), Some(hex_trace), "task-1").trace_id,
            "7afaa346b4bf92bf9dc21e9ae8098874"
        );
        assert_eq!(
            parent(None, Some("trace-abc"), "task-1").trace_id,
            task_trace_id("trace-abc")
        );
        assert_eq!(
            parent(None, Some("a b"), "task-1").trace_id,
            task_trace_id("task-1")
        );
        assert_eq!(trace_header(Some(" abc-123_X ")), Some("abc-123_X".into()));
        assert_eq!(trace_header(Some("")), None);
        assert_eq!(trace_header(Some(&"a".repeat(129))), None);
        assert_eq!(trace_header(None), None);
        assert_eq!(task_trace_id("task-1"), "7afaa346b4bf92bf9dc21e9ae8098874");
        assert_ne!(span_id(), span_id());
    }

    #[test]
    fn headers_parse_as_the_opentelemetry_variable_writes_them() {
        assert_eq!(
            parse_headers(" Authorization = Basic%20YTpi== , x-a=1,").unwrap(),
            vec![
                ("Authorization".to_owned(), "Basic YTpi==".to_owned()),
                ("x-a".to_owned(), "1".to_owned()),
            ]
        );
        assert_eq!(parse_headers("").unwrap(), Vec::new());
        assert!(parse_headers("novalue").is_err());
        assert!(parse_headers("=x").is_err());
        assert_eq!(percent_decode("100%"), "100%");
        assert!(!format!("{:?}", settings("http://h")).contains("cGst"));
    }
}
