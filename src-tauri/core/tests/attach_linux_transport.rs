#![cfg(target_os = "linux")]

use muniment_core::attach::linux::{endpoint_path_in, LinuxTransportError, OwnedUnixListener};
use std::fs::{self, Permissions};
use std::os::unix::fs::{symlink, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Runtime(PathBuf);
impl Runtime {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "muniment-attach-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn derives_the_fixed_endpoint_and_rejects_bad_roots() {
    let runtime = Runtime::new();
    assert_eq!(
        endpoint_path_in(runtime.path()).unwrap(),
        runtime.path().join("muniment/attach-v1.sock")
    );
    assert!(matches!(
        endpoint_path_in(Path::new("relative")),
        Err(LinuxTransportError::RuntimeDirNotAbsolute)
    ));
    assert!(matches!(
        endpoint_path_in(&runtime.path().join("missing")),
        Err(LinuxTransportError::RuntimeDirUnavailable)
    ));
    fs::set_permissions(runtime.path(), Permissions::from_mode(0o750)).unwrap();
    assert!(matches!(
        endpoint_path_in(runtime.path()),
        Err(LinuxTransportError::RuntimeDirInsecureMode)
    ));
}

#[test]
fn bind_verifies_modes_and_drop_removes_its_socket() {
    let runtime = Runtime::new();
    let listener = OwnedUnixListener::bind_in(runtime.path()).unwrap();
    let path = listener.local_path().to_owned();
    assert_eq!(
        fs::symlink_metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::symlink_metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    drop(listener);
    assert!(!path.exists());
}

#[test]
fn refuses_symlinks_and_non_socket_entries() {
    for symlink_entry in [false, true] {
        let runtime = Runtime::new();
        let app = runtime.path().join("muniment");
        fs::create_dir(&app).unwrap();
        fs::set_permissions(&app, Permissions::from_mode(0o700)).unwrap();
        let endpoint = app.join("attach-v1.sock");
        if symlink_entry {
            symlink(runtime.path(), &endpoint).unwrap();
        } else {
            fs::write(&endpoint, b"not a socket").unwrap();
        }
        assert!(matches!(
            OwnedUnixListener::bind_in(runtime.path()),
            Err(LinuxTransportError::ExistingEndpointUnsafe)
        ));
        assert!(fs::symlink_metadata(endpoint).is_ok());
    }
}

#[test]
fn preserves_a_live_listener_and_recovers_a_stale_socket() {
    let runtime = Runtime::new();
    let first = OwnedUnixListener::bind_in(runtime.path()).unwrap();
    assert!(matches!(
        OwnedUnixListener::bind_in(runtime.path()),
        Err(LinuxTransportError::EndpointInUse)
    ));
    let path = first.local_path().to_owned();
    drop(first);
    let stale = UnixListener::bind(&path).unwrap();
    drop(stale);
    let recovered = OwnedUnixListener::bind_in(runtime.path()).unwrap();
    drop(recovered);
}

#[test]
fn accept_returns_kernel_peer_metadata() {
    let runtime = Runtime::new();
    let listener = OwnedUnixListener::bind_in(runtime.path()).unwrap();
    let client = UnixStream::connect(listener.local_path()).unwrap();
    let accepted = listener.accept().unwrap();
    assert_eq!(accepted.peer.pid, std::process::id());
    assert_eq!(accepted.peer.uid, unsafe { libc::geteuid() });
    assert_eq!(accepted.peer.gid, unsafe { libc::getegid() });
    drop((accepted, client));
}

#[test]
fn cleanup_does_not_remove_a_replacement() {
    let runtime = Runtime::new();
    let listener = OwnedUnixListener::bind_in(runtime.path()).unwrap();
    let path = listener.local_path().to_owned();
    fs::remove_file(&path).unwrap();
    let replacement = UnixListener::bind(&path).unwrap();
    drop(listener);
    assert!(path.exists());
    drop(replacement);
}
