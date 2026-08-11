use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

use muniment_core::chat_grant::ChatGrant;
use muniment_core::memory_runtime::ApplicationMemoryRuntime;
use muniment_core::pi_launch::{
    pi_launch_config, pi_launch_config_for_executable, PiLaunchBoundaries, PiLaunchError,
};
use muniment_core::run_events::{ChatEvent, ChatEventSink};
use muniment_runtime::{open_profile_storage, RuntimeChatEventSink};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct ProfileDirectory(PathBuf);

impl ProfileDirectory {
    fn new() -> Self {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "muniment-runtime-sink-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for ProfileDirectory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn event() -> ChatEvent {
    ChatEvent {
        run_id: "run-1".into(),
        phase: "running".into(),
        text: "hello".into(),
        receipt: None,
        tool_activity: Vec::new(),
        attachments: Vec::new(),
        recalls: Vec::new(),
        applied_diffs: Vec::new(),
        pending_permission: None,
    }
}

fn grant() -> ChatGrant {
    ChatGrant {
        workspace: "/work".into(),
        gateway_url: "https://gateway.example.com".into(),
        virtual_key: "secret-key".into(),
        model: None,
        minimum_cacheable_prefix_characters: 8_192,
        receipt_url: "https://receipts.example.com".into(),
    }
}

fn memory_runtime(profile: &ProfileDirectory) -> Arc<ApplicationMemoryRuntime> {
    Arc::new(ApplicationMemoryRuntime::new(
        profile.0.clone(),
        profile.0.join("memory"),
    ))
}

#[test]
fn delivers_to_an_optional_subscriber() {
    let profile = ProfileDirectory::new();
    let (subscriber, events) = mpsc::channel();
    let sink = RuntimeChatEventSink::new(&profile.0, Some(subscriber), memory_runtime(&profile));

    sink.deliver(event()).unwrap();

    let delivered = events.recv().unwrap();
    assert_eq!(delivered.run_id, "run-1");
    assert_eq!(delivered.text, "hello");
}

#[test]
fn succeeds_without_a_subscriber() {
    let profile = ProfileDirectory::new();

    RuntimeChatEventSink::new(&profile.0, None, memory_runtime(&profile))
        .deliver(event())
        .unwrap();
}

#[test]
fn drives_pi_launch_config_over_the_profile_directory() {
    let profile = ProfileDirectory::new();
    let _storage = open_profile_storage(&profile.0).unwrap();
    let pi_install = profile.0.join("pi-install");
    fs::create_dir(&pi_install).unwrap();
    let memory_runtime = memory_runtime(&profile);
    let extension = memory_runtime.agent_extension_path();
    fs::create_dir_all(extension.parent().unwrap()).unwrap();
    fs::write(&extension, "export default function () {}\n").unwrap();
    let sink = RuntimeChatEventSink::new(&profile.0, None, memory_runtime);

    assert_eq!(
        pi_launch_config(&sink, Some(&pi_install), &grant(), None).unwrap_err(),
        PiLaunchError::UnresolvableExecutable
    );
    assert_eq!(
        sink.pi_session_root().unwrap(),
        profile.0.join("pi-sessions")
    );
    assert_eq!(sink.memory_agent_extension_path(), Some(extension.clone()));
    let config = pi_launch_config_for_executable(&sink, "pi".into(), &grant(), None).unwrap();
    assert!(config.args.windows(2).any(|args| {
        args == [
            "--session-dir",
            profile.0.join("pi-sessions").to_string_lossy().as_ref(),
        ]
    }));
    assert!(config
        .args
        .windows(2)
        .any(|args| { args == ["--extension", extension.to_string_lossy().as_ref()] }));
}
