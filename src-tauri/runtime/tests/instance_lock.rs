#![cfg(target_os = "linux")]

use muniment_core::attach::linux::AttachFilesystem;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const WAIT_TIMEOUT_ENV: &str = "MUNIMENT_RUNTIME_TEST_WAIT_TIMEOUT_MS";
const EXIT_AFTER_LOCK_ENV: &str = "MUNIMENT_RUNTIME_TEST_EXIT_AFTER_LOCK";

struct RuntimeDirectory(PathBuf);

impl RuntimeDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "muniment-runtime-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }

    fn command(&self, timeout_ms: u64) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_muniment-runtime"));
        command
            .env("XDG_RUNTIME_DIR", &self.0)
            .env(WAIT_TIMEOUT_ENV, timeout_ms.to_string());
        command
    }

    fn exit_after_lock_command(&self, timeout_ms: u64) -> Command {
        let mut command = self.command(timeout_ms);
        command.env(EXIT_AFTER_LOCK_ENV, "1");
        command
    }
}

impl Drop for RuntimeDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn wait_for_exit(mut child: Child, timeout: Duration) -> Output {
    let started = Instant::now();
    while child.try_wait().unwrap().is_none() {
        assert!(started.elapsed() < timeout, "runtime did not exit");
        std::thread::sleep(Duration::from_millis(20));
    }
    child.wait_with_output().unwrap()
}

#[test]
fn waits_for_the_instance_lock_then_acquires_it_after_release() {
    let runtime = RuntimeDirectory::new();
    let filesystem = AttachFilesystem::from_runtime_directory(&runtime.0).unwrap();
    let lock = filesystem.acquire_instance_lock().unwrap();
    let mut command = runtime.exit_after_lock_command(2_000);
    let mut child = command.stderr(Stdio::piped()).spawn().unwrap();

    std::thread::sleep(Duration::from_millis(150));
    assert!(child.try_wait().unwrap().is_none());
    drop(lock);

    let output = wait_for_exit(child, Duration::from_secs(3));
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "muniment-runtime: waiting for the instance lock\n"
    );
}

#[test]
fn bounded_wait_fails_while_another_process_holds_the_lock() {
    let runtime = RuntimeDirectory::new();
    let filesystem = AttachFilesystem::from_runtime_directory(&runtime.0).unwrap();
    let _lock = filesystem.acquire_instance_lock().unwrap();

    let output = runtime.exit_after_lock_command(100).output().unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("instance lock wait timed out"));
}

#[test]
fn rejects_an_invalid_test_wait_timeout() {
    let runtime = RuntimeDirectory::new();
    let output = runtime
        .exit_after_lock_command(100)
        .env(WAIT_TIMEOUT_ENV, "invalid")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("must be an unsigned integer"));
}

#[test]
fn sigterm_releases_the_instance_lock_and_exits_successfully() {
    let runtime = RuntimeDirectory::new();
    let filesystem = AttachFilesystem::from_runtime_directory(&runtime.0).unwrap();
    let child = runtime.command(2_000).spawn().unwrap();
    let started = Instant::now();
    loop {
        match filesystem.acquire_instance_lock() {
            Ok(lock) => {
                drop(lock);
                assert!(
                    started.elapsed() < Duration::from_secs(3),
                    "runtime did not take the lock"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(muniment_core::attach::linux::InstanceLockError::AlreadyHeld) => break,
            Err(error) => panic!("failed to check the instance lock: {error}"),
        }
    }

    let signal = Command::new("kill")
        .arg("-TERM")
        .arg(child.id().to_string())
        .output()
        .unwrap();
    assert!(signal.status.success());
    let output = wait_for_exit(child, Duration::from_secs(3));
    assert!(output.status.success());

    let second = runtime.exit_after_lock_command(2_000).output().unwrap();
    assert!(second.status.success());
}
