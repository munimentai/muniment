#![cfg(target_os = "linux")]

use muniment_core::attach::transport::linux::{
    LinuxAttachListener, LinuxTransportError, ParentOpenHook, PeerCredentialProvider,
    PeerCredentials,
};
use muniment_core::attach::{decode_frame, encode_frame, AttachListener};
use serde_json::{json, Value};
use std::fs::{self, Permissions};
use std::io::{Read, Write};
use std::os::fd::RawFd;
use std::os::unix::fs::{symlink, FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "muniment-attach-test-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
}

impl AsRef<Path> for TestDirectory {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn rejects_missing_and_relative_runtime_directory_configuration() {
    assert!(matches!(
        LinuxAttachListener::bind_in(Path::new("relative")),
        Err(LinuxTransportError::RuntimeDirectoryNotAbsolute)
    ));
    let missing = std::env::temp_dir().join(format!(
        "muniment-attach-missing-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    assert!(matches!(
        LinuxAttachListener::bind_in(&missing),
        Err(LinuxTransportError::RuntimeDirectoryInsecure)
    ));
}

#[test]
fn carries_a_bounded_protocol_frame_after_peer_authentication() {
    let runtime = TestDirectory::new();
    let listener = LinuxAttachListener::bind_in(runtime.as_ref()).unwrap();
    let frame =
        encode_frame(&json!({"protocol":"muniment.attach/1","client_nonce":"test"})).unwrap();

    std::thread::scope(|scope| {
        let endpoint = listener.endpoint().to_owned();
        scope.spawn(move || {
            let mut client = UnixStream::connect(endpoint).unwrap();
            client.write_all(&frame).unwrap();
        });
        let mut accepted = listener.accept().unwrap();
        let mut bytes = Vec::new();
        accepted.read_to_end(&mut bytes).unwrap();
        let (message, consumed) = decode_frame::<Value>(&bytes).unwrap().unwrap();
        assert_eq!(consumed, bytes.len());
        assert_eq!(message["protocol"], "muniment.attach/1");
    });
}

#[test]
fn rejects_insecure_runtime_and_existing_endpoint_types() {
    let runtime = TestDirectory::new();
    fs::set_permissions(runtime.as_ref(), Permissions::from_mode(0o750)).unwrap();
    assert!(matches!(
        LinuxAttachListener::bind_in(runtime.as_ref()),
        Err(LinuxTransportError::RuntimeDirectoryInsecure)
    ));

    fs::set_permissions(runtime.as_ref(), Permissions::from_mode(0o700)).unwrap();
    let parent = runtime.as_ref().join("muniment");
    fs::create_dir(&parent).unwrap();
    fs::set_permissions(&parent, Permissions::from_mode(0o700)).unwrap();
    let endpoint = parent.join("attach-v1.sock");
    fs::write(&endpoint, b"not a socket").unwrap();
    assert!(matches!(
        LinuxAttachListener::bind_in(runtime.as_ref()),
        Err(LinuxTransportError::EndpointInsecure)
    ));
    assert!(endpoint.is_file());

    fs::remove_file(&endpoint).unwrap();
    symlink("missing", &endpoint).unwrap();
    assert!(matches!(
        LinuxAttachListener::bind_in(runtime.as_ref()),
        Err(LinuxTransportError::EndpointInsecure)
    ));
    assert!(fs::symlink_metadata(endpoint)
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
fn live_listener_cannot_be_taken_over() {
    let runtime = TestDirectory::new();
    let first = LinuxAttachListener::bind_in(runtime.as_ref()).unwrap();
    assert!(matches!(
        LinuxAttachListener::bind_in(runtime.as_ref()),
        Err(LinuxTransportError::EndpointLiveOrAmbiguous)
    ));
    assert!(UnixStream::connect(first.endpoint()).is_ok());
}

#[derive(Clone, Copy)]
struct WrongUid;

impl PeerCredentialProvider for WrongUid {
    fn credentials(&self, _: RawFd) -> Result<PeerCredentials, LinuxTransportError> {
        Ok(PeerCredentials {
            pid: 1,
            uid: u32::MAX,
            gid: 1,
        })
    }
}

#[derive(Clone, Copy)]
struct UnavailableCredentials;

impl PeerCredentialProvider for UnavailableCredentials {
    fn credentials(&self, _: RawFd) -> Result<PeerCredentials, LinuxTransportError> {
        Err(LinuxTransportError::PeerCredentialsUnavailable)
    }
}

#[test]
fn rejects_a_peer_when_injected_credentials_do_not_match() {
    let runtime = TestDirectory::new();
    let listener = LinuxAttachListener::bind_with_credentials(runtime.as_ref(), WrongUid).unwrap();
    std::thread::scope(|scope| {
        let endpoint = listener.endpoint().to_owned();
        scope.spawn(move || UnixStream::connect(endpoint).unwrap());
        assert!(matches!(
            listener.accept(),
            Err(LinuxTransportError::PeerUidMismatch)
        ));
    });
}

#[test]
fn rejects_a_peer_when_credentials_are_unavailable() {
    let runtime = TestDirectory::new();
    let listener =
        LinuxAttachListener::bind_with_credentials(runtime.as_ref(), UnavailableCredentials)
            .unwrap();
    std::thread::scope(|scope| {
        let endpoint = listener.endpoint().to_owned();
        scope.spawn(move || UnixStream::connect(endpoint).unwrap());
        assert!(matches!(
            listener.accept(),
            Err(LinuxTransportError::PeerCredentialsUnavailable)
        ));
    });
}

#[test]
fn drop_preserves_a_replacement_endpoint() {
    let runtime = TestDirectory::new();
    let listener = LinuxAttachListener::bind_in(runtime.as_ref()).unwrap();
    let endpoint = listener.endpoint().to_owned();
    let original = endpoint.with_extension("original");
    fs::rename(&endpoint, &original).unwrap();
    fs::write(&endpoint, b"replacement").unwrap();
    drop(listener);
    assert_eq!(fs::read(&endpoint).unwrap(), b"replacement");
}

#[test]
fn removes_an_owned_stale_socket() {
    let runtime = TestDirectory::new();
    let parent = runtime.as_ref().join("muniment");
    fs::create_dir(&parent).unwrap();
    fs::set_permissions(&parent, Permissions::from_mode(0o700)).unwrap();
    let endpoint = parent.join("attach-v1.sock");
    drop(UnixListener::bind(&endpoint).unwrap());
    let listener = LinuxAttachListener::bind_in(runtime.as_ref()).unwrap();
    assert_eq!(listener.endpoint(), endpoint);
}

struct ReplaceParent {
    replacement: PathBuf,
}

impl ParentOpenHook for ReplaceParent {
    fn before_parent_open(&self, parent: &Path) {
        let verified = parent.with_extension("verified");
        fs::rename(parent, verified).unwrap();
        fs::rename(&self.replacement, parent).unwrap();
    }
}

#[test]
fn startup_never_unlinks_an_endpoint_under_a_replaced_parent() {
    let runtime = TestDirectory::new();
    let verified_parent = runtime.as_ref().join("muniment");
    fs::create_dir(&verified_parent).unwrap();
    fs::set_permissions(&verified_parent, Permissions::from_mode(0o700)).unwrap();

    let replacement = runtime.as_ref().join("replacement");
    fs::create_dir(&replacement).unwrap();
    fs::set_permissions(&replacement, Permissions::from_mode(0o700)).unwrap();
    let replacement_endpoint = replacement.join("attach-v1.sock");
    drop(UnixListener::bind(&replacement_endpoint).unwrap());

    let result = LinuxAttachListener::bind_with_credentials_and_hook(
        runtime.as_ref(),
        muniment_core::attach::transport::linux::SoPeerCredentialProvider,
        ReplaceParent {
            replacement: replacement.clone(),
        },
    );

    assert!(matches!(
        result,
        Err(LinuxTransportError::AppDirectoryInsecure)
    ));
    assert!(fs::symlink_metadata(verified_parent.join("attach-v1.sock"))
        .unwrap()
        .file_type()
        .is_socket());
}
