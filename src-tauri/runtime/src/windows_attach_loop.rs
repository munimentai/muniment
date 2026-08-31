//! Runtime-owned Windows attach accept loop.

use std::time::Duration;

#[cfg(test)]
use muniment_core::attach::thread_service::ThreadListService;
#[cfg(target_os = "windows")]
use muniment_core::attach::{
    serve_next_windows_attach_until, WindowsAttachAcceptError, WindowsAttachBindError,
    WindowsAttachListener, WindowsAttachServeOutcome, WindowsAttachStopEvent,
};
#[cfg(any(test, target_os = "windows"))]
use muniment_core::chat_profile::{ChatProfile, ChatProfileError};
#[cfg(any(test, target_os = "windows"))]
use muniment_core::journal::RunJournal;
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
    service_factory: Arc<WindowsDesktopSessionServiceFactory>,
}

#[cfg(target_os = "windows")]
type WindowsDesktopSessionServiceFactory =
    Box<dyn Fn() -> Result<RunJournal, ChatProfileError> + Send + Sync>;

#[cfg(any(test, target_os = "windows"))]
fn windows_desktop_session_service(profile: &ChatProfile) -> Result<RunJournal, ChatProfileError> {
    let (journal, _cas) = profile.open_storage()?;
    Ok(journal)
}

#[cfg(target_os = "windows")]
impl WindowsAttachAcceptor {
    /// Binds the current user's attach pipe within the supplied wait.
    pub fn bind(
        state_directory: impl AsRef<Path>,
        bounded_wait: Duration,
    ) -> Result<Self, WindowsAttachBindError> {
        let listener = WindowsAttachListener::bind(state_directory.as_ref(), bounded_wait)?;
        let stop = Arc::new(
            WindowsAttachStopEvent::new()
                .map_err(|error| WindowsAttachBindError::Pipe(std::io::Error::other(error)))?,
        );
        let profile = ChatProfile::new(state_directory.as_ref());
        Ok(Self {
            listener,
            stop,
            service_factory: Arc::new(Box::new(move || windows_desktop_session_service(&profile))),
        })
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
            Arc::clone(&self.service_factory),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_a_fresh_profile_as_an_empty_journal() {
        use muniment_core::attach::thread_service::ThreadListRequest;
        use muniment_core::attach::ErrorCode;

        let directory = std::env::temp_dir().join(format!(
            "muniment-windows-desktop-service-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let profile = ChatProfile::new(&directory);

        let mut service = windows_desktop_session_service(&profile).unwrap();
        let page = service
            .list_threads(
                "workspace",
                ThreadListRequest {
                    limit: 20,
                    cursor: None,
                },
            )
            .unwrap();

        assert!(page.threads.is_empty());
        assert_eq!(
            service.ensure_home().unwrap_err().code(),
            ErrorCode::UnsupportedOperation
        );
        assert!(profile.journal_path().is_file());
        assert!(profile.cas_directory().join("objects").is_dir());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn returns_a_profile_open_failure() {
        let directory = std::env::temp_dir().join(format!(
            "muniment-windows-desktop-service-error-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let profile = ChatProfile::new(&directory);
        std::fs::create_dir_all(profile.journal_path()).unwrap();

        assert!(windows_desktop_session_service(&profile).is_err());

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(target_os = "windows")]
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
