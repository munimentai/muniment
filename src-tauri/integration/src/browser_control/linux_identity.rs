//! Linux browser executable identity verification.

use std::fmt;
use std::fs;
use std::mem;
use std::net::{IpAddr, SocketAddr};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::RawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// The process identity observed while determining the loopback connection owner.
pub use muniment_core::process_reader::ProcessIdentity as BrowserProcessIdentity;
pub use muniment_core::process_reader::{LinuxProcReader, ProcReadError, ProcReader};

/// Proof that a live process is the desktop-selected browser executable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthorizedBrowserProcess(());

/// A bounded failure reason. Variants deliberately carry no sensitive values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerificationError {
    ProcessUnavailable,
    ProcessIdentityChanged,
    ExpectedExecutableInvalid,
    ProcessExecutableInvalid,
    ExecutableMismatch,
}

/// A bounded, redacted failure while resolving a loopback socket owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolutionError {
    InvalidEndpoint,
    DiagnosticUnavailable,
    MalformedDiagnostic,
    SocketNotFound,
    AmbiguousSocket,
    OwnerUnavailable,
    AmbiguousOwner,
    ProcessIdentityChanged,
}

/// A bounded, redacted failure while authorizing a loopback browser connection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthorizationError {
    OwnerResolutionFailed,
    ExecutableVerificationFailed,
}

impl fmt::Display for AuthorizationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::OwnerResolutionFailed => "browser connection owner could not be resolved",
            Self::ExecutableVerificationFailed => "browser connection owner was not authorized",
        })
    }
}

impl std::error::Error for AuthorizationError {}

impl fmt::Display for ResolutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidEndpoint => "connection endpoints are invalid",
            Self::DiagnosticUnavailable => "socket diagnostic is unavailable",
            Self::MalformedDiagnostic => "socket diagnostic response is invalid",
            Self::SocketNotFound => "connection socket was not found",
            Self::AmbiguousSocket => "connection socket is ambiguous",
            Self::OwnerUnavailable => "socket owner is unavailable",
            Self::AmbiguousOwner => "socket owner is ambiguous",
            Self::ProcessIdentityChanged => "socket owner identity changed",
        })
    }
}

impl std::error::Error for ResolutionError {}

impl fmt::Display for VerificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::ProcessUnavailable => "browser process is unavailable",
            Self::ProcessIdentityChanged => "browser process identity changed",
            Self::ExpectedExecutableInvalid => "expected browser executable is invalid",
            Self::ProcessExecutableInvalid => "browser process executable is invalid",
            Self::ExecutableMismatch => "browser executable does not match",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for VerificationError {}

/// Injected Linux socket-diagnostic boundary.
pub trait LinuxSocketDiagnostic {
    fn response(
        &self,
        local: SocketAddr,
        peer: SocketAddr,
        sequence: u32,
    ) -> Result<Vec<u8>, ProcReadError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SocketDiagnostic;

struct OwnedFd(RawFd);
impl Drop for OwnedFd {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.0);
        }
    }
}

