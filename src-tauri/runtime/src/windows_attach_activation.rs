//! Bind-and-serve activation for the Windows attach listener.

use std::sync::mpsc::Receiver;

#[cfg(target_os = "windows")]
use std::path::{Path, PathBuf};
#[cfg(target_os = "windows")]
use std::time::Duration;

use crate::{
    run_windows_attach_accept_loop, WindowsActivationExit, WindowsAttachAcceptBoundary,
    WindowsDiagnosticEvent,
};

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

    run_windows_attach_accept_loop(&mut acceptor, stop);
    WindowsActivationExit::Orderly(0)
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
        use muniment_core::attach::{
            WindowsAttachBindError, WindowsAttachInstanceLockError,
        };

        crate::WindowsAttachAcceptor::bind(&self.state_directory, self.bounded_wait).map_err(
            |error| match error {
                WindowsAttachBindError::InstanceLock(
                    WindowsAttachInstanceLockError::Contended,
                ) => WindowsAttachBindFailure::Contended,
                WindowsAttachBindError::InstanceLock(
                    WindowsAttachInstanceLockError::Unavailable,
                )
                | WindowsAttachBindError::Pipe(_) => WindowsAttachBindFailure::Unavailable,
            },
        )
    }
}
