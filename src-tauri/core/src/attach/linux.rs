//! Owner-only Linux Unix-socket transport for companion attach.

use std::env;
use std::fmt;
use std::fs::{self, Metadata, Permissions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::linux::fs::MetadataExt;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

const DIRECTORY_MODE: u32 = 0o700;
const SOCKET_MODE: u32 = 0o600;

/// A redacted, classifiable Linux attach transport failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinuxAttachError {
    RuntimeDirectoryMissing,
    RuntimeDirectoryInvalid,
    RuntimeDirectoryInsecure,
    AttachDirectoryInvalid,
    AttachDirectoryInsecure,
    EndpointInvalid,
    EndpointInUse,
    PeerCredentialsUnavailable,
    PeerIdentityMismatch,
    Io,
}

impl fmt::Display for LinuxAttachError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::RuntimeDirectoryMissing => "the private runtime directory is unavailable",
            Self::RuntimeDirectoryInvalid => "the private runtime directory is invalid",
            Self::RuntimeDirectoryInsecure => "the private runtime directory is insecure",
            Self::AttachDirectoryInvalid => "the attach directory is invalid",
            Self::AttachDirectoryInsecure => "the attach directory is insecure",
            Self::EndpointInvalid => "the attach endpoint is invalid",
            Self::EndpointInUse => "the attach endpoint is already in use",
            Self::PeerCredentialsUnavailable => "companion identity could not be verified",
            Self::PeerIdentityMismatch => "companion identity was rejected",
            Self::Io => "the local attach transport failed",
        };
        f.write_str(message)
    }
}

impl std::error::Error for LinuxAttachError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Identity {
    dev: u64,
    ino: u64,
}

impl Identity {
    fn of(metadata: &Metadata) -> Self {
        Self {
            dev: metadata.st_dev(),
            ino: metadata.st_ino(),
        }
    }
}

/// Derives the ADR 0009 endpoint, validating `XDG_RUNTIME_DIR` first.
pub fn linux_attach_endpoint() -> Result<PathBuf, LinuxAttachError> {
    let runtime =
        env::var_os("XDG_RUNTIME_DIR").ok_or(LinuxAttachError::RuntimeDirectoryMissing)?;
    linux_attach_endpoint_in(Path::new(&runtime))
}

/// Validates a supplied runtime directory and derives its attach endpoint.
/// This variant is useful to embedders which already captured their environment.
pub fn linux_attach_endpoint_in(runtime: &Path) -> Result<PathBuf, LinuxAttachError> {
    verify_runtime(runtime, effective_uid())?;
    Ok(runtime.join("muniment").join("attach-v1.sock"))
}

/// A listener whose endpoint is removed on shutdown only if it is still the
/// exact socket created by this instance.
#[derive(Debug)]
pub struct LinuxAttachListener {
    listener: UnixListener,
    parent: PathBuf,
    endpoint: PathBuf,
    uid: u32,
    parent_identity: Identity,
    socket_identity: Identity,
}

impl LinuxAttachListener {
    pub fn bind() -> Result<Self, LinuxAttachError> {
        let runtime =
            env::var_os("XDG_RUNTIME_DIR").ok_or(LinuxAttachError::RuntimeDirectoryMissing)?;
        Self::bind_in(Path::new(&runtime))
    }

    pub fn bind_in(runtime: &Path) -> Result<Self, LinuxAttachError> {
        let uid = effective_uid();
        verify_runtime(runtime, uid)?;
        let parent = runtime.join("muniment");
        ensure_parent(&parent, uid)?;
        let parent_metadata = verify_parent(&parent, uid)?;
        let parent_identity = Identity::of(&parent_metadata);
        let endpoint = parent.join("attach-v1.sock");
        prepare_endpoint(&parent, parent_identity, &endpoint, uid)?;

        let listener = UnixListener::bind(&endpoint).map_err(|_| LinuxAttachError::Io)?;
        if fs::set_permissions(&endpoint, Permissions::from_mode(SOCKET_MODE)).is_err() {
            let _ = remove_verified(&parent, parent_identity, &endpoint, uid, None);
            return Err(LinuxAttachError::Io);
        }

        let current_parent = verify_parent(&parent, uid)?;
        if Identity::of(&current_parent) != parent_identity {
            return Err(LinuxAttachError::AttachDirectoryInvalid);
        }
        let socket_metadata = verify_socket(&endpoint, uid)?;
        if socket_metadata.st_mode() & 0o777 != SOCKET_MODE {
            return Err(LinuxAttachError::EndpointInvalid);
        }
        let socket_identity = Identity::of(&socket_metadata);
        Ok(Self {
            listener,
            parent,
            endpoint,
            uid,
            parent_identity,
            socket_identity,
        })
    }

    pub fn endpoint(&self) -> &Path {
        &self.endpoint
    }

    pub fn accept(&self) -> Result<UnixStream, LinuxAttachError> {
        let (stream, _) = self.listener.accept().map_err(|_| LinuxAttachError::Io)?;
        let peer_uid = peer_uid(&stream)?;
        validate_linux_peer_uid(self.uid, Some(peer_uid))?;
        Ok(stream)
    }

