use std::fs::{self, OpenOptions};
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
#[cfg(unix)]
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const FAILURE_LIMIT: usize = 5;
const FAILURE_WINDOW: Duration = Duration::from_secs(5 * 60);
const RECORD_NAME: &str = "macos-starts";
pub const MACOS_RUNTIME_LOG_MAX_BYTES: u64 = 256 * 1024;
const RUNTIME_LOG_NAME: &str = "runtime.log";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MacosDiagnosticEvent {
    ActivationFailed,
    ArgumentsInvalid,
    InstanceLockWait,
    StartRecordFailed,
    RestartLoopStopped,
}

impl MacosDiagnosticEvent {
    fn record(self) -> &'static [u8] {
        match self {
            Self::ActivationFailed => b"event=activation_failed message=runtime activation failed\n",
            Self::ArgumentsInvalid => b"event=arguments_invalid message=runtime arguments invalid\n",
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

/// Appends one fixed record and truncates the log before it exceeds 256 KiB.
#[cfg(unix)]
pub fn write_macos_diagnostic(
    log_directory: impl AsRef<Path>,
    event: MacosDiagnosticEvent,
) -> io::Result<()> {
    let log_directory = log_directory.as_ref();
    match fs::symlink_metadata(log_directory) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(log_directory)?;
            fs::set_permissions(log_directory, fs::Permissions::from_mode(0o700))?;
        }
        Err(error) => return Err(error),
    }

    let mut directory_options = OpenOptions::new();
    directory_options
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let directory = directory_options.open(log_directory)?;
    validate_owner_only_directory(&directory.metadata()?)?;

    let name = std::ffi::CString::new(RUNTIME_LOG_NAME).unwrap();
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDWR
                | libc::O_APPEND
                | libc::O_CREAT
                | libc::O_NOFOLLOW
                | libc::O_CLOEXEC,
            0o600,
        )
    };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut file = unsafe { fs::File::from_raw_fd(descriptor) };
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "runtime log path is unsafe",
        ));
    }
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let metadata = file.metadata()?;
    let record = event.record();
    if metadata.len().saturating_add(record.len() as u64) > MACOS_RUNTIME_LOG_MAX_BYTES {
        file.set_len(0)?;
    }
    file.write_all(record)?;
    file.sync_data()
}

#[cfg(unix)]
fn validate_owner_only_directory(metadata: &fs::Metadata) -> io::Result<()> {
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "runtime log directory is unsafe",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MacosStartDecision {
    Run,
    StopRestartLoop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MacosStart {
    timestamp_millis: u64,
}

#[derive(Default)]
struct StartRecord {
    failures: Vec<u64>,
    needs_attention: bool,
}

/// Records a macOS runtime start before activation opens runtime state.
pub fn record_macos_start(
    state_directory: impl AsRef<Path>,
) -> io::Result<(MacosStartDecision, MacosStart)> {
    record_macos_start_at(state_directory.as_ref(), SystemTime::now())
}

/// Removes a recorded start after an orderly runtime exit.
pub fn record_macos_orderly_exit(
    state_directory: impl AsRef<Path>,
    start: MacosStart,
) -> io::Result<()> {
    let path = state_directory.as_ref().join(RECORD_NAME);
    let mut record = read_record(&path)?;
    if let Some(index) = record
        .failures
        .iter()
        .rposition(|timestamp| *timestamp == start.timestamp_millis)
    {
        record.failures.remove(index);
    }
    write_record(&path, &record)
}

/// Records a failed activation and reports when launchd must stop restarting it.
pub fn record_macos_failed_exit(
    state_directory: impl AsRef<Path>,
    start: MacosStart,
) -> io::Result<MacosStartDecision> {
    let path = state_directory.as_ref().join(RECORD_NAME);
    let mut record = read_record(&path)?;
    let decision = if record.failures.contains(&start.timestamp_millis)
        && record.failures.len() >= FAILURE_LIMIT
    {
        record.failures.clear();
        record.needs_attention = true;
        MacosStartDecision::StopRestartLoop
    } else {
        MacosStartDecision::Run
    };
    write_record(&path, &record)?;
    Ok(decision)
}

fn record_macos_start_at(
    state_directory: &Path,
    now: SystemTime,
) -> io::Result<(MacosStartDecision, MacosStart)> {
    let timestamp_millis = now
        .duration_since(UNIX_EPOCH)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "system time is invalid"))?
        .as_millis()
        .try_into()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "system time is invalid"))?;
    record_macos_start_millis(state_directory, timestamp_millis)
}

