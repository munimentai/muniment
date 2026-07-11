use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock, TryLockError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::{ProbeOutcome, RestartPolicy, SidecarConfig, SidecarIo};

pub const PI_NPM_PACKAGE: &str = "@mariozechner/pi-coding-agent";
pub const PI_VERSION: &str = "0.73.1";

/// Builds the production Pi RPC launch contract for a verified, platform-native
/// Pi executable. The executable contains its Node-compatible runtime; a system
/// `node` installation is deliberately not part of this contract.
pub fn pi_sidecar_config(program: impl Into<String>) -> SidecarConfig {
    let mut config = SidecarConfig::new(program);
    config.args = vec!["--mode".into(), "rpc".into(), "--no-session".into()];
    config.restart = RestartPolicy::default();
    config.health_interval = Duration::from_secs(15);
    config.startup_timeout = Duration::from_secs(30);
    config.shutdown_timeout = Duration::from_secs(2);
    config
}

/// The sole stdout consumer for Pi's multiplexed JSONL protocol.
///
/// Calls are serialized so responses cannot race each other or the supervisor's
/// health probe. Frames that do not match the active call are delivered to all
/// subscribers in order instead of being consumed as probe responses.
pub struct PiRpcTransport {
    io: SidecarIo,
    next_id: AtomicU64,
    call_lock: Mutex<()>,
    subscribers: Mutex<Vec<mpsc::Sender<Value>>>,
}

/// Shared wiring for the supervisor health probe and all application RPC.
/// Keeping this handle alongside the supervisor guarantees there is only one
/// stdout consumer for a child generation.
#[derive(Clone, Default)]
pub struct PiRpcWiring {
    transport: Arc<OnceLock<Arc<PiRpcTransport>>>,
}

impl PiRpcWiring {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn transport(&self) -> Option<Arc<PiRpcTransport>> {
        self.transport.get().cloned()
    }

    pub fn readiness_probe(
        &self,
        timeout: Duration,
    ) -> impl Fn(&SidecarIo) -> Result<ProbeOutcome, String> + Send + Sync + 'static {
        let shared = Arc::clone(&self.transport);
        move |io| {
            let transport = shared.get_or_init(|| Arc::new(PiRpcTransport::new(io.clone())));
            transport.health_probe(timeout)(io)
        }
    }
}

impl PiRpcTransport {
    pub fn new(io: SidecarIo) -> Self {
        Self {
            io,
            next_id: AtomicU64::new(1),
            call_lock: Mutex::new(()),
            subscribers: Mutex::new(Vec::new()),
        }
    }

    /// Receives interleaved events and responses not correlated to this
    /// transport's currently active call.
    pub fn subscribe(&self) -> mpsc::Receiver<Value> {
        let (sender, receiver) = mpsc::channel();
        self.subscribers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(sender);
        receiver
    }

    /// Sends a Pi command and waits for its correlated response. The command
    /// must be a JSON object with a `type`; the transport supplies its own ID.
    pub fn call(&self, mut command: Value, timeout: Duration) -> Result<Value, String> {
        let guard = self
            .call_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let id = format!(
            "muniment-pi-{}",
            self.next_id.fetch_add(1, Ordering::Relaxed)
        );
        let object = command
            .as_object_mut()
            .ok_or_else(|| "Pi RPC command must be a JSON object".to_string())?;
        if object.get("type").and_then(Value::as_str).is_none() {
            return Err("Pi RPC command must contain a string `type`".into());
        }
        object.insert("id".into(), Value::String(id.clone()));
        self.call_locked(command, &id, timeout, guard)
    }

    /// Builds a health probe on the same dispatcher used by application calls.
    /// If a call is active, its bounded timeout owns health detection and the
    /// periodic probe does not compete for stdout.
    pub fn health_probe(
        self: &Arc<Self>,
        timeout: Duration,
    ) -> impl Fn(&SidecarIo) -> Result<ProbeOutcome, String> + Send + Sync + 'static {
        let transport = Arc::clone(self);
        move |_| {
            let guard = match transport.call_lock.try_lock() {
                Ok(guard) => guard,
                Err(TryLockError::Poisoned(error)) => error.into_inner(),
                Err(TryLockError::WouldBlock) => return Ok(ProbeOutcome::Ready),
            };
            let id = format!(
                "muniment-ready-{}",
                transport.next_id.fetch_add(1, Ordering::Relaxed)
            );
            let response = transport.call_locked(
                json!({"id": id, "type": "get_state"}),
                &id,
                timeout,
                guard,
            )?;
            if response.get("type").and_then(Value::as_str) != Some("response")
                || response.get("command").and_then(Value::as_str) != Some("get_state")
                || response.get("success").and_then(Value::as_bool) != Some(true)
            {
                return Err(format!("unexpected Pi readiness response: {response}"));
            }
            Ok(ProbeOutcome::Ready)
        }
    }

    fn call_locked(
        &self,
        command: Value,
        id: &str,
        timeout: Duration,
        _guard: std::sync::MutexGuard<'_, ()>,
    ) -> Result<Value, String> {
        let generation = self
            .io
            .stdin
            .write_line_in_generation(&command.to_string())
            .map_err(|error| error.to_string())?;
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(format!("timed out waiting for Pi RPC response `{id}`"));
            }
            let line = self
                .io
                .stdout
                .read_line_timeout_for_generation(generation, remaining)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("timed out waiting for Pi RPC response `{id}`"))?;
            let frame: Value = serde_json::from_str(&line)
                .map_err(|error| format!("invalid Pi RPC JSON: {error}"))?;
            if frame.get("id").and_then(Value::as_str) == Some(id) {
                return Ok(frame);
            }
            self.route(frame);
        }
    }

    fn route(&self, frame: Value) {
        self.subscribers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|subscriber| subscriber.send(frame.clone()).is_ok());
    }
}

/// Pi RPC is JSONL, not JSON-RPC 2.0. `get_state` is local, side-effect free,
/// and does not contact a model provider, so it is a suitable readiness probe.
/// The lazily-created transport remains the sole stdout consumer across every
/// periodic probe.
pub fn pi_readiness_probe(
    timeout: Duration,
) -> impl Fn(&SidecarIo) -> Result<ProbeOutcome, String> + Send + Sync + 'static {
    PiRpcWiring::new().readiness_probe(timeout)
}
