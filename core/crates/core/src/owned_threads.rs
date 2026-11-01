use crate::journal::thread_summaries::{
    ThreadSummaryPage, ThreadSummaryScan, ThreadSummaryScanError,
};
use crate::journal::RunJournal;
use crate::thread_ownership::{subject_owns_first_run, ThreadOwnershipError};

pub const MAX_THREAD_SUMMARY_CORE_PAGES: usize = 100;
/// The most deleted threads a summary page names, so a client hides another
/// machine's copy of a thread this machine deleted.
pub const MAX_DELETED_THREADS_LISTED: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnedThreadsError {
    InvalidLimit,
    ThreadSummariesUnavailable(String),
    ThreadOwnershipUnavailable(ThreadOwnershipError),
}

fn scan_error(error: ThreadSummaryScanError<ThreadOwnershipError>) -> OwnedThreadsError {
    match error {
        ThreadSummaryScanError::List(error) => {
            OwnedThreadsError::ThreadSummariesUnavailable(error.to_string())
        }
        ThreadSummaryScanError::Keep(error) => OwnedThreadsError::ThreadOwnershipUnavailable(error),
    }
}

pub fn newest_owned_workspace_thread(
    journal: &mut RunJournal,
    workspace: &str,
    subject: Option<&str>,
) -> Result<Option<String>, OwnedThreadsError> {
    let scan = ThreadSummaryScan {
        limit: 1,
        page_size: Some(100),
        max_pages: MAX_THREAD_SUMMARY_CORE_PAGES,
    };
    let page = journal
        .scan_thread_summaries(Some(workspace), None, scan, |journal, thread_id| {
            subject_owns_first_run(journal, thread_id, subject)
        })
        .map_err(scan_error)?;
    Ok(page
        .summaries
        .into_iter()
        .next()
        .map(|summary| summary.thread_id))
}

pub fn chat_thread_summaries_page(
    journal: &mut RunJournal,
    subject: Option<&str>,
    limit: usize,
    cursor: Option<&str>,
) -> Result<ThreadSummaryPage, OwnedThreadsError> {
    if !(1..=100).contains(&limit) {
        return Err(OwnedThreadsError::InvalidLimit);
    }
    // Pages shrink to the rows still needed, so the scan bound matches a walk
    // of MAX_THREAD_SUMMARY_CORE_PAGES successive listing pages.
    let scan = ThreadSummaryScan {
        limit,
        page_size: None,
        max_pages: MAX_THREAD_SUMMARY_CORE_PAGES,
    };
    let mut page = journal
        .scan_thread_summaries(None, cursor, scan, |journal, thread_id| {
            Ok(listed(journal, thread_id)? && subject_owns_first_run(journal, thread_id, subject)?)
        })
        .map_err(scan_error)?;
    page.deleted_thread_ids = journal
        .deleted_thread_ids(MAX_DELETED_THREADS_LISTED)
        .map_err(|error| OwnedThreadsError::ThreadSummariesUnavailable(error.to_string()))?;
    Ok(page)
}

