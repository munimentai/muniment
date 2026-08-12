use std::collections::VecDeque;
use std::fs;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use muniment_core::active_run::cancel_active_run;
use muniment_core::attach::RuntimeActivityRegistry;
use muniment_core::chat_grant::ChatGrant;
use muniment_core::home::confirm_home;
use muniment_core::pi_execution::coordinate_prepared_prompt;
use muniment_core::run_events::{ChatEvent, ChatEventSink};
use muniment_core::run_preparation::{
    prepare_new_run_with_session_thread, OpenSelectedFile, SessionThreadStart,
};
use muniment_core::run_start::ActiveRun;
use muniment_core::session_thread::SessionThread;
use muniment_core::sidecar::pi_install::{PiArtifactDescriptor, PI_ARTIFACT};
use muniment_core::sidecar::validate_pi_session;
use muniment_runtime::{open_profile_storage, resume_run, run_prompt, thread_page};

static ENVIRONMENT: Mutex<()> = Mutex::new(());

struct FixtureSink;

fn fixture_grant() -> ChatGrant {
    ChatGrant {
        workspace: "workspace-a".into(),
        gateway_url: "https://gateway.example.com".into(),
        virtual_key: "virtual-key".into(),
        model: None,
        minimum_cacheable_prefix_characters: 8_192,
        receipt_url: "https://receipts.example.com".into(),
    }
}

impl ChatEventSink for FixtureSink {
    fn provenance(&self) -> (&str, &str) {
        ("test", "1")
    }

    fn deliver(&self, _event: ChatEvent) -> Result<(), ()> {
        Ok(())
    }
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
        temporary_root.join("config"),
        run_id.into(),
        "prompt".into(),
        None,
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
    let pi_root = temporary_root.join("pi");
    let executable = pi_root
        .join("revisions")
        .join(PI_ARTIFACT.version)
        .join(PI_ARTIFACT.executable);
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::create_dir_all(&profile).unwrap();
    let storage = open_profile_storage(&profile).unwrap();
    confirm_home(&config, &temporary_root.join("home")).unwrap();
    let build_root = temporary_root.join("build");
    let status = Command::new(env!("CARGO"))
        .args([
            "build",
            "--quiet",
            "--package",
            "muniment-core",
            "--bin",
            "sidecar-test-stub",
            "--target-dir",
        ])
        .arg(&build_root)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .unwrap();
    assert!(status.success());
    let stub_name = if cfg!(windows) {
        "sidecar-test-stub.exe"
    } else {
        "sidecar-test-stub"
    };
    fs::copy(build_root.join("debug").join(stub_name), &executable).unwrap();
    let stub_archive = b"muniment-sidecar-test-stub\n";
    fs::write(
        pi_root
            .join("revisions")
            .join(PI_ARTIFACT.version)
            .join(PI_ARTIFACT.archive),
        stub_archive,
    )
    .unwrap();
    let descriptor = PiArtifactDescriptor {
        version: PI_ARTIFACT.version,
        archive: PI_ARTIFACT.archive,
        byte_size: stub_archive.len() as u64,
        sha256: "758b0db8f6304639edfca2b779e886f3006afeb006417e49dd6bce53ff2a65ab",
        executable: PI_ARTIFACT.executable,
    };
    fs::write(
        pi_root.join("current"),
        format!("muniment-pi-pointer-v1\n{}\n", PI_ARTIFACT.version),
    )
    .unwrap();
    let captured_prompts = temporary_root.join("prompts");
    let captured_args = temporary_root.join("args");
    std::env::set_var("MUNIMENT_PI_ROOT", &pi_root);
    std::env::set_var("PI_RESUME_STUB_PROMPTS", &captured_prompts);
    std::env::set_var("PI_RESUME_STUB_ARGS", &captured_args);

    let unknown_run_id = "018f0000-0000-7000-8000-000000000002";
    let error = run_prompt(
        &profile,
        Arc::clone(&storage),
        &config,
        unknown_run_id.into(),
        "unknown thread prompt".into(),
        Some("unknown-thread".into()),
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
                &config,
                run_id.into(),
                prompt.into(),
                None,
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
    run_prompt(
        &profile,
        Arc::clone(&storage),
        &config,
        second_run_id.into(),
        second_prompt.into(),
        Some(thread_id.clone()),
        "token".into(),
        Some("owner".into()),
        vec![OpenSelectedFile {
            file: fs::File::open(&attachment_path).unwrap(),
            display_name: "runtime-attachment.txt".into(),
            byte_length: attachment_bytes.len() as u64,
        }],
        fixture_grant(),
        Arc::new(Mutex::new(None)),
        None,
        Some(descriptor),
    )
    .unwrap();

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
        .find(|entry| entry.run_id == second_run_id)
        .unwrap();
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
    let pi_root = temporary_root.join("pi");
    let executable = pi_root
        .join("revisions")
        .join(PI_ARTIFACT.version)
        .join(PI_ARTIFACT.executable);
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::create_dir_all(&profile).unwrap();
    confirm_home(&config, &temporary_root.join("home")).unwrap();
    let build_root = temporary_root.join("build");
    let status = Command::new(env!("CARGO"))
        .args([
            "build",
            "--quiet",
            "--package",
            "muniment-core",
            "--bin",
            "sidecar-test-stub",
            "--target-dir",
        ])
        .arg(&build_root)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .unwrap();
    assert!(status.success());
    let stub_name = if cfg!(windows) {
        "sidecar-test-stub.exe"
    } else {
        "sidecar-test-stub"
    };
    fs::copy(build_root.join("debug").join(stub_name), &executable).unwrap();
    let stub_archive = b"muniment-sidecar-test-stub\n";
    fs::write(
        pi_root
            .join("revisions")
            .join(PI_ARTIFACT.version)
            .join(PI_ARTIFACT.archive),
        stub_archive,
    )
    .unwrap();
    let descriptor = PiArtifactDescriptor {
        version: PI_ARTIFACT.version,
        archive: PI_ARTIFACT.archive,
        byte_size: stub_archive.len() as u64,
        sha256: "758b0db8f6304639edfca2b779e886f3006afeb006417e49dd6bce53ff2a65ab",
        executable: PI_ARTIFACT.executable,
    };
    fs::write(
        pi_root.join("current"),
        format!("muniment-pi-pointer-v1\n{}\n", PI_ARTIFACT.version),
    )
    .unwrap();
    std::env::set_var("MUNIMENT_PI_ROOT", &pi_root);

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
        &config,
        run_id.into(),
        "token".into(),
        Some("owner".into()),
        ChatGrant {
            workspace: "workspace-a".into(),
            gateway_url: "https://gateway.example.com".into(),
            virtual_key: "virtual-key".into(),
            model: None,
            minimum_cacheable_prefix_characters: 8_192,
            receipt_url: "https://receipts.example.com".into(),
        },
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
