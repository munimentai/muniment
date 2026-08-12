use std::fs;
use std::sync::{Arc, Mutex};

use muniment_core::chat_grant::ChatGrant;
use muniment_core::journal::EventPayload;
use muniment_runtime::{accept_prompt, open_profile_storage};

#[test]
fn memory_session_failure_ends_the_prepared_run() {
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let temporary_root = std::env::temp_dir().join(format!(
        "muniment-runtime-preparation-failure-{}",
        std::process::id()
    ));
    let profile = temporary_root.join("profile");
    let config = temporary_root.join("config");
    fs::create_dir_all(&profile).unwrap();
    let storage = open_profile_storage(&profile).unwrap();
    let active = Arc::new(Mutex::new(None));
    let run_id = "018f0000-0000-7000-8000-000000000001";

    let result = accept_prompt(
        &profile,
        Arc::clone(&storage),
        &config,
        run_id.into(),
        "prompt".into(),
        None,
        "token".into(),
        Some("owner".into()),
        Vec::new(),
        ChatGrant {
            workspace: "workspace-a".into(),
            gateway_url: "https://gateway.example.com".into(),
            virtual_key: "virtual-key".into(),
            model: None,
            minimum_cacheable_prefix_characters: 8_192,
            receipt_url: "https://receipts.example.com".into(),
        },
        Arc::clone(&active),
        None,
        None,
    );
    let error = match result {
        Ok(_) => panic!("the memory session opened without a confirmed Home"),
        Err(error) => error,
    };

    assert_eq!(error, "Conversation history is unavailable.");
    assert!(active.lock().unwrap().is_none());
    let events = storage.lock().unwrap().journal.events(run_id).unwrap();
    let failed = events.last().unwrap();
    assert_eq!(failed.run_seq, 2);
    assert_eq!(failed.event_type, "run.failed");
    match &failed.payload {
        EventPayload::Inline { payload_json } => {
            assert_eq!(
                payload_json.get("reason").and_then(|value| value.as_str()),
                Some("persistence")
            );
        }
        _ => panic!("run.failed did not use an inline payload"),
    }
    drop(events);
    drop(storage);

    let reopened = open_profile_storage(&profile).unwrap();
    let events = reopened.lock().unwrap().journal.events(run_id).unwrap();
    assert!(events
        .iter()
        .all(|event| event.event_type != "run.needs_attention"));
    drop(events);
    drop(reopened);

    fs::remove_dir_all(temporary_root).unwrap();
}
