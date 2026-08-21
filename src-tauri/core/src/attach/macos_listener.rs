//! macOS attach listener admission.

use std::os::unix::net::{UnixListener, UnixStream};

use super::{
    verify_macos_attach_peer, verify_macos_attach_peer_with_reader, MacosPeerReader,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MacosAttachAcceptError {
    Accept,
    PeerRejected,
}

/// Accepts a stream and verifies its owner before any frame read.
pub fn accept_macos_attach(listener: &UnixListener) -> Result<UnixStream, MacosAttachAcceptError> {
    let (stream, _) = listener.accept().map_err(|_| MacosAttachAcceptError::Accept)?;
    verify_macos_attach_peer(&stream).map_err(|_| MacosAttachAcceptError::PeerRejected)?;
    Ok(stream)
}

/// Accepts a stream through an injected peer identity boundary.
#[doc(hidden)]
pub fn accept_macos_attach_with_reader(
    listener: &UnixListener,
    reader: &impl MacosPeerReader,
) -> Result<UnixStream, MacosAttachAcceptError> {
    let (stream, _) = listener.accept().map_err(|_| MacosAttachAcceptError::Accept)?;
    verify_macos_attach_peer_with_reader(&stream, reader)
        .map_err(|_| MacosAttachAcceptError::PeerRejected)?;
    Ok(stream)
}
