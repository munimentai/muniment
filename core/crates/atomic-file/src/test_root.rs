//! A test process's temporary root. Each test process keeps its fixtures in
//! one folder of the system temporary folder, which the process removes when
//! it exits, so test runs leave nothing behind. A test calls [`test_root`]
//! wherever it would call `std::env::temp_dir()`. A Unix socket path that
//! must stay short is the one exception.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The name every test process root starts with, followed by the process id
/// and a random token.
const TEST_ROOT_PREFIX: &str = "muniment-test-";

/// How long a root whose process seems gone stays. Test processes in separate
/// containers can share one temporary directory without seeing each other's
/// process ids, so a root goes only once its process is gone and it has sat
/// unchanged this long.
const STALE_TEST_ROOT_AGE: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// The temporary directory one test process keeps its state, credentials,
/// Documents, logs and test fixtures in. The process creates it on first use
/// and removes it when it exits. The first
/// call also removes the root of every earlier test process that no longer
/// runs, so a killed run leaves nothing behind for long. The name carries a
/// random token besides the process id, so two processes that share the
/// directory and the id never share a root.
#[must_use]
pub fn test_root() -> PathBuf {
    static ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| {
        let temporary = std::env::temp_dir();
        remove_stale_test_roots(&temporary);
        let token = {
            use std::hash::{BuildHasher, Hasher};
            std::collections::hash_map::RandomState::new()
                .build_hasher()
                .finish() as u32
        };
        let root = temporary.join(format!(
            "{TEST_ROOT_PREFIX}{}-{token:08x}",
            std::process::id()
        ));
        let _ = fs::create_dir_all(&root);
        remove_test_root_at_exit(&root);
        root
    })
    .clone()
}

/// Removes each test root, and each per-process directory an older build
/// left, whose process no longer runs.
fn remove_stale_test_roots(temporary: &Path) {
    let Ok(entries) = fs::read_dir(temporary) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(rest) = name
            .to_str()
            .and_then(|name| name.strip_prefix(TEST_ROOT_PREFIX))
        else {
            continue;
        };
        let pid = ["", "state-", "credentials-", "documents-", "logs-"]
            .iter()
            .find_map(|kind| {
                let rest = rest.strip_prefix(kind)?;
                rest.split_once('-')
                    .map_or(rest, |(pid, _)| pid)
                    .parse::<u32>()
                    .ok()
            });
        let idle = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age >= STALE_TEST_ROOT_AGE);
        if idle && pid.is_some_and(|pid| pid != std::process::id() && !process_runs(pid)) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// Whether a process with this id runs. Where the answer is unknown, it runs.
fn process_runs(pid: u32) -> bool {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }
        /// "No such process" on macOS and Linux.
        const ESRCH: i32 = 3;
        let Ok(pid) = i32::try_from(pid) else {
            return true;
        };
        // Signal 0 checks the process without signaling it. EPERM names a
        // process that runs as another user.
        let signaled = unsafe { kill(pid, 0) } == 0;
        signaled || io::Error::last_os_error().raw_os_error() != Some(ESRCH)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = pid;
        true
    }
}

/// The root the exit handler removes, with the id of the process that owns
/// it. A forked child that exits keeps its parent's root.
static TEST_ROOT_AT_EXIT: std::sync::OnceLock<(u32, PathBuf)> = std::sync::OnceLock::new();

fn remove_test_root_at_exit(root: &Path) {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        extern "C" fn remove() {
            if let Some((pid, root)) = TEST_ROOT_AT_EXIT.get() {
                if *pid == std::process::id() {
                    let _ = fs::remove_dir_all(root);
                }
            }
        }
        if TEST_ROOT_AT_EXIT
            .set((std::process::id(), root.to_path_buf()))
            .is_ok()
        {
            extern "C" {
                fn atexit(callback: extern "C" fn()) -> i32;
            }
            unsafe {
                atexit(remove);
            }
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let _ = root;
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use std::fs;
    use std::time::{Duration, SystemTime};

    #[test]
    fn a_test_root_goes_once_its_process_is_gone_and_it_sat_an_hour() {
        let temporary = super::test_root().join("sweep");
        fs::create_dir_all(&temporary).unwrap();
        let mut child = std::process::Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .spawn()
            .unwrap();
        let gone = child.id();
        child.wait().unwrap();
        let live = std::os::unix::process::parent_id();
        let long_ago = SystemTime::now() - Duration::from_secs(2 * 60 * 60);
        let make = |name: &str, old: bool| {
            let path = temporary.join(name);
            fs::create_dir_all(path.join("logs")).unwrap();
            if old {
                fs::File::open(&path)
                    .unwrap()
                    .set_modified(long_ago)
                    .unwrap();
            }
            path
        };
        let stale = make(&format!("muniment-test-{gone}-0a1b2c3d"), true);
        let stale_logs = make(&format!("muniment-test-logs-{gone}"), true);
        let fresh = make(&format!("muniment-test-{gone}-4e5f6a7b"), false);
        let running = make(&format!("muniment-test-{live}-8c9d0e1f"), true);
        let home = make("muniment-test-home-Xy12Ab", true);
        super::remove_stale_test_roots(&temporary);
        assert!(!stale.exists());
        assert!(!stale_logs.exists());
        assert!(fresh.exists());
        assert!(running.exists());
        assert!(home.exists());
        fs::remove_dir_all(temporary).unwrap();
    }
}
