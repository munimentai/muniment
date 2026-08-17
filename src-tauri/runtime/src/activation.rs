//! Runtime attach service activation.

use crate::upgrade_watch::UpgradeWatch;
use crate::{
    migration::run_migration_takeover_with_activation, run_bound_attach_listener,
    AttachListenerError, MigrationTakeoverError, RuntimeAttachState,
};
use muniment_core::attach::linux::{AttachFilesystem, AttachTransport, InstanceLockError};
use std::fmt;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, mpsc::Receiver, mpsc::Sender, Arc};
use std::time::{Duration, Instant};

const RETENTION_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const UPGRADE_WATCH_INTERVAL: Duration = Duration::from_secs(1);

/// The reason a runtime activation ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeActivationExit {
    ManagerStop,
    UpgradeRefresh,
}

enum ActivationCommand {
    Stop,
    Retention,
    Finished,
}

fn start_retention_schedule(
    state: Arc<RuntimeAttachState>,
    command_rx: Receiver<ActivationCommand>,
    checked: Option<Sender<()>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let _ = state.apply_recorded_retention();
        if let Some(checked) = &checked {
            let _ = checked.send(());
        }
        loop {
            match command_rx.recv_timeout(RETENTION_INTERVAL) {
                Ok(ActivationCommand::Retention) | Err(mpsc::RecvTimeoutError::Timeout) => {
                    let _ = state.apply_recorded_retention();
                    if let Some(checked) = &checked {
                        let _ = checked.send(());
                    }
                }
                Ok(ActivationCommand::Stop) => {
                    break;
                }
                Ok(ActivationCommand::Finished) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    break;
                }
            }
        }
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeActivationError {
    State,
    Filesystem,
    InstanceLock,
    Bind,
    Listener(AttachListenerError),
    Migration(MigrationTakeoverError),
    UpgradeWatch,
}

/// Injected retention schedule channels for contract tests.
#[doc(hidden)]
pub struct RetentionScheduleTestControl {
    pub trigger: Receiver<()>,
    pub checked: Sender<()>,
}

/// Injected executable watch settings for contract tests.
#[doc(hidden)]
pub struct UpgradeWatchTestControl {
    pub path: PathBuf,
    pub poll_interval: Duration,
}

