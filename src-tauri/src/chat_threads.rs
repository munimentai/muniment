use chrono::{SecondsFormat, Utc};
use muniment_core::chat_view::chat_attachments;
use muniment_core::journal::thread_mutation::{append_thread_delete, append_thread_rename};
use muniment_core::journal::thread_summaries::ThreadSummary;
use muniment_core::journal::{Provenance, RunJournal};
#[cfg(test)]
use muniment_core::owned_threads::MAX_THREAD_SUMMARY_CORE_PAGES;
use muniment_core::owned_threads::{
    chat_thread_summaries_page as core_chat_thread_summaries_page,
    newest_owned_workspace_thread as core_newest_owned_workspace_thread, OwnedThreadsError,
};
use muniment_core::thread_history::{
    chat_thread_open_page as core_chat_thread_open_page, ChatThreadOpenPage, ThreadHistoryError,
};
use muniment_core::thread_ownership::{subject_owns_first_run, ThreadOwnershipError};
use serde::Serialize;
use std::collections::BTreeMap;

#[cfg(target_os = "linux")]
use crate::attach_service::{AttachCompanionState, DesktopClientSession};
use crate::auth;
use crate::chat::{state_session_root, ChatState};
use muniment_core::run_events::SharedStorage;
use muniment_core::session_thread::SessionThread;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatThreadSummary {
    pub(crate) thread_id: String,
    pub(crate) title: String,
    pub(crate) updated_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatThreadSummaryPage {
    pub(crate) summaries: Vec<ChatThreadSummary>,
    pub(crate) next_cursor: Option<String>,
}

fn thread_ownership_error_message(_error: ThreadOwnershipError) -> String {
    "Conversation history is unavailable.".into()
}

#[cfg(target_os = "linux")]
fn route_thread_write(
    session: DesktopClientSession,
    remote: impl FnOnce(&muniment_core::attach::DesktopClientHolder) -> Result<(), String>,
    local: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    match session {
        DesktopClientSession::NoSupervisor => local(),
        DesktopClientSession::Connected(client) => remote(&client),
        DesktopClientSession::Disconnected => {
            Err("Muniment cannot reach its background service.".into())
        }
    }
}

pub(crate) fn newest_owned_workspace_thread(
    journal: &mut RunJournal,
    workspace: &str,
    subject: Option<&str>,
) -> Result<Option<String>, String> {
    core_newest_owned_workspace_thread(journal, workspace, subject)
        .map_err(owned_threads_error_message)
}

pub(crate) fn chat_thread_summaries_page(
    journal: &mut RunJournal,
    subject: Option<&str>,
    limit: usize,
    cursor: Option<&str>,
) -> Result<ChatThreadSummaryPage, String> {
    let page = core_chat_thread_summaries_page(journal, subject, limit, cursor)
        .map_err(owned_threads_error_message)?;
    Ok(ChatThreadSummaryPage {
        summaries: page
            .summaries
            .into_iter()
            .map(
                |ThreadSummary {
                     thread_id,
                     title,
                     updated_at,
                 }| ChatThreadSummary {
                    thread_id,
                    title,
                    updated_at,
                },
            )
            .collect(),
        next_cursor: page.next_cursor,
    })
}

fn owned_threads_error_message(_error: OwnedThreadsError) -> String {
    "Conversation history is unavailable.".into()
}

pub(crate) fn chat_thread_open_page(
    journal: &mut RunJournal,
    subject: Option<&str>,
    session_root: &std::path::Path,
    thread_id: &str,
    limit: usize,
    cursor: Option<&str>,
) -> Result<ChatThreadOpenPage, String> {
    core_chat_thread_open_page(
        journal,
        None,
        subject,
        session_root,
        thread_id,
        limit,
        cursor,
    )
    .map_err(thread_history_error_message)
}

fn thread_history_error_message(_error: ThreadHistoryError) -> String {
    "Conversation history is unavailable.".into()
}

pub(crate) fn select_session_thread(
    journal: &mut RunJournal,
    tracker: &SessionThread,
    subject: Option<&str>,
    thread_id: &str,
) -> Result<(), String> {
    if !subject_owns_first_run(journal, thread_id, subject)
        .map_err(thread_ownership_error_message)?
    {
        return Err("Conversation history is unavailable.".into());
    }
    tracker.select(thread_id.to_owned(), subject);
    Ok(())
}

pub(crate) fn rename_thread(
    journal: &mut RunJournal,
    subject: Option<&str>,
    thread_id: &str,
    title: &str,
) -> Result<(), String> {
    append_thread_rename(
        journal,
        subject,
        thread_id,
        title,
        &Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true),
        &Provenance {
            source: "muniment-desktop".into(),
            source_version: env!("CARGO_PKG_VERSION").into(),
            actor_id: subject.map(str::to_owned),
            device_id: None,
            rpc_request_id: None,
            capability_versions: None,
            extra: BTreeMap::new(),
        },
    )
    .map_err(|_| "Conversation history is unavailable.".to_string())
}

pub(crate) fn delete_thread(
    journal: &mut RunJournal,
    tracker: &SessionThread,
    subject: Option<&str>,
    thread_id: &str,
) -> Result<(), String> {
    append_thread_delete(
        journal,
        subject,
        thread_id,
        &Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true),
        &Provenance {
            source: "muniment-desktop".into(),
            source_version: env!("CARGO_PKG_VERSION").into(),
            actor_id: subject.map(str::to_owned),
            device_id: None,
            rpc_request_id: None,
            capability_versions: None,
            extra: BTreeMap::new(),
        },
    )
    .map_err(|_| "Conversation history is unavailable.".to_string())?;
    tracker.fresh_if_current(thread_id, subject);
    Ok(())
}

