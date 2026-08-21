use std::fs::{self, OpenOptions};
use std::io::{self, Write};
#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::APPLICATION_IDENTIFIER;

const FAILURE_LIMIT: usize = 5;
const FAILURE_WINDOW: Duration = Duration::from_secs(5 * 60);
const RECORD_NAME: &str = "macos-starts";
pub const MACOS_ROLLBACK_MARKER_NAME: &str = "rollback-pending";
pub const MACOS_RUNTIME_LOG_MAX_BYTES: u64 = 256 * 1024;
pub const MACOS_UNIFIED_LOG_CATEGORY: &str = "runtime";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MacosRollbackMarkerError {
    Check,
    Unsafe,
}

impl std::fmt::Display for MacosRollbackMarkerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Check => "rollback marker could not be checked",
            Self::Unsafe => "rollback marker is unsafe",
        })
    }
}

impl std::error::Error for MacosRollbackMarkerError {}

/// Checks the fixed rollback marker without following symlinks.
#[cfg(unix)]
pub fn macos_rollback_pending(
    profile_directory: impl AsRef<Path>,
) -> Result<bool, MacosRollbackMarkerError> {
    let profile_missing = !profile_directory
        .as_ref()
        .try_exists()
        .map_err(|_| MacosRollbackMarkerError::Check)?;
    match fs::create_dir_all(profile_directory.as_ref()) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(MacosRollbackMarkerError::Check),
    }
    let profile = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(profile_directory)
        .map_err(|_| MacosRollbackMarkerError::Check)?;
    if profile_missing && unsafe { libc::fchmod(profile.as_raw_fd(), 0o700) } != 0 {
        return Err(MacosRollbackMarkerError::Check);
    }
    let marker_name = c"rollback-pending";
    // SAFETY: openat receives a valid directory descriptor and a static C string.
    let descriptor = unsafe {
        libc::openat(
            profile.as_raw_fd(),
            marker_name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if descriptor == -1 {
        return match io::Error::last_os_error().raw_os_error() {
            Some(libc::ENOENT) => Ok(false),
            Some(libc::ELOOP) => Err(MacosRollbackMarkerError::Unsafe),
            _ => Err(MacosRollbackMarkerError::Check),
        };
    }
    // SAFETY: openat returned an owned descriptor.
    let file = unsafe { fs::File::from_raw_fd(descriptor) };
    let metadata = file
        .metadata()
        .map_err(|_| MacosRollbackMarkerError::Check)?;
    if !private_regular_file(&metadata) {
        return Err(MacosRollbackMarkerError::Unsafe);
    }
    Ok(true)
}

#[cfg(not(unix))]
pub fn macos_rollback_pending(
    _profile_directory: impl AsRef<Path>,
) -> Result<bool, MacosRollbackMarkerError> {
    Ok(false)
}

#[cfg(unix)]
fn private_regular_file(metadata: &fs::Metadata) -> bool {
    // SAFETY: geteuid takes no arguments and has no preconditions.
    let effective_uid = unsafe { libc::geteuid() };
    metadata.is_file() && metadata.uid() == effective_uid && metadata.mode() & 0o077 == 0
}

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

    fn unified_message(self) -> &'static str {
        let record = self.record();
        std::str::from_utf8(&record[..record.len() - 1]).expect("diagnostic records are UTF-8")
    }
}

/// Fixed unified-log payload for one runtime diagnostic event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MacosUnifiedLogRecord {
    pub subsystem: &'static str,
    pub category: &'static str,
    pub message: &'static str,
}

/// Adapter seam for macOS unified logging.
pub trait MacosUnifiedLog {
    fn emit(&self, record: MacosUnifiedLogRecord);
}

/// Production unified logger. It writes through os_log on macOS.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemMacosUnifiedLog;

impl MacosUnifiedLog for SystemMacosUnifiedLog {
    fn emit(&self, record: MacosUnifiedLogRecord) {
        emit_system_unified_log(record);
    }
}