enum ExecutableConfig {
    Admission(Option<PathBuf>),
    Watch {
        expected_desktop_executable: Option<PathBuf>,
        watch: UpgradeWatch,
    },
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
            Self::UpgradeWatch => "runtime executable watch could not be started",
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
) -> Result<RuntimeActivationExit, RuntimeActivationError> {
    let runtime_executable =
        std::env::current_exe().map_err(|_| RuntimeActivationError::UpgradeWatch)?;
    run_runtime_activation_inner(
        runtime_directory,
        profile_directory,
        config_directory,
        takeover_deadline,
        Some(ExecutableConfig::Watch {
            expected_desktop_executable: crate::installed_desktop_executable(),
            watch: UpgradeWatch {
                path: runtime_executable,
                poll_interval: UPGRADE_WATCH_INTERVAL,
            },
        }),
        None,
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
) -> Result<RuntimeActivationExit, RuntimeActivationError> {
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

/// Runs activation with executable watch seams for contract tests.
#[doc(hidden)]
pub fn run_runtime_activation_with_upgrade_watch(
    runtime_directory: impl AsRef<Path>,
    profile_directory: impl AsRef<Path>,
    config_directory: impl AsRef<Path>,
    takeover_deadline: Instant,
    watch: Option<UpgradeWatchTestControl>,
    retention_control: Option<RetentionScheduleTestControl>,
    stop: Receiver<()>,
) -> Result<RuntimeActivationExit, RuntimeActivationError> {
    run_runtime_activation_inner(
        runtime_directory,
        profile_directory,
        config_directory,
        takeover_deadline,
        watch.map(|watch| ExecutableConfig::Watch {
            expected_desktop_executable: Some(watch.path.clone()),
            watch: UpgradeWatch {
                path: watch.path,
                poll_interval: watch.poll_interval,
            },
        }),
        retention_control,
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
    retention_control: Option<RetentionScheduleTestControl>,
    stop: Receiver<()>,
) -> Result<RuntimeActivationExit, RuntimeActivationError> {
    run_runtime_activation_inner(
        runtime_directory,
        profile_directory,
        config_directory,
        takeover_deadline,
        Some(ExecutableConfig::Admission(expected_desktop_executable)),
        retention_control,
        stop,
    )
}

fn run_runtime_activation_inner(
    runtime_directory: impl AsRef<Path>,
    profile_directory: impl AsRef<Path>,
    config_directory: impl AsRef<Path>,
    takeover_deadline: Instant,
    executable_config: Option<ExecutableConfig>,
    retention_control: Option<RetentionScheduleTestControl>,
    stop: Receiver<()>,
) -> Result<RuntimeActivationExit, RuntimeActivationError> {
    let runtime_directory = runtime_directory.as_ref();
    let state = Arc::new(
        RuntimeAttachState::open(profile_directory, config_directory)
            .map_err(|_| RuntimeActivationError::State)?,
    );
    let filesystem = AttachFilesystem::from_runtime_directory(runtime_directory.as_os_str())
        .map_err(|_| RuntimeActivationError::Filesystem)?;
    let (command_tx, command_rx) = mpsc::channel();
    let (listener_stop_tx, listener_stop_rx) = mpsc::channel();
    let watch_stopped = Arc::new(AtomicBool::new(false));
    let manager_stopped = Arc::new(AtomicBool::new(false));
    let refresh_pending = Arc::new(AtomicBool::new(false));
    let expected_desktop_executable = match &executable_config {
        Some(ExecutableConfig::Admission(path)) => path.clone(),
        Some(ExecutableConfig::Watch {
            expected_desktop_executable,
            ..
        }) => expected_desktop_executable.clone(),
        None => None,
    };
    let watch_thread = executable_config
        .and_then(|config| match config {
            ExecutableConfig::Watch { watch, .. } => Some(watch),
            ExecutableConfig::Admission(_) => None,
        })
        .map(|watch| {
            watch.start(
                state.drain_state(),
                state.runtime_activity(),
                Arc::clone(&watch_stopped),
                Arc::clone(&refresh_pending),
                listener_stop_tx.clone(),
            )
        })
        .transpose()
        .map_err(|_| RuntimeActivationError::UpgradeWatch)?;
    let stop_commands = command_tx.clone();
    let stop_watch = Arc::clone(&watch_stopped);
    let record_manager_stop = Arc::clone(&manager_stopped);
    std::thread::spawn(move || {
        let _ = stop.recv();
        record_manager_stop.store(true, Ordering::Release);
        stop_watch.store(true, Ordering::Release);
        let _ = stop_commands.send(ActivationCommand::Stop);
        let _ = listener_stop_tx.send(());
    });
    let retention_checked = retention_control
        .as_ref()
        .map(|control| control.checked.clone());
    if let Some(retention_control) = retention_control {
        let trigger_commands = command_tx.clone();
        std::thread::spawn(move || {
            while retention_control.trigger.recv().is_ok() {
                if trigger_commands.send(ActivationCommand::Retention).is_err() {
                    break;
                }
            }
        });
    }
    let mut retention_thread = None;
    let result = match filesystem.acquire_instance_lock() {
        Ok(instance_lock) => {
            let transport = match AttachTransport::bind(&filesystem) {
                Ok(transport) => transport,
                Err(_) => return Err(RuntimeActivationError::Bind),
            };
            retention_thread = Some(start_retention_schedule(
                Arc::clone(&state),
                command_rx,
                retention_checked,
            ));
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
            let (started_tx, started_rx) = mpsc::sync_channel(1);
            let retention_state = Arc::clone(&state);
            let takeover = run_migration_takeover_with_activation(
                runtime_directory,
                inputs,
                move || service_state.attach_service(),
                takeover_deadline,
                move || {
                    let thread =
                        start_retention_schedule(retention_state, command_rx, retention_checked);
                    let _ = started_tx.send(thread);
                },
                listener_stop_rx,
            );
            retention_thread = started_rx.try_recv().ok();
            takeover.map_err(RuntimeActivationError::Migration)
        }
        Err(_) => Err(RuntimeActivationError::InstanceLock),
    };
    let _ = command_tx.send(ActivationCommand::Finished);
    if let Some(retention_thread) = retention_thread {
        retention_thread
            .join()
            .expect("retention schedule does not panic");
    }
    watch_stopped.store(true, Ordering::Release);
    if let Some(watch_thread) = watch_thread {
        watch_thread.join().expect("upgrade watch does not panic");
    }
    result?;
    Ok(
        if refresh_pending.load(Ordering::Acquire) && !manager_stopped.load(Ordering::Acquire) {
            RuntimeActivationExit::UpgradeRefresh
        } else {
            RuntimeActivationExit::ManagerStop
        },
    )
}
