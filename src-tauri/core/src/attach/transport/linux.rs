//! Fail-closed Linux pathname Unix-socket transport.

use super::AttachListener;
use std::env;
use std::fmt;
use std::fs::{self, DirBuilder, Metadata, Permissions};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::linux::fs::MetadataExt;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

const APP_DIRECTORY: &str = "muniment";
const SOCKET_NAME: &str = "attach-v1.sock";
const QUARANTINE_NAME: &str = ".attach-v1.removing";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxTransportError {
    RuntimeDirectoryMissing,
    RuntimeDirectoryNotAbsolute,
    RuntimeDirectoryInsecure,
    AppDirectoryInsecure,
    EndpointInsecure,
    EndpointLiveOrAmbiguous,
    PeerCredentialsUnavailable,
    PeerUidMismatch,
    Io,
}

impl fmt::Display for LinuxTransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::RuntimeDirectoryMissing => "attach runtime directory is unavailable",
            Self::RuntimeDirectoryNotAbsolute => "attach runtime directory is invalid",
            Self::RuntimeDirectoryInsecure => "attach runtime directory is insecure",
            Self::AppDirectoryInsecure => "attach application directory is insecure",
            Self::EndpointInsecure => "attach endpoint is insecure",
            Self::EndpointLiveOrAmbiguous => "attach endpoint is already active or ambiguous",
            Self::PeerCredentialsUnavailable => "attach peer credentials are unavailable",
            Self::PeerUidMismatch => "attach peer identity does not match",
            Self::Io => "attach transport operation failed",
        })
    }
}

impl std::error::Error for LinuxTransportError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerCredentials {
    pub pid: i32,
    pub uid: u32,
    pub gid: u32,
}

/// Injectable only to make authentication failure contract-testable without
/// requiring a second OS account.
pub trait PeerCredentialProvider: Send + Sync {
    fn credentials(&self, socket: RawFd) -> Result<PeerCredentials, LinuxTransportError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SoPeerCredentialProvider;

impl PeerCredentialProvider for SoPeerCredentialProvider {
    fn credentials(&self, socket: RawFd) -> Result<PeerCredentials, LinuxTransportError> {
        let mut credentials = libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: both pointers reference writable storage of the advertised size.
        let result = unsafe {
            libc::getsockopt(
                socket,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut credentials as *mut libc::ucred).cast(),
                &mut length,
            )
        };
        if result != 0 || length as usize != std::mem::size_of::<libc::ucred>() {
            return Err(LinuxTransportError::PeerCredentialsUnavailable);
        }
        Ok(PeerCredentials {
            pid: credentials.pid,
            uid: credentials.uid,
            gid: credentials.gid,
        })
    }
}

/// Test seam for exercising pathname-to-descriptor replacement races.
#[doc(hidden)]
pub trait ParentOpenHook {
    fn before_parent_open(&self, _parent: &Path) {}
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoParentOpenHook;

impl ParentOpenHook for NoParentOpenHook {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Identity {
    device: u64,
    inode: u64,
}

impl Identity {
    fn of(metadata: &Metadata) -> Self {
        Self {
            device: metadata.st_dev(),
            inode: metadata.st_ino(),
        }
    }
}

pub struct LinuxAttachListener<C = SoPeerCredentialProvider> {
    listener: Option<UnixListener>,
    endpoint: PathBuf,
    parent_fd: OwnedFd,
    socket_identity: Identity,
    effective_uid: u32,
    credentials: C,
}

impl LinuxAttachListener<SoPeerCredentialProvider> {
    pub fn bind() -> Result<Self, LinuxTransportError> {
        let runtime = env::var_os("XDG_RUNTIME_DIR")
            .filter(|value| !value.is_empty())
            .ok_or(LinuxTransportError::RuntimeDirectoryMissing)?;
        Self::bind_in(Path::new(&runtime))
    }

    pub fn bind_in(runtime: &Path) -> Result<Self, LinuxTransportError> {
        Self::bind_with_credentials(runtime, SoPeerCredentialProvider)
    }
}

impl<C: PeerCredentialProvider> LinuxAttachListener<C> {
    pub fn bind_with_credentials(
        runtime: &Path,
        credentials: C,
    ) -> Result<Self, LinuxTransportError> {
        Self::bind_with_credentials_and_hook(runtime, credentials, NoParentOpenHook)
    }

