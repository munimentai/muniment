#[cfg(unix)]
use muniment_core::attach::{ClientError, DesktopClientHolder};
use muniment_core::retention_record::{
    read_retention_choice, write_retention_choice, RetentionChoice,
};
use std::path::Path;
use tauri::{AppHandle, Manager};

#[cfg(unix)]
use crate::attach_service::{AttachCompanionState, DesktopClientSession};
#[cfg(unix)]
use crate::auth;
use crate::chat::{ChatState, RetentionTrigger};

fn config_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_config_dir()
        .map_err(|_| "Muniment configuration storage is unavailable.".to_string())
}

#[tauri::command]
pub fn thread_retention_choice(app: AppHandle) -> Result<Option<RetentionChoice>, String> {
    Ok(read_retention_choice(&config_dir(&app)?))
}

#[cfg(unix)]
trait RetentionRecheckClient {
    fn recheck_retention(&self) -> Result<(), ClientError>;
}

#[cfg(unix)]
impl RetentionRecheckClient for DesktopClientHolder {
    fn recheck_retention(&self) -> Result<(), ClientError> {
        DesktopClientHolder::recheck_retention(self)
    }
}

#[cfg(unix)]
enum RetentionRecheckSession<C> {
    NoSupervisor,
    Connected(C),
    Disconnected,
}

#[cfg(unix)]
impl From<DesktopClientSession> for RetentionRecheckSession<DesktopClientHolder> {
    fn from(session: DesktopClientSession) -> Self {
        match session {
            DesktopClientSession::NoSupervisor => Self::NoSupervisor,
            DesktopClientSession::Connected(client) => Self::Connected(client),
            DesktopClientSession::Disconnected => Self::Disconnected,
        }
    }
}

/// Saves the choice, then asks the owner of the journal to check now.
///
/// The save is durable first. A failed recheck leaves the saved choice in
/// place for the next scheduled check.
#[cfg(unix)]
fn record_choice_command<C: RetentionRecheckClient>(
    config_dir: &Path,
    session: RetentionRecheckSession<C>,
    trigger: &RetentionTrigger,
    choice: RetentionChoice,
) -> Result<(), String> {
    write_retention_choice(config_dir, choice).map_err(|error| error.to_string())?;
    match session {
        RetentionRecheckSession::NoSupervisor => {
            trigger.check_now();
        }
        RetentionRecheckSession::Connected(client) => {
            let _ = client.recheck_retention();
        }
        RetentionRecheckSession::Disconnected => return Err(auth::background_service_error()),
    }
    Ok(())
}

/// Saves the choice, then asks the desktop schedule to check now.
#[cfg(not(unix))]
fn record_choice_command(
    config_dir: &Path,
    trigger: &RetentionTrigger,
    choice: RetentionChoice,
) -> Result<(), String> {
    write_retention_choice(config_dir, choice).map_err(|error| error.to_string())?;
    trigger.check_now();
    Ok(())
}

#[cfg(unix)]
#[tauri::command]
pub fn record_thread_retention_choice(
    app: AppHandle,
    state: tauri::State<'_, ChatState>,
    attach_state: tauri::State<'_, AttachCompanionState>,
    choice: RetentionChoice,
) -> Result<(), String> {
    record_choice_command(
        &config_dir(&app)?,
        attach_state.desktop_client_session().into(),
        &state.retention_trigger,
        choice,
    )
}

