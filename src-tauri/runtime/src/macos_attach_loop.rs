//! Runtime-owned macOS attach acceptor.

use std::io;
use std::path::{Path, PathBuf};
#[cfg(any(unix, target_os = "windows"))]
use std::sync::Arc;
#[cfg(target_os = "macos")]
use std::time::Duration;

#[cfg(target_os = "macos")]
use muniment_core::attach::{
    approval_waiter_with_claims, serve_macos_attach_session_with_state, MacosAttachListener,
    MacosAttachStopEvent, MacosAttachWaitOutcome,
};
#[cfg(any(unix, target_os = "windows"))]
use muniment_core::attach::{DesktopAttachService, ProtocolError};

#[cfg(target_os = "macos")]
use crate::attach_boundaries::request_approval;
#[cfg(target_os = "macos")]
use crate::{installed_desktop_executable, WindowsAttachBindFailure, WindowsAttachFactory};
#[cfg(any(unix, target_os = "windows"))]
use crate::{
    RuntimeAttachBoundaries, RuntimeAttachState, WindowsAttachAcceptBoundary,
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
    DesktopExecutableCheckFailed,
    StateOpenFailed,
}

#[cfg(any(target_os = "macos", test))]
impl MacosAttachBindFailure {
    fn diagnostic_event(self) -> crate::MacosDiagnosticEvent {
        match self {
            Self::Contended => crate::MacosDiagnosticEvent::InstanceLockWait,
            Self::Unavailable => crate::MacosDiagnosticEvent::SocketBindFailed,
            Self::DesktopExecutableCheckFailed => {
                crate::MacosDiagnosticEvent::DesktopExecutableCheckFailed
            }
            Self::StateOpenFailed => crate::MacosDiagnosticEvent::StateOpenFailed,
        }
    }
}

/// Returns the attach socket path for a profile.
pub fn macos_attach_socket_path(profile_directory: &Path) -> PathBuf {
    profile_directory
        .join(MACOS_ATTACH_DIRECTORY)
        .join(MACOS_ATTACH_SOCKET)
}

fn classify_bind_failure(error: &io::Error) -> MacosAttachBindFailure {
    if error.kind() == io::ErrorKind::AddrInUse {
        MacosAttachBindFailure::Contended
    } else {
        MacosAttachBindFailure::Unavailable
    }
}

/// Injected boundary for one macOS accept and serve attempt.
#[doc(hidden)]
#[cfg(any(unix, target_os = "windows"))]
pub trait MacosAttachServeBoundary {
    type StopSignal: WindowsAttachStopSignal;

    fn stop_signal(&self) -> Self::StopSignal;

    fn serve_next(
        &mut self,
        service_factory: Arc<
            dyn Fn() -> Result<DesktopAttachService<RuntimeAttachBoundaries>, ProtocolError>
                + Send
                + Sync,
        >,
    ) -> WindowsAttachAcceptOutcome;
}

/// A macOS acceptor with an injected transport boundary.
#[doc(hidden)]
#[cfg(any(unix, target_os = "windows"))]
pub struct MacosAttachAcceptorWithBoundary<B> {
    boundary: B,
    state: Arc<RuntimeAttachState>,
}

#[cfg(any(unix, target_os = "windows"))]
impl<B> MacosAttachAcceptorWithBoundary<B> {
    /// Binds a transport boundary and opens one activation state.
    #[doc(hidden)]
    pub fn bind_with(
        profile_directory: impl AsRef<Path>,
        config_directory: impl AsRef<Path>,
        bind_boundary: impl FnOnce(&Path) -> io::Result<B>,
    ) -> Result<Self, MacosAttachBindFailure> {
        let profile_directory = profile_directory.as_ref();
        let boundary = bind_boundary(&macos_attach_socket_path(profile_directory))
            .map_err(|error| classify_bind_failure(&error))?;
        let state = Arc::new(
            RuntimeAttachState::open(profile_directory, config_directory)
                .map_err(|_| MacosAttachBindFailure::StateOpenFailed)?,
        );
        Ok(Self { boundary, state })
    }
}

#[cfg(any(unix, target_os = "windows"))]
impl<B: MacosAttachServeBoundary> WindowsAttachAcceptBoundary
    for MacosAttachAcceptorWithBoundary<B>
{
    type StopSignal = B::StopSignal;

    fn stop_signal(&self) -> Self::StopSignal {
        self.boundary.stop_signal()
    }

    fn retention_state(&self) -> Option<Arc<RuntimeAttachState>> {
        Some(Arc::clone(&self.state))
    }

    fn serve_next(&mut self) -> WindowsAttachAcceptOutcome {
        let state = Arc::clone(&self.state);
        self.boundary
            .serve_next(Arc::new(move || state.attach_service()))
    }
}

/// A bound macOS attach acceptor.
#[cfg(target_os = "macos")]
pub type MacosAttachAcceptor = MacosAttachAcceptorWithBoundary<SystemMacosAttachBoundary>;

/// Production factory for the macOS attach acceptor.
#[cfg(target_os = "macos")]
pub struct SystemMacosAttachFactory {
    profile_directory: PathBuf,
    config_directory: PathBuf,
}

