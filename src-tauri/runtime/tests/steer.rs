use std::fs;
use std::process::Command;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use muniment_core::active_run::{queue_message, ChatDelivery, ChatQueueRequest};
use muniment_core::chat_grant::ChatGrant;
use muniment_core::home::confirm_home;
use muniment_core::sidecar::pi_install::{PiArtifactDescriptor, PI_ARTIFACT};
use muniment_runtime::{open_profile_storage, run_prompt};

static ENVIRONMENT: Mutex<()> = Mutex::new(());

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
fn a_queued_steer_reaches_a_live_runtime_run() {
    let _environment = ENVIRONMENT.lock().unwrap();
    let temporary_root =
        std::env::temp_dir().join(format!("muniment-runtime-steer-{}", std::process::id()));
    let _ = fs::remove_dir_all(&temporary_root);
    let profile = temporary_root.join("profile");
    let config = temporary_root.join("config");
    fs::create_dir_all(&profile).unwrap();
    confirm_home(&config, &temporary_root.join("home")).unwrap();
    let descriptor = install_stub(&temporary_root);
    let steer_capture = temporary_root.join("steer.json");
    std::env::set_var("PI_RESUME_STUB_STEER_CAPTURE", &steer_capture);

    let run_id = "018f0000-0000-7000-8000-000000000112";
    let storage = open_profile_storage(&profile).unwrap();
    let active = Arc::new(Mutex::new(None));
    let (subscriber, events) = mpsc::channel();
    std::thread::scope(|scope| {
        let run = scope.spawn(|| {
            run_prompt(
                &profile,
                Arc::clone(&storage),
                &config,
                run_id.into(),
                "initial prompt".into(),
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
        queue_message(
            &active,
            ChatQueueRequest {
                run_id: run_id.into(),
                delivery: ChatDelivery::Steer,
                message: "redirect here".into(),
            },
        )
        .unwrap();
        let delivered = events
            .iter()
            .take_while(|event| {
                !matches!(event.phase.as_str(), "complete" | "failed" | "cancelled")
            })
            .any(|event| event.text.contains("steered"));
        assert!(delivered);
        run.join().unwrap().unwrap();
    });

    let captured = fs::read_to_string(steer_capture).unwrap();
    assert!(captured.contains(r#""type":"steer""#));
    assert!(captured.contains(r#""message":"redirect here""#));
    let inactive = |id: &str| ChatQueueRequest {
        run_id: id.into(),
        delivery: ChatDelivery::Steer,
        message: "too late".into(),
    };
    for id in [run_id, "018f0000-0000-7000-8000-000000000999"] {
        assert_eq!(
            queue_message(&active, inactive(id)),
            Err("That reply is no longer active.".into())
        );
    }

    drop(storage);
    let storage = open_profile_storage(&profile).unwrap();
    let journal_events = storage.lock().unwrap().journal.events(run_id).unwrap();
    assert!(matches!(
        journal_events.last().map(|event| event.event_type.as_str()),
        Some("run.completed" | "run.failed" | "run.cancelled")
    ));

    drop(journal_events);
    drop(storage);
    std::env::remove_var("PI_RESUME_STUB_STEER_CAPTURE");
    std::env::remove_var("MUNIMENT_PI_ROOT");
    fs::remove_dir_all(temporary_root).unwrap();
}
