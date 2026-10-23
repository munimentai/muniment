use muniment_runtime_core::{RuntimeAttachState, RuntimeHost};
use std::sync::mpsc;
mod common;
use common::CI_WAIT;

#[test]
fn opening_the_runtime_names_legacy_session_folders() {
    let temporary = common::TemporaryProfile::new("session-names", false);
    let profile = &temporary.profile;
    let home = temporary.root.join("muniment");
    muniment_core::home::confirm_home(profile, &home).unwrap();
    let legacy = home.join("sessions").join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(legacy.join("notes.md"), "kept").unwrap();

    let state = RuntimeAttachState::open(RuntimeHost::default(), profile, profile).unwrap();
    state.wait_for_session_names();

    assert!(!legacy.exists());
    let renamed: Vec<_> = std::fs::read_dir(home.join("sessions"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.join("notes.md").is_file())
        .collect();
    assert_eq!(renamed.len(), 1);
    assert!(renamed[0]
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("Thread-"));
}

#[test]
fn a_session_folder_read_that_waits_does_not_hold_the_runtime_start() {
    let temporary = common::TemporaryProfile::new("session-names-wait", false);
    let profile = temporary.profile.clone();
    // The pass stands in for a Home read that macOS holds until the user
    // answers its folder prompt.
    let (release, held) = mpsc::channel::<()>();
    let (opened_tx, opened) = mpsc::channel();
    std::thread::spawn(move || {
        let state = RuntimeAttachState::open_with_session_names(
            RuntimeHost::default(),
            &profile,
            &profile,
            move |_| {
                let _ = held.recv();
                Ok(())
            },
        );
        let _ = opened_tx.send(state.map(|state| {
            let service = state.attach_service().is_ok();
            (state, service)
        }));
    });

    let (state, service) = opened
        .recv_timeout(CI_WAIT)
        .expect("the runtime opens while the session folder read waits")
        .unwrap();
    assert!(service, "a companion connection gets a service at once");
    release.send(()).unwrap();
    state.wait_for_session_names();
}
