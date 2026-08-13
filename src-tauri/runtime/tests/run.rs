use std::collections::VecDeque;
use std::fs;
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use muniment_core::active_run::cancel_active_run;
use muniment_core::attach::RuntimeActivityRegistry;
use muniment_core::chat_resume::clear_active_run;
use muniment_core::home::confirm_home;
use muniment_core::pi_execution::{coordinate_prepared_prompt, PiRuntime};
use muniment_core::run_events::{ChatEvent, ChatEventSink};
use muniment_core::run_preparation::{
    prepare_new_run_with_session_thread, OpenSelectedFile, SessionThreadStart,
};
use muniment_core::run_start::ActiveRun;
use muniment_core::session_thread::SessionThread;
use muniment_core::sidecar::validate_pi_session;
use muniment_runtime::{
    accept_prompt, drive_prompt, open_profile_storage, resume_run, run_prompt, thread_page,
};

mod common;
use common::{fixture_grant, stage_pi_stub};

static ENVIRONMENT: Mutex<()> = Mutex::new(());

struct FixtureSink;

impl ChatEventSink for FixtureSink {
    fn provenance(&self) -> (&str, &str) {
        ("test", "1")
    }

    fn deliver(&self, _event: ChatEvent) -> Result<(), ()> {
        Ok(())
    }
}

