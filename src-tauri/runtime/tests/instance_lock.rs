#![cfg(target_os = "linux")]

use muniment_core::attach::linux::AttachFilesystem;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const WAIT_TIMEOUT_ENV: &str = "MUNIMENT_RUNTIME_TEST_WAIT_TIMEOUT_MS";
const EXIT_AFTER_LOCK_ENV: &str = "MUNIMENT_RUNTIME_TEST_EXIT_AFTER_LOCK";

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct RuntimeDirectory(PathBuf);

impl RuntimeDirectory {
    fn new() -> Self {
        Self::new_at(SystemTime::now())
    }

    fn new_at(timestamp: SystemTime) -> Self {
        let path = std::env::temp_dir().join(format!(
            "muniment-runtime-test-{}-{}-{}",
            std::process::id(),
            timestamp.duration_since(UNIX_EPOCH).unwrap().as_nanos(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        // The runtime keeps one state root, so each test names its own instead
        // of the runner's home.
        let state = path.join("state");
        std::fs::create_dir(&state).unwrap();
        std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }

    fn command(&self, timeout_ms: u64) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_muniment-runtime"));
        command
            .env("XDG_RUNTIME_DIR", &self.0)
            .env("XDG_DATA_HOME", &self.0)
            .env("XDG_CONFIG_HOME", &self.0)
            .env(
                muniment_core::state_root::STATE_DIRECTORY_OVERRIDE,
                self.state_directory(),
            )
            .env(WAIT_TIMEOUT_ENV, timeout_ms.to_string());
        command
    }

    fn state_directory(&self) -> PathBuf {
        self.0.join("state")
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
fn directories_are_unique_when_clock_values_repeat() {
    let timestamp = SystemTime::now();
    let directories = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..16)
            .map(|_| scope.spawn(|| RuntimeDirectory::new_at(timestamp)))
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    let paths: std::collections::HashSet<_> = directories
        .iter()
        .map(|directory| directory.0.clone())
        .collect();
    assert_eq!(paths.len(), directories.len());
    for path in &paths {
        assert!(path.is_dir());
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    drop(directories);
    assert!(paths.iter().all(|path| !path.exists()));
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
fn logs_version_state_directory_and_served_endpoint_at_startup() {
    let runtime = RuntimeDirectory::new();
    let filesystem = AttachFilesystem::from_runtime_directory(&runtime.0).unwrap();
    let endpoint = filesystem.endpoint_path().to_owned();
    drop(filesystem);
    let mut command = runtime.command(2_000);
    let child = command.stderr(Stdio::piped()).spawn().unwrap();
    let started = Instant::now();
    while !endpoint.exists() {
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "runtime endpoint did not open"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    let signal = Command::new("kill")
        .arg("-TERM")
        .arg(child.id().to_string())
        .output()
        .unwrap();
    assert!(signal.status.success());
    let output = wait_for_exit(child, Duration::from_secs(3));
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        format!(
            "muniment-runtime: started version={} state_directory={} endpoint={}\n",
            env!("CARGO_PKG_VERSION"),
            runtime.state_directory().display(),
            endpoint.display()
        )
    );
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
