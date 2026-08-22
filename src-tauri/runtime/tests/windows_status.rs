#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use muniment_runtime::{
    write_windows_diagnostic, WindowsDiagnosticEvent, WINDOWS_RUNTIME_LOG_MAX_BYTES,
};

fn directory() -> PathBuf {
    static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "muniment-runtime-windows-status-{}-{}",
        std::process::id(),
        NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[test]
fn creates_owner_only_windows_logs_with_fixed_records() {
    let root = directory();
    let logs = root.join("logs");

    for event in [
        WindowsDiagnosticEvent::ActivationFailed,
        WindowsDiagnosticEvent::ArgumentsInvalid,
        WindowsDiagnosticEvent::InstanceLockWait,
        WindowsDiagnosticEvent::StartRecordFailed,
        WindowsDiagnosticEvent::RestartLoopStopped,
    ] {
        write_windows_diagnostic(&logs, event).unwrap();
    }

    let log = logs.join("runtime.log");
    assert_eq!(
        fs::metadata(&logs).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(&log).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::read_to_string(log).unwrap(),
        concat!(
            "event=activation_failed message=runtime activation failed\n",
            "event=arguments_invalid message=runtime arguments invalid\n",
            "event=instance_lock_wait message=runtime instance lock is held\n",
            "event=start_record_failed message=start record update failed\n",
            "event=restart_loop_stopped message=runtime restart limit reached\n"
        )
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn resets_the_windows_log_only_when_the_next_record_crosses_the_limit() {
    let root = directory();
    let logs = root.join("logs");
    fs::create_dir(&logs).unwrap();
    fs::set_permissions(&logs, fs::Permissions::from_mode(0o700)).unwrap();
    let log = logs.join("runtime.log");
    let record = b"event=restart_loop_stopped message=runtime restart limit reached\n";
    fs::write(
        &log,
        vec![b'x'; WINDOWS_RUNTIME_LOG_MAX_BYTES as usize - record.len()],
    )
    .unwrap();
    fs::set_permissions(&log, fs::Permissions::from_mode(0o600)).unwrap();

    write_windows_diagnostic(&logs, WindowsDiagnosticEvent::RestartLoopStopped).unwrap();
    assert_eq!(
        fs::metadata(&log).unwrap().len(),
        WINDOWS_RUNTIME_LOG_MAX_BYTES
    );
    write_windows_diagnostic(&logs, WindowsDiagnosticEvent::RestartLoopStopped).unwrap();
    assert_eq!(fs::read(&log).unwrap(), record);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejects_unsafe_windows_log_paths() {
    let root = directory();
    let target = root.join("target");
    fs::create_dir(&target).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o700)).unwrap();
    let logs = root.join("logs");
    symlink(&target, &logs).unwrap();
    assert!(write_windows_diagnostic(&logs, WindowsDiagnosticEvent::ActivationFailed).is_err());

    fs::remove_file(&logs).unwrap();
    fs::create_dir(&logs).unwrap();
    fs::set_permissions(&logs, fs::Permissions::from_mode(0o750)).unwrap();
    assert!(write_windows_diagnostic(&logs, WindowsDiagnosticEvent::ActivationFailed).is_err());

    fs::set_permissions(&logs, fs::Permissions::from_mode(0o700)).unwrap();
    let log = logs.join("runtime.log");
    symlink(&target, &log).unwrap();
    assert!(write_windows_diagnostic(&logs, WindowsDiagnosticEvent::ActivationFailed).is_err());
    fs::remove_dir_all(root).unwrap();
}