    #[doc(hidden)]
    pub fn bind_with_credentials_and_hook<H: ParentOpenHook>(
        runtime: &Path,
        credentials: C,
        hook: H,
    ) -> Result<Self, LinuxTransportError> {
        if !runtime.is_absolute() {
            return Err(LinuxTransportError::RuntimeDirectoryNotAbsolute);
        }
        let effective_uid = unsafe { libc::geteuid() };
        verify_directory(
            runtime,
            effective_uid,
            0,
            LinuxTransportError::RuntimeDirectoryInsecure,
        )?;

        let parent = runtime.join(APP_DIRECTORY);
        let mut builder = DirBuilder::new();
        builder.mode(0o700);
        match builder.create(&parent) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(LinuxTransportError::Io),
        }
        let parent_metadata = verify_directory(
            &parent,
            effective_uid,
            0o700,
            LinuxTransportError::AppDirectoryInsecure,
        )?;
        let parent_identity = Identity::of(&parent_metadata);
        let endpoint = parent.join(SOCKET_NAME);
        hook.before_parent_open(&parent);
        let parent_fd = open_directory(&parent)?;
        verify_open_directory(&parent_fd, parent_identity, effective_uid)?;
        let anchored_endpoint = anchored_path(&parent_fd, SOCKET_NAME);

        remove_stale_endpoint(&parent_fd, effective_uid)?;
        let listener = UnixListener::bind(&anchored_endpoint).map_err(|error| {
            if error.kind() == io::ErrorKind::AddrInUse {
                LinuxTransportError::EndpointLiveOrAmbiguous
            } else {
                LinuxTransportError::Io
            }
        })?;
        let socket_fd = open_nofollow(&parent_fd, SOCKET_NAME)?;
        let bound_identity = fs::metadata(anchored_fd_path(&socket_fd))
            .map(|metadata| Identity::of(&metadata))
            .map_err(|_| LinuxTransportError::EndpointInsecure)?;
        if fs::set_permissions(anchored_fd_path(&socket_fd), Permissions::from_mode(0o600)).is_err()
        {
            let _ = remove_matching_endpoint(&parent_fd, Some(bound_identity), effective_uid);
            return Err(LinuxTransportError::Io);
        }
        if !same_directory(&parent, parent_identity, effective_uid) {
            let _ = remove_matching_endpoint(&parent_fd, Some(bound_identity), effective_uid);
            return Err(LinuxTransportError::AppDirectoryInsecure);
        }
        let socket_metadata = match verify_socket(&anchored_endpoint, effective_uid, Some(0o600)) {
            Ok(metadata) => metadata,
            Err(error) => {
                let _ = remove_matching_endpoint(&parent_fd, Some(bound_identity), effective_uid);
                return Err(error);
            }
        };

        Ok(Self {
            listener: Some(listener),
            endpoint,
            parent_fd,
            socket_identity: Identity::of(&socket_metadata),
            effective_uid,
            credentials,
        })
    }

    pub fn endpoint(&self) -> &Path {
        &self.endpoint
    }
}

impl<C: PeerCredentialProvider> AttachListener for LinuxAttachListener<C> {
    type Stream = UnixStream;
    type Error = LinuxTransportError;

    fn accept(&self) -> Result<Self::Stream, Self::Error> {
        let (stream, _) = self
            .listener
            .as_ref()
            .ok_or(LinuxTransportError::Io)?
            .accept()
            .map_err(|_| LinuxTransportError::Io)?;
        let peer = self.credentials.credentials(stream.as_raw_fd())?;
        if peer.uid != self.effective_uid {
            return Err(LinuxTransportError::PeerUidMismatch);
        }
        Ok(stream)
    }
}

impl<C> Drop for LinuxAttachListener<C> {
    fn drop(&mut self) {
        self.listener.take();
        let _ = remove_matching_endpoint(
            &self.parent_fd,
            Some(self.socket_identity),
            self.effective_uid,
        );
    }
}

fn verify_directory(
    path: &Path,
    uid: u32,
    exact_mode: u32,
    error: LinuxTransportError,
) -> Result<Metadata, LinuxTransportError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| error)?;
    let mode = metadata.st_mode() & 0o777;
    if !metadata.file_type().is_dir()
        || metadata.st_uid() != uid
        || mode & 0o077 != 0
        || (exact_mode != 0 && mode != exact_mode)
    {
        return Err(error);
    }
    Ok(metadata)
}

fn same_directory(path: &Path, identity: Identity, uid: u32) -> bool {
    verify_directory(path, uid, 0o700, LinuxTransportError::AppDirectoryInsecure)
        .map(|metadata| Identity::of(&metadata) == identity)
        .unwrap_or(false)
}

fn verify_socket(
    path: &Path,
    uid: u32,
    exact_mode: Option<u32>,
) -> Result<Metadata, LinuxTransportError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| LinuxTransportError::EndpointInsecure)?;
    if !metadata.file_type().is_socket()
        || metadata.st_uid() != uid
        || exact_mode.is_some_and(|mode| metadata.st_mode() & 0o777 != mode)
    {
        return Err(LinuxTransportError::EndpointInsecure);
    }
    Ok(metadata)
}

