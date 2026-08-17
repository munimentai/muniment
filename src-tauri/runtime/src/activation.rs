//! Runtime attach service activation.

use crate::{
    run_bound_attach_listener, run_migration_takeover, AttachListenerError, MigrationTakeoverError,
    RuntimeAttachState,
};
use muniment_core::attach::linux::{AttachFilesystem, AttachTransport, InstanceLockError};
use std::fmt;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{mpsc::Receiver, Arc};
use std::time::Instant;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeActivationError {
    State,
    Filesystem,
    InstanceLock,
    Bind,
    Listener(AttachListenerError),
    Migration(MigrationTakeoverError),
}

impl fmt::Display for RuntimeActivationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::State => "runtime attach state could not be opened",
            Self::Filesystem => "runtime attach filesystem setup failed",
            Self::InstanceLock => "runtime attach instance lock could not be acquired",
            Self::Bind => "runtime attach endpoint bind failed",
            Self::Listener(error) => return error.fmt(formatter),
            Self::Migration(error) => return error.fmt(formatter),
        })
    }
}

impl std::error::Error for RuntimeActivationError {}

/// Opens runtime state and serves the attach endpoint until `stop` fires.
pub fn run_runtime_activation(
    runtime_directory: impl AsRef<Path>,
    profile_directory: impl AsRef<Path>,
    config_directory: impl AsRef<Path>,
    takeover_deadline: Instant,
    stop: Receiver<()>,
) -> Result<(), RuntimeActivationError> {
    run_runtime_activation_with_desktop_executable(
        runtime_directory,
        profile_directory,
        config_directory,
        takeover_deadline,
        crate::installed_desktop_executable(),
        stop,
    )
}

/// Runs activation with an explicit desktop executable for contract tests.
#[doc(hidden)]
pub fn run_runtime_activation_with_desktop_executable(
    runtime_directory: impl AsRef<Path>,
    profile_directory: impl AsRef<Path>,
    config_directory: impl AsRef<Path>,
    takeover_deadline: Instant,
    expected_desktop_executable: Option<PathBuf>,
    stop: Receiver<()>,
) -> Result<(), RuntimeActivationError> {
    let runtime_directory = runtime_directory.as_ref();
    let state = Arc::new(
        RuntimeAttachState::open(profile_directory, config_directory)
            .map_err(|_| RuntimeActivationError::State)?,
    );
    let filesystem = AttachFilesystem::from_runtime_directory(runtime_directory.as_os_str())
        .map_err(|_| RuntimeActivationError::Filesystem)?;

    match filesystem.acquire_instance_lock() {
        Ok(instance_lock) => {
            let transport =
                AttachTransport::bind(&filesystem).map_err(|_| RuntimeActivationError::Bind)?;
            let service_state = Arc::clone(&state);
            let mut inputs = state.attach_listener_inputs();
            inputs.expected_desktop_executable = expected_desktop_executable;
            run_bound_attach_listener(
                instance_lock,
                transport,
                inputs,
                None,
                move || service_state.attach_service(),
                stop,
            )
            .map_err(RuntimeActivationError::Listener)
        }
        Err(InstanceLockError::AlreadyHeld) => {
            let service_state = Arc::clone(&state);
            let mut inputs = state.attach_listener_inputs();
            inputs.expected_desktop_executable = expected_desktop_executable;
            run_migration_takeover(
                runtime_directory,
                inputs,
                move || service_state.attach_service(),
                takeover_deadline,
                stop,
            )
            .map_err(RuntimeActivationError::Migration)
        }
        Err(_) => Err(RuntimeActivationError::InstanceLock),
    }
}
