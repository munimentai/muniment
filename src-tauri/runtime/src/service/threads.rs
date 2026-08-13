//! Thread and run stream service operations.

#[cfg(target_os = "linux")]
use muniment_core::attach::linux::RunStreamPage;
use muniment_core::attach::ProtocolError;
use muniment_core::chat_profile::ChatProfile;
use muniment_core::journal::retention::{
    apply_retention_now_with, RetentionError, RetentionOutcome,
};
use muniment_core::journal::thread_mutation::{
    append_thread_delete_now, append_thread_rename_now, create_thread_now,
};
use muniment_core::journal::thread_summaries::ThreadSummaryPage;
#[cfg(target_os = "linux")]
use muniment_core::journal::JournalCommitHint;
use muniment_core::owned_threads::chat_thread_summaries_page;
use muniment_core::run_events::{ChatStorage, SharedStorage};
use muniment_core::thread_history::{chat_thread_open_page, ChatThreadOpenPage};
use std::path::Path;
#[cfg(target_os = "linux")]
use std::sync::mpsc::Receiver;

use super::runtime_provenance;

/// Lists the threads owned by one subject.
pub fn thread_summaries(
    storage: SharedStorage,
    subject: Option<String>,
    limit: usize,
    cursor: Option<String>,
) -> Result<ThreadSummaryPage, String> {
    let mut storage = storage
        .lock()
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    chat_thread_summaries_page(
        &mut storage.journal,
        subject.as_deref(),
        limit,
        cursor.as_deref(),
    )
    .map_err(|_| "Conversation history is unavailable.".to_string())
}

/// Reads one page of a run stream.
#[cfg(target_os = "linux")]
pub fn stream_run(
    storage: SharedStorage,
    workspace: String,
    run_id: String,
    after_run_seq: u64,
) -> Result<RunStreamPage, ProtocolError> {
    use muniment_core::attach::linux::ThreadListService;

    let mut storage = storage
        .lock()
        .map_err(|_| ProtocolError::persistence_failed())?;
    storage
        .journal
        .stream_run(&workspace, &run_id, after_run_seq)
}

/// Subscribes to commits for one run.
#[cfg(target_os = "linux")]
pub fn subscribe_run_commits(
    storage: SharedStorage,
    run_id: String,
) -> Result<Option<(u64, Receiver<JournalCommitHint>)>, ProtocolError> {
    use muniment_core::attach::linux::ThreadListService;

    let mut storage = storage
        .lock()
        .map_err(|_| ProtocolError::persistence_failed())?;
    storage.journal.subscribe_run_commits(&run_id)
}

/// Creates one thread for an authorized attach profile.
pub fn create_thread(
    storage: SharedStorage,
    workspace: String,
    attach_profile: String,
) -> Result<String, String> {
    let mut provenance = runtime_provenance();
    provenance
        .extra
        .insert("attach_profile".into(), attach_profile.into());
    let mut storage = storage
        .lock()
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    create_thread_now(&mut storage.journal, &workspace, provenance)
        .map_err(|_| "Conversation history is unavailable.".to_string())
}

/// Renames one thread owned by one subject.
pub fn rename_thread(
    storage: SharedStorage,
    subject: Option<String>,
    thread_id: String,
    title: String,
) -> Result<(), String> {
    let mut storage = storage
        .lock()
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    append_thread_rename_now(
        &mut storage.journal,
        subject.as_deref(),
        &thread_id,
        &title,
        &runtime_provenance(),
    )
    .map_err(|_| "Conversation history is unavailable.".to_string())
}

/// Deletes one thread owned by one subject.
pub fn delete_thread(
    storage: SharedStorage,
    subject: Option<String>,
    thread_id: String,
) -> Result<(), String> {
    let mut storage = storage
        .lock()
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    append_thread_delete_now(
        &mut storage.journal,
        subject.as_deref(),
        &thread_id,
        &runtime_provenance(),
    )
    .map_err(|_| "Conversation history is unavailable.".to_string())
}

/// Deletes terminal runs older than the maximum age.
pub fn apply_retention(
    storage: SharedStorage,
    max_age_seconds: i64,
) -> Result<RetentionOutcome, String> {
    let mut storage = storage
        .lock()
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    let ChatStorage { journal, cas } = &mut *storage;
    apply_retention_now_with(journal, Some(cas), max_age_seconds, |deleted_run| {
        muniment_core::chat_prompt::delete_prompt(
            &deleted_run.run_id,
            deleted_run.subject.as_deref(),
        )
        .map_err(|_| RetentionError::BeforeDelete)
    })
    .map_err(|_| "Conversation history is unavailable.".to_string())
}

/// Opens one thread owned by one subject.
pub fn thread_page(
    profile_directory: impl AsRef<Path>,
    storage: SharedStorage,
    subject: Option<String>,
    thread_id: String,
    limit: usize,
    cursor: Option<String>,
) -> Result<ChatThreadOpenPage, String> {
    let profile_directory = profile_directory.as_ref();
    let profile = ChatProfile::new(profile_directory);
    let mut storage = storage
        .lock()
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    let ChatStorage { journal, cas } = &mut *storage;
    chat_thread_open_page(
        journal,
        Some(cas),
        subject.as_deref(),
        &profile.pi_session_root(),
        &thread_id,
        limit,
        cursor.as_deref(),
    )
    .map_err(|_| "Conversation history is unavailable.".to_string())
}
