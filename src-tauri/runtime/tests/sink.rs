use std::fs;
use std::sync::mpsc;
use std::sync::Arc;

use muniment_core::chat_grant::ChatGrant;
use muniment_core::memory_runtime::ApplicationMemoryRuntime;
use muniment_core::pi_launch::{
    pi_launch_config, pi_launch_config_for_executable, PiLaunchBoundaries, PiLaunchError,
};
use muniment_core::run_events::{ChatEvent, ChatEventSink};
use muniment_runtime::{open_profile_storage, RuntimeChatEventSink};

mod common;
use common::{fixture_grant, TemporaryProfile};

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
        virtual_key: "secret-key".into(),
        ..fixture_grant()
    }
}

fn memory_runtime(profile: &TemporaryProfile) -> Arc<ApplicationMemoryRuntime> {
    Arc::new(ApplicationMemoryRuntime::new(
        profile.profile.clone(),
        profile.profile.join("memory"),
    ))
}

#[test]
fn delivers_to_an_optional_subscriber() {
    let profile = TemporaryProfile::new("sink-subscriber", false);
    let (subscriber, events) = mpsc::channel();
    let sink =
        RuntimeChatEventSink::new(&profile.profile, Some(subscriber), memory_runtime(&profile));

    sink.deliver(event()).unwrap();

    let delivered = events.recv().unwrap();
    assert_eq!(delivered.run_id, "run-1");
    assert_eq!(delivered.text, "hello");
}

#[test]
fn succeeds_without_a_subscriber() {
    let profile = TemporaryProfile::new("sink-no-subscriber", false);

    RuntimeChatEventSink::new(&profile.profile, None, memory_runtime(&profile))
        .deliver(event())
        .unwrap();
}

#[test]
fn clears_a_dropped_subscriber() {
    let profile = TemporaryProfile::new("sink-dropped-subscriber", false);
    let (subscriber, events) = mpsc::channel();
    let sink =
        RuntimeChatEventSink::new(&profile.profile, Some(subscriber), memory_runtime(&profile));
    drop(events);

    sink.deliver(event()).unwrap();
    sink.deliver(event()).unwrap();
}

#[test]
fn drives_pi_launch_config_over_the_profile_directory() {
    let profile = TemporaryProfile::new("sink-launch-config", false);
    let _storage = open_profile_storage(&profile.profile).unwrap();
    let pi_install = profile.profile.join("pi-install");
    fs::create_dir(&pi_install).unwrap();
    let memory_runtime = memory_runtime(&profile);
    let extension = memory_runtime.agent_extension_path();
    fs::create_dir_all(extension.parent().unwrap()).unwrap();
    fs::write(&extension, "export default function () {}\n").unwrap();
    let sink = RuntimeChatEventSink::new(&profile.profile, None, memory_runtime);

    assert_eq!(
        pi_launch_config(&sink, Some(&pi_install), &grant(), None).unwrap_err(),
        PiLaunchError::UnresolvableExecutable
    );
    assert_eq!(
        sink.pi_session_root().unwrap(),
        profile.profile.join("pi-sessions")
    );
    assert_eq!(sink.memory_agent_extension_path(), Some(extension.clone()));
    let config = pi_launch_config_for_executable(&sink, "pi".into(), &grant(), None).unwrap();
    assert!(config.args.windows(2).any(|args| {
        args == [
            "--session-dir",
            profile
                .profile
                .join("pi-sessions")
                .to_string_lossy()
                .as_ref(),
        ]
    }));
    assert!(config
        .args
        .windows(2)
        .any(|args| { args == ["--extension", extension.to_string_lossy().as_ref()] }));
}
