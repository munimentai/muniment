use chrono::{DateTime, Utc};
use muniment_core::journal::thread_mutation::{
    append_thread_delete, append_thread_delete_now, append_thread_rename, append_thread_rename_now,
    create_thread_now, delete_agent_creation_threads_now, delete_thread_with_creation_now,
    remove_orphan_creation_threads, ThreadMutationError,
};
use muniment_core::journal::{EventEnvelope, EventPayload, Provenance, RunJournal};
use rusqlite::Connection;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;
use uuid::Uuid;

const RECORDED_AT: &str = "2026-08-06T12:34:56Z";

fn provenance(actor_id: Option<&str>) -> Provenance {
    Provenance {
        source: "desktop-test".into(),
        source_version: "42".into(),
        actor_id: actor_id.map(str::to_owned),
        device_id: Some("device-1".into()),
        rpc_request_id: Some("request-1".into()),
        capability_versions: None,
        extra: BTreeMap::new(),
    }
}

fn attach_provenance() -> Provenance {
    let mut provenance = provenance(Some("owner"));
    provenance
        .extra
        .insert("attach_profile".into(), json!("profile-a"));
    provenance
}

fn run_started(actor_id: Option<&str>) -> EventEnvelope {
    EventEnvelope {
        event_id: Uuid::now_v7().to_string(),
        run_id: Uuid::now_v7().to_string(),
        run_seq: 1,
        event_type: "run.started".into(),
        event_version: 1,
        envelope_version: 1,
        recorded_at: "2026-08-06T12:00:00Z".into(),
        occurred_at: None,
        correlation_id: None,
        causation_id: None,
        payload: EventPayload::Inline {
            payload_json: json!({}),
        },
        provenance: provenance(actor_id),
        extra: BTreeMap::new(),
    }
}

fn journal_with_thread(actor_id: Option<&str>) -> (PathBuf, RunJournal, String) {
    let path = muniment_core::state_root::test_root().join(format!(
        "muniment-journal-thread-mutation-{}.sqlite3",
        Uuid::new_v4()
    ));
    let mut journal = RunJournal::open(&path).unwrap();
    let thread_id = journal
        .append_new_run("workspace-a", &run_started(actor_id))
        .unwrap();
    (path, journal, thread_id)
}

fn stored_thread_event(path: &PathBuf, thread_id: &str) -> (String, u64, String, String) {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT event_type, thread_seq, recorded_at, envelope_json
             FROM thread_events WHERE thread_id=?1 ORDER BY thread_seq DESC LIMIT 1",
            [thread_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap()
}

#[test]
fn creates_thread_now_and_reads_it_from_the_journal() {
    let path = muniment_core::state_root::test_root().join(format!(
        "muniment-journal-thread-create-{}.sqlite3",
        Uuid::new_v4()
    ));
    let mut journal = RunJournal::open(&path).unwrap();
    let before = Utc::now();

    let thread_id = create_thread_now(&mut journal, "workspace-a", attach_provenance()).unwrap();
    let after = Utc::now();

    let events = journal.thread_events(&thread_id).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "thread.created");
    let recorded_at = stored_thread_event(&path, &thread_id).2;
    let parsed = DateTime::parse_from_rfc3339(&recorded_at).unwrap();
    assert!(recorded_at.ends_with('Z'));
    assert!(parsed >= before && parsed <= after);
}

#[test]
fn create_thread_now_rejects_invalid_workspace_or_provenance() {
    let path = muniment_core::state_root::test_root().join(format!(
        "muniment-journal-thread-create-invalid-{}.sqlite3",
        Uuid::new_v4()
    ));
    let mut journal = RunJournal::open(path).unwrap();

    assert!(create_thread_now(&mut journal, "", attach_provenance()).is_err());
    assert!(create_thread_now(&mut journal, "workspace-a", provenance(Some("owner"))).is_err());
}

#[test]
fn appends_rename_with_caller_fields() {
    let (path, mut journal, thread_id) = journal_with_thread(Some("owner"));
    let caller_provenance = provenance(Some("owner"));

    append_thread_rename(
        &mut journal,
        Some("owner"),
        &thread_id,
        "  New title  ",
        RECORDED_AT,
        &caller_provenance,
    )
    .unwrap();

    let event = stored_thread_event(&path, &thread_id);
    assert_eq!(event.0, "thread.title.renamed");
    assert_eq!(event.1, 2);
    assert_eq!(event.2, RECORDED_AT);
    let envelope: serde_json::Value = serde_json::from_str(&event.3).unwrap();
    assert_eq!(envelope["payload_json"], json!({"title": "New title"}));
    assert_eq!(
        serde_json::from_value::<Provenance>(envelope["provenance"].clone()).unwrap(),
        caller_provenance
    );
}

