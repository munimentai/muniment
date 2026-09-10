use muniment_core::runtime_eprintln as eprintln;
use std::io;
use std::path::Path;
use std::time::Duration;

use crate::install_lock::{self, InstallLockAcquireError};
use crate::start_record::{self, Start, StartDecision};
use crate::windows_log_directory_from_local_app_data;

const RECORD_NAME: &str = "windows-starts";
pub const WINDOWS_RUNTIME_LOG_MAX_BYTES: u64 = 256 * 1024;

pub type WindowsStartDecision = StartDecision;
pub type WindowsStart = Start;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowsActivationExit {
    Orderly(i32),
    Failed(i32),
}

#[derive(Debug)]
pub enum ClearWindowsCrashWindowError {
    Lock(InstallLockAcquireError),
    Record(io::Error),
}

impl std::fmt::Display for ClearWindowsCrashWindowError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lock(error) => write!(formatter, "install lock failed: {error}"),
            Self::Record(error) => write!(formatter, "start record clear failed: {error}"),
        }
    }
}

impl std::error::Error for ClearWindowsCrashWindowError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Lock(error) => Some(error),
            Self::Record(error) => Some(error),
        }
    }
}

/// Clears the Windows crash window under the per-user install lock.
pub fn clear_windows_crash_window(
    state_directory: impl AsRef<Path>,
    bounded_wait: Duration,
) -> Result<(), ClearWindowsCrashWindowError> {
    let state_directory = state_directory.as_ref();
    let _guard = install_lock::acquire(state_directory, bounded_wait)
        .map_err(ClearWindowsCrashWindowError::Lock)?;
    start_record::clear(state_directory, RECORD_NAME).map_err(ClearWindowsCrashWindowError::Record)
}

/// Records a Windows runtime start before activation opens runtime state.
pub fn record_windows_start(
    state_directory: impl AsRef<Path>,
) -> io::Result<(WindowsStartDecision, WindowsStart)> {
    start_record::record_start(state_directory, RECORD_NAME)
}

/// Removes a recorded start after an orderly runtime exit.
pub fn record_windows_orderly_exit(
    state_directory: impl AsRef<Path>,
    start: WindowsStart,
) -> io::Result<()> {
    start_record::record_orderly_exit(state_directory, RECORD_NAME, start)
}

/// Records a failed activation and reports when Task Scheduler must stop restarting it.
pub fn record_windows_failed_exit(
    state_directory: impl AsRef<Path>,
    start: WindowsStart,
) -> io::Result<WindowsStartDecision> {
    start_record::record_failed_exit(state_directory, RECORD_NAME, start)
}

/// Records a Windows start and its failed activation without writing a diagnostic.
pub fn record_windows_failed_activation(state_directory: impl AsRef<Path>) -> i32 {
    let state_directory = state_directory.as_ref();
    let start = match record_windows_start(state_directory) {
        Ok((WindowsStartDecision::StopRestartLoop, _)) => return 0,
        Ok((WindowsStartDecision::Run, start)) => start,
        Err(error) => {
            eprintln!("muniment-runtime: start record failed: {error}");
            return 1;
        }
    };
    match record_windows_failed_exit(state_directory, start) {
        Ok(WindowsStartDecision::StopRestartLoop) => 0,
        Ok(WindowsStartDecision::Run) => 1,
        Err(error) => {
            eprintln!("muniment-runtime: start record failed: {error}");
            1
        }
    }
}

/// Records a Windows activation and maps its outcome to a process exit status.
#[cfg(any(unix, target_os = "windows"))]
pub fn run_recorded_windows_activation(
    state_directory: impl AsRef<Path>,
    local_app_data: impl AsRef<Path>,
    activate: impl FnOnce() -> WindowsActivationExit,
) -> i32 {
    const FAILURE_EXIT_STATUS: i32 = 1;
    const SUCCESS_EXIT_STATUS: i32 = 0;

    let state_directory = state_directory.as_ref();
    let local_app_data = local_app_data.as_ref();
    let start = match record_windows_start(state_directory) {
        Ok((WindowsStartDecision::StopRestartLoop, _)) => {
            let _ = write_windows_diagnostic(
                local_app_data,
                WindowsDiagnosticEvent::RestartLoopStopped,
            );
            return SUCCESS_EXIT_STATUS;
        }
        Ok((WindowsStartDecision::Run, start)) => start,
        Err(error) => {
            eprintln!("muniment-runtime: start record failed: {error}");
            let _ =
                write_windows_diagnostic(local_app_data, WindowsDiagnosticEvent::StartRecordFailed);
            return FAILURE_EXIT_STATUS;
        }
    };

    match activate() {
        WindowsActivationExit::Orderly(status) => {
            if let Err(error) = record_windows_orderly_exit(state_directory, start) {
                eprintln!("muniment-runtime: start record failed: {error}");
                let _ = write_windows_diagnostic(
                    local_app_data,
                    WindowsDiagnosticEvent::StartRecordFailed,
                );
                return FAILURE_EXIT_STATUS;
            }
            status
        }
        WindowsActivationExit::Failed(status) => {
            match record_windows_failed_exit(state_directory, start) {
                Ok(WindowsStartDecision::StopRestartLoop) => {
                    let _ = write_windows_diagnostic(
                        local_app_data,
                        WindowsDiagnosticEvent::RestartLoopStopped,
                    );
                    SUCCESS_EXIT_STATUS
                }
                Ok(WindowsStartDecision::Run) => {
                    let _ = write_windows_diagnostic(
                        local_app_data,
                        WindowsDiagnosticEvent::ActivationFailed,
                    );
                    status
                }
                Err(error) => {
                    eprintln!("muniment-runtime: start record failed: {error}");
                    let _ = write_windows_diagnostic(
                        local_app_data,
                        WindowsDiagnosticEvent::StartRecordFailed,
                    );
                    FAILURE_EXIT_STATUS
                }
            }
        }
    }
}

