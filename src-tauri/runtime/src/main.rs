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
#[cfg(any(target_os = "linux", target_os = "macos"))]
const UPGRADE_REFRESH_EXIT_STATUS: i32 = 75;
const FAILURE_EXIT_STATUS: i32 = 1;
const SUCCESS_EXIT_STATUS: i32 = 0;
const MACOS_TEST_EXIT_ENV: &str = "MUNIMENT_RUNTIME_TEST_MACOS_ACTIVATION_EXIT";

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
            #[cfg(target_os = "macos")]
            record_macos_diagnostic(muniment_runtime::MacosDiagnosticEvent::ArgumentsInvalid);
            #[cfg(target_os = "windows")]
            record_windows_diagnostic(muniment_runtime::WindowsDiagnosticEvent::ArgumentsInvalid);
            std::process::exit(1);
        }
    }

    #[cfg(target_os = "windows")]
    {
        use muniment_runtime::{
            profile_directory, record_windows_failed_activation, run_recorded_windows_activation,
            run_windows_attach_activation, windows_local_app_data, SystemWindowsAttachFactory,
        };

        let state_directory = match profile_directory() {
            Ok(directory) => directory,
            Err(error) => {
                eprintln!("muniment-runtime: {error}");
                record_windows_diagnostic(
                    muniment_runtime::WindowsDiagnosticEvent::StartRecordFailed,
                );
                std::process::exit(FAILURE_EXIT_STATUS);
            }
        };
        let local_app_data = match windows_local_app_data() {
            Ok(directory) => directory,
            Err(error) => {
                eprintln!("muniment-runtime: {error}");
                std::process::exit(record_windows_failed_activation(&state_directory));
            }
        };
        let factory =
            SystemWindowsAttachFactory::new(&state_directory, std::time::Duration::from_secs(2));
        let diagnostics = MainWindowsDiagnosticSink {
            local_app_data: &local_app_data,
        };
        let (_stop_sender, stop_receiver) = std::sync::mpsc::channel();
        std::process::exit(run_recorded_windows_activation(
            &state_directory,
            &local_app_data,
            || run_windows_attach_activation(&factory, stop_receiver, &diagnostics),
        ));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if let Some(exit) = test_macos_activation_exit() {
        std::process::exit(run_recorded_macos_activation(|| match macos_activation() {
            MacosActivationExit::Orderly(_) => exit(),
            failed => failed,
        }));
    }

    #[cfg(target_os = "linux")]
    match run(RuntimeDirectorySource::Environment) {
        Ok(RuntimeActivationExit::ManagerStop) => {}
        Ok(RuntimeActivationExit::UpgradeRefresh) => {
            std::process::exit(UPGRADE_REFRESH_EXIT_STATUS)
        }
        Err(error) => {
            eprintln!("muniment-runtime: {error}");
            std::process::exit(1);
        }
    }

    #[cfg(target_os = "macos")]
    std::process::exit(run_recorded_macos_activation(macos_activation));

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        eprintln!("muniment-runtime: Linux is the only supported platform");
        std::process::exit(1);
    }
}

#[derive(Clone, Copy)]
enum MacosActivationExit {
    Orderly(i32),
    Failed(i32),
}