#[test]
fn appends_delete_with_caller_fields() {
    let (path, mut journal, thread_id) = journal_with_thread(Some("owner"));
    let caller_provenance = provenance(Some("owner"));

    append_thread_delete(
        &mut journal,
        Some("owner"),
        &thread_id,
        RECORDED_AT,
        &caller_provenance,
    )
    .unwrap();

    let event = stored_thread_event(&path, &thread_id);
    assert_eq!(event.0, "thread.deleted");
    assert_eq!(event.1, 2);
    assert_eq!(event.2, RECORDED_AT);
    let envelope: serde_json::Value = serde_json::from_str(&event.3).unwrap();
    assert_eq!(envelope["payload_json"], json!({}));
    assert_eq!(
        serde_json::from_value::<Provenance>(envelope["provenance"].clone()).unwrap(),
        caller_provenance
    );
}

#[test]
fn rename_now_stores_a_generated_utc_rfc3339_timestamp() {
    let (path, mut journal, thread_id) = journal_with_thread(Some("owner"));
    let before = Utc::now();

    append_thread_rename_now(
        &mut journal,
        Some("owner"),
        &thread_id,
        "New title",
        &provenance(Some("owner")),
    )
    .unwrap();
    let after = Utc::now();

    let recorded_at = stored_thread_event(&path, &thread_id).2;
    let parsed = DateTime::parse_from_rfc3339(&recorded_at).unwrap();
    assert_eq!(parsed.offset().local_minus_utc(), 0);
    assert!(recorded_at.ends_with('Z'));
    assert!(parsed >= before && parsed <= after);
}

#[test]
fn delete_now_stores_a_generated_utc_rfc3339_timestamp() {
    let (path, mut journal, thread_id) = journal_with_thread(Some("owner"));
    let before = Utc::now();

    append_thread_delete_now(
        &mut journal,
        Some("owner"),
        &thread_id,
        &provenance(Some("owner")),
    )
    .unwrap();
    let after = Utc::now();

    let recorded_at = stored_thread_event(&path, &thread_id).2;
    let parsed = DateTime::parse_from_rfc3339(&recorded_at).unwrap();
    assert_eq!(parsed.offset().local_minus_utc(), 0);
    assert!(recorded_at.ends_with('Z'));
    assert!(parsed >= before && parsed <= after);
}

#[test]
fn rejects_rename_by_non_owner() {
    let (_path, mut journal, thread_id) = journal_with_thread(Some("owner"));

    assert!(matches!(
        append_thread_rename(
            &mut journal,
            Some("other"),
            &thread_id,
            "New title",
            RECORDED_AT,
            &provenance(Some("other")),
        ),
        Err(ThreadMutationError::NotOwned)
    ));
    assert_eq!(journal.last_thread_seq(&thread_id).unwrap(), 1);
}

#[test]
fn rejects_delete_by_non_owner() {
    let (_path, mut journal, thread_id) = journal_with_thread(Some("owner"));

    assert!(matches!(
        append_thread_delete(
            &mut journal,
            Some("other"),
            &thread_id,
            RECORDED_AT,
            &provenance(Some("other")),
        ),
        Err(ThreadMutationError::NotOwned)
    ));
    assert_eq!(journal.last_thread_seq(&thread_id).unwrap(), 1);
}

#[test]
fn permits_mutations_when_the_first_run_has_no_recorded_subject() {
    let (_path, mut journal, thread_id) = journal_with_thread(None);
    let caller_provenance = provenance(Some("subject"));

    append_thread_rename(
        &mut journal,
        Some("subject"),
        &thread_id,
        "New title",
        RECORDED_AT,
        &caller_provenance,
    )
    .unwrap();
    append_thread_delete(
        &mut journal,
        Some("subject"),
        &thread_id,
        RECORDED_AT,
        &caller_provenance,
    )
    .unwrap();

    assert_eq!(journal.last_thread_seq(&thread_id).unwrap(), 3);
}

#[test]
fn first_message_name_persists_and_never_overwrites_a_manual_name() {
    use muniment_core::journal::thread_mutation::{
        first_thread_needs_name, save_first_thread_name,
    };
    let (path, mut journal, thread) = journal_with_thread(Some("owner"));
    let run = journal.thread_run_ids(&thread, 1, None).unwrap().run_ids[0].clone();
    assert!(save_first_thread_name(&mut journal, &run, "Help with report", false).unwrap());
    assert_eq!(
        first_thread_needs_name(&mut journal, &run),
        Some(thread.clone())
    );
    assert!(!save_first_thread_name(
        &mut journal,
        &run,
        "This title is far too long to keep",
        true
    )
    .unwrap());
    assert!(save_first_thread_name(&mut journal, &run, "Report repair", true).unwrap());
    assert!(first_thread_needs_name(&mut journal, &run).is_none());
    drop(journal);
    let mut journal = RunJournal::open(&path).unwrap();
    let events = journal.thread_events(&thread).unwrap();
    assert!(serde_json::to_string(events.last().unwrap())
        .unwrap()
        .contains("Report repair"));
    append_thread_rename_now(
        &mut journal,
        Some("owner"),
        &thread,
        "My title",
        &provenance(Some("owner")),
    )
    .unwrap();
    assert!(!save_first_thread_name(&mut journal, &run, "Late name", true).unwrap());
}

