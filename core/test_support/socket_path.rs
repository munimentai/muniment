use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_PATH: AtomicU64 = AtomicU64::new(0);

// Reserve the terminating NUL in sockaddr_un.sun_path.
#[cfg(target_os = "linux")]
const SOCKET_PATH_LIMIT: usize = 107;
#[cfg(not(target_os = "linux"))]
const SOCKET_PATH_LIMIT: usize = 103;
const ATTACH_ENDPOINT: &str = "muniment/attach-v1.sock";

pub(crate) fn validate_endpoint(endpoint: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let length = endpoint.as_os_str().as_bytes().len();
    if length > SOCKET_PATH_LIMIT {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "Test socket endpoint {} is {length} bytes; pathname limit is {SOCKET_PATH_LIMIT} bytes",
                endpoint.display()
            ),
        ));
    }
    Ok(())
}

fn next_sequence() -> u64 {
    NEXT_PATH
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .expect("The test socket path counter overflowed.")
}

fn short_path_in(directory: &Path, process: u32, sequence: u64) -> std::io::Result<PathBuf> {
    let preferred = path_in(directory, process, sequence);
    if directory.is_absolute() && validate_endpoint(&preferred.join(ATTACH_ENDPOINT)).is_ok() {
        return Ok(preferred);
    }
    // /tmp is independent of the host's potentially long per-user TMPDIR.
    let fallback = path_in(Path::new("/tmp"), process, sequence);
    validate_endpoint(&fallback.join(ATTACH_ENDPOINT))?;
    Ok(fallback)
}

pub(crate) fn socket_temp_path() -> PathBuf {
    short_path_in(&std::env::temp_dir(), std::process::id(), next_sequence())
        .expect("A short test socket endpoint is required")
}

/// An exclusively allocated private root, removed even when a fixture unwinds.
#[derive(Debug)]
pub(crate) struct SocketDirectory(PathBuf);

impl SocketDirectory {
    pub(crate) fn new() -> std::io::Result<Self> {
        Self::in_directory(&std::env::temp_dir())
    }

    fn in_directory(directory: &Path) -> std::io::Result<Self> {
        use std::os::unix::fs::DirBuilderExt;
        loop {
            let path = short_path_in(directory, std::process::id(), next_sequence())?;
            let endpoint = path.join(ATTACH_ENDPOINT);
            validate_endpoint(&endpoint)?;
            match std::fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    use std::os::unix::ffi::OsStrExt;
                    return Err(std::io::Error::new(error.kind(), format!(
                        "Cannot allocate test socket endpoint {} ({} bytes; pathname limit {SOCKET_PATH_LIMIT} bytes): {error}",
                        endpoint.display(), endpoint.as_os_str().as_bytes().len()
                    )));
                }
            }
        }
    }
}

