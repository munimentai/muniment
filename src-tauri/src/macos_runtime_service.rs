use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
const RUNTIME_AGENT_PLIST: &str = "ai.muniment.runtime.plist";
const MACOS_RUNTIME_LOG_MAX_BYTES: u64 = 256 * 1024;
const RUNTIME_SERVICE_NOT_FOUND_RECORD: &[u8] =
    b"event=runtime_service_not_found message=runtime service not found\n";
const RUNTIME_SERVICE_REGISTRATION_FAILED_RECORD: &[u8] =
    b"event=runtime_service_registration_failed message=runtime service registration failed\n";
const RUNTIME_SERVICE_START_FAILED_RECORD: &[u8] =
    b"event=runtime_service_start_failed message=runtime service start failed\n";

/// What the start path did after a kickstart of an enabled service failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StartRecovery {
    None,
    Reregistered,
    Failed,
}

impl fmt::Display for StartRecovery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::None => "none",
            Self::Reregistered => "reregistered",
            Self::Failed => "failed",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeStartOutcome {
    SkippedActivation,
    EndpointPresent,
    Requested,
    EndpointCheckFailed,
    RequestFailed,
    /// launchd accepted the start request and never kept the job running.
    JobInactive,
}

impl fmt::Display for RuntimeStartOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::SkippedActivation => "skippedActivation",
            Self::EndpointPresent => "endpointPresent",
            Self::Requested => "requested",
            Self::JobInactive => "jobInactive",
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
    fn unregister(&self) -> Result<(), ()> {
        Err(())
    }
    fn registration_diagnostic_written(&self) -> bool {
        false
    }
}

trait RuntimeStartAdapter {
    fn endpoint_exists(&self) -> Result<bool, ()>;
    fn request_start(&self) -> Result<(), ()>;
    /// Whether launchd holds the job running after a start request. An adapter
    /// that cannot tell answers Err, and the request stands.
    fn job_active(&self) -> Result<bool, ()> {
        Ok(true)
    }
    /// The text of the last failed start request, for the diagnostic record.
    fn request_failure(&self) -> Option<String> {
        None
    }
}

// SMAppService can report Enabled while launchd holds no job, for example
// after the bundle changed under the same path. A failed kickstart then means
// the registration must be made again before a second start request.
fn start_enabled_runtime_with_recovery(
    service: &impl RuntimeServiceAdapter,
    start: &impl RuntimeStartAdapter,
) -> (RuntimeStartOutcome, StartRecovery) {
    let outcome = request_enabled_runtime_start(RuntimeServiceActivation::Enabled, start);
    if outcome != RuntimeStartOutcome::RequestFailed {
        return (outcome, StartRecovery::None);
    }
    if service.unregister().is_err()
        || service.register().is_err()
        || service.status() != Ok(ServiceStatus::Enabled)
    {
        return (outcome, StartRecovery::Failed);
    }
    (
        request_enabled_runtime_start(RuntimeServiceActivation::Enabled, start),
        StartRecovery::Reregistered,
    )
}

// A registered job that launchd cannot keep up, a launch constraint or a
// configuration exit, answers the start request and never serves the endpoint.
// The desktop then runs the bundled runtime as its child instead of waiting.
fn confirm_requested_start(
    outcome: RuntimeStartOutcome,
    adapter: &impl RuntimeStartAdapter,
) -> RuntimeStartOutcome {
    if outcome == RuntimeStartOutcome::Requested && adapter.job_active() == Ok(false) {
        return RuntimeStartOutcome::JobInactive;
    }
    outcome
}

