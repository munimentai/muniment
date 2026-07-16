//! Fail-closed Linux transport for the local companion attach protocol.

use std::env;
use std::ffi::CString;
use std::fs::{self, File, Metadata};
use std::io;
use std::os::linux::fs::MetadataExt as LinuxMetadataExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const APP_DIR: &str = "muniment";
const SOCKET_NAME: &str = "attach-v1.sock";
static UNIQUE_NAME: AtomicU64 = AtomicU64::new(0);

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
    parent: File,
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
        let parent_path = path.parent().ok_or(LinuxTransportError::AppDirUnsafe)?;
        let runtime_root = parent_path
            .parent()
            .ok_or(LinuxTransportError::RuntimeDirNotAbsolute)?;
        if endpoint_path_in(runtime_root)? != path {
            return Err(LinuxTransportError::EndpointVerificationFailed);
        }
        let runtime = open_runtime_dir(runtime_root, uid)?;
        let created = match mkdir_at(&runtime, APP_DIR, 0o700) {
            Ok(()) => true,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => false,
            Err(error) => return Err(error.into()),
        };
        let parent = open_at(
            &runtime,
            APP_DIR,
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        )
        .map_err(|_| LinuxTransportError::AppDirUnsafe)?;
        if created {
            chmod_file(&parent, 0o700)?;
        }
        verify_dir(&parent.metadata()?, uid)?;
        recover_existing(&parent, &path, uid)?;

        // Bind under an unguessable private name. Permission and identity checks happen
        // there before one atomic rename publishes it as the endpoint.
        let temporary = unique_name(".attach-v1.bind");
        let temporary_path = PathBuf::from(format!(
            "/proc/self/fd/{}/{}",
            parent.as_raw_fd(),
            temporary
        ));
        let listener = UnixListener::bind(&temporary_path)?;
        let socket_file = match open_at(&parent, &temporary, libc::O_PATH | libc::O_NOFOLLOW) {
            Ok(file) => file,
            Err(error) => {
                let _ = quarantine_remove(&parent, &temporary, None, uid);
                return Err(error.into());
            }
        };
        let before = match socket_file.metadata() {
            Ok(meta) => meta,
            Err(error) => {
                let _ = quarantine_remove(&parent, &temporary, None, uid);
                return Err(error.into());
            }
        };
        if !before.file_type().is_socket() || before.st_uid() != uid {
            let _ = quarantine_remove(&parent, &temporary, Some(identity(&before)), uid);
            return Err(LinuxTransportError::EndpointVerificationFailed);
        }
        if let Err(error) = chmod_fd(&socket_file, 0o600) {
            let _ = quarantine_remove(&parent, &temporary, Some(identity(&before)), uid);
            return Err(error.into());
        }
        let after = match socket_file.metadata() {
            Ok(meta) => meta,
            Err(error) => {
                let _ = quarantine_remove(&parent, &temporary, Some(identity(&before)), uid);
                return Err(error.into());
            }
        };
        let socket_identity = identity(&after);
        if socket_identity != identity(&before) || after.mode() & 0o777 != 0o600 {
            let _ = quarantine_remove(&parent, &temporary, Some(socket_identity), uid);
            return Err(LinuxTransportError::EndpointVerificationFailed);
        }
        if let Err(error) = rename_noreplace(&parent, &temporary, SOCKET_NAME) {
            let _ = quarantine_remove(&parent, &temporary, Some(socket_identity), uid);
            return Err(if error.kind() == io::ErrorKind::AlreadyExists {
                LinuxTransportError::ExistingEndpointUnsafe
            } else {
                error.into()
            });
        }
        let published = match stat_at(&parent, SOCKET_NAME) {
            Ok(meta) => meta,
            Err(error) => {
                let _ = quarantine_remove(&parent, SOCKET_NAME, Some(socket_identity), uid);
                return Err(error.into());
            }
        };
        if identity(&published) != socket_identity
            || !published.file_type().is_socket()
            || published.st_uid() != uid
            || published.mode() & 0o777 != 0o600
        {
            let _ = quarantine_remove(&parent, SOCKET_NAME, Some(socket_identity), uid);
            return Err(LinuxTransportError::EndpointVerificationFailed);
        }
        Ok(Self {
            listener,
            path,
            parent,
            socket_identity,
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
        let _ = quarantine_remove(
            &self.parent,
            SOCKET_NAME,
            Some(self.socket_identity),
            self.uid,
        );
    }
}

