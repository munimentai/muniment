//! Probe-only attach listener used during a prepared runtime handoff.

use muniment_attach::{decode_frame, encode_frame, negotiate_first, welcome, FirstMessage};
use muniment_core::attach::linux::{
    AttachAcceptError, AttachFilesystem, AttachTransport, InstanceLockError,
};
use std::fmt;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::Duration;

const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
const DESKTOP_PROTOCOL: muniment_attach::VersionRange =
    muniment_attach::VersionRange { min: 1, max: 1 };
const SERVER_NONCE: &str = "runtime-handoff-readiness";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandoffListenerError {
    Filesystem,
    InstanceLock,
    Bind,
    Accept,
}

impl fmt::Display for HandoffListenerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Filesystem => "runtime handoff filesystem setup failed",
            Self::InstanceLock => "runtime handoff instance lock could not be acquired",
            Self::Bind => "runtime handoff endpoint bind failed",
            Self::Accept => "runtime handoff connection accept failed",
        })
    }
}

impl std::error::Error for HandoffListenerError {}

/// Owns the profile endpoint and answers readiness probes until `stop` fires.
pub fn run_handoff_listener(
    profile_directory: impl AsRef<Path>,
    handoff_nonce: &str,
    stop: Receiver<()>,
) -> Result<(), HandoffListenerError> {
    let filesystem =
        AttachFilesystem::from_runtime_directory(profile_directory.as_ref().as_os_str())
            .map_err(|_| HandoffListenerError::Filesystem)?;
    let _instance_lock = filesystem
        .acquire_instance_lock()
        .map_err(|_: InstanceLockError| HandoffListenerError::InstanceLock)?;
    let transport = AttachTransport::bind(&filesystem).map_err(|_| HandoffListenerError::Bind)?;
    let stop_handle = transport.stop_handle();
    let finished = Arc::new(AtomicBool::new(false));

    std::thread::scope(|scope| {
        let stop_finished = finished.clone();
        let stop_thread = scope.spawn(move || {
            while !stop_finished.load(Ordering::Acquire) {
                match stop.recv_timeout(Duration::from_millis(10)) {
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => {}
                }
            }
            stop_handle.stop();
        });
        let result = loop {
            match transport.accept() {
                Ok((stream, _)) => answer_probe(stream, handoff_nonce),
                Err(AttachAcceptError::Closed) => break Ok(()),
                Err(AttachAcceptError::WrongUid(_)) => continue,
                Err(_) => break Err(HandoffListenerError::Accept),
            }
        };
        finished.store(true, Ordering::Release);
        stop_thread
            .join()
            .expect("handoff stop thread does not panic");
        result
    })
}

fn answer_probe(mut stream: UnixStream, handoff_nonce: &str) {
    let _ = stream.set_read_timeout(Some(HELLO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(HELLO_TIMEOUT));
    let Ok(message) = read_first_message(&mut stream) else {
        return;
    };
    let Ok(selected) = negotiate_first(message, DESKTOP_PROTOCOL) else {
        return;
    };
    let response = welcome(selected, env!("CARGO_PKG_VERSION"), SERVER_NONCE, "")
        .with_handoff_nonce(handoff_nonce);
    if let Ok(frame) = encode_frame(&response) {
        let _ = stream.write_all(&frame);
    }
}

fn read_first_message(stream: &mut UnixStream) -> io::Result<FirstMessage> {
    let mut prefix = [0_u8; 4];
    stream.read_exact(&mut prefix)?;
    let length = u32::from_be_bytes(prefix) as usize;
    if length > muniment_attach::MAX_FRAME_LENGTH {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mut frame = vec![0_u8; 4 + length];
    frame[..4].copy_from_slice(&prefix);
    stream.read_exact(&mut frame[4..])?;
    decode_frame(&frame)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid frame"))?
        .map(|(message, _)| message)
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "incomplete frame"))
}
