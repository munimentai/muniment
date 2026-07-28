use muniment_core::journal::reducer::{project_chat, RunStatus};
use muniment_core::journal::thread_summaries::ThreadSummary;
use muniment_core::journal::RunJournal;
use serde::Serialize;
use serde_json::Value;

use crate::auth;
use crate::chat::{
    chat_attachments, chat_pending_permission, chat_tool_activity, resumable_context,
    state_session_root, ChatAttachment, ChatPendingPermission, ChatState, ChatToolActivity,
    SharedStorage,
};
use crate::session_thread::SessionThread;

const MAX_THREAD_SUMMARY_CORE_PAGES: usize = 100;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub(crate) run_id: String,
    pub(crate) prompt: Option<String>,
    pub(crate) phase: String,
    pub(crate) text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) receipt: Option<Value>,
    pub(crate) tool_activity: Vec<ChatToolActivity>,
    pub(crate) attachments: Vec<ChatAttachment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) pending_permission: Option<ChatPendingPermission>,
    pub(crate) resumable: bool,
}

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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatThreadOpenPage {
    pub(crate) entries: Vec<HistoryEntry>,
    pub(crate) next_cursor: Option<String>,
}

fn load_prompt(run_id: &str, subject: Option<&str>) -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(
        crate::chat::PROMPT_SERVICE,
        &crate::chat::prompt_user(subject, run_id),
    )
    .map_err(|_| "Conversation history is unavailable.".to_string())?;
    match entry.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err("Conversation history is unavailable.".to_string()),
    }
}

pub(crate) fn projection_phase(status: &Option<RunStatus>) -> &'static str {
    match status {
        Some(RunStatus::Streaming) => "streaming",
        Some(RunStatus::Completed) => "complete",
        Some(RunStatus::Cancelled) => "cancelled",
        Some(RunStatus::Failed { .. }) => "failed",
        Some(RunStatus::NeedsAttention(_)) => "interrupted",
        Some(RunStatus::PendingPermission(_)) => "pending-permission",
        _ => "thinking",
    }
}

fn subject_owns_first_run(
    journal: &mut RunJournal,
    thread_id: &str,
    subject: Option<&str>,
) -> Result<bool, String> {
    let first_run = journal
        .thread_run_ids(thread_id, 1, None)
        .map_err(|_| "Conversation history is unavailable.".to_string())?
        .run_ids
        .into_iter()
        .next()
        .ok_or_else(|| "Conversation history is unavailable.".to_string())?;
    let first = journal
        .first_envelope(&first_run)
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    Ok(!matches!(
        first.provenance.actor_id.as_deref(),
        Some(owner) if Some(owner) != subject
    ))
}

pub(crate) fn newest_owned_workspace_thread(
    journal: &mut RunJournal,
    workspace: &str,
    subject: Option<&str>,
) -> Result<Option<String>, String> {
    let mut cursor = None;
    for _ in 0..MAX_THREAD_SUMMARY_CORE_PAGES {
        let page = journal
            .workspace_thread_summaries(workspace, 100, cursor.as_deref())
            .map_err(|_| "Conversation history is unavailable.".to_string())?;
        for summary in page.summaries {
            if subject_owns_first_run(journal, &summary.thread_id, subject)? {
                return Ok(Some(summary.thread_id));
            }
        }
        match page.next_cursor {
            Some(next_cursor) => cursor = Some(next_cursor),
            None => return Ok(None),
        }
    }
    Ok(None)
}

pub(crate) fn chat_thread_summaries_page(
    journal: &mut RunJournal,
    subject: Option<&str>,
    limit: usize,
    cursor: Option<&str>,
) -> Result<ChatThreadSummaryPage, String> {
    if !(1..=100).contains(&limit) {
        return Err("Conversation history is unavailable.".into());
    }
    let mut summaries = Vec::with_capacity(limit);
    let mut next_cursor = cursor.map(str::to_owned);
    for _ in 0..MAX_THREAD_SUMMARY_CORE_PAGES {
        let page = journal
            .thread_summaries(limit - summaries.len(), next_cursor.as_deref())
            .map_err(|_| "Conversation history is unavailable.".to_string())?;
        for summary in page.summaries {
            if subject_owns_first_run(journal, &summary.thread_id, subject)? {
                summaries.push(summary);
            }
        }
        next_cursor = page.next_cursor;
        if summaries.len() == limit || next_cursor.is_none() {
            break;
        }
    }
    Ok(ChatThreadSummaryPage {
        summaries: summaries
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
        next_cursor,
    })
}

fn project_history_entry(
    journal: &mut RunJournal,
    run_id: String,
    subject: Option<&str>,
    session_root: &std::path::Path,
) -> Result<HistoryEntry, String> {
    let events = journal
        .events(&run_id)
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    let projection =
        project_chat(&events).map_err(|_| "Conversation history is unavailable.".to_string())?;
    let resumable = resumable_context(&events, subject, session_root).is_ok();
    Ok(HistoryEntry {
        prompt: load_prompt(&run_id, subject)?,
        phase: projection_phase(&projection.status).into(),
        text: projection.text,
        receipt: projection.receipt,
        tool_activity: chat_tool_activity(&projection.tool_activity),
        attachments: chat_attachments(&projection.attachments),
        pending_permission: chat_pending_permission(projection.pending_permission),
        resumable,
        run_id,
    })
}

pub(crate) fn chat_thread_open_page(
    journal: &mut RunJournal,
    subject: Option<&str>,
    session_root: &std::path::Path,
    thread_id: &str,
    limit: usize,
    cursor: Option<&str>,
) -> Result<ChatThreadOpenPage, String> {
    if !subject_owns_first_run(journal, thread_id, subject)? {
        return Err("Conversation history is unavailable.".into());
    }
    let page = journal
        .thread_run_ids(thread_id, limit, cursor)
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    let entries = page
        .run_ids
        .into_iter()
        .map(|run_id| project_history_entry(journal, run_id, subject, session_root))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ChatThreadOpenPage {
        entries,
        next_cursor: page.next_cursor,
    })
}

pub(crate) fn select_session_thread(
    journal: &mut RunJournal,
    tracker: &SessionThread,
    subject: Option<&str>,
    thread_id: &str,
) -> Result<(), String> {
    if !subject_owns_first_run(journal, thread_id, subject)? {
        return Err("Conversation history is unavailable.".into());
    }
    tracker.select(thread_id.to_owned(), subject);
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
    chat_thread_open_page(
        &mut storage.journal,
        tokens.subject.as_deref(),
        &session_root,
        &thread_id,
        limit,
        cursor.as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::event_envelope;
    use crate::session_thread::OfferedThread;
    use chrono::{SecondsFormat, Utc};
    use muniment_core::journal::reducer::PermissionRequest;
    use muniment_core::journal::Provenance;
    use serde_json::json;
    use std::collections::{BTreeMap, BTreeSet};
    use uuid::Uuid;

    fn append_test_event(
        journal: &mut RunJournal,
        run_id: &str,
        seq: u64,
        kind: &str,
        payload: Value,
        subject: Option<&str>,
    ) {
        journal
            .append(
                seq - 1,
                &event_envelope(run_id, seq, kind, payload, subject),
            )
            .unwrap();
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
}
