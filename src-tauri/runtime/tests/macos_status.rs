use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use muniment_runtime::{
    MacosDiagnosticEvent, MacosUnifiedLog, MacosUnifiedLogRecord, APPLICATION_IDENTIFIER,
    MACOS_UNIFIED_LOG_CATEGORY,
};

const MACOS_TEST_EXIT_ENV: &str = "MUNIMENT_RUNTIME_TEST_MACOS_ACTIVATION_EXIT";
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

fn directory() -> PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "muniment-runtime-macos-status-{}-{sequence}",
        std::process::id(),
    ));
    let _ = fs::remove_dir_all(&path);
    let profile = path.join(muniment_runtime::APPLICATION_IDENTIFIER);
    fs::create_dir_all(&profile).unwrap();
    fs::set_permissions(profile, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

fn runtime(directory: &Path, exit: &str) -> Output {
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
fn missing_data_directory_allows_normal_activation() {
    let directory = directory();
    let profile = directory.join(muniment_runtime::APPLICATION_IDENTIFIER);
    fs::remove_dir_all(&directory).unwrap();

    assert!(runtime(&directory, "orderly").status.success());
    assert!(profile.join("macos-starts").is_file());

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn rollback_marker_stops_before_profile_state_opens() {
    let directory = directory();
    let profile = directory.join(muniment_runtime::APPLICATION_IDENTIFIER);
    let marker = profile.join(muniment_runtime::MACOS_ROLLBACK_MARKER_NAME);
    fs::write(&marker, []).unwrap();
    fs::set_permissions(&marker, fs::Permissions::from_mode(0o600)).unwrap();

    assert!(runtime(&directory, "failed").status.success());
    for name in ["macos-starts", "runs.sqlite3", "cas", "pi-sessions"] {
        assert!(!profile.join(name).exists(), "{name} must remain unopened");
    }
    assert!(!profile.join("muniment/attach-v1.sock").exists());

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn profile_directory_symlink_fails_before_state_opens() {
    let directory = directory();
    let profile = directory.join(muniment_runtime::APPLICATION_IDENTIFIER);
    let target = directory.join("profile-target");
    fs::rename(&profile, &target).unwrap();
    symlink(&target, &profile).unwrap();

    let output = runtime(&directory, "orderly");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "muniment-runtime: rollback marker could not be checked\n"
    );
    for name in ["macos-starts", "runs.sqlite3", "cas", "pi-sessions"] {
        assert!(!target.join(name).exists(), "{name} must remain unopened");
    }
    assert!(!target.join("muniment/attach-v1.sock").exists());

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn unsafe_rollback_markers_fail_with_a_redacted_error() {
    for (name, setup) in [
        ("permissions", insecure_marker as fn(&Path)),
        ("symlink", symlink_marker),
        ("directory", directory_marker),
    ] {
        let directory = directory();
        let profile = directory.join(muniment_runtime::APPLICATION_IDENTIFIER);
        let marker = profile.join(muniment_runtime::MACOS_ROLLBACK_MARKER_NAME);
        setup(&marker);

        let output = runtime(&directory, "orderly");
        assert_eq!(output.status.code(), Some(1), "{name}");
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "muniment-runtime: rollback marker is unsafe\n"
        );
        assert!(!profile.join("macos-starts").exists());

        fs::remove_dir_all(directory).unwrap();
    }
}

fn insecure_marker(path: &Path) {
    fs::write(path, []).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o644)).unwrap();
}

fn symlink_marker(path: &Path) {
    let target = path.with_extension("target");
    fs::write(&target, []).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    symlink(target, path).unwrap();
}

fn directory_marker(path: &Path) {
    fs::create_dir(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
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

fn diagnostic_events() -> [MacosDiagnosticEvent; 5] {
    [
        MacosDiagnosticEvent::ActivationFailed,
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