pub(crate) fn fresh_session_thread(
    storage: &SharedStorage,
    tracker: &SessionThread,
    subject: Option<&str>,
) -> Result<(), String> {
    let _storage = storage
        .lock()
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    tracker.fresh(subject);
    Ok(())
}

#[tauri::command]
pub async fn chat_current_thread(
    app_handle: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
) -> Result<Option<String>, String> {
    let tokens = auth::fresh_tokens(&auth_state, &app_handle)?;
    Ok(state.session_thread.current(tokens.subject.as_deref()))
}

#[tauri::command]
pub async fn chat_thread_summaries(
    app_handle: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
    limit: usize,
    cursor: Option<String>,
) -> Result<ChatThreadSummaryPage, String> {
    let tokens = auth::fresh_tokens(&auth_state, &app_handle)?;
    let mut storage = state
        .storage
        .lock()
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    chat_thread_summaries_page(
        &mut storage.journal,
        tokens.subject.as_deref(),
        limit,
        cursor.as_deref(),
    )
}

#[tauri::command]
pub async fn chat_select_thread(
    app_handle: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
    thread_id: String,
) -> Result<(), String> {
    let tokens = auth::fresh_tokens(&auth_state, &app_handle)?;
    let mut storage = state
        .storage
        .lock()
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    select_session_thread(
        &mut storage.journal,
        &state.session_thread,
        tokens.subject.as_deref(),
        &thread_id,
    )
}

#[tauri::command]
pub async fn chat_rename_thread(
    app_handle: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
    #[cfg(target_os = "linux")] attach_state: tauri::State<'_, AttachCompanionState>,
    thread_id: String,
    title: String,
) -> Result<(), String> {
    let tokens = auth::fresh_tokens(&auth_state, &app_handle)?;
    #[cfg(target_os = "linux")]
    return route_thread_write(
        attach_state.desktop_client_session(),
        |client| {
            client
                .rename_thread(&thread_id, &title)
                .map_err(|_| "Muniment cannot reach its background service.".to_string())
        },
        || {
            let mut storage = state
                .storage
                .lock()
                .map_err(|_| "Conversation history is unavailable.".to_string())?;
            rename_thread(
                &mut storage.journal,
                tokens.subject.as_deref(),
                &thread_id,
                &title,
            )
        },
    );
    #[cfg(not(target_os = "linux"))]
    let mut storage = state
        .storage
        .lock()
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    #[cfg(not(target_os = "linux"))]
    return rename_thread(
        &mut storage.journal,
        tokens.subject.as_deref(),
        &thread_id,
        &title,
    );
}

#[tauri::command]
pub async fn chat_delete_thread(
    app_handle: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
    #[cfg(target_os = "linux")] attach_state: tauri::State<'_, AttachCompanionState>,
    thread_id: String,
) -> Result<(), String> {
    let tokens = auth::fresh_tokens(&auth_state, &app_handle)?;
    #[cfg(target_os = "linux")]
    return route_thread_write(
        attach_state.desktop_client_session(),
        |client| {
            client
                .delete_thread(&thread_id)
                .map_err(|_| "Muniment cannot reach its background service.".to_string())
        },
        || {
            let mut storage = state
                .storage
                .lock()
                .map_err(|_| "Conversation history is unavailable.".to_string())?;
            delete_thread(
                &mut storage.journal,
                &state.session_thread,
                tokens.subject.as_deref(),
                &thread_id,
            )
        },
    );
    #[cfg(not(target_os = "linux"))]
    let mut storage = state
        .storage
        .lock()
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    #[cfg(not(target_os = "linux"))]
    return delete_thread(
        &mut storage.journal,
        &state.session_thread,
        tokens.subject.as_deref(),
        &thread_id,
    );
}

#[tauri::command]
pub async fn chat_new_thread(
    app_handle: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
) -> Result<(), String> {
    let tokens = auth::fresh_tokens(&auth_state, &app_handle)?;
    fresh_session_thread(
        &state.storage,
        &state.session_thread,
        tokens.subject.as_deref(),
    )
}

