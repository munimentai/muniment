//! Runs the real `muniment-server` binary: it serves, answers its own health
//! check, holds the instance lock, stops on a signal and keeps its state.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

const SERVER: &str = env!("CARGO_BIN_EXE_muniment-server");

struct Root(PathBuf);

impl Root {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "muniment-server-serve-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn state(&self) -> PathBuf {
        self.0.join("state")
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A command with the state root under `root` and no inherited home, so the
/// server adopts nothing from the user's own files.
fn server(root: &Root, command: &str) -> Command {
    let mut server = Command::new(SERVER);
    server
        .arg(command)
        .env("MUNIMENT_STATE_DIR", root.state())
        .env("HOME", &root.0)
        .env_remove("XDG_RUNTIME_DIR")
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("PI_CODING_AGENT_DIR");
    server
}

fn healthy(root: &Root) -> bool {
    server(root, "health")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

/// The running server this test started. It owns the child from the moment
/// it spawns, so a failed readiness wait, assertion, signal or stop deadline
/// still kills and reaps the server on unwind instead of leaving it running
/// with the instance lock held.
struct Server(Child);

impl Server {
    fn id(&self) -> u32 {
        self.0.id()
    }

    fn stderr(&mut self) -> std::process::ChildStderr {
        self.0.stderr.take().unwrap()
    }
}

impl Drop for Server {
    /// Best-effort: the kill fails harmlessly on a child that already exited,
    /// and the wait returns the recorded status of a child already reaped.
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start(root: &Root) -> Server {
    let mut started = Server(
        server(root, "serve")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(60);
    while !healthy(root) {
        if let Some(status) = started.0.try_wait().unwrap() {
            panic!("the server exited before it answered: {status}");
        }
        assert!(Instant::now() < deadline, "the server never answered");
        std::thread::sleep(Duration::from_millis(100));
    }
    started
}

fn signal(server: &Server, signal: libc::c_int) {
    let pid = libc::pid_t::try_from(server.id()).unwrap();
    // SAFETY: the pid names the child this test started and has not reaped.
    assert_eq!(unsafe { libc::kill(pid, signal) }, 0);
}

fn wait(server: &mut Server) -> ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = server.0.try_wait().unwrap() {
            return status;
        }
        assert!(Instant::now() < deadline, "the server did not stop");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn health_fails_while_nothing_serves() {
    let root = Root::new("idle");
    assert!(!healthy(&root));
}

#[test]
fn serves_until_sigterm_and_keeps_its_state_across_a_restart() {
    let root = Root::new("restart");
    let mut first = start(&root);
    assert_eq!(mode(&root.state().join("run")), 0o700);
    assert!(root.state().join("runs.sqlite3").is_file());

    let second = server(&root, "serve")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!second.success(), "a second server took the instance lock");
    assert!(healthy(&root));

    signal(&first, libc::SIGTERM);
    assert!(wait(&mut first).success());
    let log = std::io::read_to_string(first.stderr()).unwrap();
    assert!(log.contains("muniment-server: started"), "{log}");
    assert!(log.contains("muniment-server: stopped"), "{log}");
    assert!(!healthy(&root));

    let journal = std::fs::metadata(root.state().join("runs.sqlite3")).unwrap();
    let mut again = start(&root);
    assert!(root.state().join("runs.sqlite3").is_file());
    assert!(
        std::fs::metadata(root.state().join("runs.sqlite3"))
            .unwrap()
            .len()
            >= journal.len()
    );
    signal(&again, libc::SIGINT);
    assert!(wait(&mut again).success());
}

#[test]
fn a_panic_before_shutdown_kills_and_reaps_the_server() {
    let root = Root::new("unwind");
    let (outcome_tx, outcome_rx) = std::sync::mpsc::channel();
    let state = root.state();
    let home = root.0.clone();
    // The failing test body runs on its own thread so a cleanup that hangs
    // fails this test at the timeout below instead of hanging it.
    std::thread::spawn(move || {
        let root = Root(home);
        let mut pid = None;
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let server = start(&root);
            pid = Some(server.id());
            panic!("a failure before the server is stopped");
        }))
        .is_err();
        // The panic dropped the server; this probe reports whether its pid is
        // gone, which holds only once the child is both dead and reaped.
        let reaped = pid.map(|pid| {
            let pid = libc::pid_t::try_from(pid).unwrap();
            // SAFETY: signal 0 only checks whether the pid exists.
            let missing = unsafe { libc::kill(pid, 0) } == -1;
            missing && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        });
        // The directory belongs to the outer `Root`.
        std::mem::forget(root);
        let _ = outcome_tx.send((unwound, reaped));
    });
    let (unwound, reaped) = outcome_rx
        .recv_timeout(Duration::from_secs(120))
        .expect("the server cleanup did not finish");
    assert!(unwound, "the test body did not panic");
    assert_eq!(reaped, Some(true), "the server was not reaped");
    assert!(state.join("runs.sqlite3").is_file());
    assert!(!healthy(&root));

    // A new server takes the instance lock the killed one held.
    let mut again = start(&root);
    signal(&again, libc::SIGTERM);
    assert!(wait(&mut again).success());
}
