#![cfg(target_os = "linux")]

use muniment_core::attach::linux::{AttachFilesystem, AttachTransport, AttachTransportError};
use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "muniment-attach-transport-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn publishes_authenticates_and_removes_a_private_socket() {
    let runtime = TestDirectory::new();
    let filesystem = AttachFilesystem::from_runtime_directory(&runtime.0).unwrap();
    let transport = AttachTransport::bind(&filesystem).unwrap();
    let metadata = fs::symlink_metadata(filesystem.endpoint_path()).unwrap();
    assert!(metadata.file_type().is_socket());
    assert_eq!(metadata.mode() & 0o777, 0o600);

    let client = UnixStream::connect(filesystem.endpoint_path()).unwrap();
    let (_stream, peer) = transport.accept().unwrap();
    assert_eq!(peer.pid, std::process::id() as libc::pid_t);
    assert_eq!(peer.uid, unsafe { libc::geteuid() });
    assert_eq!(peer.gid, unsafe { libc::getegid() });
    drop(client);

    drop(transport);
    assert!(!filesystem.endpoint_path().exists());
}

#[test]
fn refuses_a_live_listener_and_recovers_a_stale_socket() {
    let runtime = TestDirectory::new();
    let filesystem = AttachFilesystem::from_runtime_directory(&runtime.0).unwrap();
    let first = AttachTransport::bind(&filesystem).unwrap();
    assert_eq!(
        AttachTransport::bind(&filesystem).unwrap_err(),
        AttachTransportError::ExistingListener
    );
    drop(first);

    let stale = UnixListener::bind(filesystem.endpoint_path()).unwrap();
    fs::set_permissions(
        filesystem.endpoint_path(),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    drop(stale);
    let recovered = AttachTransport::bind(&filesystem).unwrap();
    drop(recovered);
}

#[test]
fn refuses_unsafe_entries_and_preserves_replacements_on_drop() {
    let runtime = TestDirectory::new();
    let filesystem = AttachFilesystem::from_runtime_directory(&runtime.0).unwrap();
    fs::write(filesystem.endpoint_path(), b"unrelated").unwrap();
    assert_eq!(
        AttachTransport::bind(&filesystem).unwrap_err(),
        AttachTransportError::ExistingEndpointUnsafe
    );
    fs::remove_file(filesystem.endpoint_path()).unwrap();

    let transport = AttachTransport::bind(&filesystem).unwrap();
    fs::remove_file(filesystem.endpoint_path()).unwrap();
    fs::write(filesystem.endpoint_path(), b"replacement").unwrap();
    drop(transport);
    assert_eq!(
        fs::read(filesystem.endpoint_path()).unwrap(),
        b"replacement"
    );
}

#[test]
fn displayed_errors_do_not_disclose_runtime_paths() {
    let error = AttachTransportError::EndpointMetadata;
    assert!(!error.to_string().contains("/"));
}
