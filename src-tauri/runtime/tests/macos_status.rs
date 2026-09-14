use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use muniment_runtime::{
    MacosDiagnosticEvent, MacosUnifiedLog, MacosUnifiedLogRecord, APPLICATION_IDENTIFIER,
    MACOS_UNIFIED_LOG_CATEGORY,
};

const MACOS_TEST_EXIT_ENV: &str = "MUNIMENT_RUNTIME_TEST_MACOS_ACTIVATION_EXIT";

fn directory() -> PathBuf {
    static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "muniment-runtime-macos-status-{}-{}",
        std::process::id(),
        NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&path);
    let profile = path.join(".muniment");
    fs::create_dir_all(&profile).unwrap();
    fs::set_permissions(profile, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

fn runtime(directory: &PathBuf, exit: &str) -> Output {
    // The binary reads a temporary home, so this machine's state root stays untouched.
    Command::new(env!("CARGO_BIN_EXE_muniment-runtime"))
        .env("HOME", directory)
        .env("MUNIMENT_STATE_DIR", directory.join(".muniment"))
        .env_remove("XDG_RUNTIME_DIR")
        .env(MACOS_TEST_EXIT_ENV, exit)
        .output()
        .unwrap()
}

#[test]
fn maps_macos_activation_outcomes_to_process_statuses() {
    let directory = directory();

    let orderly = runtime(&directory, "orderly");
    assert!(orderly.status.success());
    assert_exit_line(
        &orderly,
        "muniment-runtime: exit status=0 cause=manager stop",
    );
    for _ in 0..4 {
        let failed = runtime(&directory, "failed");
        assert_eq!(failed.status.code(), Some(1));
        assert_exit_line(
            &failed,
            "muniment-runtime: exit status=1 cause=activation failed",
        );
    }
    let stopped = runtime(&directory, "failed");
    assert!(stopped.status.success());
    assert_exit_line(
        &stopped,
        "muniment-runtime: exit status=0 cause=restart loop stopped after activation failure",
    );

    fs::remove_dir_all(directory).unwrap();
}

fn assert_exit_line(output: &Output, expected: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let lines: Vec<_> = stderr
        .lines()
        .filter(|line| line.starts_with("muniment-runtime: exit "))
        .collect();
    assert_eq!(lines, [expected]);
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
    if !Command::new("id")
        .args(["-u"])
        .output()
        .unwrap()
        .stdout
        .starts_with(b"0\n")
    {
        return;
    }
    let root = directory();
    let logs = root.join("logs");
    fs::create_dir(&logs).unwrap();
    fs::set_permissions(&logs, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(Command::new("chown")
        .args(["1:1"])
        .arg(&logs)
        .status()
        .unwrap()
        .success());

    assert!(muniment_runtime::write_macos_diagnostic(
        &logs,
        muniment_runtime::MacosDiagnosticEvent::ActivationFailed,
    )
    .is_err());

    assert!(Command::new("chown")
        .args(["0:0"])
        .arg(&logs)
        .status()
        .unwrap()
        .success());
    let log = logs.join("runtime.log");
    fs::write(&log, b"").unwrap();
    fs::set_permissions(&log, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(Command::new("chown")
        .args(["1:1"])
        .arg(&log)
        .status()
        .unwrap()
        .success());
    assert!(muniment_runtime::write_macos_diagnostic(
        &logs,
        muniment_runtime::MacosDiagnosticEvent::ActivationFailed,
    )
    .is_err());
    assert!(Command::new("chown")
        .args(["0:0"])
        .arg(&log)
        .status()
        .unwrap()
        .success());
    fs::remove_dir_all(root).unwrap();
}

#[derive(Default)]
struct RecordingUnifiedLog {
    records: Mutex<Vec<MacosUnifiedLogRecord>>,
}

impl MacosUnifiedLog for RecordingUnifiedLog {
    fn emit(&self, record: MacosUnifiedLogRecord) {
        self.records.lock().unwrap().push(record);
    }
}

impl RecordingUnifiedLog {
    fn records(&self) -> Vec<MacosUnifiedLogRecord> {
        self.records.lock().unwrap().clone()
    }
}

fn expected_unified_message(event: MacosDiagnosticEvent) -> &'static str {
    match event {
        MacosDiagnosticEvent::ActivationFailed => {
            "event=activation_failed message=runtime activation failed"
        }
        MacosDiagnosticEvent::DesktopExecutableCheckFailed => {
            "event=activation_failed step=desktop_executable_check message=runtime desktop executable check failed"
        }
        MacosDiagnosticEvent::SocketBindFailed => {
            "event=activation_failed step=socket_bind message=runtime socket bind failed"
        }
        MacosDiagnosticEvent::StateOpenFailed => {
            "event=activation_failed step=state_open message=runtime state open failed"
        }
        MacosDiagnosticEvent::ArgumentsInvalid => {
            "event=arguments_invalid message=runtime arguments invalid"
        }
        MacosDiagnosticEvent::InstanceLockWait => {
            "event=instance_lock_wait message=runtime instance lock is held"
        }
        MacosDiagnosticEvent::StartRecordFailed => {
            "event=start_record_failed message=start record update failed"
        }
        MacosDiagnosticEvent::RestartLoopStopped => {
            "event=restart_loop_stopped message=runtime restart limit reached"
        }
    }
}

fn diagnostic_events() -> [MacosDiagnosticEvent; 8] {
    [
        MacosDiagnosticEvent::ActivationFailed,
        MacosDiagnosticEvent::DesktopExecutableCheckFailed,
        MacosDiagnosticEvent::SocketBindFailed,
        MacosDiagnosticEvent::StateOpenFailed,
        MacosDiagnosticEvent::ArgumentsInvalid,
        MacosDiagnosticEvent::InstanceLockWait,
        MacosDiagnosticEvent::StartRecordFailed,
        MacosDiagnosticEvent::RestartLoopStopped,
    ]
}

#[test]
fn mirrors_each_fixed_event_through_the_unified_log_adapter() {
    let root = directory();
    let logs = root.join("logs");
    let logger = RecordingUnifiedLog::default();

    for event in diagnostic_events() {
        muniment_runtime::write_macos_diagnostic_with(&logs, event, &logger).unwrap();
    }

    let records = logger.records();
    assert_eq!(records.len(), diagnostic_events().len());
    for (index, event) in diagnostic_events().into_iter().enumerate() {
        assert_eq!(
            records[index],
            MacosUnifiedLogRecord {
                subsystem: APPLICATION_IDENTIFIER,
                category: MACOS_UNIFIED_LOG_CATEGORY,
                message: expected_unified_message(event),
            }
        );
        assert_eq!(records[index].subsystem, "ai.muniment.desktop");
        assert_eq!(records[index].category, "runtime");
        assert!(!records[index].message.contains('%'));
        assert!(!records[index]
            .message
            .contains(&root.to_string_lossy().to_string()));
    }

    let contents = fs::read_to_string(logs.join("runtime.log")).unwrap();
    let expected_file: String = diagnostic_events()
        .into_iter()
        .map(|event| format!("{}\n", expected_unified_message(event)))
        .collect();
    assert_eq!(contents, expected_file);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn emits_unified_log_when_the_file_write_fails() {
    let root = directory();
    let target = root.join("target");
    fs::create_dir(&target).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o700)).unwrap();
    let linked_logs = root.join("linked-logs");
    symlink(&target, &linked_logs).unwrap();
    let logger = RecordingUnifiedLog::default();

    assert!(muniment_runtime::write_macos_diagnostic_with(
        &linked_logs,
        MacosDiagnosticEvent::ActivationFailed,
        &logger,
    )
    .is_err());
    assert_eq!(
        logger.records(),
        [MacosUnifiedLogRecord {
            subsystem: APPLICATION_IDENTIFIER,
            category: MACOS_UNIFIED_LOG_CATEGORY,
            message: expected_unified_message(MacosDiagnosticEvent::ActivationFailed),
        }]
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn emit_macos_unified_log_with_does_not_write_the_file() {
    let root = directory();
    let logs = root.join("logs");
    fs::create_dir(&logs).unwrap();
    fs::set_permissions(&logs, fs::Permissions::from_mode(0o700)).unwrap();
    let logger = RecordingUnifiedLog::default();

    muniment_runtime::emit_macos_unified_log_with(&logger, MacosDiagnosticEvent::ArgumentsInvalid);

    assert_eq!(
        logger.records(),
        [MacosUnifiedLogRecord {
            subsystem: APPLICATION_IDENTIFIER,
            category: MACOS_UNIFIED_LOG_CATEGORY,
            message: expected_unified_message(MacosDiagnosticEvent::ArgumentsInvalid),
        }]
    );
    assert!(!logs.join("runtime.log").exists());
    fs::remove_dir_all(root).unwrap();
}
