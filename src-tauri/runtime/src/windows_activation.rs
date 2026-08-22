use std::io;
use std::path::Path;

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

/// Appends one fixed record to the owner-only Windows runtime log.
#[cfg(unix)]
pub fn write_windows_diagnostic(
    log_directory: impl AsRef<Path>,
    event: WindowsDiagnosticEvent,
) -> io::Result<()> {
    muniment_core::user_diagnostics::append_owner_only_record(
        log_directory.as_ref(),
        c"runtime.log",
        WINDOWS_RUNTIME_LOG_MAX_BYTES,
        event.record(),
    )
}

/// Appends one fixed record to the owner-only Windows runtime log.
#[cfg(target_os = "windows")]
pub fn write_windows_diagnostic(
    log_directory: impl AsRef<Path>,
    event: WindowsDiagnosticEvent,
) -> io::Result<()> {
    windows_file::append(log_directory.as_ref(), event.record())
}

#[cfg(target_os = "windows")]
mod windows_file {
    use super::WINDOWS_RUNTIME_LOG_MAX_BYTES;
    use std::fs::{self, OpenOptions};
    use std::io::{self, Seek, SeekFrom, Write};
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;
    use windows_sys::Win32::Storage::FileSystem::{
        LockFileEx, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        FILE_SHARE_WRITE, LOCKFILE_EXCLUSIVE_LOCK,
    };
    use windows_sys::Win32::System::IO::OVERLAPPED;

    pub fn append(directory: &Path, record: &[u8]) -> io::Result<()> {
        match fs::symlink_metadata(directory) {
            Ok(metadata) => validate_directory(&metadata)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir(directory)?,
            Err(error) => return Err(error),
        }
        validate_directory(&fs::symlink_metadata(directory)?)?;

        let path = directory.join("runtime.log");
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            validate_file(&metadata)?;
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&path)?;
        validate_file(&file.metadata()?)?;
        let mut overlapped = OVERLAPPED::default();
        if unsafe {
            LockFileEx(
                file.as_raw_handle(),
                LOCKFILE_EXCLUSIVE_LOCK,
                0,
                u32::MAX,
                u32::MAX,
                &mut overlapped,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if file.metadata()?.len().saturating_add(record.len() as u64)
            > WINDOWS_RUNTIME_LOG_MAX_BYTES
        {
            file.set_len(0)?;
        }
        file.seek(SeekFrom::End(0))?;
        file.write_all(record)?;
        file.sync_data()
    }

    fn validate_directory(metadata: &fs::Metadata) -> io::Result<()> {
        if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "diagnostic log directory is unsafe",
            ));
        }
        Ok(())
    }

    fn validate_file(metadata: &fs::Metadata) -> io::Result<()> {
        if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "diagnostic log path is unsafe",
            ));
        }
        Ok(())
    }
}
