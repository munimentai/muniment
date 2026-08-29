//! Runtime-owned Windows attach accept loop.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
use muniment_core::attach::{
    serve_next_windows_attach, WindowsAttachAcceptError, WindowsAttachBindError,
    WindowsAttachListener,
};
#[cfg(target_os = "windows")]
use std::path::Path;

const ACCEPT_TIMEOUT: Duration = Duration::from_millis(100);
const FAILED_ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(50);
/// The consecutive failed accept limit for one activation.
pub const MAX_CONSECUTIVE_FAILED_ACCEPTS: usize = 5;

/// The result of one bounded attach accept and serve attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowsAttachAcceptOutcome {
    Served,
    Idle,
    Failed,
}

/// The boundary for one bounded Windows attach accept and serve attempt.
pub trait WindowsAttachAcceptBoundary {
    fn serve_next(&mut self, accept_deadline: Instant) -> WindowsAttachAcceptOutcome;
}

/// A bound Windows attach acceptor.
#[cfg(target_os = "windows")]
pub struct WindowsAttachAcceptor {
    listener: WindowsAttachListener,
}

#[cfg(target_os = "windows")]
impl WindowsAttachAcceptor {
    /// Binds the current user's attach pipe within the supplied wait.
    pub fn bind(
        state_directory: impl AsRef<Path>,
        bounded_wait: Duration,
    ) -> Result<Self, WindowsAttachBindError> {
        Ok(Self {
            listener: WindowsAttachListener::bind(state_directory, bounded_wait)?,
        })
    }
}

#[cfg(target_os = "windows")]
impl WindowsAttachAcceptBoundary for WindowsAttachAcceptor {
    fn serve_next(&mut self, accept_deadline: Instant) -> WindowsAttachAcceptOutcome {
        windows_attach_accept_outcome(serve_next_windows_attach(
            &mut self.listener,
            env!("CARGO_PKG_VERSION"),
            accept_deadline,
        ))
    }
}

#[cfg(target_os = "windows")]
fn windows_attach_accept_outcome(
    result: Result<(), WindowsAttachAcceptError>,
) -> WindowsAttachAcceptOutcome {
    match result {
        Ok(()) => WindowsAttachAcceptOutcome::Served,
        Err(WindowsAttachAcceptError::DeadlineExpired) => WindowsAttachAcceptOutcome::Idle,
        Err(_) => WindowsAttachAcceptOutcome::Failed,
    }
}

/// The reason the Windows attach accept loop ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowsAttachAcceptLoopExit {
    Stopped,
    Failed,
}

/// Serves Windows attach sessions until the stop channel fires or accepts keep failing.
pub fn run_windows_attach_accept_loop(
    acceptor: &mut impl WindowsAttachAcceptBoundary,
    stop: Receiver<()>,
) -> WindowsAttachAcceptLoopExit {
    let mut consecutive_failed_accepts = 0;
    loop {
        match stop.try_recv() {
            Ok(()) | Err(TryRecvError::Disconnected) => {
                return WindowsAttachAcceptLoopExit::Stopped
            }
            Err(TryRecvError::Empty) => {}
        }

        match acceptor.serve_next(Instant::now() + ACCEPT_TIMEOUT) {
            WindowsAttachAcceptOutcome::Failed => {
                consecutive_failed_accepts += 1;
                if consecutive_failed_accepts == MAX_CONSECUTIVE_FAILED_ACCEPTS {
                    return WindowsAttachAcceptLoopExit::Failed;
                }
                std::thread::sleep(FAILED_ACCEPT_RETRY_DELAY);
            }
            WindowsAttachAcceptOutcome::Served | WindowsAttachAcceptOutcome::Idle => {
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
            windows_attach_accept_outcome(Ok(())),
            WindowsAttachAcceptOutcome::Served
        );
        assert_eq!(
            windows_attach_accept_outcome(Err(WindowsAttachAcceptError::DeadlineExpired)),
            WindowsAttachAcceptOutcome::Idle
        );
        for error in [
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