fn verify_dir(meta: &Metadata, uid: u32) -> Result<(), LinuxTransportError> {
    if !meta.file_type().is_dir() || meta.st_uid() != uid || meta.mode() & 0o777 != 0o700 {
        return Err(LinuxTransportError::AppDirUnsafe);
    }
    Ok(())
}

fn open_runtime_dir(path: &Path, uid: u32) -> Result<File, LinuxTransportError> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.file_type().is_dir() || meta.st_uid() != uid || meta.mode() & 0o077 != 0 {
        return Err(LinuxTransportError::RuntimeDirUnavailable);
    }
    Ok(file)
}

fn mkdir_at(parent: &File, name: &str, mode: libc::mode_t) -> io::Result<()> {
    let name = c_name(name)?;
    let result = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), mode) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn chmod_file(file: &File, mode: libc::mode_t) -> io::Result<()> {
    let result = unsafe { libc::fchmod(file.as_raw_fd(), mode) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn recover_existing(parent: &File, path: &Path, uid: u32) -> Result<(), LinuxTransportError> {
    let meta = match stat_at(parent, SOCKET_NAME) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if !meta.file_type().is_socket() || meta.st_uid() != uid {
        return Err(LinuxTransportError::ExistingEndpointUnsafe);
    }
    match UnixStream::connect(path) {
        Ok(_) => return Err(LinuxTransportError::EndpointInUse),
        Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {}
        Err(_) => return Err(LinuxTransportError::ExistingEndpointUnsafe),
    }
    quarantine_remove(parent, SOCKET_NAME, Some(identity(&meta)), uid)
        .map_err(|_| LinuxTransportError::ExistingEndpointUnsafe)
}

// Atomically move a name out of service before inspecting it. If it is not the
// expected inode, it is deliberately left in quarantine and never unlinked.
fn quarantine_remove(
    parent: &File,
    name: &str,
    expected: Option<Identity>,
    uid: u32,
) -> io::Result<()> {
    let quarantine = unique_name(".attach-v1.quarantine");
    rename_noreplace(parent, name, &quarantine)?;
    let meta = stat_at(parent, &quarantine)?;
    if expected.is_none()
        || expected.is_some_and(|wanted| identity(&meta) != wanted)
        || !meta.file_type().is_socket()
        || meta.st_uid() != uid
    {
        let _ = rename_noreplace(parent, &quarantine, name);
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "quarantined endpoint identity changed",
        ));
    }
    unlink_at(parent, &quarantine)
}

fn unique_name(prefix: &str) -> String {
    format!(
        "{prefix}.{}.{}",
        std::process::id(),
        UNIQUE_NAME.fetch_add(1, Ordering::Relaxed)
    )
}

fn c_name(name: &str) -> io::Result<CString> {
    CString::new(name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid endpoint name"))
}

fn open_at(parent: &File, name: &str, flags: i32) -> io::Result<File> {
    let name = c_name(name)?;
    let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags | libc::O_CLOEXEC) };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}

fn stat_at(parent: &File, name: &str) -> io::Result<Metadata> {
    open_at(parent, name, libc::O_PATH | libc::O_NOFOLLOW)?.metadata()
}

