#![cfg(target_os = "linux")]

use muniment_attach::{connect_desktop_client_at, Operation};
use muniment_core::attach::linux::AttachFilesystem;
use muniment_runtime::run_runtime_activation_with_desktop_executable;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

mod common;
use common::TemporaryProfile;

#[test]
fn fresh_profile_serves_session_status_and_releases_the_endpoint() {
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let profile = TemporaryProfile::new("activation", false);
    fs::set_permissions(&profile.root, fs::Permissions::from_mode(0o700)).unwrap();
    let filesystem = AttachFilesystem::from_runtime_directory(&profile.root).unwrap();
    let endpoint = filesystem.endpoint_path().to_owned();
    drop(filesystem);
    let (stop_tx, stop_rx) = mpsc::channel();

    thread::scope(|scope| {
        let activation = scope.spawn(|| {
            run_runtime_activation_with_desktop_executable(
                &profile.root,
                &profile.profile,
                &profile.config,
                Instant::now() + Duration::from_secs(2),
                Some(std::env::current_exe().unwrap()),
                stop_rx,
            )
            .unwrap()
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut client = loop {
            match connect_desktop_client_at(&endpoint, "1.0.0", Duration::from_secs(1)) {
                Ok(client) => break client,
                Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                Err(error) => panic!("runtime activation did not accept the client: {error}"),
            }
        };
        let response = client
            .request(
                Operation::SessionStatus,
                None,
                std::iter::empty::<(String, u64)>().collect(),
            )
            .unwrap();
        assert_eq!(response.body["signed_in"], false);

        drop(client);
        stop_tx.send(()).unwrap();
        activation.join().unwrap();
    });

    let filesystem = AttachFilesystem::from_runtime_directory(&profile.root).unwrap();
    let lock = filesystem.acquire_instance_lock().unwrap();
    drop(lock);
    assert!(std::os::unix::net::UnixStream::connect(endpoint).is_err());
}
