use std::fmt;
use std::io;
use std::path::Path;

#[cfg(target_os = "macos")]
use std::path::PathBuf;

#[cfg(target_os = "macos")]
const RUNTIME_AGENT_PLIST: &str = "ai.muniment.runtime.plist";
const MACOS_RUNTIME_LOG_MAX_BYTES: u64 = 256 * 1024;
const RUNTIME_SERVICE_NOT_FOUND_RECORD: &[u8] =
    b"event=runtime_service_not_found message=runtime service not found\n";
const RUNTIME_SERVICE_REGISTRATION_FAILED_RECORD: &[u8] =
    b"event=runtime_service_registration_failed message=runtime service registration failed\n";
const RUNTIME_SERVICE_START_FAILED_RECORD: &[u8] =
    b"event=runtime_service_start_failed message=runtime service start failed\n";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeStartOutcome {
    SkippedActivation,
    EndpointPresent,
    Requested,
    EndpointCheckFailed,
    RequestFailed,
}

impl fmt::Display for RuntimeStartOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::SkippedActivation => "skippedActivation",
            Self::EndpointPresent => "endpointPresent",
            Self::Requested => "requested",
            Self::EndpointCheckFailed => "endpointCheckFailed",
            Self::RequestFailed => "requestFailed",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum RuntimeServiceActivation {
    Enabled,
    RequiresApproval,
    NotFound,
    Failed,
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub(crate) fn runtime_service_activation(
    activation: tauri::State<'_, RuntimeServiceActivation>,
) -> RuntimeServiceActivation {
    *activation
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub(crate) fn open_login_items() {
    use objc2_service_management::SMAppService;

    // SAFETY: This class method only asks System Settings to show Login Items.
    unsafe { SMAppService::openSystemSettingsLoginItems() };
}

impl fmt::Display for RuntimeServiceActivation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Enabled => "enabled",
            Self::RequiresApproval => "requiresApproval",
            Self::NotFound => "notFound",
            Self::Failed => "failed",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ServiceStatus {
    NotRegistered,
    Enabled,
    RequiresApproval,
    NotFound,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RegistrationError {
    Denied,
    Failed,
}

trait RuntimeServiceAdapter {
    fn status(&self) -> Result<ServiceStatus, ()>;
    fn register(&self) -> Result<(), RegistrationError>;
}

trait RuntimeStartAdapter {
    fn endpoint_exists(&self) -> Result<bool, ()>;
    fn request_start(&self) -> Result<(), ()>;
}

fn request_enabled_runtime_start(
    activation: RuntimeServiceActivation,
    adapter: &impl RuntimeStartAdapter,
) -> RuntimeStartOutcome {
    if activation != RuntimeServiceActivation::Enabled {
        return RuntimeStartOutcome::SkippedActivation;
    }
    match adapter.endpoint_exists() {
        Ok(true) => RuntimeStartOutcome::EndpointPresent,
        Ok(false) => match adapter.request_start() {
            Ok(()) => RuntimeStartOutcome::Requested,
            Err(()) => RuntimeStartOutcome::RequestFailed,
        },
        Err(()) => RuntimeStartOutcome::EndpointCheckFailed,
    }
}

fn activate_runtime_service(adapter: &impl RuntimeServiceAdapter) -> RuntimeServiceActivation {
    match adapter.status() {
        Ok(ServiceStatus::NotRegistered) => {
            let registration = adapter.register();
            let rechecked_status = adapter.status();
            if registration == Err(RegistrationError::Denied) {
                return RuntimeServiceActivation::RequiresApproval;
            }
            if registration.is_err() {
                return RuntimeServiceActivation::Failed;
            }
            map_status(rechecked_status)
        }
        status => map_status(status),
    }
}

#[cfg(target_os = "macos")]
fn activate_runtime_service_at_startup(
    adapter: &impl RuntimeServiceAdapter,
) -> Option<RuntimeServiceActivation> {
    Some(activate_runtime_service(adapter))
}

#[cfg(not(target_os = "macos"))]
fn activate_runtime_service_at_startup(
    _adapter: &impl RuntimeServiceAdapter,
) -> Option<RuntimeServiceActivation> {
    None
}

fn map_status(status: Result<ServiceStatus, ()>) -> RuntimeServiceActivation {
    match status {
        Ok(ServiceStatus::Enabled) => RuntimeServiceActivation::Enabled,
        Ok(ServiceStatus::RequiresApproval) => RuntimeServiceActivation::RequiresApproval,
        Ok(ServiceStatus::NotFound) => RuntimeServiceActivation::NotFound,
        Ok(ServiceStatus::NotRegistered) | Err(()) => RuntimeServiceActivation::Failed,
    }
}

fn diagnostic_record(activation: RuntimeServiceActivation) -> Option<&'static [u8]> {
    match activation {
        RuntimeServiceActivation::NotFound => Some(RUNTIME_SERVICE_NOT_FOUND_RECORD),
        RuntimeServiceActivation::Failed => Some(RUNTIME_SERVICE_REGISTRATION_FAILED_RECORD),
        RuntimeServiceActivation::Enabled | RuntimeServiceActivation::RequiresApproval => None,
    }
}

fn write_activation_diagnostic(
    log_directory: &Path,
    activation: RuntimeServiceActivation,
) -> io::Result<()> {
    let Some(record) = diagnostic_record(activation) else {
        return Ok(());
    };
    muniment_core::user_diagnostics::append_owner_only_record(
        log_directory,
        c"runtime.log",
        MACOS_RUNTIME_LOG_MAX_BYTES,
        record,
    )
}

fn write_start_diagnostic(log_directory: &Path, outcome: RuntimeStartOutcome) -> io::Result<()> {
    if !matches!(
        outcome,
        RuntimeStartOutcome::EndpointCheckFailed | RuntimeStartOutcome::RequestFailed
    ) {
        return Ok(());
    }
    muniment_core::user_diagnostics::append_owner_only_record(
        log_directory,
        c"runtime.log",
        MACOS_RUNTIME_LOG_MAX_BYTES,
        RUNTIME_SERVICE_START_FAILED_RECORD,
    )
}

#[cfg(target_os = "macos")]
pub(crate) fn activate_bundled_runtime_service() -> RuntimeServiceActivation {
    let activation = activate_runtime_service_at_startup(&MacosRuntimeServiceAdapter::new())
        .expect("macOS startup activates the runtime service");
    if let Ok(home) = muniment_core::user_diagnostics::effective_user_home() {
        let log_directory = home.join("Library/Logs/Muniment");
        let _ = write_activation_diagnostic(&log_directory, activation);
        let start_adapter = MacosRuntimeStartAdapter::new(home);
        let outcome = request_enabled_runtime_start(activation, &start_adapter);
        let _ = write_start_diagnostic(&log_directory, outcome);
    }
    activation
}

#[cfg(target_os = "macos")]
struct MacosRuntimeStartAdapter {
    endpoint: PathBuf,
}

#[cfg(target_os = "macos")]
impl MacosRuntimeStartAdapter {
    fn new(home: PathBuf) -> Self {
        Self {
            endpoint: home.join("Library/Application Support/Muniment/runtime/attach-v1.sock"),
        }
    }
}

#[cfg(target_os = "macos")]
impl RuntimeStartAdapter for MacosRuntimeStartAdapter {
    fn endpoint_exists(&self) -> Result<bool, ()> {
        match std::fs::symlink_metadata(&self.endpoint) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(_) => Err(()),
        }
    }

    fn request_start(&self) -> Result<(), ()> {
        use std::process::{Command, Stdio};

        // SAFETY: geteuid reads the effective user ID without dereferencing memory.
        let effective_uid = unsafe { libc::geteuid() };
        let target = format!("gui/{effective_uid}/ai.muniment.runtime");
        Command::new("/bin/launchctl")
            .args(["kickstart", target.as_str()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|_| ())?
            .success()
            .then_some(())
            .ok_or(())
    }
}

#[cfg(target_os = "macos")]
struct MacosRuntimeServiceAdapter {
    service: objc2::rc::Retained<objc2_service_management::SMAppService>,
}

#[cfg(target_os = "macos")]
impl MacosRuntimeServiceAdapter {
    fn new() -> Self {
        use objc2_foundation::NSString;
        use objc2_service_management::SMAppService;

        let plist = NSString::from_str(RUNTIME_AGENT_PLIST);
        Self {
            // SAFETY: The plist name is a valid NSString and names a bundled LaunchAgent.
            service: unsafe { SMAppService::agentServiceWithPlistName(&plist) },
        }
    }
}

#[cfg(target_os = "macos")]
impl RuntimeServiceAdapter for MacosRuntimeServiceAdapter {
    fn status(&self) -> Result<ServiceStatus, ()> {
        use objc2_service_management::SMAppServiceStatus;

        // SAFETY: SMAppService accepts status queries from the application's main process.
        match unsafe { self.service.status() } {
            SMAppServiceStatus::NotRegistered => Ok(ServiceStatus::NotRegistered),
            SMAppServiceStatus::Enabled => Ok(ServiceStatus::Enabled),
            SMAppServiceStatus::RequiresApproval => Ok(ServiceStatus::RequiresApproval),
            SMAppServiceStatus::NotFound => Ok(ServiceStatus::NotFound),
            _ => Err(()),
        }
    }

    fn register(&self) -> Result<(), RegistrationError> {
        use objc2_service_management::kSMErrorLaunchDeniedByUser;

        // SAFETY: The service comes from agentServiceWithPlistName and remains retained.
        unsafe { self.service.registerAndReturnError() }.map_err(|error| {
            if error.code() == kSMErrorLaunchDeniedByUser as isize {
                RegistrationError::Denied
            } else {
                RegistrationError::Failed
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct FakeAdapter {
        statuses: RefCell<VecDeque<Result<ServiceStatus, ()>>>,
        registration: Result<(), RegistrationError>,
        status_calls: Cell<usize>,
        registration_calls: Cell<usize>,
    }

    struct FakeStartAdapter {
        endpoint: Result<bool, ()>,
        request: Result<(), ()>,
        endpoint_calls: Cell<usize>,
        request_calls: Cell<usize>,
    }

    impl FakeStartAdapter {
        fn new(endpoint: Result<bool, ()>, request: Result<(), ()>) -> Self {
            Self {
                endpoint,
                request,
                endpoint_calls: Cell::new(0),
                request_calls: Cell::new(0),
            }
        }
    }

    impl RuntimeStartAdapter for FakeStartAdapter {
        fn endpoint_exists(&self) -> Result<bool, ()> {
            self.endpoint_calls.set(self.endpoint_calls.get() + 1);
            self.endpoint
        }

        fn request_start(&self) -> Result<(), ()> {
            self.request_calls.set(self.request_calls.get() + 1);
            self.request
        }
    }

    impl FakeAdapter {
        fn new(
            statuses: impl IntoIterator<Item = Result<ServiceStatus, ()>>,
            registration: Result<(), RegistrationError>,
        ) -> Self {
            Self {
                statuses: RefCell::new(statuses.into_iter().collect()),
                registration,
                status_calls: Cell::new(0),
                registration_calls: Cell::new(0),
            }
        }
    }

    impl RuntimeServiceAdapter for FakeAdapter {
        fn status(&self) -> Result<ServiceStatus, ()> {
            self.status_calls.set(self.status_calls.get() + 1);
            self.statuses.borrow_mut().pop_front().unwrap()
        }

        fn register(&self) -> Result<(), RegistrationError> {
            self.registration_calls
                .set(self.registration_calls.get() + 1);
            self.registration
        }
    }

    fn directory() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "muniment-desktop-runtime-service-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn reports_each_existing_status_after_one_query() {
        for (status, expected) in [
            (ServiceStatus::Enabled, RuntimeServiceActivation::Enabled),
            (
                ServiceStatus::RequiresApproval,
                RuntimeServiceActivation::RequiresApproval,
            ),
            (ServiceStatus::NotFound, RuntimeServiceActivation::NotFound),
        ] {
            let adapter = FakeAdapter::new([Ok(status)], Ok(()));
            assert_eq!(activate_runtime_service(&adapter), expected);
            assert_eq!(adapter.status_calls.get(), 1);
            assert_eq!(adapter.registration_calls.get(), 0);
        }
    }

    #[test]
    fn skips_start_request_unless_activation_is_enabled() {
        for activation in [
            RuntimeServiceActivation::RequiresApproval,
            RuntimeServiceActivation::NotFound,
            RuntimeServiceActivation::Failed,
        ] {
            let adapter = FakeStartAdapter::new(Ok(false), Ok(()));
            assert_eq!(
                request_enabled_runtime_start(activation, &adapter),
                RuntimeStartOutcome::SkippedActivation
            );
            assert_eq!(adapter.endpoint_calls.get(), 0);
            assert_eq!(adapter.request_calls.get(), 0);
        }
    }

    #[test]
    fn skips_start_request_when_the_endpoint_exists() {
        let adapter = FakeStartAdapter::new(Ok(true), Ok(()));
        assert_eq!(
            request_enabled_runtime_start(RuntimeServiceActivation::Enabled, &adapter),
            RuntimeStartOutcome::EndpointPresent
        );
        assert_eq!(adapter.endpoint_calls.get(), 1);
        assert_eq!(adapter.request_calls.get(), 0);
    }

    #[test]
    fn requests_one_start_when_enabled_endpoint_is_absent() {
        let adapter = FakeStartAdapter::new(Ok(false), Ok(()));
        assert_eq!(
            request_enabled_runtime_start(RuntimeServiceActivation::Enabled, &adapter),
            RuntimeStartOutcome::Requested
        );
        assert_eq!(adapter.endpoint_calls.get(), 1);
        assert_eq!(adapter.request_calls.get(), 1);
    }

    #[test]
    fn returns_redacted_typed_start_failures() {
        let endpoint_failure = FakeStartAdapter::new(Err(()), Ok(()));
        assert_eq!(
            request_enabled_runtime_start(RuntimeServiceActivation::Enabled, &endpoint_failure),
            RuntimeStartOutcome::EndpointCheckFailed
        );
        assert_eq!(endpoint_failure.request_calls.get(), 0);

        let request_failure = FakeStartAdapter::new(Ok(false), Err(()));
        let outcome =
            request_enabled_runtime_start(RuntimeServiceActivation::Enabled, &request_failure);
        assert_eq!(outcome, RuntimeStartOutcome::RequestFailed);
        assert_eq!(outcome.to_string(), "requestFailed");
        assert_eq!(request_failure.request_calls.get(), 1);
    }

    #[test]
    fn serializes_each_activation_state_for_the_shell() {
        for (activation, expected) in [
            (RuntimeServiceActivation::Enabled, "\"enabled\""),
            (
                RuntimeServiceActivation::RequiresApproval,
                "\"requiresApproval\"",
            ),
            (RuntimeServiceActivation::NotFound, "\"notFound\""),
            (RuntimeServiceActivation::Failed, "\"failed\""),
        ] {
            assert_eq!(serde_json::to_string(&activation).unwrap(), expected);
        }
    }

    #[test]
    fn main_registers_runtime_service_commands() {
        let main = include_str!("main.rs");
        assert!(main.contains("macos_runtime_service::runtime_service_activation"));
        assert!(main.contains("macos_runtime_service::open_login_items"));
    }

    #[test]
    fn registers_not_registered_service_once_and_rechecks_once() {
        let adapter = FakeAdapter::new(
            [Ok(ServiceStatus::NotRegistered), Ok(ServiceStatus::Enabled)],
            Ok(()),
        );

        assert_eq!(
            activate_runtime_service(&adapter),
            RuntimeServiceActivation::Enabled
        );
        assert_eq!(adapter.status_calls.get(), 2);
        assert_eq!(adapter.registration_calls.get(), 1);
    }

    #[test]
    fn registration_denial_requires_approval_and_still_rechecks() {
        let adapter = FakeAdapter::new(
            [
                Ok(ServiceStatus::NotRegistered),
                Ok(ServiceStatus::RequiresApproval),
            ],
            Err(RegistrationError::Denied),
        );

        assert_eq!(
            activate_runtime_service(&adapter),
            RuntimeServiceActivation::RequiresApproval
        );
        assert_eq!(adapter.status_calls.get(), 2);
        assert_eq!(adapter.registration_calls.get(), 1);
    }

    #[test]
    fn status_and_registration_failures_are_redacted() {
        let status_failure = FakeAdapter::new([Err(())], Ok(()));
        assert_eq!(
            activate_runtime_service(&status_failure),
            RuntimeServiceActivation::Failed
        );

        let registration_failure = FakeAdapter::new(
            [
                Ok(ServiceStatus::NotRegistered),
                Ok(ServiceStatus::NotFound),
            ],
            Err(RegistrationError::Failed),
        );
        assert_eq!(
            activate_runtime_service(&registration_failure),
            RuntimeServiceActivation::Failed
        );
        assert_eq!(registration_failure.status_calls.get(), 2);
        assert_eq!(RuntimeServiceActivation::Failed.to_string(), "failed");
    }

    #[test]
    fn maps_only_failure_outcomes_to_exact_redacted_records() {
        assert_eq!(
            diagnostic_record(RuntimeServiceActivation::NotFound),
            Some(b"event=runtime_service_not_found message=runtime service not found\n".as_slice())
        );
        assert_eq!(
            diagnostic_record(RuntimeServiceActivation::Failed),
            Some(
                b"event=runtime_service_registration_failed message=runtime service registration failed\n"
                    .as_slice()
            )
        );
        assert_eq!(diagnostic_record(RuntimeServiceActivation::Enabled), None);
        assert_eq!(
            diagnostic_record(RuntimeServiceActivation::RequiresApproval),
            None
        );
    }

    #[test]
    fn writes_only_failure_outcomes_to_the_bounded_runtime_log() {
        let root = directory();
        let logs = root.join("Library/Logs/Muniment");
        fs::create_dir_all(root.join("Library/Logs")).unwrap();
        fs::set_permissions(root.join("Library/Logs"), fs::Permissions::from_mode(0o700)).unwrap();

        write_activation_diagnostic(&logs, RuntimeServiceActivation::Enabled).unwrap();
        write_activation_diagnostic(&logs, RuntimeServiceActivation::RequiresApproval).unwrap();
        assert!(!logs.exists());

        write_activation_diagnostic(&logs, RuntimeServiceActivation::NotFound).unwrap();
        write_activation_diagnostic(&logs, RuntimeServiceActivation::Failed).unwrap();
        write_start_diagnostic(&logs, RuntimeStartOutcome::Requested).unwrap();
        write_start_diagnostic(&logs, RuntimeStartOutcome::RequestFailed).unwrap();
        assert_eq!(
            fs::read(logs.join("runtime.log")).unwrap(),
            [
                RUNTIME_SERVICE_NOT_FOUND_RECORD,
                RUNTIME_SERVICE_REGISTRATION_FAILED_RECORD,
                RUNTIME_SERVICE_START_FAILED_RECORD,
            ]
            .concat()
        );
        assert_eq!(
            fs::metadata(logs.join("runtime.log"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        fs::write(
            logs.join("runtime.log"),
            vec![b'x'; MACOS_RUNTIME_LOG_MAX_BYTES as usize],
        )
        .unwrap();
        write_activation_diagnostic(&logs, RuntimeServiceActivation::NotFound).unwrap();
        assert_eq!(
            fs::read(logs.join("runtime.log")).unwrap(),
            RUNTIME_SERVICE_NOT_FOUND_RECORD
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn not_registered_after_registration_is_a_failure() {
        let adapter = FakeAdapter::new(
            [
                Ok(ServiceStatus::NotRegistered),
                Ok(ServiceStatus::NotRegistered),
            ],
            Ok(()),
        );

        assert_eq!(
            activate_runtime_service(&adapter),
            RuntimeServiceActivation::Failed
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn non_macos_startup_does_not_query_or_register() {
        let adapter = FakeAdapter::new([], Ok(()));

        assert_eq!(activate_runtime_service_at_startup(&adapter), None);
        assert_eq!(adapter.status_calls.get(), 0);
        assert_eq!(adapter.registration_calls.get(), 0);
    }
}
