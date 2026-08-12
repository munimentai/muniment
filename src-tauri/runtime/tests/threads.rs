use std::collections::BTreeMap;
use std::fs;
use std::sync::Arc;

use muniment_core::journal::Provenance;
use muniment_core::run_events::SharedStorage;
use muniment_core::run_preparation::{
    prepare_new_run_in_thread_after_validation, prepare_new_run_with_session_thread,
    SessionThreadStart,
};
use muniment_core::session_thread::SessionThread;
use muniment_runtime::{
    create_thread, delete_thread, open_profile_storage, rename_thread, thread_page,
    thread_summaries,
};

fn prepare_run(storage: &SharedStorage, run_id: &str, subject: &str) -> String {
    prepare_new_run_with_session_thread(
        storage,
        SessionThreadStart {
            tracker: &SessionThread::default(),
            continue_existing: false,
        },
        run_id,
        "workspace-a",
        Some(subject),
        Vec::new(),
        None,
        "test",
        "1",
        || Ok(()),
    )
    .unwrap();
    let thread_id = storage
        .lock()
        .unwrap()
        .journal
        .run_thread_id(run_id)
        .unwrap()
        .unwrap();
    thread_id
}

fn attach_provenance() -> Provenance {
    let mut provenance = Provenance {
        source: "test".into(),
        source_version: "1".into(),
        actor_id: None,
        device_id: None,
        rpc_request_id: None,
        capability_versions: None,
        extra: BTreeMap::new(),
    };
    provenance
        .extra
        .insert("attach_profile".into(), "profile-a".into());
    provenance
}

#[test]
fn creates_a_thread_for_a_run_and_rejects_invalid_inputs_without_events() {
    let temporary_root =
        std::env::temp_dir().join(format!("muniment-runtime-thread-create-{}", std::process::id()));
    let profile = temporary_root.join("profile");
    fs::create_dir_all(&profile).unwrap();
    let storage = open_profile_storage(&profile).unwrap();

    assert!(create_thread(
        Arc::clone(&storage),
        String::new(),
        "profile-a".into(),
    )
    .is_err());
    assert!(create_thread(
        Arc::clone(&storage),
        "workspace-a".into(),
        String::new(),
    )
    .is_err());
    assert!(thread_summaries(Arc::clone(&storage), Some("owner".into()), 10, None)
        .unwrap()
        .summaries
        .is_empty());

    let thread_id = create_thread(
        Arc::clone(&storage),
        "workspace-a".into(),
        "profile-a".into(),
    )
    .unwrap();
    let run_id = "01900000-0000-7000-8000-000000000000";
    prepare_new_run_in_thread_after_validation(
        &storage,
        run_id,
        "workspace-a",
        Some("owner"),
        Vec::new(),
        Some(attach_provenance()),
        &thread_id,
        "test",
        "1",
        || Ok(()),
    )
    .unwrap();

    let page = thread_page(
        &profile,
        Arc::clone(&storage),
        Some("owner".into()),
        thread_id,
        10,
        None,
    )
    .unwrap();
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].run_id, run_id);

    drop(storage);
    fs::remove_dir_all(temporary_root).unwrap();
}

#[test]
fn lists_threads_and_opens_the_selected_thread() {
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let temporary_root =
        std::env::temp_dir().join(format!("muniment-runtime-threads-{}", std::process::id()));
    let profile = temporary_root.join("profile");
    fs::create_dir_all(&profile).unwrap();
    let storage = open_profile_storage(&profile).unwrap();
    let first_run = "01900000-0000-7000-8000-000000000001";
    let second_run = "01900000-0000-7000-8000-000000000002";
    let first_thread = prepare_run(&storage, first_run, "owner");
    let second_thread = prepare_run(&storage, second_run, "owner");

    let summaries = thread_summaries(Arc::clone(&storage), Some("owner".into()), 10, None).unwrap();

    assert_eq!(summaries.summaries.len(), 2);
    assert!(summaries
        .summaries
        .iter()
        .any(|summary| summary.thread_id == first_thread));
    assert!(summaries
        .summaries
        .iter()
        .any(|summary| summary.thread_id == second_thread));
    let page = thread_page(
        &profile,
        Arc::clone(&storage),
        Some("owner".into()),
        first_thread,
        10,
        None,
    )
    .unwrap();
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].run_id, first_run);

    drop(storage);
    fs::remove_dir_all(temporary_root).unwrap();
}