#[cfg(target_os = "macos")]
impl SystemMacosAttachFactory {
    /// Creates a factory for one profile and config directory.
    pub fn new(profile_directory: impl AsRef<Path>, config_directory: impl AsRef<Path>) -> Self {
        Self {
            profile_directory: profile_directory.as_ref().to_owned(),
            config_directory: config_directory.as_ref().to_owned(),
        }
    }
}

#[cfg(target_os = "macos")]
impl WindowsAttachFactory for SystemMacosAttachFactory {
    type Acceptor = MacosAttachAcceptor;

    fn bind(&self) -> Result<Self::Acceptor, WindowsAttachBindFailure> {
        MacosAttachAcceptor::bind(&self.profile_directory, &self.config_directory).map_err(
            |error| {
                let event = error.diagnostic_event();
                if let Ok(directory) = crate::effective_user_macos_log_directory() {
                    let _ = crate::write_macos_diagnostic(directory, event);
                } else {
                    crate::emit_macos_unified_log(event);
                }
                match error {
                    MacosAttachBindFailure::Contended => WindowsAttachBindFailure::Contended,
                    MacosAttachBindFailure::Unavailable
                    | MacosAttachBindFailure::DesktopExecutableCheckFailed
                    | MacosAttachBindFailure::StateOpenFailed => {
                        WindowsAttachBindFailure::Unavailable
                    }
                }
            },
        )
    }
}

/// The native macOS attach transport boundary.
#[doc(hidden)]
#[cfg(target_os = "macos")]
pub struct SystemMacosAttachBoundary {
    listener: MacosAttachListener,
    stop: Arc<MacosAttachStopEvent>,
    expected_desktop_executable: PathBuf,
}

#[cfg(target_os = "macos")]
impl MacosAttachAcceptorWithBoundary<SystemMacosAttachBoundary> {
    /// Opens the activation state and binds its profile attach socket.
    pub fn bind(
        profile_directory: impl AsRef<Path>,
        config_directory: impl AsRef<Path>,
    ) -> Result<Self, MacosAttachBindFailure> {
        let expected_desktop_executable = installed_desktop_executable()
            .ok_or(MacosAttachBindFailure::DesktopExecutableCheckFailed)?;
        Self::bind_with(profile_directory, config_directory, move |path| {
            let listener = MacosAttachListener::bind(path)?;
            let stop = Arc::new(MacosAttachStopEvent::new()?);
            Ok(SystemMacosAttachBoundary {
                listener,
                stop,
                expected_desktop_executable,
            })
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
impl MacosAttachServeBoundary for SystemMacosAttachBoundary {
    type StopSignal = Arc<MacosAttachStopEvent>;

    fn stop_signal(&self) -> Self::StopSignal {
        Arc::clone(&self.stop)
    }

    fn serve_next(
        &mut self,
        service_factory: Arc<
            dyn Fn() -> Result<DesktopAttachService<RuntimeAttachBoundaries>, ProtocolError>
                + Send
                + Sync,
        >,
    ) -> WindowsAttachAcceptOutcome {
        match self.listener.accept_until(&self.stop) {
            Ok(MacosAttachWaitOutcome::Connected(stream)) => {
                let expected_desktop_executable = self.expected_desktop_executable.clone();
                std::thread::spawn(move || {
                    let Ok(mut service) = service_factory() else {
                        return;
                    };
                    let approval = service.boundaries.signed_workspace_approval();
                    let coordinator = service.boundaries.approval_coordinator();
                    let live_connections = service.boundaries.live_connections();
                    let approval_waiter = service.boundaries.approval_coordinator();
                    let waiter_approval = approval.clone();
                    let waiter = approval_waiter_with_claims(
                        move |challenge: &muniment_core::attach::PairingChallenge,
                              kind: &str,
                              version: &str,
                              remaining: Duration| {
                            Some(request_approval(
                                &waiter_approval,
                                &approval_waiter,
                                challenge.as_str(),
                                kind,
                                version,
                                remaining,
                            ))
                        },
                    );
                    let _ = serve_macos_attach_session_with_state(
                        stream,
                        &expected_desktop_executable,
                        env!("CARGO_PKG_VERSION"),
                        MACOS_ATTACH_SESSION_TIMEOUT,
                        &mut service,
                        approval.approval(),
                        coordinator,
                        waiter,
                        &live_connections,
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
    fn names_each_activation_failure_step() {
        use crate::MacosDiagnosticEvent;

        for (failure, event) in [
            (
                MacosAttachBindFailure::DesktopExecutableCheckFailed,
                MacosDiagnosticEvent::DesktopExecutableCheckFailed,
            ),
            (
                MacosAttachBindFailure::Unavailable,
                MacosDiagnosticEvent::SocketBindFailed,
            ),
            (
                MacosAttachBindFailure::StateOpenFailed,
                MacosDiagnosticEvent::StateOpenFailed,
            ),
            (
                MacosAttachBindFailure::Contended,
                MacosDiagnosticEvent::InstanceLockWait,
            ),
        ] {
            assert_eq!(failure.diagnostic_event(), event);
        }
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
