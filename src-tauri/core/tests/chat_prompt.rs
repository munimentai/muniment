#![cfg(feature = "keyring")]

use muniment_core::chat_prompt::{load_prompt, prompt_user, store_prompt};
use std::sync::Once;
use uuid::Uuid;

fn use_mock_keyring() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
    });
}

#[test]
fn key_includes_subject() {
    assert_eq!(
        prompt_user(Some("subject-a"), "run-a"),
        "protected-prompts:subject-a:run-a"
    );
}

#[test]
fn key_omits_missing_subject() {
    assert_eq!(prompt_user(None, "run-a"), "protected-prompts:run-a");
}

#[test]
fn store_then_load_round_trip() {
    use_mock_keyring();
    let run_id = Uuid::now_v7().to_string();

    store_prompt(&run_id, "saved prompt", Some("subject-a")).unwrap();

    assert_eq!(
        load_prompt(&run_id, Some("subject-a")).unwrap(),
        Some("saved prompt".to_string())
    );
}

#[test]
fn missing_entry_returns_none() {
    use_mock_keyring();
    let run_id = Uuid::now_v7().to_string();

    assert_eq!(load_prompt(&run_id, None).unwrap(), None);
}
