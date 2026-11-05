use std::collections::VecDeque;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::ChatGrant;
use muniment_core::attach::RuntimeActivityRegistry;
use muniment_core::cas::LocalCas;
use muniment_core::chat_coordinate::coordinate;
use muniment_core::journal::{EventPayload, RunJournal};
use muniment_core::memory_runtime::ApplicationMemoryRuntime;
use muniment_core::pi_launch::{PiLaunchBoundaries, PiLaunchError};
use muniment_core::run_events::{ChatEvent, ChatEventSink, ChatStorage, SharedStorage};
use muniment_core::run_preparation::{prepare_new_run_with_session_thread, SessionThreadStart};
use muniment_core::session_thread::SessionThread;
use muniment_core::sidecar::pi_install::{PiArtifactDescriptor, PI_ARTIFACT};

const ARCHIVE: &[u8] = b"muniment-sidecar-test-stub\n";
const ARTIFACT: PiArtifactDescriptor = PiArtifactDescriptor {
    byte_size: ARCHIVE.len() as u64,
    sha256: "758b0db8f6304639edfca2b779e886f3006afeb006417e49dd6bce53ff2a65ab",
    ..PI_ARTIFACT
};

struct Boundary(PathBuf);
impl ChatEventSink for Boundary {
    fn provenance(&self) -> (&str, &str) {
        ("test", "1")
    }
    fn deliver(&self, _: ChatEvent) -> Result<(), ()> {
        Ok(())
    }
}
impl PiLaunchBoundaries for Boundary {
    fn pi_session_root(&self) -> Result<PathBuf, PiLaunchError> {
        Ok(self.0.join("sessions"))
    }
    fn memory_agent_extension_path(&self) -> Option<PathBuf> {
        None
    }
    fn pi_artifact(&self) -> PiArtifactDescriptor {
        ARTIFACT
    }
    fn prepare_pi_settings(&self, _: PiArtifactDescriptor, _: &Path) -> Result<(), PiLaunchError> {
        Ok(())
    }
}

/// Runs one reply to its end and answers its journal: each event's type and
/// payload.
fn reply(
    root: &Path,
    storage: &SharedStorage,
    runtime: &Arc<Mutex<Option<muniment_core::pi_execution::PiRuntime>>>,
    memory: &Arc<ApplicationMemoryRuntime>,
    prompt: &str,
) -> Vec<(String, serde_json::Value)> {
    let run = uuid::Uuid::now_v7().to_string();
    let prepared = prepare_new_run_with_session_thread(
        storage,
        SessionThreadStart {
            tracker: &SessionThread::default(),
            continue_existing: false,
        },
        &run,
        "local",
        Some("owner"),
        Vec::new(),
        None,
        "test",
        "1",
        || Ok(()),
    )
    .unwrap();
    coordinate(
        Boundary(root.to_owned()),
        Arc::clone(storage),
        Arc::clone(runtime),
        RuntimeActivityRegistry::new(),
        Arc::clone(memory),
        run.clone(),
        prompt.into(),
        None,
        String::new(),
        Some("owner".into()),
        ChatGrant::local(),
        Arc::new(AtomicBool::new(false)),
        Arc::new(Mutex::new(None)),
        Arc::new(Mutex::new(None)),
        Arc::new(Mutex::new(VecDeque::new())),
        None,
        None,
        Some(prepared),
    );
    storage
        .lock()
        .unwrap()
        .journal
        .events(&run)
        .unwrap()
        .into_iter()
        .map(|event| {
            let payload = match event.payload {
                EventPayload::Inline { payload_json } => payload_json,
                _ => serde_json::Value::Null,
            };
            (event.event_type, payload)
        })
        .collect()
}

fn failure(events: &[(String, serde_json::Value)]) -> String {
    let (kind, payload) = events.last().unwrap();
    assert_eq!(kind, "run.failed", "{events:?}");
    payload["reason"].as_str().unwrap().to_owned()
}

