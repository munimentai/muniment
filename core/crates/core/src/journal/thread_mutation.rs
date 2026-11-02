use crate::thread_ownership::{subject_owns_first_run, ThreadOwnershipError};
use chrono::{SecondsFormat, Utc};

use super::{JournalError, Provenance, RunJournal};

pub fn create_thread_now(
    journal: &mut RunJournal,
    workspace: &str,
    provenance: Provenance,
) -> Result<String, JournalError> {
    journal.create_thread(
        workspace,
        &Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true),
        provenance,
    )
}

#[derive(Debug)]
pub enum ThreadMutationError {
    Ownership(ThreadOwnershipError),
    NotOwned,
    Journal(JournalError),
    /// The creation chat record could not be removed.
    Creation(String),
}

fn check_ownership(
    journal: &mut RunJournal,
    thread_id: &str,
    subject: Option<&str>,
) -> Result<(), ThreadMutationError> {
    if !subject_owns_first_run(journal, thread_id, subject)
        .map_err(ThreadMutationError::Ownership)?
    {
        return Err(ThreadMutationError::NotOwned);
    }
    Ok(())
}

pub fn append_thread_rename(
    journal: &mut RunJournal,
    subject: Option<&str>,
    thread_id: &str,
    title: &str,
    recorded_at: &str,
    provenance: &Provenance,
) -> Result<(), ThreadMutationError> {
    check_ownership(journal, thread_id, subject)?;
    let last_thread_seq = journal
        .last_thread_seq(thread_id)
        .map_err(ThreadMutationError::Journal)?;
    journal
        .append_thread_title_renamed(last_thread_seq, thread_id, title, recorded_at, provenance)
        .map_err(ThreadMutationError::Journal)
}

pub fn append_thread_rename_now(
    journal: &mut RunJournal,
    subject: Option<&str>,
    thread_id: &str,
    title: &str,
    provenance: &Provenance,
) -> Result<(), ThreadMutationError> {
    append_thread_rename(
        journal,
        subject,
        thread_id,
        title,
        &Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true),
        provenance,
    )
}

pub fn append_thread_delete(
    journal: &mut RunJournal,
    subject: Option<&str>,
    thread_id: &str,
    recorded_at: &str,
    provenance: &Provenance,
) -> Result<(), ThreadMutationError> {
    check_ownership(journal, thread_id, subject)?;
    let last_thread_seq = journal
        .last_thread_seq(thread_id)
        .map_err(ThreadMutationError::Journal)?;
    journal
        .append_thread_deleted(last_thread_seq, thread_id, recorded_at, provenance)
        .map_err(ThreadMutationError::Journal)
}

pub fn append_thread_delete_now(
    journal: &mut RunJournal,
    subject: Option<&str>,
    thread_id: &str,
    provenance: &Provenance,
) -> Result<(), ThreadMutationError> {
    append_thread_delete(
        journal,
        subject,
        thread_id,
        &Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true),
        provenance,
    )
}

/// Deletes a thread and its creation chat record in `profile`. An agent's
/// creation chat holds its messages in that record and may hold no run, so
/// the record alone is enough to delete it. Answers the thread's folder when
/// it stays because it holds the user's files.
pub fn delete_thread_with_creation_now(
    journal: &mut RunJournal,
    profile: &std::path::Path,
    subject: Option<&str>,
    thread_id: &str,
    provenance: &Provenance,
) -> Result<Option<std::path::PathBuf>, ThreadMutationError> {
    let creation = crate::creations::read(profile, thread_id)
        .ok()
        .flatten()
        .is_some();
    match append_thread_delete_now(journal, subject, thread_id, provenance) {
        Ok(()) => {}
        // The journal holds no run of the chat.
        Err(ThreadMutationError::NotOwned | ThreadMutationError::Ownership(_)) if creation => {}
        Err(error) => return Err(error),
    }
    if creation {
        crate::creations::remove(profile, thread_id).map_err(ThreadMutationError::Creation)?;
    }
    Ok(tidy_deleted_thread(profile, thread_id))
}

