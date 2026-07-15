#![cfg(target_os = "linux")]

use muniment_core::attach::{
    linux_attach_endpoint_in, validate_linux_peer_uid, LinuxAttachError, LinuxAttachListener,
};
use std::fs::{self, Permissions};
use std::io::{Read, Write};
use std::os::unix::fs::{symlink, FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

static NEXT_RUNTIME: AtomicU64 = AtomicU64::new(0);

struct Runtime(PathBuf);

impl Runtime {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "muniment-attach-test-{}-{}",
            std::process::id(),
            NEXT_RUNTIME.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn endpoint_is_exact_and_rejects_invalid_runtime_directories() {
    let runtime = Runtime::new();
    assert_eq!(
        linux_attach_endpoint_in(&runtime.0).unwrap(),
        runtime.0.join("muniment/attach-v1.sock")
    );
    assert_eq!(
        linux_attach_endpoint_in(PathBuf::from("relative").as_path()),
        Err(LinuxAttachError::RuntimeDirectoryInvalid)
    );
    assert_eq!(
        linux_attach_endpoint_in(&runtime.0.join("missing")),
        Err(LinuxAttachError::RuntimeDirectoryInvalid)
    );

    let runtime_link = runtime.0.with_extension("link");
    symlink(&runtime.0, &runtime_link).unwrap();
    assert_eq!(
        linux_attach_endpoint_in(&runtime_link),
        Err(LinuxAttachError::RuntimeDirectoryInvalid)
    );
    fs::remove_file(runtime_link).unwrap();

    fs::set_permissions(&runtime.0, Permissions::from_mode(0o750)).unwrap();
    assert_eq!(
        linux_attach_endpoint_in(&runtime.0),
        Err(LinuxAttachError::RuntimeDirectoryInsecure)
    );
}

#[test]
fn bind_sets_exact_modes_accepts_same_uid_and_cleans_up() {
    let runtime = Runtime::new();
    let listener = LinuxAttachListener::bind_in(&runtime.0).unwrap();
    let endpoint = listener.endpoint().to_owned();
    assert_eq!(
        fs::symlink_metadata(endpoint.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    let socket = fs::symlink_metadata(&endpoint).unwrap();
    assert!(socket.file_type().is_socket());
    assert_eq!(socket.permissions().mode() & 0o777, 0o600);

    let client = thread::spawn(move || {
        let mut stream = UnixStream::connect(endpoint).unwrap();
        stream.write_all(b"x").unwrap();
    });
    let mut stream = listener.accept().unwrap();
    let mut byte = [0];
    stream.read_exact(&mut byte).unwrap();
    assert_eq!(byte, *b"x");
    client.join().unwrap();
    let endpoint = listener.endpoint().to_owned();
    listener.shutdown().unwrap();
    assert!(!endpoint.exists());
}

#[test]
fn stale_socket_is_replaced_but_live_socket_and_other_entries_are_preserved() {
    let runtime = Runtime::new();
    let parent = runtime.0.join("muniment");
    fs::create_dir(&parent).unwrap();
    fs::set_permissions(&parent, Permissions::from_mode(0o700)).unwrap();
    let endpoint = parent.join("attach-v1.sock");
    drop(UnixListener::bind(&endpoint).unwrap());
    let listener = LinuxAttachListener::bind_in(&runtime.0).unwrap();
    drop(listener);

    let live = UnixListener::bind(&endpoint).unwrap();
    assert_eq!(
        LinuxAttachListener::bind_in(&runtime.0).unwrap_err(),
        LinuxAttachError::EndpointInUse
    );
    assert!(fs::symlink_metadata(&endpoint)
        .unwrap()
        .file_type()
        .is_socket());
    drop(live);
    fs::remove_file(&endpoint).unwrap();

    fs::write(&endpoint, b"not a socket").unwrap();
    assert_eq!(
        LinuxAttachListener::bind_in(&runtime.0).unwrap_err(),
        LinuxAttachError::EndpointInvalid
    );
    assert!(endpoint.is_file());
    fs::remove_file(&endpoint).unwrap();
    symlink("target", &endpoint).unwrap();
    assert_eq!(
        LinuxAttachListener::bind_in(&runtime.0).unwrap_err(),
        LinuxAttachError::EndpointInvalid
    );
    assert!(fs::symlink_metadata(&endpoint)
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
fn cleanup_does_not_remove_a_replacement() {
    let runtime = Runtime::new();
    let listener = LinuxAttachListener::bind_in(&runtime.0).unwrap();
    let endpoint = listener.endpoint().to_owned();
    fs::remove_file(&endpoint).unwrap();
    fs::write(&endpoint, b"replacement").unwrap();
    assert_eq!(listener.shutdown(), Err(LinuxAttachError::EndpointInvalid));
    assert_eq!(fs::read(endpoint).unwrap(), b"replacement");
}

#[test]
fn peer_validation_fails_closed() {
    assert_eq!(validate_linux_peer_uid(7, Some(7)), Ok(()));
    assert_eq!(
        validate_linux_peer_uid(7, None),
        Err(LinuxAttachError::PeerCredentialsUnavailable)
    );
    assert_eq!(
        validate_linux_peer_uid(7, Some(8)),
        Err(LinuxAttachError::PeerIdentityMismatch)
    );
    assert!(!LinuxAttachError::PeerIdentityMismatch
        .to_string()
        .contains('8'));
}
