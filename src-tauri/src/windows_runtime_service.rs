use std::path::Path;
use std::time::Duration;

const INSTALL_LOCK_WAIT: Duration = Duration::from_secs(2);
#[cfg(target_os = "windows")]
const ATTACH_ENDPOINT_CHECK_WAIT: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeTaskStartOutcome {
    EndpointPresent,
    Requested,
    EndpointCheckFailed,
    ClearFailed,
    StartFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeTaskStartError {
    ClearFailed,
    StartFailed,
}

trait AttachEndpointAdapter {
    fn endpoint_exists(&self) -> Result<bool, ()>;
}

trait CrashWindowAdapter {
    fn clear(&self) -> Result<(), ()>;
}

trait RuntimeTaskStartAdapter {
    fn start(
        &self,
        crash_window_adapter: &impl CrashWindowAdapter,
    ) -> Result<(), RuntimeTaskStartError>;
}

fn start_runtime_task(
    endpoint_adapter: &impl AttachEndpointAdapter,
    crash_window_adapter: &impl CrashWindowAdapter,
    task_adapter: &impl RuntimeTaskStartAdapter,
) -> RuntimeTaskStartOutcome {
    match endpoint_adapter.endpoint_exists() {
        Ok(true) => RuntimeTaskStartOutcome::EndpointPresent,
        Ok(false) => match task_adapter.start(crash_window_adapter) {
            Ok(()) => RuntimeTaskStartOutcome::Requested,
            Err(RuntimeTaskStartError::ClearFailed) => RuntimeTaskStartOutcome::ClearFailed,
            Err(RuntimeTaskStartError::StartFailed) => RuntimeTaskStartOutcome::StartFailed,
        },
        Err(()) => RuntimeTaskStartOutcome::EndpointCheckFailed,
    }
}

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
    RuntimeTaskStartFailed,
}

trait DiagnosticSink {
    fn write(&self, event: DiagnosticEvent);
}

