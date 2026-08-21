#[cfg(target_os = "linux")]
mod activation;
#[cfg(target_os = "linux")]
mod attach_boundaries;
#[cfg(target_os = "linux")]
mod attach_listener;
#[cfg(target_os = "linux")]
mod attach_service;
#[cfg(target_os = "linux")]
mod attach_state;
mod directories;
mod macos_activation;
#[cfg(target_os = "linux")]
mod migration;
pub mod service;
mod sink;
#[cfg(target_os = "linux")]
mod upgrade_watch;

#[cfg(target_os = "linux")]
pub use activation::{
    run_runtime_activation, run_runtime_activation_with_desktop_executable,
    run_runtime_activation_with_retention_trigger, run_runtime_activation_with_upgrade_watch,
    RetentionScheduleTestControl, RuntimeActivationError, RuntimeActivationExit,
    UpgradeWatchTestControl,
};
#[cfg(target_os = "linux")]
pub use attach_boundaries::RuntimeAttachBoundaries;
#[cfg(target_os = "linux")]
pub use attach_listener::{
    run_attach_listener, run_bound_attach_listener, AttachListenerError, AttachListenerInputs,
};
#[cfg(target_os = "linux")]
pub use attach_service::compose_attach_service;
#[cfg(target_os = "linux")]
pub use attach_state::RuntimeAttachState;
pub use directories::{
    config_directory, installed_desktop_executable, installed_desktop_executable_from,
    macos_log_directory_from_home, profile_directory, resolve_directory,
    DirectoryUnavailableError, APPLICATION_IDENTIFIER,
};
#[cfg(target_os = "macos")]
pub use directories::effective_user_macos_log_directory;
pub use macos_activation::{
    record_macos_failed_exit, record_macos_orderly_exit, record_macos_start, MacosDiagnosticEvent,
    MacosStart, MacosStartDecision, MACOS_RUNTIME_LOG_MAX_BYTES,
};
#[cfg(unix)]
pub use macos_activation::write_macos_diagnostic;
#[cfg(target_os = "linux")]
pub use migration::{run_migration_takeover, MigrationTakeoverError};
pub use service::{
    accept_prompt, answer_permission, apply_retention, cancel_run, configure_run, create_thread,
    delete_thread, drive_prompt, ensure_home, ensure_native_session, entitlement_snapshot,
    list_devices, onboard_workspace, open_profile_storage, queue_run_message, rename_thread,
    resume_run, run_prompt, select_thread, session_status, sign_in, sign_out, thread_page,
    thread_summaries, ConfigureRunError, EntitlementSnapshotError, EntitlementSnapshotResult,
    PromptAcceptance, PromptLaunch, SignOutError,
};
#[cfg(target_os = "linux")]
pub use service::{
    list_companions, open_companion_registry, revoke_companion, stream_run, subscribe_run_commits,
};
pub use sink::{
    RuntimeChatEventBroadcast, RuntimeChatEventSink, RuntimeChatEventTarget,
    CHAT_EVENT_SUBSCRIBER_QUEUE_CAPACITY,
};