#[cfg(test)]
fn record_windows_start_millis(
    state_directory: &Path,
    timestamp_millis: u64,
) -> io::Result<(WindowsStartDecision, WindowsStart)> {
    start_record::record_start_millis(state_directory, RECORD_NAME, timestamp_millis)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowsDiagnosticEvent {
    ActivationFailed,
    ArgumentsInvalid,
    InstanceLockWait,
    InstallLockUnavailable,
    RuntimeTaskRegistrationFailed,
    RuntimeTaskStartFailed,
    StartRecordFailed,
    RestartLoopStopped,
}

impl WindowsDiagnosticEvent {
    fn record(self) -> &'static [u8] {
        match self {
            Self::ActivationFailed => {
                b"event=activation_failed message=runtime activation failed\n"
            }
            Self::ArgumentsInvalid => {
                b"event=arguments_invalid message=runtime arguments invalid\n"
            }
            Self::InstanceLockWait => {
                b"event=instance_lock_wait message=runtime instance lock is held\n"
            }
            Self::InstallLockUnavailable => {
                b"event=install_lock_unavailable message=install lock unavailable\n"
            }
            Self::RuntimeTaskRegistrationFailed => {
                b"event=runtime_task_registration_failed message=runtime task registration failed\n"
            }
            Self::RuntimeTaskStartFailed => {
                b"event=runtime_task_start_failed message=runtime task start failed\n"
            }
            Self::StartRecordFailed => {
                b"event=start_record_failed message=start record update failed\n"
            }
            Self::RestartLoopStopped => {
                b"event=restart_loop_stopped message=runtime restart limit reached\n"
            }
        }
    }
}

/// Appends one fixed record below the supplied local application data root.
#[cfg(unix)]
pub fn write_windows_diagnostic(
    local_app_data: impl AsRef<Path>,
    event: WindowsDiagnosticEvent,
) -> io::Result<()> {
    use std::fs;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    let directory = windows_log_directory_from_local_app_data(local_app_data.as_ref())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let application_directory = directory.parent().expect("the resolver adds two segments");
    match fs::symlink_metadata(application_directory) {
        Ok(metadata) if metadata.is_dir() && metadata.permissions().mode() & 0o777 == 0o700 => {}
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "diagnostic application directory is unsafe",
            ))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(application_directory)?;
        }
        Err(error) => return Err(error),
    }
    muniment_core::user_diagnostics::append_owner_only_record(
        &directory,
        c"runtime.log",
        WINDOWS_RUNTIME_LOG_MAX_BYTES,
        event.record(),
    )
}

/// Appends one fixed record below the supplied local application data root.
#[cfg(target_os = "windows")]
pub fn write_windows_diagnostic(
    local_app_data: impl AsRef<Path>,
    event: WindowsDiagnosticEvent,
) -> io::Result<()> {
    let directory = windows_log_directory_from_local_app_data(local_app_data.as_ref())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    muniment_core::windows_user_diagnostics::append_owner_only_record(
        local_app_data.as_ref(),
        &directory,
        WINDOWS_RUNTIME_LOG_MAX_BYTES,
        event.record(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::start_record::FAILURE_WINDOW;
    use std::fs;
    use std::path::PathBuf;

    fn directory(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "muniment-runtime-windows-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn fifth_failed_start_stops_the_restart_loop() {
        let directory = directory("threshold");
        for timestamp in 0..4 {
            let (_, start) = record_windows_start_millis(&directory, 1_000 + timestamp).unwrap();
            assert_eq!(
                record_windows_failed_exit(&directory, start).unwrap(),
                WindowsStartDecision::Run
            );
        }
        let (_, start) = record_windows_start_millis(&directory, 1_004).unwrap();
        assert_eq!(
            record_windows_failed_exit(&directory, start).unwrap(),
            WindowsStartDecision::StopRestartLoop
        );
        let contents = fs::read_to_string(directory.join(RECORD_NAME)).unwrap();
        assert_eq!(contents, "needs_attention=true\n");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn an_expired_failure_does_not_reach_the_threshold() {
        let directory = directory("expired");
        for timestamp in 0..4 {
            let (_, start) = record_windows_start_millis(&directory, timestamp).unwrap();
            record_windows_failed_exit(&directory, start).unwrap();
        }
        let (_, start) =
            record_windows_start_millis(&directory, FAILURE_WINDOW.as_millis() as u64).unwrap();
        assert_eq!(
            record_windows_failed_exit(&directory, start).unwrap(),
            WindowsStartDecision::Run
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn an_orderly_exit_does_not_count_as_a_failure() {
        let directory = directory("orderly");
        for timestamp in 0..4 {
            let (_, start) = record_windows_start_millis(&directory, timestamp).unwrap();
            record_windows_failed_exit(&directory, start).unwrap();
        }
        let (_, orderly_start) = record_windows_start_millis(&directory, 4).unwrap();
        record_windows_orderly_exit(&directory, orderly_start).unwrap();
        let (_, start) = record_windows_start_millis(&directory, 5).unwrap();
        assert_eq!(
            record_windows_failed_exit(&directory, start).unwrap(),
            WindowsStartDecision::StopRestartLoop
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_start_after_the_limit_opens_a_new_window() {
        let directory = directory("new-window");
        for timestamp in 0..5 {
            let (_, start) = record_windows_start_millis(&directory, timestamp).unwrap();
            record_windows_failed_exit(&directory, start).unwrap();
        }
        assert_eq!(
            record_windows_start_millis(&directory, 6).unwrap().0,
            WindowsStartDecision::Run
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
