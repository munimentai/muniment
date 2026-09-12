use std::process::Command;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use muniment_core::attach::RuntimeActivityRegistry;
use muniment_core::auth::{
    EntitlementSnapshotTracker, KeyringNativeCredentialStore, NativeCredentialStore,
};
use muniment_core::session_thread::SessionThread;
use muniment_core::sidecar::SidecarStatus;
use muniment_runtime::{
    accept_prompt, drive_prompt, open_profile_storage, session_status, sign_out,
    RuntimeChatEventTarget,
};

mod common;
use common::{credentials, fixture_grant, spawn_server, stage_pi_stub, TemporaryProfile};

const RUN_ID: &str = "01900000-0000-7000-8000-000000000031";

#[test]
fn signed_in_starting_pi_rejects_and_releases_the_runtime() {
    if std::env::var_os("MUNIMENT_READINESS_TEST_CHILD").is_some() {
        run_starting_pi();
        return;
    }
    // Isolate the stub controls and capture the runtime envelope by run id.
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "signed_in_starting_pi_rejects_and_releases_the_runtime",
            "--nocapture",
        ])
        .env("MUNIMENT_READINESS_TEST_CHILD", "1")
        .env("PI_STUB_STAY_STARTING", "1")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{log}");
    let lines: Vec<_> = log.lines().filter(|line| line.contains(RUN_ID)).collect();
    assert!(lines.iter().any(|line| line.contains("Starting")));
    assert!(!lines.iter().any(|line| line.contains("Healthy")));
    let rejection = lines
        .iter()
        .position(|line| {
            line.contains("pi_spawn rejected cause=pi_not_ready")
                && line.contains("readiness_bound_ms=30000")
        })
        .expect("The envelope must name the readiness failure and its bound.");
    let stderr = lines
        .iter()
        .position(|line| {
            line.contains("pi_stderr_tail=")
                && line.contains("Pi stub cannot finish cloud extension setup.")
        })
        .expect("The envelope must contain the Pi stderr tail.");
    let shell = lines
        .iter()
        .position(|line| line.starts_with("shell-failure:"))
        .unwrap();
    assert!(rejection < shell && stderr < shell, "{log}");
    assert!(lines
        .iter()
        .any(|line| { line.contains("provider_request outcome=not_started_pi_not_ready") }));
}

fn run_starting_pi() {
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let store = KeyringNativeCredentialStore::new();
    store.save_credentials(&credentials()).unwrap();
    let profile = TemporaryProfile::new("signed-in-readiness", true);
    let artifact = stage_pi_stub(&profile.root);
    let storage = open_profile_storage(&profile.profile).unwrap();
    let runtime = Arc::new(Mutex::new(None));
    let activity = RuntimeActivityRegistry::new();
    let active = Arc::new(Mutex::new(None));
    let (events_tx, events_rx) = mpsc::channel();
    let mut grant = fixture_grant();
    grant.model = Some("muniment-stub-chat".into());
    let (_, launch) = accept_prompt(
        &profile.profile,
        Arc::clone(&storage),
        Arc::clone(&runtime),
        &activity,
        &profile.config,
        RUN_ID.into(),
        "signed-in prompt".into(),
        None,
        &SessionThread::default(),
        false,
        "token".into(),
        Some("user".into()),
        Vec::new(),
        grant,
        Arc::clone(&active),
        RuntimeChatEventTarget::Subscriber(Some(events_tx)),
        Some(artifact),
    )
    .unwrap();
    let started = Instant::now();
    let (done_tx, done_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        drive_prompt(launch);
        done_tx.send(()).unwrap();
    });
    let failure = loop {
        let event = events_rx
            .recv_timeout(Duration::from_secs(31).saturating_sub(started.elapsed()))
            .expect("The shell must get the readiness rejection within 31 seconds.");
        if event.phase == "failed" {
            break event;
        }
    };
    assert!(failure.text.is_empty());
    let reason = failure.failure_reason.unwrap();
    eprintln!("shell-failure: run_id={RUN_ID} reason={reason}");
    assert!(
        reason == "Pi did not become ready within 30 seconds. Try again."
            || reason == "Pi stopped before it became ready. Try again.",
        "{reason}"
    );
    done_rx
        .recv_timeout(Duration::from_secs(36).saturating_sub(started.elapsed()))
        .expect("The runtime must release the run after the readiness rejection.");
    worker.join().unwrap();
    assert!(active.lock().unwrap().is_none());
    assert!(!activity.snapshot().active_run);
    assert_eq!(
        runtime
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .supervisor
            .status(),
        SidecarStatus::Stopped
    );
    let journal_events = storage.lock().unwrap().journal.events(RUN_ID).unwrap();
    let terminal = journal_events.last().unwrap();
    assert_eq!(terminal.event_type, "run.failed");
    let muniment_core::journal::EventPayload::Inline { payload_json } = &terminal.payload else {
        panic!("The journal must store the failure cause.");
    };
    assert_eq!(payload_json["reason"].as_str(), Some(reason.as_str()));

    // A second request can claim the slot without waiting for the failed Pi child.
    let (_, next) = accept_prompt(
        &profile.profile,
        Arc::clone(&storage),
        runtime,
        &activity,
        &profile.config,
        "01900000-0000-7000-8000-000000000032".into(),
        "next prompt".into(),
        None,
        &SessionThread::default(),
        false,
        "token".into(),
        Some("user".into()),
        Vec::new(),
        fixture_grant(),
        Arc::clone(&active),
        RuntimeChatEventTarget::Subscriber(None),
        Some(artifact),
    )
    .unwrap();
    active
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .cancelled
        .store(true, std::sync::atomic::Ordering::SeqCst);
    drive_prompt(next);
    assert!(active.lock().unwrap().is_none());
    let (base_url, server) = spawn_server(200, r#"{"ok":true}"#.into());
    std::env::set_var("MUNIMENT_API_BASE_URL", base_url);
    assert!(
        !sign_out(&EntitlementSnapshotTracker::new(), &activity)
            .unwrap()
            .signed_in
    );
    server.join().unwrap();
    assert!(!session_status().unwrap().signed_in);
    assert!(store.load_credentials().unwrap().is_none());
}