fn remove_stale_endpoint(parent_fd: &OwnedFd, uid: u32) -> Result<(), LinuxTransportError> {
    let endpoint = anchored_path(parent_fd, SOCKET_NAME);
    let first = match fs::symlink_metadata(&endpoint) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(LinuxTransportError::EndpointLiveOrAmbiguous),
    };
    if !first.file_type().is_socket() || first.st_uid() != uid {
        return Err(LinuxTransportError::EndpointInsecure);
    }
    match UnixStream::connect(&endpoint) {
        Ok(_) => return Err(LinuxTransportError::EndpointLiveOrAmbiguous),
        Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {}
        Err(_) => return Err(LinuxTransportError::EndpointLiveOrAmbiguous),
    }
    remove_matching_endpoint(parent_fd, Some(Identity::of(&first)), uid)
}

fn open_directory(path: &Path) -> Result<OwnedFd, LinuxTransportError> {
    let path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| LinuxTransportError::AppDirectoryInsecure)?;
    // SAFETY: path is a valid C string; the returned descriptor is uniquely owned.
    let fd = unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        return Err(LinuxTransportError::AppDirectoryInsecure);
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn verify_open_directory(
    fd: &OwnedFd,
    expected: Identity,
    uid: u32,
) -> Result<(), LinuxTransportError> {
    let metadata = fs::metadata(anchored_fd_path(fd))
        .map_err(|_| LinuxTransportError::AppDirectoryInsecure)?;
    if !metadata.file_type().is_dir()
        || metadata.st_uid() != uid
        || metadata.st_mode() & 0o777 != 0o700
        || Identity::of(&metadata) != expected
    {
        return Err(LinuxTransportError::AppDirectoryInsecure);
    }
    Ok(())
}

fn open_nofollow(parent: &OwnedFd, name: &str) -> Result<OwnedFd, LinuxTransportError> {
    let name = std::ffi::CString::new(name).expect("constant contains no NUL");
    // O_PATH permits opening a socket filesystem object without following links.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(LinuxTransportError::EndpointInsecure);
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn anchored_path(parent: &OwnedFd, name: &str) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}/{}", parent.as_raw_fd(), name))
}

fn anchored_fd_path(fd: &OwnedFd) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", fd.as_raw_fd()))
}

/// Atomically moves the name out of service before deciding whether it is safe
/// to unlink. A raced replacement is restored with `RENAME_NOREPLACE`.
fn remove_matching_endpoint(
    parent: &OwnedFd,
    expected: Option<Identity>,
    uid: u32,
) -> Result<(), LinuxTransportError> {
    let quarantine = anchored_path(parent, QUARANTINE_NAME);
    rename_noreplace(parent, SOCKET_NAME, QUARANTINE_NAME)?;
    let metadata = fs::symlink_metadata(&quarantine)
        .map_err(|_| LinuxTransportError::EndpointLiveOrAmbiguous)?;
    let matches = metadata.file_type().is_socket()
        && metadata.st_uid() == uid
        && expected.is_none_or(|identity| Identity::of(&metadata) == identity);
    if matches {
        fs::remove_file(&quarantine).map_err(|_| LinuxTransportError::EndpointLiveOrAmbiguous)
    } else {
        // Do not overwrite a newer endpoint while restoring the object we moved.
        let old = std::ffi::CString::new(QUARANTINE_NAME).unwrap();
        let new = std::ffi::CString::new(SOCKET_NAME).unwrap();
        let result = unsafe {
            libc::syscall(
                libc::SYS_renameat2,
                parent.as_raw_fd(),
                old.as_ptr(),
                parent.as_raw_fd(),
                new.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if result != 0 {
            return Err(LinuxTransportError::EndpointLiveOrAmbiguous);
        }
        Err(LinuxTransportError::EndpointInsecure)
    }
}

fn rename_noreplace(
    parent: &OwnedFd,
    old_name: &str,
    new_name: &str,
) -> Result<(), LinuxTransportError> {
    let old = std::ffi::CString::new(old_name).unwrap();
    let new = std::ffi::CString::new(new_name).unwrap();
    // SAFETY: names are valid C strings and both operations are anchored to the
    // already verified directory descriptor.
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            parent.as_raw_fd(),
            old.as_ptr(),
            parent.as_raw_fd(),
            new.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result != 0 {
        return Err(LinuxTransportError::EndpointLiveOrAmbiguous);
    }
    Ok(())
}
