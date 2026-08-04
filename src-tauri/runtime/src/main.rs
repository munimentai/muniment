#[cfg(target_os = "linux")]
use muniment_core::attach::linux::{AttachFilesystem, InstanceLockError};
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
const WAIT_INTERVAL: Duration = Duration::from_millis(25);
#[cfg(target_os = "linux")]
const WAIT_TIMEOUT_ENV: &str = "MUNIMENT_RUNTIME_TEST_WAIT_TIMEOUT_MS";
#[cfg(target_os = "linux")]
const EXIT_AFTER_LOCK_ENV: &str = "MUNIMENT_RUNTIME_TEST_EXIT_AFTER_LOCK";

#[cfg(target_os = "linux")]
fn main() {
    if let Err(error) = run() {
        eprintln!("muniment-runtime: {error}");
        std::process::exit(1);
    }
}

#[cfg(target_os = "linux")]
fn run() -> Result<(), String> {
    let wait_timeout = test_wait_timeout()?;
    let filesystem = AttachFilesystem::from_environment().map_err(|error| error.to_string())?;
    let started = Instant::now();

    let _instance_lock = loop {
        match filesystem.acquire_instance_lock() {
            Ok(lock) => break lock,
            Err(InstanceLockError::AlreadyHeld) => {
                if wait_timeout.is_some_and(|timeout| started.elapsed() >= timeout) {
                    return Err("instance lock wait timed out".to_owned());
                }
                std::thread::sleep(WAIT_INTERVAL);
            }
            Err(error) => return Err(error.to_string()),
        }
    };

    if std::env::var_os(EXIT_AFTER_LOCK_ENV).is_some() {
        return Ok(());
    }
    loop {
        std::thread::park();
    }
}

#[cfg(target_os = "linux")]
fn test_wait_timeout() -> Result<Option<Duration>, String> {
    let Some(value) = std::env::var_os(WAIT_TIMEOUT_ENV) else {
        return Ok(None);
    };
    let value = value
        .to_str()
        .ok_or_else(|| format!("{WAIT_TIMEOUT_ENV} must contain UTF-8"))?;
    let milliseconds = value
        .parse::<u64>()
        .map_err(|_| format!("{WAIT_TIMEOUT_ENV} must be an unsigned integer"))?;
    Ok(Some(Duration::from_millis(milliseconds)))
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("muniment-runtime: Linux is the only supported platform");
    std::process::exit(1);
}
