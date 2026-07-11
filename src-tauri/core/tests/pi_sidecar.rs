use std::time::{Duration, Instant};

use muniment_core::sidecar::{
    pi_readiness_probe, pi_sidecar_config, SidecarStatus, SidecarSupervisor, PI_VERSION,
};

/// Opt-in because release/CI jobs must acquire the pinned executable rather
/// than committing it. Example:
/// `MUNIMENT_PI_EXECUTABLE=/path/to/pi cargo test --test pi_sidecar`
#[test]
fn real_pinned_pi_reaches_ready_through_the_supervisor() {
    let Ok(executable) = std::env::var("MUNIMENT_PI_EXECUTABLE") else {
        eprintln!("skipping real Pi {PI_VERSION} spawn: MUNIMENT_PI_EXECUTABLE is not set");
        return;
    };

    let mut config = pi_sidecar_config(executable);
    config.health_interval = Duration::from_secs(60);
    let mut supervisor =
        SidecarSupervisor::spawn(config, pi_readiness_probe(Duration::from_secs(10)))
            .expect("spawn pinned Pi artifact");

    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline
        && !matches!(
            supervisor.status(),
            SidecarStatus::Healthy | SidecarStatus::Failed
        )
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(supervisor.status(), SidecarStatus::Healthy);
    supervisor.shutdown().expect("shut down Pi");
}
