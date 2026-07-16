//! Fail-closed Linux transport for the local companion attach protocol.

use std::env;
use std::fs::{self, DirBuilder, Metadata, Permissions};
use std::io;
use std::os::linux::fs::MetadataExt as LinuxMetadataExt;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

const APP_DIR: &str = "muniment";
const SOCKET_NAME: &str = "attach-v1.sock";

/// A non-secret reason the attach endpoint could not be safely created.
#[derive(Debug)]
pub enum LinuxTransportError {
    RuntimeDirMissing,
    RuntimeDirNotAbsolute,
    RuntimeDirUnavailable,
    RuntimeDirNotDirectory,
    RuntimeDirWrongOwner,
    RuntimeDirInsecureMode,
    AppDirUnsafe,
    ExistingEndpointUnsafe,
    EndpointInUse,
    EndpointVerificationFailed,
    PeerCredentialsUnavailable,
    PeerUidMismatch,
    Io(io::Error),
}

impl std::fmt::Display for LinuxTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::RuntimeDirMissing => "XDG_RUNTIME_DIR is not set",
            Self::RuntimeDirNotAbsolute => "XDG_RUNTIME_DIR is not absolute",
            Self::RuntimeDirUnavailable => "XDG_RUNTIME_DIR cannot be inspected",
            Self::RuntimeDirNotDirectory => "XDG_RUNTIME_DIR is not a directory",
            Self::RuntimeDirWrongOwner => "XDG_RUNTIME_DIR has the wrong owner",
            Self::RuntimeDirInsecureMode => "XDG_RUNTIME_DIR is accessible by group or others",
            Self::AppDirUnsafe => "the attach runtime directory is unsafe",
            Self::ExistingEndpointUnsafe => "the existing attach endpoint is unsafe",
            Self::EndpointInUse => "the attach endpoint is already in use",
            Self::EndpointVerificationFailed => "the attach endpoint could not be verified",
            Self::PeerCredentialsUnavailable => "peer credentials are unavailable",
            Self::PeerUidMismatch => "peer UID does not match the desktop UID",
            Self::Io(_) => "the attach transport encountered an operating system error",
        })
    }
}

impl std::error::Error for LinuxTransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for LinuxTransportError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

/// Kernel-provided identity metadata for an accepted Unix stream peer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerCredentials {
    pub pid: u32,
    pub uid: u32,
    pub gid: u32,
}

/// An accepted connection which has passed the mandatory UID check.
#[derive(Debug)]
pub struct AcceptedConnection {
    pub stream: UnixStream,
    pub peer: PeerCredentials,
}

/// Derive and validate the attach endpoint from the process environment.
pub fn endpoint_path() -> Result<PathBuf, LinuxTransportError> {
    let root = env::var_os("XDG_RUNTIME_DIR").ok_or(LinuxTransportError::RuntimeDirMissing)?;
    endpoint_path_in(Path::new(&root))
}

