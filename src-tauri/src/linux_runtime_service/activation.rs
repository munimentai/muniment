use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use muniment_core::attach::linux::{AttachFilesystem, InstanceLockError};

const RETRY_INTERVAL: Duration = Duration::from_millis(25);

pub(crate) fn activate_runtime(
    filesystem: &AttachFilesystem,
    start: impl FnOnce() -> Result<(), String>,
    mut connected: impl FnMut() -> bool,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| "The runtime start timeout is invalid.".to_owned())?;
    let startup_timeout = "The runtime start timed out while waiting for the startup lock.";
    let _startup_lock = loop {
        if connected() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(startup_timeout.into());
        }
        match filesystem.acquire_startup_lock() {
            Ok(lock) => break lock,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => return Err(format!("Muniment could not lock runtime startup: {error}")),
        }
        wait_for_retry(deadline, startup_timeout)?;
    };
    if connected() {
        return Ok(());
    }
    let connection_timeout = match filesystem.acquire_instance_lock() {
        Ok(lock) => {
            // Only the runtime owns the endpoint. Release the probe lock before the child starts.
            drop(lock);
            start()?;
            "The runtime start timed out while waiting for the new runtime clients."
        }
        // A service or another desktop already started the runtime. Wait for its client handshake.
        Err(InstanceLockError::AlreadyHeld) => {
            "The runtime start timed out while waiting for the running runtime clients."
        }
        Err(error) => return Err(error.to_string()),
    };
    while !connected() {
        wait_for_retry(deadline, connection_timeout)?;
    }
    Ok(())
}

fn wait_for_retry(deadline: Instant, error: &str) -> Result<(), String> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(error.into());
    }
    std::thread::sleep(RETRY_INTERVAL.min(remaining));
    Ok(())
}

pub(crate) fn spawn_runtime(executable: &Path) -> Result<Child, String> {
    Command::new(executable)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .map_err(|error| format!("Muniment could not start its runtime: {error}"))
}