fn start_runtime_task_with_diagnostic(
    endpoint_adapter: &impl AttachEndpointAdapter,
    crash_window_adapter: &impl CrashWindowAdapter,
    task_adapter: &impl RuntimeTaskStartAdapter,
    diagnostic_sink: &impl DiagnosticSink,
) -> RuntimeTaskStartOutcome {
    let outcome = start_runtime_task(endpoint_adapter, crash_window_adapter, task_adapter);
    if matches!(
        outcome,
        RuntimeTaskStartOutcome::EndpointCheckFailed
            | RuntimeTaskStartOutcome::ClearFailed
            | RuntimeTaskStartOutcome::StartFailed
    ) {
        diagnostic_sink.write(DiagnosticEvent::RuntimeTaskStartFailed);
    }
    outcome
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
pub(crate) fn start_runtime_task_at_startup(state_directory: &Path) {
    let _ = start_runtime_task_with_diagnostic(
        &WindowsAttachEndpointAdapter,
        &WindowsCrashWindowAdapter { state_directory },
        &WindowsRuntimeTaskStartAdapter,
        &WindowsDiagnosticSink,
    );
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
            DiagnosticEvent::RuntimeTaskStartFailed => {
                muniment_runtime::WindowsDiagnosticEvent::RuntimeTaskStartFailed
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
pub(crate) struct WindowsAttachEndpointAdapter;

#[cfg(target_os = "windows")]
impl AttachEndpointAdapter for WindowsAttachEndpointAdapter {
    fn endpoint_exists(&self) -> Result<bool, ()> {
        use muniment_core::attach::{connect_windows_attach_endpoint, WindowsAttachConnectError};
        use std::time::Instant;

        match connect_windows_attach_endpoint(Instant::now() + ATTACH_ENDPOINT_CHECK_WAIT) {
            Ok(stream) => {
                drop(stream);
                Ok(true)
            }
            Err(WindowsAttachConnectError::EndpointAbsent) => Ok(false),
            Err(_) => Err(()),
        }
    }
}

#[cfg(target_os = "windows")]
pub(crate) struct WindowsCrashWindowAdapter<'a> {
    state_directory: &'a Path,
}

#[cfg(target_os = "windows")]
impl CrashWindowAdapter for WindowsCrashWindowAdapter<'_> {
    fn clear(&self) -> Result<(), ()> {
        muniment_runtime::clear_windows_crash_window(self.state_directory, INSTALL_LOCK_WAIT)
            .map_err(|_| ())
    }
}

#[cfg(target_os = "windows")]
pub(crate) struct WindowsRuntimeTaskStartAdapter;

#[cfg(target_os = "windows")]
impl RuntimeTaskStartAdapter for WindowsRuntimeTaskStartAdapter {
    fn start(
        &self,
        crash_window_adapter: &impl CrashWindowAdapter,
    ) -> Result<(), RuntimeTaskStartError> {
        use muniment_core::windows_payload::resolve_live_windows_payload;
        use muniment_core::windows_sid::current_process_user_sid;
        use muniment_core::windows_task_service::{
            start_registered_task, StartRegisteredTaskError,
        };

        let sid = current_process_user_sid().map_err(|_| RuntimeTaskStartError::StartFailed)?;
        let payload = resolve_live_windows_payload()
            .map_err(|_| RuntimeTaskStartError::StartFailed)?
            .ok_or(RuntimeTaskStartError::StartFailed)?;
        match start_registered_task(sid.as_str(), payload.payload_path, || {
            crash_window_adapter.clear()
        }) {
            Ok(_) => Ok(()),
            Err(StartRegisteredTaskError::ClearCrashWindow(())) => {
                Err(RuntimeTaskStartError::ClearFailed)
            }
            Err(_) => Err(RuntimeTaskStartError::StartFailed),
        }
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

    struct FakeEndpointAdapter {
        result: Result<bool, ()>,
        calls: Cell<usize>,
        order: Rc<RefCell<Vec<&'static str>>>,
    }

    impl FakeEndpointAdapter {
        fn new(result: Result<bool, ()>, order: Rc<RefCell<Vec<&'static str>>>) -> Self {
            Self {
                result,
                calls: Cell::new(0),
                order,
            }
        }
    }

    impl AttachEndpointAdapter for FakeEndpointAdapter {
        fn endpoint_exists(&self) -> Result<bool, ()> {
            self.calls.set(self.calls.get() + 1);
            self.order.borrow_mut().push("endpoint");
            self.result
        }
    }

    struct FakeCrashWindowAdapter {
        result: Result<(), ()>,
        calls: Cell<usize>,
        order: Rc<RefCell<Vec<&'static str>>>,
    }

    impl FakeCrashWindowAdapter {
        fn new(result: Result<(), ()>, order: Rc<RefCell<Vec<&'static str>>>) -> Self {
            Self {
                result,
                calls: Cell::new(0),
                order,
            }
        }
    }

    impl CrashWindowAdapter for FakeCrashWindowAdapter {
        fn clear(&self) -> Result<(), ()> {
            self.calls.set(self.calls.get() + 1);
            self.order.borrow_mut().push("clear");
            self.result
        }
    }

    struct FakeRuntimeTaskStartAdapter {
        result: Result<(), RuntimeTaskStartError>,
        calls: Cell<usize>,
        run_calls: Cell<usize>,
        order: Rc<RefCell<Vec<&'static str>>>,
    }

    impl FakeRuntimeTaskStartAdapter {
        fn new(
            result: Result<(), RuntimeTaskStartError>,
            order: Rc<RefCell<Vec<&'static str>>>,
        ) -> Self {
            Self {
                result,
                calls: Cell::new(0),
                run_calls: Cell::new(0),
                order,
            }
        }
    }

    impl RuntimeTaskStartAdapter for FakeRuntimeTaskStartAdapter {
        fn start(
            &self,
            crash_window_adapter: &impl CrashWindowAdapter,
        ) -> Result<(), RuntimeTaskStartError> {
            self.calls.set(self.calls.get() + 1);
            crash_window_adapter
                .clear()
                .map_err(|()| RuntimeTaskStartError::ClearFailed)?;
            self.run_calls.set(self.run_calls.get() + 1);
            self.order.borrow_mut().push("start");
            self.result
        }
    }

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
    fn present_endpoint_skips_the_crash_window_and_task() {
        let order = Rc::new(RefCell::new(Vec::new()));
        let endpoint = FakeEndpointAdapter::new(Ok(true), Rc::clone(&order));
        let crash_window = FakeCrashWindowAdapter::new(Ok(()), Rc::clone(&order));
        let task = FakeRuntimeTaskStartAdapter::new(Ok(()), Rc::clone(&order));

        assert_eq!(
            start_runtime_task(&endpoint, &crash_window, &task),
            RuntimeTaskStartOutcome::EndpointPresent
        );
        assert_eq!(endpoint.calls.get(), 1);
        assert_eq!(crash_window.calls.get(), 0);
        assert_eq!(task.calls.get(), 0);
        assert_eq!(*order.borrow(), vec!["endpoint"]);
    }

    #[test]
    fn absent_endpoint_clears_the_crash_window_before_requesting_start() {
        let order = Rc::new(RefCell::new(Vec::new()));
        let endpoint = FakeEndpointAdapter::new(Ok(false), Rc::clone(&order));
        let crash_window = FakeCrashWindowAdapter::new(Ok(()), Rc::clone(&order));
        let task = FakeRuntimeTaskStartAdapter::new(Ok(()), Rc::clone(&order));

        assert_eq!(
            start_runtime_task(&endpoint, &crash_window, &task),
            RuntimeTaskStartOutcome::Requested
        );
        assert_eq!(crash_window.calls.get(), 1);
        assert_eq!(task.run_calls.get(), 1);
        assert_eq!(*order.borrow(), vec!["endpoint", "clear", "start"]);
    }

    #[test]
    fn endpoint_check_failure_skips_the_crash_window_and_task() {
        let order = Rc::new(RefCell::new(Vec::new()));
        let endpoint = FakeEndpointAdapter::new(Err(()), Rc::clone(&order));
        let crash_window = FakeCrashWindowAdapter::new(Ok(()), Rc::clone(&order));
        let task = FakeRuntimeTaskStartAdapter::new(Ok(()), Rc::clone(&order));

        assert_eq!(
            start_runtime_task(&endpoint, &crash_window, &task),
            RuntimeTaskStartOutcome::EndpointCheckFailed
        );
        assert_eq!(crash_window.calls.get(), 0);
        assert_eq!(task.calls.get(), 0);
        assert_eq!(*order.borrow(), vec!["endpoint"]);
    }

    #[test]
    fn crash_window_failure_skips_the_task_start() {
        let order = Rc::new(RefCell::new(Vec::new()));
        let endpoint = FakeEndpointAdapter::new(Ok(false), Rc::clone(&order));
        let crash_window = FakeCrashWindowAdapter::new(Err(()), Rc::clone(&order));
        let task = FakeRuntimeTaskStartAdapter::new(Ok(()), Rc::clone(&order));

        assert_eq!(
            start_runtime_task(&endpoint, &crash_window, &task),
            RuntimeTaskStartOutcome::ClearFailed
        );
        assert_eq!(crash_window.calls.get(), 1);
        assert_eq!(task.run_calls.get(), 0);
        assert_eq!(*order.borrow(), vec!["endpoint", "clear"]);
    }

    #[test]
    fn task_start_failure_has_its_own_outcome() {
        let order = Rc::new(RefCell::new(Vec::new()));
        let endpoint = FakeEndpointAdapter::new(Ok(false), Rc::clone(&order));
        let crash_window = FakeCrashWindowAdapter::new(Ok(()), Rc::clone(&order));
        let task = FakeRuntimeTaskStartAdapter::new(
            Err(RuntimeTaskStartError::StartFailed),
            Rc::clone(&order),
        );

        assert_eq!(
            start_runtime_task(&endpoint, &crash_window, &task),
            RuntimeTaskStartOutcome::StartFailed
        );
        assert_eq!(crash_window.calls.get(), 1);
        assert_eq!(task.run_calls.get(), 1);
        assert_eq!(*order.borrow(), vec!["endpoint", "clear", "start"]);
    }

    #[test]
    fn writes_a_diagnostic_only_for_failed_start_outcomes() {
        for (endpoint_result, clear_result, start_result, expected_outcome, expected_events) in [
            (
                Ok(true),
                Ok(()),
                Ok(()),
                RuntimeTaskStartOutcome::EndpointPresent,
                vec![],
            ),
            (
                Ok(false),
                Ok(()),
                Ok(()),
                RuntimeTaskStartOutcome::Requested,
                vec![],
            ),
            (
                Err(()),
                Ok(()),
                Ok(()),
                RuntimeTaskStartOutcome::EndpointCheckFailed,
                vec![DiagnosticEvent::RuntimeTaskStartFailed],
            ),
            (
                Ok(false),
                Err(()),
                Ok(()),
                RuntimeTaskStartOutcome::ClearFailed,
                vec![DiagnosticEvent::RuntimeTaskStartFailed],
            ),
            (
                Ok(false),
                Ok(()),
                Err(RuntimeTaskStartError::StartFailed),
                RuntimeTaskStartOutcome::StartFailed,
                vec![DiagnosticEvent::RuntimeTaskStartFailed],
            ),
        ] {
            let order = Rc::new(RefCell::new(Vec::new()));
            let endpoint = FakeEndpointAdapter::new(endpoint_result, Rc::clone(&order));
            let crash_window = FakeCrashWindowAdapter::new(clear_result, Rc::clone(&order));
            let task = FakeRuntimeTaskStartAdapter::new(start_result, order);
            let diagnostics = FakeDiagnosticSink::default();

            assert_eq!(
                start_runtime_task_with_diagnostic(&endpoint, &crash_window, &task, &diagnostics,),
                expected_outcome
            );
            assert_eq!(*diagnostics.events.borrow(), expected_events);
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
