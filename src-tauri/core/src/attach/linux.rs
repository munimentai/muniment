//! Owner-only Linux Unix-socket transport for companion attach.

use std::env;
use std::ffi::{CString, OsStr};
use std::fmt;
use std::fs::{self, File, Metadata, OpenOptions, Permissions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::linux::fs::MetadataExt;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const DIRECTORY_MODE: u32 = 0o700;
const SOCKET_MODE: u32 = 0o600;
const ENDPOINT_NAME: &str = "attach-v1.sock";
static NEXT_QUARANTINE: AtomicU64 = AtomicU64::new(0);

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
    parent_handle: File,
    endpoint: PathBuf,
    uid: u32,
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
        let runtime_handle = open_verified_directory(runtime, uid, true)?;
        let parent = runtime.join("muniment");
        let parent_handle = open_or_create_parent(&runtime_handle, uid)?;
        let parent_metadata = parent_handle.metadata().map_err(|_| LinuxAttachError::Io)?;
        let parent_identity = Identity::of(&parent_metadata);
        let endpoint = parent.join("attach-v1.sock");
        prepare_endpoint(&parent_handle, uid)?;

        let handle_endpoint = handle_path(&parent_handle);
        let listener = UnixListener::bind(&handle_endpoint).map_err(|_| LinuxAttachError::Io)?;
        if set_endpoint_mode(&parent_handle).is_err() {
            let _ = remove_verified(&parent_handle, uid, None);
            return Err(LinuxAttachError::Io);
        }

        let current_parent = verify_parent(&parent, uid)?;
        if Identity::of(&current_parent) != parent_identity {
            return Err(LinuxAttachError::AttachDirectoryInvalid);
        }
        let socket_metadata =
            endpoint_metadata(&parent_handle)?.ok_or(LinuxAttachError::EndpointInvalid)?;
        verify_socket_metadata(&socket_metadata, uid)?;
        if socket_metadata.st_mode() & 0o777 != SOCKET_MODE {
            return Err(LinuxAttachError::EndpointInvalid);
        }
        let socket_identity = Identity::of(&socket_metadata);
        Ok(Self {
            listener,
            parent_handle,
            endpoint,
            uid,
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
        remove_verified(&self.parent_handle, self.uid, Some(self.socket_identity))
    }
}

impl Drop for LinuxAttachListener {
    fn drop(&mut self) {
        let _ = remove_verified(&self.parent_handle, self.uid, Some(self.socket_identity));
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

fn verify_socket_metadata(metadata: &Metadata, uid: u32) -> Result<(), LinuxAttachError> {
    if !metadata.file_type().is_socket()
        || metadata.file_type().is_symlink()
        || metadata.st_uid() != uid
    {
        return Err(LinuxAttachError::EndpointInvalid);
    }
    Ok(())
}

fn prepare_endpoint(parent: &File, uid: u32) -> Result<(), LinuxAttachError> {
    let Some(metadata) = endpoint_metadata(parent)? else {
        return Ok(());
    };
    verify_socket_metadata(&metadata, uid)?;
    match UnixStream::connect(handle_path(parent)) {
        Ok(_) => return Err(LinuxAttachError::EndpointInUse),
        Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {}
        Err(_) => return Err(LinuxAttachError::EndpointInUse),
    }
    remove_verified(parent, uid, Some(Identity::of(&metadata)))
}

fn remove_verified(
    parent: &File,
    uid: u32,
    socket_id: Option<Identity>,
) -> Result<(), LinuxAttachError> {
    remove_verified_with(parent, uid, socket_id, || {})
}

fn remove_verified_with(
    parent: &File,
    uid: u32,
    socket_id: Option<Identity>,
    before_rename: impl FnOnce(),
) -> Result<(), LinuxAttachError> {
    let Some(metadata) = endpoint_metadata(parent)? else {
        return Ok(());
    };
    verify_socket_metadata(&metadata, uid)?;
    if socket_id.is_some_and(|identity| identity != Identity::of(&metadata)) {
        return Err(LinuxAttachError::EndpointInvalid);
    }
    before_rename();

    // Linux has no inode-conditional unlink. Atomically move the name aside,
    // then validate the moved inode before unlinking it. If another process
    // won the race and supplied a replacement, put it back and fail closed.
    let quarantine = format!(
        ".attach-v1.sock.remove-{}-{}",
        std::process::id(),
        NEXT_QUARANTINE.fetch_add(1, Ordering::Relaxed)
    );
    rename_at_noreplace(parent, ENDPOINT_NAME, &quarantine)?;
    let moved = metadata_at(parent, &quarantine)?.ok_or(LinuxAttachError::Io)?;
    let expected = Identity::of(&metadata);
    if Identity::of(&moved) != expected {
        rename_at_noreplace(parent, &quarantine, ENDPOINT_NAME)?;
        return Err(LinuxAttachError::EndpointInvalid);
    }
    unlink_at(parent, &quarantine)
}

fn open_verified_directory(path: &Path, uid: u32, runtime: bool) -> Result<File, LinuxAttachError> {
    if runtime && !path.is_absolute() {
        return Err(LinuxAttachError::RuntimeDirectoryInvalid);
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| {
            if runtime {
                LinuxAttachError::RuntimeDirectoryInvalid
            } else {
                LinuxAttachError::AttachDirectoryInvalid
            }
        })?;
    let metadata = file.metadata().map_err(|_| LinuxAttachError::Io)?;
    let mode = metadata.st_mode() & 0o777;
    let insecure_mode = if runtime {
        mode & 0o077 != 0
    } else {
        mode != DIRECTORY_MODE
    };
    if metadata.st_uid() != uid || insecure_mode {
        return Err(if runtime {
            LinuxAttachError::RuntimeDirectoryInsecure
        } else {
            LinuxAttachError::AttachDirectoryInsecure
        });
    }
    Ok(file)
}

fn open_or_create_parent(runtime: &File, uid: u32) -> Result<File, LinuxAttachError> {
    let name = cstring("muniment")?;
    // SAFETY: the directory fd and NUL-terminated name are valid for the call.
    let result = unsafe { libc::mkdirat(runtime.as_raw_fd(), name.as_ptr(), DIRECTORY_MODE) };
    let created = result == 0;
    if result != 0 && io::Error::last_os_error().kind() != io::ErrorKind::AlreadyExists {
        return Err(LinuxAttachError::Io);
    }
    let path = PathBuf::from(format!("/proc/self/fd/{}/muniment", runtime.as_raw_fd()));
    if created {
        fs::set_permissions(&path, Permissions::from_mode(DIRECTORY_MODE))
            .map_err(|_| LinuxAttachError::Io)?;
    }
    open_verified_directory(&path, uid, false)
}

fn handle_path(parent: &File) -> PathBuf {
    PathBuf::from(format!(
        "/proc/self/fd/{}/{}",
        parent.as_raw_fd(),
        ENDPOINT_NAME
    ))
}

fn endpoint_metadata(parent: &File) -> Result<Option<Metadata>, LinuxAttachError> {
    metadata_at(parent, ENDPOINT_NAME)
}

fn metadata_at(parent: &File, name: &str) -> Result<Option<Metadata>, LinuxAttachError> {
    let path = PathBuf::from(format!("/proc/self/fd/{}/{}", parent.as_raw_fd(), name));
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(LinuxAttachError::Io),
    }
}

fn set_endpoint_mode(parent: &File) -> Result<(), LinuxAttachError> {
    let name = cstring(ENDPOINT_NAME)?;
    // SAFETY: arguments reference a live directory and valid C string.
    let result = unsafe { libc::fchmodat(parent.as_raw_fd(), name.as_ptr(), SOCKET_MODE, 0) };
    if result == 0 {
        Ok(())
    } else {
        Err(LinuxAttachError::Io)
    }
}

fn rename_at_noreplace(parent: &File, from: &str, to: &str) -> Result<(), LinuxAttachError> {
    let from = cstring(from)?;
    let to = cstring(to)?;
    // SAFETY: both names are valid and resolved relative to the same live directory fd.
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            parent.as_raw_fd(),
            from.as_ptr(),
            parent.as_raw_fd(),
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(LinuxAttachError::Io)
    }
}

fn unlink_at(parent: &File, name: &str) -> Result<(), LinuxAttachError> {
    let name = cstring(name)?;
    // SAFETY: the name is valid and the directory fd remains live.
    let result = unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) };
    if result == 0 {
        Ok(())
    } else {
        Err(LinuxAttachError::Io)
    }
}

