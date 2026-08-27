#![cfg(target_os = "windows")]

use std::time::{Duration, Instant};

use muniment_core::attach::{
    connect_windows_attach_endpoint, WindowsAttachConnectError, WindowsAttachListener,
};

#[test]
fn connects_to_a_live_current_user_endpoint_and_bounds_a_busy_wait() {
    let listener = WindowsAttachListener::bind().unwrap();
    let stream = connect_windows_attach_endpoint(Instant::now() + Duration::from_secs(1)).unwrap();

    let busy = connect_windows_attach_endpoint(Instant::now() + Duration::from_millis(20));
    assert!(matches!(
        busy,
        Err(WindowsAttachConnectError::DeadlineExpired)
    ));

    drop(stream);
    drop(listener);
    let absent = connect_windows_attach_endpoint(Instant::now() + Duration::from_secs(1));
    assert!(matches!(
        absent,
        Err(WindowsAttachConnectError::EndpointAbsent)
    ));
}

#[test]
fn rejects_an_expired_deadline_before_opening_the_endpoint() {
    let result = connect_windows_attach_endpoint(Instant::now());

    assert!(matches!(
        result,
        Err(WindowsAttachConnectError::DeadlineExpired)
    ));
}