fn rename_noreplace(parent: &File, old: &str, new: &str) -> io::Result<()> {
    let old = c_name(old)?;
    let new = c_name(new)?;
    let result = unsafe {
        libc::renameat2(
            parent.as_raw_fd(),
            old.as_ptr(),
            parent.as_raw_fd(),
            new.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn unlink_at(parent: &File, name: &str) -> io::Result<()> {
    let name = c_name(name)?;
    let result = unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn chmod_fd(file: &File, mode: libc::mode_t) -> io::Result<()> {
    chmod_fd_with(file, mode, fchmodat2)
}

fn chmod_fd_with(
    file: &File,
    mode: libc::mode_t,
    fchmodat2: impl FnOnce(RawFd, libc::mode_t) -> io::Result<()>,
) -> io::Result<()> {
    match fchmodat2(file.as_raw_fd(), mode) {
        Ok(()) => Ok(()),
        Err(error) if error.raw_os_error() == Some(libc::ENOSYS) => {
            // fchmod(2) rejects O_PATH descriptors, and fchmodat2(2) was only
            // added in Linux 6.5. The procfs magic link resolves the already
            // pinned inode, so replacing the endpoint pathname cannot redirect
            // this compatibility path.
            let path = CString::new(format!("/proc/self/fd/{}", file.as_raw_fd()))
                .expect("file descriptor path contains no NUL bytes");
            let result = unsafe { libc::chmod(path.as_ptr(), mode) };
            if result == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        }
        Err(error) => Err(error),
    }
}

fn fchmodat2(fd: RawFd, mode: libc::mode_t) -> io::Result<()> {
    let empty = c"";
    let result = unsafe {
        libc::syscall(
            libc::SYS_fchmodat2,
            fd,
            empty.as_ptr(),
            mode,
            libc::AT_EMPTY_PATH,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::Permissions;
    use std::os::unix::fs::PermissionsExt;

    fn replacement_is_preserved_when_expected_identity_is_stale(name: &str) {
        let root = env::temp_dir().join(unique_name("muniment-attach-race-test"));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, Permissions::from_mode(0o700)).unwrap();
        let parent = open_runtime_dir(&root, effective_uid()).unwrap();
        let path = root.join(name);

        let stale = UnixListener::bind(&path).unwrap();
        let stale_identity = identity(&fs::symlink_metadata(&path).unwrap());
        drop(stale);
        let held_stale = root.join(unique_name(".held-stale"));
        fs::rename(&path, &held_stale).unwrap();
        let replacement = UnixListener::bind(&path).unwrap();
        let replacement_identity = identity(&fs::symlink_metadata(&path).unwrap());

        assert!(quarantine_remove(&parent, name, Some(stale_identity), effective_uid()).is_err());
        assert_eq!(
            identity(&fs::symlink_metadata(&path).unwrap()),
            replacement_identity
        );

        drop(replacement);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stale_recovery_does_not_delete_a_raced_replacement() {
        replacement_is_preserved_when_expected_identity_is_stale(SOCKET_NAME);
    }

    #[test]
    fn bind_error_cleanup_does_not_delete_a_raced_replacement() {
        replacement_is_preserved_when_expected_identity_is_stale(&unique_name(".bind"));
    }

    #[test]
    fn chmod_fd_falls_back_when_fchmodat2_is_unavailable() {
        let root = env::temp_dir().join(unique_name("muniment-attach-chmod-test"));
        fs::create_dir(&root).unwrap();
        let path = root.join("socket");
        let listener = UnixListener::bind(&path).unwrap();
        let socket = open_at(
            &File::open(&root).unwrap(),
            "socket",
            libc::O_PATH | libc::O_NOFOLLOW,
        )
        .unwrap();

        chmod_fd_with(&socket, 0o600, |_, _| {
            Err(io::Error::from_raw_os_error(libc::ENOSYS))
        })
        .unwrap();

        assert_eq!(socket.metadata().unwrap().mode() & 0o777, 0o600);
        drop(listener);
        fs::remove_dir_all(root).unwrap();
    }
}
