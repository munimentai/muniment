//! Linux attach connection routing.

use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use muniment_attach::{decode_frame, Hello};

use super::linux::PeerCredentials;
use super::verify_approval_presenter_peer_with_reader;
use crate::browser_control::LinuxProcReader;

const ROUTE_PEEK_CAP: usize = 4 * 1024;

/// The handler for a new attach connection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttachConnectionRoute {
    ApprovalPresenter,
    Companion,
}

/// Names the route without consuming the connection's first frame.
pub fn name_attach_connection_route(
    stream: &UnixStream,
    peer_credentials: PeerCredentials,
    expected_desktop_executable: &Path,
    process_reader: &dyn LinuxProcReader,
    timeout: Duration,
) -> AttachConnectionRoute {
    let peer_authorized = u32::try_from(peer_credentials.pid).is_ok_and(|peer_pid| {
        verify_approval_presenter_peer_with_reader(
            peer_pid,
            expected_desktop_executable,
            process_reader,
        )
        .is_ok()
    });
    if !peer_authorized || timeout.is_zero() {
        return AttachConnectionRoute::Companion;
    }

    let Ok(previous_timeout) = stream.read_timeout() else {
        return AttachConnectionRoute::Companion;
    };
    if stream.set_read_timeout(Some(timeout)).is_err() {
        return AttachConnectionRoute::Companion;
    }

    let route = peek_hello(stream).map_or(AttachConnectionRoute::Companion, |hello| {
        if hello.client.kind == "desktop" {
            AttachConnectionRoute::ApprovalPresenter
        } else {
            AttachConnectionRoute::Companion
        }
    });
    let _ = stream.set_read_timeout(previous_timeout);
    route
}

fn peek_hello(stream: &UnixStream) -> Option<Hello> {
    let mut prefix = [0_u8; 4];
    peek_exact(stream, &mut prefix).ok()?;
    let frame_length = (u32::from_be_bytes(prefix) as usize).checked_add(prefix.len())?;
    if frame_length > ROUTE_PEEK_CAP {
        return None;
    }

    let mut frame = vec![0_u8; frame_length];
    peek_exact(stream, &mut frame).ok()?;
    match decode_frame::<Hello>(&frame) {
        Ok(Some((hello, consumed))) if consumed == frame.len() => Some(hello),
        _ => None,
    }
}

fn peek_exact(stream: &UnixStream, buffer: &mut [u8]) -> io::Result<()> {
    loop {
        let read = unsafe {
            libc::recv(
                stream.as_raw_fd(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                libc::MSG_PEEK | libc::MSG_WAITALL,
            )
        };
        if read == buffer.len() as isize {
            return Ok(());
        }
        if read == -1 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
            continue;
        }
        return Err(if read == -1 {
            io::Error::last_os_error()
        } else {
            io::Error::from(io::ErrorKind::UnexpectedEof)
        });
    }
}
