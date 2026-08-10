#![cfg(target_os = "linux")]

use muniment_attach::{decode_frame, Request};
use muniment_core::attach::linux::{AttachFilesystem, AttachTransport};
use muniment_core::attach::probe_handoff;
use muniment_runtime::run_migration_takeover;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
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
            "muniment-runtime-migration-{}-{sequence}",
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
fn takes_over_the_endpoint_with_the_minted_nonce() {
    let runtime = RuntimeDirectory::new();
    let filesystem = AttachFilesystem::from_runtime_directory(&runtime.0).unwrap();
    let endpoint = filesystem.endpoint_path().to_owned();
    let desktop_lock = filesystem.acquire_instance_lock().unwrap();
    let desktop = AttachTransport::bind(&filesystem).unwrap();
    let (stop_tx, stop_rx) = mpsc::channel();
    let test_timeout = Duration::from_secs(2);

    thread::scope(|scope| {
        let takeover = scope.spawn(|| {
            run_migration_takeover(&runtime.0, Instant::now() + test_timeout, stop_rx).unwrap()
        });
        let (mut stream, _) = desktop.accept().unwrap();
        let _hello: muniment_attach::Hello = read_hello(&mut stream);
        write_json_frame(
            &mut stream,
            r#"{"selected":1,"desktop_version":"0.0.1","server_nonce":"11111111111111111111111111111111","authorization":"authorized","approval_challenge":""}"#,
        );
        write_json_frame(
            &mut stream,
            r#"{"profile_id":"","capability":"3333333333333333333333333333333333333333333333333333333333333333","expires_at":60,"idle_timeout_seconds":60,"workspace_scopes":{}}"#,
        );
        let request = read_request(&mut stream);
        let nonce = request.body["handoff_nonce"].as_str().unwrap().to_owned();
        let response = format!(
            r#"{{"protocol":"muniment.attach/1","request_id":"{}","ok":true,"body":{{"handoff_nonce":"{nonce}"}}}}"#,
            request.request_id.as_str()
        );
        write_json_frame(&mut stream, &response);

        drop(stream);
        drop(desktop);
        drop(desktop_lock);
        assert!(probe_handoff(&endpoint, &nonce, Instant::now() + test_timeout).is_ok());

        stop_tx.send(()).unwrap();
        takeover.join().unwrap();
    });
}

fn read_bytes(stream: &mut impl Read) -> Vec<u8> {
    let mut prefix = [0_u8; 4];
    stream.read_exact(&mut prefix).unwrap();
    let mut frame = vec![0_u8; 4 + u32::from_be_bytes(prefix) as usize];
    frame[..4].copy_from_slice(&prefix);
    stream.read_exact(&mut frame[4..]).unwrap();
    frame
}

fn read_hello(stream: &mut impl Read) -> muniment_attach::Hello {
    decode_frame(&read_bytes(stream)).unwrap().unwrap().0
}

fn read_request(stream: &mut impl Read) -> Request {
    decode_frame(&read_bytes(stream)).unwrap().unwrap().0
}

fn write_json_frame(stream: &mut impl Write, json: &str) {
    stream
        .write_all(&(json.len() as u32).to_be_bytes())
        .unwrap();
    stream.write_all(json.as_bytes()).unwrap();
}