#[test]
fn a_manual_name_during_generation_and_later_messages_block_automatic_names() {
    use muniment_core::journal::thread_mutation::save_first_thread_name;
    let (_, mut journal, thread) = journal_with_thread(Some("owner"));
    let first = journal.thread_run_ids(&thread, 1, None).unwrap().run_ids[0].clone();
    assert!(save_first_thread_name(&mut journal, &first, "Temporary title", false).unwrap());
    let later = run_started(Some("owner"));
    journal
        .append_new_run_in_thread("workspace-a", &thread, &later)
        .unwrap();
    assert!(!save_first_thread_name(&mut journal, &later.run_id, "Wrong topic", true).unwrap());
    append_thread_rename_now(
        &mut journal,
        Some("owner"),
        &thread,
        "Chosen name",
        &provenance(Some("owner")),
    )
    .unwrap();
    assert!(!save_first_thread_name(&mut journal, &first, "Late title", true).unwrap());
}

#[test]
fn a_prompt_fallback_name_keeps_whole_words_and_marks_a_cut() {
    use muniment_core::journal::thread_mutation::fallback_thread_name;
    assert_eq!(fallback_thread_name("Say hi in French"), "Say hi in French");
    assert_eq!(
        fallback_thread_name("Make a small HTML page that shows a countdown to New Year"),
        "Make a small HTML page that…"
    );
    assert_eq!(
        fallback_thread_name("Summarize   the\nquarterly report, please"),
        "Summarize the quarterly report, please"
    );
    assert_eq!(
        fallback_thread_name("Compare these: alpha, beta, gamma, delta, epsilon"),
        "Compare these: alpha, beta, gamma, delta…"
    );
    let long = "x".repeat(60);
    assert_eq!(fallback_thread_name(&long), format!("{}…", "x".repeat(47)));
    assert_eq!(fallback_thread_name("   "), "");
}

fn bound(run: &EventEnvelope, locator: &str) -> EventEnvelope {
    EventEnvelope {
        event_id: Uuid::now_v7().to_string(),
        run_seq: 2,
        event_type: "runtime.pi_session.bound".into(),
        payload: EventPayload::Inline {
            payload_json: json!({"run_id": run.run_id, "locator": locator}),
        },
        ..run.clone()
    }
}

#[test]
fn a_rewind_takes_a_run_and_later_runs_out_of_the_conversation() {
    use muniment_core::journal::thread_mutation::append_thread_rewind_now;
    let (_, mut journal, thread) = journal_with_thread(Some("owner"));
    let first = journal.thread_run_ids(&thread, 1, None).unwrap().run_ids[0].clone();
    let first_events = journal.events(&first).unwrap();
    journal
        .append(1, &bound(&first_events[0], "thread.jsonl"))
        .unwrap();
    let mut later = Vec::new();
    for _ in 0..2 {
        let run = run_started(Some("owner"));
        journal
            .append_new_run_in_thread("workspace-a", &thread, &run)
            .unwrap();
        journal.append(1, &bound(&run, "thread.jsonl")).unwrap();
        later.push(run.run_id);
    }
    assert_eq!(
        append_thread_rewind_now(
            &mut journal,
            Some("other"),
            &thread,
            &later[0],
            &provenance(Some("other")),
        )
        .err()
        .map(|error| matches!(error, ThreadMutationError::NotOwned)),
        Some(true)
    );

    let taken = append_thread_rewind_now(
        &mut journal,
        Some("owner"),
        &thread,
        &later[0],
        &provenance(Some("owner")),
    )
    .unwrap();
    assert_eq!(taken, later);
    assert_eq!(
        journal.thread_run_ids(&thread, 10, None).unwrap().run_ids,
        std::slice::from_ref(&first)
    );
    assert_eq!(
        journal
            .thread_session_binding(&thread)
            .unwrap()
            .unwrap()
            .run_id,
        first
    );
    let next = run_started(Some("owner"));
    journal
        .append_new_run_in_thread("workspace-a", &thread, &next)
        .unwrap();
    assert_eq!(
        journal
            .previous_thread_session_binding(&next.run_id)
            .unwrap()
            .unwrap()
            .run_id,
        first
    );
    assert_eq!(
        journal.thread_run_ids(&thread, 10, None).unwrap().run_ids,
        [first, next.run_id]
    );
    assert_eq!(journal.events(&later[0]).unwrap().len(), 2);
}

