#![cfg(target_os = "linux")]

use muniment_attach::{decode_frame, Request};
use muniment_core::attach::linux::{AttachFilesystem, AttachTransport};
use muniment_core::attach::probe_handoff;
use muniment_runtime::{run_migration_takeover, MigrationTakeoverError};
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
        let request = complete_handshake(&mut stream);
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

#[test]
fn retries_migration_not_ready_before_acceptance() {
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
        let (mut first, _) = desktop.accept().unwrap();
        let first_request = complete_handshake(&mut first);
        write_error(&mut first, &first_request, "migration_not_ready", true);
        drop(first);

        let (mut second, _) = desktop.accept().unwrap();
        let second_request = complete_handshake(&mut second);
        let nonce = second_request.body["handoff_nonce"]
            .as_str()
            .unwrap()
            .to_owned();
        write_success(&mut second, &second_request, &nonce);
        drop(second);
        drop(desktop);
        drop(desktop_lock);

        assert!(probe_handoff(&endpoint, &nonce, Instant::now() + test_timeout).is_ok());
        stop_tx.send(()).unwrap();
        takeover.join().unwrap();
    });
}

#[test]
fn maps_nonretryable_control_answers_to_typed_errors() {
    let cases = [
        (
            "migration_not_ready",
            false,
            MigrationTakeoverError::MigrationNotReady,
        ),
        ("unauthorized", false, MigrationTakeoverError::Unauthorized),
        (
            "unsupported_operation",
            false,
            MigrationTakeoverError::UnsupportedOperation,
        ),
    ];
    let test_timeout = Duration::from_secs(2);

    for (code, retryable, expected) in cases {
        let runtime = RuntimeDirectory::new();
        let filesystem = AttachFilesystem::from_runtime_directory(&runtime.0).unwrap();
        let _desktop_lock = filesystem.acquire_instance_lock().unwrap();
        let desktop = AttachTransport::bind(&filesystem).unwrap();
        let (_stop_tx, stop_rx) = mpsc::channel();

        thread::scope(|scope| {
            let takeover = scope.spawn(|| {
                run_migration_takeover(&runtime.0, Instant::now() + test_timeout, stop_rx)
            });
            let (mut stream, _) = desktop.accept().unwrap();
            let request = complete_handshake(&mut stream);
            write_error(&mut stream, &request, code, retryable);
            assert_eq!(takeover.join().unwrap().unwrap_err(), expected);
        });
    }
}

#[test]
fn stalled_desktop_cannot_extend_the_caller_deadline() {
    let runtime = RuntimeDirectory::new();
    let filesystem = AttachFilesystem::from_runtime_directory(&runtime.0).unwrap();
    let _desktop_lock = filesystem.acquire_instance_lock().unwrap();
    let desktop = AttachTransport::bind(&filesystem).unwrap();
    let (_stop_tx, stop_rx) = mpsc::channel();
    let caller_deadline = Duration::from_millis(100);
    let test_timeout = Duration::from_secs(1);
    let started = Instant::now();

    thread::scope(|scope| {
        let takeover =
            scope.spawn(|| run_migration_takeover(&runtime.0, started + caller_deadline, stop_rx));
        let (mut stream, _) = desktop.accept().unwrap();
        let _request = complete_handshake(&mut stream);
        assert_eq!(
            takeover.join().unwrap().unwrap_err(),
            MigrationTakeoverError::DeadlineElapsed
        );
    });
    assert!(started.elapsed() < test_timeout);
}

fn complete_handshake(stream: &mut impl ReadWrite) -> Request {
    let _hello: muniment_attach::Hello = read_hello(stream);
    write_json_frame(
        stream,
        r#"{"selected":1,"desktop_version":"0.0.1","server_nonce":"11111111111111111111111111111111","authorization":"authorized","approval_challenge":""}"#,
    );
    write_json_frame(
        stream,
        r#"{"profile_id":"","capability":"3333333333333333333333333333333333333333333333333333333333333333","expires_at":60,"idle_timeout_seconds":60,"workspace_scopes":{}}"#,
    );
    read_request(stream)
}

fn write_success(stream: &mut impl Write, request: &Request, nonce: &str) {
    let response = format!(
        r#"{{"protocol":"muniment.attach/1","request_id":"{}","ok":true,"body":{{"handoff_nonce":"{nonce}"}}}}"#,
        request.request_id.as_str()
    );
    write_json_frame(stream, &response);
}

fn write_error(stream: &mut impl Write, request: &Request, code: &str, retryable: bool) {
    let message = match code {
        "migration_not_ready" => "The desktop cannot hand off ownership yet.",
        "unauthorized" => "The capability is not authorized.",
        "unsupported_operation" => "The operation is not supported.",
        _ => unreachable!(),
    };
    let response = format!(
        r#"{{"protocol":"muniment.attach/1","request_id":"{}","ok":false,"error":{{"code":"{code}","message":"{message}","retryable":{retryable}}}}}"#,
        request.request_id.as_str()
    );
    write_json_frame(stream, &response);
}

trait ReadWrite: Read + Write {}
impl<T: Read + Write> ReadWrite for T {}

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
