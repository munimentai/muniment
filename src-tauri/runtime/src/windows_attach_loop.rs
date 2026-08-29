//! Runtime-owned Windows attach accept loop.

use std::time::Duration;

#[cfg(target_os = "windows")]
use muniment_core::attach::{
    serve_next_windows_attach_until, WindowsAttachAcceptError, WindowsAttachBindError,
    WindowsAttachListener, WindowsAttachServeOutcome, WindowsAttachStopEvent,
};
#[cfg(target_os = "windows")]
use std::path::Path;
#[cfg(target_os = "windows")]
use std::sync::Arc;

const FAILED_ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(50);
/// The consecutive failed accept limit for one activation.
pub const MAX_CONSECUTIVE_FAILED_ACCEPTS: usize = 5;

/// The result of one attach accept and serve attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowsAttachAcceptOutcome {
    Served,
    Stopped,
    Failed,
}

/// A signal that stops a blocked Windows attach accept.
pub trait WindowsAttachStopSignal: Send + 'static {
    fn signal(&self);
}

/// The boundary for one Windows attach accept and serve attempt.
pub trait WindowsAttachAcceptBoundary {
    type StopSignal: WindowsAttachStopSignal;

    fn stop_signal(&self) -> Self::StopSignal;
    fn serve_next(&mut self) -> WindowsAttachAcceptOutcome;
}

/// A bound Windows attach acceptor.
#[cfg(target_os = "windows")]
pub struct WindowsAttachAcceptor {
    listener: WindowsAttachListener,
    stop: Arc<WindowsAttachStopEvent>,
}

#[cfg(target_os = "windows")]
impl WindowsAttachAcceptor {
    /// Binds the current user's attach pipe within the supplied wait.
    pub fn bind(
        state_directory: impl AsRef<Path>,
        bounded_wait: Duration,
    ) -> Result<Self, WindowsAttachBindError> {
        let listener = WindowsAttachListener::bind(state_directory, bounded_wait)?;
        let stop = Arc::new(
            WindowsAttachStopEvent::new()
                .map_err(|error| WindowsAttachBindError::Pipe(std::io::Error::other(error)))?,
        );
        Ok(Self { listener, stop })
    }
}

#[cfg(target_os = "windows")]
impl WindowsAttachStopSignal for Arc<WindowsAttachStopEvent> {
    fn signal(&self) {
        let _ = WindowsAttachStopEvent::signal(self);
    }
}

#[cfg(target_os = "windows")]
impl WindowsAttachAcceptBoundary for WindowsAttachAcceptor {
    type StopSignal = Arc<WindowsAttachStopEvent>;

    fn stop_signal(&self) -> Self::StopSignal {
        Arc::clone(&self.stop)
    }

    fn serve_next(&mut self) -> WindowsAttachAcceptOutcome {
        windows_attach_accept_outcome(serve_next_windows_attach_until(
            &mut self.listener,
            env!("CARGO_PKG_VERSION"),
            &self.stop,
        ))
    }
}

#[cfg(target_os = "windows")]
fn windows_attach_accept_outcome(
    result: Result<WindowsAttachServeOutcome, WindowsAttachAcceptError>,
) -> WindowsAttachAcceptOutcome {
    match result {
        Ok(WindowsAttachServeOutcome::Served) => WindowsAttachAcceptOutcome::Served,
        Ok(WindowsAttachServeOutcome::Stopped) => WindowsAttachAcceptOutcome::Stopped,
        Err(_) => WindowsAttachAcceptOutcome::Failed,
    }
}

/// The reason the Windows attach accept loop ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowsAttachAcceptLoopExit {
    Stopped,
    Failed,
}

/// Serves Windows attach sessions until stopped or accepts keep failing.
pub fn run_windows_attach_accept_loop(
    acceptor: &mut impl WindowsAttachAcceptBoundary,
) -> WindowsAttachAcceptLoopExit {
    let mut consecutive_failed_accepts = 0;
    loop {
        match acceptor.serve_next() {
            WindowsAttachAcceptOutcome::Stopped => return WindowsAttachAcceptLoopExit::Stopped,
            WindowsAttachAcceptOutcome::Failed => {
                consecutive_failed_accepts += 1;
                if consecutive_failed_accepts == MAX_CONSECUTIVE_FAILED_ACCEPTS {
                    return WindowsAttachAcceptLoopExit::Failed;
                }
                std::thread::sleep(FAILED_ACCEPT_RETRY_DELAY);
            }
            WindowsAttachAcceptOutcome::Served => {
                consecutive_failed_accepts = 0;
            }
        }
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn maps_each_accept_result() {
        assert_eq!(
            windows_attach_accept_outcome(Ok(WindowsAttachServeOutcome::Served)),
            WindowsAttachAcceptOutcome::Served
        );
        assert_eq!(
            windows_attach_accept_outcome(Ok(WindowsAttachServeOutcome::Stopped)),
            WindowsAttachAcceptOutcome::Stopped
        );
        for error in [
            WindowsAttachAcceptError::DeadlineExpired,
            WindowsAttachAcceptError::CreateEvent(1),
            WindowsAttachAcceptError::CreateInstance(1),
            WindowsAttachAcceptError::VerifyInstanceSecurity,
            WindowsAttachAcceptError::Connect(1),
            WindowsAttachAcceptError::Wait(1),
            WindowsAttachAcceptError::Cancel(1),
        ] {
            assert_eq!(
                windows_attach_accept_outcome(Err(error)),
                WindowsAttachAcceptOutcome::Failed
            );
        }
    }
}
