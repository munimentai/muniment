#![cfg(target_os = "macos")]

use muniment_core::attach::{
    accept_macos_attach_with_reader, verify_macos_attach_peer_with_reader, MacosAttachAcceptError,
    MacosPeerError, MacosPeerReadError, MacosPeerReader,
};
use std::cell::Cell;
use std::io::{self, Read};
use std::os::fd::RawFd;
use std::os::unix::net::{UnixListener, UnixStream};

struct FakePeerReader {
    peer_uid: Result<libc::uid_t, MacosPeerReadError>,
    local_uid: libc::uid_t,
    reads: Cell<usize>,
}

impl MacosPeerReader for FakePeerReader {
    fn peer_effective_uid(&self, _socket: RawFd) -> Result<libc::uid_t, MacosPeerReadError> {
        self.reads.set(self.reads.get() + 1);
        self.peer_uid
    }

    fn local_effective_uid(&self) -> libc::uid_t {
        self.local_uid
    }
}

fn verify(reader: &FakePeerReader) -> Result<(), MacosPeerError> {
    let (stream, _peer) = UnixStream::pair().unwrap();
    verify_macos_attach_peer_with_reader(&stream, reader)
}

fn verify_and_assert_no_response(reader: &FakePeerReader) -> Result<(), MacosPeerError> {
    let (stream, mut peer) = UnixStream::pair().unwrap();
    let result = verify_macos_attach_peer_with_reader(&stream, reader);
    peer.set_nonblocking(true).unwrap();
    assert!(matches!(
        peer.read(&mut [0]),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock
    ));
    result
}

fn accept_and_assert_no_response(reader: &FakePeerReader) {
    let path = std::env::temp_dir().join(format!(
        "muniment-attach-peer-{}-{}.sock",
        std::process::id(),
        reader as *const FakePeerReader as usize
    ));
    let listener = UnixListener::bind(&path).unwrap();
    let mut client = UnixStream::connect(&path).unwrap();

    assert_eq!(
        accept_macos_attach_with_reader(&listener, reader).unwrap_err(),
        MacosAttachAcceptError::PeerRejected
    );
    client.set_nonblocking(true).unwrap();
    assert_eq!(client.read(&mut [0]).unwrap(), 0);

    drop(listener);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn accepts_a_matching_effective_uid() {
    let reader = FakePeerReader {
        peer_uid: Ok(501),
        local_uid: 501,
        reads: Cell::new(0),
    };

    assert_eq!(verify(&reader), Ok(()));
    assert_eq!(reader.reads.get(), 1);
}

#[test]
fn rejects_a_mismatched_effective_uid() {
    let reader = FakePeerReader {
        peer_uid: Ok(502),
        local_uid: 501,
        reads: Cell::new(0),
    };

    assert_eq!(
        verify_and_assert_no_response(&reader),
        Err(MacosPeerError::WrongUid)
    );
    assert_eq!(reader.reads.get(), 1);
    accept_and_assert_no_response(&reader);
}

#[test]
fn rejects_a_peer_identity_syscall_failure() {
    let reader = FakePeerReader {
        peer_uid: Err(MacosPeerReadError),
        local_uid: 501,
        reads: Cell::new(0),
    };

    assert_eq!(
        verify_and_assert_no_response(&reader),
        Err(MacosPeerError::IdentityUnavailable)
    );
    assert_eq!(reader.reads.get(), 1);
    accept_and_assert_no_response(&reader);
}
