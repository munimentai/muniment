//! Runtime attach service activation.

use crate::retention_schedule::{start_retention_schedule, RetentionScheduleCommand};
use crate::upgrade_watch::UpgradeWatch;
use crate::{
    migration::run_migration_takeover_with_activation, run_bound_attach_listener,
    AttachListenerError, MigrationTakeoverError, RuntimeAttachState,
};
use muniment_core::attach::evaluate_quiesce;
use muniment_core::attach::linux::{AttachFilesystem, AttachTransport, InstanceLockError};
use muniment_core::attach::RuntimeActivityRegistry;
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

/// How often a pending refresh reads runtime activity. No event announces
/// the end of the last blocking activity, so only this wait polls.
const QUIESCE_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// The events that wake the activation coordinator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActivationWake {
    ManagerStop,
    ListenerFinished,
    RefreshDetected,
}

/// Forwards the manager's stop into the coordinator's wake channel. A closed
/// stop channel is a stop.
fn forward_manager_stop(stop: Receiver<()>, wake: Sender<ActivationWake>) {
    match stop.try_recv() {
        Ok(()) | Err(mpsc::TryRecvError::Disconnected) => {
            let _ = wake.send(ActivationWake::ManagerStop);
        }
        Err(mpsc::TryRecvError::Empty) => {
            std::thread::spawn(move || {
                let _ = stop.recv();
                let _ = wake.send(ActivationWake::ManagerStop);
            });
        }
    }
}