fn run_recorded_macos_activation(activate: impl FnOnce() -> MacosActivationExit) -> i32 {
    use muniment_runtime::{
        profile_directory, record_macos_failed_exit, record_macos_orderly_exit, record_macos_start,
        MacosDiagnosticEvent, MacosStartDecision,
    };

    let state_directory = match profile_directory() {
        Ok(directory) => directory,
        Err(error) => {
            eprintln!("muniment-runtime: {error}");
            record_macos_diagnostic(MacosDiagnosticEvent::StartRecordFailed);
            return FAILURE_EXIT_STATUS;
        }
    };
    let start = match record_macos_start(&state_directory) {
        Ok((MacosStartDecision::StopRestartLoop, _)) => {
            record_macos_diagnostic(MacosDiagnosticEvent::RestartLoopStopped);
            return SUCCESS_EXIT_STATUS;
        }
        Ok((MacosStartDecision::Run, start)) => start,
        Err(error) => {
            eprintln!("muniment-runtime: start record failed: {error}");
            record_macos_diagnostic(MacosDiagnosticEvent::StartRecordFailed);
            return FAILURE_EXIT_STATUS;
        }
    };
    match activate() {
        MacosActivationExit::Orderly(status) => {
            if let Err(error) = record_macos_orderly_exit(state_directory, start) {
                eprintln!("muniment-runtime: start record failed: {error}");
                record_macos_diagnostic(MacosDiagnosticEvent::StartRecordFailed);
                return FAILURE_EXIT_STATUS;
            }
            status
        }
        MacosActivationExit::Failed(status) => {
            match record_macos_failed_exit(state_directory, start) {
                Ok(MacosStartDecision::StopRestartLoop) => {
                    record_macos_diagnostic(MacosDiagnosticEvent::RestartLoopStopped);
                    SUCCESS_EXIT_STATUS
                }
                Ok(MacosStartDecision::Run) => {
                    record_macos_diagnostic(MacosDiagnosticEvent::ActivationFailed);
                    status
                }
                Err(error) => {
                    eprintln!("muniment-runtime: start record failed: {error}");
                    record_macos_diagnostic(MacosDiagnosticEvent::StartRecordFailed);
                    FAILURE_EXIT_STATUS
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn record_macos_diagnostic(event: muniment_runtime::MacosDiagnosticEvent) {
    if let Ok(directory) = muniment_runtime::effective_user_macos_log_directory() {
        let _ = muniment_runtime::write_macos_diagnostic(directory, event);
    } else {
        muniment_runtime::emit_macos_unified_log(event);
    }
}

#[cfg(target_os = "windows")]
struct MainWindowsDiagnosticSink<'a> {
    local_app_data: &'a std::path::Path,
}

#[cfg(target_os = "windows")]
impl muniment_runtime::WindowsDiagnosticSink for MainWindowsDiagnosticSink<'_> {
    fn record(&self, event: muniment_runtime::WindowsDiagnosticEvent) {
        let _ = muniment_runtime::write_windows_diagnostic(self.local_app_data, event);
    }
}

#[cfg(target_os = "windows")]
fn record_windows_diagnostic(event: muniment_runtime::WindowsDiagnosticEvent) {
    if let Ok(local_app_data) = muniment_runtime::windows_local_app_data() {
        let _ = muniment_runtime::write_windows_diagnostic(local_app_data, event);
    }
}

#[cfg(not(target_os = "macos"))]
fn record_macos_diagnostic(_event: muniment_runtime::MacosDiagnosticEvent) {}

fn test_macos_activation_exit() -> Option<impl FnOnce() -> MacosActivationExit> {
    let exit = match std::env::var_os(MACOS_TEST_EXIT_ENV)?.to_str()? {
        "orderly" => MacosActivationExit::Orderly(SUCCESS_EXIT_STATUS),
        "failed" => MacosActivationExit::Failed(FAILURE_EXIT_STATUS),
        _ => return None,
    };
    Some(move || exit)
}

#[cfg(target_os = "linux")]
fn macos_activation() -> MacosActivationExit {
    match run(RuntimeDirectorySource::Profile) {
        Ok(RuntimeActivationExit::ManagerStop) => MacosActivationExit::Orderly(SUCCESS_EXIT_STATUS),
        Ok(RuntimeActivationExit::UpgradeRefresh) => {
            MacosActivationExit::Orderly(UPGRADE_REFRESH_EXIT_STATUS)
        }
        Err(error) => {
            eprintln!("muniment-runtime: {error}");
            MacosActivationExit::Failed(FAILURE_EXIT_STATUS)
        }
    }
}

#[cfg(target_os = "macos")]
fn macos_activation() -> MacosActivationExit {
    use muniment_core::attach::TerminationSignalWait;
    use muniment_runtime::{
        config_directory, profile_directory, run_windows_attach_activation_with_upgrade_watch,
        MacosUpgradeWatchTestControl, SystemMacosAttachFactory, WindowsActivationExit,
    };

    let termination_signal = match TerminationSignalWait::new() {
        Ok(signal) => signal,
        Err(error) => {
            eprintln!("muniment-runtime: {error}");
            return MacosActivationExit::Failed(FAILURE_EXIT_STATUS);
        }
    };
    let profile_directory = match profile_directory() {
        Ok(directory) => directory,
        Err(error) => {
            eprintln!("muniment-runtime: {error}");
            return MacosActivationExit::Failed(FAILURE_EXIT_STATUS);
        }
    };
    let config_directory = match config_directory() {
        Ok(directory) => directory,
        Err(error) => {
            eprintln!("muniment-runtime: {error}");
            return MacosActivationExit::Failed(FAILURE_EXIT_STATUS);
        }
    };
    let runtime_executable = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("muniment-runtime: {error}");
            return MacosActivationExit::Failed(FAILURE_EXIT_STATUS);
        }
    };
    let factory = SystemMacosAttachFactory::new(profile_directory, config_directory);
    let (stop_tx, stop_rx) = std::sync::mpsc::channel();
    if std::env::var_os(MACOS_TEST_EXIT_ENV).is_some() {
        let _ = stop_tx.send(());
    }
    std::thread::spawn(move || {
        if termination_signal.wait().is_ok() {
            let _ = stop_tx.send(());
        }
    });

    match run_windows_attach_activation_with_upgrade_watch(
        &factory,
        stop_rx,
        &MainMacosDiagnosticSink,
        MacosUpgradeWatchTestControl {
            path: runtime_executable,
            poll_interval: std::time::Duration::from_secs(1),
            ready: None,
            refresh_detected: None,
        },
    ) {
        WindowsActivationExit::Orderly(status) => MacosActivationExit::Orderly(status),
        WindowsActivationExit::Failed(status) => MacosActivationExit::Failed(status),
    }
}

#[cfg(target_os = "macos")]
struct MainMacosDiagnosticSink;

#[cfg(target_os = "macos")]
impl muniment_runtime::WindowsDiagnosticSink for MainMacosDiagnosticSink {
    fn record(&self, event: muniment_runtime::WindowsDiagnosticEvent) {
        let event = match event {
            muniment_runtime::WindowsDiagnosticEvent::InstanceLockWait => {
                Some(muniment_runtime::MacosDiagnosticEvent::InstanceLockWait)
            }
            muniment_runtime::WindowsDiagnosticEvent::ActivationFailed => {
                Some(muniment_runtime::MacosDiagnosticEvent::ActivationFailed)
            }
            _ => None,
        };
        if let Some(event) = event {
            record_macos_diagnostic(event);
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
enum RuntimeDirectorySource {
    Environment,
    Profile,
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
fn run(runtime_directory_source: RuntimeDirectorySource) -> Result<RuntimeActivationExit, String> {
    let wait_timeout = test_wait_timeout()?;
    if std::env::var_os(EXIT_AFTER_LOCK_ENV).is_some() {
        return wait_for_instance_lock(wait_timeout).map(|_| RuntimeActivationExit::ManagerStop);
    }
    let profile_directory = profile_directory().map_err(|error| error.to_string())?;
    let runtime_directory = match runtime_directory_source {
        RuntimeDirectorySource::Environment => std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| "XDG_RUNTIME_DIR is not set".to_owned())?,
        RuntimeDirectorySource::Profile => profile_directory.clone(),
    };
    let config_directory = config_directory().map_err(|error| error.to_string())?;
    let filesystem = AttachFilesystem::from_runtime_directory(&runtime_directory)
        .map_err(|error| error.to_string())?;
    eprintln!(
        "muniment-runtime: started version={} state_directory={} endpoint={}",
        env!("CARGO_PKG_VERSION"),
        profile_directory.display(),
        filesystem.endpoint_path().display()
    );
    drop(filesystem);
    let termination_signal = TerminationSignalWait::new().map_err(|error| error.to_string())?;
    let takeover_deadline = wait_timeout
        .and_then(|timeout| Instant::now().checked_add(timeout))
        .unwrap_or_else(|| Instant::now() + Duration::from_secs(100 * 365 * 24 * 60 * 60));
    let (stop_tx, stop_rx) = mpsc::channel();
    if std::env::var_os(MACOS_TEST_EXIT_ENV).is_some() {
        let _ = stop_tx.send(());
    }
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
                    #[cfg(target_os = "macos")]
                    record_macos_diagnostic(
                        muniment_runtime::MacosDiagnosticEvent::InstanceLockWait,
                    );
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