#[tauri::command]
pub async fn chat_thread_open(
    app_handle: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
    thread_id: String,
    limit: usize,
    cursor: Option<String>,
) -> Result<ChatThreadOpenPage, String> {
    let tokens = auth::fresh_tokens(&auth_state, &app_handle)?;
    let session_root = state_session_root(&app_handle)?;
    let mut storage = state
        .storage
        .lock()
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    let muniment_core::run_events::ChatStorage { journal, cas } = &mut *storage;
    core_chat_thread_open_page(
        journal,
        Some(cas),
        tokens.subject.as_deref(),
        &session_root,
        &thread_id,
        limit,
        cursor.as_deref(),
    )
    .map_err(thread_history_error_message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{
        desktop_provenance, event_envelope, prepare_new_run, prepare_new_run_with_session_thread,
        SessionThreadStart,
    };
    use crate::test_support::append_test_event;
    use chrono::{SecondsFormat, Utc};
    use muniment_core::cas::LocalCas;
    use muniment_core::chat_view::projection_phase;
    use muniment_core::chat_view::SelectedFile;
    use muniment_core::journal::reconciliation::reconcile_interrupted_runs;
    use muniment_core::journal::reducer::{
        project_chat, project_chat_with_state, reduce, PermissionRequest, RunStatus,
    };
    use muniment_core::journal::{EventPayload, Provenance};
    use muniment_core::run_events::ChatStorage;
    use muniment_core::session_thread::OfferedThread;
    use muniment_core::thread_history::history_resumable;
    use serde_json::{json, Value};
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use uuid::Uuid;

    #[cfg(target_os = "linux")]
    #[test]
    fn thread_writes_route_both_commands_for_each_desktop_client_state() {
        use std::cell::Cell;

        for command in ["rename", "delete"] {
            for (session, expected_remote, expected_local, expected_error) in [
                (DesktopClientSession::NoSupervisor, 0, 1, None),
                (
                    DesktopClientSession::Connected(
                        muniment_core::attach::DesktopClientHolder::new(),
                    ),
                    1,
                    0,
                    None,
                ),
                (
                    DesktopClientSession::Disconnected,
                    0,
                    0,
                    Some("Muniment cannot reach its background service."),
                ),
            ] {
                let remote_calls = Cell::new(0);
                let local_calls = Cell::new(0);
                let result = route_thread_write(
                    session,
                    |_| {
                        remote_calls.set(remote_calls.get() + 1);
                        Ok(())
                    },
                    || {
                        local_calls.set(local_calls.get() + 1);
                        Ok(())
                    },
                );

                assert_eq!(remote_calls.get(), expected_remote, "{command}");
                assert_eq!(local_calls.get(), expected_local, "{command}");
                assert_eq!(result.err().as_deref(), expected_error, "{command}");
            }
        }

        let error = route_thread_write(
            DesktopClientSession::NoSupervisor,
            |_| Ok(()),
            || Err("Conversation history is unavailable.".into()),
        )
        .unwrap_err();
        assert_eq!(error, "Conversation history is unavailable.");
    }

    #[test]
    fn rename_thread_accepts_owned_thread_and_rejects_invalid_requests() {
        let directory = std::env::temp_dir().join(format!("muniment-chat-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut journal = RunJournal::open(directory.join("runs.sqlite3")).unwrap();
        let mut new_thread = |owner| {
            journal
                .append_new_run(
                    "workspace-a",
                    &event_envelope(
                        &Uuid::now_v7().to_string(),
                        1,
                        "run.started",
                        json!({}),
                        Some(owner),
                    ),
                )
                .unwrap()
        };
        let owned_thread = new_thread("owner");
        let foreign_thread = new_thread("other");
        let deleted_thread = new_thread("owner");
        let unknown_thread = Uuid::now_v7().to_string();
        let provenance = Provenance {
            source: "test".into(),
            source_version: "1".into(),
            actor_id: Some("owner".into()),
            device_id: None,
            rpc_request_id: None,
            capability_versions: None,
            extra: BTreeMap::new(),
        };
        journal
            .append_thread_deleted(1, &deleted_thread, "2026-07-10T12:00:01Z", &provenance)
            .unwrap();

        for (thread_id, title, expected_seq) in [
            (unknown_thread.as_str(), "title".to_string(), 0),
            (foreign_thread.as_str(), "title".to_string(), 1),
            (deleted_thread.as_str(), "title".to_string(), 2),
            (owned_thread.as_str(), " \n\t ".to_string(), 1),
            (
                owned_thread.as_str(),
                "é".repeat(muniment_core::journal::MAX_THREAD_TITLE_CHARS + 1),
                1,
            ),
        ] {
            assert_eq!(
                rename_thread(&mut journal, Some("owner"), thread_id, &title).unwrap_err(),
                "Conversation history is unavailable."
            );
            assert_eq!(journal.last_thread_seq(thread_id).unwrap(), expected_seq);
        }

        rename_thread(&mut journal, Some("owner"), &owned_thread, "  New title  ").unwrap();
        assert_eq!(journal.last_thread_seq(&owned_thread).unwrap(), 2);
        let summaries = chat_thread_summaries_page(&mut journal, Some("owner"), 10, None).unwrap();
        assert_eq!(
            summaries
                .summaries
                .iter()
                .find(|summary| summary.thread_id == owned_thread)
                .unwrap()
                .title,
            "New title"
        );

        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn delete_thread_hides_owned_thread_and_rejects_invalid_requests() {
        let directory = std::env::temp_dir().join(format!("muniment-chat-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut journal = RunJournal::open(directory.join("runs.sqlite3")).unwrap();
        let mut new_thread = |owner| {
            journal
                .append_new_run(
                    "workspace-a",
                    &event_envelope(
                        &Uuid::now_v7().to_string(),
                        1,
                        "run.started",
                        json!({}),
                        Some(owner),
                    ),
                )
                .unwrap()
        };
        let owned_thread = new_thread("owner");
        let other_owned_thread = new_thread("owner");
        let foreign_thread = new_thread("other");
        let deleted_thread = new_thread("owner");
        let unknown_thread = Uuid::now_v7().to_string();
        let provenance = Provenance {
            source: "test".into(),
            source_version: "1".into(),
            actor_id: Some("owner".into()),
            device_id: None,
            rpc_request_id: None,
            capability_versions: None,
            extra: BTreeMap::new(),
        };
        journal
            .append_thread_deleted(1, &deleted_thread, "2026-07-10T12:00:01Z", &provenance)
            .unwrap();
        let tracker = SessionThread::default();
        tracker.select(other_owned_thread.clone(), Some("owner"));

        for (thread_id, expected_seq) in [
            (unknown_thread.as_str(), 0),
            (foreign_thread.as_str(), 1),
            (deleted_thread.as_str(), 2),
        ] {
            assert_eq!(
                delete_thread(&mut journal, &tracker, Some("owner"), thread_id).unwrap_err(),
                "Conversation history is unavailable."
            );
            assert_eq!(journal.last_thread_seq(thread_id).unwrap(), expected_seq);
            assert_eq!(
                tracker.current(Some("owner")),
                Some(other_owned_thread.clone())
            );
        }

        let before = chat_thread_summaries_page(&mut journal, Some("owner"), 10, None).unwrap();
        assert!(before
            .summaries
            .iter()
            .any(|summary| summary.thread_id == owned_thread));

        delete_thread(&mut journal, &tracker, Some("owner"), &owned_thread).unwrap();
        assert_eq!(journal.last_thread_seq(&owned_thread).unwrap(), 2);
        assert_eq!(
            tracker.current(Some("owner")),
            Some(other_owned_thread.clone())
        );
        let after = chat_thread_summaries_page(&mut journal, Some("owner"), 10, None).unwrap();
        assert!(!after
            .summaries
            .iter()
            .any(|summary| summary.thread_id == owned_thread));

        tracker.select(other_owned_thread.clone(), Some("owner"));
        delete_thread(&mut journal, &tracker, Some("owner"), &other_owned_thread).unwrap();
        assert_eq!(tracker.current(Some("owner")), None);
        assert_eq!(
            tracker.offered("workspace-a", Some("owner")),
            OfferedThread::Fresh
        );

        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn thread_open_projects_and_clears_a_typed_pending_permission() {
        keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
        let directory = std::env::temp_dir().join(format!("muniment-chat-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut journal = RunJournal::open(directory.join("runs.sqlite3")).unwrap();
        let run_id = Uuid::now_v7().to_string();
        journal
            .append_new_run(
                "workspace-a",
                &event_envelope(&run_id, 1, "run.started", json!({}), None),
            )
            .unwrap();
        append_test_event(
            &mut journal,
            &run_id,
            2,
            "permission.requested",
            json!({
                "gate_id": "pi-request-1",
                "kind": "editor",
                "title": "Review command",
                "prefill": "cargo test",
                "timeout": 30_000
            }),
            None,
        );

        let summaries = chat_thread_summaries_page(&mut journal, None, 10, None).unwrap();
        let thread_id = summaries.summaries[0].thread_id.clone();
        let entries = chat_thread_open_page(
            &mut journal,
            None,
            std::path::Path::new("."),
            &thread_id,
            10,
            None,
        )
        .unwrap();
        let entry = &entries.entries[0];
        assert_eq!(entry.phase, "pending-permission");
        let pending = entry.pending_permission.as_ref().unwrap();
        assert_eq!(pending.gate_id, "pi-request-1");
        assert!(matches!(
            &pending.request,
            PermissionRequest::Editor { title, prefill, timeout }
                if title == "Review command"
                    && prefill.as_deref() == Some("cargo test")
                    && *timeout == Some(30_000)
        ));

        append_test_event(
            &mut journal,
            &run_id,
            3,
            "permission.resolved",
            json!({"gate_id": "pi-request-1", "decision": "cancelled"}),
            None,
        );
        let entries = chat_thread_open_page(
            &mut journal,
            None,
            std::path::Path::new("."),
            &thread_id,
            10,
            None,
        )
        .unwrap();
        let entry = &entries.entries[0];
        assert!(entry.pending_permission.is_none());
        assert_eq!(entry.phase, "thinking");

        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn history_resumable_rejects_each_unsafe_state() {
        let directory = std::env::temp_dir().join(format!("muniment-resume-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("session.jsonl"), "{}\n").unwrap();
        let run_id = Uuid::now_v7().to_string();
        let events = |tail: Vec<(&str, Value)>| {
            let mut values = vec![event_envelope(
                &run_id,
                1,
                "run.started",
                json!({}),
                Some("owner"),
            )];
            values.extend(
                tail.into_iter()
                    .enumerate()
                    .map(|(index, (kind, payload))| {
                        event_envelope(&run_id, index as u64 + 2, kind, payload, Some("owner"))
                    }),
            );
            values
        };
        let eligible_tail = || {
            vec![
                (
                    "runtime.pi_session.bound",
                    json!({"run_id":run_id, "locator":"session.jsonl"}),
                ),
                ("run.needs_attention", json!({"reason":"interrupted"})),
            ]
        };
        let eligible = events(eligible_tail());
        let (_, eligible_state) = project_chat_with_state(&eligible).unwrap();
        assert!(history_resumable(
            &eligible,
            &eligible_state,
            Some("owner"),
            &directory
        ));
        let rejected = [
            (events(eligible_tail()), Some("another-subject")),
            (
                events(vec![(
                    "runtime.pi_session.bound",
                    json!({"run_id":run_id, "locator":"session.jsonl"}),
                )]),
                Some("owner"),
            ),
            (
                events(vec![
                    (
                        "runtime.pi_session.bound",
                        json!({"run_id":run_id, "locator":"session.jsonl"}),
                    ),
                    (
                        "permission.requested",
                        json!({"gate_id":"gate", "kind":"confirm", "title":"Allow?", "message":"Proceed?"}),
                    ),
                    ("run.needs_attention", json!({"reason":"interrupted"})),
                ]),
                Some("owner"),
            ),
            (
                events(vec![
                    (
                        "runtime.pi_session.bound",
                        json!({"run_id":run_id, "locator":"session.jsonl"}),
                    ),
                    (
                        "tool.effect.started",
                        json!({"effect_id":"effect", "display_name":"Command"}),
                    ),
                    ("run.needs_attention", json!({"reason":"interrupted"})),
                ]),
                Some("owner"),
            ),
            (
                events(vec![
                    (
                        "runtime.pi_session.bound",
                        json!({"run_id":run_id, "locator":"missing.jsonl"}),
                    ),
                    ("run.needs_attention", json!({"reason":"interrupted"})),
                ]),
                Some("owner"),
            ),
        ];

        for (events, subject) in rejected {
            let (_, state) = project_chat_with_state(&events).unwrap();
            assert!(!history_resumable(&events, &state, subject, &directory));
        }

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn thread_commands_are_scoped_by_the_first_events_actor() {
        keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
        let directory = std::env::temp_dir().join(format!("muniment-chat-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut journal = RunJournal::open(directory.join("runs.sqlite3")).unwrap();
        let run_a = "01900000-0000-7000-8000-000000000001";
        let run_a_reconciled = "01900000-0000-7000-8000-000000000002";
        let run_b = "01900000-0000-7000-8000-000000000003";
        let run_legacy = "01900000-0000-7000-8000-000000000004";

        journal
            .append_new_run(
                "workspace-a",
                &event_envelope(run_a, 1, "run.started", json!({}), Some("sub-a")),
            )
            .unwrap();
        append_test_event(
            &mut journal,
            run_a,
            2,
            "model.stream.delta",
            json!({"text": "private-a"}),
            Some("sub-a"),
        );
        append_test_event(
            &mut journal,
            run_a,
            3,
            "run.completed",
            json!({"receipt": {"owner": "sub-a"}}),
            Some("sub-a"),
        );
        journal
            .append_new_run(
                "workspace-a",
                &event_envelope(run_a_reconciled, 1, "run.started", json!({}), Some("sub-a")),
            )
            .unwrap();
        append_test_event(
            &mut journal,
            run_a_reconciled,
            2,
            "run.needs_attention",
            json!({"reason": "interrupted"}),
            None,
        );

        journal
            .append_new_run(
                "workspace-a",
                &event_envelope(run_b, 1, "run.started", json!({}), Some("sub-b")),
            )
            .unwrap();
        append_test_event(
            &mut journal,
            run_b,
            2,
            "model.stream.delta",
            json!({"text": "private-b"}),
            Some("sub-b"),
        );
        append_test_event(
            &mut journal,
            run_b,
            3,
            "run.completed",
            json!({"receipt": {"owner": "sub-b"}}),
            Some("sub-b"),
        );

        journal
            .append_new_run(
                "workspace-a",
                &event_envelope(run_legacy, 1, "run.started", json!({}), None),
            )
            .unwrap();
        append_test_event(
            &mut journal,
            run_legacy,
            2,
            "run.completed",
            json!({"receipt": {"legacy": true}}),
            None,
        );

        let sub_b_summaries =
            chat_thread_summaries_page(&mut journal, Some("sub-b"), 10, None).unwrap();
        let sub_b = sub_b_summaries
            .summaries
            .iter()
            .flat_map(|summary| {
                chat_thread_open_page(
                    &mut journal,
                    Some("sub-b"),
                    std::path::Path::new("."),
                    &summary.thread_id,
                    10,
                    None,
                )
                .unwrap()
                .entries
            })
            .collect::<Vec<_>>();
        assert_eq!(
            sub_b
                .iter()
                .map(|entry| entry.run_id.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([run_b, run_legacy])
        );
        assert!(sub_b.iter().all(|entry| !entry.text.contains("private-a")));
        assert!(sub_b.iter().all(|entry| {
            entry
                .receipt
                .as_ref()
                .and_then(|receipt| receipt.get("owner"))
                != Some(&json!("sub-a"))
        }));

        let sub_a_summaries =
            chat_thread_summaries_page(&mut journal, Some("sub-a"), 10, None).unwrap();
        let sub_a = sub_a_summaries
            .summaries
            .iter()
            .flat_map(|summary| {
                chat_thread_open_page(
                    &mut journal,
                    Some("sub-a"),
                    std::path::Path::new("."),
                    &summary.thread_id,
                    10,
                    None,
                )
                .unwrap()
                .entries
            })
            .collect::<Vec<_>>();
        assert_eq!(
            sub_a
                .iter()
                .map(|entry| entry.run_id.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([run_a, run_a_reconciled, run_legacy])
        );
        assert_eq!(
            sub_a
                .iter()
                .find(|entry| entry.run_id == run_a)
                .unwrap()
                .text,
            "private-a"
        );

        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn thread_summary_page_fills_across_foreign_core_page_boundaries() {
        let directory = std::env::temp_dir().join(format!("muniment-chat-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("runs.sqlite3");
        let runs = [
            ("01900000-0000-7000-8000-000000000021", "other"),
            ("01900000-0000-7000-8000-000000000022", "owner"),
            ("01900000-0000-7000-8000-000000000023", "other"),
            ("01900000-0000-7000-8000-000000000024", "owner"),
        ];
        let mut journal = RunJournal::open(&path).unwrap();
        for (index, (run_id, subject)) in runs.into_iter().enumerate() {
            let mut envelope = event_envelope(run_id, 1, "run.started", json!({}), Some(subject));
            envelope.recorded_at = format!("2026-01-{:02}T00:00:00Z", 4 - index);
            journal.append_new_run("workspace-a", &envelope).unwrap();
        }

        let page = chat_thread_summaries_page(&mut journal, Some("owner"), 2, None).unwrap();
        assert_eq!(page.summaries.len(), 2);
        assert!(page.next_cursor.is_none());

        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn thread_summary_page_exhausts_an_all_foreign_journal() {
        let directory = std::env::temp_dir().join(format!("muniment-chat-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("runs.sqlite3");
        let mut journal = RunJournal::open(&path).unwrap();
        for index in 0..3 {
            let run_id = format!("01900000-0000-7000-8000-{index:012x}");
            journal
                .append_new_run(
                    "workspace-a",
                    &event_envelope(&run_id, 1, "run.started", json!({}), Some("other")),
                )
                .unwrap();
        }

        let page = chat_thread_summaries_page(&mut journal, Some("owner"), 2, None).unwrap();
        assert!(page.summaries.is_empty());
        assert!(page.next_cursor.is_none());

        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn thread_summary_page_returns_advanced_cursor_at_scan_bound() {
        let directory = std::env::temp_dir().join(format!("muniment-chat-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("runs.sqlite3");
        let mut journal = RunJournal::open(&path).unwrap();
        for index in 0..(MAX_THREAD_SUMMARY_CORE_PAGES + 2) {
            let run_id = format!("01900000-0000-7000-8001-{index:012x}");
            let subject = if index == 0 || index == MAX_THREAD_SUMMARY_CORE_PAGES + 1 {
                "owner"
            } else {
                "other"
            };
            let mut envelope = event_envelope(&run_id, 1, "run.started", json!({}), Some(subject));
            envelope.recorded_at = format!("2026-01-01T00:{:02}:{:02}Z", index / 60, index % 60);
            journal.append_new_run("workspace-a", &envelope).unwrap();
        }

        let first = chat_thread_summaries_page(&mut journal, Some("owner"), 2, None).unwrap();
        assert_eq!(first.summaries.len(), 1);
        assert!(first.next_cursor.is_some());

        let second = chat_thread_summaries_page(
            &mut journal,
            Some("owner"),
            2,
            first.next_cursor.as_deref(),
        )
        .unwrap();
        assert_eq!(second.summaries.len(), 1);
        assert!(second.next_cursor.is_none());

        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn thread_history_pages_and_rejects_inaccessible_threads() {
        keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
        let directory = std::env::temp_dir().join(format!("muniment-chat-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("runs.sqlite3");
        let owner_runs = [
            "01900000-0000-7000-8000-000000000011",
            "01900000-0000-7000-8000-000000000012",
            "01900000-0000-7000-8000-000000000013",
            "01900000-0000-7000-8000-000000000014",
        ];
        let foreign_run = "01900000-0000-7000-8000-000000000015";
        let mut journal = RunJournal::open(&path).unwrap();
        for run_id in owner_runs {
            journal
                .append_new_run(
                    "workspace-a",
                    &event_envelope(run_id, 1, "run.started", json!({}), Some("owner")),
                )
                .unwrap();
        }
        journal
            .append_new_run(
                "workspace-a",
                &event_envelope(foreign_run, 1, "run.started", json!({}), Some("other")),
            )
            .unwrap();
        drop(journal);

        let connection = rusqlite::Connection::open(&path).unwrap();
        let thread_for = |run_id: &str| {
            connection
                .query_row(
                    "SELECT thread_id FROM run_threads WHERE run_id=?1",
                    [run_id],
                    |row| row.get::<_, String>(0),
                )
                .unwrap()
        };
        let open_thread = thread_for(owner_runs[0]);
        let merged_thread = thread_for(owner_runs[1]);
        let deleted_thread = thread_for(owner_runs[2]);
        let foreign_thread = thread_for(foreign_run);
        connection
            .execute(
                "UPDATE run_threads SET thread_id=?1,thread_run_ordinal=2 WHERE run_id=?2",
                rusqlite::params![open_thread, owner_runs[1]],
            )
            .unwrap();
        connection
            .execute(
                "DELETE FROM thread_events WHERE thread_id=?1",
                [merged_thread],
            )
            .unwrap();
        drop(connection);

        let mut journal = RunJournal::open(&path).unwrap();
        let first = chat_thread_summaries_page(&mut journal, Some("owner"), 1, None).unwrap();
        let second = chat_thread_summaries_page(
            &mut journal,
            Some("owner"),
            1,
            first.next_cursor.as_deref(),
        )
        .unwrap();
        let third = chat_thread_summaries_page(
            &mut journal,
            Some("owner"),
            1,
            second.next_cursor.as_deref(),
        )
        .unwrap();
        let fourth = chat_thread_summaries_page(
            &mut journal,
            Some("owner"),
            1,
            third.next_cursor.as_deref(),
        )
        .unwrap();
        let visible = [first, second, third, fourth]
            .into_iter()
            .flat_map(|page| page.summaries)
            .map(|summary| summary.thread_id)
            .collect::<BTreeSet<_>>();
        assert_eq!(visible.len(), 3);
        assert!(!visible.contains(&foreign_thread));

        let first_open = chat_thread_open_page(
            &mut journal,
            Some("owner"),
            &directory,
            &open_thread,
            1,
            None,
        )
        .unwrap();
        assert_eq!(first_open.entries[0].run_id, owner_runs[0]);
        let second_open = chat_thread_open_page(
            &mut journal,
            Some("owner"),
            &directory,
            &open_thread,
            1,
            first_open.next_cursor.as_deref(),
        )
        .unwrap();
        assert_eq!(second_open.entries[0].run_id, owner_runs[1]);
        assert!(second_open.next_cursor.is_none());

        let tracker = SessionThread::default();
        select_session_thread(&mut journal, &tracker, Some("owner"), &open_thread).unwrap();
        for thread_id in ["unknown", foreign_thread.as_str()] {
            let error =
                chat_thread_open_page(&mut journal, Some("owner"), &directory, thread_id, 1, None)
                    .err()
                    .unwrap();
            assert_eq!(error, "Conversation history is unavailable.");
            let error = select_session_thread(&mut journal, &tracker, Some("owner"), thread_id)
                .unwrap_err();
            assert_eq!(error, "Conversation history is unavailable.");
            assert_eq!(
                tracker.offered("workspace-a", Some("owner")),
                OfferedThread::Selected(open_thread.clone())
            );
        }
        journal
            .append_thread_deleted(
                1,
                &deleted_thread,
                &Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
                &Provenance {
                    source: "test".into(),
                    source_version: "1".into(),
                    actor_id: Some("owner".into()),
                    device_id: None,
                    rpc_request_id: None,
                    capability_versions: None,
                    extra: BTreeMap::new(),
                },
            )
            .unwrap();
        let error = chat_thread_open_page(
            &mut journal,
            Some("owner"),
            &directory,
            &deleted_thread,
            1,
            None,
        )
        .err()
        .unwrap();
        assert_eq!(error, "Conversation history is unavailable.");
        let error = select_session_thread(&mut journal, &tracker, Some("owner"), &deleted_thread)
            .unwrap_err();
        assert_eq!(error, "Conversation history is unavailable.");
        assert_eq!(
            tracker.offered("workspace-a", Some("owner")),
            OfferedThread::Selected(open_thread)
        );

        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn fresh_session_tracker_adopts_the_newest_owned_workspace_thread() {
        let directory =
            std::env::temp_dir().join(format!("muniment-session-adoption-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let database = directory.join("runs.sqlite3");
        let mut journal = RunJournal::open(&database).unwrap();
        let first_run = Uuid::now_v7().to_string();
        journal
            .append_new_run(
                "workspace-a",
                &event_envelope(&first_run, 1, "run.started", json!({}), Some("owner")),
            )
            .unwrap();
        drop(journal);
        let connection = rusqlite::Connection::open(&database).unwrap();
        let thread_id = connection
            .query_row(
                "SELECT thread_id FROM run_threads WHERE run_id=?1",
                [&first_run],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        drop(connection);
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(&database).unwrap(),
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let tracker = SessionThread::default();
        let second_run = Uuid::now_v7().to_string();

        prepare_new_run_with_session_thread(
            &storage,
            SessionThreadStart {
                tracker: &tracker,
                continue_existing: true,
            },
            &second_run,
            "workspace-a",
            Some("owner"),
            Vec::new(),
            None,
        )
        .unwrap();

        let connection = rusqlite::Connection::open(&database).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT thread_run_ordinal FROM run_threads \
                     WHERE run_id=?1 AND thread_id=?2",
                    (&second_run, &thread_id),
                    |row| row.get::<_, u64>(0),
                )
                .unwrap(),
            2
        );
        assert_eq!(
            tracker.offered("workspace-a", Some("owner")),
            OfferedThread::Selected(thread_id)
        );

        drop(connection);
        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn completed_fresh_choice_controls_the_next_unstamped_run() {
        let directory =
            std::env::temp_dir().join(format!("muniment-session-fresh-race-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let database = directory.join("runs.sqlite3");
        let existing_run = Uuid::now_v7().to_string();
        let mut journal = RunJournal::open(&database).unwrap();
        journal
            .append_new_run(
                "workspace-a",
                &event_envelope(&existing_run, 1, "run.started", json!({}), Some("owner")),
            )
            .unwrap();
        drop(journal);
        let connection = rusqlite::Connection::open(&database).unwrap();
        let existing_thread = connection
            .query_row(
                "SELECT thread_id FROM run_threads WHERE run_id=?1",
                [&existing_run],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        drop(connection);
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(&database).unwrap(),
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let tracker = SessionThread::default();

        std::thread::scope(|scope| {
            let storage_guard = storage.lock().unwrap();
            let (started, waiting) = std::sync::mpsc::channel();
            let (completed, completion) = std::sync::mpsc::channel();
            let command_storage = Arc::clone(&storage);
            let command_tracker = &tracker;
            let command = scope.spawn(move || {
                started.send(()).unwrap();
                let result = fresh_session_thread(&command_storage, command_tracker, Some("owner"));
                completed.send(()).unwrap();
                result
            });
            waiting.recv().unwrap();
            assert!(
                completion.recv_timeout(Duration::from_millis(100)).is_err(),
                "the fresh command must wait for run preparation"
            );
            tracker.record(existing_thread.clone(), "workspace-a", Some("owner"));
            drop(storage_guard);
            completion.recv().unwrap();
            command.join().unwrap().unwrap();
        });

        let fresh_run = Uuid::now_v7().to_string();
        prepare_new_run_with_session_thread(
            &storage,
            SessionThreadStart {
                tracker: &tracker,
                continue_existing: true,
            },
            &fresh_run,
            "workspace-a",
            Some("owner"),
            Vec::new(),
            None,
        )
        .unwrap();

        let connection = rusqlite::Connection::open(&database).unwrap();
        let fresh_thread = connection
            .query_row(
                "SELECT thread_id FROM run_threads WHERE run_id=?1",
                [&fresh_run],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        assert_ne!(fresh_thread, existing_thread);

        drop(connection);
        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn new_run_ingests_multiple_files_into_one_sequence_and_projector() {
        keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
        let directory =
            std::env::temp_dir().join(format!("muniment-attachments-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let first = directory.join("first.txt");
        let second = directory.join("second.bin");
        std::fs::write(&first, b"first attachment").unwrap();
        std::fs::write(&second, b"second attachment").unwrap();
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(directory.join("runs.sqlite3")).unwrap(),
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let run_id = Uuid::now_v7().to_string();

        let (seq, mut projector) = prepare_new_run(
            &storage,
            &run_id,
            "workspace-a",
            Some("owner"),
            vec![SelectedFile { path: first }, SelectedFile { path: second }],
            None,
        )
        .unwrap();
        assert_eq!(seq, 3);
        let mut storage = storage.lock().unwrap();
        assert!(storage
            .journal
            .run_belongs_to_workspace(&run_id, "workspace-a")
            .unwrap());
        assert!(!storage
            .journal
            .run_belongs_to_workspace(&run_id, "owner")
            .unwrap());
        assert_eq!(
            storage
                .journal
                .workspace_thread_summaries("workspace-a", 10, None)
                .unwrap()
                .summaries
                .len(),
            1
        );
        assert!(storage
            .journal
            .projected_thread_entries("workspace-a", &run_id, seq, 0, 10)
            .is_ok());
        assert!(storage
            .journal
            .projected_thread_entries("owner", &run_id, seq, 0, 10)
            .is_err());
        let events = storage.journal.events(&run_id).unwrap();
        assert_eq!(
            events
                .iter()
                .map(|event| event.event_type.as_str())
                .collect::<Vec<_>>(),
            [
                "run.started",
                "chat.attachment.ingested",
                "chat.attachment.ingested",
            ]
        );
        for event in &events[1..] {
            let EventPayload::Attachment { attachment } = &event.payload else {
                panic!("attachment payload")
            };
            storage.cas.verify(attachment.sha256()).unwrap();
        }
        let projection = projector.projection().unwrap();
        assert_eq!(
            projection
                .attachments
                .iter()
                .map(|attachment| attachment.display_name.as_str())
                .collect::<Vec<_>>(),
            ["first.txt", "second.bin"]
        );
        let public = serde_json::to_string(&chat_attachments(&projection.attachments)).unwrap();
        assert!(!public.contains("sha256"));
        assert!(!public.contains(directory.to_string_lossy().as_ref()));
        let summaries =
            chat_thread_summaries_page(&mut storage.journal, Some("owner"), 10, None).unwrap();
        let history = chat_thread_open_page(
            &mut storage.journal,
            Some("owner"),
            &directory,
            &summaries.summaries[0].thread_id,
            10,
            None,
        )
        .unwrap();
        assert_eq!(history.entries.len(), 1);
        assert_eq!(history.entries[0].attachments.len(), 2);
        let restored = serde_json::to_string(&history.entries).unwrap();
        assert!(!restored.contains("sha256"));
        assert!(!restored.contains(directory.to_string_lossy().as_ref()));
        let serialized = serde_json::to_string(&events).unwrap();
        assert!(!serialized.contains(directory.to_string_lossy().as_ref()));
        let next = event_envelope(
            &run_id,
            4,
            "assistant.delta",
            json!({"text":"ready"}),
            Some("owner"),
        );
        projector.apply(&next).unwrap();
        storage.journal.append(3, &next).unwrap();
        assert_eq!(
            storage
                .journal
                .events(&run_id)
                .unwrap()
                .last()
                .unwrap()
                .run_seq,
            4
        );
        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn startup_reconciles_only_non_terminal_runs_once() {
        let directory = std::env::temp_dir().join(format!("muniment-chat-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("runs.sqlite3");
        let interrupted = Uuid::now_v7().to_string();
        let completed = Uuid::now_v7().to_string();

        {
            let mut journal = RunJournal::open(&path).unwrap();
            append_test_event(
                &mut journal,
                &interrupted,
                1,
                "run.started",
                json!({}),
                None,
            );
            append_test_event(
                &mut journal,
                &interrupted,
                2,
                "runtime.pi_session.bound",
                json!({"run_id": interrupted, "locator": "session.jsonl"}),
                None,
            );
            append_test_event(
                &mut journal,
                &interrupted,
                3,
                "permission.requested",
                json!({"gate_id":"gate-1","kind":"confirm","title":"Allow?","message":"Proceed?"}),
                None,
            );
            let pending = project_chat(&journal.events(&interrupted).unwrap()).unwrap();
            assert_eq!(projection_phase(&pending.status), "pending-permission");
            assert_eq!(pending.pending_permission.unwrap().gate_id, "gate-1");
            append_test_event(&mut journal, &completed, 1, "run.started", json!({}), None);
            append_test_event(
                &mut journal,
                &completed,
                2,
                "run.completed",
                json!({}),
                None,
            );
        }

        {
            let mut reopened = RunJournal::open(&path).unwrap();
            reconcile_interrupted_runs(&mut reopened, &desktop_provenance(None));
            let interrupted_events = reopened.events(&interrupted).unwrap();
            assert_eq!(interrupted_events.len(), 4);
            assert_eq!(interrupted_events[3].event_type, "run.needs_attention");
            assert_eq!(interrupted_events[3].provenance.actor_id, None);
            let state = reduce(&interrupted_events).unwrap();
            assert!(matches!(state.status, RunStatus::NeedsAttention(_)));
            assert_eq!(state.pi_session.unwrap().locator, "session.jsonl");
            assert_eq!(reopened.events(&completed).unwrap().len(), 2);

            reconcile_interrupted_runs(&mut reopened, &desktop_provenance(None));
            assert_eq!(reopened.events(&interrupted).unwrap().len(), 4);
            assert_eq!(reopened.events(&completed).unwrap().len(), 2);
        }

        std::fs::remove_dir_all(directory).unwrap();
    }
}
