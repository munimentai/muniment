#![cfg(target_os = "macos")]

use muniment_core::attach::{
    verify_macos_attach_peer_with_reader, MacosPeerError, MacosPeerReadError, MacosPeerReader,
};
use std::cell::Cell;
use std::io::{self, Read};
use std::os::fd::RawFd;
use std::os::unix::net::UnixStream;

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
}
