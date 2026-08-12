#[cfg(target_os = "linux")]
pub mod handoff_listener;
#[cfg(target_os = "linux")]
mod migration;
pub mod service;
mod sink;

#[cfg(target_os = "linux")]
pub use migration::{run_migration_takeover, MigrationTakeoverError};
pub use service::{
    accept_prompt, answer_permission, apply_retention, cancel_run, create_thread, delete_thread,
    drive_prompt, ensure_home, ensure_native_session, entitlement_snapshot, fetch_chat_grant,
    onboard_workspace, open_profile_storage, queue_run_message, rename_thread, resume_run,
    run_prompt, thread_page, thread_summaries, EntitlementSnapshotError, EntitlementSnapshotResult,
    PromptAcceptance, PromptLaunch,
};
#[cfg(target_os = "linux")]
pub use service::{stream_run, subscribe_run_commits};
pub use sink::RuntimeChatEventSink;