fn accept_two_prompts_with_session_thread(continue_existing: bool) -> (String, String) {
    let _environment = ENVIRONMENT.lock().unwrap();
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let mode = if continue_existing {
        "continued"
    } else {
        "separate"
    };
    let temporary_root = std::env::temp_dir().join(format!(
        "muniment-runtime-session-thread-{mode}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&temporary_root);
    let profile = temporary_root.join("profile");
    let config = temporary_root.join("config");
    fs::create_dir_all(&profile).unwrap();
    confirm_home(&config, &temporary_root.join("home")).unwrap();
    let storage = open_profile_storage(&profile).unwrap();
    let runtime = Arc::new(Mutex::new(None));
    let runtime_activity = RuntimeActivityRegistry::new();
    let active = Arc::new(Mutex::new(None));
    let session_thread = SessionThread::default();
    let first_run_id = "018f0000-0000-7000-8000-000000000021";
    let second_run_id = "018f0000-0000-7000-8000-000000000022";

    let (first, first_launch) = accept_prompt(
        &profile,
        Arc::clone(&storage),
        Arc::clone(&runtime),
        &runtime_activity,
        &config,
        first_run_id.into(),
        "first prompt".into(),
        None,
        &session_thread,
        continue_existing,
        "token".into(),
        Some("owner".into()),
        Vec::new(),
        fixture_grant(),
        Arc::clone(&active),
        None,
        None,
    )
    .unwrap();
    drop(first_launch);
    clear_active_run(&active, first_run_id);

    let (second, second_launch) = accept_prompt(
        &profile,
        Arc::clone(&storage),
        runtime,
        &runtime_activity,
        &config,
        second_run_id.into(),
        "second prompt".into(),
        None,
        &session_thread,
        continue_existing,
        "token".into(),
        Some("owner".into()),
        Vec::new(),
        fixture_grant(),
        Arc::clone(&active),
        None,
        None,
    )
    .unwrap();
    drop(second_launch);
    clear_active_run(&active, second_run_id);

    let thread_ids = (first.thread_id, second.thread_id);
    drop(storage);
    fs::remove_dir_all(temporary_root).unwrap();
    thread_ids
}

#[test]
fn session_thread_continues_two_prompts() {
    let (first_thread_id, second_thread_id) = accept_two_prompts_with_session_thread(true);

    assert_eq!(first_thread_id, second_thread_id);
}

#[test]
fn disabled_session_thread_continuation_starts_two_threads() {
    let (first_thread_id, second_thread_id) = accept_two_prompts_with_session_thread(false);

    assert_ne!(first_thread_id, second_thread_id);
}

#[test]
fn an_occupied_active_run_slot_does_not_prepare_a_new_run() {
    let temporary_root =
        std::env::temp_dir().join(format!("muniment-runtime-occupied-{}", std::process::id()));
    let profile = temporary_root.join("profile");
    fs::create_dir_all(&profile).unwrap();
    let activity = RuntimeActivityRegistry::new();
    let active = Arc::new(Mutex::new(Some(ActiveRun {
        id: "018f0000-0000-7000-8000-000000000001".into(),
        workspace: "workspace-a".into(),
        cancelled: Arc::new(AtomicBool::new(false)),
        transport: Arc::new(Mutex::new(None)),
        adapter: Arc::new(Mutex::new(None)),
        permission_answers: Arc::new(Mutex::new(VecDeque::new())),
        _activity: activity.mark_active_run(),
    })));
    let run_id = "018f0000-0000-7000-8000-000000000002";
    let storage = open_profile_storage(&profile).unwrap();

    let error = run_prompt(
        &profile,
        Arc::clone(&storage),
        Arc::new(Mutex::new(None)),
        &activity,
        temporary_root.join("config"),
        run_id.into(),
        "prompt".into(),
        None,
        &SessionThread::default(),
        false,
        "token".into(),
        Some("owner".into()),
        Vec::new(),
        fixture_grant(),
        active,
        None,
        None,
    )
    .unwrap_err();

    assert_eq!(error, "A reply is already in progress.");
    assert!(storage
        .lock()
        .unwrap()
        .journal
        .events(run_id)
        .unwrap()
        .is_empty());
    fs::remove_dir_all(temporary_root).unwrap();
}

#[test]
fn runs_two_prompts_in_one_named_thread_and_rejects_an_unknown_thread() {
    let _environment = ENVIRONMENT.lock().unwrap();
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let temporary_root =
        std::env::temp_dir().join(format!("muniment-runtime-run-{}", std::process::id()));
    let profile = temporary_root.join("profile");
    let config = temporary_root.join("config");
    fs::create_dir_all(&profile).unwrap();
    let storage = open_profile_storage(&profile).unwrap();
    let runtime = Arc::new(Mutex::new(None::<PiRuntime>));
    let runtime_activity = RuntimeActivityRegistry::new();
    confirm_home(&config, &temporary_root.join("home")).unwrap();
    let descriptor = stage_pi_stub(&temporary_root);
    let captured_prompts = temporary_root.join("prompts");
    let captured_args = temporary_root.join("args");
    std::env::set_var("PI_RESUME_STUB_PROMPTS", &captured_prompts);
    std::env::set_var("PI_RESUME_STUB_ARGS", &captured_args);

    let unknown_run_id = "018f0000-0000-7000-8000-000000000002";
    let error = run_prompt(
        &profile,
        Arc::clone(&storage),
        Arc::clone(&runtime),
        &runtime_activity,
        &config,
        unknown_run_id.into(),
        "unknown thread prompt".into(),
        Some("unknown-thread".into()),
        &SessionThread::default(),
        false,
        "token".into(),
        Some("owner".into()),
        Vec::new(),
        fixture_grant(),
        Arc::new(Mutex::new(None)),
        None,
        Some(descriptor),
    )
    .unwrap_err();
    assert_eq!(error, "thread_not_found");
    assert!(storage
        .lock()
        .unwrap()
        .journal
        .events(unknown_run_id)
        .unwrap()
        .is_empty());
    let run_id = "018f0000-0000-7000-8000-000000000003";
    let prompt = "pointer install prompt";
    let (subscriber, events) = mpsc::channel();
    let active = Arc::new(Mutex::new(None));
    std::env::set_var("PI_RESUME_STUB_MEMORY_QUERY", "hold the run open");
    std::thread::scope(|scope| {
        let run = scope.spawn(|| {
            run_prompt(
                &profile,
                Arc::clone(&storage),
                Arc::clone(&runtime),
                &runtime_activity,
                &config,
                run_id.into(),
                prompt.into(),
                None,
                &SessionThread::default(),
                false,
                "token".into(),
                Some("owner".into()),
                Vec::new(),
                fixture_grant(),
                Arc::clone(&active),
                Some(subscriber),
                Some(descriptor),
            )
        });
        assert_eq!(
            events.recv_timeout(Duration::from_secs(5)).unwrap().phase,
            "thinking"
        );
        cancel_active_run(&active, run_id, Some("workspace-a")).unwrap();
        run.join().unwrap().unwrap();
    });
    std::env::remove_var("PI_RESUME_STUB_MEMORY_QUERY");
    assert!(active.lock().unwrap().is_none());
    assert_eq!(
        cancel_active_run(&active, run_id, Some("workspace-a")),
        Err("That reply is no longer active.".into())
    );

    let thread_id = storage
        .lock()
        .unwrap()
        .journal
        .run_thread_id(run_id)
        .unwrap()
        .unwrap();
    let second_run_id = "018f0000-0000-7000-8000-000000000005";
    let second_prompt = "named thread prompt";
    let attachment_path = temporary_root.join("runtime-attachment.txt");
    let attachment_bytes = b"runtime attachment";
    fs::write(&attachment_path, attachment_bytes).unwrap();
    let second_active = Arc::new(Mutex::new(None));
    let (accepted, launch) = accept_prompt(
        &profile,
        Arc::clone(&storage),
        Arc::clone(&runtime),
        &runtime_activity,
        &config,
        second_run_id.into(),
        second_prompt.into(),
        Some(thread_id.clone()),
        &SessionThread::default(),
        false,
        "token".into(),
        Some("owner".into()),
        vec![OpenSelectedFile {
            file: fs::File::open(&attachment_path).unwrap(),
            display_name: "runtime-attachment.txt".into(),
            byte_length: attachment_bytes.len() as u64,
        }],
        fixture_grant(),
        Arc::clone(&second_active),
        None,
        Some(descriptor),
    )
    .unwrap();
    assert_eq!(accepted.run_id, second_run_id);
    assert_eq!(accepted.thread_id, thread_id);
    assert_eq!(accepted.attachments.len(), 1);
    assert_eq!(
        accepted.attachments[0].display_name,
        "runtime-attachment.txt"
    );
    assert!(accepted.committed_seq > 0);
    assert!(!accepted.accepted_at.is_empty());
    assert_eq!(
        second_active.lock().unwrap().as_ref().unwrap().id,
        second_run_id
    );
    assert!(runtime_activity.snapshot().active_run);
    assert!(storage
        .lock()
        .unwrap()
        .journal
        .events(second_run_id)
        .unwrap()
        .iter()
        .all(|event| !matches!(
            event.event_type.as_str(),
            "run.completed" | "run.failed" | "run.cancelled" | "run.needs_attention"
        )));
    drive_prompt(launch);
    assert!(second_active.lock().unwrap().is_none());
    assert!(!runtime_activity.snapshot().active_run);
    assert!(runtime.lock().unwrap().is_some());

    assert_eq!(
        fs::read_to_string(captured_prompts).unwrap(),
        format!("{prompt}\n{second_prompt}\n")
    );
    let args = fs::read_to_string(captured_args).unwrap();
    let extension = profile.join("memory").join("memory-search-extension.js");
    assert!(args
        .lines()
        .collect::<Vec<_>>()
        .windows(2)
        .any(|args| { args == ["--extension", extension.to_string_lossy().as_ref()] }));
    let mut stored_chat = storage.lock().unwrap();
    let journal_events = stored_chat.journal.events(run_id).unwrap();
    assert_eq!(journal_events.last().unwrap().event_type, "run.cancelled");
    assert!(journal_events
        .iter()
        .all(|event| event.provenance.source == "muniment-runtime"));
    let named_thread_events = stored_chat.journal.events(second_run_id).unwrap();
    assert_eq!(
        named_thread_events
            .iter()
            .filter(|event| event.event_type == "chat.attachment.ingested")
            .count(),
        1
    );
    assert!(named_thread_events
        .iter()
        .all(|event| event.provenance.source == "muniment-runtime"));
    assert_eq!(
        stored_chat
            .journal
            .thread_run_ids(&thread_id, 10, None)
            .unwrap()
            .run_ids,
        [run_id, second_run_id]
    );

    drop(stored_chat);
    let page = thread_page(
        &profile,
        Arc::clone(&storage),
        Some("owner".into()),
        thread_id,
        10,
        None,
    )
    .unwrap();
    let entry = page
        .entries
        .iter()
        .find(|entry| entry.run_id == run_id)
        .unwrap();
    assert_eq!(entry.prompt.as_deref(), Some(prompt));
    let entry = page
        .entries
        .iter()
        .find(|entry| entry.run_id == second_run_id)
        .unwrap();
    assert_eq!(entry.prompt.as_deref(), Some(second_prompt));
    assert_eq!(entry.attachments.len(), 1);
    assert_eq!(entry.attachments[0].display_name, "runtime-attachment.txt");
    assert_eq!(
        entry.attachments[0].byte_length,
        attachment_bytes.len() as u64
    );
    std::env::remove_var("MUNIMENT_PI_ROOT");
    std::env::remove_var("PI_RESUME_STUB_PROMPTS");
    std::env::remove_var("PI_RESUME_STUB_ARGS");
    fs::remove_dir_all(temporary_root).unwrap();
}

#[test]
fn resumes_an_interrupted_run_to_a_terminal_event() {
    let _environment = ENVIRONMENT.lock().unwrap();
    let temporary_root =
        std::env::temp_dir().join(format!("muniment-runtime-resume-{}", std::process::id()));
    let profile = temporary_root.join("profile");
    let config = temporary_root.join("config");
    fs::create_dir_all(&profile).unwrap();
    confirm_home(&config, &temporary_root.join("home")).unwrap();
    let descriptor = stage_pi_stub(&temporary_root);
    let runtime_activity = RuntimeActivityRegistry::new();

    let run_id = "018f0000-0000-7000-8000-000000000004";
    let session_root = profile.join("pi-sessions");
    fs::create_dir_all(&session_root).unwrap();
    fs::write(session_root.join("session.jsonl"), b"{}\n").unwrap();
    let storage = open_profile_storage(&profile).unwrap();
    let session_thread = SessionThread::default();
    let (mut sequence, mut projector) = prepare_new_run_with_session_thread(
        &storage,
        SessionThreadStart {
            tracker: &session_thread,
            continue_existing: false,
        },
        run_id,
        "workspace-a",
        Some("owner"),
        Vec::new(),
        None,
        "test",
        "1",
        || Ok(()),
    )
    .unwrap();
    coordinate_prepared_prompt(
        &FixtureSink,
        &storage,
        &mut projector,
        run_id,
        &mut sequence,
        Some("owner"),
        || {
            let (locator, _) = validate_pi_session(&session_root, "session.jsonl")
                .map_err(|_| muniment_core::pi_execution::PreparedPromptError::SessionRoot)?;
            Ok(((), locator, Vec::new()))
        },
    )
    .unwrap();
    drop(storage);
    let storage = open_profile_storage(&profile).unwrap();
    resume_run(
        &profile,
        Arc::clone(&storage),
        Arc::new(Mutex::new(None)),
        &runtime_activity,
        &config,
        run_id.into(),
        "token".into(),
        Some("owner".into()),
        fixture_grant(),
        Arc::new(Mutex::new(None)),
        None,
        Some(descriptor),
    )
    .unwrap();

    let events = storage.lock().unwrap().journal.events(run_id).unwrap();
    assert!(matches!(
        events.last().map(|event| event.event_type.as_str()),
        Some("run.completed" | "run.failed" | "run.cancelled")
    ));

    drop(events);
    drop(storage);
    std::env::remove_var("MUNIMENT_PI_ROOT");
    fs::remove_dir_all(temporary_root).unwrap();
}
