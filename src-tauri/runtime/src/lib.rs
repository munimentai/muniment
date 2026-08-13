#[cfg(target_os = "linux")]
mod attach_boundaries;
#[cfg(target_os = "linux")]
pub mod handoff_listener;
#[cfg(target_os = "linux")]
mod migration;
pub mod service;
mod sink;

#[cfg(target_os = "linux")]
pub use attach_boundaries::RuntimeAttachBoundaries;
#[cfg(target_os = "linux")]
pub use migration::{run_migration_takeover, MigrationTakeoverError};
pub use service::{
    accept_prompt, answer_permission, apply_retention, cancel_run, configure_run, create_thread,
    delete_thread, drive_prompt, ensure_home, ensure_native_session, entitlement_snapshot,
    list_devices, onboard_workspace, open_profile_storage, queue_run_message, rename_thread,
    resume_run, run_prompt, session_status, sign_out, thread_page, thread_summaries,
    ConfigureRunError, EntitlementSnapshotError, EntitlementSnapshotResult, PromptAcceptance,
    PromptLaunch, SignOutError,
};
#[cfg(target_os = "linux")]
pub use service::{
    list_companions, open_companion_registry, revoke_companion, stream_run, subscribe_run_commits,
};
pub use sink::RuntimeChatEventSink;
