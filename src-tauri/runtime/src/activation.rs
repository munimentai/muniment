//! Runtime attach service activation.

use crate::{
    run_bound_attach_listener, run_migration_takeover, AttachListenerError, MigrationTakeoverError,
    RuntimeAttachState,
};
use muniment_core::attach::linux::{AttachFilesystem, AttachTransport, InstanceLockError};
use std::fmt;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{mpsc, mpsc::Receiver, Arc};
use std::time::{Duration, Instant};

const RETENTION_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

enum ActivationCommand {
    Stop,
    Retention,
    Finished,
}

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
    run_runtime_activation_with_retention_trigger(
        runtime_directory,
        profile_directory,
        config_directory,
        takeover_deadline,
        expected_desktop_executable,
        None,
        stop,
    )
}

/// Runs activation with a trigger for retention schedule tests.
#[doc(hidden)]
pub fn run_runtime_activation_with_retention_trigger(
    runtime_directory: impl AsRef<Path>,
    profile_directory: impl AsRef<Path>,
    config_directory: impl AsRef<Path>,
    takeover_deadline: Instant,
    expected_desktop_executable: Option<PathBuf>,
    retention_trigger: Option<Receiver<()>>,
    stop: Receiver<()>,
) -> Result<(), RuntimeActivationError> {
    let runtime_directory = runtime_directory.as_ref();
    let state = Arc::new(
        RuntimeAttachState::open(profile_directory, config_directory)
            .map_err(|_| RuntimeActivationError::State)?,
    );
    let filesystem = AttachFilesystem::from_runtime_directory(runtime_directory.as_os_str())
        .map_err(|_| RuntimeActivationError::Filesystem)?;
    let (command_tx, command_rx) = mpsc::channel();
    let stop_commands = command_tx.clone();
    std::thread::spawn(move || {
        let _ = stop.recv();
        let _ = stop_commands.send(ActivationCommand::Stop);
    });
    if let Some(retention_trigger) = retention_trigger {
        let trigger_commands = command_tx.clone();
        std::thread::spawn(move || {
            while retention_trigger.recv().is_ok() {
                if trigger_commands.send(ActivationCommand::Retention).is_err() {
                    break;
                }
            }
        });
    }
    let (listener_stop_tx, listener_stop_rx) = mpsc::channel();
    let retention_state = Arc::clone(&state);
    let retention_thread = std::thread::spawn(move || {
        let _ = retention_state.apply_recorded_retention();
        loop {
            match command_rx.recv_timeout(RETENTION_INTERVAL) {
                Ok(ActivationCommand::Retention) | Err(mpsc::RecvTimeoutError::Timeout) => {
                    let _ = retention_state.apply_recorded_retention();
                }
                Ok(ActivationCommand::Stop) => {
                    let _ = listener_stop_tx.send(());
                    break;
                }
                Ok(ActivationCommand::Finished) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    break
                }
            }
        }
    });
    let result = match filesystem.acquire_instance_lock() {
        Ok(instance_lock) => {
            let transport = match AttachTransport::bind(&filesystem) {
                Ok(transport) => transport,
                Err(_) => {
                    let _ = command_tx.send(ActivationCommand::Finished);
                    retention_thread
                        .join()
                        .expect("retention schedule does not panic");
                    return Err(RuntimeActivationError::Bind);
                }
            };
            let service_state = Arc::clone(&state);
            let mut inputs = state.attach_listener_inputs();
            inputs.expected_desktop_executable = expected_desktop_executable;
            run_bound_attach_listener(
                instance_lock,
                transport,
                inputs,
                None,
                move || service_state.attach_service(),
                listener_stop_rx,
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
                listener_stop_rx,
            )
            .map_err(RuntimeActivationError::Migration)
        }
        Err(_) => Err(RuntimeActivationError::InstanceLock),
    };
    let _ = command_tx.send(ActivationCommand::Finished);
    retention_thread
        .join()
        .expect("retention schedule does not panic");
    result
}
