#[cfg(target_os = "linux")]
pub mod handoff_listener;
#[cfg(target_os = "linux")]
mod migration;
pub mod service;
mod sink;

#[cfg(target_os = "linux")]
pub use migration::{run_migration_takeover, MigrationTakeoverError};
pub use service::{
    accept_prompt, apply_retention, create_thread, delete_thread, drive_prompt, ensure_native_session,
    fetch_chat_grant, open_profile_storage, rename_thread, resume_run, run_prompt, thread_page,
    thread_summaries, PromptAcceptance, PromptLaunch,
};
pub use sink::RuntimeChatEventSink;