impl LinuxSocketDiagnostic for SocketDiagnostic {
    fn response(
        &self,
        local: SocketAddr,
        peer: SocketAddr,
        sequence: u32,
    ) -> Result<Vec<u8>, ProcReadError> {
        let fd = unsafe {
            libc::socket(
                libc::AF_NETLINK,
                libc::SOCK_RAW | libc::SOCK_CLOEXEC,
                libc::NETLINK_SOCK_DIAG,
            )
        };
        if fd < 0 {
            return Err(ProcReadError);
        }
        let fd = OwnedFd(fd);
        let mut address: libc::sockaddr_nl = unsafe { mem::zeroed() };
        address.nl_family = libc::AF_NETLINK as u16;
        let bound = unsafe {
            libc::bind(
                fd.0,
                &address as *const _ as *const libc::sockaddr,
                mem::size_of_val(&address) as libc::socklen_t,
            )
        };
        if bound != 0 {
            return Err(ProcReadError);
        }

        let mut request = [0u8; 72];
        request[0..4].copy_from_slice(&(72u32).to_ne_bytes());
        request[4..6].copy_from_slice(&SOCK_DIAG_BY_FAMILY.to_ne_bytes());
        request[6..8]
            .copy_from_slice(&(libc::NLM_F_REQUEST as u16 | libc::NLM_F_DUMP as u16).to_ne_bytes());
        request[8..12].copy_from_slice(&sequence.to_ne_bytes());
        request[16] = if local.is_ipv4() {
            libc::AF_INET as u8
        } else {
            libc::AF_INET6 as u8
        };
        request[17] = libc::IPPROTO_TCP as u8;
        request[20..24].copy_from_slice(&(1u32 << TCP_ESTABLISHED).to_ne_bytes());
        // Query the browser-owned client half of the accepted connection.
        request[24..26].copy_from_slice(&peer.port().to_be_bytes());
        request[26..28].copy_from_slice(&local.port().to_be_bytes());
        match (local.ip(), peer.ip()) {
            (IpAddr::V4(a), IpAddr::V4(b)) => {
                request[28..32].copy_from_slice(&b.octets());
                request[44..48].copy_from_slice(&a.octets());
            }
            (IpAddr::V6(a), IpAddr::V6(b)) => {
                request[28..44].copy_from_slice(&b.octets());
                request[44..60].copy_from_slice(&a.octets());
            }
            _ => return Err(ProcReadError),
        }
        request[64..72].fill(0xff); // INET_DIAG_NOCOOKIE
        let sent = unsafe {
            libc::send(
                fd.0,
                request.as_ptr() as *const libc::c_void,
                request.len(),
                0,
            )
        };
        if sent != request.len() as isize {
            return Err(ProcReadError);
        }
        let mut result = Vec::new();
        loop {
            let mut buffer = [0u8; 16 * 1024];
            let read = unsafe {
                libc::recv(
                    fd.0,
                    buffer.as_mut_ptr() as *mut libc::c_void,
                    buffer.len(),
                    0,
                )
            };
            if read <= 0 {
                return Err(ProcReadError);
            }
            result.extend_from_slice(&buffer[..read as usize]);
            if netlink_contains_done(&buffer[..read as usize], sequence)? {
                return Ok(result);
            }
        }
    }
}

fn netlink_contains_done(bytes: &[u8], sequence: u32) -> Result<bool, ProcReadError> {
    let mut offset = 0;
    while offset < bytes.len() {
        if bytes.len() - offset < 16 {
            return Err(ProcReadError);
        }
        let len = u32::from_ne_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .map_err(|_| ProcReadError)?,
        ) as usize;
        if len < 16 || len > bytes.len() - offset {
            return Err(ProcReadError);
        }
        let kind = u16::from_ne_bytes(
            bytes[offset + 4..offset + 6]
                .try_into()
                .map_err(|_| ProcReadError)?,
        );
        let flags = u16::from_ne_bytes(
            bytes[offset + 6..offset + 8]
                .try_into()
                .map_err(|_| ProcReadError)?,
        );
        let seq = u32::from_ne_bytes(
            bytes[offset + 8..offset + 12]
                .try_into()
                .map_err(|_| ProcReadError)?,
        );
        if seq != sequence || kind == libc::NLMSG_ERROR as u16 || flags != NLM_F_MULTI {
            return Err(ProcReadError);
        }
        if kind == NLMSG_DONE {
            return Ok(true);
        }
        offset += (len + 3) & !3;
    }
    Ok(false)
}

static NEXT_SEQUENCE: AtomicU32 = AtomicU32::new(1);

/// Resolves and verifies the owner of an accepted loopback browser connection.
pub fn authorize_browser_process(
    local: SocketAddr,
    peer: SocketAddr,
    expected_executable: &Path,
) -> Result<AuthorizedBrowserProcess, AuthorizationError> {
    authorize_browser_process_with_readers(
        local,
        peer,
        expected_executable,
        &SocketDiagnostic,
        &ProcReader,
    )
}

/// Authorizes using injected socket-diagnostic and procfs boundaries.
#[doc(hidden)]
pub fn authorize_browser_process_with_readers(
    local: SocketAddr,
    peer: SocketAddr,
    expected_executable: &Path,
    diagnostic: &impl LinuxSocketDiagnostic,
    procfs: &impl LinuxProcReader,
) -> Result<AuthorizedBrowserProcess, AuthorizationError> {
    let observed = resolve_browser_process_with_readers(local, peer, diagnostic, procfs)
        .map_err(|_| AuthorizationError::OwnerResolutionFailed)?;
    verify_browser_process_with_reader(observed, expected_executable, procfs)
        .map_err(|_| AuthorizationError::ExecutableVerificationFailed)
}

