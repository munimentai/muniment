#[cfg(target_os = "linux")]
use muniment_core::attach::linux::{AttachFilesystem, InstanceLockError, TerminationSignalWait};
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
    if let Err(error) = run() {
        eprintln!("muniment-runtime: {error}");
        std::process::exit(1);
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
fn run() -> Result<(), String> {
    let termination_signal = TerminationSignalWait::new().map_err(|error| error.to_string())?;
    let wait_timeout = test_wait_timeout()?;
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

    if std::env::var_os(EXIT_AFTER_LOCK_ENV).is_some() {
        return Ok(());
    }
    termination_signal.wait().map_err(|error| error.to_string())
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
