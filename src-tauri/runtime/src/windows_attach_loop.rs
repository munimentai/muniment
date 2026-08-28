//! Runtime-owned Windows attach accept loop.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

const ACCEPT_TIMEOUT: Duration = Duration::from_millis(100);
const FAILED_ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(50);

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

/// Serves Windows attach sessions until the stop channel fires or disconnects.
pub fn run_windows_attach_accept_loop(
    acceptor: &mut impl WindowsAttachAcceptBoundary,
    stop: Receiver<()>,
) {
    loop {
        match stop.try_recv() {
            Ok(()) | Err(TryRecvError::Disconnected) => return,
            Err(TryRecvError::Empty) => {}
        }

        let outcome = acceptor.serve_next(Instant::now() + ACCEPT_TIMEOUT);
        if outcome == WindowsAttachAcceptOutcome::Failed {
            std::thread::sleep(FAILED_ACCEPT_RETRY_DELAY);
        }
    }
}
