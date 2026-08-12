use std::fs;
use std::process::Command;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use muniment_core::chat_grant::ChatGrant;
use muniment_core::home::confirm_home;
use muniment_core::sidecar::pi_install::{PiArtifactDescriptor, PI_ARTIFACT};
use muniment_runtime::{cancel_run, open_profile_storage, run_prompt};

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
fn cancelling_a_live_run_sends_abort_to_pi() {
    let _environment = ENVIRONMENT.lock().unwrap();
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let temporary_root =
        std::env::temp_dir().join(format!("muniment-runtime-cancel-{}", std::process::id()));
    let _ = fs::remove_dir_all(&temporary_root);
    let profile = temporary_root.join("profile");
    let config = temporary_root.join("config");
    fs::create_dir_all(&profile).unwrap();
    confirm_home(&config, &temporary_root.join("home")).unwrap();
    let descriptor = install_stub(&temporary_root);
    let abort_capture = temporary_root.join("abort.json");
    std::env::set_var("PI_RESUME_STUB_ABORT_CAPTURE", &abort_capture);
    std::env::set_var("PI_RESUME_STUB_MEMORY_QUERY", "hold the run open");

    let run_id = "018f0000-0000-7000-8000-000000000113";
    let storage = open_profile_storage(&profile).unwrap();
    let runtime = Arc::new(Mutex::new(None));
    let active = Arc::new(Mutex::new(None));
    let (subscriber, events) = mpsc::channel();
    std::thread::scope(|scope| {
        let run = scope.spawn(|| {
            run_prompt(
                &profile,
                Arc::clone(&storage),
                Arc::clone(&runtime),
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
        cancel_run(Arc::clone(&active), "workspace-a".into(), run_id.into()).unwrap();
        run.join().unwrap().unwrap();
    });

    let captured = fs::read_to_string(abort_capture).unwrap();
    assert!(captured.contains(r#""type":"abort""#));
    assert_eq!(
        cancel_run(Arc::clone(&active), "workspace-a".into(), run_id.into()),
        Err("That reply is no longer active.".into())
    );
    let journal_events = storage.lock().unwrap().journal.events(run_id).unwrap();
    assert_eq!(journal_events.last().unwrap().event_type, "run.cancelled");

    drop(journal_events);
    drop(storage);
    std::env::remove_var("PI_RESUME_STUB_ABORT_CAPTURE");
    std::env::remove_var("PI_RESUME_STUB_MEMORY_QUERY");
    std::env::remove_var("MUNIMENT_PI_ROOT");
    fs::remove_dir_all(temporary_root).unwrap();
}