/// Removes what a deleted thread leaves outside the journal: its session
/// folder when that holds only Muniment's files, and its place in the agent
/// state. Answers the folder when it stays because it holds the user's files.
fn tidy_deleted_thread(profile: &std::path::Path, thread_id: &str) -> Option<std::path::PathBuf> {
    // The thread's own session folder goes when it holds only Muniment's
    // files. A folder with the user's files stays, and the answer names it.
    let folder_kept = match crate::projects::remove_thread_folder(profile, thread_id) {
        Ok(None) => None,
        Ok(Some(folder)) => {
            crate::runtime_eprintln!(
                "thread_delete: folder kept thread={thread_id} reason=holds_user_files"
            );
            Some(folder)
        }
        Err(error) => {
            crate::runtime_eprintln!(
                "thread_delete: folder not removed thread={thread_id} error={error}"
            );
            None
        }
    };
    // An agent whose primary thread the user deletes starts a new one on its
    // next run. The delete is committed, so a state error only logs.
    let agent_thread = crate::agents::state(profile).is_ok_and(|state| {
        state.threads.contains_key(thread_id)
            || state
                .primary_threads
                .values()
                .any(|primary| primary == thread_id)
    });
    if !agent_thread {
        return folder_kept;
    }
    if let Err(error) = crate::agents::with_state(profile, |state| {
        Ok(crate::agents::forget_thread(state, thread_id))
    }) {
        crate::runtime_eprintln!(
            "thread_delete: agent state not updated thread={thread_id} error={error}"
        );
    }
    folder_kept
}

/// Whether the journal holds the thread and no delete of it.
fn thread_live(journal: &mut RunJournal, thread_id: &str) -> Result<bool, JournalError> {
    let events = journal.thread_events(thread_id)?;
    Ok(!events.is_empty()
        && events
            .iter()
            .all(|event| event.event_type != "thread.deleted"))
}

/// Deletes the creation chats of an agent the user deleted, with their
/// records and folders. The chat belongs to the agent, so the delete needs
/// no other owner check. Answers the threads it deleted.
pub fn delete_agent_creation_threads_now(
    journal: &mut RunJournal,
    profile: &std::path::Path,
    threads: &[String],
    provenance: &Provenance,
) -> Result<Vec<String>, ThreadMutationError> {
    let mut deleted = Vec::new();
    for thread_id in threads {
        if thread_live(journal, thread_id).map_err(ThreadMutationError::Journal)? {
            let last_thread_seq = journal
                .last_thread_seq(thread_id)
                .map_err(ThreadMutationError::Journal)?;
            // A thread with no run names its owner's attach profile on every
            // event, so the delete carries the profile the thread was created with.
            let mut provenance = provenance.clone();
            if !provenance.extra.contains_key("attach_profile") {
                if let Some(profile) = journal
                    .thread_scope(thread_id)
                    .map_err(ThreadMutationError::Journal)?
                    .and_then(|(_, profile)| profile)
                {
                    provenance
                        .extra
                        .insert("attach_profile".into(), profile.into());
                }
            }
            journal
                .append_thread_deleted(
                    last_thread_seq,
                    thread_id,
                    &Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true),
                    &provenance,
                )
                .map_err(ThreadMutationError::Journal)?;
            deleted.push(thread_id.clone());
        }
        crate::creations::remove(profile, thread_id).map_err(ThreadMutationError::Creation)?;
        tidy_deleted_thread(profile, thread_id);
    }
    Ok(deleted)
}

/// How long a thread that holds only its creation waits before the startup
/// sweep may delete it, so a fresh thread the user is about to write in stays.
const ORPHAN_THREAD_MIN_AGE: chrono::TimeDelta = chrono::TimeDelta::days(1);

