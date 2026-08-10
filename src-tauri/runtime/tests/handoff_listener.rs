#![cfg(target_os = "linux")]

use muniment_attach::{
    decode_frame, encode_frame, Client, Hello, Id, Protocol, VersionRange, Welcome,
};
use muniment_core::attach::linux::AttachFilesystem;
use muniment_core::attach::{probe_handoff, HandoffProbeError};
use muniment_runtime::handoff_listener::run_handoff_listener;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct RuntimeDirectory(PathBuf);

impl RuntimeDirectory {
    fn new() -> Self {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "muniment-runtime-handoff-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
}

impl Drop for RuntimeDirectory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn confirms_the_prepared_nonce_and_stops_cleanly() {
    let runtime = RuntimeDirectory::new();
    let endpoint = AttachFilesystem::from_runtime_directory(&runtime.0)
        .unwrap()
        .endpoint_path()
        .to_owned();
    let (stop_tx, stop_rx) = mpsc::channel();

    thread::scope(|scope| {
        let listener =
            scope.spawn(|| run_handoff_listener(&runtime.0, "prepared-nonce", stop_rx).unwrap());

        assert!(probe_handoff(
            &endpoint,
            "prepared-nonce",
            Instant::now() + Duration::from_secs(1),
        )
        .is_ok());
        assert_eq!(
            probe_handoff(
                &endpoint,
                "wrong-nonce",
                Instant::now() + Duration::from_secs(1),
            ),
            Err(HandoffProbeError::NonceMismatch)
        );

        let mut stream = UnixStream::connect(&endpoint).unwrap();
        let hello = Hello {
            protocol: Protocol,
            client: Client {
                kind: "test-probe".into(),
                version: "1.0.0".into(),
            },
            supported: VersionRange { min: 1, max: 1 },
            client_nonce: "client-nonce".into(),
            authorized_client_id: Id::new("018f0000-0000-7000-8000-000000000001").unwrap(),
            authorized_client_credential: None,
        };
        stream.write_all(&encode_frame(&hello).unwrap()).unwrap();
        let mut prefix = [0_u8; 4];
        stream.read_exact(&mut prefix).unwrap();
        let mut response = vec![0_u8; 4 + u32::from_be_bytes(prefix) as usize];
        response[..4].copy_from_slice(&prefix);
        stream.read_exact(&mut response[4..]).unwrap();
        let welcome: Welcome = decode_frame(&response).unwrap().unwrap().0;
        assert_eq!(welcome.handoff_nonce.as_deref(), Some("prepared-nonce"));
        let mut extra = [0_u8; 1];
        assert_eq!(stream.read(&mut extra).unwrap(), 0);

        stop_tx.send(()).unwrap();
        listener.join().unwrap();
    });

    assert!(!endpoint.exists());
    let filesystem = AttachFilesystem::from_runtime_directory(&runtime.0).unwrap();
    assert!(filesystem.acquire_instance_lock().is_ok());
}
