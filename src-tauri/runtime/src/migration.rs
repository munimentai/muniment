//! Runtime composition for a prepared migration handoff.

use crate::handoff_listener::{run_bound_handoff_listener, HandoffListenerError};
use muniment_attach::{
    handshake_migration_control_stream, ClientError, MigrationControlFailure,
    MigrationControlOutcome,
};
use muniment_core::attach::linux::{
    AttachFilesystem, AttachFilesystemError, AttachTransport, InstanceLockError,
};
use muniment_core::attach::{mint_handoff_nonce, HandoffNonceRandomnessError};
use std::fmt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

const RETRY_INTERVAL: Duration = Duration::from_millis(10);
const MAX_CONTROL_DEADLINE: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationTakeoverError {
    NonceRandomness,
    Filesystem,
    DesktopUnavailable,
    Control(ClientError),
    ControlRequest(MigrationControlFailure),
    MigrationNotReady,
    Unauthorized,
    UnsupportedOperation,
    InstanceLock,
    DeadlineElapsed,
    Listener(HandoffListenerError),
}

impl fmt::Display for MigrationTakeoverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonceRandomness => formatter.write_str("handoff nonce randomness is unavailable"),
            Self::Filesystem => formatter.write_str("runtime handoff filesystem setup failed"),
            Self::DesktopUnavailable => {
                formatter.write_str("the desktop attach endpoint is unavailable")
            }
            Self::Control(error) => error.fmt(formatter),
            Self::ControlRequest(error) => error.fmt(formatter),
            Self::MigrationNotReady => formatter.write_str("the migration is not ready"),
            Self::Unauthorized => formatter.write_str("the migration request is unauthorized"),
            Self::UnsupportedOperation => {
                formatter.write_str("the desktop does not support migration control")
            }
            Self::InstanceLock => formatter.write_str("runtime handoff instance lock failed"),
            Self::DeadlineElapsed => formatter.write_str("the migration takeover deadline elapsed"),
            Self::Listener(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for MigrationTakeoverError {}

/// Requests a migration handoff and owns its readiness listener until `stop` fires.
pub fn run_migration_takeover(
    profile_directory: impl AsRef<Path>,
    deadline: Instant,
    stop: Receiver<()>,
) -> Result<(), MigrationTakeoverError> {
    let nonce = mint_handoff_nonce().map_err(map_nonce_error)?;
    let filesystem =
        AttachFilesystem::from_runtime_directory(profile_directory.as_ref().as_os_str())
            .map_err(map_filesystem_error)?;

    request_handoff(&filesystem, &nonce, deadline)?;

    let instance_lock = loop {
        check_deadline(deadline)?;
        match filesystem.acquire_instance_lock() {
            Ok(lock) => break lock,
            Err(InstanceLockError::AlreadyHeld) => wait_to_retry(deadline)?,
            Err(_) => return Err(MigrationTakeoverError::InstanceLock),
        }
    };
    let transport = loop {
        check_deadline(deadline)?;
        match AttachTransport::bind(&filesystem) {
            Ok(transport) => break transport,
            Err(_) => wait_to_retry(deadline)?,
        }
    };

    run_bound_handoff_listener(instance_lock, transport, &nonce, stop)
        .map_err(MigrationTakeoverError::Listener)
}

fn request_handoff(
    filesystem: &AttachFilesystem,
    nonce: &str,
    deadline: Instant,
) -> Result<(), MigrationTakeoverError> {
    loop {
        let io_timeout = remaining(deadline)?;
        let stream = UnixStream::connect(filesystem.endpoint_path())
            .map_err(|_| MigrationTakeoverError::DesktopUnavailable)?;
        let mut client = handshake_migration_control_stream(
            stream,
            env!("CARGO_PKG_VERSION"),
            io_timeout.min(MAX_CONTROL_DEADLINE),
        )
        .map_err(MigrationTakeoverError::Control)?;
        let remaining = remaining(deadline)?;
        let deadline_ms = remaining
            .min(MAX_CONTROL_DEADLINE)
            .as_millis()
            .clamp(1, 60_000) as u64;
        match client.control_migration(nonce, deadline_ms) {
            Ok(MigrationControlOutcome::Accepted) => return Ok(()),
            Ok(MigrationControlOutcome::MigrationNotReady { retryable: true }) => {
                wait_to_retry(deadline)?
            }
            Ok(MigrationControlOutcome::MigrationNotReady { retryable: false }) => {
                return Err(MigrationTakeoverError::MigrationNotReady)
            }
            Ok(MigrationControlOutcome::Unauthorized) => {
                return Err(MigrationTakeoverError::Unauthorized)
            }
            Ok(MigrationControlOutcome::UnsupportedOperation) => {
                return Err(MigrationTakeoverError::UnsupportedOperation)
            }
            Err(error) => return Err(MigrationTakeoverError::ControlRequest(error)),
        }
    }
}

fn remaining(deadline: Instant) -> Result<Duration, MigrationTakeoverError> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or(MigrationTakeoverError::DeadlineElapsed)
}

fn check_deadline(deadline: Instant) -> Result<(), MigrationTakeoverError> {
    remaining(deadline).map(|_| ())
}

fn wait_to_retry(deadline: Instant) -> Result<(), MigrationTakeoverError> {
    std::thread::sleep(remaining(deadline)?.min(RETRY_INTERVAL));
    check_deadline(deadline)
}

fn map_nonce_error(_: HandoffNonceRandomnessError) -> MigrationTakeoverError {
    MigrationTakeoverError::NonceRandomness
}

fn map_filesystem_error(_: AttachFilesystemError) -> MigrationTakeoverError {
    MigrationTakeoverError::Filesystem
}
