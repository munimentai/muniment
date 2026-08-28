use std::path::Path;
use std::time::Duration;

const INSTALL_LOCK_WAIT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeTaskRegistrationOutcome {
    Registered,
    Updated,
    Unchanged,
    NoInstalledPayload,
    LockUnavailable,
    LockTimedOut,
    Refused,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LockError {
    Unavailable,
    TimedOut,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RegistrationResult {
    Registered,
    Updated,
    Unchanged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RegistrationError {
    NoInstalledPayload,
    Refused,
    Failed,
}

trait InstallLockGuard {}

trait InstallLockAdapter {
    fn acquire(
        &self,
        state_directory: &Path,
        bounded_wait: Duration,
    ) -> Result<Box<dyn InstallLockGuard>, LockError>;
}

trait RuntimeTaskAdapter {
    fn ensure_registration(&self) -> Result<RegistrationResult, RegistrationError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DiagnosticEvent {
    InstallLockUnavailable,
    RuntimeTaskRegistrationFailed,
}

trait DiagnosticSink {
    fn write(&self, event: DiagnosticEvent);
}

fn register_runtime_task(
    state_directory: &Path,
    lock_adapter: &impl InstallLockAdapter,
    task_adapter: &impl RuntimeTaskAdapter,
    diagnostic_sink: &impl DiagnosticSink,
) -> RuntimeTaskRegistrationOutcome {
    let _guard = match lock_adapter.acquire(state_directory, INSTALL_LOCK_WAIT) {
        Ok(guard) => guard,
        Err(LockError::Unavailable) => {
            diagnostic_sink.write(DiagnosticEvent::InstallLockUnavailable);
            return RuntimeTaskRegistrationOutcome::LockUnavailable;
        }
        Err(LockError::TimedOut) => {
            diagnostic_sink.write(DiagnosticEvent::InstallLockUnavailable);
            return RuntimeTaskRegistrationOutcome::LockTimedOut;
        }
    };

    let outcome = match task_adapter.ensure_registration() {
        Ok(RegistrationResult::Registered) => RuntimeTaskRegistrationOutcome::Registered,
        Ok(RegistrationResult::Updated) => RuntimeTaskRegistrationOutcome::Updated,
        Ok(RegistrationResult::Unchanged) => RuntimeTaskRegistrationOutcome::Unchanged,
        Err(RegistrationError::NoInstalledPayload) => {
            RuntimeTaskRegistrationOutcome::NoInstalledPayload
        }
        Err(RegistrationError::Refused) => RuntimeTaskRegistrationOutcome::Refused,
        Err(RegistrationError::Failed) => RuntimeTaskRegistrationOutcome::Failed,
    };
    if matches!(
        outcome,
        RuntimeTaskRegistrationOutcome::NoInstalledPayload
            | RuntimeTaskRegistrationOutcome::Refused
            | RuntimeTaskRegistrationOutcome::Failed
    ) {
        diagnostic_sink.write(DiagnosticEvent::RuntimeTaskRegistrationFailed);
    }
    outcome
}

#[cfg(target_os = "windows")]
pub(crate) fn register_runtime_task_at_startup(state_directory: &Path) {
    let _ = register_runtime_task(
        state_directory,
        &WindowsInstallLockAdapter,
        &WindowsRuntimeTaskAdapter,
        &WindowsDiagnosticSink,
    );
}

#[cfg(target_os = "windows")]
struct WindowsDiagnosticSink;

#[cfg(target_os = "windows")]
impl DiagnosticSink for WindowsDiagnosticSink {
    fn write(&self, event: DiagnosticEvent) {
        let event = match event {
            DiagnosticEvent::InstallLockUnavailable => {
                muniment_runtime::WindowsDiagnosticEvent::InstallLockUnavailable
            }
            DiagnosticEvent::RuntimeTaskRegistrationFailed => {
                muniment_runtime::WindowsDiagnosticEvent::RuntimeTaskRegistrationFailed
            }
        };
        if let Ok(local_app_data) = muniment_runtime::windows_local_app_data() {
            let _ = muniment_runtime::write_windows_diagnostic(local_app_data, event);
        }
    }
}

#[cfg(target_os = "windows")]
struct WindowsInstallLockAdapter;

#[cfg(target_os = "windows")]
impl InstallLockGuard for muniment_runtime::install_lock::InstallLockGuard {}

#[cfg(target_os = "windows")]
impl InstallLockAdapter for WindowsInstallLockAdapter {
    fn acquire(
        &self,
        state_directory: &Path,
        bounded_wait: Duration,
    ) -> Result<Box<dyn InstallLockGuard>, LockError> {
        muniment_runtime::install_lock::acquire(state_directory, bounded_wait)
            .map(|guard| Box::new(guard) as Box<dyn InstallLockGuard>)
            .map_err(|error| match error {
                muniment_runtime::install_lock::InstallLockAcquireError::Unavailable => {
                    LockError::Unavailable
                }
                muniment_runtime::install_lock::InstallLockAcquireError::TimedOut => {
                    LockError::TimedOut
                }
            })
    }
}

#[cfg(target_os = "windows")]
struct WindowsRuntimeTaskAdapter;

#[cfg(target_os = "windows")]
impl RuntimeTaskAdapter for WindowsRuntimeTaskAdapter {
    fn ensure_registration(&self) -> Result<RegistrationResult, RegistrationError> {
        use muniment_core::windows_task::TaskRegistrationPlan;
        use muniment_core::windows_task_service::{
            ensure_live_task_registration, EnsureLiveTaskRegistrationError,
            EnsureTaskRegistrationError,
        };

        match ensure_live_task_registration() {
            Ok(TaskRegistrationPlan::Register) => Ok(RegistrationResult::Registered),
            Ok(TaskRegistrationPlan::Update) => Ok(RegistrationResult::Updated),
            Ok(TaskRegistrationPlan::LeaveUnchanged) => Ok(RegistrationResult::Unchanged),
            Ok(TaskRegistrationPlan::Refuse) => Err(RegistrationError::Refused),
            Err(EnsureLiveTaskRegistrationError::NoInstalledPayload) => {
                Err(RegistrationError::NoInstalledPayload)
            }
            Err(EnsureLiveTaskRegistrationError::EnsureRegistration(
                EnsureTaskRegistrationError::Refused,
            )) => Err(RegistrationError::Refused),
            Err(_) => Err(RegistrationError::Failed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    struct FakeGuard {
        held: Rc<Cell<bool>>,
    }

    impl Drop for FakeGuard {
        fn drop(&mut self) {
            self.held.set(false);
        }
    }

    impl InstallLockGuard for FakeGuard {}

    struct FakeLockAdapter {
        result: Result<(), LockError>,
        calls: Cell<usize>,
        wait: Cell<Option<Duration>>,
        held: Rc<Cell<bool>>,
    }

    impl FakeLockAdapter {
        fn new(result: Result<(), LockError>) -> Self {
            Self {
                result,
                calls: Cell::new(0),
                wait: Cell::new(None),
                held: Rc::new(Cell::new(false)),
            }
        }
    }

    impl InstallLockAdapter for FakeLockAdapter {
        fn acquire(
            &self,
            _state_directory: &Path,
            bounded_wait: Duration,
        ) -> Result<Box<dyn InstallLockGuard>, LockError> {
            self.calls.set(self.calls.get() + 1);
            self.wait.set(Some(bounded_wait));
            self.result.map(|()| {
                self.held.set(true);
                Box::new(FakeGuard {
                    held: Rc::clone(&self.held),
                }) as Box<dyn InstallLockGuard>
            })
        }
    }

    struct FakeTaskAdapter {
        result: Result<RegistrationResult, RegistrationError>,
        calls: Cell<usize>,
        lock_held: Option<Rc<Cell<bool>>>,
    }

    impl FakeTaskAdapter {
        fn new(result: Result<RegistrationResult, RegistrationError>) -> Self {
            Self {
                result,
                calls: Cell::new(0),
                lock_held: None,
            }
        }

        fn with_lock(
            result: Result<RegistrationResult, RegistrationError>,
            lock_held: Rc<Cell<bool>>,
        ) -> Self {
            Self {
                result,
                calls: Cell::new(0),
                lock_held: Some(lock_held),
            }
        }
    }

    impl RuntimeTaskAdapter for FakeTaskAdapter {
        fn ensure_registration(&self) -> Result<RegistrationResult, RegistrationError> {
            if let Some(lock_held) = &self.lock_held {
                assert!(lock_held.get());
            }
            self.calls.set(self.calls.get() + 1);
            self.result
        }
    }

    #[derive(Default)]
    struct FakeDiagnosticSink {
        events: RefCell<Vec<DiagnosticEvent>>,
    }

    impl DiagnosticSink for FakeDiagnosticSink {
        fn write(&self, event: DiagnosticEvent) {
            self.events.borrow_mut().push(event);
        }
    }

    #[test]
    fn maps_each_registration_result_while_holding_the_lock() {
        for (result, expected) in [
            (
                Ok(RegistrationResult::Registered),
                RuntimeTaskRegistrationOutcome::Registered,
            ),
            (
                Ok(RegistrationResult::Updated),
                RuntimeTaskRegistrationOutcome::Updated,
            ),
            (
                Ok(RegistrationResult::Unchanged),
                RuntimeTaskRegistrationOutcome::Unchanged,
            ),
            (
                Err(RegistrationError::NoInstalledPayload),
                RuntimeTaskRegistrationOutcome::NoInstalledPayload,
            ),
            (
                Err(RegistrationError::Refused),
                RuntimeTaskRegistrationOutcome::Refused,
            ),
            (
                Err(RegistrationError::Failed),
                RuntimeTaskRegistrationOutcome::Failed,
            ),
        ] {
            let lock = FakeLockAdapter::new(Ok(()));
            let task = FakeTaskAdapter::with_lock(result, Rc::clone(&lock.held));
            let diagnostics = FakeDiagnosticSink::default();

            assert_eq!(
                register_runtime_task(Path::new("state"), &lock, &task, &diagnostics),
                expected
            );
            assert_eq!(lock.calls.get(), 1);
            assert_eq!(lock.wait.get(), Some(INSTALL_LOCK_WAIT));
            assert_eq!(task.calls.get(), 1);
            assert!(!lock.held.get());
            let expected_events = match expected {
                RuntimeTaskRegistrationOutcome::NoInstalledPayload
                | RuntimeTaskRegistrationOutcome::Refused
                | RuntimeTaskRegistrationOutcome::Failed => {
                    vec![DiagnosticEvent::RuntimeTaskRegistrationFailed]
                }
                _ => Vec::new(),
            };
            assert_eq!(*diagnostics.events.borrow(), expected_events);
        }
    }

    #[test]
    fn maps_lock_failures_without_attempting_registration() {
        for (error, expected) in [
            (
                LockError::Unavailable,
                RuntimeTaskRegistrationOutcome::LockUnavailable,
            ),
            (
                LockError::TimedOut,
                RuntimeTaskRegistrationOutcome::LockTimedOut,
            ),
        ] {
            let lock = FakeLockAdapter::new(Err(error));
            let task = FakeTaskAdapter::new(Ok(RegistrationResult::Registered));
            let diagnostics = FakeDiagnosticSink::default();

            assert_eq!(
                register_runtime_task(Path::new("state"), &lock, &task, &diagnostics),
                expected
            );
            assert_eq!(lock.calls.get(), 1);
            assert_eq!(task.calls.get(), 0);
            assert_eq!(
                *diagnostics.events.borrow(),
                vec![DiagnosticEvent::InstallLockUnavailable]
            );
        }
    }
}