impl RunJournal {
    /// The threads this runtime created that hold nothing but their
    /// creation, no run and no other event, each with its creation time. A
    /// replica of another member's thread is left out.
    fn bare_threads(&mut self) -> Result<Vec<(String, String)>, JournalError> {
        let coordination = self.coordination.clone();
        let _operation = coordination
            .as_ref()
            .map(|state| state.operation.lock().unwrap());
        self.refresh_after_compaction()?;
        let connection = self
            .connection
            .as_ref()
            .expect("journal connection is always present outside compaction");
        let mut statement = connection.prepare(
            "SELECT te.thread_id, te.envelope_json FROM thread_events te \
             WHERE te.event_type='thread.created' \
             AND NOT EXISTS(SELECT 1 FROM thread_events other \
                 WHERE other.thread_id=te.thread_id AND other.event_type<>'thread.created') \
             AND NOT EXISTS(SELECT 1 FROM run_threads rt WHERE rt.thread_id=te.thread_id)",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut bare = Vec::new();
        for row in rows {
            let (thread_id, raw) = row?;
            let created: super::ThreadEventEnvelope =
                serde_json::from_str(&raw).map_err(|error| {
                    JournalError::Corrupt(format!("invalid stored thread envelope JSON: {error}"))
                })?;
            if created
                .provenance
                .extra
                .contains_key(crate::swarm::SWARM_ORIGIN)
            {
                continue;
            }
            bare.push((thread_id, created.recorded_at));
        }
        Ok(bare)
    }
}

/// Deletes, at start, the creation chats that deleted agents left: a chat
/// whose creation record names an agent that no longer exists and that
/// holds no run, and a thread that holds only its creation, older than a
/// day, that no creation record, agent or project names, as an agent's
/// delete left its creation chat before it deleted the chat too. A thread
/// with a run, which holds the user's messages, always stays. Answers the
/// threads it deleted.
pub fn remove_orphan_creation_threads(
    journal: &mut RunJournal,
    profile: &std::path::Path,
    provenance: &Provenance,
) -> Result<Vec<String>, String> {
    let error = |error: &dyn std::fmt::Debug| format!("{error:?}");
    let runless = |journal: &mut RunJournal, thread: &str| {
        journal
            .thread_run_ids(thread, 1, None)
            .map(|page| page.run_ids.is_empty())
            .map_err(|failure| error(&failure))
    };
    let creations = crate::creations::list(profile)?;
    let mut orphans = Vec::new();
    for creation in &creations {
        let Some(agent) = creation.result_id.as_deref() else {
            continue;
        };
        if creation.kind == "agent"
            && !crate::agents::exists(profile, agent)
            && runless(journal, &creation.thread_id)?
        {
            orphans.push(creation.thread_id.clone());
        }
    }
    let state = crate::agents::state(profile)?;
    let catalog = crate::projects::list(profile)?;
    let cutoff = Utc::now() - ORPHAN_THREAD_MIN_AGE;
    for (thread, created_at) in journal.bare_threads().map_err(|failure| error(&failure))? {
        let old =
            chrono::DateTime::parse_from_rfc3339(&created_at).is_ok_and(|created| created < cutoff);
        let named = creations
            .iter()
            .any(|creation| creation.thread_id == thread)
            || state.threads.contains_key(&thread)
            || state
                .primary_threads
                .values()
                .any(|primary| *primary == thread)
            || catalog.threads.contains_key(&thread);
        if old && !named {
            orphans.push(thread);
        }
    }
    delete_agent_creation_threads_now(journal, profile, &orphans, provenance)
        .map_err(|failure| error(&failure))
}

/// Takes a run and every later run out of the thread's conversation.
/// Answers the runs it took out.
pub fn append_thread_rewind_now(
    journal: &mut RunJournal,
    subject: Option<&str>,
    thread_id: &str,
    run_id: &str,
    provenance: &Provenance,
) -> Result<Vec<String>, ThreadMutationError> {
    check_ownership(journal, thread_id, subject)?;
    if journal
        .run_thread_id(run_id)
        .map_err(ThreadMutationError::Journal)?
        .as_deref()
        != Some(thread_id)
    {
        return Err(ThreadMutationError::NotOwned);
    }
    let run_ids = journal
        .thread_runs_from(run_id)
        .map_err(ThreadMutationError::Journal)?;
    if run_ids.is_empty() {
        return Ok(run_ids);
    }
    let last_thread_seq = journal
        .last_thread_seq(thread_id)
        .map_err(ThreadMutationError::Journal)?;
    journal
        .append_thread_rewound(
            last_thread_seq,
            thread_id,
            &run_ids,
            &Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true),
            provenance,
        )
        .map_err(ThreadMutationError::Journal)?;
    Ok(run_ids)
}

