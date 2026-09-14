#![cfg(target_os = "macos")]

use std::fs;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Runtime {
    child: Child,
    directory: std::path::PathBuf,
}

impl Drop for Runtime {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn hand_launched_runtime_writes_started_with_stderr_discarded_within_one_second() {
    use std::os::unix::fs::{symlink, DirBuilderExt};

    let directory = std::env::temp_dir().join(format!(
        "muniment-service-log-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .unwrap();
    // The runner links the login home's state root to the spec home's, and the
    // launchd runtime reads it through the login home alone.
    let spec_state = directory.join("spec home/.muniment");
    fs::create_dir_all(&spec_state).unwrap();
    let login_home = directory.join("login home");
    let profile = login_home.join(".muniment");
    fs::create_dir_all(&login_home).unwrap();
    symlink(&spec_state, &profile).unwrap();
    let expected = format!(
        "muniment-runtime: started version={} state_directory={} config_directory={} endpoint={}\n",
        env!("CARGO_PKG_VERSION"),
        profile.display(),
        fs::canonicalize(&spec_state).unwrap().display(),
        muniment_runtime::macos_attach_socket_path(&profile).display()
    );
    let log = muniment_runtime::effective_user_macos_log_directory()
        .unwrap()
        .join("runtime-service.log");
    let started = Instant::now();
    let child = Command::new(env!("CARGO_BIN_EXE_muniment-runtime"))
        // Model launchd with the login home and no variable from the runner.
        .env_clear()
        .env("HOME", &login_home)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _runtime = Runtime { child, directory };
    loop {
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "The runtime did not log its start within one second."
        );
        if fs::read_to_string(&log)
            .unwrap_or_default()
            .contains(&expected)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
