use std::collections::VecDeque;
use std::fs;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use muniment_core::attach::RuntimeActivityRegistry;
use muniment_core::chat_grant::ChatGrant;
use muniment_core::home::confirm_home;
use muniment_core::permission_gate::ChatPermissionAnswer;
use muniment_core::run_start::ActiveRun;
use muniment_core::sidecar::pi_install::{PiArtifactDescriptor, PI_ARTIFACT};
use muniment_runtime::{answer_permission, open_profile_storage, run_prompt};

static ENVIRONMENT: Mutex<()> = Mutex::new(());

fn inactive_run() -> ActiveRun {
    let runtime_activity = RuntimeActivityRegistry::new();
    ActiveRun {
        id: "run-1".into(),
        workspace: "workspace-a".into(),
        cancelled: Arc::new(AtomicBool::new(false)),
        transport: Arc::new(Mutex::new(None)),
        adapter: Arc::new(Mutex::new(None)),
        permission_answers: Arc::new(Mutex::new(VecDeque::new())),
        _activity: runtime_activity.mark_active_run(),
    }
}

#[test]
fn permission_answer_reports_rejection_and_timeout() {
    let active = Arc::new(Mutex::new(Some(inactive_run())));
    std::thread::scope(|scope| {
        let answer = scope.spawn(|| {
            answer_permission(
                Arc::clone(&active),
                "workspace-a".into(),
                "run-1".into(),
                "gate-1".into(),
                ChatPermissionAnswer::Confirm(true),
                Duration::from_secs(1),
            )
        });
        loop {
            let sender = active
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .permission_answers
                .lock()
                .unwrap()
                .pop_front()
                .and_then(|answer| answer.resolved);
            if let Some(sender) = sender {
                sender.send(None).unwrap();
                break;
            }
            std::thread::yield_now();
        }
        assert_eq!(
            answer.join().unwrap().unwrap_err(),
            "The permission answer was rejected."
        );
    });

    assert_eq!(
        answer_permission(
            active,
            "workspace-a".into(),
            "run-1".into(),
            "gate-2".into(),
            ChatPermissionAnswer::Confirm(true),
            Duration::ZERO,
        )
        .unwrap_err(),
        "The permission answer did not commit in time."
    );
}

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

fn install_stub(temporary_root: &std::path::Path) -> PiArtifactDescriptor {
    let pi_root = temporary_root.join("pi");
    let executable = pi_root
        .join("revisions")
        .join(PI_ARTIFACT.version)
        .join(PI_ARTIFACT.executable);
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
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
    fs::copy(build_root.join("debug").join(stub_name), executable).unwrap();
    let stub_archive = b"muniment-sidecar-test-stub\n";
    fs::write(
        pi_root
            .join("revisions")
            .join(PI_ARTIFACT.version)
            .join(PI_ARTIFACT.archive),
        stub_archive,
    )
    .unwrap();
    fs::write(
        pi_root.join("current"),
        format!("muniment-pi-pointer-v1\n{}\n", PI_ARTIFACT.version),
    )
    .unwrap();
    std::env::set_var("MUNIMENT_PI_ROOT", pi_root);
    PiArtifactDescriptor {
        version: PI_ARTIFACT.version,
        archive: PI_ARTIFACT.archive,
        byte_size: stub_archive.len() as u64,
        sha256: "758b0db8f6304639edfca2b779e886f3006afeb006417e49dd6bce53ff2a65ab",
        executable: PI_ARTIFACT.executable,
    }
}

#[test]
fn a_queued_permission_answer_reaches_a_live_runtime_run() {
    let _environment = ENVIRONMENT.lock().unwrap();
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let temporary_root = std::env::temp_dir().join(format!(
        "muniment-runtime-permission-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&temporary_root);
    let profile = temporary_root.join("profile");
    let config = temporary_root.join("config");
    fs::create_dir_all(&profile).unwrap();
    confirm_home(&config, &temporary_root.join("home")).unwrap();
    let descriptor = install_stub(&temporary_root);
    let permission_capture = temporary_root.join("permission.jsonl");
    std::env::set_var("PI_RESUME_STUB_PERMISSION_CAPTURE", &permission_capture);

    let run_id = "018f0000-0000-7000-8000-000000000113";
    let storage = open_profile_storage(&profile).unwrap();
    let runtime = Arc::new(Mutex::new(None));
    let active = Arc::new(Mutex::new(None));
    let (subscriber, events) = mpsc::channel();
    let committed_seq = std::thread::scope(|scope| {
        let run = scope.spawn(|| {
            run_prompt(
                &profile,
                Arc::clone(&storage),
                Arc::clone(&runtime),
                &config,
                run_id.into(),
                "initial prompt".into(),
                None,
                &muniment_core::session_thread::SessionThread::default(),
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
        let gate_id = loop {
            let event = events.recv_timeout(Duration::from_secs(5)).unwrap();
            if let Some(permission) = event.pending_permission {
                break permission.gate_id;
            }
        };
        let committed_seq = answer_permission(
            Arc::clone(&active),
            "workspace-a".into(),
            run_id.into(),
            gate_id,
            ChatPermissionAnswer::Confirm(true),
            Duration::from_secs(5),
        )
        .unwrap();
        run.join().unwrap().unwrap();
        committed_seq
    });

    let captured = fs::read_to_string(permission_capture).unwrap();
    assert!(captured.contains(r#""type":"extension_ui_response""#));
    assert!(captured.contains(r#""id":"permission-1""#));
    assert!(captured.contains(r#""confirmed":true"#));

    drop(storage);
    let storage = open_profile_storage(&profile).unwrap();
    let journal_events = storage.lock().unwrap().journal.events(run_id).unwrap();
    let event_types: Vec<_> = journal_events
        .iter()
        .map(|event| event.event_type.as_str())
        .collect();
    let requested = event_types
        .iter()
        .position(|event| *event == "permission.requested")
        .unwrap();
    let resolved = event_types
        .iter()
        .position(|event| *event == "permission.resolved")
        .unwrap();
    let terminal = event_types
        .iter()
        .position(|event| matches!(*event, "run.completed" | "run.failed" | "run.cancelled"))
        .unwrap();
    assert!(requested < resolved && resolved < terminal);
    assert_eq!(journal_events[resolved].run_seq, committed_seq);

    drop(journal_events);
    drop(storage);
    std::env::remove_var("PI_RESUME_STUB_PERMISSION_CAPTURE");
    std::env::remove_var("MUNIMENT_PI_ROOT");
    fs::remove_dir_all(temporary_root).unwrap();
}
