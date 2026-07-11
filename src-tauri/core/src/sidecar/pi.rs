use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

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

/// Pi RPC is JSONL, not JSON-RPC 2.0. `get_state` is local, side-effect free,
/// and does not contact a model provider, so it is a suitable readiness probe.
pub fn pi_readiness_probe(
    timeout: Duration,
) -> impl Fn(&SidecarIo) -> Result<ProbeOutcome, String> + Send + Sync + 'static {
    let next_id = Arc::new(AtomicU64::new(1));
    move |io| {
        let id = format!("muniment-ready-{}", next_id.fetch_add(1, Ordering::Relaxed));
        io.stdin
            .write_line(&json!({"id": id, "type": "get_state"}).to_string())
            .map_err(|error| error.to_string())?;

        let line = io
            .stdout
            .read_line_timeout(timeout)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "Pi closed stdout before its readiness response".to_string())?;
        let response: Value =
            serde_json::from_str(&line).map_err(|error| format!("invalid Pi RPC JSON: {error}"))?;
        if response.get("id").and_then(Value::as_str) != Some(id.as_str())
            || response.get("type").and_then(Value::as_str) != Some("response")
            || response.get("command").and_then(Value::as_str) != Some("get_state")
            || response.get("success").and_then(Value::as_bool) != Some(true)
        {
            return Err(format!("unexpected Pi readiness response: {response}"));
        }
        Ok(ProbeOutcome::Ready)
    }
}