/// Validate an explicit runtime root and derive its exact attach endpoint.
/// This variant lets callers and tests avoid mutating process environment.
pub fn endpoint_path_in(root: &Path) -> Result<PathBuf, LinuxTransportError> {
    if !root.is_absolute() {
        return Err(LinuxTransportError::RuntimeDirNotAbsolute);
    }
    let meta =
        fs::symlink_metadata(root).map_err(|_| LinuxTransportError::RuntimeDirUnavailable)?;
    if !meta.file_type().is_dir() {
        return Err(LinuxTransportError::RuntimeDirNotDirectory);
    }
    if meta.st_uid() != effective_uid() {
        return Err(LinuxTransportError::RuntimeDirWrongOwner);
    }
    if meta.mode() & 0o077 != 0 {
        return Err(LinuxTransportError::RuntimeDirInsecureMode);
    }
    Ok(root.join(APP_DIR).join(SOCKET_NAME))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Identity {
    dev: u64,
    ino: u64,
}
fn identity(meta: &Metadata) -> Identity {
    Identity {
        dev: meta.st_dev(),
        ino: meta.st_ino(),
    }
}
fn effective_uid() -> u32 {
    unsafe { libc::geteuid() }
}

/// A verified, publishable owner-only attach listener.
#[derive(Debug)]
pub struct OwnedUnixListener {
    listener: UnixListener,
    path: PathBuf,
    parent_identity: Identity,
    socket_identity: Identity,
    uid: u32,
}

impl OwnedUnixListener {
    pub fn bind() -> Result<Self, LinuxTransportError> {
        Self::bind_in_path(endpoint_path()?)
    }

    pub fn bind_in(root: &Path) -> Result<Self, LinuxTransportError> {
        Self::bind_in_path(endpoint_path_in(root)?)
    }

    fn bind_in_path(path: PathBuf) -> Result<Self, LinuxTransportError> {
        let uid = effective_uid();
        let parent = path.parent().ok_or(LinuxTransportError::AppDirUnsafe)?;
        let runtime_root = parent
            .parent()
            .ok_or(LinuxTransportError::RuntimeDirNotAbsolute)?;
        if endpoint_path_in(runtime_root)? != path {
            return Err(LinuxTransportError::EndpointVerificationFailed);
        }
        match fs::symlink_metadata(parent) {
            Ok(meta) => verify_dir(&meta, uid)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let mut builder = DirBuilder::new();
                builder
                    .mode(0o700)
                    .create(parent)
                    .map_err(LinuxTransportError::Io)?;
                fs::set_permissions(parent, Permissions::from_mode(0o700))?;
            }
            Err(error) => return Err(LinuxTransportError::Io(error)),
        }
        let parent_meta = fs::symlink_metadata(parent)?;
        verify_dir(&parent_meta, uid)?;
        let parent_identity = identity(&parent_meta);

        recover_existing(&path, parent, parent_identity, uid)?;
        let listener = UnixListener::bind(&path)?;
        if let Err(error) = fs::set_permissions(&path, Permissions::from_mode(0o600)) {
            let _ = fs::remove_file(&path);
            return Err(LinuxTransportError::Io(error));
        }
        let socket_meta = fs::symlink_metadata(&path)?;
        if !socket_meta.file_type().is_socket()
            || socket_meta.st_uid() != uid
            || socket_meta.mode() & 0o777 != 0o600
        {
            let _ = fs::remove_file(&path);
            return Err(LinuxTransportError::EndpointVerificationFailed);
        }
        let current_parent = fs::symlink_metadata(parent)?;
        if identity(&current_parent) != parent_identity || verify_dir(&current_parent, uid).is_err()
        {
            let _ = fs::remove_file(&path);
            return Err(LinuxTransportError::EndpointVerificationFailed);
        }
        Ok(Self {
            listener,
            path,
            parent_identity,
            socket_identity: identity(&socket_meta),
            uid,
        })
    }

    pub fn local_path(&self) -> &Path {
        &self.path
    }
    pub fn inner(&self) -> &UnixListener {
        &self.listener
    }

    /// Accept one connection and authenticate it with `SO_PEERCRED` before return.
    pub fn accept(&self) -> Result<AcceptedConnection, LinuxTransportError> {
        let (stream, _) = self.listener.accept()?;
        let peer = peer_credentials(&stream)?;
        if peer.uid != self.uid {
            return Err(LinuxTransportError::PeerUidMismatch);
        }
        Ok(AcceptedConnection { stream, peer })
    }
}

impl Drop for OwnedUnixListener {
    fn drop(&mut self) {
        let Some(parent) = self.path.parent() else {
            return;
        };
        let Ok(parent_meta) = fs::symlink_metadata(parent) else {
            return;
        };
        if identity(&parent_meta) != self.parent_identity
            || verify_dir(&parent_meta, self.uid).is_err()
        {
            return;
        }
        let Ok(socket_meta) = fs::symlink_metadata(&self.path) else {
            return;
        };
        if socket_meta.file_type().is_socket()
            && socket_meta.st_uid() == self.uid
            && identity(&socket_meta) == self.socket_identity
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn verify_dir(meta: &Metadata, uid: u32) -> Result<(), LinuxTransportError> {
    if !meta.file_type().is_dir() || meta.st_uid() != uid || meta.mode() & 0o777 != 0o700 {
        return Err(LinuxTransportError::AppDirUnsafe);
    }
    Ok(())
}

fn recover_existing(
    path: &Path,
    parent: &Path,
    parent_identity: Identity,
    uid: u32,
) -> Result<(), LinuxTransportError> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(LinuxTransportError::Io(error)),
    };
    if !meta.file_type().is_socket() || meta.st_uid() != uid {
        return Err(LinuxTransportError::ExistingEndpointUnsafe);
    }
    match UnixStream::connect(path) {
        Ok(_) => return Err(LinuxTransportError::EndpointInUse),
        Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {}
        Err(_) => return Err(LinuxTransportError::ExistingEndpointUnsafe),
    }
    let parent_now = fs::symlink_metadata(parent).map_err(LinuxTransportError::Io)?;
    let endpoint_now = fs::symlink_metadata(path).map_err(LinuxTransportError::Io)?;
    if identity(&parent_now) != parent_identity
        || verify_dir(&parent_now, uid).is_err()
        || identity(&endpoint_now) != identity(&meta)
        || !endpoint_now.file_type().is_socket()
        || endpoint_now.st_uid() != uid
    {
        return Err(LinuxTransportError::ExistingEndpointUnsafe);
    }
    fs::remove_file(path)?;
    Ok(())
}

fn peer_credentials(stream: &UnixStream) -> Result<PeerCredentials, LinuxTransportError> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut credentials as *mut _ as *mut libc::c_void,
            &mut length,
        )
    };
    if result != 0 || length as usize != std::mem::size_of::<libc::ucred>() {
        return Err(LinuxTransportError::PeerCredentialsUnavailable);
    }
    Ok(PeerCredentials {
        pid: credentials.pid as u32,
        uid: credentials.uid,
        gid: credentials.gid,
    })
}