/// Resolves an accepted numeric loopback TCP connection to its live process.
pub fn resolve_browser_process(
    local: SocketAddr,
    peer: SocketAddr,
) -> Result<BrowserProcessIdentity, ResolutionError> {
    resolve_browser_process_with_readers(local, peer, &SocketDiagnostic, &ProcReader)
}

#[doc(hidden)]
pub fn resolve_browser_process_with_readers(
    local: SocketAddr,
    peer: SocketAddr,
    diagnostic: &impl LinuxSocketDiagnostic,
    procfs: &impl LinuxProcReader,
) -> Result<BrowserProcessIdentity, ResolutionError> {
    validate_endpoints(local, peer)?;
    let sequence = NEXT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let bytes = diagnostic
        .response(local, peer, sequence)
        .map_err(|_| ResolutionError::DiagnosticUnavailable)?;
    let inodes = parse_diagnostic(&bytes, local, peer, sequence)?;
    let inode = match inodes.as_slice() {
        [] => return Err(ResolutionError::SocketNotFound),
        [inode] => *inode,
        _ => return Err(ResolutionError::AmbiguousSocket),
    };
    let owners = procfs
        .socket_owners(inode)
        .map_err(|_| ResolutionError::OwnerUnavailable)?;
    let owner = match owners.as_slice() {
        [] => return Err(ResolutionError::OwnerUnavailable),
        [owner] => *owner,
        _ => return Err(ResolutionError::AmbiguousOwner),
    };
    let after = procfs
        .start_identity(owner.pid)
        .map_err(|_| ResolutionError::OwnerUnavailable)?;
    if after != owner.start_identity {
        return Err(ResolutionError::ProcessIdentityChanged);
    }
    let confirmed = procfs
        .socket_owners(inode)
        .map_err(|_| ResolutionError::OwnerUnavailable)?;
    if confirmed.as_slice() != [owner] {
        return Err(if confirmed.len() > 1 {
            ResolutionError::AmbiguousOwner
        } else {
            ResolutionError::ProcessIdentityChanged
        });
    }
    Ok(owner)
}

fn validate_endpoints(local: SocketAddr, peer: SocketAddr) -> Result<(), ResolutionError> {
    if local.port() == 0
        || peer.port() == 0
        || !local.ip().is_loopback()
        || !peer.ip().is_loopback()
        || mem::discriminant(&local.ip()) != mem::discriminant(&peer.ip())
        || local == peer
    {
        return Err(ResolutionError::InvalidEndpoint);
    }
    Ok(())
}

const NLMSG_DONE: u16 = 3;
const NLM_F_MULTI: u16 = 2;
const SOCK_DIAG_BY_FAMILY: u16 = 20;
const TCP_ESTABLISHED: u8 = 1;

