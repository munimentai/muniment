use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use muniment_core::model_install::{InstallLock, InstallLockState};
use muniment_core::model_install_native::NativeInstallLock;

const LOCK_NAME: &str = "install.lock";
const RETRY_INTERVAL: Duration = Duration::from_millis(50);

/// An owned per-user install lock guard.
#[derive(Debug)]
pub struct InstallLockGuard {
    _file: std::fs::File,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallLockAcquireError {
    Unavailable,
    TimedOut,
}

impl std::fmt::Display for InstallLockAcquireError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Unavailable => "the install lock is unavailable",
            Self::TimedOut => "the install lock wait timed out",
        })
    }
}

impl std::error::Error for InstallLockAcquireError {}

/// Acquires the per-user install lock within `bounded_wait`.
pub fn acquire(
    state_directory: impl AsRef<Path>,
    bounded_wait: Duration,
) -> Result<InstallLockGuard, InstallLockAcquireError> {
    fs::create_dir_all(state_directory.as_ref())
        .map_err(|_| InstallLockAcquireError::Unavailable)?;

    let started = Instant::now();
    let mut first_attempt = true;
    loop {
        let elapsed = started.elapsed();
        if !first_attempt && elapsed >= bounded_wait {
            return Err(InstallLockAcquireError::TimedOut);
        }
        first_attempt = false;
        let remaining = bounded_wait.saturating_sub(elapsed);
        let mut lock = NativeInstallLock::with_retry_interval(
            state_directory.as_ref().join(LOCK_NAME),
            RETRY_INTERVAL.min(remaining),
        );
        match lock
            .try_lock_exclusive()
            .map_err(|_| InstallLockAcquireError::Unavailable)?
        {
            InstallLockState::Acquired(file) => return Ok(InstallLockGuard { _file: file }),
            InstallLockState::Contended if remaining.is_zero() => {
                return Err(InstallLockAcquireError::TimedOut)
            }
            InstallLockState::Contended => {
                let _ = lock.wait_for_retry(&|| false);
            }
        }
    }
}
