use std::fs;
use std::sync::mpsc;

use muniment_core::chat_grant::ChatGrant;
use muniment_runtime::{open_profile_storage, run_prompt};

#[test]
fn settles_and_delivers_events_when_the_runtime_is_missing() {
    std::env::remove_var("MUNIMENT_PI_ROOT");
    let profile = std::env::temp_dir().join(format!("muniment-runtime-run-{}", std::process::id()));
    fs::create_dir(&profile).unwrap();
    let run_id = "018f0000-0000-7000-8000-000000000003";
    let (subscriber, events) = mpsc::channel();

    run_prompt(
        &profile,
        run_id.into(),
        "hello".into(),
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
    assert!(!delivered.is_empty());
    assert_eq!(delivered.last().unwrap().phase, "failed");
    let storage = open_profile_storage(&profile).unwrap();
    let journal_events = storage.lock().unwrap().journal.events(run_id).unwrap();
    assert_eq!(journal_events.last().unwrap().event_type, "run.failed");
    assert_eq!(journal_events[0].provenance.source, "muniment-runtime");

    drop(storage);
    fs::remove_dir_all(profile).unwrap();
}
