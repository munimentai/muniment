//! Unix termination signal wait.

use std::io;

/// A signal the wait received.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceivedSignal {
    /// `SIGTERM` or `SIGINT`: stop.
    Terminate,
    /// `SIGUSR1`: a launcher asks the runtime to drain for an update.
    UpdateDrain,
}

/// This wait handles `SIGTERM` and `SIGINT` synchronously.
pub struct TerminationSignalWait {
    signals: libc::sigset_t,
}

impl TerminationSignalWait {
    /// Creates a wait and blocks both signals on this thread.
    pub fn new() -> io::Result<Self> {
        Self::blocking(&[libc::SIGTERM, libc::SIGINT])
    }

    /// Creates a wait that also takes `SIGUSR1`, the update drain request a
    /// launcher sends, and blocks all three signals on this thread.
    pub fn with_update_drain() -> io::Result<Self> {
        Self::blocking(&[libc::SIGTERM, libc::SIGINT, libc::SIGUSR1])
    }

    fn blocking(numbers: &[libc::c_int]) -> io::Result<Self> {
        let mut signals = unsafe { std::mem::zeroed() };
        if unsafe { libc::sigemptyset(&mut signals) } != 0 {
            return Err(io::Error::last_os_error());
        }
        for number in numbers {
            if unsafe { libc::sigaddset(&mut signals, *number) } != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        let mask_result =
            unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &signals, std::ptr::null_mut()) };
        if mask_result != 0 {
            return Err(io::Error::from_raw_os_error(mask_result));
        }

        Ok(Self { signals })
    }

    /// Blocks until this thread receives either signal.
    pub fn wait(self) -> io::Result<()> {
        loop {
            if self.next()? == ReceivedSignal::Terminate {
                return Ok(());
            }
        }
    }

    /// Blocks until this thread receives one of the wait's signals.
    pub fn next(&self) -> io::Result<ReceivedSignal> {
        let mut signal = 0;
        let wait_result = unsafe { libc::sigwait(&self.signals, &mut signal) };
        if wait_result != 0 {
            return Err(io::Error::from_raw_os_error(wait_result));
        }
        Ok(if signal == libc::SIGUSR1 {
            ReceivedSignal::UpdateDrain
        } else {
            ReceivedSignal::Terminate
        })
    }
}