fn coordinate_activation(
    wake: Receiver<ActivationWake>,
    stopped: Arc<AtomicBool>,
    refresh_pending: Arc<AtomicBool>,
    activity: RuntimeActivityRegistry,
    commands: Sender<RetentionScheduleCommand>,
    listener_stop: Sender<()>,
) -> RuntimeActivationExit {
    let finish = |exit| {
        stopped.store(true, Ordering::Release);
        let _ = commands.send(RetentionScheduleCommand::Stop);
        let _ = listener_stop.send(());
        exit
    };
    let stops = |event: Result<ActivationWake, mpsc::RecvTimeoutError>| {
        matches!(
            event,
            Ok(ActivationWake::ManagerStop | ActivationWake::ListenerFinished)
                | Err(mpsc::RecvTimeoutError::Disconnected)
        )
    };
    loop {
        let event = if refresh_pending.load(Ordering::Acquire) {
            wake.recv_timeout(QUIESCE_POLL_INTERVAL)
        } else {
            wake.recv()
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected)
        };
        if stops(event) {
            return finish(RuntimeActivationExit::ManagerStop);
        }
        if refresh_pending.load(Ordering::Acquire) && evaluate_quiesce(activity.snapshot()).is_ok()
        {
            // A manager stop sent before the quiesce can still be on its way
            // through the forwarder. One more short wait lets it win.
            if stops(wake.recv_timeout(QUIESCE_POLL_INTERVAL)) {
                return finish(RuntimeActivationExit::ManagerStop);
            }
            return finish(RuntimeActivationExit::UpgradeRefresh);
        }
        if stopped.load(Ordering::Acquire) {
            return RuntimeActivationExit::ManagerStop;
        }
    }
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
    let (wake_tx, wake_rx) = mpsc::channel();
    forward_manager_stop(stop, wake_tx.clone());
    let watch_stopped = Arc::new(AtomicBool::new(false));
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
            let refresh_wake = wake_tx.clone();
            watch.start(
                state.drain_state(),
                Arc::clone(&watch_stopped),
                Arc::clone(&refresh_pending),
                move || {
                    let _ = refresh_wake.send(ActivationWake::RefreshDetected);
                },
            )
        })
        .transpose()
        .map_err(|_| RuntimeActivationError::UpgradeWatch)?;
    let coordinator_commands = command_tx.clone();
    let coordinator_stopped = Arc::clone(&watch_stopped);
    let coordinator_pending = Arc::clone(&refresh_pending);
    let coordinator_activity = state.runtime_activity();
    let coordinator = std::thread::spawn(move || {
        coordinate_activation(
            wake_rx,
            coordinator_stopped,
            coordinator_pending,
            coordinator_activity,
            coordinator_commands,
            listener_stop_tx,
        )
    });
    let retention_checked = retention_control
        .as_ref()
        .map(|control| control.checked.clone());
    if let Some(retention_control) = retention_control {
        let trigger_commands = command_tx.clone();
        std::thread::spawn(move || {
            while retention_control.trigger.recv().is_ok() {
                if trigger_commands
                    .send(RetentionScheduleCommand::Recheck)
                    .is_err()
                {
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
                Err(_) => {
                    let _ = wake_tx.send(ActivationWake::ListenerFinished);
                    watch_stopped.store(true, Ordering::Release);
                    if let Some(watch_thread) = watch_thread {
                        watch_thread.join().expect("upgrade watch does not panic");
                    }
                    coordinator
                        .join()
                        .expect("activation coordinator does not panic");
                    return Err(RuntimeActivationError::Bind);
                }
            };
            retention_thread = Some(start_retention_schedule(
                Arc::clone(&state),
                RETENTION_INTERVAL,
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
                    let thread = start_retention_schedule(
                        retention_state,
                        RETENTION_INTERVAL,
                        command_rx,
                        retention_checked,
                    );
                    let _ = started_tx.send(thread);
                },
                listener_stop_rx,
            );
            retention_thread = started_rx.try_recv().ok();
            takeover.map_err(RuntimeActivationError::Migration)
        }
        Err(_) => Err(RuntimeActivationError::InstanceLock),
    };
    let _ = wake_tx.send(ActivationWake::ListenerFinished);
    let _ = command_tx.send(RetentionScheduleCommand::Stop);
    if let Some(retention_thread) = retention_thread {
        retention_thread
            .join()
            .expect("retention schedule does not panic");
    }
    watch_stopped.store(true, Ordering::Release);
    if let Some(watch_thread) = watch_thread {
        watch_thread.join().expect("upgrade watch does not panic");
    }
    let exit = coordinator
        .join()
        .expect("activation coordinator does not panic");
    result?;
    Ok(exit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manager_stop_precedes_refresh_after_drain_starts() {
        let drain = muniment_core::attach::DrainState::new();
        let activity = RuntimeActivityRegistry::with_drain_state(&drain);
        let blocker = activity.mark_active_run();
        let stopped = Arc::new(AtomicBool::new(false));
        let pending = Arc::new(AtomicBool::new(true));
        let (stop_tx, stop_rx) = mpsc::channel();
        let (wake_tx, wake_rx) = mpsc::channel();
        let (command_tx, _command_rx) = mpsc::channel();
        let (listener_tx, _listener_rx) = mpsc::channel();

        drain.set();
        forward_manager_stop(stop_rx, wake_tx.clone());
        let coordinator = std::thread::spawn(move || {
            coordinate_activation(wake_rx, stopped, pending, activity, command_tx, listener_tx)
        });
        stop_tx.send(()).unwrap();
        drop(blocker);

        assert_eq!(
            coordinator.join().unwrap(),
            RuntimeActivationExit::ManagerStop
        );
        drop(wake_tx);
    }

    #[test]
    fn an_idle_coordinator_blocks_until_an_event_arrives() {
        let activity = RuntimeActivityRegistry::new();
        let stopped = Arc::new(AtomicBool::new(false));
        let pending = Arc::new(AtomicBool::new(false));
        let (wake_tx, wake_rx) = mpsc::channel();
        let (command_tx, command_rx) = mpsc::channel();
        let (listener_tx, listener_rx) = mpsc::channel();
        let coordinator_pending = Arc::clone(&pending);
        let coordinator = std::thread::spawn(move || {
            coordinate_activation(
                wake_rx,
                stopped,
                coordinator_pending,
                activity,
                command_tx,
                listener_tx,
            )
        });

        // An idle coordinator sends nothing until an event wakes it.
        assert!(listener_rx.recv_timeout(Duration::from_millis(50)).is_err());
        pending.store(true, Ordering::Release);
        wake_tx.send(ActivationWake::RefreshDetected).unwrap();

        assert_eq!(
            coordinator.join().unwrap(),
            RuntimeActivationExit::UpgradeRefresh
        );
        assert!(listener_rx.try_recv().is_ok());
        assert!(matches!(
            command_rx.try_recv(),
            Ok(RetentionScheduleCommand::Stop)
        ));
    }

    #[test]
    fn a_finished_listener_ends_an_idle_coordinator() {
        let (wake_tx, wake_rx) = mpsc::channel();
        let (command_tx, _command_rx) = mpsc::channel();
        let (listener_tx, _listener_rx) = mpsc::channel();
        let coordinator = std::thread::spawn(move || {
            coordinate_activation(
                wake_rx,
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicBool::new(false)),
                RuntimeActivityRegistry::new(),
                command_tx,
                listener_tx,
            )
        });
        wake_tx.send(ActivationWake::ListenerFinished).unwrap();
        assert_eq!(
            coordinator.join().unwrap(),
            RuntimeActivationExit::ManagerStop
        );
    }
}