/// Whether a thread lists. A created thread with no run holds no message, so
/// a list leaves it out, and the shell shows the open fresh thread itself. A
/// thread another swarm member runs lists before its history arrives.
pub fn listed(journal: &mut RunJournal, thread_id: &str) -> Result<bool, ThreadOwnershipError> {
    let has_run = !journal
        .thread_run_ids(thread_id, 1, None)
        .map_err(|error| ThreadOwnershipError::ThreadRunsUnavailable(format!("{error:?}")))?
        .run_ids
        .is_empty();
    if has_run {
        return Ok(true);
    }
    Ok(journal
        .thread_events_after(thread_id, 0)
        .map_err(|error| ThreadOwnershipError::FirstEnvelopeUnavailable(error.to_string()))?
        .first()
        .is_some_and(|created| {
            created
                .provenance
                .extra
                .contains_key(crate::swarm::SWARM_ORIGIN)
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::{EventEnvelope, EventPayload, Provenance};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::time::Duration;
    use uuid::Uuid;

    fn journal_file() -> PathBuf {
        crate::state_root::test_root()
            .join(format!("muniment-owned-threads-{}.sqlite3", Uuid::new_v4()))
    }

    fn append_thread(journal: &mut RunJournal, run_id: &str, actor_id: &str, recorded_at: String) {
        let event = EventEnvelope {
            event_id: Uuid::now_v7().to_string(),
            run_id: run_id.into(),
            run_seq: 1,
            event_type: "run.started".into(),
            event_version: 1,
            envelope_version: 1,
            recorded_at,
            occurred_at: None,
            correlation_id: None,
            causation_id: None,
            payload: EventPayload::Inline {
                payload_json: json!({}),
            },
            provenance: Provenance {
                source: "test".into(),
                source_version: "1".into(),
                actor_id: Some(actor_id.into()),
                device_id: None,
                rpc_request_id: None,
                capability_versions: None,
                extra: BTreeMap::new(),
            },
            extra: BTreeMap::new(),
        };
        journal.append_new_run("workspace", &event).unwrap();
    }

    #[test]
    fn returns_an_owned_thread() {
        let path = journal_file();
        let mut journal =
            RunJournal::open_with_busy_timeout(&path, Duration::from_secs(60)).unwrap();
        append_thread(
            &mut journal,
            "01900000-0000-7000-8000-000000000001",
            "owner",
            "2026-08-05T00:00:00Z".into(),
        );

        let thread_id = newest_owned_workspace_thread(&mut journal, "workspace", Some("owner"))
            .unwrap()
            .unwrap();

        assert_eq!(
            subject_owns_first_run(&mut journal, &thread_id, Some("owner")),
            Ok(true)
        );
    }

    #[test]
    fn rejects_a_thread_owned_by_another_subject() {
        let path = journal_file();
        let mut journal =
            RunJournal::open_with_busy_timeout(&path, Duration::from_secs(60)).unwrap();
        append_thread(
            &mut journal,
            "01900000-0000-7000-8000-000000000001",
            "other",
            "2026-08-05T00:00:00Z".into(),
        );

        assert_eq!(
            newest_owned_workspace_thread(&mut journal, "workspace", Some("owner")),
            Ok(None)
        );
    }

    #[test]
    fn walks_across_more_than_one_summary_page() {
        let path = journal_file();
        let mut journal =
            RunJournal::open_with_busy_timeout(&path, Duration::from_secs(60)).unwrap();
        for index in 0..3 {
            append_thread(
                &mut journal,
                &format!("01900000-0000-7000-8000-{index:012x}"),
                if index == 2 { "owner" } else { "other" },
                format!("2026-08-05T00:00:0{}Z", 3 - index),
            );
        }

        let page = chat_thread_summaries_page(&mut journal, Some("owner"), 1, None).unwrap();

        assert_eq!(page.summaries.len(), 1);
        assert_eq!(
            subject_owns_first_run(&mut journal, &page.summaries[0].thread_id, Some("owner")),
            Ok(true)
        );
    }

    #[test]
    fn leaves_out_a_created_thread_that_never_received_a_message() {
        let path = journal_file();
        let mut journal =
            RunJournal::open_with_busy_timeout(&path, Duration::from_secs(60)).unwrap();
        append_thread(
            &mut journal,
            "01900000-0000-7000-8000-000000000001",
            "owner",
            "2026-08-05T00:00:00Z".into(),
        );
        let mut provenance = Provenance {
            source: "test".into(),
            source_version: "1".into(),
            actor_id: Some("owner".into()),
            device_id: None,
            rpc_request_id: None,
            capability_versions: None,
            extra: BTreeMap::new(),
        };
        provenance
            .extra
            .insert("attach_profile".into(), "desktop-owner".into());
        let empty = journal
            .create_thread("workspace", "2026-08-05T00:00:01Z", provenance)
            .unwrap();

        let page = chat_thread_summaries_page(&mut journal, Some("owner"), 20, None).unwrap();

        assert_eq!(page.summaries.len(), 1);
        assert_ne!(page.summaries[0].thread_id, empty);
        assert_eq!(
            subject_owns_first_run(&mut journal, &empty, Some("owner")),
            Ok(true)
        );
    }

    fn run_event(run_id: &str, recorded_at: &str) -> EventEnvelope {
        let mut provenance = replica_provenance();
        provenance.source = "test".into();
        EventEnvelope {
            event_id: Uuid::now_v7().to_string(),
            run_id: run_id.into(),
            run_seq: 1,
            event_type: "run.started".into(),
            event_version: 1,
            envelope_version: 1,
            recorded_at: recorded_at.into(),
            occurred_at: None,
            correlation_id: None,
            causation_id: None,
            payload: EventPayload::Inline {
                payload_json: json!({}),
            },
            provenance,
            extra: BTreeMap::new(),
        }
    }

    fn replica_provenance() -> Provenance {
        let mut provenance = Provenance {
            source: "muniment-swarm".into(),
            source_version: "1".into(),
            actor_id: Some("owner".into()),
            device_id: None,
            rpc_request_id: None,
            capability_versions: None,
            extra: BTreeMap::new(),
        };
        provenance
            .extra
            .insert("attach_profile".into(), "desktop-owner".into());
        provenance
    }

    #[test]
    fn names_the_threads_the_journal_deleted() {
        let path = journal_file();
        let mut journal =
            RunJournal::open_with_busy_timeout(&path, Duration::from_secs(60)).unwrap();
        let kept_run = "01900000-0000-7000-8000-000000000001";
        let deleted_run = "01900000-0000-7000-8000-000000000002";
        append_thread(
            &mut journal,
            kept_run,
            "owner",
            "2026-08-05T00:00:00Z".into(),
        );
        append_thread(
            &mut journal,
            deleted_run,
            "owner",
            "2026-08-05T00:00:01Z".into(),
        );
        let kept = journal.run_thread_id(kept_run).unwrap().unwrap();
        let deleted = journal.run_thread_id(deleted_run).unwrap().unwrap();
        crate::journal::thread_mutation::append_thread_delete_now(
            &mut journal,
            Some("owner"),
            &deleted,
            &replica_provenance(),
        )
        .unwrap();

        let page = chat_thread_summaries_page(&mut journal, Some("owner"), 20, None).unwrap();

        assert_eq!(
            page.summaries
                .iter()
                .map(|summary| summary.thread_id.as_str())
                .collect::<Vec<_>>(),
            [kept.as_str()]
        );
        assert_eq!(page.deleted_thread_ids, std::slice::from_ref(&deleted));
        assert!(journal.thread_deleted(&deleted).unwrap());
        assert!(!journal.thread_deleted(&kept).unwrap());
    }

    #[test]
    fn a_replica_lists_at_its_newest_message_not_its_arrival() {
        let path = journal_file();
        let mut journal =
            RunJournal::open_with_busy_timeout(&path, Duration::from_secs(60)).unwrap();
        // A copy made before replicas took their first event's time: its
        // creation records when it arrived, a day after its last message.
        let replica = "01900000-0000-7000-8000-0000000000aa";
        journal
            .create_thread_with_id(
                replica,
                "workspace",
                "2026-08-06T00:00:00Z",
                replica_provenance(),
            )
            .unwrap();
        journal
            .append_new_run_in_thread(
                "workspace",
                replica,
                &run_event(
                    "01900000-0000-7000-8000-0000000000ab",
                    "2026-08-05T00:00:00Z",
                ),
            )
            .unwrap();
        // A local thread written after the replica's last message.
        append_thread(
            &mut journal,
            "01900000-0000-7000-8000-000000000001",
            "owner",
            "2026-08-05T12:00:00Z".into(),
        );
        let local = journal
            .run_thread_id("01900000-0000-7000-8000-000000000001")
            .unwrap()
            .unwrap();
        // A thread that holds only its creation lists at it.
        let empty = "01900000-0000-7000-8000-0000000000ac";
        journal
            .create_thread_with_id(
                empty,
                "workspace",
                "2026-08-04T00:00:00Z",
                replica_provenance(),
            )
            .unwrap();

        let page = journal.thread_summaries(20, None).unwrap();
        let listed: Vec<(&str, &str)> = page
            .summaries
            .iter()
            .map(|summary| (summary.thread_id.as_str(), summary.updated_at.as_str()))
            .collect();
        assert_eq!(
            listed,
            [
                (local.as_str(), "2026-08-05T12:00:00Z"),
                (replica, "2026-08-05T00:00:00Z"),
                (empty, "2026-08-04T00:00:00Z"),
            ]
        );
        let held = journal.summaries_of_threads(&[replica.to_owned()]).unwrap();
        assert_eq!(held[0].updated_at, "2026-08-05T00:00:00Z");
        // A cursor taken at the replica reads on past it.
        let first = journal.thread_summaries(2, None).unwrap();
        let next = journal
            .thread_summaries(2, first.next_cursor.as_deref())
            .unwrap();
        assert_eq!(next.summaries[0].thread_id, empty);
    }

    #[test]
    fn rejects_limits_outside_one_through_one_hundred() {
        let path = journal_file();
        let mut journal =
            RunJournal::open_with_busy_timeout(&path, Duration::from_secs(60)).unwrap();

        assert_eq!(
            chat_thread_summaries_page(&mut journal, None, 0, None),
            Err(OwnedThreadsError::InvalidLimit)
        );
        assert_eq!(
            chat_thread_summaries_page(&mut journal, None, 101, None),
            Err(OwnedThreadsError::InvalidLimit)
        );
    }
}
