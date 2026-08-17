#[cfg(target_os = "linux")]
use muniment_core::attach::linux::{AttachFilesystem, InstanceLockError, TerminationSignalWait};
#[cfg(target_os = "linux")]
use muniment_runtime::{
    config_directory, profile_directory, run_runtime_activation, RuntimeActivationExit,
};
#[cfg(target_os = "linux")]
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::sync::mpsc;
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
const INITIAL_WAIT_INTERVAL: Duration = Duration::from_millis(25);
// Limit lock polling to one wakeup every two seconds during long waits.
#[cfg(target_os = "linux")]
const MAX_WAIT_INTERVAL: Duration = Duration::from_secs(2);
#[cfg(target_os = "linux")]
const WAIT_TIMEOUT_ENV: &str = "MUNIMENT_RUNTIME_TEST_WAIT_TIMEOUT_MS";
#[cfg(target_os = "linux")]
const EXIT_AFTER_LOCK_ENV: &str = "MUNIMENT_RUNTIME_TEST_EXIT_AFTER_LOCK";
#[cfg(target_os = "linux")]
const UPGRADE_REFRESH_EXIT_STATUS: i32 = 75;

const HELP: &str = "\
Usage: muniment-runtime [OPTIONS]

Options:
  -h, --help  Print help
  --version   Print version";

fn main() {
    match handle_arguments() {
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => {
            eprintln!("muniment-runtime: {error}");
            std::process::exit(1);
        }
    }

    #[cfg(target_os = "linux")]
    match run() {
        Ok(RuntimeActivationExit::ManagerStop) => {}
        Ok(RuntimeActivationExit::UpgradeRefresh) => {
            std::process::exit(UPGRADE_REFRESH_EXIT_STATUS)
        }
        Err(error) => {
            eprintln!("muniment-runtime: {error}");
            std::process::exit(1);
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("muniment-runtime: Linux is the only supported platform");
        std::process::exit(1);
    }
}

fn handle_arguments() -> Result<bool, String> {
    let mut args = std::env::args_os().skip(1);
    let Some(argument) = args.next() else {
        return Ok(false);
    };
    if argument != "-h" && argument != "--help" && argument != "--version" {
        return Err(format!("unknown argument: {}", argument.to_string_lossy()));
    }
    if let Some(extra) = args.next() {
        return Err(format!("unknown argument: {}", extra.to_string_lossy()));
    }
    if argument == "-h" || argument == "--help" {
        println!("{HELP}");
        return Ok(true);
    }
    println!("{}", env!("CARGO_PKG_VERSION"));
    Ok(true)
}

#[cfg(target_os = "linux")]
fn run() -> Result<RuntimeActivationExit, String> {
    let termination_signal = TerminationSignalWait::new().map_err(|error| error.to_string())?;
    let wait_timeout = test_wait_timeout()?;
    if std::env::var_os(EXIT_AFTER_LOCK_ENV).is_some() {
        return wait_for_instance_lock(wait_timeout).map(|_| RuntimeActivationExit::ManagerStop);
    }
    let runtime_directory = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| "XDG_RUNTIME_DIR is not set".to_owned())?;
    let profile_directory = profile_directory().map_err(|error| error.to_string())?;
    let config_directory = config_directory().map_err(|error| error.to_string())?;
    let takeover_deadline = wait_timeout
        .and_then(|timeout| Instant::now().checked_add(timeout))
        .unwrap_or_else(|| Instant::now() + Duration::from_secs(100 * 365 * 24 * 60 * 60));
    let (stop_tx, stop_rx) = mpsc::channel();
    std::thread::spawn(move || {
        if termination_signal.wait().is_ok() {
            let _ = stop_tx.send(());
        }
    });
    run_runtime_activation(
        runtime_directory,
        profile_directory,
        config_directory,
        takeover_deadline,
        stop_rx,
    )
    .map_err(|error| error.to_string())
}

#[cfg(target_os = "linux")]
fn wait_for_instance_lock(wait_timeout: Option<Duration>) -> Result<(), String> {
    let filesystem = AttachFilesystem::from_environment().map_err(|error| error.to_string())?;
    let started = Instant::now();
    let mut wait_interval = INITIAL_WAIT_INTERVAL;
    let mut reported_wait = false;

    let _instance_lock = loop {
        match filesystem.acquire_instance_lock() {
            Ok(lock) => break lock,
            Err(InstanceLockError::AlreadyHeld) => {
                if !reported_wait {
                    eprintln!("muniment-runtime: waiting for the instance lock");
                    reported_wait = true;
                }
                let sleep_duration = match wait_timeout {
                    Some(timeout) => timeout
                        .checked_sub(started.elapsed())
                        .map(|remaining| wait_interval.min(remaining))
                        .ok_or_else(|| "instance lock wait timed out".to_owned())?,
                    None => wait_interval,
                };
                std::thread::sleep(sleep_duration);
                wait_interval = (wait_interval * 2).min(MAX_WAIT_INTERVAL);
            }
            Err(error) => return Err(error.to_string()),
        }
    };

    Ok(())
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