fn cstring(value: impl AsRef<OsStr>) -> Result<CString, LinuxAttachError> {
    CString::new(value.as_ref().as_bytes()).map_err(|_| LinuxAttachError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn test_parent() -> (PathBuf, File) {
        let root = env::temp_dir().join(format!(
            "muniment-linux-race-{}-{}",
            std::process::id(),
            NEXT_QUARANTINE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(DIRECTORY_MODE)).unwrap();
        let handle = open_verified_directory(&root, effective_uid(), false).unwrap();
        (root, handle)
    }

    #[test]
    fn removal_restores_socket_replaced_after_validation() {
        let (parent_path, parent) = test_parent();
        let endpoint = parent_path.join(ENDPOINT_NAME);
        let original = UnixListener::bind(&endpoint).unwrap();
        let identity = Identity::of(&fs::symlink_metadata(&endpoint).unwrap());
        let mut replacement = None;
        let mut replacement_identity = None;

        let result = remove_verified_with(&parent, effective_uid(), Some(identity), || {
            fs::remove_file(&endpoint).unwrap();
            replacement = Some(UnixListener::bind(&endpoint).unwrap());
            replacement_identity = Some(Identity::of(&fs::symlink_metadata(&endpoint).unwrap()));
        });

        assert_eq!(result, Err(LinuxAttachError::EndpointInvalid));
        assert_eq!(
            Identity::of(&fs::symlink_metadata(&endpoint).unwrap()),
            replacement_identity.unwrap()
        );
        drop(original);
        drop(replacement);
        fs::remove_dir_all(parent_path).unwrap();
    }

    #[test]
    fn parent_handle_does_not_follow_a_path_replacement() {
        let (parent_path, parent) = test_parent();
        let moved = parent_path.with_extension("moved");
        fs::rename(&parent_path, &moved).unwrap();
        fs::create_dir(&parent_path).unwrap();
        fs::set_permissions(&parent_path, fs::Permissions::from_mode(DIRECTORY_MODE)).unwrap();

        let listener = UnixListener::bind(handle_path(&parent)).unwrap();
        assert!(moved.join(ENDPOINT_NAME).exists());
        assert!(!parent_path.join(ENDPOINT_NAME).exists());

        drop(listener);
        fs::remove_dir_all(parent_path).unwrap();
        fs::remove_dir_all(moved).unwrap();
    }
}
