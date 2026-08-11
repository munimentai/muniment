use std::fs;
use std::process::Command;
use std::sync::mpsc;

use muniment_core::chat_grant::ChatGrant;
use muniment_core::home::confirm_home;
use muniment_core::sidecar::pi_install::{PiArtifactDescriptor, PI_ARTIFACT};
use muniment_runtime::{open_profile_storage, run_prompt};

#[test]
fn settles_after_the_subscriber_is_dropped() {
    let temporary_root =
        std::env::temp_dir().join(format!("muniment-runtime-run-{}", std::process::id()));
    let profile = temporary_root.join("profile");
    let pi_root = temporary_root.join("pi");
    let executable = pi_root
        .join("revisions")
        .join(PI_ARTIFACT.version)
        .join(PI_ARTIFACT.executable);
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::create_dir_all(&profile).unwrap();
    confirm_home(&profile, &temporary_root.join("home")).unwrap();
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

    let run_id = "018f0000-0000-7000-8000-000000000003";
    let prompt = "pointer install prompt";
    let (subscriber, events) = mpsc::channel();
    drop(events);
    run_prompt(
        &profile,
        run_id.into(),
        prompt.into(),
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
        Some(subscriber),
        Some(descriptor),
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(captured_prompts).unwrap(),
        format!("{prompt}\n")
    );
    let args = fs::read_to_string(captured_args).unwrap();
    let extension = profile.join("memory").join("memory-search-extension.js");
    assert!(args
        .lines()
        .collect::<Vec<_>>()
        .windows(2)
        .any(|args| { args == ["--extension", extension.to_string_lossy().as_ref()] }));
    let storage = open_profile_storage(&profile).unwrap();
    let journal_events = storage.lock().unwrap().journal.events(run_id).unwrap();
    assert_eq!(journal_events.last().unwrap().event_type, "run.failed");
    assert!(journal_events
        .iter()
        .all(|event| event.provenance.source == "muniment-runtime"));

    drop(storage);
    std::env::remove_var("MUNIMENT_PI_ROOT");
    std::env::remove_var("PI_RESUME_STUB_PROMPTS");
    std::env::remove_var("PI_RESUME_STUB_ARGS");
    fs::remove_dir_all(temporary_root).unwrap();
}