/// The service's own state line from `launchctl print`, the one at a single
/// tab of indentation. Nested endpoint states sit deeper and are not the job.
fn launchctl_job_state(output: &str) -> Option<&str> {
    output.lines().find_map(|line| {
        line.strip_prefix("\tstate = ")
            .filter(|state| !state.starts_with('\t'))
    })
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
        Ok(ServiceStatus::NotRegistered | ServiceStatus::NotFound) => {
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

fn activate_and_diagnose_runtime_service(
    adapter: &impl RuntimeServiceAdapter,
    log_directory: Option<&Path>,
) -> RuntimeServiceActivation {
    let activation = activate_runtime_service(adapter);
    // A second failure record can rotate the log and erase the native error.
    if !adapter.registration_diagnostic_written() {
        if let Some(log_directory) = log_directory {
            let _ = write_activation_diagnostic(log_directory, activation);
        }
    }
    activation
}

#[cfg(target_os = "macos")]
fn activate_runtime_service_at_startup(
    adapter: &impl RuntimeServiceAdapter,
    log_directory: Option<&Path>,
) -> Option<RuntimeServiceActivation> {
    Some(activate_and_diagnose_runtime_service(
        adapter,
        log_directory,
    ))
}

#[cfg(not(target_os = "macos"))]
fn activate_runtime_service_at_startup(
    _adapter: &impl RuntimeServiceAdapter,
    _log_directory: Option<&Path>,
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

fn write_registration_diagnostic(
    log_directory: &Path,
    domain: &str,
    code: isize,
    description: &str,
) -> io::Result<()> {
    // Bound each field before JSON escaping to keep the record below the log cap.
    let bounded_domain: String = domain.chars().take(4096).collect();
    let bounded_description: String = description.chars().take(4096).collect();
    let truncated =
        bounded_domain.len() != domain.len() || bounded_description.len() != description.len();
    // JSON strings keep native error text on one line without dumping userInfo.
    let record = format!(
        "event=runtime_service_registration_failed domain={} code={code} truncated={truncated} description={}\n",
        serde_json::to_string(&bounded_domain)?,
        serde_json::to_string(&bounded_description)?,
    );
    muniment_core::user_diagnostics::append_owner_only_record(
        log_directory,
        c"runtime.log",
        MACOS_RUNTIME_LOG_MAX_BYTES,
        record.as_bytes(),
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

fn write_start_detail(
    log_directory: &Path,
    outcome: RuntimeStartOutcome,
    recovery: StartRecovery,
    failure: Option<&str>,
) -> io::Result<()> {
    if recovery == StartRecovery::None
        && !matches!(
            outcome,
            RuntimeStartOutcome::EndpointCheckFailed | RuntimeStartOutcome::RequestFailed
        )
    {
        return Ok(());
    }
    let bounded: String = failure.unwrap_or("").chars().take(4096).collect();
    let record = format!(
        "event=runtime_service_start_detail outcome={outcome} recovery={recovery} request_error={}\n",
        serde_json::to_string(&bounded)?
    );
    muniment_core::user_diagnostics::append_owner_only_record(
        log_directory,
        c"runtime.log",
        MACOS_RUNTIME_LOG_MAX_BYTES,
        record.as_bytes(),
    )
}

#[cfg(target_os = "macos")]
pub(crate) fn start() -> crate::runtime_owner::RuntimeEvent {
    use crate::runtime_owner::RuntimeEvent;
    let log_directory = muniment_core::user_diagnostics::effective_user_home()
        .ok()
        .map(|home| home.join("Library/Logs/Muniment"));
    let activation = activate_runtime_service_at_startup(
        &MacosRuntimeServiceAdapter::new(),
        log_directory.as_deref(),
    )
    .expect("macOS startup activates the runtime service");
    match activation {
        RuntimeServiceActivation::RequiresApproval => return RuntimeEvent::RequiresApproval,
        RuntimeServiceActivation::NotFound => return RuntimeEvent::NotFound,
        RuntimeServiceActivation::Failed => return RuntimeEvent::RegistrationFailed,
        RuntimeServiceActivation::Enabled => {}
    }
    // Kickstart is idempotent without -k. A stale socket must not block a retry.
    let (outcome, recovery, failure) = match muniment_runtime::profile_directory() {
        Ok(directory) => {
            let mut adapter = MacosRuntimeStartAdapter::new(directory);
            adapter.force_request = true;
            let (outcome, recovery) =
                start_enabled_runtime_with_recovery(&MacosRuntimeServiceAdapter::new(), &adapter);
            let outcome = confirm_requested_start(outcome, &adapter);
            (outcome, recovery, adapter.request_failure())
        }
        Err(_) => (
            RuntimeStartOutcome::RequestFailed,
            StartRecovery::None,
            Some("The runtime profile directory is unavailable.".to_owned()),
        ),
    };
    if let Some(directory) = &log_directory {
        let _ = write_start_diagnostic(directory, outcome);
        let _ = write_start_detail(directory, outcome, recovery, failure.as_deref());
    }
    // launchd would keep respawning an inactive job. Unregister it, so the
    // child runtime runs alone until the next start registers again.
    if matches!(
        outcome,
        RuntimeStartOutcome::JobInactive | RuntimeStartOutcome::RequestFailed
    ) {
        let unregistered = MacosRuntimeServiceAdapter::new().unregister().is_ok();
        if let Some(directory) = &log_directory {
            let _ = write_child_diagnostic(
                directory,
                &format!("event=runtime_service_job_inactive unregistered={unregistered}\n"),
            );
        }
    }
    match outcome {
        RuntimeStartOutcome::Requested => RuntimeEvent::Starting,
        RuntimeStartOutcome::JobInactive | RuntimeStartOutcome::RequestFailed => {
            RuntimeEvent::ServiceInactive
        }
        _ => RuntimeEvent::StartFailed,
    }
}

/// The runtime binary the bundle carries beside the desktop executable.
fn bundled_runtime_executable(desktop_executable: &Path) -> Option<PathBuf> {
    let contents = desktop_executable.parent()?.parent()?;
    Some(contents.join("Library/LaunchServices/muniment-runtime"))
}

fn write_child_diagnostic(log_directory: &Path, record: &str) -> io::Result<()> {
    muniment_core::user_diagnostics::append_owner_only_record(
        log_directory,
        c"runtime.log",
        MACOS_RUNTIME_LOG_MAX_BYTES,
        record.as_bytes(),
    )
}

// A build whose code signature the service cannot verify never registers. The
// desktop then runs the bundled runtime as its own child, stops it when the
// desktop exits, and the shell says so.
#[cfg(target_os = "macos")]
pub(crate) fn start_child(
    owner: &crate::runtime_owner::RuntimeOwner,
) -> crate::runtime_owner::RuntimeEvent {
    use crate::runtime_owner::RuntimeEvent;
    use std::process::{Command, Stdio};
    if owner.child_running() {
        return RuntimeEvent::ChildStarted;
    }
    // A runtime that another agent runs already listens. The owner's endpoint watch
    // connects to it, and a stale socket with no listener falls through to a child.
    if muniment_runtime::profile_directory().is_ok_and(|profile| {
        std::os::unix::net::UnixStream::connect(profile.join("muniment/attach-v1.sock")).is_ok()
    }) {
        return RuntimeEvent::Starting;
    }
    let executable = std::env::current_exe()
        .ok()
        .and_then(|desktop| bundled_runtime_executable(&desktop))
        .filter(|path| path.is_file());
    let Some(executable) = executable else {
        return RuntimeEvent::RegistrationFailed;
    };
    let log_directory = muniment_core::user_diagnostics::effective_user_home()
        .ok()
        .map(|home| home.join("Library/Logs/Muniment"));
    let spawned = Command::new(&executable)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn();
    let record = match &spawned {
        Ok(child) => format!(
            "event=runtime_service_child_started pid={} executable={}\n",
            child.id(),
            serde_json::to_string(&executable.display().to_string()).unwrap_or_default()
        ),
        Err(error) => format!(
            "event=runtime_service_child_start_failed error={}\n",
            serde_json::to_string(&error.to_string()).unwrap_or_default()
        ),
    };
    if let Some(directory) = log_directory {
        let _ = write_child_diagnostic(&directory, &record);
    }
    match spawned {
        Ok(child) => {
            owner.keep_child(child);
            RuntimeEvent::ChildStarted
        }
        Err(_) => RuntimeEvent::RegistrationFailed,
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn requires_approval() -> Option<bool> {
    match MacosRuntimeServiceAdapter::new().status() {
        Ok(ServiceStatus::RequiresApproval) => Some(true),
        Ok(ServiceStatus::Enabled) => Some(false),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn stop() -> Result<(), ()> {
    let adapter = MacosRuntimeServiceAdapter::new();
    // SAFETY: The adapter retains the bundled service for the duration of this call.
    unsafe { adapter.service.unregisterAndReturnError() }.map_err(|_| ())
}

// launchctl can wait indefinitely when a registered job cannot spawn.
fn wait_for_start_command(
    mut child: std::process::Child,
    timeout: std::time::Duration,
) -> io::Result<std::process::Output> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output(),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            outcome => {
                let _ = child.kill();
                let _ = child.wait();
                return match outcome {
                    Err(error) => Err(error),
                    _ => Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "The runtime start request timed out.",
                    )),
                };
            }
        }
    }
}

// A socket file can remain after a killed runtime. Only a listener proves activity.
fn runtime_endpoint_accepts_connections(endpoint: &Path) -> bool {
    std::os::unix::net::UnixStream::connect(endpoint).is_ok()
}

struct MacosRuntimeStartAdapter {
    endpoint: PathBuf,
    force_request: bool,
    failure: std::cell::RefCell<Option<String>>,
}

impl MacosRuntimeStartAdapter {
    fn new(profile_directory: PathBuf) -> Self {
        Self {
            endpoint: profile_directory.join("muniment/attach-v1.sock"),
            force_request: false,
            failure: std::cell::RefCell::new(None),
        }
    }
}

#[cfg(target_os = "macos")]
impl RuntimeStartAdapter for MacosRuntimeStartAdapter {
    fn endpoint_exists(&self) -> Result<bool, ()> {
        if self.force_request {
            return Ok(false);
        }
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
        let child = Command::new("/bin/launchctl")
            .args(["kickstart", target.as_str()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                *self.failure.borrow_mut() = Some(format!("launchctl did not run: {error}"));
            })?;
        let output =
            wait_for_start_command(child, std::time::Duration::from_secs(5)).map_err(|error| {
                *self.failure.borrow_mut() = Some(format!("launchctl kickstart {target}: {error}"));
            })?;
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        *self.failure.borrow_mut() = Some(format!(
            "launchctl kickstart {target} exit={:?} stderr={}",
            output.status.code(),
            stderr.trim()
        ));
        Err(())
    }

    fn request_failure(&self) -> Option<String> {
        self.failure.borrow().clone()
    }

    fn job_active(&self) -> Result<bool, ()> {
        use std::process::{Command, Stdio};

        // The endpoint is the proof of a running job. Wait two seconds for it.
        for _ in 0..10 {
            std::thread::sleep(std::time::Duration::from_millis(200));
            if runtime_endpoint_accepts_connections(&self.endpoint) {
                return Ok(true);
            }
        }
        // SAFETY: geteuid reads the effective user ID without dereferencing memory.
        let effective_uid = unsafe { libc::geteuid() };
        let target = format!("gui/{effective_uid}/ai.muniment.runtime");
        let output = Command::new("/bin/launchctl")
            .args(["print", target.as_str()])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .map_err(|_| ())?;
        if !output.status.success() {
            return Err(());
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let Some(state) = launchctl_job_state(&stdout) else {
            return Err(());
        };
        if state == "running" {
            return Ok(true);
        }
        let exit = stdout
            .lines()
            .find_map(|line| line.trim().strip_prefix("last exit code = "))
            .unwrap_or("none");
        *self.failure.borrow_mut() = Some(format!(
            "launchctl print {target} state={state} last_exit_code={exit}"
        ));
        Ok(false)
    }
}

#[cfg(target_os = "macos")]
struct MacosRuntimeServiceAdapter {
    service: objc2::rc::Retained<objc2_service_management::SMAppService>,
    registration_diagnostic_written: std::cell::Cell<bool>,
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
            registration_diagnostic_written: std::cell::Cell::new(false),
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

        self.registration_diagnostic_written.set(false);
        // SAFETY: The service comes from agentServiceWithPlistName and remains retained.
        unsafe { self.service.registerAndReturnError() }.map_err(|error| {
            if let Ok(home) = muniment_core::user_diagnostics::effective_user_home() {
                self.registration_diagnostic_written.set(
                    write_registration_diagnostic(
                        &home.join("Library/Logs/Muniment"),
                        &error.domain().to_string(),
                        error.code(),
                        &error.localizedDescription().to_string(),
                    )
                    .is_ok(),
                );
            }
            if error.code() == kSMErrorLaunchDeniedByUser as isize {
                RegistrationError::Denied
            } else {
                RegistrationError::Failed
            }
        })
    }

    fn unregister(&self) -> Result<(), ()> {
        // SAFETY: The service comes from agentServiceWithPlistName and remains retained.
        unsafe { self.service.unregisterAndReturnError() }.map_err(|_| ())
    }

    fn registration_diagnostic_written(&self) -> bool {
        self.registration_diagnostic_written.get()
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

    #[test]
    fn start_command_timeout_reaps_the_child() {
        let child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id();
        let started = std::time::Instant::now();
        let error =
            wait_for_start_command(child, std::time::Duration::from_millis(40)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        // SAFETY: signal zero checks this test's child without sending a signal.
        assert_eq!(unsafe { libc::kill(pid as libc::pid_t, 0) }, -1);
    }

    #[test]
    fn start_command_preserves_success_and_failure() {
        for (program, success) in [("/usr/bin/true", true), ("/usr/bin/false", false)] {
            let child = std::process::Command::new(program).spawn().unwrap();
            let output = wait_for_start_command(child, std::time::Duration::from_secs(2)).unwrap();
            assert_eq!(output.status.success(), success);
        }
    }

    #[test]
    fn runtime_activity_requires_a_listener_not_a_stale_socket() {
        use std::os::unix::net::UnixListener;
        let directory = directory();
        fs::create_dir_all(&directory).unwrap();
        let endpoint = directory.join("runtime.sock");
        assert!(!runtime_endpoint_accepts_connections(&endpoint));
        let listener = UnixListener::bind(&endpoint).unwrap();
        assert!(runtime_endpoint_accepts_connections(&endpoint));
        drop(listener);
        assert!(endpoint.exists());
        assert!(!runtime_endpoint_accepts_connections(&endpoint));
        fs::remove_dir_all(directory).unwrap();
    }

    struct FakeAdapter {
        statuses: RefCell<VecDeque<Result<ServiceStatus, ()>>>,
        registration: Result<(), RegistrationError>,
        status_calls: Cell<usize>,
        registration_calls: Cell<usize>,
        unregister_calls: Cell<usize>,
        diagnostic_directory: Option<PathBuf>,
        diagnostic_written: Cell<bool>,
    }

    struct FakeStartAdapter {
        endpoint: Result<bool, ()>,
        requests: RefCell<VecDeque<Result<(), ()>>>,
        endpoint_calls: Cell<usize>,
        request_calls: Cell<usize>,
    }

    /// A start adapter whose launchd job answers one fixed activity reading.
    struct InactiveJobAdapter(Result<bool, ()>);

    impl RuntimeStartAdapter for InactiveJobAdapter {
        fn endpoint_exists(&self) -> Result<bool, ()> {
            Ok(false)
        }

        fn request_start(&self) -> Result<(), ()> {
            Ok(())
        }

        fn job_active(&self) -> Result<bool, ()> {
            self.0
        }
    }

    #[test]
    fn a_requested_start_whose_job_never_runs_is_inactive() {
        assert_eq!(
            confirm_requested_start(
                RuntimeStartOutcome::Requested,
                &InactiveJobAdapter(Ok(false))
            ),
            RuntimeStartOutcome::JobInactive
        );
        assert_eq!(
            confirm_requested_start(
                RuntimeStartOutcome::Requested,
                &InactiveJobAdapter(Ok(true))
            ),
            RuntimeStartOutcome::Requested
        );
        // An unreadable job leaves the request standing.
        assert_eq!(
            confirm_requested_start(RuntimeStartOutcome::Requested, &InactiveJobAdapter(Err(()))),
            RuntimeStartOutcome::Requested
        );
        // Only a request is confirmed; a present endpoint or a failure stays as it is.
        assert_eq!(
            confirm_requested_start(
                RuntimeStartOutcome::EndpointPresent,
                &InactiveJobAdapter(Ok(false))
            ),
            RuntimeStartOutcome::EndpointPresent
        );
        assert_eq!(
            confirm_requested_start(
                RuntimeStartOutcome::RequestFailed,
                &InactiveJobAdapter(Ok(false))
            ),
            RuntimeStartOutcome::RequestFailed
        );
        assert_eq!(RuntimeStartOutcome::JobInactive.to_string(), "jobInactive");
    }

    #[test]
    fn launchctl_job_state_reads_the_service_line_and_not_the_nested_endpoints() {
        let output = "gui/501/ai.muniment.runtime = {\n\tactive count = 0\n\tpath = /Users/a/Applications/muniment.app/Contents/Library/LaunchAgents/ai.muniment.runtime.plist\n\tstate = spawn scheduled\n\tprogram identifier = Contents/Library/LaunchServices/muniment-runtime (mode: 2)\n\tlast exit code = 78: EX_CONFIG\n\tendpoints = {\n\t\t\"ai.muniment.runtime\" = {\n\t\t\tstate = active\n\t\t}\n\t}\n}\n";
        assert_eq!(launchctl_job_state(output), Some("spawn scheduled"));
        assert_eq!(
            launchctl_job_state(
                "gui/501/ai.muniment.runtime = {\n\tstate = running\n\t\tstate = active\n}\n"
            ),
            Some("running")
        );
        assert_eq!(launchctl_job_state("Could not find service\n"), None);
    }

    impl FakeStartAdapter {
        fn new(endpoint: Result<bool, ()>, request: Result<(), ()>) -> Self {
            Self::with_requests(endpoint, [request])
        }

        /// Each start request answers the next result; the last one repeats.
        fn with_requests(
            endpoint: Result<bool, ()>,
            requests: impl IntoIterator<Item = Result<(), ()>>,
        ) -> Self {
            Self {
                endpoint,
                requests: RefCell::new(requests.into_iter().collect()),
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
            let mut requests = self.requests.borrow_mut();
            if requests.len() > 1 {
                requests.pop_front().unwrap()
            } else {
                *requests.front().unwrap()
            }
        }

        fn request_failure(&self) -> Option<String> {
            Some("launchctl kickstart gui/501/ai.muniment.runtime exit=Some(113) stderr=Could not find service".into())
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
                unregister_calls: Cell::new(0),
                diagnostic_directory: None,
                diagnostic_written: Cell::new(false),
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
            self.diagnostic_written.set(false);
            if self.registration.is_err() {
                if let Some(directory) = &self.diagnostic_directory {
                    self.diagnostic_written.set(
                        write_registration_diagnostic(
                            directory,
                            "SMAppServiceErrorDomain",
                            -108,
                            "The plist is invalid.",
                        )
                        .is_ok(),
                    );
                }
            }
            self.registration
        }

        fn unregister(&self) -> Result<(), ()> {
            self.unregister_calls.set(self.unregister_calls.get() + 1);
            Ok(())
        }

        fn registration_diagnostic_written(&self) -> bool {
            self.diagnostic_written.get()
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
    fn the_bundled_runtime_sits_beside_the_desktop_in_the_bundle() {
        assert_eq!(
            bundled_runtime_executable(Path::new(
                "/Users/me/Applications/muniment.app/Contents/MacOS/muniment-desktop"
            )),
            Some(PathBuf::from(
                "/Users/me/Applications/muniment.app/Contents/Library/LaunchServices/muniment-runtime"
            ))
        );
        assert_eq!(
            bundled_runtime_executable(Path::new("muniment-desktop")),
            None
        );
    }

    #[test]
    fn a_child_start_record_names_the_event_and_the_executable() {
        let directory = directory();
        write_child_diagnostic(
            &directory,
            "event=runtime_service_child_started pid=7 executable=\"/a b/muniment-runtime\"\n",
        )
        .unwrap();
        let log = fs::read_to_string(directory.join("runtime.log")).unwrap();
        assert!(log.starts_with("event=runtime_service_child_started pid=7 executable="));
        assert!(log.ends_with("\n"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn reports_each_existing_status_after_one_query() {
        for (status, expected) in [
            (ServiceStatus::Enabled, RuntimeServiceActivation::Enabled),
            (
                ServiceStatus::RequiresApproval,
                RuntimeServiceActivation::RequiresApproval,
            ),
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
    fn start_adapter_checks_the_canonical_profile_endpoint() {
        let profile_directory = PathBuf::from("/profiles/current");
        let adapter = MacosRuntimeStartAdapter::new(profile_directory);
        assert!(!adapter.force_request);

        assert_eq!(
            adapter.endpoint,
            PathBuf::from("/profiles/current/muniment/attach-v1.sock")
        );
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
    fn reregisters_an_enabled_service_whose_kickstart_fails_and_starts_once_more() {
        let service = FakeAdapter::new([Ok(ServiceStatus::Enabled)], Ok(()));
        let start = FakeStartAdapter::with_requests(Ok(false), [Err(()), Ok(())]);
        assert_eq!(
            start_enabled_runtime_with_recovery(&service, &start),
            (RuntimeStartOutcome::Requested, StartRecovery::Reregistered)
        );
        assert_eq!(service.unregister_calls.get(), 1);
        assert_eq!(service.registration_calls.get(), 1);
        assert_eq!(service.status_calls.get(), 1);
        assert_eq!(start.request_calls.get(), 2);
    }

    #[test]
    fn keeps_the_request_failure_when_the_registration_does_not_recover() {
        for (statuses, registration) in [
            (
                vec![Ok(ServiceStatus::Enabled)],
                Err(RegistrationError::Failed),
            ),
            (vec![Ok(ServiceStatus::NotRegistered)], Ok(())),
            (vec![Err(())], Ok(())),
        ] {
            let service = FakeAdapter::new(statuses, registration);
            let start = FakeStartAdapter::with_requests(Ok(false), [Err(()), Ok(())]);
            assert_eq!(
                start_enabled_runtime_with_recovery(&service, &start),
                (RuntimeStartOutcome::RequestFailed, StartRecovery::Failed)
            );
            assert_eq!(service.unregister_calls.get(), 1);
            assert_eq!(start.request_calls.get(), 1);
        }
        let service = FakeAdapter::new([Ok(ServiceStatus::Enabled)], Ok(()));
        let start = FakeStartAdapter::new(Ok(false), Ok(()));
        assert_eq!(
            start_enabled_runtime_with_recovery(&service, &start),
            (RuntimeStartOutcome::Requested, StartRecovery::None)
        );
        assert_eq!(service.unregister_calls.get(), 0);
    }

    #[test]
    fn start_detail_names_the_outcome_recovery_and_launchctl_text() {
        let root = directory();
        let logs = root.join("Library/Logs/Muniment");
        fs::create_dir_all(root.join("Library/Logs")).unwrap();
        fs::set_permissions(root.join("Library/Logs"), fs::Permissions::from_mode(0o700)).unwrap();
        write_start_detail(
            &logs,
            RuntimeStartOutcome::Requested,
            StartRecovery::None,
            None,
        )
        .unwrap();
        assert!(!logs.exists());
        let start = FakeStartAdapter::new(Ok(false), Err(()));
        write_start_detail(
            &logs,
            RuntimeStartOutcome::RequestFailed,
            StartRecovery::Failed,
            start.request_failure().as_deref(),
        )
        .unwrap();
        write_start_detail(
            &logs,
            RuntimeStartOutcome::Requested,
            StartRecovery::Reregistered,
            None,
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(logs.join("runtime.log")).unwrap(),
            "event=runtime_service_start_detail outcome=requestFailed recovery=failed request_error=\"launchctl kickstart gui/501/ai.muniment.runtime exit=Some(113) stderr=Could not find service\"\n\
             event=runtime_service_start_detail outcome=requested recovery=reregistered request_error=\"\"\n"
        );
        fs::remove_dir_all(root).unwrap();
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
        let main = include_str!("desktop.rs");
        assert!(main.contains("runtime_owner::runtime_state"));
        assert!(main.contains("runtime_owner::runtime_start"));
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
    fn registers_not_found_once_and_maps_the_rechecked_status() {
        for (status, expected) in [
            (
                Ok(ServiceStatus::Enabled),
                RuntimeServiceActivation::Enabled,
            ),
            (
                Ok(ServiceStatus::RequiresApproval),
                RuntimeServiceActivation::RequiresApproval,
            ),
            (
                Ok(ServiceStatus::NotFound),
                RuntimeServiceActivation::NotFound,
            ),
            (
                Ok(ServiceStatus::NotRegistered),
                RuntimeServiceActivation::Failed,
            ),
            (Err(()), RuntimeServiceActivation::Failed),
        ] {
            let adapter = FakeAdapter::new([Ok(ServiceStatus::NotFound), status], Ok(()));
            assert_eq!(activate_runtime_service(&adapter), expected);
            assert_eq!(adapter.registration_calls.get(), 1);
            assert_eq!(adapter.status_calls.get(), 2);
        }
    }

    #[test]
    fn not_found_registration_errors_override_the_rechecked_status() {
        for (error, expected) in [
            (
                RegistrationError::Denied,
                RuntimeServiceActivation::RequiresApproval,
            ),
            (RegistrationError::Failed, RuntimeServiceActivation::Failed),
        ] {
            for status in [
                Ok(ServiceStatus::Enabled),
                Ok(ServiceStatus::NotFound),
                Err(()),
            ] {
                let adapter = FakeAdapter::new([Ok(ServiceStatus::NotFound), status], Err(error));
                assert_eq!(activate_runtime_service(&adapter), expected);
                assert_eq!(adapter.registration_calls.get(), 1);
                assert_eq!(adapter.status_calls.get(), 2);
            }
        }
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
    fn registration_diagnostic_keeps_native_fields_in_one_owner_only_record() {
        let root = directory();
        fs::create_dir_all(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let logs = root.join("logs");
        let description = "The plist \"ai.muniment.runtime\" is invalid.\nCheck BundleProgram.\r\t";
        write_registration_diagnostic(&logs, "SMAppServiceErrorDomain", -108, description).unwrap();
        let record = fs::read_to_string(logs.join("runtime.log")).unwrap();
        assert_eq!(record.lines().count(), 1);
        assert!(record.starts_with(
            "event=runtime_service_registration_failed domain=\"SMAppServiceErrorDomain\" code=-108 truncated=false description="
        ));
        let (_, encoded) = record.split_once(" description=").unwrap();
        assert_eq!(
            serde_json::from_str::<String>(encoded).unwrap(),
            description
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
        write_registration_diagnostic(&logs, "", 0, "").unwrap();
        assert_eq!(fs::read_to_string(logs.join("runtime.log")).unwrap(),
            "event=runtime_service_registration_failed domain=\"\" code=0 truncated=false description=\"\"\n");

        fs::write(logs.join("runtime.log"), []).unwrap();
        write_registration_diagnostic(&logs, &"\0".repeat(5000), isize::MAX, &"é\n".repeat(5000))
            .unwrap();
        let record = fs::read_to_string(logs.join("runtime.log")).unwrap();
        assert!(record.len() < MACOS_RUNTIME_LOG_MAX_BYTES as usize);
        assert_eq!(record.lines().count(), 1);
        assert!(record.contains("truncated=true"));
        let (_, encoded) = record.split_once(" description=").unwrap();
        assert_eq!(
            serde_json::from_str::<String>(encoded).unwrap(),
            "é\n".repeat(2048)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn startup_preserves_native_registration_error_at_the_log_limit() {
        let root = directory();
        fs::create_dir_all(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let logs = root.join("logs");
        let log = logs.join("runtime.log");
        let record = b"event=runtime_service_registration_failed domain=\"SMAppServiceErrorDomain\" code=-108 truncated=false description=\"The plist is invalid.\"\n";

        for status in [ServiceStatus::NotFound, ServiceStatus::NotRegistered] {
            for (error, expected) in [
                (RegistrationError::Failed, RuntimeServiceActivation::Failed),
                (
                    RegistrationError::Denied,
                    RuntimeServiceActivation::RequiresApproval,
                ),
            ] {
                for remaining in [
                    0,
                    record.len() - 1,
                    record.len(),
                    record.len() + 1,
                    record.len() + RUNTIME_SERVICE_REGISTRATION_FAILED_RECORD.len() - 1,
                    record.len() + RUNTIME_SERVICE_REGISTRATION_FAILED_RECORD.len(),
                    MACOS_RUNTIME_LOG_MAX_BYTES as usize,
                ] {
                    write_activation_diagnostic(&logs, RuntimeServiceActivation::Failed).unwrap();
                    let prefix = vec![b'x'; MACOS_RUNTIME_LOG_MAX_BYTES as usize - remaining];
                    fs::write(&log, &prefix).unwrap();
                    let mut adapter =
                        FakeAdapter::new([Ok(status), Ok(ServiceStatus::NotFound)], Err(error));
                    adapter.diagnostic_directory = Some(logs.clone());
                    assert_eq!(
                        activate_and_diagnose_runtime_service(&adapter, Some(&logs)),
                        expected
                    );
                    let start_adapter = FakeStartAdapter::new(Ok(false), Ok(()));
                    let outcome = request_enabled_runtime_start(expected, &start_adapter);
                    write_start_diagnostic(&logs, outcome).unwrap();
                    assert_eq!(outcome, RuntimeStartOutcome::SkippedActivation);
                    assert_eq!(adapter.registration_calls.get(), 1);
                    assert_eq!(adapter.status_calls.get(), 2);
                    assert!(adapter.registration_diagnostic_written());
                    let expected_log = if remaining < record.len() {
                        record.to_vec()
                    } else {
                        [prefix.as_slice(), record].concat()
                    };
                    assert_eq!(fs::read(&log).unwrap(), expected_log);
                }
            }
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn startup_keeps_generic_diagnostic_when_native_diagnostic_fails() {
        let root = directory();
        fs::create_dir_all(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let logs = root.join("logs");
        let mut adapter = FakeAdapter::new(
            [Ok(ServiceStatus::NotFound), Ok(ServiceStatus::NotFound)],
            Err(RegistrationError::Failed),
        );
        adapter.diagnostic_directory = Some(root.join("missing/logs"));
        assert_eq!(
            activate_and_diagnose_runtime_service(&adapter, Some(&logs)),
            RuntimeServiceActivation::Failed
        );
        assert!(!adapter.registration_diagnostic_written());
        assert_eq!(
            fs::read(logs.join("runtime.log")).unwrap(),
            RUNTIME_SERVICE_REGISTRATION_FAILED_RECORD
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

        assert_eq!(activate_runtime_service_at_startup(&adapter, None), None);
        assert_eq!(adapter.status_calls.get(), 0);
        assert_eq!(adapter.registration_calls.get(), 0);
    }
}