/// The most words a first-turn name holds, generated or taken from the prompt.
pub const FIRST_NAME_WORDS: usize = 6;
/// The most characters a prompt fallback keeps before its ellipsis.
const FALLBACK_NAME_CHARS: usize = 48;

/// The name a thread carries until the model names it: the first message
/// whole when it is short, else its leading words cut at a word boundary
/// with an ellipsis.
pub fn fallback_thread_name(prompt: &str) -> String {
    let words: Vec<&str> = prompt.split_whitespace().collect();
    let mut name = String::new();
    let mut kept = 0;
    for word in &words {
        let length = name.chars().count() + usize::from(!name.is_empty()) + word.chars().count();
        if kept == FIRST_NAME_WORDS || length > FALLBACK_NAME_CHARS {
            break;
        }
        if !name.is_empty() {
            name.push(' ');
        }
        name.push_str(word);
        kept += 1;
    }
    if kept == words.len() {
        return name;
    }
    if name.is_empty() {
        // A first word longer than the bound keeps its leading characters.
        name = words[0].chars().take(FALLBACK_NAME_CHARS - 1).collect();
    }
    let name = name.trim_end_matches(|c: char| c.is_ascii_punctuation());
    format!("{name}…")
}

/// A first-turn name may replace only its own temporary fallback, never a user's name.
pub fn first_thread_needs_name(journal: &mut RunJournal, run_id: &str) -> Option<String> {
    let thread_id = journal.run_thread_id(run_id).ok()??;
    let runs = journal.thread_run_ids(&thread_id, 1, None).ok()?;
    if runs.run_ids.first().map(String::as_str) != Some(run_id) {
        return None;
    }
    let events = journal.thread_events(&thread_id).ok()?;
    if events.iter().any(|event| {
        event.event_type == "thread.deleted"
            || (event.event_type == "thread.title.renamed"
                && event.provenance.source != "thread-name-fallback")
    }) {
        return None;
    }
    Some(thread_id)
}

pub fn save_first_thread_name(
    journal: &mut RunJournal,
    run_id: &str,
    title: &str,
    generated: bool,
) -> Result<bool, JournalError> {
    let Some(thread_id) = first_thread_needs_name(journal, run_id) else {
        return Ok(false);
    };
    let title = title.trim();
    let title = if crate::memory_secret::reject_memory_secret(title).is_err() {
        if generated {
            return Ok(false);
        }
        "New conversation"
    } else {
        title
    };
    if title.is_empty()
        || title.chars().count() > 80
        || title.split_whitespace().count() > FIRST_NAME_WORDS
        || title.chars().any(char::is_control)
    {
        return Ok(false);
    }
    let mut provenance = journal
        .events(run_id)?
        .first()
        .ok_or_else(|| JournalError::InvalidEnvelope("The thread has no first message.".into()))?
        .provenance
        .clone();
    provenance.source = if generated {
        "thread-name-model"
    } else {
        "thread-name-fallback"
    }
    .into();
    let seq = journal.last_thread_seq(&thread_id)?;
    journal.append_thread_title_renamed(
        seq,
        &thread_id,
        title,
        &Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true),
        &provenance,
    )?;
    Ok(true)
}
