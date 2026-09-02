//! Runtime-owned macOS attach acceptor.

#[cfg(any(test, target_os = "macos"))]
use std::io;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::sync::Arc;
#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
use muniment_core::attach::{
    serve_macos_attach_session, MacosAttachListener, MacosAttachStopEvent, MacosAttachWaitOutcome,
};

#[cfg(target_os = "macos")]
use crate::{
    installed_desktop_executable, RuntimeAttachState, WindowsAttachAcceptBoundary,
    WindowsAttachAcceptOutcome, WindowsAttachStopSignal,
};

#[cfg(target_os = "macos")]
const MACOS_ATTACH_SESSION_TIMEOUT: Duration = Duration::from_secs(5);
const MACOS_ATTACH_DIRECTORY: &str = "muniment";
const MACOS_ATTACH_SOCKET: &str = "attach-v1.sock";

/// The reason a macOS attach acceptor could not bind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MacosAttachBindFailure {
    Contended,
    Unavailable,
}

/// Returns the attach socket path for a profile.
pub fn macos_attach_socket_path(profile_directory: &Path) -> PathBuf {
    profile_directory
        .join(MACOS_ATTACH_DIRECTORY)
        .join(MACOS_ATTACH_SOCKET)
}

#[cfg(any(test, target_os = "macos"))]
fn classify_bind_failure(error: &io::Error) -> MacosAttachBindFailure {
    if error.kind() == io::ErrorKind::AddrInUse {
        MacosAttachBindFailure::Contended
    } else {
        MacosAttachBindFailure::Unavailable
    }
}

/// A bound macOS attach acceptor.
#[cfg(target_os = "macos")]
pub struct MacosAttachAcceptor {
    listener: MacosAttachListener,
    stop: Arc<MacosAttachStopEvent>,
    state: Arc<RuntimeAttachState>,
    expected_desktop_executable: PathBuf,
}

#[cfg(target_os = "macos")]
impl MacosAttachAcceptor {
    /// Opens the activation state and binds its profile attach socket.
    pub fn bind(
        profile_directory: impl AsRef<Path>,
        config_directory: impl AsRef<Path>,
    ) -> Result<Self, MacosAttachBindFailure> {
        let profile_directory = profile_directory.as_ref();
        let attach_directory = profile_directory.join(MACOS_ATTACH_DIRECTORY);
        std::fs::create_dir_all(&attach_directory)
            .map_err(|_| MacosAttachBindFailure::Unavailable)?;
        let listener = MacosAttachListener::bind(macos_attach_socket_path(profile_directory))
            .map_err(|error| classify_bind_failure(&error))?;
        let stop =
            Arc::new(MacosAttachStopEvent::new().map_err(|_| MacosAttachBindFailure::Unavailable)?);
        let state = Arc::new(
            RuntimeAttachState::open(profile_directory, config_directory)
                .map_err(|_| MacosAttachBindFailure::Unavailable)?,
        );
        let expected_desktop_executable =
            installed_desktop_executable().ok_or(MacosAttachBindFailure::Unavailable)?;
        Ok(Self {
            listener,
            stop,
            state,
            expected_desktop_executable,
        })
    }
}

#[cfg(target_os = "macos")]
impl WindowsAttachStopSignal for Arc<MacosAttachStopEvent> {
    fn signal(&self) {
        let _ = MacosAttachStopEvent::signal(self);
    }
}

#[cfg(target_os = "macos")]
impl WindowsAttachAcceptBoundary for MacosAttachAcceptor {
    type StopSignal = Arc<MacosAttachStopEvent>;

    fn stop_signal(&self) -> Self::StopSignal {
        Arc::clone(&self.stop)
    }

    fn retention_state(&self) -> Option<Arc<RuntimeAttachState>> {
        Some(Arc::clone(&self.state))
    }

    fn serve_next(&mut self) -> WindowsAttachAcceptOutcome {
        match self.listener.accept_until(&self.stop) {
            Ok(MacosAttachWaitOutcome::Connected(stream)) => {
                let state = Arc::clone(&self.state);
                let expected_desktop_executable = self.expected_desktop_executable.clone();
                std::thread::spawn(move || {
                    let Ok(mut service) = state.attach_service() else {
                        return;
                    };
                    let _ = serve_macos_attach_session(
                        stream,
                        &expected_desktop_executable,
                        env!("CARGO_PKG_VERSION"),
                        Instant::now() + MACOS_ATTACH_SESSION_TIMEOUT,
                        &mut service,
                    );
                });
                WindowsAttachAcceptOutcome::Served
            }
            Ok(MacosAttachWaitOutcome::Stopped) => WindowsAttachAcceptOutcome::Stopped,
            Err(_) => WindowsAttachAcceptOutcome::Failed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_the_profile_attach_socket_path() {
        assert_eq!(
            macos_attach_socket_path(Path::new("/profiles/current")),
            Path::new("/profiles/current/muniment/attach-v1.sock")
        );
    }

    #[test]
    fn distinguishes_a_contended_bind() {
        assert_eq!(
            classify_bind_failure(&io::Error::from(io::ErrorKind::AddrInUse)),
            MacosAttachBindFailure::Contended
        );
        assert_eq!(
            classify_bind_failure(&io::Error::from(io::ErrorKind::PermissionDenied)),
            MacosAttachBindFailure::Unavailable
        );
    }
}