#[test]
fn rejects_a_thread_owned_by_another_subject() {
    let temporary_root = std::env::temp_dir().join(format!(
        "muniment-runtime-thread-owner-{}",
        std::process::id()
    ));
    let profile = temporary_root.join("profile");
    fs::create_dir_all(&profile).unwrap();
    let storage = open_profile_storage(&profile).unwrap();
    let thread_id = prepare_run(&storage, "01900000-0000-7000-8000-000000000003", "owner");

    let error = thread_page(&profile, storage, Some("other".into()), thread_id, 10, None)
        .err()
        .unwrap();

    assert_eq!(error, "Conversation history is unavailable.");
    fs::remove_dir_all(temporary_root).unwrap();
}

#[test]
fn renames_and_deletes_owned_threads() {
    let temporary_root = std::env::temp_dir().join(format!(
        "muniment-runtime-thread-mutations-{}",
        std::process::id()
    ));
    let profile = temporary_root.join("profile");
    fs::create_dir_all(&profile).unwrap();
    let storage = open_profile_storage(&profile).unwrap();
    let renamed_thread = prepare_run(&storage, "01900000-0000-7000-8000-000000000004", "owner");
    let deleted_thread = prepare_run(&storage, "01900000-0000-7000-8000-000000000005", "owner");

    rename_thread(
        Arc::clone(&storage),
        Some("owner".into()),
        renamed_thread.clone(),
        "New title".into(),
    )
    .unwrap();
    delete_thread(
        Arc::clone(&storage),
        Some("owner".into()),
        deleted_thread.clone(),
    )
    .unwrap();

    {
        let mut storage = storage.lock().unwrap();
        let rename_events = storage.journal.thread_events(&renamed_thread).unwrap();
        let rename_event = rename_events.last().unwrap();
        assert_eq!(rename_event.event_type, "thread.title.renamed");
        assert_eq!(rename_event.provenance.source, "muniment-runtime");
        assert_eq!(
            rename_event.provenance.source_version,
            env!("CARGO_PKG_VERSION")
        );

        let delete_events = storage.journal.thread_events(&deleted_thread).unwrap();
        let delete_event = delete_events.last().unwrap();
        assert_eq!(delete_event.event_type, "thread.deleted");
        assert_eq!(delete_event.provenance.source, "muniment-runtime");
        assert_eq!(
            delete_event.provenance.source_version,
            env!("CARGO_PKG_VERSION")
        );
    }

    let summaries = thread_summaries(Arc::clone(&storage), Some("owner".into()), 10, None).unwrap();
    assert_eq!(summaries.summaries.len(), 1);
    assert_eq!(summaries.summaries[0].thread_id, renamed_thread);
    assert_eq!(summaries.summaries[0].title, "New title");
    assert!(!summaries
        .summaries
        .iter()
        .any(|summary| summary.thread_id == deleted_thread));

    drop(storage);
    fs::remove_dir_all(temporary_root).unwrap();
}

#[test]
fn rejects_mutations_by_another_subject_without_appending_events() {
    let temporary_root = std::env::temp_dir().join(format!(
        "muniment-runtime-thread-mutation-owner-{}",
        std::process::id()
    ));
    let profile = temporary_root.join("profile");
    fs::create_dir_all(&profile).unwrap();
    let storage = open_profile_storage(&profile).unwrap();
    let thread_id = prepare_run(&storage, "01900000-0000-7000-8000-000000000006", "owner");

    assert_eq!(
        rename_thread(
            Arc::clone(&storage),
            Some("other".into()),
            thread_id.clone(),
            "New title".into(),
        )
        .unwrap_err(),
        "Conversation history is unavailable."
    );
    assert_eq!(
        delete_thread(
            Arc::clone(&storage),
            Some("other".into()),
            thread_id.clone(),
        )
        .unwrap_err(),
        "Conversation history is unavailable."
    );
    assert_eq!(
        storage
            .lock()
            .unwrap()
            .journal
            .last_thread_seq(&thread_id)
            .unwrap(),
        1
    );

    drop(storage);
    fs::remove_dir_all(temporary_root).unwrap();
}
