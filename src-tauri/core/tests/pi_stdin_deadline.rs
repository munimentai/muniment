use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use muniment_core::attach::RuntimeActivityRegistry;
use muniment_core::cas::LocalCas;
use muniment_core::chat_coordinate::coordinate;
use muniment_core::chat_grant::ChatGrant;
use muniment_core::journal::RunJournal;
use muniment_core::memory_runtime::ApplicationMemoryRuntime;
use muniment_core::pi_launch::{PiLaunchBoundaries, PiLaunchError};
use muniment_core::run_events::{ChatEvent, ChatEventSink, ChatStorage};
use muniment_core::run_preparation::{prepare_new_run_with_session_thread, SessionThreadStart};
use muniment_core::session_thread::SessionThread;
use muniment_core::sidecar::pi_chat::FIRST_EVENT_TIMEOUT;
use muniment_core::sidecar::pi_install::{PiArtifactDescriptor, PI_ARTIFACT};

const ARCHIVE: &[u8] = b"muniment-sidecar-test-stub\n";
const ARTIFACT: PiArtifactDescriptor = PiArtifactDescriptor {
    byte_size: ARCHIVE.len() as u64,
    sha256: "758b0db8f6304639edfca2b779e886f3006afeb006417e49dd6bce53ff2a65ab",
    ..PI_ARTIFACT
};
const RUN_ID: &str = "01900000-0000-7000-8000-000000000001";
const FAILURE: &str = "Pi did not acknowledge the prompt. Try again.";

struct Boundary(PathBuf);

impl ChatEventSink for Boundary {
    fn provenance(&self) -> (&str, &str) {
        ("test", "1")
    }

    fn deliver(&self, event: ChatEvent) -> Result<(), ()> {
        eprintln!("shell-event: {}", serde_json::to_string(&event).unwrap());
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

#[test]
fn blocked_stdin_fails_the_shell_and_logs_stderr_within_the_first_event_bound() {
    if let Some(root) = std::env::var_os("MUNIMENT_STDIN_DEADLINE_CHILD") {
        run_coordinate(PathBuf::from(root));
        return;
    }
    let root =
        std::env::temp_dir().join(format!("muniment-stdin-deadline-{}", uuid::Uuid::new_v4()));
    let revision = root.join("revisions").join(ARTIFACT.version);
    let executable = revision.join(ARTIFACT.executable);
    std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
    std::fs::create_dir_all(root.join("sessions")).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_sidecar-test-stub"), executable).unwrap();
    std::fs::write(revision.join(ARTIFACT.archive), ARCHIVE).unwrap();
    std::fs::write(
        root.join("current"),
        format!("muniment-pi-pointer-v1\n{}\n", ARTIFACT.version),
    )
    .unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "blocked_stdin_fails_the_shell_and_logs_stderr_within_the_first_event_bound",
            "--nocapture",
        ])
        .env("MUNIMENT_STDIN_DEADLINE_CHILD", &root)
        .env("MUNIMENT_PI_ROOT", &root)
        .env("PI_STUB_BLOCK_STDIN", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut started = Instant::now();
    let mut lines = Vec::new();
    for line in BufReader::new(child.stderr.take().unwrap()).lines() {
        let line = line.unwrap();
        if line == format!("muniment-runtime: run_id={RUN_ID} run_start") {
            started = Instant::now();
        }
        lines.push((started.elapsed(), line));
    }
    let status = child.wait().unwrap();
    std::fs::remove_dir_all(root).unwrap();
    assert!(status.success(), "{lines:?}");
    // Allow one second for readiness and shared-runner scheduling, not another timeout.
    let bound = FIRST_EVENT_TIMEOUT + Duration::from_secs(1);
    for expected in [
        format!("muniment-runtime: run_id={RUN_ID} first_event absent"),
        format!("muniment-runtime: run_id={RUN_ID} pi_stderr_tail="),
        "shell-event: ".into(),
    ] {
        let (elapsed, line) = lines
            .iter()
            .find(|(_, line)| {
                line.starts_with(&expected)
                    && (!expected.starts_with("shell-event") || line.contains(FAILURE))
            })
            .unwrap_or_else(|| panic!("Missing {expected}: {lines:?}"));
        assert!(
            *elapsed <= bound,
            "Late diagnostic or shell failure: {elapsed:?}: {line}"
        );
    }
    assert!(lines.iter().any(|(_, line)| {
        line.contains("pi_stderr_tail=") && line.contains("Pi stub stopped reading stdin.")
    }));
    assert!(lines
        .iter()
        .any(|(_, line)| line.contains("timed out writing Pi RPC stdin")));
    assert!(lines
        .iter()
        .any(|(_, line)| line.contains("pi_spawn") && line.contains("Healthy")));
    assert!(lines.iter().any(|(_, line)| {
        line.contains("provider_request outcome=unknown_prompt_not_acknowledged")
    }));
}

fn run_coordinate(root: PathBuf) {
    let storage = Arc::new(Mutex::new(ChatStorage {
        journal: RunJournal::open(root.join("runs.sqlite3")).unwrap(),
        cas: LocalCas::open(&root.join("cas")).unwrap(),
    }));
    let prepared = prepare_new_run_with_session_thread(
        &storage,
        SessionThreadStart {
            tracker: &SessionThread::default(),
            continue_existing: false,
        },
        RUN_ID,
        "local",
        None,
        Vec::new(),
        None,
        "test",
        "1",
        || Ok(()),
    )
    .unwrap();
    let runtime = Arc::new(Mutex::new(None));
    coordinate(
        Boundary(root.clone()),
        Arc::clone(&storage),
        Arc::clone(&runtime),
        RuntimeActivityRegistry::new(),
        Arc::new(ApplicationMemoryRuntime::new(
            root.join("memory-config"),
            root.join("memory-cache"),
        )),
        RUN_ID.into(),
        "x".repeat(128 * 1024),
        String::new(),
        None,
        ChatGrant::local(),
        Arc::new(AtomicBool::new(false)),
        Arc::new(Mutex::new(None)),
        Arc::new(Mutex::new(None)),
        Arc::new(Mutex::new(VecDeque::new())),
        None,
        None,
        Some(prepared),
    );
    let events = storage.lock().unwrap().journal.events(RUN_ID).unwrap();
    let terminal = events.last().unwrap();
    assert_eq!(terminal.event_type, "run.failed");
    assert!(serde_json::to_string(terminal).unwrap().contains(FAILURE));
    assert_eq!(
        runtime
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .supervisor
            .status(),
        muniment_core::sidecar::SidecarStatus::Stopped
    );
}
