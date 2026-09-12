#![cfg(target_os = "windows")]

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use muniment_runtime::{
    write_windows_diagnostic, WindowsDiagnosticEvent, WINDOWS_RUNTIME_LOG_MAX_BYTES,
};

fn local_app_data() -> PathBuf {
    static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    loop {
        let path = std::env::temp_dir().join(format!(
            "muniment-runtime-windows-acl-{}-{timestamp}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::create_dir(&path) {
            Ok(()) => return path,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => panic!("failed to create Windows ACL test directory: {error}"),
        }
    }
}

#[test]
fn creates_the_managed_tree_and_writes_fixed_records() {
    let root = local_app_data();
    let install_directory = root.join("muniment");
    fs::create_dir(&install_directory).unwrap();

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

    assert_eq!(
        fs::read_to_string(root.join("ai.muniment.desktop/logs/runtime.log")).unwrap(),
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
    assert!(!root.join("runtime.log").exists());
    fs::remove_dir(&install_directory).unwrap();
    assert!(!install_directory.exists());
    assert!(root.join("ai.muniment.desktop/logs/runtime.log").is_file());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn writes_beneath_an_application_directory_with_an_inherited_acl() {
    let root = local_app_data();
    let status = Command::new("icacls")
        .arg(&root)
        .args(["/grant", "*S-1-1-0:(OI)(CI)(R)"])
        .status()
        .unwrap();
    assert!(status.success());
    let application = root.join("ai.muniment.desktop");
    fs::create_dir(&application).unwrap();
    let before = Command::new("icacls").arg(&application).output().unwrap();
    assert!(before.status.success());
    assert!(String::from_utf8_lossy(&before.stdout).contains("(I)"));

    let event = WindowsDiagnosticEvent::RuntimeTaskStartFailed;
    let cause = "Runtime readiness failed: the attach endpoint did not appear within 5 seconds.";
    write_windows_diagnostic(&root, event.with_cause(cause)).unwrap();
    write_windows_diagnostic(&root, event.with_cause(cause)).unwrap();
    assert_eq!(
        fs::read_to_string(application.join("logs/runtime.log")).unwrap(),
        format!(
            "event=runtime_task_start_failed message=runtime task start failed cause={cause}\n"
        )
        .repeat(2)
    );
    let after = Command::new("icacls").arg(&application).output().unwrap();
    assert!(after.status.success());
    assert_eq!(before.stdout, after.stdout);
    for path in [
        application.join("logs"),
        application.join("logs/runtime.log"),
    ] {
        let acl = Command::new("icacls").arg(path).output().unwrap();
        assert!(acl.status.success());
        assert!(!String::from_utf8_lossy(&acl.stdout).contains("(I)"));
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn refuses_an_existing_logs_directory_with_an_inherited_acl() {
    let root = local_app_data();
    let logs = root.join("ai.muniment.desktop/logs");
    fs::create_dir_all(&logs).unwrap();
    assert!(write_windows_diagnostic(&root, WindowsDiagnosticEvent::ActivationFailed).is_err());
    assert!(!logs.join("runtime.log").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejects_relative_roots_and_unsafe_native_access_lists() {
    assert!(
        write_windows_diagnostic("relative", WindowsDiagnosticEvent::ActivationFailed).is_err()
    );

    let root = local_app_data();
    write_windows_diagnostic(&root, WindowsDiagnosticEvent::ActivationFailed).unwrap();
    let logs = root.join("ai.muniment.desktop/logs");
    let status = Command::new("icacls")
        .arg(&logs)
        .args(["/grant", "*S-1-1-0:(R)", "/inheritance:e"])
        .status()
        .unwrap();
    assert!(status.success());
    assert!(write_windows_diagnostic(&root, WindowsDiagnosticEvent::ActivationFailed).is_err());
    fs::remove_dir_all(root).unwrap();

    let root = local_app_data();
    write_windows_diagnostic(&root, WindowsDiagnosticEvent::ActivationFailed).unwrap();
    let log = root.join("ai.muniment.desktop/logs/runtime.log");
    let status = Command::new("icacls")
        .arg(&log)
        .args(["/grant", "*S-1-1-0:(R)"])
        .status()
        .unwrap();
    assert!(status.success());
    assert!(write_windows_diagnostic(&root, WindowsDiagnosticEvent::ActivationFailed).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejects_hard_linked_log_files() {
    let root = local_app_data();
    write_windows_diagnostic(&root, WindowsDiagnosticEvent::ActivationFailed).unwrap();
    let log = root.join("ai.muniment.desktop/logs/runtime.log");
    let outside_link = root.join("outside.log");
    fs::hard_link(&log, &outside_link).unwrap();
    let contents = fs::read(&log).unwrap();

    assert!(write_windows_diagnostic(&root, WindowsDiagnosticEvent::RestartLoopStopped).is_err());
    assert_eq!(fs::read(&log).unwrap(), contents);
    assert_eq!(fs::read(&outside_link).unwrap(), contents);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejects_reparse_points_in_the_managed_path() {
    let root = local_app_data();
    write_windows_diagnostic(&root, WindowsDiagnosticEvent::ActivationFailed).unwrap();
    fs::remove_dir_all(root.join("ai.muniment.desktop/logs")).unwrap();
    let target = root.join("target");
    fs::create_dir(&target).unwrap();
    let link = root.join("ai.muniment.desktop").join("logs");
    let status = Command::new("cmd")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&link)
        .arg(&target)
        .status()
        .unwrap();
    assert!(status.success());

    assert!(write_windows_diagnostic(&root, WindowsDiagnosticEvent::ActivationFailed).is_err());
    fs::remove_dir_all(root).unwrap();

    let base = local_app_data();
    let target = base.join("target");
    let nested_root = target.join("local");
    fs::create_dir_all(&nested_root).unwrap();
    let link = base.join("link");
    let status = Command::new("cmd")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&link)
        .arg(&target)
        .status()
        .unwrap();
    assert!(status.success());
    assert!(
        write_windows_diagnostic(link.join("local"), WindowsDiagnosticEvent::ActivationFailed)
            .is_err()
    );
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn rejects_an_application_directory_junction() {
    let root = local_app_data();
    let target = root.join("target");
    fs::create_dir(&target).unwrap();
    let link = root.join("ai.muniment.desktop");
    let status = Command::new("cmd")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&link)
        .arg(&target)
        .status()
        .unwrap();
    assert!(status.success());

    assert!(write_windows_diagnostic(&root, WindowsDiagnosticEvent::ActivationFailed).is_err());
    assert!(!target.join("logs").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn resets_only_when_the_next_record_crosses_the_limit() {
    let root = local_app_data();
    write_windows_diagnostic(&root, WindowsDiagnosticEvent::RestartLoopStopped).unwrap();
    let log = root.join("ai.muniment.desktop/logs/runtime.log");
    let record = b"event=restart_loop_stopped message=runtime restart limit reached\n";
    let file = fs::OpenOptions::new().write(true).open(&log).unwrap();
    file.set_len(WINDOWS_RUNTIME_LOG_MAX_BYTES - record.len() as u64)
        .unwrap();

    write_windows_diagnostic(&root, WindowsDiagnosticEvent::RestartLoopStopped).unwrap();
    assert_eq!(
        fs::metadata(&log).unwrap().len(),
        WINDOWS_RUNTIME_LOG_MAX_BYTES
    );
    write_windows_diagnostic(&root, WindowsDiagnosticEvent::RestartLoopStopped).unwrap();
    assert_eq!(fs::read(&log).unwrap(), record);
    fs::remove_dir_all(root).unwrap();
}
