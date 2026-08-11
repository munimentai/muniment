use std::fs;

use muniment_core::run_preparation::{prepare_new_run_with_session_thread, SessionThreadStart};
use muniment_core::session_thread::SessionThread;
use muniment_runtime::{open_profile_storage, thread_page, thread_summaries};

fn prepare_run(profile: &std::path::Path, run_id: &str, subject: &str) -> String {
    let storage = open_profile_storage(profile).unwrap();
    prepare_new_run_with_session_thread(
        &storage,
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

#[test]
fn lists_threads_and_opens_the_selected_thread() {
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let temporary_root =
        std::env::temp_dir().join(format!("muniment-runtime-threads-{}", std::process::id()));
    let profile = temporary_root.join("profile");
    fs::create_dir_all(&profile).unwrap();
    let first_run = "01900000-0000-7000-8000-000000000001";
    let second_run = "01900000-0000-7000-8000-000000000002";
    let first_thread = prepare_run(&profile, first_run, "owner");
    let second_thread = prepare_run(&profile, second_run, "owner");

    let summaries = thread_summaries(&profile, Some("owner".into()), 10, None).unwrap();

    assert_eq!(summaries.summaries.len(), 2);
    assert!(summaries
        .summaries
        .iter()
        .any(|summary| summary.thread_id == first_thread));
    assert!(summaries
        .summaries
        .iter()
        .any(|summary| summary.thread_id == second_thread));
    let page = thread_page(&profile, Some("owner".into()), first_thread, 10, None).unwrap();
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].run_id, first_run);

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
    let thread_id = prepare_run(&profile, "01900000-0000-7000-8000-000000000003", "owner");

    let error = thread_page(&profile, Some("other".into()), thread_id, 10, None)
        .err()
        .unwrap();

    assert_eq!(error, "Conversation history is unavailable.");
    fs::remove_dir_all(temporary_root).unwrap();
}