    pub fn set_nonblocking(&self, nonblocking: bool) -> Result<(), LinuxAttachError> {
        self.listener
            .set_nonblocking(nonblocking)
            .map_err(|_| LinuxAttachError::Io)
    }

    pub fn shutdown(self) -> Result<(), LinuxAttachError> {
        remove_verified(
            &self.parent,
            self.parent_identity,
            &self.endpoint,
            self.uid,
            Some(self.socket_identity),
        )
    }
}

impl Drop for LinuxAttachListener {
    fn drop(&mut self) {
        let _ = remove_verified(
            &self.parent,
            self.parent_identity,
            &self.endpoint,
            self.uid,
            Some(self.socket_identity),
        );
    }
}

/// Fail-closed credential validation seam used by the accept path.
pub fn validate_linux_peer_uid(
    expected_uid: u32,
    peer_uid: Option<u32>,
) -> Result<(), LinuxAttachError> {
    match peer_uid {
        None => Err(LinuxAttachError::PeerCredentialsUnavailable),
        Some(uid) if uid != expected_uid => Err(LinuxAttachError::PeerIdentityMismatch),
        Some(_) => Ok(()),
    }
}

fn effective_uid() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

fn peer_uid(stream: &UnixStream) -> Result<u32, LinuxAttachError> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: the descriptor is live for the call and both output pointers
    // refer to initialized, correctly sized writable storage.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    if result != 0 || length as usize != std::mem::size_of::<libc::ucred>() {
        return Err(LinuxAttachError::PeerCredentialsUnavailable);
    }
    Ok(credentials.uid)
}

fn verify_runtime(path: &Path, uid: u32) -> Result<Metadata, LinuxAttachError> {
    if !path.is_absolute() {
        return Err(LinuxAttachError::RuntimeDirectoryInvalid);
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => LinuxAttachError::RuntimeDirectoryInvalid,
        _ => LinuxAttachError::Io,
    })?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(LinuxAttachError::RuntimeDirectoryInvalid);
    }
    if metadata.st_uid() != uid || metadata.st_mode() & 0o077 != 0 {
        return Err(LinuxAttachError::RuntimeDirectoryInsecure);
    }
    Ok(metadata)
}

fn ensure_parent(path: &Path, uid: u32) -> Result<(), LinuxAttachError> {
    match fs::create_dir(path) {
        Ok(()) => fs::set_permissions(path, Permissions::from_mode(DIRECTORY_MODE))
            .map_err(|_| LinuxAttachError::Io),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            verify_parent(path, uid).map(|_| ())
        }
        Err(_) => Err(LinuxAttachError::Io),
    }
}

fn verify_parent(path: &Path, uid: u32) -> Result<Metadata, LinuxAttachError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| LinuxAttachError::AttachDirectoryInvalid)?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(LinuxAttachError::AttachDirectoryInvalid);
    }
    if metadata.st_uid() != uid || metadata.st_mode() & 0o777 != DIRECTORY_MODE {
        return Err(LinuxAttachError::AttachDirectoryInsecure);
    }
    Ok(metadata)
}

fn verify_socket(path: &Path, uid: u32) -> Result<Metadata, LinuxAttachError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| LinuxAttachError::EndpointInvalid)?;
    if !metadata.file_type().is_socket()
        || metadata.file_type().is_symlink()
        || metadata.st_uid() != uid
        || metadata.st_mode() & 0o777 != SOCKET_MODE
    {
        return Err(LinuxAttachError::EndpointInvalid);
    }
    Ok(metadata)
}

fn prepare_endpoint(
    parent: &Path,
    parent_id: Identity,
    endpoint: &Path,
    uid: u32,
) -> Result<(), LinuxAttachError> {
    let metadata = match fs::symlink_metadata(endpoint) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(LinuxAttachError::Io),
    };
    if !metadata.file_type().is_socket()
        || metadata.file_type().is_symlink()
        || metadata.st_uid() != uid
    {
        return Err(LinuxAttachError::EndpointInvalid);
    }
    match UnixStream::connect(endpoint) {
        Ok(_) => return Err(LinuxAttachError::EndpointInUse),
        Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {}
        Err(_) => return Err(LinuxAttachError::EndpointInUse),
    }
    remove_verified(
        parent,
        parent_id,
        endpoint,
        uid,
        Some(Identity::of(&metadata)),
    )
}

fn remove_verified(
    parent: &Path,
    parent_id: Identity,
    endpoint: &Path,
    uid: u32,
    socket_id: Option<Identity>,
) -> Result<(), LinuxAttachError> {
    let parent_metadata = verify_parent(parent, uid)?;
    if Identity::of(&parent_metadata) != parent_id {
        return Err(LinuxAttachError::AttachDirectoryInvalid);
    }
    let metadata = match fs::symlink_metadata(endpoint) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(LinuxAttachError::Io),
    };
    if !metadata.file_type().is_socket()
        || metadata.file_type().is_symlink()
        || metadata.st_uid() != uid
    {
        return Err(LinuxAttachError::EndpointInvalid);
    }
    if socket_id.is_some_and(|identity| identity != Identity::of(&metadata)) {
        return Err(LinuxAttachError::EndpointInvalid);
    }
    fs::remove_file(endpoint).map_err(|_| LinuxAttachError::Io)
}
