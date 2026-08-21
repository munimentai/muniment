use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const MACOS_TEST_EXIT_ENV: &str = "MUNIMENT_RUNTIME_TEST_MACOS_ACTIVATION_EXIT";

fn directory() -> PathBuf {
    static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "muniment-runtime-macos-status-{}-{}",
        std::process::id(),
        NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&path);
    let profile = path.join(muniment_runtime::APPLICATION_IDENTIFIER);
    fs::create_dir_all(&profile).unwrap();
    fs::set_permissions(profile, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

fn runtime(directory: &PathBuf, exit: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_muniment-runtime"))
        .env("XDG_DATA_HOME", directory)
        .env_remove("XDG_RUNTIME_DIR")
        .env(MACOS_TEST_EXIT_ENV, exit)
        .output()
        .unwrap()
}

#[test]
fn maps_macos_activation_outcomes_to_process_statuses() {
    let directory = directory();

    assert!(runtime(&directory, "orderly").status.success());
    for _ in 0..4 {
        assert_eq!(runtime(&directory, "failed").status.code(), Some(1));
    }
    assert!(runtime(&directory, "failed").status.success());

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn creates_an_owner_only_log_with_fixed_redacted_records() {
    let root = directory();
    let logs = root.join("logs");

    muniment_runtime::write_macos_diagnostic(
        &logs,
        muniment_runtime::MacosDiagnosticEvent::ActivationFailed,
    )
    .unwrap();

    let log = logs.join("runtime.log");
    assert_eq!(
        fs::metadata(&logs).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(&log).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let contents = fs::read_to_string(log).unwrap();
    assert_eq!(
        contents,
        "event=activation_failed message=runtime activation failed\n"
    );
    assert!(!contents.contains(&root.to_string_lossy().to_string()));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn truncates_the_log_deterministically_at_the_size_limit() {
    let root = directory();
    let logs = root.join("logs");
    fs::create_dir(&logs).unwrap();
    fs::set_permissions(&logs, fs::Permissions::from_mode(0o700)).unwrap();
    let log = logs.join("runtime.log");
    fs::write(
        &log,
        vec![b'x'; muniment_runtime::MACOS_RUNTIME_LOG_MAX_BYTES as usize],
    )
    .unwrap();
    fs::set_permissions(&log, fs::Permissions::from_mode(0o600)).unwrap();

    muniment_runtime::write_macos_diagnostic(
        &logs,
        muniment_runtime::MacosDiagnosticEvent::RestartLoopStopped,
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(log).unwrap(),
        "event=restart_loop_stopped message=runtime restart limit reached\n"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejects_symlinks_and_group_access() {
    let root = directory();
    let target = root.join("target");
    fs::create_dir(&target).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o700)).unwrap();
    let linked_logs = root.join("linked-logs");
    symlink(&target, &linked_logs).unwrap();
    assert!(muniment_runtime::write_macos_diagnostic(
        &linked_logs,
        muniment_runtime::MacosDiagnosticEvent::ActivationFailed,
    )
    .is_err());

    let logs = root.join("logs");
    fs::create_dir(&logs).unwrap();
    fs::set_permissions(&logs, fs::Permissions::from_mode(0o750)).unwrap();
    assert!(muniment_runtime::write_macos_diagnostic(
        &logs,
        muniment_runtime::MacosDiagnosticEvent::ActivationFailed,
    )
    .is_err());

    fs::set_permissions(&logs, fs::Permissions::from_mode(0o700)).unwrap();
    let log = logs.join("runtime.log");
    fs::write(&log, b"").unwrap();
    fs::set_permissions(&log, fs::Permissions::from_mode(0o640)).unwrap();
    assert!(muniment_runtime::write_macos_diagnostic(
        &logs,
        muniment_runtime::MacosDiagnosticEvent::ActivationFailed,
    )
    .is_err());

    fs::remove_file(&log).unwrap();
    symlink(&target, &log).unwrap();
    assert!(muniment_runtime::write_macos_diagnostic(
        &logs,
        muniment_runtime::MacosDiagnosticEvent::ActivationFailed,
    )
    .is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejects_foreign_ownership_when_the_test_can_change_owners() {
    if unsafe { libc::geteuid() } != 0 {
        return;
    }
    let root = directory();
    let logs = root.join("logs");
    fs::create_dir(&logs).unwrap();
    fs::set_permissions(&logs, fs::Permissions::from_mode(0o700)).unwrap();
    let path = std::ffi::CString::new(logs.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::chown(path.as_ptr(), 1, 1) }, 0);

    assert!(muniment_runtime::write_macos_diagnostic(
        &logs,
        muniment_runtime::MacosDiagnosticEvent::ActivationFailed,
    )
    .is_err());

    assert_eq!(unsafe { libc::chown(path.as_ptr(), 0, 0) }, 0);
    let log = logs.join("runtime.log");
    fs::write(&log, b"").unwrap();
    fs::set_permissions(&log, fs::Permissions::from_mode(0o600)).unwrap();
    let log_path = std::ffi::CString::new(log.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::chown(log_path.as_ptr(), 1, 1) }, 0);
    assert!(muniment_runtime::write_macos_diagnostic(
        &logs,
        muniment_runtime::MacosDiagnosticEvent::ActivationFailed,
    )
    .is_err());
    assert_eq!(unsafe { libc::chown(log_path.as_ptr(), 0, 0) }, 0);
    fs::remove_dir_all(root).unwrap();
}
