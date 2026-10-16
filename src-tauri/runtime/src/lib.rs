//! The desktop's runtime binary: OS service registration and activation
//! around the core runtime in `muniment-runtime-core`, with the desktop host.

mod account;
#[cfg(target_os = "linux")]
mod activation;
mod directories;
mod host;
pub mod install_lock;
mod macos_activation;
mod sign_in_page;
mod start_record;
pub mod update_drain;
#[cfg(unix)]
mod upgrade_watch;
mod windows_activation;
mod windows_attach_activation;

pub use account::{
    ensure_native_session, entitlement_snapshot, list_devices, pairing_status,
    request_pairing_challenge, revoke_pairing_pair, session_status, sign_in, sign_out,
    DesktopAccount, EntitlementSnapshotError, SignOutError,
};
#[cfg(target_os = "linux")]
pub use activation::{
    run_runtime_activation, run_runtime_activation_with_desktop_executable,
    run_runtime_activation_with_retention_trigger, run_runtime_activation_with_upgrade_watch,
    RetentionScheduleTestControl, RuntimeActivationError, RuntimeActivationExit,
    UpgradeWatchTestControl,
};
#[cfg(target_os = "macos")]
pub use directories::effective_user_macos_log_directory;
pub use directories::{
    installed_desktop_executable, installed_desktop_executable_from, macos_log_directory_from_home,
    windows_log_directory_from_local_app_data, APPLICATION_IDENTIFIER,
};
#[cfg(target_os = "windows")]
pub use directories::{windows_local_app_data, windows_log_directory};
pub use host::{desktop_host, start_remote_route, DesktopChatGrants};
#[cfg(target_os = "macos")]
pub use macos_activation::SystemMacosAttachFactory;
pub use macos_activation::{
    emit_macos_unified_log, emit_macos_unified_log_with, record_macos_failed_exit,
    record_macos_orderly_exit, record_macos_start, MacosDiagnosticEvent, MacosStart,
    MacosStartDecision, MacosUnifiedLog, MacosUnifiedLogRecord, MACOS_RUNTIME_LOG_MAX_BYTES,
    MACOS_UNIFIED_LOG_CATEGORY,
};
#[cfg(unix)]
pub use macos_activation::{write_macos_diagnostic, write_macos_diagnostic_with};
// The desktop shell reads the one state root through this crate.
pub use muniment_runtime_core::{
    adopt_state_directory, config_directory, profile_directory, DirectoryUnavailableError,
};
pub use windows_activation::{
    clear_windows_crash_window, record_windows_failed_activation, record_windows_failed_exit,
    record_windows_orderly_exit, record_windows_start, ClearWindowsCrashWindowError,
    WindowsActivationExit, WindowsDiagnosticEvent, WindowsDiagnosticRecord, WindowsStart,
    WindowsStartDecision, WINDOWS_RUNTIME_LOG_MAX_BYTES,
};
#[cfg(any(unix, target_os = "windows"))]
pub use windows_activation::{run_recorded_windows_activation, write_windows_diagnostic};
#[cfg(target_os = "windows")]
pub use windows_attach_activation::SystemWindowsAttachFactory;
pub use windows_attach_activation::{
    run_windows_attach_activation, run_windows_attach_activation_with_retention_schedule,
    WindowsAttachBindFailure, WindowsAttachFactory, WindowsDiagnosticSink,
};
#[cfg(unix)]
pub use windows_attach_activation::{
    run_windows_attach_activation_with_upgrade_watch, MacosUpgradeWatchTestControl,
};