#[cfg(not(unix))]
#[tauri::command]
pub fn record_thread_retention_choice(
    app: AppHandle,
    state: tauri::State<'_, ChatState>,
    choice: RetentionChoice,
) -> Result<(), String> {
    record_choice_command(&config_dir(&app)?, &state.retention_trigger, choice)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::TryRecvError;
    use uuid::Uuid;

    fn test_directory(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("muniment-retention-save-{name}-{}", Uuid::now_v7()))
    }

    #[cfg(unix)]
    #[derive(Clone)]
    struct FakeRecheckClient {
        calls: std::sync::Arc<std::sync::Mutex<usize>>,
        result: Result<(), ClientError>,
    }

    #[cfg(unix)]
    impl FakeRecheckClient {
        fn new(result: Result<(), ClientError>) -> Self {
            Self {
                calls: std::sync::Arc::new(std::sync::Mutex::new(0)),
                result,
            }
        }

        fn calls(&self) -> usize {
            *self.calls.lock().unwrap()
        }
    }

    #[cfg(unix)]
    impl RetentionRecheckClient for FakeRecheckClient {
        fn recheck_retention(&self) -> Result<(), ClientError> {
            *self.calls.lock().unwrap() += 1;
            self.result.clone()
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_connected_save_sends_the_recheck_after_the_local_write() {
        let directory = test_directory("connected");
        let (trigger, checks) = RetentionTrigger::for_test();
        let client = FakeRecheckClient::new(Ok(()));

        record_choice_command(
            &directory,
            RetentionRecheckSession::Connected(client.clone()),
            &trigger,
            RetentionChoice::DeleteAfter90Days,
        )
        .unwrap();

        assert_eq!(
            read_retention_choice(&directory),
            Some(RetentionChoice::DeleteAfter90Days)
        );
        assert_eq!(client.calls(), 1);
        assert_eq!(checks.try_recv(), Err(TryRecvError::Empty));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_recheck_keeps_the_saved_choice() {
        let directory = test_directory("failed-recheck");
        let (trigger, _checks) = RetentionTrigger::for_test();
        let client = FakeRecheckClient::new(Err(ClientError::DesktopUnavailable));

        record_choice_command(
            &directory,
            RetentionRecheckSession::Connected(client.clone()),
            &trigger,
            RetentionChoice::DeleteAfter30Days,
        )
        .unwrap();

        assert_eq!(client.calls(), 1);
        assert_eq!(
            read_retention_choice(&directory),
            Some(RetentionChoice::DeleteAfter30Days)
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_save_without_a_supervisor_triggers_the_desktop_check() {
        let directory = test_directory("no-supervisor");
        let (trigger, checks) = RetentionTrigger::for_test();

        record_choice_command::<FakeRecheckClient>(
            &directory,
            RetentionRecheckSession::NoSupervisor,
            &trigger,
            RetentionChoice::DeleteAfter1Year,
        )
        .unwrap();

        assert_eq!(checks.try_recv(), Ok(()));
        assert_eq!(
            read_retention_choice(&directory),
            Some(RetentionChoice::DeleteAfter1Year)
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_disconnected_save_returns_the_background_service_error() {
        let directory = test_directory("disconnected");
        let (trigger, checks) = RetentionTrigger::for_test();

        let error = record_choice_command::<FakeRecheckClient>(
            &directory,
            RetentionRecheckSession::Disconnected,
            &trigger,
            RetentionChoice::KeepEveryThread,
        )
        .unwrap_err();

        assert_eq!(error, "Muniment cannot reach its background service.");
        assert_eq!(checks.try_recv(), Err(TryRecvError::Empty));
        assert_eq!(
            read_retention_choice(&directory),
            Some(RetentionChoice::KeepEveryThread)
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_write_sends_no_recheck() {
        let directory = test_directory("failed-write");
        std::fs::write(&directory, b"not a directory").unwrap();
        let (trigger, checks) = RetentionTrigger::for_test();
        let client = FakeRecheckClient::new(Ok(()));

        assert!(record_choice_command(
            &directory,
            RetentionRecheckSession::Connected(client.clone()),
            &trigger,
            RetentionChoice::DeleteAfter30Days,
        )
        .is_err());

        assert_eq!(client.calls(), 0);
        assert_eq!(checks.try_recv(), Err(TryRecvError::Empty));
        std::fs::remove_file(directory).unwrap();
    }

    #[cfg(not(unix))]
    #[test]
    fn a_save_triggers_the_desktop_check() {
        let directory = test_directory("desktop-journal");
        let (trigger, checks) = RetentionTrigger::for_test();

        record_choice_command(&directory, &trigger, RetentionChoice::DeleteAfter90Days).unwrap();

        assert_eq!(checks.try_recv(), Ok(()));
        assert_eq!(
            read_retention_choice(&directory),
            Some(RetentionChoice::DeleteAfter90Days)
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(not(unix))]
    #[test]
    fn a_failed_write_sends_no_trigger() {
        let directory = test_directory("desktop-failed-write");
        std::fs::write(&directory, b"not a directory").unwrap();
        let (trigger, checks) = RetentionTrigger::for_test();

        assert!(
            record_choice_command(&directory, &trigger, RetentionChoice::DeleteAfter30Days)
                .is_err()
        );

        assert_eq!(checks.try_recv(), Err(TryRecvError::Empty));
        std::fs::remove_file(directory).unwrap();
    }
}
