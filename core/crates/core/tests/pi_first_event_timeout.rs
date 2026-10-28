//! The first event bound reads its environment once, so this binary sets the
//! variable before anything reads it.

use std::time::{Duration, Instant};

use muniment_core::sidecar::pi_chat::{
    first_event_timeout, first_event_timeout_reason, PiRunAdapter,
};
use muniment_core::sidecar::{PiRpcWiring, SidecarConfig, SidecarStatus, SidecarSupervisor};

#[test]
fn a_host_raises_the_first_event_bound_from_the_environment() {
    std::env::set_var("MUNIMENT_PI_FIRST_EVENT_TIMEOUT_S", "6");
    assert_eq!(first_event_timeout(), Duration::from_secs(6));
    assert_eq!(
        first_event_timeout_reason(),
        "No reply arrived within 6 seconds. Try again."
    );

    let root = std::env::temp_dir().join(format!("muniment-first-event-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut config = SidecarConfig::new(env!("CARGO_BIN_EXE_sidecar-test-stub"));
    config.args = vec![
        "pi-chat-capture".into(),
        root.join("prompt.json").to_string_lossy().into_owned(),
    ];
    config.health_interval = Duration::from_secs(60);
    let wiring = PiRpcWiring::new();
    let mut supervisor =
        SidecarSupervisor::spawn(config, wiring.readiness_probe(Duration::from_millis(100)))
            .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while supervisor.status() != SidecarStatus::Healthy && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(supervisor.status(), SidecarStatus::Healthy);
    let transport = wiring.transport().unwrap();

    // The stub acknowledges the prompt and never sends a reply event.
    let started = Instant::now();
    let (adapter, _) =
        PiRunAdapter::start("run-slow", &transport, "prompt", Duration::from_secs(1)).unwrap();
    let error = loop {
        match adapter.next(Duration::from_secs(1)) {
            Err(error) if error == "timed out waiting for Pi stream" => {}
            Err(error) => break error,
            Ok(event) => panic!("unexpected event {event:?}"),
        }
    };
    let elapsed = started.elapsed();
    assert_eq!(error, "No reply arrived within 6 seconds. Try again.");
    assert!(elapsed >= Duration::from_secs(6), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(20), "{elapsed:?}");

    supervisor.shutdown().unwrap();
    let _ = std::fs::remove_dir_all(root);
}