fn unified_record(event: MacosDiagnosticEvent) -> MacosUnifiedLogRecord {
    MacosUnifiedLogRecord {
        subsystem: APPLICATION_IDENTIFIER,
        category: MACOS_UNIFIED_LOG_CATEGORY,
        message: event.unified_message(),
    }
}

/// Emits one fixed unified-log record for `event`.
pub fn emit_macos_unified_log(event: MacosDiagnosticEvent) {
    emit_macos_unified_log_with(&SystemMacosUnifiedLog, event);
}

/// Emits one fixed unified-log record through `logger`.
#[doc(hidden)]
pub fn emit_macos_unified_log_with(logger: &impl MacosUnifiedLog, event: MacosDiagnosticEvent) {
    logger.emit(unified_record(event));
}

/// Appends one fixed record and truncates the log before it exceeds 256 KiB.
/// It also emits the same record through unified logging.
#[cfg(unix)]
pub fn write_macos_diagnostic(
    log_directory: impl AsRef<Path>,
    event: MacosDiagnosticEvent,
) -> io::Result<()> {
    write_macos_diagnostic_with(log_directory, event, &SystemMacosUnifiedLog)
}

/// Writes the bounded file log and emits one unified-log record through `logger`.
#[doc(hidden)]
#[cfg(unix)]
pub fn write_macos_diagnostic_with(
    log_directory: impl AsRef<Path>,
    event: MacosDiagnosticEvent,
    logger: &impl MacosUnifiedLog,
) -> io::Result<()> {
    emit_macos_unified_log_with(logger, event);
    muniment_core::user_diagnostics::append_owner_only_record(
        log_directory.as_ref(),
        c"runtime.log",
        MACOS_RUNTIME_LOG_MAX_BYTES,
        event.record(),
    )
}

fn emit_system_unified_log(record: MacosUnifiedLogRecord) {
    #[cfg(target_os = "macos")]
    macos_os_log::emit(record);
    #[cfg(not(target_os = "macos"))]
    let _ = record;
}

#[cfg(target_os = "macos")]
mod macos_os_log {
    use super::MacosUnifiedLogRecord;
    use std::ffi::{c_char, c_void, CString};
    use std::ptr;
    use std::sync::OnceLock;

    const OS_LOG_TYPE_ERROR: u8 = 16;

    struct CachedLog {
        handle: usize,
        _subsystem: CString,
        _category: CString,
    }

    // os_log_with_type is a C macro. `_os_log_internal` is the exported implementation.
    extern "C" {
        static __dso_handle: u8;
        fn os_log_create(subsystem: *const c_char, category: *const c_char) -> *mut c_void;
        fn _os_log_internal(
            dso: *const c_void,
            log: *mut c_void,
            type_: u8,
            format: *const c_char,
            ...
        );
    }

    fn runtime_log(subsystem: &str, category: &str) -> *mut c_void {
        static LOG: OnceLock<CachedLog> = OnceLock::new();
        let cached = LOG.get_or_init(|| {
            let subsystem = CString::new(subsystem).expect("unified log subsystem is a C string");
            let category = CString::new(category).expect("unified log category is a C string");
            let handle = unsafe { os_log_create(subsystem.as_ptr(), category.as_ptr()) as usize };
            CachedLog {
                handle,
                _subsystem: subsystem,
                _category: category,
            }
        });
        cached.handle as *mut c_void
    }

    pub fn emit(record: MacosUnifiedLogRecord) {
        let Ok(message) = CString::new(record.message) else {
            return;
        };
        let log = runtime_log(record.subsystem, record.category);
        if log.is_null() {
            return;
        }
        unsafe {
            _os_log_internal(
                ptr::addr_of!(__dso_handle).cast(),
                log,
                OS_LOG_TYPE_ERROR,
                message.as_ptr(),
            );
        }
    }
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
