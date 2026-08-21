use std::fmt;

#[cfg(target_os = "macos")]
const RUNTIME_AGENT_PLIST: &str = "ai.muniment.runtime.plist";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeServiceActivation {
    Enabled,
    RequiresApproval,
    NotFound,
    Failed,
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

#[cfg(target_os = "macos")]
pub(crate) fn activate_bundled_runtime_service() -> RuntimeServiceActivation {
    activate_runtime_service_at_startup(&MacosRuntimeServiceAdapter::new())
        .expect("macOS startup activates the runtime service")
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
            if error.code() == i64::from(kSMErrorLaunchDeniedByUser) {
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

    struct FakeAdapter {
        statuses: RefCell<VecDeque<Result<ServiceStatus, ()>>>,
        registration: Result<(), RegistrationError>,
        status_calls: Cell<usize>,
        registration_calls: Cell<usize>,
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