impl std::ops::Deref for SocketDirectory {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for SocketDirectory {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<std::ffi::OsStr> for SocketDirectory {
    fn as_ref(&self) -> &std::ffi::OsStr {
        self.0.as_os_str()
    }
}

impl Drop for SocketDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn path_in(directory: &Path, process: u32, sequence: u64) -> PathBuf {
    directory.join(format!("mt-{process:x}-{sequence:x}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::net::UnixListener;

    #[test]
    fn socket_paths_fit_a_49_byte_temp_directory() {
        let directory = PathBuf::from(format!("/tmp/{:0<44}", "muniment"));
        assert_eq!(directory.as_os_str().as_bytes().len(), 49);
        for (process, sequence) in [(0, 0), (u32::MAX, u64::MAX)] {
            let path = path_in(&directory, process, sequence);
            assert!(path.as_os_str().as_bytes().len() < 104);
            assert!(
                path.join("muniment/attach-v1.sock")
                    .as_os_str()
                    .as_bytes()
                    .len()
                    < 104
            );
        }
    }

    #[test]
    fn socket_paths_are_unique_across_threads() {
        let workers: Vec<_> = (0..8)
            .map(|_| {
                std::thread::spawn(|| (0..128).map(|_| socket_temp_path()).collect::<Vec<_>>())
            })
            .collect();
        let mut paths = HashSet::new();
        for worker in workers {
            for path in worker.join().unwrap() {
                assert!(paths.insert(path));
            }
        }
        assert_eq!(paths.len(), 1024);
    }

    #[test]
    fn sockets_bind_under_a_49_byte_temp_directory() {
        let name = socket_temp_path();
        let directory = PathBuf::from(format!(
            "/tmp/{:0<44}",
            name.file_name().unwrap().to_str().unwrap()
        ));
        assert_eq!(directory.as_os_str().as_bytes().len(), 49);
        std::fs::create_dir(&directory).unwrap();
        let direct = path_in(&directory, u32::MAX, u64::MAX);
        let nested = path_in(&directory, u32::MAX - 1, u64::MAX).join("muniment/attach-v1.sock");
        std::fs::create_dir_all(nested.parent().unwrap()).unwrap();
        let first = UnixListener::bind(&direct).unwrap();
        let second = UnixListener::bind(&nested).unwrap();
        drop((first, second));
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[cfg(test)]
mod long_path_tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;

    #[test]
    fn long_temp_bases_bind_direct_and_nested_sockets() {
        let owner = SocketDirectory::in_directory(Path::new("/tmp")).unwrap();
        for (length, multibyte) in [(60, false), (220, false), (180, true)] {
            let prefix = owner.as_os_str().as_bytes().len() + 1;
            let name = if multibyte {
                "é".repeat((length - prefix) / 2)
            } else {
                "x".repeat(length - prefix)
            };
            let base = owner.join(name);
            std::fs::create_dir(&base).unwrap();
            assert!(base.as_os_str().as_bytes().len() >= length - 1);
            let root = SocketDirectory::in_directory(&base).unwrap();
            let other = SocketDirectory::in_directory(&base).unwrap();
            assert_ne!(root.to_path_buf(), other.to_path_buf());
            assert_eq!(
                std::fs::metadata(&*root).unwrap().permissions().mode() & 0o777,
                0o700
            );
            let direct = root.join("direct.sock");
            let nested = root.join(ATTACH_ENDPOINT);
            validate_endpoint(&direct).unwrap();
            validate_endpoint(&nested).unwrap();
            std::fs::create_dir(nested.parent().unwrap()).unwrap();
            let listeners = (
                UnixListener::bind(&direct).unwrap(),
                UnixListener::bind(&nested).unwrap(),
            );
            drop(listeners);
            let path = root.to_path_buf();
            drop(root);
            assert!(!path.exists());
        }
    }

    #[test]
    fn endpoint_diagnostic_counts_bytes_and_names_limit() {
        let endpoint = PathBuf::from(format!("/tmp/{}", "é".repeat(SOCKET_PATH_LIMIT / 2)));
        let length = endpoint.as_os_str().as_bytes().len();
        assert!(length > SOCKET_PATH_LIMIT);
        assert!(endpoint.to_str().unwrap().chars().count() < SOCKET_PATH_LIMIT);
        let error = validate_endpoint(&endpoint).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        let diagnostic = error.to_string();
        assert!(diagnostic.contains(&endpoint.display().to_string()));
        assert!(diagnostic.contains(&format!("{length} bytes")));
        assert!(diagnostic.contains(&format!("limit is {SOCKET_PATH_LIMIT} bytes")));
        assert!(validate_endpoint(Path::new(&"x".repeat(SOCKET_PATH_LIMIT))).is_ok());
        assert!(validate_endpoint(Path::new(&"x".repeat(SOCKET_PATH_LIMIT + 1))).is_err());
    }

    #[test]
    fn allocation_failure_reports_the_complete_endpoint() {
        let owner = SocketDirectory::in_directory(Path::new("/tmp")).unwrap();
        let base = owner.join("missing");
        let error = SocketDirectory::in_directory(&base).unwrap_err();
        let diagnostic = error.to_string();
        assert!(diagnostic.contains(&base.display().to_string()));
        assert!(diagnostic.contains(ATTACH_ENDPOINT));
        assert!(diagnostic.contains("bytes; pathname limit"));
        assert!(diagnostic.contains(&format!("{SOCKET_PATH_LIMIT} bytes")));
    }

    #[test]
    fn exclusive_roots_survive_other_allocations_and_cleanup_on_unwind() {
        let owner = SocketDirectory::new().unwrap();
        let sentinel = owner.join("sentinel");
        std::fs::write(&sentinel, b"owned").unwrap();
        let mut unwound_path = None;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let root = SocketDirectory::new().unwrap();
            unwound_path = Some(root.to_path_buf());
            panic!("fixture unwind");
        }));
        assert!(result.is_err());
        assert!(!unwound_path.unwrap().exists());
        assert_eq!(std::fs::read(sentinel).unwrap(), b"owned");
    }
}