#[test]
fn a_server_that_does_not_answer_is_named_and_retried_once() {
    let _environment = super::ENVIRONMENT.lock().unwrap();
    let root = std::env::temp_dir().join(format!(
        "muniment-endpoint-wait-coordinate-{}",
        uuid::Uuid::new_v4()
    ));
    let revision = root.join("revisions").join(ARTIFACT.version);
    let executable = revision.join(ARTIFACT.executable);
    std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
    std::fs::create_dir_all(root.join("sessions")).unwrap();
    let agent = root.join("state").join("agent");
    std::fs::create_dir_all(&agent).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_sidecar-test-stub"), executable).unwrap();
    std::fs::write(revision.join(ARTIFACT.archive), ARCHIVE).unwrap();
    std::fs::write(
        root.join("current"),
        format!("muniment-pi-pointer-v1\n{}\n", ARTIFACT.version),
    )
    .unwrap();
    std::env::set_var("MUNIMENT_PI_ROOT", &root);
    std::env::set_var("MUNIMENT_STATE_DIR", root.join("state"));
    let config = root.join("config");
    muniment_core::home::confirm_home(&config, &root.join("home")).unwrap();
    // The thread's model lives on a custom endpoint the person named.
    std::fs::write(
        agent.join("settings.json"),
        r#"{"defaultProvider":"custom-do-test","defaultModel":"qwen"}"#,
    )
    .unwrap();
    let models = |url: &str| {
        std::fs::write(
            muniment_core::models::pi_models::models_file(&agent),
            serde_json::json!({"providers": {"custom-do-test": {
                "baseUrl": url, "api": "openai-completions", "apiKey": "local",
                "models": [{"id": "qwen"}]
            }}})
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            agent.join("muniment-models.json"),
            r#"{"names":{"custom-do-test":"DO Test"}}"#,
        )
        .unwrap();
    };
    let memory = Arc::new(ApplicationMemoryRuntime::new(
        config.clone(),
        root.join("memory-cache"),
    ));
    let storage = Arc::new(Mutex::new(ChatStorage {
        journal: RunJournal::open(root.join("runs.sqlite3")).unwrap(),
        cas: LocalCas::open(&root.join("cas")).unwrap(),
    }));
    let runtime = Arc::new(Mutex::new(None));

    // The server accepts the connect, and Pi's calls fail before an answer:
    // the run names the server, lets Pi try once more and stops the next try.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let open = format!("http://{}/v1", listener.local_addr().unwrap());
    models(&open);
    let requests = root.join("requests.jsonl");
    std::env::set_var("PI_RESUME_STUB_REQUESTS", &requests);
    std::env::set_var("PI_RESUME_STUB_RETRY_ERROR", "Connection error.");
    let events = reply(&root, &storage, &runtime, &memory, "Hello");
    std::env::remove_var("PI_RESUME_STUB_RETRY_ERROR");
    let stages: Vec<&str> = events
        .iter()
        .filter(|(kind, _)| kind == "model.routing.stage")
        .filter_map(|(_, payload)| payload["stage"].as_str())
        .collect();
    assert_eq!(stages, ["retrying:2:2:DO Test"], "{events:?}");
    let sent = std::fs::read_to_string(&requests).unwrap();
    assert_eq!(sent.matches("\"abort_retry\"").count(), 1, "{sent}");
    assert_eq!(
        failure(&events),
        format!(
            "DO Test did not answer at {open}. Check that the server is running, then send again."
        )
    );

    // The server refuses the connect: the probe ends the reply within its
    // bound, while Pi still waits on its call.
    let closed = {
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        format!("http://127.0.0.1:{port}/v1")
    };
    models(&closed);
    let started = Instant::now();
    let events = reply(&root, &storage, &runtime, &memory, "Hello [hold the run]");
    assert!(started.elapsed() < Duration::from_secs(30));
    assert_eq!(
        failure(&events),
        format!("DO Test did not answer at {closed}. Check that the server is running, then send again.")
    );
    std::env::remove_var("PI_RESUME_STUB_REQUESTS");
    drop(listener);

    // The silent server's reply stopped its runtime.
    if let Some(mut runtime) = runtime.lock().unwrap().take() {
        let _ = runtime.supervisor.shutdown();
    };
    std::env::remove_var("MUNIMENT_PI_ROOT");
    std::env::remove_var("MUNIMENT_STATE_DIR");
    drop(storage);
    std::fs::remove_dir_all(root).unwrap();
}
