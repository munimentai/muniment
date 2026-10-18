use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::ChatGrant;
use muniment_core::attach::RuntimeActivityRegistry;
use muniment_core::chat_coordinate::coordinate;
use muniment_core::chat_profile::ChatProfile;
use muniment_core::folder_access::{failure_reason, FolderAccess};
use muniment_core::memory_runtime::ApplicationMemoryRuntime;
use muniment_core::pi_launch::{PiLaunchBoundaries, PiLaunchError};
use muniment_core::run_events::{ChatEvent, ChatEventSink, ChatStorage};
use muniment_core::run_preparation::{prepare_new_run_with_session_thread, SessionThreadStart};
use muniment_core::session_thread::SessionThread;
use muniment_core::sidecar::pi_install::{PiArtifactDescriptor, PI_ARTIFACT};

const ARCHIVE: &[u8] = b"muniment-sidecar-test-stub\n";
const ARTIFACT: PiArtifactDescriptor = PiArtifactDescriptor {
    byte_size: ARCHIVE.len() as u64,
    sha256: "758b0db8f6304639edfca2b779e886f3006afeb006417e49dd6bce53ff2a65ab",
    ..PI_ARTIFACT
};
/// The FIFO in the thread folder that holds the stub Pi's startup read, as
/// macOS holds a protected folder until the user answers its prompt.
const GATE: &str = "consent-gate";

/// Stands in for macOS: the folder stays held until the check numbered
/// `allow_at`, which opens the gate, or for good when `answer` says so.
#[derive(Clone)]
struct Boundary {
    profile: ChatProfile,
    folder: PathBuf,
    events: Arc<Mutex<Vec<ChatEvent>>>,
    checks: Arc<AtomicUsize>,
    allow_at: Option<usize>,
    answer: FolderAccess,
}

impl ChatEventSink for Boundary {
    fn provenance(&self) -> (&str, &str) {
        ("test", "1")
    }
    fn deliver(&self, event: ChatEvent) -> Result<(), ()> {
        self.events.lock().unwrap().push(event);
        Ok(())
    }
}

impl PiLaunchBoundaries for Boundary {
    fn pi_session_root(&self) -> Result<PathBuf, PiLaunchError> {
        Ok(self.profile.pi_session_root())
    }
    fn project_directory(&self) -> Result<Option<PathBuf>, PiLaunchError> {
        Ok(Some(self.folder.clone()))
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
    fn acquire_pi(&self, root: &Path, _: &AtomicBool) -> Result<PathBuf, PiLaunchError> {
        let revision = root.join("revisions").join(ARTIFACT.version);
        let executable = revision.join(ARTIFACT.executable);
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::copy(env!("CARGO_BIN_EXE_sidecar-test-stub"), &executable).unwrap();
        std::fs::write(revision.join(ARTIFACT.archive), ARCHIVE).unwrap();
        std::fs::write(
            root.join("current"),
            format!("muniment-pi-pointer-v1\n{}\n", ARTIFACT.version),
        )
        .unwrap();
        Ok(executable)
    }
    fn folder_access(&self, directory: &Path, _: Duration, _: &AtomicBool) -> FolderAccess {
        assert_eq!(directory, self.folder);
        let check = self.checks.fetch_add(1, Ordering::SeqCst) + 1;
        if self.allow_at != Some(check) {
            return self.answer;
        }
        // The user chose Allow: Pi's held read completes once Pi makes it.
        let gate = self.folder.join(GATE);
        std::thread::spawn(move || drop(std::fs::OpenOptions::new().write(true).open(gate)));
        FolderAccess::Open
    }
}

#[cfg(unix)]
#[test]
fn a_held_working_folder_names_the_consent_prompt_instead_of_timing_out() {
    let _environment = super::ENVIRONMENT.lock().unwrap();
    if std::env::var_os("MUNIMENT_FOLDER_ACCESS_CHILD").is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "folder_access_coordinate::a_held_working_folder_names_the_consent_prompt_instead_of_timing_out",
                "--nocapture",
            ])
            .env("MUNIMENT_FOLDER_ACCESS_CHILD", "1")
            .env("PI_RESUME_STUB_FOLDER_GATE", GATE)
            .env_remove("MUNIMENT_PI_ROOT")
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stderr}");
        assert_eq!(
            stderr
                .matches("pi_spawn waiting_on=folder_access folder=other")
                .count(),
            2,
            "{stderr}"
        );
        assert!(
            stderr.contains("config_error=FolderAccess { access: Waiting"),
            "{stderr}"
        );
        assert!(
            stderr.contains("config_error=FolderAccess { access: Denied"),
            "{stderr}"
        );
        assert!(stderr.contains("pi_spawn started"), "{stderr}");
        return;
    }
    for (allow_at, answer) in [
        (None, FolderAccess::Waiting),
        (None, FolderAccess::Denied),
        (Some(2), FolderAccess::Waiting),
    ] {
        let directory =
            std::env::temp_dir().join(format!("muniment-folder-access-{}", uuid::Uuid::new_v4()));
        let folder = directory.join("thread");
        std::fs::create_dir_all(&folder).unwrap();
        assert!(std::process::Command::new("mkfifo")
            .arg(folder.join(GATE))
            .status()
            .unwrap()
            .success());
        let profile = ChatProfile::new(&directory);
        let (journal, cas) = profile.open_storage().unwrap();
        let storage = Arc::new(Mutex::new(ChatStorage { journal, cas }));
        let boundary = Boundary {
            profile,
            folder,
            events: Arc::new(Mutex::new(Vec::new())),
            checks: Arc::new(AtomicUsize::new(0)),
            allow_at,
            answer,
        };
        let run_id = uuid::Uuid::now_v7().to_string();
        let prepared = prepare_new_run_with_session_thread(
            &storage,
            SessionThreadStart {
                tracker: &SessionThread::default(),
                continue_existing: false,
            },
            &run_id,
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
        let started = Instant::now();
        coordinate(
            boundary.clone(),
            Arc::clone(&storage),
            Arc::clone(&runtime),
            RuntimeActivityRegistry::new(),
            Arc::new(ApplicationMemoryRuntime::new(
                directory.join("memory-config"),
                directory.join("memory-cache"),
            )),
            run_id.clone(),
            "Reply briefly.".into(),
            None,
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
        let events = boundary.events.lock().unwrap();
        let last = events.last().unwrap();
        let phases = events
            .iter()
            .map(|event| (event.phase.as_str(), event.failure_reason.as_deref()))
            .collect::<Vec<_>>();
        if allow_at.is_some() {
            // The answer arrived while the run waited, so Pi read its folder and replied.
            assert_eq!(last.phase, "complete", "{phases:?}");
            assert!(events
                .iter()
                .any(|event| event.phase == "streaming" && !event.text.is_empty()));
        } else {
            // A held folder must not reach Pi, whose read would hang until its readiness bound.
            assert_eq!(last.phase, "failed", "{phases:?}");
            assert_eq!(
                last.failure_reason.as_deref(),
                Some(failure_reason(answer, None).as_str())
            );
            assert!(started.elapsed() < Duration::from_secs(30));
            assert!(runtime.lock().unwrap().is_none());
        }
        drop(events);
        if let Some(mut runtime) = runtime.lock().unwrap().take() {
            runtime.supervisor.shutdown().unwrap();
        };
        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