#[test]
fn deletes_an_agent_creation_chat_that_holds_no_run() {
    let profile = muniment_core::state_root::test_root()
        .join(format!("muniment-creation-delete-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&profile).unwrap();
    let (path, mut journal, owned) = journal_with_thread(Some("owner"));
    let chat = Uuid::new_v4().to_string();
    for thread_id in [&chat, &owned] {
        muniment_core::creations::save(
            &profile,
            muniment_core::creations::Creation {
                thread_id: thread_id.clone(),
                kind: "agent".into(),
                goal: "Find sources".into(),
                output: "A saved agent".into(),
                result_id: None,
                turns: Vec::new(),
                title: None,
            },
        )
        .unwrap();
    }
    delete_thread_with_creation_now(
        &mut journal,
        &profile,
        Some("owner"),
        &chat,
        &provenance(Some("owner")),
    )
    .unwrap();
    assert!(muniment_core::creations::read(&profile, &chat)
        .unwrap()
        .is_none());
    delete_thread_with_creation_now(
        &mut journal,
        &profile,
        Some("owner"),
        &owned,
        &provenance(Some("owner")),
    )
    .unwrap();
    assert_eq!(stored_thread_event(&path, &owned).0, "thread.deleted");
    assert!(muniment_core::creations::read(&profile, &owned)
        .unwrap()
        .is_none());
    // A thread with neither a run nor a creation chat is still not found.
    assert!(delete_thread_with_creation_now(
        &mut journal,
        &profile,
        Some("owner"),
        &Uuid::new_v4().to_string(),
        &provenance(Some("owner")),
    )
    .is_err());
    std::fs::remove_dir_all(profile).unwrap();
}

#[test]
fn a_deleted_threads_session_folder_goes_unless_it_holds_the_users_files() {
    let root = muniment_core::state_root::test_root()
        .join(format!("muniment-thread-folder-delete-{}", Uuid::new_v4()));
    let profile = root.join("profile");
    std::fs::create_dir_all(&profile).unwrap();
    muniment_core::home::confirm_home(&profile, &root.join("muniment")).unwrap();
    let mut journal = RunJournal::open(profile.join("runs.sqlite3")).unwrap();
    let mut thread = || {
        let thread = journal
            .append_new_run("workspace-a", &run_started(Some("owner")))
            .unwrap();
        let folder = muniment_core::projects::workspace(&profile, &thread).unwrap();
        std::fs::create_dir_all(folder.join(".muniment/tasks/run-1")).unwrap();
        std::fs::write(folder.join(".muniment/tasks/run-1/output.txt"), "done").unwrap();
        (thread, folder)
    };
    let (internal, internal_folder) = thread();
    let (mine, mine_folder) = thread();
    std::fs::write(mine_folder.join("report.md"), "mine").unwrap();
    let (swept, swept_folder) = thread();
    let (live, live_folder) = thread();

    // A delete removes the folder that holds only Muniment's files, and
    // answers the folder that stays with the user's files.
    let mut kept = Vec::new();
    for thread in [&internal, &mine] {
        kept.push(
            delete_thread_with_creation_now(
                &mut journal,
                &profile,
                Some("owner"),
                thread,
                &provenance(Some("owner")),
            )
            .unwrap(),
        );
    }
    assert_eq!(kept, [None, Some(mine_folder.clone())]);
    assert!(!internal_folder.exists());
    assert_eq!(
        std::fs::read_to_string(mine_folder.join("report.md")).unwrap(),
        "mine"
    );

    // A delete that left its folder, such as one a synced machine sent, is
    // swept at start, and a live thread's folder stays.
    append_thread_delete_now(
        &mut journal,
        Some("owner"),
        &swept,
        &provenance(Some("owner")),
    )
    .unwrap();
    assert!(swept_folder.exists());
    drop(journal);
    let removed = muniment_core::projects::remove_deleted_thread_folders(&profile).unwrap();
    assert_eq!(
        removed,
        [swept_folder
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()]
    );
    assert!(!swept_folder.exists());
    assert!(mine_folder.join("report.md").exists());
    assert!(live_folder
        .join(".muniment/tasks/run-1/output.txt")
        .exists());
    assert_eq!(
        muniment_core::projects::workspace(&profile, &live).unwrap(),
        live_folder
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_deleted_thread_leaves_its_project_and_the_start_pass_drops_the_ones_left() {
    let root = muniment_core::state_root::test_root()
        .join(format!("muniment-thread-project-delete-{}", Uuid::new_v4()));
    let profile = root.join("profile");
    std::fs::create_dir_all(&profile).unwrap();
    muniment_core::home::confirm_home(&profile, &root.join("muniment")).unwrap();
    let project = muniment_core::projects::create(&profile, "Research").unwrap();
    let mut journal = RunJournal::open(profile.join("runs.sqlite3")).unwrap();
    let mut thread = || {
        let thread = journal
            .append_new_run("workspace-a", &run_started(Some("owner")))
            .unwrap();
        muniment_core::projects::assign(&profile, &thread, &project).unwrap();
        thread
    };
    let (deleted, synced, live) = (thread(), thread(), thread());
    let mapped = |thread: &str| {
        muniment_core::projects::list(&profile)
            .unwrap()
            .threads
            .contains_key(thread)
    };

    delete_thread_with_creation_now(
        &mut journal,
        &profile,
        Some("owner"),
        &deleted,
        &provenance(Some("owner")),
    )
    .unwrap();
    assert!(!mapped(&deleted));

    // A delete that left its mapping, such as one a synced machine sent,
    // is dropped at start, and a live thread keeps its project.
    append_thread_delete_now(
        &mut journal,
        Some("owner"),
        &synced,
        &provenance(Some("owner")),
    )
    .unwrap();
    assert!(mapped(&synced));
    drop(journal);
    assert_eq!(
        muniment_core::projects::forget_deleted_threads(&profile).unwrap(),
        1
    );
    assert!(!mapped(&synced));
    assert!(mapped(&live));
    assert_eq!(
        muniment_core::projects::forget_deleted_threads(&profile).unwrap(),
        0
    );
    std::fs::remove_dir_all(root).unwrap();
}

fn save_creation(profile: &std::path::Path, thread_id: &str, agent: &str) {
    muniment_core::creations::save(
        profile,
        muniment_core::creations::Creation {
            thread_id: thread_id.to_owned(),
            kind: "agent".into(),
            goal: "Find sources".into(),
            output: "A saved agent".into(),
            result_id: Some(agent.to_owned()),
            turns: Vec::new(),
            title: None,
        },
    )
    .unwrap();
}

fn thread_deleted(journal: &mut RunJournal, thread_id: &str) -> bool {
    journal
        .thread_events(thread_id)
        .unwrap()
        .iter()
        .any(|event| event.event_type == "thread.deleted")
}

#[test]
fn a_deleted_agents_creation_chats_go_and_the_start_sweep_removes_the_ones_left() {
    let root = muniment_core::state_root::test_root()
        .join(format!("muniment-creation-sweep-{}", Uuid::new_v4()));
    let profile = root.join("profile");
    std::fs::create_dir_all(&profile).unwrap();
    muniment_core::home::confirm_home(&profile, &root.join("muniment")).unwrap();
    let mut journal = RunJournal::open(profile.join("runs.sqlite3")).unwrap();
    // A thread created a long time ago that holds only its creation.
    let created_long_ago = |journal: &mut RunJournal, origin: Option<&str>| {
        let mut provenance = attach_provenance();
        if let Some(origin) = origin {
            provenance
                .extra
                .insert("swarm_origin".into(), json!(origin));
        }
        journal
            .create_thread("workspace-a", RECORDED_AT, provenance)
            .unwrap()
    };

    // An agent's delete takes each of its creation chats, one with a run
    // and one without, and leaves the user's other threads.
    let deleted_agent = Uuid::new_v4().to_string();
    let chat_with_run = journal
        .append_new_run("workspace-a", &run_started(Some("owner")))
        .unwrap();
    let chat = created_long_ago(&mut journal, None);
    let other = journal
        .append_new_run("workspace-a", &run_started(Some("owner")))
        .unwrap();
    save_creation(&profile, &chat_with_run, &deleted_agent);
    save_creation(&profile, &chat, &deleted_agent);
    let mut threads = muniment_core::agents::creation_threads(&profile, &deleted_agent);
    threads.sort();
    let mut expected = vec![chat_with_run.clone(), chat.clone()];
    expected.sort();
    assert_eq!(threads, expected);
    let mut deleted =
        delete_agent_creation_threads_now(&mut journal, &profile, &threads, &provenance(None))
            .unwrap();
    deleted.sort();
    assert_eq!(deleted, expected);
    for thread in &expected {
        assert!(thread_deleted(&mut journal, thread));
        assert!(muniment_core::creations::read(&profile, thread)
            .unwrap()
            .is_none());
    }
    assert!(!thread_deleted(&mut journal, &other));

    // At start, a creation chat whose agent is gone and that holds no run
    // goes, and so does an old thread that holds only its creation and that
    // nothing names. A chat with a run, a fresh thread, a replica, a renamed
    // thread and an agent's primary thread stay.
    let gone_agent = Uuid::new_v4().to_string();
    let left_chat = created_long_ago(&mut journal, None);
    save_creation(&profile, &left_chat, &gone_agent);
    let left_with_run = journal
        .append_new_run("workspace-a", &run_started(Some("owner")))
        .unwrap();
    save_creation(&profile, &left_with_run, &gone_agent);
    let bare = created_long_ago(&mut journal, None);
    let fresh = create_thread_now(&mut journal, "workspace-a", attach_provenance()).unwrap();
    let replica = created_long_ago(&mut journal, Some("member-b"));
    let renamed = created_long_ago(&mut journal, None);
    append_thread_rename_now(
        &mut journal,
        Some("owner"),
        &renamed,
        "Plans",
        &attach_provenance(),
    )
    .unwrap();
    let primary = created_long_ago(&mut journal, None);
    muniment_core::agents::with_state(&profile, |state| {
        state
            .primary_threads
            .insert(Uuid::new_v4().to_string(), primary.clone());
        Ok(())
    })
    .unwrap();

    let mut removed =
        remove_orphan_creation_threads(&mut journal, &profile, &provenance(None)).unwrap();
    removed.sort();
    let mut expected = vec![left_chat.clone(), bare.clone()];
    expected.sort();
    assert_eq!(removed, expected);
    assert!(muniment_core::creations::read(&profile, &left_chat)
        .unwrap()
        .is_none());
    for kept in [&left_with_run, &fresh, &replica, &renamed, &primary, &other] {
        assert!(!thread_deleted(&mut journal, kept), "{kept}");
    }
    assert!(muniment_core::creations::read(&profile, &left_with_run)
        .unwrap()
        .is_some());
    assert!(
        remove_orphan_creation_threads(&mut journal, &profile, &provenance(None))
            .unwrap()
            .is_empty()
    );
    // Each delete names the profile its thread was created with, and the
    // journal opens again after the sweep.
    for thread in [&chat, &left_chat, &bare] {
        let events = journal.thread_events(thread).unwrap();
        let deleted = events.last().unwrap();
        assert_eq!(deleted.event_type, "thread.deleted");
        assert_eq!(
            deleted.provenance.extra.get("attach_profile"),
            Some(&json!("profile-a"))
        );
    }
    drop(journal);
    let mut journal = RunJournal::open(profile.join("runs.sqlite3")).unwrap();
    assert!(thread_deleted(&mut journal, &left_chat));
    drop(journal);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_journal_opens_when_a_runless_threads_delete_names_no_attach_profile() {
    // The startup sweep once deleted run-less creation chats with the
    // runtime's own provenance, which names no attach profile, and the
    // next open rejected the journal.
    let root = muniment_core::state_root::test_root()
        .join(format!("muniment-unstamped-delete-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("runs.sqlite3");
    let mut journal = RunJournal::open(&path).unwrap();
    let thread = create_thread_now(&mut journal, "workspace-a", attach_provenance()).unwrap();
    let kept = create_thread_now(&mut journal, "workspace-a", attach_provenance()).unwrap();
    journal
        .append_thread_deleted(1, &thread, RECORDED_AT, &provenance(None))
        .unwrap();
    drop(journal);

    let mut journal = RunJournal::open(&path).unwrap();
    assert!(thread_deleted(&mut journal, &thread));
    assert!(!thread_deleted(&mut journal, &kept));
    // Any other run-less event without a profile still fails the open.
    journal
        .append_thread_title_renamed(1, &kept, "Plans", RECORDED_AT, &provenance(None))
        .unwrap();
    drop(journal);
    assert!(RunJournal::open(&path).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_start_pass_removes_day_old_folders_unsent_chats_left_and_keeps_the_users_files() {
    let root = muniment_core::state_root::test_root()
        .join(format!("muniment-unsent-folders-{}", Uuid::new_v4()));
    let profile = root.join("profile");
    std::fs::create_dir_all(&profile).unwrap();
    let home = root.join("muniment");
    muniment_core::home::confirm_home(&profile, &home).unwrap();
    let sessions = home.join("sessions");
    let mut journal = RunJournal::open(profile.join("runs.sqlite3")).unwrap();
    let folder = |thread: &str, title: &str| {
        muniment_core::workspace_names::resolve(&profile, &sessions, thread, Some(title), None)
            .unwrap()
    };
    let leaf = |path: &std::path::Path| path.file_name().unwrap().to_string_lossy().into_owned();
    // An unsent thread and an unsent agent chat opened Files and left
    // empty folders, one with Finder's file.
    let unsent_thread = folder(&Uuid::new_v4().to_string(), "New thread");
    std::fs::write(unsent_thread.join(".DS_Store"), "").unwrap();
    let unsent_agent = folder(&Uuid::new_v4().to_string(), "New agent");
    // An unsent chat's folder that holds the user's file stays.
    let mine = folder(&Uuid::new_v4().to_string(), "New thread");
    std::fs::write(mine.join("notes.md"), "mine").unwrap();
    // A thread that ran keeps its empty folder.
    let ran = journal
        .append_new_run("workspace-a", &run_started(Some("owner")))
        .unwrap();
    let ran_folder = folder(&ran, "New thread");
    // A draft that holds turns keeps its folder.
    let drafted = Uuid::new_v4().to_string();
    muniment_core::creations::save(
        &profile,
        muniment_core::creations::Creation {
            thread_id: drafted.clone(),
            kind: "agent".into(),
            goal: "Find sources".into(),
            output: "A saved agent".into(),
            result_id: None,
            turns: vec![muniment_core::creations::DraftTurn {
                message: "Find sources".into(),
                at: RECORDED_AT.into(),
                ..Default::default()
            }],
            title: None,
        },
    )
    .unwrap();
    let drafted_folder = folder(&drafted, "New agent");
    // A titled thread's folder and an unrecorded empty folder.
    let titled = folder(&Uuid::new_v4().to_string(), "Sky color");
    let orphan = sessions.join("New_agent-0b3e9d21");
    std::fs::create_dir_all(&orphan).unwrap();
    drop(journal);

    // A folder younger than a day stays, since its chat may still be open.
    let past = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    assert!(
        muniment_core::projects::remove_unsent_thread_folders_before(&profile, past)
            .unwrap()
            .is_empty()
    );
    assert!(unsent_thread.is_dir() && unsent_agent.is_dir() && orphan.is_dir());

    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
    let mut removed =
        muniment_core::projects::remove_unsent_thread_folders_before(&profile, later).unwrap();
    removed.sort();
    let mut expected = vec![leaf(&unsent_thread), leaf(&unsent_agent), leaf(&orphan)];
    expected.sort();
    assert_eq!(removed, expected);
    assert!(!unsent_thread.exists() && !unsent_agent.exists() && !orphan.exists());
    assert_eq!(
        std::fs::read_to_string(mine.join("notes.md")).unwrap(),
        "mine"
    );
    assert!(ran_folder.is_dir() && drafted_folder.is_dir() && titled.is_dir());
    // The removed folders' name records go with them.
    let names = muniment_core::workspace_names::recorded_names(&profile, &sessions).unwrap();
    assert!(!names.contains(&leaf(&unsent_thread)) && !names.contains(&leaf(&unsent_agent)));
    assert!(names.contains(&leaf(&mine)));
    std::fs::remove_dir_all(root).unwrap();
}

/// Deletes agent `id` the way the app does: core's delete, then its creation
/// chats, then, with "Also delete its threads", each thread the agent holds.
fn delete_agent_as_the_app_does(
    journal: &mut RunJournal,
    profile: &std::path::Path,
    id: &str,
    remove_memory: bool,
    remove_threads: bool,
) {
    let creation_threads = muniment_core::agents::creation_threads(profile, id);
    let agent_threads: Vec<String> = muniment_core::agents::state(profile)
        .unwrap()
        .threads
        .into_iter()
        .filter(|(_, agent)| agent == id)
        .map(|(thread, _)| thread)
        .collect();
    muniment_core::agents::operation(
        profile,
        "agent.delete",
        json!({"id": id, "remove_memory": remove_memory}),
    )
    .unwrap();
    delete_agent_creation_threads_now(journal, profile, &creation_threads, &provenance(None))
        .unwrap();
    if remove_threads {
        for thread in agent_threads {
            delete_thread_with_creation_now(
                journal,
                profile,
                Some("owner"),
                &thread,
                &provenance(Some("owner")),
            )
            .unwrap();
        }
    }
}

#[test]
fn an_agent_saved_in_an_ordinary_thread_leaves_the_thread_and_its_delete_keeps_it() {
    use muniment_core::journal::thread_mutation::repair_taken_over_threads;
    let root = muniment_core::state_root::test_root()
        .join(format!("muniment-agent-save-thread-{}", Uuid::new_v4()));
    let profile = root.join("profile");
    std::fs::create_dir_all(&profile).unwrap();
    muniment_core::home::confirm_home(&profile, &root.join("muniment")).unwrap();
    let mut journal = RunJournal::open(profile.join("runs.sqlite3")).unwrap();
    // An ordinary thread holds the user's messages before the agent exists.
    let thread = journal
        .append_new_run("workspace-a", &run_started(Some("owner")))
        .unwrap();
    let runtime = muniment_core::memory_runtime::ApplicationMemoryRuntime::new(
        profile.clone(),
        profile.join("memory"),
    );
    let combinations = [(false, false), (true, false), (false, true), (true, true)];
    for (index, (remove_memory, remove_threads)) in combinations.into_iter().enumerate() {
        // The user asks the model for an agent in a later reply.
        journal
            .append_new_run_in_thread("workspace-a", &thread, &run_started(Some("owner")))
            .unwrap();
        let session = format!("run-{index}");
        runtime
            .open_session(
                &session,
                &thread,
                muniment_core::memory_index::ModelMemoryCapability {
                    minimum_cacheable_prefix_characters: 8192,
                },
            )
            .unwrap();
        let saved = runtime
            .agent_tool(
                &session,
                "agent-save",
                &json!({"name": "Reply Time Check", "instructions": "Check reply times."})
                    .to_string(),
            )
            .unwrap();
        let agent = saved["agent"]["id"].as_str().unwrap().to_owned();
        // The routine tools still name the agent the reply made.
        let routines = runtime
            .agent_tool(&session, "routine-list", &json!({}).to_string())
            .unwrap();
        assert_eq!(routines["agent_id"], agent.as_str());
        runtime.close_session(&session);
        // The thread gets no creation record and stays out of the agent.
        assert!(muniment_core::creations::read(&profile, &thread)
            .unwrap()
            .is_none());
        let state = muniment_core::agents::state(&profile).unwrap();
        assert!(!state.threads.contains_key(&thread));
        assert!(!state.primary_threads.contains_key(&agent));
        assert!(muniment_core::agents::creation_threads(&profile, &agent).is_empty());

        delete_agent_as_the_app_does(
            &mut journal,
            &profile,
            &agent,
            remove_memory,
            remove_threads,
        );
        assert!(!muniment_core::agents::exists(&profile, &agent));
        assert!(
            !thread_deleted(&mut journal, &thread),
            "memory={remove_memory} threads={remove_threads}"
        );
    }

    // A thread an older save took over carries the agent's record and
    // binding. The start repair gives it back, and each delete keeps it.
    for (index, (remove_memory, remove_threads)) in combinations.into_iter().enumerate() {
        let taken = journal
            .append_new_run("workspace-a", &run_started(Some("owner")))
            .unwrap();
        let agent = muniment_core::agents::save(
            &profile,
            muniment_core::agents::Agent {
                name: format!("Reply Time Check {index}"),
                instructions: "Check reply times.".into(),
                ..Default::default()
            },
        )
        .unwrap();
        muniment_core::agents::assign(&profile, &taken, &agent.id).unwrap();
        muniment_core::creations::save(
            &profile,
            muniment_core::creations::Creation {
                thread_id: taken.clone(),
                kind: "agent".into(),
                goal: format!("Create {}", agent.name),
                output: format!("Saved agent: {}", agent.name),
                result_id: Some(agent.id.clone()),
                turns: Vec::new(),
                title: Some("Reply Time Check".into()),
            },
        )
        .unwrap();
        // Before the repair, the agent's delete already leaves the thread.
        assert!(muniment_core::agents::creation_threads(&profile, &agent.id).is_empty());
        // A creation chat with its own record stays the agent's.
        let chat = journal
            .append_new_run("workspace-a", &run_started(Some("owner")))
            .unwrap();
        save_creation(&profile, &chat, &agent.id);

        assert_eq!(
            repair_taken_over_threads(&mut journal, &profile).unwrap(),
            vec![taken.clone()]
        );
        assert!(muniment_core::creations::read(&profile, &taken)
            .unwrap()
            .is_none());
        assert!(muniment_core::creations::read(&profile, &chat)
            .unwrap()
            .is_some());
        let state = muniment_core::agents::state(&profile).unwrap();
        assert!(!state.threads.contains_key(&taken));
        assert!(!state.primary_threads.contains_key(&agent.id));
        assert!(repair_taken_over_threads(&mut journal, &profile)
            .unwrap()
            .is_empty());

        delete_agent_as_the_app_does(
            &mut journal,
            &profile,
            &agent.id,
            remove_memory,
            remove_threads,
        );
        assert!(
            !thread_deleted(&mut journal, &taken),
            "memory={remove_memory} threads={remove_threads}"
        );
        assert!(thread_deleted(&mut journal, &chat));
    }
    assert!(!thread_deleted(&mut journal, &thread));
    drop(journal);
    drop(runtime);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while root.exists() && std::time::Instant::now() < deadline {
        let _ = std::fs::remove_dir_all(&root);
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[test]
fn a_deleted_threads_pi_transcripts_go_and_a_live_threads_stay() {
    use muniment_core::journal::thread_mutation::remove_deleted_thread_sessions;
    let root = muniment_core::state_root::test_root()
        .join(format!("muniment-thread-transcripts-{}", Uuid::new_v4()));
    let profile = root.join("profile");
    let sessions = profile.join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    muniment_core::home::confirm_home(&profile, &root.join("muniment")).unwrap();
    let mut journal = RunJournal::open(profile.join("runs.sqlite3")).unwrap();
    // A thread whose runs bound `locators`, each written in the sessions folder.
    let thread_with = |journal: &mut RunJournal, locators: &[&str]| {
        let first = run_started(Some("owner"));
        let thread = journal.append_new_run("workspace-a", &first).unwrap();
        journal.append(1, &bound(&first, locators[0])).unwrap();
        for locator in &locators[1..] {
            let run = run_started(Some("owner"));
            journal
                .append_new_run_in_thread("workspace-a", &thread, &run)
                .unwrap();
            journal.append(1, &bound(&run, locator)).unwrap();
        }
        for locator in locators {
            std::fs::write(sessions.join(locator), "{}\n").unwrap();
        }
        thread
    };
    let deleted = thread_with(&mut journal, &["a_deleted.jsonl", "b_deleted.jsonl"]);
    let live = thread_with(&mut journal, &["c_live.jsonl"]);
    // A transcript a live thread also binds stays.
    let shared = thread_with(&mut journal, &["d_shared.jsonl"]);
    let sharer = run_started(Some("owner"));
    journal
        .append_new_run_in_thread("workspace-a", &live, &sharer)
        .unwrap();
    journal
        .append(1, &bound(&sharer, "d_shared.jsonl"))
        .unwrap();
    std::fs::write(sessions.join("unrelated.jsonl"), "{}\n").unwrap();

    for thread in [&deleted, &shared] {
        delete_thread_with_creation_now(
            &mut journal,
            &profile,
            Some("owner"),
            thread,
            &provenance(Some("owner")),
        )
        .unwrap();
    }
    assert!(!sessions.join("a_deleted.jsonl").exists());
    assert!(!sessions.join("b_deleted.jsonl").exists());
    for kept in ["c_live.jsonl", "d_shared.jsonl", "unrelated.jsonl"] {
        assert!(sessions.join(kept).is_file(), "{kept}");
    }

    // A delete on another path leaves its transcript, and the start pass
    // removes it. A locator that names a path never reaches outside.
    let elsewhere = thread_with(&mut journal, &["e_elsewhere.jsonl", "../outside.jsonl"]);
    append_thread_delete_now(
        &mut journal,
        Some("owner"),
        &elsewhere,
        &provenance(Some("owner")),
    )
    .unwrap();
    assert!(sessions.join("e_elsewhere.jsonl").is_file());
    assert_eq!(
        remove_deleted_thread_sessions(&mut journal, &profile).unwrap(),
        1
    );
    assert!(!sessions.join("e_elsewhere.jsonl").exists());
    assert!(profile.join("outside.jsonl").is_file());
    for kept in ["c_live.jsonl", "d_shared.jsonl", "unrelated.jsonl"] {
        assert!(sessions.join(kept).is_file(), "{kept}");
    }
    assert_eq!(
        remove_deleted_thread_sessions(&mut journal, &profile).unwrap(),
        0
    );
    drop(journal);
    std::fs::remove_dir_all(root).unwrap();
}
