//! Bind-and-serve activation for the Windows attach listener.

use std::sync::{mpsc, mpsc::Receiver, mpsc::Sender};
use std::time::Duration;

#[cfg(target_os = "windows")]
use std::path::Path;
#[cfg(any(unix, target_os = "windows"))]
use std::path::PathBuf;
#[cfg(unix)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(unix)]
use std::sync::mpsc::TryRecvError;
#[cfg(unix)]
use std::sync::Arc;

use crate::retention_schedule::{start_retention_schedule, RetentionScheduleCommand};
#[cfg(unix)]
use crate::upgrade_watch::UpgradeWatch;
use crate::{
    run_windows_attach_accept_loop, WindowsActivationExit, WindowsAttachAcceptBoundary,
    WindowsAttachAcceptLoopExit, WindowsAttachStopSignal, WindowsDiagnosticEvent,
};

const RETENTION_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
#[cfg(unix)]
const UPGRADE_REFRESH_EXIT_STATUS: i32 = 75;

/// Injected executable watch settings for activation contract tests.
#[cfg(unix)]
#[doc(hidden)]
pub struct MacosUpgradeWatchTestControl {
    pub path: PathBuf,
    pub poll_interval: Duration,
    pub ready: Option<Sender<()>>,
    pub refresh_detected: Option<Sender<()>>,
}

/// A runtime-owned attach bind failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowsAttachBindFailure {
    Contended,
    Unavailable,
}

/// Creates a bound acceptor for one Windows activation.
pub trait WindowsAttachFactory {
    type Acceptor: WindowsAttachAcceptBoundary;

    fn bind(&self) -> Result<Self::Acceptor, WindowsAttachBindFailure>;
}

/// Records a Windows activation diagnostic.
pub trait WindowsDiagnosticSink {
    fn record(&self, event: WindowsDiagnosticEvent);
}

/// Binds and serves Windows attach sessions until a stop arrives.
pub fn run_windows_attach_activation(
    factory: &impl WindowsAttachFactory,
    stop: Receiver<()>,
    diagnostics: &impl WindowsDiagnosticSink,
) -> WindowsActivationExit {
    run_windows_attach_activation_with_retention_schedule(
        factory,
        stop,
        diagnostics,
        RETENTION_INTERVAL,
        None,
    )
}

/// Runs an activation with an executable replacement watch.
#[cfg(unix)]
#[doc(hidden)]
pub fn run_windows_attach_activation_with_upgrade_watch(
    factory: &impl WindowsAttachFactory,
    stop: Receiver<()>,
    diagnostics: &impl WindowsDiagnosticSink,
    watch: MacosUpgradeWatchTestControl,
) -> WindowsActivationExit {
    let (activation_stop_tx, activation_stop_rx) = mpsc::channel();
    let manager_stop_pending = Arc::new(AtomicBool::new(false));
    let manager_stop_tx = activation_stop_tx.clone();
    match stop.try_recv() {
        Ok(()) | Err(TryRecvError::Disconnected) => {
            manager_stop_pending.store(true, Ordering::Release);
            let _ = manager_stop_tx.send(());
        }
        Err(TryRecvError::Empty) => {
            let manager_stop_pending_tx = Arc::clone(&manager_stop_pending);
            std::thread::spawn(move || {
                let _ = stop.recv();
                manager_stop_pending_tx.store(true, Ordering::Release);
                let _ = manager_stop_tx.send(());
            });
        }
    }

    let watch_stopped = Arc::new(AtomicBool::new(false));
    let refresh_pending = Arc::new(AtomicBool::new(false));
    let watch_thread = match (UpgradeWatch {
        path: watch.path,
        poll_interval: watch.poll_interval,
    })
    .start_stop(
        activation_stop_tx,
        Arc::clone(&watch_stopped),
        Arc::clone(&refresh_pending),
        watch.ready,
        watch.refresh_detected,
    ) {
        Ok(thread) => thread,
        Err(_) => {
            diagnostics.record(WindowsDiagnosticEvent::ActivationFailed);
            return WindowsActivationExit::Failed(1);
        }
    };

    let exit = run_windows_attach_activation(factory, activation_stop_rx, diagnostics);
    watch_stopped.store(true, Ordering::Release);
    watch_thread
        .join()
        .expect("executable replacement watch does not panic");
    if manager_stop_pending.load(Ordering::Acquire) {
        exit
    } else if refresh_pending.load(Ordering::Acquire) {
        match exit {
            WindowsActivationExit::Orderly(_) => {
                WindowsActivationExit::Orderly(UPGRADE_REFRESH_EXIT_STATUS)
            }
            failed => failed,
        }
    } else {
        exit
    }
}

