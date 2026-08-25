use std::io;
use std::path::Path;

use crate::windows_log_directory_from_local_app_data;

pub const WINDOWS_RUNTIME_LOG_MAX_BYTES: u64 = 256 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowsDiagnosticEvent {
    ActivationFailed,
    ArgumentsInvalid,
    InstanceLockWait,
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
