#![allow(dead_code)]

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use muniment_core::auth::{InstallationRecord, NativeCredentials, TokenSet};
use muniment_core::chat_grant::ChatGrant;
use muniment_core::sidecar::pi_install::{PiArtifactDescriptor, PI_ARTIFACT};

const DEVICE_ID: &str = "10000000-0000-4000-8000-000000000001";
static TEMPORARY_PROFILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub struct TemporaryProfile {
    pub root: PathBuf,
    pub profile: PathBuf,
    pub config: PathBuf,
}

impl TemporaryProfile {
    pub fn new(test_name: &str, scaffold_home: bool) -> Self {
        let sequence = TEMPORARY_PROFILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "muniment-runtime-{test_name}-{}-{sequence}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let profile = root.join("profile");
        let config = root.join("config");
        let temporary_profile = Self {
            root,
            profile,
            config,
        };
        fs::create_dir_all(&temporary_profile.profile).unwrap();
        fs::create_dir_all(&temporary_profile.config).unwrap();
        if scaffold_home {
            muniment_core::home::confirm_home(
                &temporary_profile.config,
                &temporary_profile.root.join("home"),
            )
            .unwrap();
        }
        temporary_profile
    }
}

impl Drop for TemporaryProfile {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub fn fixture_grant() -> ChatGrant {
    ChatGrant {
        workspace: "workspace-a".into(),
        gateway_url: "https://gateway.example.com".into(),
        virtual_key: "virtual-key".into(),
        model: None,
        minimum_cacheable_prefix_characters: 8_192,
        receipt_url: "https://receipts.example.com".into(),
    }
}

pub fn spawn_server(status: u16, body: String) -> (String, thread::JoinHandle<String>) {
    spawn_server_with(status, body, || {})
}

pub fn spawn_server_with<F>(
    status: u16,
    body: String,
    before_response: F,
) -> (String, thread::JoinHandle<String>)
where
    F: FnOnce() + Send + 'static,
{
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_request(&mut stream);
        before_response();
        write!(
            stream,
            "HTTP/1.1 {status} Result\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        request
    });
    (base_url, handle)
}

pub fn read_request(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    loop {
        let mut buffer = [0; 1024];
        let read = stream.read(&mut buffer).unwrap();
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8(bytes).unwrap()
}

pub fn credentials() -> NativeCredentials {
    let unix_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    credentials_with_expiry(unix_time + 3_600)
}

pub fn credentials_with_expiry(expires_at: u64) -> NativeCredentials {
    NativeCredentials {
        installation: InstallationRecord {
            private_key: [1; 32],
            device_id: DEVICE_ID.parse().unwrap(),
            registration_token: "registration-secret".into(),
            device_challenge: "challenge-secret".into(),
            registration_expires_at: expires_at + 3_600,
        },
        tokens: TokenSet {
            access_token: "access-secret".into(),
            refresh_token: Some("refresh-secret".into()),
            expires_at: Some(expires_at),
            subject: Some("user".into()),
        },
        refresh_expires_at: expires_at + 3_600,
    }
}

pub fn stage_pi_stub(temporary_root: &Path) -> PiArtifactDescriptor {
    let pi_root = temporary_root.join("pi");
    let revision_root = pi_root.join("revisions").join(PI_ARTIFACT.version);
    let staged_stub = revision_root.join(PI_ARTIFACT.executable);
    fs::create_dir_all(staged_stub.parent().unwrap()).unwrap();

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut workspace_hasher = DefaultHasher::new();
    manifest_dir.hash(&mut workspace_hasher);
    let build_root = std::env::temp_dir().join(format!(
        "muniment-runtime-sidecar-test-stub-{:x}",
        workspace_hasher.finish()
    ));
    let status = Command::new(env!("CARGO"))
        .args([
            "build",
            "--quiet",
            "--package",
            "muniment-core",
            "--bin",
            "sidecar-test-stub",
            "--target-dir",
        ])
        .arg(&build_root)
        .current_dir(manifest_dir)
        .status()
        .unwrap();
    assert!(status.success());

    let stub_name = if cfg!(windows) {
        "sidecar-test-stub.exe"
    } else {
        "sidecar-test-stub"
    };
    let built_stub = build_root.join("debug").join(stub_name);
    fs::copy(built_stub, staged_stub).unwrap();
    let stub_archive = b"muniment-sidecar-test-stub\n";
    fs::write(revision_root.join(PI_ARTIFACT.archive), stub_archive).unwrap();
    fs::write(
        pi_root.join("current"),
        format!("muniment-pi-pointer-v1\n{}\n", PI_ARTIFACT.version),
    )
    .unwrap();
    std::env::set_var("MUNIMENT_PI_ROOT", pi_root);

    PiArtifactDescriptor {
        version: PI_ARTIFACT.version,
        archive: PI_ARTIFACT.archive,
        byte_size: stub_archive.len() as u64,
        sha256: "758b0db8f6304639edfca2b779e886f3006afeb006417e49dd6bce53ff2a65ab",
        executable: PI_ARTIFACT.executable,
    }
}
