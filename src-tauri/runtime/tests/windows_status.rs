#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::Duration;

use muniment_runtime::{
    clear_windows_crash_window, record_windows_failed_activation, record_windows_failed_exit,
    record_windows_start, run_recorded_windows_activation, write_windows_diagnostic,
    ClearWindowsCrashWindowError, WindowsActivationExit, WindowsDiagnosticEvent,
    WindowsStartDecision, WINDOWS_RUNTIME_LOG_MAX_BYTES,
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
fn startup_causes_reach_the_owner_only_log() {
    let root = directory();
    let cause = "Task start failed: could not start the runtime task (RunTask HRESULT(0x80041326))";
    write_windows_diagnostic(
        &root,
        WindowsDiagnosticEvent::RuntimeTaskStartFailed.with_cause(cause),
    )
    .unwrap();
    let path = root.join("muniment/logs/runtime.log");
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(
        text,
        format!(
            "event=runtime_task_start_failed message=runtime task start failed cause={cause}\n"
        )
    );
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_failed_windows_activation_without_local_app_data_returns_failure() {
    let state = directory();

    assert_eq!(record_windows_failed_activation(&state), 1);
    assert!(fs::read_to_string(state.join("windows-starts"))
        .unwrap()
        .starts_with("failure="));
    assert_eq!(fs::read_dir(&state).unwrap().count(), 1);

    fs::remove_dir_all(state).unwrap();
}

#[test]
fn the_fifth_failed_windows_activation_without_local_app_data_stops_restarts() {
    let state = directory();

    for _ in 0..4 {
        assert_eq!(record_windows_failed_activation(&state), 1);
    }
    assert_eq!(record_windows_failed_activation(&state), 0);
    assert_eq!(
        fs::read_to_string(state.join("windows-starts")).unwrap(),
        "needs_attention=true\n"
    );
    assert_eq!(fs::read_dir(&state).unwrap().count(), 1);

    fs::remove_dir_all(state).unwrap();
}

#[test]
fn an_orderly_windows_activation_removes_its_start() {
    let root = directory();
    let state = root.join("state");

    assert_eq!(
        run_recorded_windows_activation(&state, &root, || WindowsActivationExit::Orderly(75)),
        75
    );
    assert_eq!(
        fs::read_to_string(state.join("windows-starts")).unwrap(),
        ""
    );
    assert!(!root.join("muniment/logs/runtime.log").exists());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_failed_windows_activation_records_the_failure() {
    let root = directory();
    let state = root.join("state");

    assert_eq!(
        run_recorded_windows_activation(&state, &root, || WindowsActivationExit::Failed(23)),
        23
    );
    assert!(fs::read_to_string(state.join("windows-starts"))
        .unwrap()
        .starts_with("failure="));
    assert_eq!(
        fs::read_to_string(root.join("muniment/logs/runtime.log")).unwrap(),
        "event=activation_failed message=runtime activation failed\n"
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_fifth_failed_windows_activation_stops_the_restart_loop() {
    let root = directory();
    let state = root.join("state");

    for _ in 0..4 {
        assert_eq!(
            run_recorded_windows_activation(&state, &root, || WindowsActivationExit::Failed(1)),
            1
        );
    }
    assert_eq!(
        run_recorded_windows_activation(&state, &root, || WindowsActivationExit::Failed(1)),
        0
    );
    assert_eq!(
        fs::read_to_string(state.join("windows-starts")).unwrap(),
        "needs_attention=true\n"
    );
    let log = fs::read_to_string(root.join("muniment/logs/runtime.log")).unwrap();
    assert_eq!(log.matches("event=activation_failed").count(), 4);
    assert_eq!(log.matches("event=restart_loop_stopped").count(), 1);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_stop_decision_skips_windows_activation() {
    let root = directory();
    let state = root.join("state");
    for _ in 0..5 {
        record_windows_start(&state).unwrap();
    }
    let activated = AtomicBool::new(false);

    assert_eq!(
        run_recorded_windows_activation(&state, &root, || {
            activated.store(true, Ordering::Relaxed);
            WindowsActivationExit::Orderly(0)
        }),
        0
    );
    assert!(!activated.load(Ordering::Relaxed));
    assert_eq!(
        fs::read_to_string(root.join("muniment/logs/runtime.log")).unwrap(),
        "event=restart_loop_stopped message=runtime restart limit reached\n"
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_orderly_windows_exit_record_failure_returns_failure() {
    let root = directory();
    let state = root.join("state");

    assert_eq!(
        run_recorded_windows_activation(&state, &root, || {
            fs::remove_file(state.join("windows-starts")).unwrap();
            fs::create_dir(state.join("windows-starts")).unwrap();
            WindowsActivationExit::Orderly(0)
        }),
        1
    );
    assert_eq!(
        fs::read_to_string(root.join("muniment/logs/runtime.log")).unwrap(),
        "event=start_record_failed message=start record update failed\n"
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_failed_windows_exit_record_failure_returns_failure() {
    let root = directory();
    let state = root.join("state");

    assert_eq!(
        run_recorded_windows_activation(&state, &root, || {
            fs::remove_file(state.join("windows-starts")).unwrap();
            fs::create_dir(state.join("windows-starts")).unwrap();
            WindowsActivationExit::Failed(23)
        }),
        1
    );
    assert_eq!(
        fs::read_to_string(root.join("muniment/logs/runtime.log")).unwrap(),
        "event=start_record_failed message=start record update failed\n"
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_windows_start_record_failure_returns_failure() {
    let root = directory();
    let state = root.join("state");
    fs::write(&state, []).unwrap();
    let activated = AtomicBool::new(false);

    assert_eq!(
        run_recorded_windows_activation(&state, &root, || {
            activated.store(true, Ordering::Relaxed);
            WindowsActivationExit::Orderly(0)
        }),
        1
    );
    assert!(!activated.load(Ordering::Relaxed));
    assert_eq!(
        fs::read_to_string(root.join("muniment/logs/runtime.log")).unwrap(),
        "event=start_record_failed message=start record update failed\n"
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn clear_restores_five_windows_starts() {
    let state_directory = directory();
    for failure in 0..5 {
        let (_, start) = record_windows_start(&state_directory).unwrap();
        let expected = if failure == 4 {
            WindowsStartDecision::StopRestartLoop
        } else {
            WindowsStartDecision::Run
        };
        assert_eq!(
            record_windows_failed_exit(&state_directory, start).unwrap(),
            expected
        );
    }

    clear_windows_crash_window(&state_directory, Duration::ZERO).unwrap();

    for _ in 0..5 {
        assert_eq!(
            record_windows_start(&state_directory).unwrap().0,
            WindowsStartDecision::Run
        );
    }
    fs::remove_dir_all(state_directory).unwrap();
}

#[test]
fn clear_succeeds_without_a_windows_start_record() {
    let state_directory = directory();

    clear_windows_crash_window(&state_directory, Duration::ZERO).unwrap();

    fs::remove_dir_all(state_directory).unwrap();
}

#[test]
fn two_concurrent_windows_crash_window_clears_succeed() {
    let state_directory = directory();
    record_windows_start(&state_directory).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let callers: Vec<_> = (0..2)
        .map(|_| {
            let state_directory = state_directory.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                clear_windows_crash_window(state_directory, Duration::from_secs(1))
            })
        })
        .collect();

    for caller in callers {
        caller.join().unwrap().unwrap();
    }
    assert!(!state_directory.join("windows-starts").exists());
    fs::remove_dir_all(state_directory).unwrap();
}

#[test]
fn clear_reports_lock_and_record_failures_separately() {
    let unavailable_state_directory = directory().join("state-file");
    fs::write(&unavailable_state_directory, []).unwrap();
    assert!(matches!(
        clear_windows_crash_window(&unavailable_state_directory, Duration::ZERO),
        Err(ClearWindowsCrashWindowError::Lock(_))
    ));

    let invalid_record_directory = directory();
    fs::create_dir(invalid_record_directory.join("windows-starts")).unwrap();
    assert!(matches!(
        clear_windows_crash_window(&invalid_record_directory, Duration::ZERO),
        Err(ClearWindowsCrashWindowError::Record(_))
    ));

    fs::remove_dir_all(unavailable_state_directory.parent().unwrap()).unwrap();
    fs::remove_dir_all(invalid_record_directory).unwrap();
}

#[test]
fn creates_owner_only_windows_logs_with_fixed_records() {
    let root = directory();
    let logs = root.join("muniment/logs");

    for event in [
        WindowsDiagnosticEvent::ActivationFailed,
        WindowsDiagnosticEvent::ArgumentsInvalid,
        WindowsDiagnosticEvent::InstanceLockWait,
        WindowsDiagnosticEvent::InstallLockUnavailable,
        WindowsDiagnosticEvent::RuntimeTaskRegistrationFailed,
        WindowsDiagnosticEvent::RuntimeTaskStartFailed,
        WindowsDiagnosticEvent::StartRecordFailed,
        WindowsDiagnosticEvent::RestartLoopStopped,
    ] {
        write_windows_diagnostic(&root, event).unwrap();
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
            "event=install_lock_unavailable message=install lock unavailable\n",
            "event=runtime_task_registration_failed message=runtime task registration failed\n",
            "event=runtime_task_start_failed message=runtime task start failed\n",
            "event=start_record_failed message=start record update failed\n",
            "event=restart_loop_stopped message=runtime restart limit reached\n"
        )
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn resets_the_windows_log_only_when_the_next_record_crosses_the_limit() {
    let root = directory();
    let application = root.join("muniment");
    fs::create_dir(&application).unwrap();
    fs::set_permissions(&application, fs::Permissions::from_mode(0o700)).unwrap();
    let logs = application.join("logs");
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

    write_windows_diagnostic(&root, WindowsDiagnosticEvent::RestartLoopStopped).unwrap();
    assert_eq!(
        fs::metadata(&log).unwrap().len(),
        WINDOWS_RUNTIME_LOG_MAX_BYTES
    );
    write_windows_diagnostic(&root, WindowsDiagnosticEvent::RestartLoopStopped).unwrap();
    assert_eq!(fs::read(&log).unwrap(), record);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejects_unsafe_windows_log_paths() {
    let root = directory();
    let target = root.join("target");
    fs::create_dir(&target).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o700)).unwrap();
    let application = root.join("muniment");
    symlink(&target, &application).unwrap();
    assert!(write_windows_diagnostic(&root, WindowsDiagnosticEvent::ActivationFailed).is_err());

    fs::remove_file(&application).unwrap();
    fs::create_dir(&application).unwrap();
    fs::set_permissions(&application, fs::Permissions::from_mode(0o700)).unwrap();
    let logs = application.join("logs");
    fs::create_dir(&logs).unwrap();
    fs::set_permissions(&logs, fs::Permissions::from_mode(0o750)).unwrap();
    assert!(write_windows_diagnostic(&root, WindowsDiagnosticEvent::ActivationFailed).is_err());

    fs::set_permissions(&logs, fs::Permissions::from_mode(0o700)).unwrap();
    let log = logs.join("runtime.log");
    symlink(&target, &log).unwrap();
    assert!(write_windows_diagnostic(&root, WindowsDiagnosticEvent::ActivationFailed).is_err());
    assert!(
        write_windows_diagnostic("relative", WindowsDiagnosticEvent::ActivationFailed).is_err()
    );
    fs::remove_dir_all(root).unwrap();
}
