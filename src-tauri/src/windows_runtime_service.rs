use std::path::Path;
use std::time::Duration;

const INSTALL_LOCK_WAIT: Duration = Duration::from_secs(2);
const ATTACH_ENDPOINT_READINESS_WAIT: Duration = Duration::from_secs(5);
#[cfg(target_os = "windows")]
const ATTACH_ENDPOINT_CHECK_WAIT: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, Eq, PartialEq)]
enum RuntimeTaskStartOutcome {
    EndpointPresent,
    Ready,
    EndpointCheckFailed(String),
    ClearFailed(String),
    StartFailed(String),
    Queued { session_id: u32 },
    ReadinessTimedOut,
    ReadinessFailed(String),
}

impl RuntimeTaskStartOutcome {
    fn cause(&self) -> Option<String> {
        match self {
            Self::EndpointPresent | Self::Ready => None,
            Self::EndpointCheckFailed(error) => Some(format!("Endpoint lookup failed: {error}")),
            Self::ClearFailed(error) => Some(format!("Crash window clear failed: {error}")),
            Self::StartFailed(error) => Some(error.clone()),
            Self::Queued { session_id } => Some(format!(
                "Task Scheduler kept the runtime task in state Queued for session id {session_id}."
            )),
            Self::ReadinessTimedOut => Some(
                "Runtime readiness failed: the attach endpoint did not appear within 5 seconds."
                    .into(),
            ),
            Self::ReadinessFailed(error) => Some(format!("Runtime readiness failed: {error}")),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RuntimeTaskStartError {
    ClearFailed(String),
    StartFailed(String),
    Queued { session_id: u32 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RuntimeReadinessError {
    TimedOut,
    Failed(String),
}

trait AttachEndpointAdapter {
    fn endpoint_exists(&self) -> Result<bool, String>;
}

trait CrashWindowAdapter {
    fn clear(&self) -> Result<(), String>;
}

trait RuntimeReadinessAdapter {
    fn wait_until_ready(&self, bounded_wait: Duration) -> Result<(), RuntimeReadinessError>;
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
    readiness_adapter: &impl RuntimeReadinessAdapter,
) -> RuntimeTaskStartOutcome {
    match endpoint_adapter.endpoint_exists() {
        Ok(true) => RuntimeTaskStartOutcome::EndpointPresent,
        Ok(false) => match task_adapter.start(crash_window_adapter) {
            Ok(()) => match readiness_adapter.wait_until_ready(ATTACH_ENDPOINT_READINESS_WAIT) {
                Ok(()) => RuntimeTaskStartOutcome::Ready,
                Err(RuntimeReadinessError::TimedOut) => RuntimeTaskStartOutcome::ReadinessTimedOut,
                Err(RuntimeReadinessError::Failed(error)) => {
                    RuntimeTaskStartOutcome::ReadinessFailed(error)
                }
            },
            Err(RuntimeTaskStartError::ClearFailed(error)) => {
                RuntimeTaskStartOutcome::ClearFailed(error)
            }
            Err(RuntimeTaskStartError::StartFailed(error)) => {
                RuntimeTaskStartOutcome::StartFailed(error)
            }
            Err(RuntimeTaskStartError::Queued { session_id }) => {
                RuntimeTaskStartOutcome::Queued { session_id }
            }
        },
        Err(error) => RuntimeTaskStartOutcome::EndpointCheckFailed(error),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RuntimeTaskRegistrationOutcome {
    Registered,
    Updated,
    Unchanged,
    NoInstalledPayload,
    LockUnavailable,
    LockTimedOut,
    Refused,
    Failed(String),
}

impl RuntimeTaskRegistrationOutcome {
    fn cause(&self) -> Option<String> {
        match self {
            Self::Registered | Self::Updated | Self::Unchanged => None,
            Self::NoInstalledPayload => {
                Some("Payload lookup failed: no installed runtime payload exists.".into())
            }
            Self::LockUnavailable => {
                Some("Task registration failed: the install lock is unavailable.".into())
            }
            Self::LockTimedOut => {
                Some("Task registration failed: the install lock wait timed out.".into())
            }
            Self::Refused => {
                Some("Task registration failed: refused to replace a foreign runtime task.".into())
            }
            Self::Failed(error) => Some(error.clone()),
        }
    }
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

#[derive(Clone, Debug, Eq, PartialEq)]
enum RegistrationError {
    NoInstalledPayload,
    Refused,
    Failed(String),
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
    fn write(&self, event: DiagnosticEvent, cause: &str);
}

fn start_runtime_task_with_diagnostic(
    endpoint_adapter: &impl AttachEndpointAdapter,
    crash_window_adapter: &impl CrashWindowAdapter,
    task_adapter: &impl RuntimeTaskStartAdapter,
    readiness_adapter: &impl RuntimeReadinessAdapter,
    diagnostic_sink: &impl DiagnosticSink,
) -> RuntimeTaskStartOutcome {
    let outcome = start_runtime_task(
        endpoint_adapter,
        crash_window_adapter,
        task_adapter,
        readiness_adapter,
    );
    if let Some(cause) = outcome.cause() {
        diagnostic_sink.write(DiagnosticEvent::RuntimeTaskStartFailed, &cause);
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
            diagnostic_sink.write(
                DiagnosticEvent::InstallLockUnavailable,
                &RuntimeTaskRegistrationOutcome::LockUnavailable
                    .cause()
                    .unwrap(),
            );
            return RuntimeTaskRegistrationOutcome::LockUnavailable;
        }
        Err(LockError::TimedOut) => {
            diagnostic_sink.write(
                DiagnosticEvent::InstallLockUnavailable,
                &RuntimeTaskRegistrationOutcome::LockTimedOut
                    .cause()
                    .unwrap(),
            );
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
        Err(RegistrationError::Failed(error)) => RuntimeTaskRegistrationOutcome::Failed(error),
    };
    if let Some(cause) = outcome.cause() {
        diagnostic_sink.write(DiagnosticEvent::RuntimeTaskRegistrationFailed, &cause);
    }
    outcome
}

#[cfg(target_os = "windows")]
pub(crate) fn start_runtime_task_at_startup(state_directory: &Path) -> Result<(), String> {
    let outcome = start_runtime_task_with_diagnostic(
        &WindowsAttachEndpointAdapter,
        &WindowsCrashWindowAdapter { state_directory },
        &WindowsRuntimeTaskStartAdapter,
        &WindowsRuntimeReadinessAdapter,
        &WindowsDiagnosticSink,
    );
    outcome.cause().map_or(Ok(()), Err)
}

#[cfg(target_os = "windows")]
pub(crate) fn stop_runtime() -> Result<(), ()> {
    use muniment_core::windows_payload::resolve_live_windows_payload;
    use muniment_core::windows_sid::current_process_user_sid;
    use muniment_core::windows_task::{registration_verdict, RegistrationVerdict, TaskDefinition};
    use muniment_core::windows_task_service::read_observed_registration;
    use std::process::{Command, Stdio};

    let sid = current_process_user_sid().map_err(|_| ())?;
    let payload = resolve_live_windows_payload().map_err(|_| ())?.ok_or(())?;
    let expected = TaskDefinition::new(sid.as_str(), payload.payload_path).map_err(|_| ())?;
    let observed = read_observed_registration(sid.as_str())
        .map_err(|_| ())?
        .ok_or(())?;
    if registration_verdict(&expected, &observed) == RegistrationVerdict::Foreign {
        return Err(());
    }
    let system = std::env::var_os("SystemRoot").ok_or(())?;
    Command::new(std::path::PathBuf::from(system).join("System32/schtasks.exe"))
        .args(["/End", "/TN", expected.uri.as_str()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| ())?
        .success()
        .then_some(())
        .ok_or(())
}

#[cfg(target_os = "windows")]
pub(crate) fn register_runtime_task_at_startup(state_directory: &Path) -> Result<(), String> {
    let outcome = register_runtime_task(
        state_directory,
        &WindowsInstallLockAdapter,
        &WindowsRuntimeTaskAdapter,
        &WindowsDiagnosticSink,
    );
    outcome.cause().map_or(Ok(()), Err)
}

#[cfg(target_os = "windows")]
struct WindowsDiagnosticSink;

#[cfg(target_os = "windows")]
impl DiagnosticSink for WindowsDiagnosticSink {
    fn write(&self, event: DiagnosticEvent, cause: &str) {
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
        match muniment_runtime::windows_local_app_data() {
            Ok(local_app_data) => {
                if let Err(error) = muniment_runtime::write_windows_diagnostic(
                    local_app_data,
                    event.with_cause(cause),
                ) {
                    eprintln!("Runtime diagnostic write failed: {error}");
                }
            }
            Err(error) => eprintln!("Runtime diagnostic path lookup failed: {error}"),
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
    fn endpoint_exists(&self) -> Result<bool, String> {
        use muniment_core::attach::{connect_windows_attach_endpoint, WindowsAttachConnectError};
        use std::time::Instant;

        match connect_windows_attach_endpoint(Instant::now() + ATTACH_ENDPOINT_CHECK_WAIT) {
            Ok(stream) => {
                drop(stream);
                Ok(true)
            }
            Err(WindowsAttachConnectError::EndpointAbsent) => Ok(false),
            Err(error) => Err(format!("{error:?}")),
        }
    }
}

#[cfg(target_os = "windows")]
struct WindowsRuntimeReadinessAdapter;

#[cfg(target_os = "windows")]
impl RuntimeReadinessAdapter for WindowsRuntimeReadinessAdapter {
    fn wait_until_ready(&self, bounded_wait: Duration) -> Result<(), RuntimeReadinessError> {
        use muniment_core::attach::{wait_for_windows_attach_endpoint, WindowsAttachConnectError};
        use std::time::Instant;

        match wait_for_windows_attach_endpoint(Instant::now() + bounded_wait) {
            Ok(stream) => {
                drop(stream);
                Ok(())
            }
            Err(WindowsAttachConnectError::DeadlineExpired) => Err(RuntimeReadinessError::TimedOut),
            Err(error) => Err(RuntimeReadinessError::Failed(format!("{error:?}"))),
        }
    }
}

#[cfg(target_os = "windows")]
pub(crate) struct WindowsCrashWindowAdapter<'a> {
    state_directory: &'a Path,
}

#[cfg(target_os = "windows")]
impl CrashWindowAdapter for WindowsCrashWindowAdapter<'_> {
    fn clear(&self) -> Result<(), String> {
        muniment_runtime::clear_windows_crash_window(self.state_directory, INSTALL_LOCK_WAIT)
            .map_err(|error| error.to_string())
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

        let sid = current_process_user_sid().map_err(|error| {
            RuntimeTaskStartError::StartFailed(format!("SID lookup failed: {error:?}"))
        })?;
        let payload = resolve_live_windows_payload()
            .map_err(|error| {
                RuntimeTaskStartError::StartFailed(format!("Payload lookup failed: {error:?}"))
            })?
            .ok_or_else(|| {
                RuntimeTaskStartError::StartFailed(
                    "Payload lookup failed: no installed runtime payload exists.".into(),
                )
            })?;
        match start_registered_task(sid.as_str(), payload.payload_path, || {
            crash_window_adapter.clear()
        }) {
            Ok(_) => Ok(()),
            Err(StartRegisteredTaskError::ClearCrashWindow(error)) => {
                Err(RuntimeTaskStartError::ClearFailed(error))
            }
            Err(StartRegisteredTaskError::Queued { session_id }) => {
                Err(RuntimeTaskStartError::Queued { session_id })
            }
            Err(error) => Err(RuntimeTaskStartError::StartFailed(format!(
                "Task start failed: {error} ({error:?})"
            ))),
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
            Err(EnsureLiveTaskRegistrationError::ReadProcessUserSid(error)) => Err(
                RegistrationError::Failed(format!("SID lookup failed: {error:?}")),
            ),
            Err(EnsureLiveTaskRegistrationError::ResolvePayload(error)) => Err(
                RegistrationError::Failed(format!("Payload lookup failed: {error:?}")),
            ),
            Err(error) => Err(RegistrationError::Failed(format!(
                "Task registration failed: {error} ({error:?})"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    struct FakeEndpointAdapter {
        result: Result<bool, String>,
        calls: Cell<usize>,
        order: Rc<RefCell<Vec<&'static str>>>,
    }

    impl FakeEndpointAdapter {
        fn new(result: Result<bool, String>, order: Rc<RefCell<Vec<&'static str>>>) -> Self {
            Self {
                result,
                calls: Cell::new(0),
                order,
            }
        }
    }

    impl AttachEndpointAdapter for FakeEndpointAdapter {
        fn endpoint_exists(&self) -> Result<bool, String> {
            self.calls.set(self.calls.get() + 1);
            self.order.borrow_mut().push("endpoint");
            self.result.clone()
        }
    }

    struct FakeRuntimeReadinessAdapter {
        result: Result<(), RuntimeReadinessError>,
        calls: Cell<usize>,
        bounded_wait: Cell<Option<Duration>>,
        order: Rc<RefCell<Vec<&'static str>>>,
    }

    impl FakeRuntimeReadinessAdapter {
        fn new(
            result: Result<(), RuntimeReadinessError>,
            order: Rc<RefCell<Vec<&'static str>>>,
        ) -> Self {
            Self {
                result,
                calls: Cell::new(0),
                bounded_wait: Cell::new(None),
                order,
            }
        }
    }

    impl RuntimeReadinessAdapter for FakeRuntimeReadinessAdapter {
        fn wait_until_ready(&self, bounded_wait: Duration) -> Result<(), RuntimeReadinessError> {
            self.calls.set(self.calls.get() + 1);
            self.bounded_wait.set(Some(bounded_wait));
            self.order.borrow_mut().push("readiness");
            self.result.clone()
        }
    }

    struct FakeCrashWindowAdapter {
        result: Result<(), String>,
        calls: Cell<usize>,
        order: Rc<RefCell<Vec<&'static str>>>,
    }

    impl FakeCrashWindowAdapter {
        fn new(result: Result<(), String>, order: Rc<RefCell<Vec<&'static str>>>) -> Self {
            Self {
                result,
                calls: Cell::new(0),
                order,
            }
        }
    }

    impl CrashWindowAdapter for FakeCrashWindowAdapter {
        fn clear(&self) -> Result<(), String> {
            self.calls.set(self.calls.get() + 1);
            self.order.borrow_mut().push("clear");
            self.result.clone()
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
                .map_err(RuntimeTaskStartError::ClearFailed)?;
            self.run_calls.set(self.run_calls.get() + 1);
            self.order.borrow_mut().push("start");
            self.result.clone()
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
            self.result.clone()
        }
    }

    #[derive(Default)]
    struct FakeDiagnosticSink {
        events: RefCell<Vec<DiagnosticEvent>>,
        causes: RefCell<Vec<String>>,
    }

    impl DiagnosticSink for FakeDiagnosticSink {
        fn write(&self, event: DiagnosticEvent, cause: &str) {
            self.events.borrow_mut().push(event);
            self.causes.borrow_mut().push(cause.to_owned());
        }
    }

    #[test]
    fn present_endpoint_skips_the_crash_window_task_and_readiness_wait() {
        let order = Rc::new(RefCell::new(Vec::new()));
        let endpoint = FakeEndpointAdapter::new(Ok(true), Rc::clone(&order));
        let crash_window = FakeCrashWindowAdapter::new(Ok(()), Rc::clone(&order));
        let task = FakeRuntimeTaskStartAdapter::new(Ok(()), Rc::clone(&order));
        let readiness = FakeRuntimeReadinessAdapter::new(Ok(()), Rc::clone(&order));

        assert_eq!(
            start_runtime_task(&endpoint, &crash_window, &task, &readiness),
            RuntimeTaskStartOutcome::EndpointPresent
        );
        assert_eq!(endpoint.calls.get(), 1);
        assert_eq!(crash_window.calls.get(), 0);
        assert_eq!(task.calls.get(), 0);
        assert_eq!(readiness.calls.get(), 0);
        assert_eq!(*order.borrow(), vec!["endpoint"]);
    }

    #[test]
    fn requested_start_waits_for_the_endpoint_to_become_ready() {
        let order = Rc::new(RefCell::new(Vec::new()));
        let endpoint = FakeEndpointAdapter::new(Ok(false), Rc::clone(&order));
        let crash_window = FakeCrashWindowAdapter::new(Ok(()), Rc::clone(&order));
        let task = FakeRuntimeTaskStartAdapter::new(Ok(()), Rc::clone(&order));
        let readiness = FakeRuntimeReadinessAdapter::new(Ok(()), Rc::clone(&order));

        assert_eq!(
            start_runtime_task(&endpoint, &crash_window, &task, &readiness),
            RuntimeTaskStartOutcome::Ready
        );
        assert_eq!(crash_window.calls.get(), 1);
        assert_eq!(task.run_calls.get(), 1);
        assert_eq!(readiness.calls.get(), 1);
        assert_eq!(
            readiness.bounded_wait.get(),
            Some(ATTACH_ENDPOINT_READINESS_WAIT)
        );
        assert_eq!(
            *order.borrow(),
            vec!["endpoint", "clear", "start", "readiness"]
        );
    }

    #[test]
    fn requested_start_reports_a_readiness_timeout() {
        let order = Rc::new(RefCell::new(Vec::new()));
        let endpoint = FakeEndpointAdapter::new(Ok(false), Rc::clone(&order));
        let crash_window = FakeCrashWindowAdapter::new(Ok(()), Rc::clone(&order));
        let task = FakeRuntimeTaskStartAdapter::new(Ok(()), Rc::clone(&order));
        let readiness = FakeRuntimeReadinessAdapter::new(
            Err(RuntimeReadinessError::TimedOut),
            Rc::clone(&order),
        );

        assert_eq!(
            start_runtime_task(&endpoint, &crash_window, &task, &readiness),
            RuntimeTaskStartOutcome::ReadinessTimedOut
        );
        assert_eq!(readiness.calls.get(), 1);
        assert_eq!(
            *order.borrow(),
            vec!["endpoint", "clear", "start", "readiness"]
        );
    }

    #[test]
    fn endpoint_check_failure_skips_the_crash_window_and_task() {
        let order = Rc::new(RefCell::new(Vec::new()));
        let endpoint = FakeEndpointAdapter::new(Err("denied".into()), Rc::clone(&order));
        let crash_window = FakeCrashWindowAdapter::new(Ok(()), Rc::clone(&order));
        let task = FakeRuntimeTaskStartAdapter::new(Ok(()), Rc::clone(&order));
        let readiness = FakeRuntimeReadinessAdapter::new(Ok(()), Rc::clone(&order));

        assert_eq!(
            start_runtime_task(&endpoint, &crash_window, &task, &readiness),
            RuntimeTaskStartOutcome::EndpointCheckFailed("denied".into())
        );
        assert_eq!(crash_window.calls.get(), 0);
        assert_eq!(task.calls.get(), 0);
        assert_eq!(readiness.calls.get(), 0);
        assert_eq!(*order.borrow(), vec!["endpoint"]);
    }

    #[test]
    fn crash_window_failure_skips_the_task_start() {
        let order = Rc::new(RefCell::new(Vec::new()));
        let endpoint = FakeEndpointAdapter::new(Ok(false), Rc::clone(&order));
        let crash_window = FakeCrashWindowAdapter::new(Err("denied".into()), Rc::clone(&order));
        let task = FakeRuntimeTaskStartAdapter::new(Ok(()), Rc::clone(&order));
        let readiness = FakeRuntimeReadinessAdapter::new(Ok(()), Rc::clone(&order));

        assert_eq!(
            start_runtime_task(&endpoint, &crash_window, &task, &readiness),
            RuntimeTaskStartOutcome::ClearFailed("denied".into())
        );
        assert_eq!(crash_window.calls.get(), 1);
        assert_eq!(task.run_calls.get(), 0);
        assert_eq!(readiness.calls.get(), 0);
        assert_eq!(*order.borrow(), vec!["endpoint", "clear"]);
    }

    #[test]
    fn task_start_failure_has_its_own_outcome() {
        let order = Rc::new(RefCell::new(Vec::new()));
        let endpoint = FakeEndpointAdapter::new(Ok(false), Rc::clone(&order));
        let crash_window = FakeCrashWindowAdapter::new(Ok(()), Rc::clone(&order));
        let task = FakeRuntimeTaskStartAdapter::new(
            Err(RuntimeTaskStartError::StartFailed(
                "Task start failed: RunTask HRESULT(0x80041326)".into(),
            )),
            Rc::clone(&order),
        );
        let readiness = FakeRuntimeReadinessAdapter::new(Ok(()), Rc::clone(&order));

        assert_eq!(
            start_runtime_task(&endpoint, &crash_window, &task, &readiness),
            RuntimeTaskStartOutcome::StartFailed(
                "Task start failed: RunTask HRESULT(0x80041326)".into()
            )
        );
        assert_eq!(crash_window.calls.get(), 1);
        assert_eq!(task.run_calls.get(), 1);
        assert_eq!(readiness.calls.get(), 0);
        assert_eq!(*order.borrow(), vec!["endpoint", "clear", "start"]);
    }

    #[test]
    fn queued_start_keeps_the_session_cause_and_skips_readiness() {
        let order = Rc::new(RefCell::new(Vec::new()));
        let endpoint = FakeEndpointAdapter::new(Ok(false), Rc::clone(&order));
        let crash_window = FakeCrashWindowAdapter::new(Ok(()), Rc::clone(&order));
        let task = FakeRuntimeTaskStartAdapter::new(
            Err(RuntimeTaskStartError::Queued { session_id: 7 }),
            Rc::clone(&order),
        );
        let readiness = FakeRuntimeReadinessAdapter::new(Ok(()), Rc::clone(&order));
        let diagnostics = FakeDiagnosticSink::default();

        let outcome = start_runtime_task_with_diagnostic(
            &endpoint,
            &crash_window,
            &task,
            &readiness,
            &diagnostics,
        );

        assert_eq!(outcome, RuntimeTaskStartOutcome::Queued { session_id: 7 });
        assert_eq!(
            outcome.cause().as_deref(),
            Some("Task Scheduler kept the runtime task in state Queued for session id 7.")
        );
        assert_eq!(readiness.calls.get(), 0);
        assert_eq!(*order.borrow(), vec!["endpoint", "clear", "start"]);
        assert_eq!(
            *diagnostics.events.borrow(),
            vec![DiagnosticEvent::RuntimeTaskStartFailed]
        );
        assert_eq!(
            *diagnostics.causes.borrow(),
            outcome.cause().into_iter().collect::<Vec<_>>()
        );
    }

    #[test]
    fn writes_a_diagnostic_only_for_failed_start_outcomes() {
        for (
            endpoint_result,
            clear_result,
            start_result,
            readiness_result,
            expected_outcome,
            expected_events,
        ) in [
            (
                Ok(true),
                Ok(()),
                Ok(()),
                Ok(()),
                RuntimeTaskStartOutcome::EndpointPresent,
                vec![],
            ),
            (
                Ok(false),
                Ok(()),
                Ok(()),
                Ok(()),
                RuntimeTaskStartOutcome::Ready,
                vec![],
            ),
            (
                Err("denied".into()),
                Ok(()),
                Ok(()),
                Ok(()),
                RuntimeTaskStartOutcome::EndpointCheckFailed("denied".into()),
                vec![DiagnosticEvent::RuntimeTaskStartFailed],
            ),
            (
                Ok(false),
                Err("denied".into()),
                Ok(()),
                Ok(()),
                RuntimeTaskStartOutcome::ClearFailed("denied".into()),
                vec![DiagnosticEvent::RuntimeTaskStartFailed],
            ),
            (
                Ok(false),
                Ok(()),
                Err(RuntimeTaskStartError::StartFailed(
                    "Task start failed: scheduler code".into(),
                )),
                Ok(()),
                RuntimeTaskStartOutcome::StartFailed("Task start failed: scheduler code".into()),
                vec![DiagnosticEvent::RuntimeTaskStartFailed],
            ),
            (
                Ok(false),
                Ok(()),
                Ok(()),
                Err(RuntimeReadinessError::TimedOut),
                RuntimeTaskStartOutcome::ReadinessTimedOut,
                vec![DiagnosticEvent::RuntimeTaskStartFailed],
            ),
            (
                Ok(false),
                Ok(()),
                Ok(()),
                Err(RuntimeReadinessError::Failed("denied".into())),
                RuntimeTaskStartOutcome::ReadinessFailed("denied".into()),
                vec![DiagnosticEvent::RuntimeTaskStartFailed],
            ),
        ] {
            let order = Rc::new(RefCell::new(Vec::new()));
            let endpoint = FakeEndpointAdapter::new(endpoint_result, Rc::clone(&order));
            let crash_window = FakeCrashWindowAdapter::new(clear_result, Rc::clone(&order));
            let task = FakeRuntimeTaskStartAdapter::new(start_result, Rc::clone(&order));
            let readiness = FakeRuntimeReadinessAdapter::new(readiness_result, order);
            let diagnostics = FakeDiagnosticSink::default();

            assert_eq!(
                start_runtime_task_with_diagnostic(
                    &endpoint,
                    &crash_window,
                    &task,
                    &readiness,
                    &diagnostics,
                ),
                expected_outcome
            );
            assert_eq!(*diagnostics.events.borrow(), expected_events);
            assert_eq!(
                *diagnostics.causes.borrow(),
                expected_outcome.cause().into_iter().collect::<Vec<_>>()
            );
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
                Err(RegistrationError::Failed(
                    "Task registration failed: scheduler code".into(),
                )),
                RuntimeTaskRegistrationOutcome::Failed(
                    "Task registration failed: scheduler code".into(),
                ),
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
                | RuntimeTaskRegistrationOutcome::Failed(_) => {
                    vec![DiagnosticEvent::RuntimeTaskRegistrationFailed]
                }
                _ => Vec::new(),
            };
            assert_eq!(*diagnostics.events.borrow(), expected_events);
            assert_eq!(
                *diagnostics.causes.borrow(),
                expected.cause().into_iter().collect::<Vec<_>>()
            );
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
            assert_eq!(
                *diagnostics.causes.borrow(),
                expected.cause().into_iter().collect::<Vec<_>>()
            );
        }
    }
}