/// Runs a Windows activation with retention schedule settings for contract tests.
#[doc(hidden)]
pub fn run_windows_attach_activation_with_retention_schedule(
    factory: &impl WindowsAttachFactory,
    stop: Receiver<()>,
    diagnostics: &impl WindowsDiagnosticSink,
    retention_interval: Duration,
    retention_checked: Option<Sender<()>>,
) -> WindowsActivationExit {
    let mut acceptor = match factory.bind() {
        Ok(acceptor) => acceptor,
        Err(WindowsAttachBindFailure::Contended) => {
            diagnostics.record(WindowsDiagnosticEvent::InstanceLockWait);
            return WindowsActivationExit::Orderly(0);
        }
        Err(WindowsAttachBindFailure::Unavailable) => {
            diagnostics.record(WindowsDiagnosticEvent::ActivationFailed);
            return WindowsActivationExit::Failed(1);
        }
    };

    let (retention_commands, retention_command_rx) = mpsc::channel();
    let retention_thread = acceptor.retention_state().map(|state| {
        start_retention_schedule(
            state,
            retention_interval,
            retention_command_rx,
            retention_checked,
        )
    });
    let stop_signal = acceptor.stop_signal();
    let stop_retention_commands = retention_commands.clone();
    std::thread::spawn(move || {
        let _ = stop.recv();
        let _ = stop_retention_commands.send(RetentionScheduleCommand::Stop);
        stop_signal.signal();
    });

    let loop_exit = run_windows_attach_accept_loop(&mut acceptor);
    let _ = retention_commands.send(RetentionScheduleCommand::Stop);
    if let Some(retention_thread) = retention_thread {
        retention_thread
            .join()
            .expect("retention schedule does not panic");
    }

    match loop_exit {
        WindowsAttachAcceptLoopExit::Stopped => WindowsActivationExit::Orderly(0),
        WindowsAttachAcceptLoopExit::Failed => {
            diagnostics.record(WindowsDiagnosticEvent::ActivationFailed);
            WindowsActivationExit::Failed(1)
        }
        WindowsAttachAcceptLoopExit::AcceptFailed(error) => {
            diagnostics.record(WindowsDiagnosticEvent::AttachAcceptFailed(error));
            WindowsActivationExit::Failed(1)
        }
    }
}

/// Production factory for the Windows attach acceptor.
#[cfg(target_os = "windows")]
pub struct SystemWindowsAttachFactory {
    state_directory: PathBuf,
    bounded_wait: Duration,
}

#[cfg(target_os = "windows")]
impl SystemWindowsAttachFactory {
    /// Creates a factory for one state directory and bounded lock wait.
    pub fn new(state_directory: impl AsRef<Path>, bounded_wait: Duration) -> Self {
        Self {
            state_directory: state_directory.as_ref().to_owned(),
            bounded_wait,
        }
    }
}

#[cfg(target_os = "windows")]
impl WindowsAttachFactory for SystemWindowsAttachFactory {
    type Acceptor = crate::WindowsAttachAcceptor;

    fn bind(&self) -> Result<Self::Acceptor, WindowsAttachBindFailure> {
        use muniment_core::attach::{WindowsAttachBindError, WindowsAttachInstanceLockError};

        crate::WindowsAttachAcceptor::bind(&self.state_directory, self.bounded_wait).map_err(
            |error| match error {
                WindowsAttachBindError::InstanceLock(WindowsAttachInstanceLockError::Contended) => {
                    WindowsAttachBindFailure::Contended
                }
                WindowsAttachBindError::InstanceLock(
                    WindowsAttachInstanceLockError::Unavailable,
                )
                | WindowsAttachBindError::Pipe(_) => WindowsAttachBindFailure::Unavailable,
            },
        )
    }
}
