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
pub const TEST_ROOT_PREFIX: &str = "muniment-test-";

/// How long a root whose process seems gone stays when another host or
/// container made it. Test processes in separate containers can share one
/// temporary directory without seeing each other's process ids, so such a
/// root goes only once its process seems gone and it has sat unchanged this
/// long. A root this host made goes as soon as its process is gone.
const STALE_TEST_ROOT_AGE: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// The temporary directory one test process keeps its state, credentials,
/// Documents, logs and test fixtures in. The process creates it on first use
/// and removes it when it exits. The first
/// call also removes the root of every earlier test process on this host
/// that no longer runs, so a killed run leaves one folder until the next
/// test process starts. The name carries the process id, a mark of the host
/// that sees that id, and a random token, so two processes that share the
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
            "{TEST_ROOT_PREFIX}{}-{:08x}-{token:08x}",
            std::process::id(),
            host_mark()
        ));
        let _ = fs::create_dir_all(&root);
        remove_test_root_at_exit(&root);
        root
    })
    .clone()
}

/// Removes each test root, and each per-process directory an older build
/// left, whose process no longer runs. A root this host made goes at once.
/// A root another host or container made, or an older build's directory,
/// goes once it has also sat unchanged for [`STALE_TEST_ROOT_AGE`].
fn remove_stale_test_roots(temporary: &Path) {
    let Ok(entries) = fs::read_dir(temporary) else {
        return;
    };
    let host = format!("{:08x}", host_mark());
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(rest) = name
            .to_str()
            .and_then(|name| name.strip_prefix(TEST_ROOT_PREFIX))
        else {
            continue;
        };
        let (pid, this_host) = match rest.split('-').collect::<Vec<_>>()[..] {
            [pid, mark, token] if mark.len() == 8 && token.len() == 8 => {
                (pid.parse::<u32>().ok(), mark == host)
            }
            _ => (
                ["", "state-", "credentials-", "documents-", "logs-"]
                    .iter()
                    .find_map(|kind| {
                        let rest = rest.strip_prefix(kind)?;
                        rest.split_once('-')
                            .map_or(rest, |(pid, _)| pid)
                            .parse::<u32>()
                            .ok()
                    }),
                false,
            ),
        };
        let Some(pid) = pid else {
            continue;
        };
        if pid == std::process::id() || process_runs(pid) {
            continue;
        }
        let idle = || {
            entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age >= STALE_TEST_ROOT_AGE)
        };
        if this_host || idle() {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// A mark of the host and process namespace that sees this process's id:
/// a hash of the host name and, on Linux, the process id namespace. Two
/// containers that share a temporary directory get different marks.
fn host_mark() -> u32 {
    let mut identity = Vec::new();
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        extern "C" {
            fn gethostname(name: *mut std::ffi::c_char, length: usize) -> i32;
        }
        let mut name = [0u8; 256];
        if unsafe { gethostname(name.as_mut_ptr().cast(), name.len()) } == 0 {
            let end = name
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(name.len());
            identity.extend_from_slice(&name[..end]);
        }
    }
    #[cfg(target_os = "linux")]
    if let Ok(namespace) = fs::read_link("/proc/self/ns/pid") {
        identity.extend_from_slice(namespace.as_os_str().as_encoded_bytes());
    }
    // FNV-1a, so every build and process computes the same mark.
    identity.iter().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193)
    })
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
    fn a_dead_process_root_from_this_host_goes_at_once_and_another_hosts_after_an_hour() {
        let temporary = super::test_root().join("sweep");
        fs::create_dir_all(&temporary).unwrap();
        let mut child = std::process::Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .spawn()
            .unwrap();
        let gone = child.id();
        child.wait().unwrap();
        let live = std::os::unix::process::parent_id();
        let host = format!("{:08x}", super::host_mark());
        let other = format!("{:08x}", super::host_mark() ^ 1);
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
        let killed = make(&format!("muniment-test-{gone}-{host}-0a1b2c3d"), false);
        let running = make(&format!("muniment-test-{live}-{host}-8c9d0e1f"), true);
        let elsewhere = make(&format!("muniment-test-{gone}-{other}-4e5f6a7b"), false);
        let elsewhere_old = make(&format!("muniment-test-{gone}-{other}-5e5f6a7b"), true);
        let older_build = make(&format!("muniment-test-{gone}-6a7b8c9d"), false);
        let older_build_old = make(&format!("muniment-test-{gone}-7a7b8c9d"), true);
        let stale_logs = make(&format!("muniment-test-logs-{gone}"), true);
        let home = make("muniment-test-home-Xy12Ab", true);
        super::remove_stale_test_roots(&temporary);
        assert!(!killed.exists());
        assert!(running.exists());
        assert!(elsewhere.exists());
        assert!(!elsewhere_old.exists());
        assert!(older_build.exists());
        assert!(!older_build_old.exists());
        assert!(!stale_logs.exists());
        assert!(home.exists());
        fs::remove_dir_all(temporary).unwrap();
    }

    #[test]
    fn the_root_names_this_process_and_host() {
        let root = super::test_root();
        let name = root.file_name().unwrap().to_str().unwrap();
        let prefix = format!(
            "muniment-test-{}-{:08x}-",
            std::process::id(),
            super::host_mark()
        );
        assert!(name.starts_with(&prefix), "{name}");
        assert_eq!(super::host_mark(), super::host_mark());
    }
}
