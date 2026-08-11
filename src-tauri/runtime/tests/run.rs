use std::fs;
use std::process::Command;
use std::sync::mpsc;

use muniment_core::chat_grant::ChatGrant;
use muniment_core::sidecar::pi_install::PI_ARTIFACT;
use muniment_runtime::{open_profile_storage, run_prompt};

#[test]
fn settles_after_driving_the_prompt_through_the_pointer_install() {
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
    let build_root = temporary_root.join("build");
    let status = Command::new(env!("CARGO"))
        .args([
            "build",
            "--quiet",
            "--package",
            "muniment-runtime",
            "--example",
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
    fs::copy(
        build_root.join("debug/examples").join(stub_name),
        &executable,
    )
    .unwrap();
    fs::write(
        pi_root.join("current"),
        format!("muniment-pi-pointer-v1\n{}\n", PI_ARTIFACT.version),
    )
    .unwrap();
    let captured_prompts = temporary_root.join("prompts");
    std::env::set_var("MUNIMENT_PI_ROOT", &pi_root);
    std::env::set_var("PI_RESUME_STUB_PROMPTS", &captured_prompts);

    let run_id = "018f0000-0000-7000-8000-000000000003";
    let prompt = "pointer install prompt";
    let (subscriber, events) = mpsc::channel();
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
    )
    .unwrap();

    let delivered: Vec<_> = events.try_iter().collect();
    assert!(delivered.iter().any(|event| event.text.contains("resumed")));
    assert_eq!(
        fs::read_to_string(captured_prompts).unwrap(),
        format!("{prompt}\n")
    );
    assert_eq!(delivered.last().unwrap().phase, "failed");
    let storage = open_profile_storage(&profile).unwrap();
    let journal_events = storage.lock().unwrap().journal.events(run_id).unwrap();
    assert_eq!(journal_events.last().unwrap().event_type, "run.failed");
    assert_eq!(journal_events[0].provenance.source, "muniment-runtime");

    drop(storage);
    std::env::remove_var("MUNIMENT_PI_ROOT");
    std::env::remove_var("PI_RESUME_STUB_PROMPTS");
    fs::remove_dir_all(temporary_root).unwrap();
}
