// A declined prompt is a macOS keychain status, so this runs on macOS.
#![cfg(all(feature = "keyring", target_os = "macos"))]

use muniment_core::chat_prompt::{
    keychain_refusals, load_prompt, retry_keychain, store_prompt, use_declining_keyring_for_tests,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[test]
fn a_refused_prompt_store_asks_the_keychain_once_until_a_retry() {
    let writes = Arc::new(AtomicUsize::new(0));
    use_declining_keyring_for_tests(-128, Arc::clone(&writes));
    assert!(store_prompt("run-1", "first", None).is_err());
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    // Later prompts and reads answer the held refusal without the keychain,
    // whose refused read would panic.
    for index in 0..10 {
        assert!(store_prompt(&format!("run-{index}"), "again", None).is_err());
        assert!(load_prompt(&format!("run-{index}"), None).is_err());
    }
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    let refusals = keychain_refusals();
    assert_eq!(refusals.len(), 1);
    assert_eq!(refusals[0].service, "ai.muniment.desktop.chat");
    // A retry asks the keychain once more.
    retry_keychain();
    assert!(keychain_refusals().is_empty());
    assert!(store_prompt("run-2", "retry", None).is_err());
    assert_eq!(writes.load(Ordering::SeqCst), 2);
}