fn parse_diagnostic(
    bytes: &[u8],
    local: SocketAddr,
    peer: SocketAddr,
    sequence: u32,
) -> Result<Vec<u32>, ResolutionError> {
    let mut offset = 0usize;
    let mut done = false;
    let mut matches = Vec::new();
    let mut response_pid = None;
    while offset < bytes.len() {
        if bytes.len() - offset < 16 {
            return Err(ResolutionError::MalformedDiagnostic);
        }
        let len = u32::from_ne_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let kind = u16::from_ne_bytes(bytes[offset + 4..offset + 6].try_into().unwrap());
        let flags = u16::from_ne_bytes(bytes[offset + 6..offset + 8].try_into().unwrap());
        let seq = u32::from_ne_bytes(bytes[offset + 8..offset + 12].try_into().unwrap());
        let pid = u32::from_ne_bytes(bytes[offset + 12..offset + 16].try_into().unwrap());
        if len < 16 || len > bytes.len() - offset || flags != NLM_F_MULTI || seq != sequence || done
        {
            return Err(ResolutionError::MalformedDiagnostic);
        }
        if response_pid
            .replace(pid)
            .is_some_and(|expected| expected != pid)
        {
            return Err(ResolutionError::MalformedDiagnostic);
        }
        if kind == NLMSG_DONE {
            // Some kernels include a zero `nlmsgerr` status in dump completion.
            if len != 16 && !(len == 20 && bytes[offset + 16..offset + 20] == [0; 4]) {
                return Err(ResolutionError::MalformedDiagnostic);
            }
            done = true;
        } else if kind == SOCK_DIAG_BY_FAMILY {
            if len < 88 {
                return Err(ResolutionError::MalformedDiagnostic);
            }
            let msg = &bytes[offset + 16..offset + 88];
            let family = msg[0];
            if msg[1] != TCP_ESTABLISHED {
                return Err(ResolutionError::MalformedDiagnostic);
            }
            let expected_family = if local.is_ipv4() {
                libc::AF_INET as u8
            } else {
                libc::AF_INET6 as u8
            };
            if family != expected_family {
                return Err(ResolutionError::MalformedDiagnostic);
            }
            let sport = u16::from_be_bytes([msg[4], msg[5]]);
            let dport = u16::from_be_bytes([msg[6], msg[7]]);
            let addresses_match = match (local.ip(), peer.ip()) {
                (IpAddr::V4(a), IpAddr::V4(b)) => {
                    msg[8..12] == b.octets() && msg[24..28] == a.octets()
                }
                (IpAddr::V6(a), IpAddr::V6(b)) => {
                    msg[8..24] == b.octets() && msg[24..40] == a.octets()
                }
                _ => false,
            };
            if sport != peer.port() || dport != local.port() || !addresses_match {
                return Err(ResolutionError::MalformedDiagnostic);
            }
            let inode = u32::from_ne_bytes(msg[68..72].try_into().unwrap());
            if inode == 0 {
                return Err(ResolutionError::MalformedDiagnostic);
            }
            matches.push(inode);
        } else {
            return Err(ResolutionError::MalformedDiagnostic);
        }
        offset += (len + 3) & !3;
        if offset > bytes.len() {
            return Err(ResolutionError::MalformedDiagnostic);
        }
    }
    if !done {
        return Err(ResolutionError::MalformedDiagnostic);
    }
    Ok(matches)
}

/// Verifies a live process against the desktop-owned expected browser path.
pub fn verify_browser_process(
    observed: BrowserProcessIdentity,
    expected_executable: &Path,
) -> Result<AuthorizedBrowserProcess, VerificationError> {
    verify_browser_process_with_reader(observed, expected_executable, &ProcReader)
}

/// Verifies using an injected procfs boundary.
#[doc(hidden)]
pub fn verify_browser_process_with_reader(
    observed: BrowserProcessIdentity,
    expected_executable: &Path,
    reader: &impl LinuxProcReader,
) -> Result<AuthorizedBrowserProcess, VerificationError> {
    if !expected_executable.is_absolute() {
        return Err(VerificationError::ExpectedExecutableInvalid);
    }

    let before = reader
        .start_identity(observed.pid)
        .map_err(|_| VerificationError::ProcessUnavailable)?;
    if before != observed.start_identity {
        return Err(VerificationError::ProcessIdentityChanged);
    }

    let actual = reader
        .executable(observed.pid)
        .map_err(|_| VerificationError::ProcessExecutableInvalid)?;
    if !actual.is_absolute() || actual.as_os_str().as_bytes().ends_with(b" (deleted)") {
        return Err(VerificationError::ProcessExecutableInvalid);
    }

    let expected = canonical_executable(expected_executable)
        .map_err(|_| VerificationError::ExpectedExecutableInvalid)?;
    let actual =
        canonical_executable(&actual).map_err(|_| VerificationError::ProcessExecutableInvalid)?;

    let after = reader
        .start_identity(observed.pid)
        .map_err(|_| VerificationError::ProcessUnavailable)?;
    if after != observed.start_identity || after != before {
        return Err(VerificationError::ProcessIdentityChanged);
    }
    if actual.as_os_str().as_bytes() != expected.as_os_str().as_bytes() {
        return Err(VerificationError::ExecutableMismatch);
    }

    Ok(AuthorizedBrowserProcess(()))
}

fn canonical_executable(path: &Path) -> std::io::Result<PathBuf> {
    fs::canonicalize(path)
}