fn record_macos_start_millis(
    state_directory: &Path,
    timestamp_millis: u64,
) -> io::Result<(MacosStartDecision, MacosStart)> {
    fs::create_dir_all(state_directory)?;
    let path = state_directory.join(RECORD_NAME);
    let mut record = read_record(&path)?;
    let window_millis: u64 = FAILURE_WINDOW.as_millis().try_into().unwrap();
    record.failures.retain(|failure| {
        *failure <= timestamp_millis && timestamp_millis - *failure < window_millis
    });
    let decision = if record.failures.len() >= FAILURE_LIMIT {
        record.failures.clear();
        record.needs_attention = true;
        MacosStartDecision::StopRestartLoop
    } else {
        record.failures.push(timestamp_millis);
        MacosStartDecision::Run
    };
    write_record(&path, &record)?;
    Ok((decision, MacosStart { timestamp_millis }))
}

fn read_record(path: &Path) -> io::Result<StartRecord> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(StartRecord::default()),
        Err(error) => return Err(error),
    };
    let mut record = StartRecord::default();
    for line in contents.lines() {
        if line == "needs_attention=true" {
            record.needs_attention = true;
        } else if let Some(timestamp) = line.strip_prefix("failure=") {
            if let Ok(timestamp) = timestamp.parse() {
                record.failures.push(timestamp);
            }
        }
    }
    Ok(record)
}

fn write_record(path: &Path, record: &StartRecord) -> io::Result<()> {
    let temporary_path = temporary_path(path);
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&temporary_path)?;
    if record.needs_attention {
        writeln!(file, "needs_attention=true")?;
    }
    for timestamp in &record.failures {
        writeln!(file, "failure={timestamp}")?;
    }
    file.sync_all()?;
    fs::rename(temporary_path, path)
}

fn temporary_path(path: &Path) -> PathBuf {
    path.with_extension("tmp")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "muniment-runtime-{name}-{}-{:?}",
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
            let (_, start) = record_macos_start_millis(&directory, 1_000 + timestamp).unwrap();
            assert_eq!(
                record_macos_failed_exit(&directory, start).unwrap(),
                MacosStartDecision::Run
            );
        }
        let (_, start) = record_macos_start_millis(&directory, 1_004).unwrap();
        assert_eq!(
            record_macos_failed_exit(&directory, start).unwrap(),
            MacosStartDecision::StopRestartLoop
        );
        let contents = fs::read_to_string(directory.join(RECORD_NAME)).unwrap();
        assert_eq!(contents, "needs_attention=true\n");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn an_expired_failure_does_not_reach_the_threshold() {
        let directory = directory("expired");
        for timestamp in 0..4 {
            let (_, start) = record_macos_start_millis(&directory, timestamp).unwrap();
            record_macos_failed_exit(&directory, start).unwrap();
        }
        let (_, start) =
            record_macos_start_millis(&directory, FAILURE_WINDOW.as_millis() as u64).unwrap();
        assert_eq!(
            record_macos_failed_exit(&directory, start).unwrap(),
            MacosStartDecision::Run
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn an_orderly_exit_does_not_count_as_a_failure() {
        let directory = directory("orderly");
        for timestamp in 0..4 {
            let (_, start) = record_macos_start_millis(&directory, timestamp).unwrap();
            record_macos_failed_exit(&directory, start).unwrap();
        }
        let (_, orderly_start) = record_macos_start_millis(&directory, 4).unwrap();
        record_macos_orderly_exit(&directory, orderly_start).unwrap();
        let (_, start) = record_macos_start_millis(&directory, 5).unwrap();
        assert_eq!(
            record_macos_failed_exit(&directory, start).unwrap(),
            MacosStartDecision::StopRestartLoop
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_start_after_the_limit_opens_a_new_window() {
        let directory = directory("new-window");
        for timestamp in 0..5 {
            let (_, start) = record_macos_start_millis(&directory, timestamp).unwrap();
            record_macos_failed_exit(&directory, start).unwrap();
        }
        assert_eq!(
            record_macos_start_millis(&directory, 6).unwrap().0,
            MacosStartDecision::Run
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
