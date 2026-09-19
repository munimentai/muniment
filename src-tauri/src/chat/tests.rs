use super::state::*;
use super::*;
use crate::test_support::append_test_event;
use base64::{engine::general_purpose::STANDARD, Engine};
use muniment_core::journal::reducer::reduce;
use muniment_core::sidecar::pi_install::{PiArtifactDescriptor, PI_ARTIFACT};
use muniment_core::sidecar::validate_pi_session;

static PI_ENV_LOCK: Mutex<()> = Mutex::new(());

#[cfg(any(unix, target_os = "windows"))]
#[derive(Clone, Debug, Eq, PartialEq)]
enum RunClientCall {
    Submit(String, Vec<String>, Option<String>),
    Resume(String),
    Steer(String, String),
    FollowUp(String, String),
    Cancel(String),
    PermissionAnswer(String, String, AttachChatPermissionAnswer),
}

#[cfg(any(unix, target_os = "windows"))]
#[derive(Clone, Default)]
struct FakeRunClient(Arc<Mutex<Vec<RunClientCall>>>, bool);

#[cfg(any(unix, target_os = "windows"))]
impl FakeRunClient {
    fn calls(&self) -> Vec<RunClientCall> {
        self.0.lock().unwrap().clone()
    }

    fn with_runtime_upgrade_pending() -> Self {
        Self(Arc::default(), true)
    }
}

#[cfg(any(unix, target_os = "windows"))]
impl RunCommandClient for FakeRunClient {
    fn run_submit(
        &self,
        text: &str,
        files: &[String],
        thread_id: Option<&str>,
    ) -> Result<RunSubmitAccepted, String> {
        if self.1 {
            return Err(auth::desktop_client_error(
                ClientError::RuntimeUpgradePending,
            ));
        }
        self.0.lock().unwrap().push(RunClientCall::Submit(
            text.to_owned(),
            files.to_vec(),
            thread_id.map(str::to_owned),
        ));
        Ok(RunSubmitAccepted {
            run_id: "remote-run".to_string(),
            thread_id: thread_id.unwrap_or("accepted-thread").to_string(),
            attachments: Vec::new(),
            committed_seq: 7,
            accepted_at: "now".to_string(),
        })
    }

    fn run_resume(&self, run_id: &str) -> Result<RunResumeAccepted, ClientError> {
        if self.1 {
            return Err(ClientError::RuntimeUpgradePending);
        }
        self.0
            .lock()
            .unwrap()
            .push(RunClientCall::Resume(run_id.to_string()));
        Ok(RunResumeAccepted {
            run_id: run_id.to_string(),
            thread_id: "thread-1".to_string(),
            committed_seq: 8,
            accepted_at: "now".to_string(),
        })
    }

    fn run_steer(&self, run_id: &str, text: &str) -> Result<RunMessageAccepted, ClientError> {
        if self.1 {
            return Err(ClientError::RuntimeUpgradePending);
        }
        self.0
            .lock()
            .unwrap()
            .push(RunClientCall::Steer(run_id.to_string(), text.to_string()));
        Ok(RunMessageAccepted {
            run_id: run_id.to_string(),
            accepted_at: "now".to_string(),
        })
    }

    fn run_follow_up(&self, run_id: &str, text: &str) -> Result<RunMessageAccepted, ClientError> {
        if self.1 {
            return Err(ClientError::RuntimeUpgradePending);
        }
        self.0.lock().unwrap().push(RunClientCall::FollowUp(
            run_id.to_string(),
            text.to_string(),
        ));
        Ok(RunMessageAccepted {
            run_id: run_id.to_string(),
            accepted_at: "now".to_string(),
        })
    }

    fn run_cancel(&self, run_id: &str) -> Result<RunCancelAccepted, ClientError> {
        self.0
            .lock()
            .unwrap()
            .push(RunClientCall::Cancel(run_id.to_string()));
        Ok(RunCancelAccepted {
            run_id: run_id.to_string(),
            accepted_at: "now".to_string(),
        })
    }

    fn run_permission_answer(
        &self,
        run_id: &str,
        gate_id: &str,
        answer: AttachChatPermissionAnswer,
    ) -> Result<RunPermissionAnswerAccepted, ClientError> {
        self.0.lock().unwrap().push(RunClientCall::PermissionAnswer(
            run_id.to_string(),
            gate_id.to_string(),
            answer.clone(),
        ));
        Ok(RunPermissionAnswerAccepted {
            run_id: run_id.to_string(),
            gate_id: gate_id.to_string(),
            answer,
            committed_seq: 9,
            accepted_at: "now".to_string(),
        })
    }
}

#[cfg(any(unix, target_os = "windows"))]
fn assert_disconnected<T>(result: Result<T, String>) {
    assert_eq!(
        result.err().unwrap(),
        "Muniment cannot reach its background service."
    );
}

#[cfg(any(unix, target_os = "windows"))]
fn assert_runtime_update_pending<T>(result: Result<T, String>) {
    assert_eq!(
        result.err().unwrap(),
        "A runtime update is pending. Muniment will start new runs after the update."
    );
}

#[cfg(any(unix, target_os = "windows"))]
fn command_test_state() -> (PathBuf, ChatState) {
    let directory = std::env::temp_dir().join(format!("muniment-run-command-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&directory).unwrap();
    let storage = Arc::new(Mutex::new(ChatStorage {
        journal: RunJournal::open(directory.join("runs.sqlite3")).unwrap(),
        cas: LocalCas::open(&directory.join("cas")).unwrap(),
    }));
    let state = ChatState {
        storage: DeferredStorage(OnceLock::from(storage)),
        active: Arc::new(Mutex::new(None)),
        runtime: Arc::new(Mutex::new(None)),
        session_thread: SessionThread::default(),
        runtime_activity: RuntimeActivityRegistry::new(),
        retention_trigger: RetentionTrigger::default(),
    };
    (directory, state)
}

#[cfg(any(unix, target_os = "windows"))]
fn deferred_command_test_state() -> (Option<PathBuf>, ChatState) {
    (None, ChatState::new(RuntimeActivityRegistry::new()))
}

#[cfg(any(unix, target_os = "windows"))]
#[test]
fn chat_state_starts_without_desktop_storage() {
    let state = ChatState::new_runtime_owned(RuntimeActivityRegistry::new());

    assert!(state.storage.get().is_none());
    assert!(matches!(
        state.storage(),
        Err(error) if error == "Muniment cannot reach its background service."
    ));
    assert!(!state.retention_trigger.check_now());
}

#[cfg(target_os = "linux")]
#[test]
fn linux_chat_storage_starts_deferred() {
    let (_, state) = deferred_command_test_state();

    assert!(state.storage.get().is_none());
    assert!(matches!(
        state.storage(),
        Err(error) if error == "Muniment cannot reach its background service."
    ));
}

#[cfg(any(unix, target_os = "windows"))]
#[test]
fn chat_submit_routes_all_states_and_connected_stops_before_local_run() {
    let (directory, state) = command_test_state();
    let local_called = Arc::new(AtomicBool::new(false));
    let local_flag = Arc::clone(&local_called);
    let local = tauri::async_runtime::block_on(handle_run_submit::<FakeRunClient, _, _>(
        RunCommandSession::NoSupervisor,
        &state,
        None,
        "prompt",
        vec![],
        |_| async move {
            local_flag.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(SubmitResult {
                run_id: "local-run".into(),
                attachments: vec![],
                committed_seq: 0,
                accepted_at: String::new(),
            })
        },
    ));
    #[cfg(target_os = "linux")]
    {
        assert_eq!(local.unwrap().run_id, "local-run");
        assert!(local_called.load(std::sync::atomic::Ordering::SeqCst));
    }
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        assert_disconnected(local);
        assert!(!local_called.load(std::sync::atomic::Ordering::SeqCst));
    }

    let (deferred_directory, deferred_state) = deferred_command_test_state();
    deferred_state
        .session_thread
        .select("selected-thread".into(), Some("subject-1"));
    let client = FakeRunClient::default();
    let connected = tauri::async_runtime::block_on(handle_run_submit(
        RunCommandSession::Connected(client.clone()),
        &deferred_state,
        Some("subject-1"),
        "prompt",
        vec![SelectedFile {
            path: PathBuf::from("/tmp/a.txt"),
        }],
        |_| async { panic!("connected submit called the local operation") },
    ));
    assert_eq!(connected.unwrap().run_id, "remote-run");
    assert_eq!(
        deferred_state.session_thread.current(Some("subject-1")),
        Some("selected-thread".to_string())
    );
    assert_eq!(
        client.calls(),
        [RunClientCall::Submit(
            "prompt".to_string(),
            vec!["/tmp/a.txt".to_string()],
            Some("selected-thread".to_string())
        )]
    );
    assert!(state.active.lock().unwrap().is_none());
    assert!(state.runtime.lock().unwrap().is_none());

    let new_client = FakeRunClient::default();
    let created = tauri::async_runtime::block_on(handle_run_submit(
        RunCommandSession::Connected(new_client),
        &deferred_state,
        Some("subject-2"),
        "new prompt",
        vec![],
        |_| async { panic!("connected submit called the local operation") },
    ));
    assert!(created.is_ok());
    assert_eq!(
        deferred_state.session_thread.current(Some("subject-2")),
        Some("accepted-thread".to_string())
    );
    assert_runtime_update_pending(tauri::async_runtime::block_on(handle_run_submit(
        RunCommandSession::Connected(FakeRunClient::with_runtime_upgrade_pending()),
        &deferred_state,
        None,
        "prompt",
        vec![],
        |_| async { panic!("held submit called the local operation") },
    )));
    assert_disconnected(tauri::async_runtime::block_on(handle_run_submit::<
        FakeRunClient,
        _,
        _,
    >(
        RunCommandSession::Disconnected,
        &deferred_state,
        None,
        "prompt",
        vec![],
        |_| async { panic!("disconnected submit called the local operation") },
    )));
    if let Some(directory) = deferred_directory {
        std::fs::remove_dir_all(directory).unwrap();
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[cfg(any(unix, target_os = "windows"))]
#[test]
fn chat_resume_routes_all_states_and_connected_stops_before_local_run() {
    let (directory, state) = command_test_state();
    let local_called = Arc::new(AtomicBool::new(false));
    let local_flag = Arc::clone(&local_called);
    let local = tauri::async_runtime::block_on(handle_run_resume::<FakeRunClient, _, _>(
        RunCommandSession::NoSupervisor,
        &state,
        "run-1",
        || async move {
            local_flag.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(SubmitResult {
                run_id: "local-run".into(),
                attachments: vec![],
                committed_seq: 0,
                accepted_at: String::new(),
            })
        },
    ));
    #[cfg(target_os = "linux")]
    {
        assert_eq!(local.unwrap().run_id, "local-run");
        assert!(local_called.load(std::sync::atomic::Ordering::SeqCst));
    }
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        assert_disconnected(local);
        assert!(!local_called.load(std::sync::atomic::Ordering::SeqCst));
    }
    let client = FakeRunClient::default();
    let (deferred_directory, deferred_state) = deferred_command_test_state();
    let connected = tauri::async_runtime::block_on(handle_run_resume(
        RunCommandSession::Connected(client.clone()),
        &deferred_state,
        "run-1",
        || async { panic!("connected resume called the local operation") },
    ));
    assert!(connected.is_ok());
    assert_eq!(client.calls(), [RunClientCall::Resume("run-1".to_string())]);
    assert_runtime_update_pending(tauri::async_runtime::block_on(handle_run_resume(
        RunCommandSession::Connected(FakeRunClient::with_runtime_upgrade_pending()),
        &deferred_state,
        "run-1",
        || async { panic!("held resume called the local operation") },
    )));
    assert!(state.active.lock().unwrap().is_none());
    assert!(state.runtime.lock().unwrap().is_none());
    assert_disconnected(tauri::async_runtime::block_on(handle_run_resume::<
        FakeRunClient,
        _,
        _,
    >(
        RunCommandSession::Disconnected,
        &deferred_state,
        "run-1",
        || async { panic!("disconnected resume called the local operation") },
    )));
    if let Some(directory) = deferred_directory {
        std::fs::remove_dir_all(directory).unwrap();
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[cfg(any(unix, target_os = "windows"))]
#[test]
fn chat_queue_routes_all_desktop_client_states_and_deliveries() {
    let local_called = AtomicBool::new(false);
    let local = handle_run_queue::<FakeRunClient, _>(
        RunCommandSession::NoSupervisor,
        "run-1",
        ChatDelivery::Steer,
        "message",
        || {
            local_called.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        },
    );
    #[cfg(target_os = "linux")]
    {
        assert!(local.is_ok());
        assert!(local_called.load(std::sync::atomic::Ordering::SeqCst));
    }
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        assert_disconnected(local);
        assert!(!local_called.load(std::sync::atomic::Ordering::SeqCst));
    }
    let client = FakeRunClient::default();
    assert!(handle_run_queue(
        RunCommandSession::Connected(client.clone()),
        "run-1",
        ChatDelivery::Steer,
        "steer",
        || panic!("connected queue called the local operation")
    )
    .is_ok());
    assert!(handle_run_queue(
        RunCommandSession::Connected(client.clone()),
        "run-2",
        ChatDelivery::FollowUp,
        "follow-up",
        || panic!("connected queue called the local operation")
    )
    .is_ok());
    assert_eq!(
        client.calls(),
        [
            RunClientCall::Steer("run-1".to_string(), "steer".to_string()),
            RunClientCall::FollowUp("run-2".to_string(), "follow-up".to_string())
        ]
    );
    assert_runtime_update_pending(handle_run_queue(
        RunCommandSession::Connected(FakeRunClient::with_runtime_upgrade_pending()),
        "run-1",
        ChatDelivery::FollowUp,
        "message",
        || panic!("held queue called the local operation"),
    ));
    assert_disconnected(handle_run_queue::<FakeRunClient, _>(
        RunCommandSession::Disconnected,
        "run-1",
        ChatDelivery::FollowUp,
        "message",
        || panic!("disconnected queue called the local operation"),
    ));
}

#[cfg(any(unix, target_os = "windows"))]
#[test]
fn chat_cancel_routes_all_desktop_client_states() {
    let local_called = AtomicBool::new(false);
    let local =
        handle_run_cancel::<FakeRunClient, _>(RunCommandSession::NoSupervisor, "run-1", || {
            local_called.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        });
    #[cfg(target_os = "linux")]
    {
        assert!(local.is_ok());
        assert!(local_called.load(std::sync::atomic::Ordering::SeqCst));
    }
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        assert_disconnected(local);
        assert!(!local_called.load(std::sync::atomic::Ordering::SeqCst));
    }
    let client = FakeRunClient::default();
    assert!(handle_run_cancel(
        RunCommandSession::Connected(client.clone()),
        "run-1",
        || panic!("connected cancel called the local operation")
    )
    .is_ok());
    assert_eq!(client.calls(), [RunClientCall::Cancel("run-1".to_string())]);
    assert_disconnected(handle_run_cancel::<FakeRunClient, _>(
        RunCommandSession::Disconnected,
        "run-1",
        || panic!("disconnected cancel called the local operation"),
    ));
}

#[cfg(any(unix, target_os = "windows"))]
#[test]
fn chat_answer_permission_routes_all_desktop_client_states() {
    let answer = AttachChatPermissionAnswer::Confirm(true);
    let local_called = AtomicBool::new(false);
    let local = handle_run_permission_answer::<FakeRunClient, _>(
        RunCommandSession::NoSupervisor,
        "run-1",
        "gate-1",
        answer.clone(),
        || {
            local_called.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        },
    );
    #[cfg(target_os = "linux")]
    {
        assert!(local.is_ok());
        assert!(local_called.load(std::sync::atomic::Ordering::SeqCst));
    }
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        assert_disconnected(local);
        assert!(!local_called.load(std::sync::atomic::Ordering::SeqCst));
    }
    let client = FakeRunClient::default();
    assert!(handle_run_permission_answer(
        RunCommandSession::Connected(client.clone()),
        "run-1",
        "gate-1",
        answer.clone(),
        || panic!("connected permission answer called the local operation")
    )
    .is_ok());
    assert_eq!(
        client.calls(),
        [RunClientCall::PermissionAnswer(
            "run-1".to_string(),
            "gate-1".to_string(),
            answer.clone()
        )]
    );
    assert_disconnected(handle_run_permission_answer::<FakeRunClient, _>(
        RunCommandSession::Disconnected,
        "run-1",
        "gate-1",
        answer,
        || panic!("disconnected permission answer called the local operation"),
    ));
}

const TEST_PI_ARCHIVE: &[u8] = b"muniment-sidecar-test-stub\n";
const TEST_PI_ARTIFACT: PiArtifactDescriptor = PiArtifactDescriptor {
    version: PI_ARTIFACT.version,
    archive: PI_ARTIFACT.archive,
    byte_size: TEST_PI_ARCHIVE.len() as u64,
    sha256: "758b0db8f6304639edfca2b779e886f3006afeb006417e49dd6bce53ff2a65ab",
    executable: PI_ARTIFACT.executable,
};

struct FakeCoordinateSink {
    session_root: PathBuf,
    memory_agent_extension_path: Option<PathBuf>,
    pi_artifact: PiArtifactDescriptor,
}

impl FakeCoordinateSink {
    fn new(app_data_dir: &std::path::Path) -> Self {
        Self {
            session_root: ChatProfile::new(app_data_dir.to_owned()).pi_session_root(),
            memory_agent_extension_path: None,
            pi_artifact: PI_ARTIFACT,
        }
    }

    fn with_pi_stub(
        app_data_dir: &std::path::Path,
        pi_root: &std::path::Path,
        stub: &std::path::Path,
    ) -> Self {
        let revision = pi_root.join("revisions").join(TEST_PI_ARTIFACT.version);
        let executable = revision.join(TEST_PI_ARTIFACT.executable);
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::copy(stub, executable).unwrap();
        std::fs::write(revision.join(TEST_PI_ARTIFACT.archive), TEST_PI_ARCHIVE).unwrap();
        std::fs::write(
            pi_root.join("current"),
            format!("muniment-pi-pointer-v1\n{}\n", TEST_PI_ARTIFACT.version),
        )
        .unwrap();
        Self {
            session_root: ChatProfile::new(app_data_dir.to_owned()).pi_session_root(),
            memory_agent_extension_path: None,
            pi_artifact: TEST_PI_ARTIFACT,
        }
    }

    fn with_memory_agent_extension(mut self, path: PathBuf) -> Self {
        self.memory_agent_extension_path = Some(path);
        self
    }
}

impl ChatEventSink for FakeCoordinateSink {
    fn provenance(&self) -> (&str, &str) {
        ("test", "0.0.0")
    }

    fn deliver(&self, _event: ChatEvent) -> Result<(), ()> {
        Ok(())
    }
}

impl PiLaunchBoundaries for FakeCoordinateSink {
    fn pi_session_root(&self) -> Result<PathBuf, PiLaunchError> {
        Ok(self.session_root.clone())
    }

    fn memory_agent_extension_path(&self) -> Option<PathBuf> {
        self.memory_agent_extension_path.clone()
    }

    fn pi_artifact(&self) -> PiArtifactDescriptor {
        self.pi_artifact
    }

    fn prepare_pi_settings(
        &self,
        _artifact: PiArtifactDescriptor,
        _executable: &std::path::Path,
    ) -> Result<(), PiLaunchError> {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
#[test]
fn grant_errors_are_mapped_and_redacted() {
    let error = map_fetch_grant_error(FetchGrantError::Unauthorized).protocol_error();
    let encoded = serde_json::to_string(&error).unwrap();
    assert!(encoded.contains("unauthorized"), "{encoded}");
    for secret in ["private-prompt", "secret-token", "/private/work", "sidecar"] {
        assert!(!encoded.contains(secret), "leaked {secret}: {encoded}");
    }

    let internal = map_fetch_grant_error(FetchGrantError::Unavailable).protocol_error();
    assert_eq!(
        serde_json::to_value(internal).unwrap()["code"],
        "persistence_failed"
    );
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(error) => error.into_inner(),
    }
}

// Acquire the shared PI-environment lock without propagating poisoning: if a
// prior test panicked while holding the guard (e.g. a slow CI VM tripped the
// receipt-handshake deadline), recover the guard instead of turning that one
// flake into cascaded PoisonError failures across every sibling test.
fn lock_pi_environment() -> std::sync::MutexGuard<'static, ()> {
    lock_unpoisoned(&PI_ENV_LOCK)
}

#[test]
fn lock_unpoisoned_recovers_guard_after_panic() {
    let mutex = Mutex::new(0);

    std::thread::scope(|scope| {
        let result = scope.spawn(|| {
            let mut value = mutex.lock().unwrap();
            *value = 42;
            panic!("poison mutex");
        });
        assert!(result.join().is_err());
    });

    assert!(mutex.is_poisoned());
    let value = lock_unpoisoned(&mutex);
    assert_eq!(*value, 42);
}

fn accept_receipt_request(listener: std::net::TcpListener) -> std::net::TcpStream {
    listener.set_nonblocking(true).unwrap();
    // 120s (not 10s): on the contended single linux CI VM the coordinator can
    // take over a minute to open its receipt connection. A healthy run still
    // connects in milliseconds; this is only a generous ceiling before we
    // declare the handshake genuinely broken.
    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    loop {
        match listener.accept() {
            Ok((stream, _)) => return stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "coordinator did not request its receipt"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("receipt listener failed: {error}"),
        }
    }
}

fn read_sidecar_request_log(path: &std::path::Path) -> String {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        match std::fs::read_to_string(path) {
            Ok(requests) if !requests.is_empty() => return requests,
            Ok(_) => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "sidecar recorded an empty coordinator request at {}",
                    path.display()
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "sidecar did not record a coordinator request at {}",
                    path.display()
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!(
                "failed to read sidecar coordinator requests at {}: {error}",
                path.display()
            ),
        }
    }
}

#[test]
fn session_thread_continues_at_the_next_ordinal() {
    let directory =
        std::env::temp_dir().join(format!("muniment-session-thread-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&directory).unwrap();
    let database = directory.join("runs.sqlite3");
    let storage = Arc::new(Mutex::new(ChatStorage {
        journal: RunJournal::open(&database).unwrap(),
        cas: LocalCas::open(&directory.join("cas")).unwrap(),
    }));
    let tracker = SessionThread::default();
    let first_run = Uuid::now_v7().to_string();
    let second_run = Uuid::now_v7().to_string();

    prepare_new_run_with_session_thread(
        &storage,
        SessionThreadStart {
            tracker: &tracker,
            continue_existing: true,
        },
        &first_run,
        "workspace-a",
        Some("owner"),
        Vec::new(),
        None,
    )
    .unwrap();
    let OfferedThread::Selected(thread_id) = tracker.offered("workspace-a", Some("owner")) else {
        panic!("the first run must record its thread");
    };
    prepare_new_run_with_session_thread(
        &storage,
        SessionThreadStart {
            tracker: &tracker,
            continue_existing: true,
        },
        &second_run,
        "workspace-a",
        Some("owner"),
        Vec::new(),
        None,
    )
    .unwrap();

    let connection = rusqlite::Connection::open(&database).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT thread_run_ordinal FROM run_threads \
                 WHERE run_id=?1 AND thread_id=?2",
                (&second_run, &thread_id),
                |row| row.get::<_, u64>(0),
            )
            .unwrap(),
        2
    );
    assert_eq!(
        tracker.offered("workspace-a", Some("owner")),
        OfferedThread::Selected(thread_id)
    );

    drop(connection);
    drop(storage);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn fresh_session_tracker_skips_a_newer_foreign_thread() {
    let directory = std::env::temp_dir().join(format!("muniment-session-owner-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&directory).unwrap();
    let database = directory.join("runs.sqlite3");
    let owner_run = Uuid::now_v7().to_string();
    let foreign_run = Uuid::now_v7().to_string();
    let mut owner_started = event_envelope(&owner_run, 1, "run.started", json!({}), Some("owner"));
    owner_started.recorded_at = "2026-01-01T00:00:00Z".into();
    let mut foreign_started =
        event_envelope(&foreign_run, 1, "run.started", json!({}), Some("other"));
    foreign_started.recorded_at = "2026-01-02T00:00:00Z".into();
    let mut journal = RunJournal::open(&database).unwrap();
    journal
        .append_new_run("workspace-a", &owner_started)
        .unwrap();
    journal
        .append_new_run("workspace-a", &foreign_started)
        .unwrap();
    drop(journal);
    let connection = rusqlite::Connection::open(&database).unwrap();
    let owner_thread = connection
        .query_row(
            "SELECT thread_id FROM run_threads WHERE run_id=?1",
            [&owner_run],
            |row| row.get::<_, String>(0),
        )
        .unwrap();
    drop(connection);
    let storage = Arc::new(Mutex::new(ChatStorage {
        journal: RunJournal::open(&database).unwrap(),
        cas: LocalCas::open(&directory.join("cas")).unwrap(),
    }));
    let tracker = SessionThread::default();
    let continued_run = Uuid::now_v7().to_string();

    prepare_new_run_with_session_thread(
        &storage,
        SessionThreadStart {
            tracker: &tracker,
            continue_existing: true,
        },
        &continued_run,
        "workspace-a",
        Some("owner"),
        Vec::new(),
        None,
    )
    .unwrap();

    assert_eq!(
        tracker.offered("workspace-a", Some("owner")),
        OfferedThread::Selected(owner_thread)
    );

    drop(storage);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn fresh_session_tracker_mints_a_thread_when_no_candidate_exists() {
    let directory = std::env::temp_dir().join(format!("muniment-session-empty-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&directory).unwrap();
    let database = directory.join("runs.sqlite3");
    let storage = Arc::new(Mutex::new(ChatStorage {
        journal: RunJournal::open(&database).unwrap(),
        cas: LocalCas::open(&directory.join("cas")).unwrap(),
    }));
    let tracker = SessionThread::default();
    let run_id = Uuid::now_v7().to_string();

    prepare_new_run_with_session_thread(
        &storage,
        SessionThreadStart {
            tracker: &tracker,
            continue_existing: true,
        },
        &run_id,
        "workspace-a",
        Some("owner"),
        Vec::new(),
        None,
    )
    .unwrap();

    let connection = rusqlite::Connection::open(&database).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM events \
                 WHERE run_id=?1 AND event_type='run.started'",
                [&run_id],
                |row| row.get::<_, u64>(0),
            )
            .unwrap(),
        1
    );
    assert!(matches!(
        tracker.offered("workspace-a", Some("owner")),
        OfferedThread::Selected(_)
    ));

    drop(connection);
    drop(storage);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn selected_thread_overrides_the_newest_workspace_thread() {
    let directory =
        std::env::temp_dir().join(format!("muniment-session-selected-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&directory).unwrap();
    let database = directory.join("runs.sqlite3");
    let selected_run = Uuid::now_v7().to_string();
    let newest_run = Uuid::now_v7().to_string();
    let mut journal = RunJournal::open(&database).unwrap();
    journal
        .append_new_run(
            "workspace-a",
            &event_envelope(&selected_run, 1, "run.started", json!({}), Some("owner")),
        )
        .unwrap();
    journal
        .append_new_run(
            "workspace-a",
            &event_envelope(&newest_run, 1, "run.started", json!({}), Some("owner")),
        )
        .unwrap();
    drop(journal);
    let connection = rusqlite::Connection::open(&database).unwrap();
    let selected_thread = connection
        .query_row(
            "SELECT thread_id FROM run_threads WHERE run_id=?1",
            [&selected_run],
            |row| row.get::<_, String>(0),
        )
        .unwrap();
    drop(connection);
    let storage = Arc::new(Mutex::new(ChatStorage {
        journal: RunJournal::open(&database).unwrap(),
        cas: LocalCas::open(&directory.join("cas")).unwrap(),
    }));
    let tracker = SessionThread::default();
    tracker.select(selected_thread.clone(), Some("owner"));
    let continued_run = Uuid::now_v7().to_string();

    prepare_new_run_with_session_thread(
        &storage,
        SessionThreadStart {
            tracker: &tracker,
            continue_existing: true,
        },
        &continued_run,
        "workspace-a",
        Some("owner"),
        Vec::new(),
        None,
    )
    .unwrap();

    let connection = rusqlite::Connection::open(&database).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT thread_id FROM run_threads WHERE run_id=?1",
                [&continued_run],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        selected_thread
    );

    drop(connection);
    drop(storage);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn fresh_choice_overrides_the_newest_workspace_thread() {
    let directory = std::env::temp_dir().join(format!("muniment-session-fresh-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&directory).unwrap();
    let database = directory.join("runs.sqlite3");
    let existing_run = Uuid::now_v7().to_string();
    let mut journal = RunJournal::open(&database).unwrap();
    journal
        .append_new_run(
            "workspace-a",
            &event_envelope(&existing_run, 1, "run.started", json!({}), Some("owner")),
        )
        .unwrap();
    drop(journal);
    let storage = Arc::new(Mutex::new(ChatStorage {
        journal: RunJournal::open(&database).unwrap(),
        cas: LocalCas::open(&directory.join("cas")).unwrap(),
    }));
    let tracker = SessionThread::default();
    tracker.fresh(Some("owner"));
    let fresh_run = Uuid::now_v7().to_string();

    prepare_new_run_with_session_thread(
        &storage,
        SessionThreadStart {
            tracker: &tracker,
            continue_existing: true,
        },
        &fresh_run,
        "workspace-a",
        Some("owner"),
        Vec::new(),
        None,
    )
    .unwrap();

    let connection = rusqlite::Connection::open(&database).unwrap();
    let thread_for = |run_id: &str| {
        connection
            .query_row(
                "SELECT thread_id FROM run_threads WHERE run_id=?1",
                [run_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
    };
    assert_ne!(thread_for(&fresh_run), thread_for(&existing_run));
    assert_eq!(
        tracker.offered("workspace-a", Some("owner")),
        OfferedThread::Selected(thread_for(&fresh_run))
    );

    drop(connection);
    drop(storage);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn rejected_session_thread_falls_back_without_duplicate_start() {
    let directory =
        std::env::temp_dir().join(format!("muniment-session-fallback-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&directory).unwrap();
    let database = directory.join("runs.sqlite3");
    let storage = Arc::new(Mutex::new(ChatStorage {
        journal: RunJournal::open(&database).unwrap(),
        cas: LocalCas::open(&directory.join("cas")).unwrap(),
    }));
    let tracker = SessionThread::default();
    tracker.record("unknown-thread".into(), "workspace-a", Some("owner"));
    let run_id = Uuid::now_v7().to_string();

    prepare_new_run_with_session_thread(
        &storage,
        SessionThreadStart {
            tracker: &tracker,
            continue_existing: true,
        },
        &run_id,
        "workspace-a",
        Some("owner"),
        Vec::new(),
        None,
    )
    .unwrap();

    let connection = rusqlite::Connection::open(&database).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM events \
                 WHERE run_id=?1 AND event_type='run.started'",
                [&run_id],
                |row| row.get::<_, u64>(0),
            )
            .unwrap(),
        1
    );
    assert_ne!(
        tracker.offered("workspace-a", Some("owner")),
        OfferedThread::Selected("unknown-thread".into())
    );

    drop(connection);
    drop(storage);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn disabled_continuation_leaves_the_session_thread_unchanged() {
    let directory =
        std::env::temp_dir().join(format!("muniment-session-disabled-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&directory).unwrap();
    let storage = Arc::new(Mutex::new(ChatStorage {
        journal: RunJournal::open(directory.join("runs.sqlite3")).unwrap(),
        cas: LocalCas::open(&directory.join("cas")).unwrap(),
    }));
    let tracker = SessionThread::default();
    tracker.record("thread-a".into(), "workspace-a", Some("owner"));

    prepare_new_run_with_session_thread(
        &storage,
        SessionThreadStart {
            tracker: &tracker,
            continue_existing: false,
        },
        &Uuid::now_v7().to_string(),
        "workspace-a",
        Some("owner"),
        Vec::new(),
        None,
    )
    .unwrap();

    assert_eq!(
        tracker.offered("workspace-a", Some("owner")),
        OfferedThread::Selected("thread-a".into())
    );

    drop(storage);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn missing_attachment_returns_non_path_leaking_copy_before_pi_can_start() {
    let _environment = lock_pi_environment();
    let directory = std::env::temp_dir().join(format!("muniment-missing-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&directory).unwrap();
    let prompt_log = directory.join("prompt.txt");
    std::env::set_var("PI_RESUME_STUB_PROMPTS", &prompt_log);
    let storage = Arc::new(Mutex::new(ChatStorage {
        journal: RunJournal::open(directory.join("runs.sqlite3")).unwrap(),
        cas: LocalCas::open(&directory.join("cas")).unwrap(),
    }));
    let missing = directory.join("private-name.txt");
    let error = match prepare_new_run(
        &storage,
        &Uuid::now_v7().to_string(),
        "workspace-a",
        Some("owner"),
        vec![SelectedFile {
            path: missing.clone(),
        }],
        None,
    ) {
        Ok(_) => panic!("missing attachment must fail"),
        Err(error) => error,
    };
    assert_eq!(error, attachment_error());
    assert!(!error.contains(missing.to_string_lossy().as_ref()));
    assert!(!prompt_log.exists(), "Pi must not receive a prompt");
    std::env::remove_var("PI_RESUME_STUB_PROMPTS");
    drop(storage);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn non_regular_attachment_is_rejected_before_a_run_is_created() {
    let directory = std::env::temp_dir().join(format!("muniment-non-regular-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&directory).unwrap();
    let storage = Arc::new(Mutex::new(ChatStorage {
        journal: RunJournal::open(directory.join("runs.sqlite3")).unwrap(),
        cas: LocalCas::open(&directory.join("cas")).unwrap(),
    }));
    let run_id = Uuid::now_v7().to_string();

    assert!(prepare_new_run(
        &storage,
        &run_id,
        "workspace-a",
        Some("owner"),
        vec![SelectedFile {
            path: directory.clone(),
        }],
        None,
    )
    .is_err());
    assert!(storage
        .lock()
        .unwrap()
        .journal
        .events(&run_id)
        .unwrap()
        .is_empty());

    drop(storage);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn changed_second_attachment_fails_the_run_without_prompting_pi() {
    let _environment = lock_pi_environment();
    let directory = std::env::temp_dir().join(format!("muniment-changed-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&directory).unwrap();
    let first = directory.join("first.txt");
    let second = directory.join("private-second.txt");
    let prompt_log = directory.join("prompt.txt");
    std::fs::write(&first, b"first attachment").unwrap();
    std::fs::write(&second, b"second attachment").unwrap();
    let storage = Arc::new(Mutex::new(ChatStorage {
        journal: RunJournal::open(directory.join("runs.sqlite3")).unwrap(),
        cas: LocalCas::open(&directory.join("cas")).unwrap(),
    }));
    let run_id = Uuid::now_v7().to_string();
    let opened = open_selected_files(vec![
        SelectedFile { path: first },
        SelectedFile {
            path: second.clone(),
        },
    ])
    .unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&second)
        .unwrap()
        .set_len(1)
        .unwrap();
    std::env::set_var("PI_RESUME_STUB_PROMPTS", &prompt_log);

    let error = match prepare_opened_run(
        &storage,
        SessionThreadStart {
            tracker: &SessionThread::default(),
            continue_existing: false,
        },
        &run_id,
        "workspace-a",
        Some("owner"),
        opened,
        None,
        None,
        || Ok(()),
    ) {
        Ok(_) => panic!("changed attachment length must fail"),
        Err(error) => error,
    };
    assert_eq!(error, attachment_error());
    assert!(!error.contains(second.to_string_lossy().as_ref()));
    assert!(!prompt_log.exists(), "Pi must receive zero prompts");
    let events = storage.lock().unwrap().journal.events(&run_id).unwrap();
    assert_eq!(
        events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>(),
        ["run.started", "chat.attachment.ingested", "run.failed"]
    );
    assert!(reduce(&events).unwrap().is_terminal());

    std::env::remove_var("PI_RESUME_STUB_PROMPTS");
    drop(storage);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn coordinator_sends_exact_ordered_images_and_preserves_text_only_prompt_shape() {
    let _environment = lock_pi_environment();
    for with_images in [true, false] {
        let directory =
            std::env::temp_dir().join(format!("muniment-prompt-capture-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let sessions = ChatProfile::new(directory.clone()).pi_session_root();
        std::fs::create_dir_all(&sessions).unwrap();
        let request_log = directory.join("requests.jsonl");
        let mut files = Vec::new();
        if with_images {
            let png = directory.join("first.bin");
            let unsupported = directory.join("local-only.png");
            let gif = directory.join("second.dat");
            std::fs::write(&png, valid_test_png()).unwrap();
            std::fs::write(&unsupported, b"not an image").unwrap();
            std::fs::write(
                &gif,
                STANDARD
                    .decode("R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==")
                    .unwrap(),
            )
            .unwrap();
            files = vec![
                SelectedFile { path: png },
                SelectedFile { path: unsupported },
                SelectedFile { path: gif },
            ];
        }
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(directory.join("runs.sqlite3")).unwrap(),
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let run_id = Uuid::now_v7().to_string();
        let prepared =
            prepare_new_run(&storage, &run_id, "workspace-a", Some("owner"), files, None).unwrap();

        let executable_name = if cfg!(windows) {
            "sidecar-test-stub.exe"
        } else {
            "sidecar-test-stub"
        };
        let test_executable = std::env::current_exe().unwrap();
        let stub = test_executable
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("examples")
            .join(executable_name);
        assert!(stub.is_file(), "sidecar test stub was not built");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let receipt_url = format!("http://{}/receipt", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let (stop_receipt_server, receipt_server_stop) = std::sync::mpsc::channel();
        let receipt_server = std::thread::spawn(move || {
            use std::io::{Read, Write};
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut request = [0; 4096];
                        let _ = stream.read(&mut request).unwrap();
                        let body = r#"{"route":"capture-stub","model":"test"}"#;
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                        .unwrap();
                        break;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if receipt_server_stop.try_recv().is_ok() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("receipt listener failed: {error}"),
                }
            }
        });
        std::env::set_var("MUNIMENT_PI_ROOT", &directory);
        std::env::set_var("PI_RESUME_STUB_REQUESTS", &request_log);
        coordinate(
            FakeCoordinateSink::with_pi_stub(&directory, &directory, &stub),
            Arc::clone(&storage),
            Arc::new(Mutex::new(None)),
            RuntimeActivityRegistry::new(),
            Arc::new(crate::memory::ApplicationMemoryRuntime::new(
                directory.join("memory-config"),
                directory.join("memory-cache"),
            )),
            run_id,
            "original text prompt".into(),
            "token".into(),
            Some("owner".into()),
            ChatGrant {
                native_access_token: None,
                expires_at: None,
                workspace: "workspace-a".into(),
                gateway_url: "https://gateway.invalid".into(),
                virtual_key: "virtual-key".into(),
                model: None,
                minimum_cacheable_prefix_characters: 8_192,
                receipt_url,
            },
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(VecDeque::new())),
            None,
            None,
            Some(prepared),
        );
        let _ = stop_receipt_server.send(());
        receipt_server.join().unwrap();
        for key in ["MUNIMENT_PI_ROOT", "PI_RESUME_STUB_REQUESTS"] {
            std::env::remove_var(key);
        }

        let requests = read_sidecar_request_log(&request_log);
        let prompt: serde_json::Value =
            serde_json::from_str(requests.lines().next().unwrap()).unwrap();
        assert_eq!(prompt["type"], "prompt");
        // The prompt arrives behind the line that says when it was sent.
        let message = prompt["message"].as_str().unwrap();
        assert!(message.starts_with("[sent 20"), "{message}");
        assert!(message.ends_with("\noriginal text prompt"), "{message}");
        assert!(prompt.get("classification").is_none());
        if with_images {
            assert_eq!(
                prompt["images"],
                json!([
                    {
                        "type": "image",
                        "data": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=",
                        "mimeType": "image/png"
                    },
                    {
                        "type": "image",
                        "data": "R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==",
                        "mimeType": "image/gif"
                    }
                ])
            );
        } else {
            assert!(prompt.get("images").is_none());
        }
        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[test]
fn missing_oversized_and_ambiguous_images_fail_before_prompt_with_non_leaking_copy() {
    let _environment = lock_pi_environment();
    for (name, bytes, remove_object) in [
        ("private-missing.png", valid_test_png(), true),
        (
            "private-large.jpg",
            {
                let mut bytes =
                    vec![
                        0_u8;
                        usize::try_from(muniment_core::attachment::MAX_PI_IMAGE_BYTES + 1).unwrap()
                    ];
                bytes[..3].copy_from_slice(b"\xff\xd8\xff");
                let length = bytes.len();
                bytes[length - 2..].copy_from_slice(b"\xff\xd9");
                bytes
            },
            false,
        ),
        (
            "private-malformed.jpg",
            b"\xff\xd8\xffprivate-attachment-marker\xff\xd9".to_vec(),
            false,
        ),
    ] {
        let directory =
            std::env::temp_dir().join(format!("muniment-image-fail-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(name);
        std::fs::write(&path, &bytes).unwrap();
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(directory.join("runs.sqlite3")).unwrap(),
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let run_id = Uuid::now_v7().to_string();
        let prepared = prepare_new_run(
            &storage,
            &run_id,
            "workspace-a",
            Some("owner"),
            vec![SelectedFile { path: path.clone() }],
            None,
        )
        .unwrap();
        let attachment_hash = {
            let mut storage = storage.lock().unwrap();
            let events = storage.journal.events(&run_id).unwrap();
            let EventPayload::Attachment { attachment } = &events[1].payload else {
                panic!("attachment event")
            };
            attachment.sha256().clone()
        };
        if remove_object {
            storage
                .lock()
                .unwrap()
                .cas
                .remove(&attachment_hash)
                .unwrap();
        }

        let request_log = directory.join("requests.jsonl");
        let executable_name = if cfg!(windows) {
            "sidecar-test-stub.exe"
        } else {
            "sidecar-test-stub"
        };
        let test_executable = std::env::current_exe().unwrap();
        let stub = test_executable
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("examples")
            .join(executable_name);
        assert!(stub.is_file(), "sidecar test stub was not built");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let receipt_url = format!("http://{}/receipt", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let (stop_receipt_server, receipt_server_stop) = std::sync::mpsc::channel();
        let receipt_server = std::thread::spawn(move || {
            use std::io::{Read, Write};
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut request = [0; 4096];
                        let _ = stream.read(&mut request).unwrap();
                        let body = r#"{"route":"capture-stub","model":"test"}"#;
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                        .unwrap();
                        break;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if receipt_server_stop.try_recv().is_ok() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("receipt listener failed: {error}"),
                }
            }
        });
        std::env::set_var("MUNIMENT_PI_ROOT", &directory);
        std::env::set_var("MUNIMENT_PI_TEST_EXECUTABLE", &stub);
        std::env::set_var("PI_RESUME_STUB_REQUESTS", &request_log);
        coordinate(
            FakeCoordinateSink::new(&directory),
            Arc::clone(&storage),
            Arc::new(Mutex::new(None)),
            RuntimeActivityRegistry::new(),
            Arc::new(crate::memory::ApplicationMemoryRuntime::new(
                directory.join("memory-config"),
                directory.join("memory-cache"),
            )),
            run_id.clone(),
            "private prompt bytes".into(),
            "token".into(),
            Some("owner".into()),
            ChatGrant {
                native_access_token: None,
                expires_at: None,
                workspace: "workspace-a".into(),
                gateway_url: "https://gateway.invalid".into(),
                virtual_key: "virtual-key".into(),
                model: None,
                minimum_cacheable_prefix_characters: 8_192,
                receipt_url,
            },
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(VecDeque::new())),
            None,
            None,
            Some(prepared),
        );
        stop_receipt_server.send(()).unwrap();
        receipt_server.join().unwrap();
        std::env::remove_var("MUNIMENT_PI_ROOT");
        std::env::remove_var("MUNIMENT_PI_TEST_EXECUTABLE");
        std::env::remove_var("PI_RESUME_STUB_REQUESTS");
        assert!(!request_log.exists(), "Pi must receive zero prompts");
        let events = storage.lock().unwrap().journal.events(&run_id).unwrap();
        assert_eq!(events.last().unwrap().event_type, "run.failed");
        let public_error = serde_json::to_string(events.last().unwrap()).unwrap();
        let expected_error = match name {
            "private-large.jpg" => {
                attachment_delivery_error(AttachmentDeliveryError::ImageSizeLimit {
                    display_name: name.into(),
                })
            }
            "private-malformed.jpg" => {
                attachment_delivery_error(AttachmentDeliveryError::AmbiguousFormat {
                    display_name: name.into(),
                })
            }
            _ => attachment_error(),
        };
        assert!(public_error.contains(&expected_error));
        for secret in [
            path.to_string_lossy().as_ref(),
            attachment_hash.as_str(),
            &STANDARD.encode(&bytes),
            "private-attachment-marker",
        ] {
            assert!(!public_error.contains(secret));
        }

        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }
}

fn valid_test_png() -> Vec<u8> {
    STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")
        .unwrap()
}

#[test]
fn event_envelope_records_the_owning_subject() {
    let owned = event_envelope("run-1", 1, "run.started", json!({}), Some("sub-a"));
    assert_eq!(owned.provenance.actor_id.as_deref(), Some("sub-a"));
    assert_eq!(owned.provenance.source, "muniment-desktop");
    assert_eq!(owned.provenance.source_version, env!("CARGO_PKG_VERSION"));

    let unowned = event_envelope("run-2", 1, "run.started", json!({}), None);
    assert_eq!(unowned.provenance.actor_id, None);
}

#[test]
fn permission_answers_are_closed_and_cover_each_dialog() {
    for value in [
        json!({"type": "select", "value": "A"}),
        json!({"type": "confirm", "value": true}),
        json!({"type": "input", "value": "text"}),
        json!({"type": "editor", "value": "draft"}),
        json!({"type": "cancelled"}),
    ] {
        assert!(serde_json::from_value::<ChatPermissionAnswer>(value).is_ok());
    }
    for value in [
        json!({"type": "select", "value": 1}),
        json!({"type": "unknown", "value": "A"}),
        json!({"type": "cancelled", "extra": true}),
    ] {
        assert!(serde_json::from_value::<ChatPermissionAnswer>(value).is_err());
    }
}

#[test]
fn resume_validation_fails_closed_for_every_unsafe_projection() {
    let directory = std::env::temp_dir().join(format!("muniment-resume-{}", Uuid::now_v7()));
    let sessions = ChatProfile::new(&directory).pi_session_root();
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(sessions.join("session.jsonl"), "{}\n").unwrap();
    let run_id = Uuid::now_v7().to_string();
    let events = |tail: Vec<(&str, Value)>| {
        let mut values = vec![event_envelope(
            &run_id,
            1,
            "run.started",
            json!({}),
            Some("owner"),
        )];
        values.extend(
            tail.into_iter()
                .enumerate()
                .map(|(index, (kind, payload))| {
                    event_envelope(&run_id, index as u64 + 2, kind, payload, Some("owner"))
                }),
        );
        values
    };
    let eligible = events(vec![
        (
            "runtime.pi_session.bound",
            json!({"run_id":run_id, "locator":"session.jsonl"}),
        ),
        ("run.needs_attention", json!({"reason":"interrupted"})),
    ]);
    assert!(resumable_context(&eligible, Some("owner"), &sessions).is_ok());
    assert!(resumable_context(&eligible, Some("another-subject"), &sessions).is_err());

    for unsafe_events in [
        events(vec![(
            "run.needs_attention",
            json!({"reason":"interrupted"}),
        )]),
        events(vec![
            (
                "runtime.pi_session.bound",
                json!({"run_id":run_id, "locator":"missing.jsonl"}),
            ),
            ("run.needs_attention", json!({"reason":"interrupted"})),
        ]),
        events(vec![("run.completed", json!({}))]),
        events(vec![
            (
                "runtime.pi_session.bound",
                json!({"run_id":run_id, "locator":"session.jsonl"}),
            ),
            (
                "permission.requested",
                json!({"gate_id":"gate", "kind":"confirm", "title":"Allow?", "message":"Proceed?"}),
            ),
            ("run.needs_attention", json!({"reason":"interrupted"})),
        ]),
        events(vec![
            (
                "runtime.pi_session.bound",
                json!({"run_id":run_id, "locator":"session.jsonl"}),
            ),
            (
                "tool.effect.started",
                json!({"effect_id":"effect", "display_name":"Command"}),
            ),
            ("run.needs_attention", json!({"reason":"interrupted"})),
        ]),
        Vec::new(),
    ] {
        assert!(resumable_context(&unsafe_events, Some("owner"), &sessions).is_err());
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn resume_runtime_failure_leaves_the_existing_journal_event_for_event_unchanged() {
    let _environment = lock_pi_environment();
    let directory = std::env::temp_dir().join(format!("muniment-resume-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("runs.sqlite3");
    let run_id = Uuid::now_v7().to_string();
    let mut journal = RunJournal::open(&path).unwrap();
    append_test_event(
        &mut journal,
        &run_id,
        1,
        "run.started",
        json!({}),
        Some("owner"),
    );
    append_test_event(
        &mut journal,
        &run_id,
        2,
        "runtime.pi_session.bound",
        json!({"run_id":run_id, "locator":"session.jsonl"}),
        Some("owner"),
    );
    append_test_event(
        &mut journal,
        &run_id,
        3,
        "run.needs_attention",
        json!({"reason":"interrupted"}),
        Some("owner"),
    );
    let before = journal.events(&run_id).unwrap();
    let sessions = ChatProfile::new(&directory).pi_session_root();
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(sessions.join("session.jsonl"), "{}\n").unwrap();
    let (locator, _) = validate_pi_session(&sessions, "session.jsonl").unwrap();
    let shared = Arc::new(Mutex::new(ChatStorage {
        journal,
        cas: LocalCas::open(&directory.join("cas")).unwrap(),
    }));
    let previous_root = std::env::var_os("MUNIMENT_PI_ROOT");
    // An unset override selects the profile root. An empty override rejects the launch before acquisition.
    std::env::set_var("MUNIMENT_PI_ROOT", "");
    let (sender, receiver) = std::sync::mpsc::channel();
    coordinate(
        FakeCoordinateSink::new(&directory),
        Arc::clone(&shared),
        Arc::new(Mutex::new(None)),
        RuntimeActivityRegistry::new(),
        Arc::new(crate::memory::ApplicationMemoryRuntime::new(
            directory.join("memory-config"),
            directory.join("memory-cache"),
        )),
        run_id.clone(),
        RESUME_PROMPT.into(),
        "token".into(),
        Some("owner".into()),
        ChatGrant {
            workspace: "workspace-a".into(),
            gateway_url: "https://gateway.invalid".into(),
            virtual_key: "virtual-key".into(),
            model: None,
            minimum_cacheable_prefix_characters: 8_192,
            receipt_url: "https://receipt.invalid".into(),
            expires_at: None,
            native_access_token: None,
        },
        Arc::new(AtomicBool::new(false)),
        Arc::new(Mutex::new(None)),
        Arc::new(Mutex::new(None)),
        Arc::new(Mutex::new(VecDeque::new())),
        Some(ResumeContext {
            events: before.clone(),
            locator,
        }),
        Some(sender),
        None,
    );
    if let Some(root) = previous_root {
        std::env::set_var("MUNIMENT_PI_ROOT", root);
    } else {
        std::env::remove_var("MUNIMENT_PI_ROOT");
    }
    assert!(receiver.recv().unwrap().is_err());
    assert!(!ChatProfile::new(&directory).pi_install_root().exists());
    assert_eq!(
        shared.lock().unwrap().journal.events(&run_id).unwrap(),
        before
    );
    drop(shared);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn resume_reopens_the_stub_session_and_completes_the_same_contiguous_run() {
    let _environment = lock_pi_environment();
    let directory = std::env::temp_dir().join(format!("muniment-resume-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&directory).unwrap();
    let args_log = directory.join("args.txt");
    let prompt_log = directory.join("prompt.txt");
    let request_log = directory.join("requests.jsonl");
    let session_name = format!("{}.jsonl", Uuid::now_v7());
    let sessions = ChatProfile::new(directory.clone()).pi_session_root();
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(sessions.join(&session_name), "persisted Pi data\n").unwrap();

    let executable_name = if cfg!(windows) {
        "sidecar-test-stub.exe"
    } else {
        "sidecar-test-stub"
    };
    let test_executable = std::env::current_exe().unwrap();
    let target_dir = test_executable.parent().unwrap().parent().unwrap();
    let stub = target_dir.join("examples").join(executable_name);
    assert!(stub.is_file(), "sidecar test stub was not built");

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let receipt_url = format!("http://{}/receipt", listener.local_addr().unwrap());
    let receipt_server = std::thread::spawn(move || {
        use std::io::{Read, Write};
        let mut stream = accept_receipt_request(listener);
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = [0; 4096];
        let _ = stream.read(&mut request).unwrap();
        let body = r#"{"route":"resume-stub","model":"test"}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    });

    let run_id = Uuid::now_v7().to_string();
    let mut journal = RunJournal::open(directory.join("runs.sqlite3")).unwrap();
    append_test_event(
        &mut journal,
        &run_id,
        1,
        "run.started",
        json!({}),
        Some("owner"),
    );
    append_test_event(
        &mut journal,
        &run_id,
        2,
        "runtime.pi_session.bound",
        json!({"run_id":run_id, "locator":session_name}),
        Some("owner"),
    );
    append_test_event(
        &mut journal,
        &run_id,
        3,
        "run.needs_attention",
        json!({"reason":"interrupted"}),
        Some("owner"),
    );
    let existing = journal.events(&run_id).unwrap();
    let (locator, _) = validate_pi_session(&sessions, &session_name).unwrap();
    let shared = Arc::new(Mutex::new(ChatStorage {
        journal,
        cas: LocalCas::open(&directory.join("cas")).unwrap(),
    }));

    std::env::set_var("MUNIMENT_PI_ROOT", &directory);
    std::env::set_var("PI_RESUME_STUB_ARGS", &args_log);
    std::env::set_var("PI_RESUME_STUB_PROMPTS", &prompt_log);
    std::env::set_var("PI_RESUME_STUB_REQUESTS", &request_log);
    let (sender, receiver) = std::sync::mpsc::channel();
    coordinate(
        FakeCoordinateSink::with_pi_stub(&directory, &directory, &stub),
        Arc::clone(&shared),
        Arc::new(Mutex::new(None)),
        RuntimeActivityRegistry::new(),
        Arc::new(crate::memory::ApplicationMemoryRuntime::new(
            directory.join("memory-config"),
            directory.join("memory-cache"),
        )),
        run_id.clone(),
        RESUME_PROMPT.into(),
        "token".into(),
        Some("owner".into()),
        ChatGrant {
            workspace: "workspace-a".into(),
            gateway_url: "https://gateway.invalid".into(),
            virtual_key: "virtual-key".into(),
            model: None,
            minimum_cacheable_prefix_characters: 8_192,
            receipt_url,
            expires_at: None,
            native_access_token: None,
        },
        Arc::new(AtomicBool::new(false)),
        Arc::new(Mutex::new(None)),
        Arc::new(Mutex::new(None)),
        Arc::new(Mutex::new(VecDeque::new())),
        Some(ResumeContext {
            events: existing,
            locator,
        }),
        Some(sender),
        None,
    );
    assert_eq!(receiver.recv().unwrap(), Ok(()));
    receipt_server.join().unwrap();
    for key in [
        "MUNIMENT_PI_ROOT",
        "PI_RESUME_STUB_ARGS",
        "PI_RESUME_STUB_PROMPTS",
        "PI_RESUME_STUB_REQUESTS",
    ] {
        std::env::remove_var(key);
    }

    let events = shared.lock().unwrap().journal.events(&run_id).unwrap();
    assert_eq!(
        events.iter().map(|event| event.run_seq).collect::<Vec<_>>(),
        (1..=events.len() as u64).collect::<Vec<_>>()
    );
    assert_eq!(events.last().unwrap().event_type, "run.completed");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "run.started")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "runtime.pi_session.bound")
            .count(),
        1
    );
    let args = std::fs::read_to_string(args_log).unwrap();
    assert!(args.lines().any(|argument| argument == "--session"));
    assert!(args.contains(&session_name));
    let sent_prompt = std::fs::read_to_string(prompt_log).unwrap();
    assert_eq!(sent_prompt, format!("{RESUME_PROMPT}\n"));
    assert!(!sent_prompt.contains("ORIGINAL PROTECTED PROMPT"));
    let request: serde_json::Value =
        serde_json::from_str(std::fs::read_to_string(request_log).unwrap().trim()).unwrap();
    assert!(request.get("images").is_none());

    std::fs::remove_file(sessions.join(session_name)).unwrap();
    drop(shared);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn resumed_run_answers_a_memory_search_and_ends_without_a_session() {
    let _environment = lock_pi_environment();
    let app = tauri::test::mock_app();
    let directory = std::env::temp_dir().join(format!("muniment-resume-memory-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&directory).unwrap();
    let config = directory.join("config");
    let home = directory.join("home");
    muniment_core::home::confirm_home(&config, &home).unwrap();
    std::fs::write(home.join("memory/fact.md"), "saffron belongs in the pantry").unwrap();
    app.manage(Arc::new(crate::memory::ApplicationMemoryRuntime::new(
        config,
        directory.join("cache"),
    )));

    let session_name = format!("{}.jsonl", Uuid::now_v7());
    let sessions = ChatProfile::new(app.path().app_data_dir().unwrap()).pi_session_root();
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(sessions.join(&session_name), "persisted Pi data\n").unwrap();

    let executable_name = if cfg!(windows) {
        "sidecar-test-stub.exe"
    } else {
        "sidecar-test-stub"
    };
    let test_executable = std::env::current_exe().unwrap();
    let stub = test_executable
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples")
        .join(executable_name);
    assert!(stub.is_file(), "sidecar test stub was not built");

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let receipt_url = format!("http://{}/receipt", listener.local_addr().unwrap());
    let receipt_server = std::thread::spawn(move || {
        use std::io::{Read, Write};
        let mut stream = accept_receipt_request(listener);
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = [0; 4096];
        let _ = stream.read(&mut request).unwrap();
        let body = r#"{"route":"resume-stub","model":"test"}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    });

    let run_id = Uuid::now_v7().to_string();
    let mut journal = RunJournal::open(directory.join("runs.sqlite3")).unwrap();
    journal
        .append_new_run(
            "workspace-a",
            &event_envelope(&run_id, 1, "run.started", json!({}), Some("owner")),
        )
        .unwrap();
    append_test_event(
        &mut journal,
        &run_id,
        2,
        "runtime.pi_session.bound",
        json!({"run_id":run_id, "locator":session_name}),
        Some("owner"),
    );
    append_test_event(
        &mut journal,
        &run_id,
        3,
        "run.needs_attention",
        json!({"reason":"interrupted"}),
        Some("owner"),
    );
    let thread_id = journal.run_thread_id(&run_id).unwrap().unwrap();
    let existing = journal.events(&run_id).unwrap();
    let (locator, _) = validate_pi_session(&sessions, &session_name).unwrap();
    let shared = Arc::new(Mutex::new(ChatStorage {
        journal,
        cas: LocalCas::open(&directory.join("cas")).unwrap(),
    }));

    let cancelled = Arc::new(AtomicBool::new(false));
    let transport = Arc::new(Mutex::new(None));
    let adapter = Arc::new(Mutex::new(None));
    let permission_answers = Arc::new(Mutex::new(VecDeque::new()));
    let runtime_activity = RuntimeActivityRegistry::new();
    let active = Arc::new(Mutex::new(None));
    install_resume_run(
        app.state::<Arc<crate::memory::ApplicationMemoryRuntime>>()
            .inner(),
        &active,
        ActiveRun {
            id: run_id.clone(),
            workspace: "workspace-a".into(),
            cancelled: Arc::clone(&cancelled),
            transport: Arc::clone(&transport),
            adapter: Arc::clone(&adapter),
            permission_answers: Arc::clone(&permission_answers),
            _activity: runtime_activity.mark_active_run(),
        },
        &thread_id,
        8_192,
    )
    .unwrap();
    std::env::set_var("MUNIMENT_PI_ROOT", &directory);
    std::env::set_var("PI_RESUME_STUB_MEMORY_QUERY", "saffron");
    let (sender, receiver) = std::sync::mpsc::channel();
    run_resume(ResumeLaunch {
        sink: FakeCoordinateSink::with_pi_stub(
            &app.path().app_data_dir().unwrap(),
            &directory,
            &stub,
        )
        .with_memory_agent_extension(
            app.state::<Arc<crate::memory::ApplicationMemoryRuntime>>()
                .agent_extension_path(),
        ),
        storage: Arc::clone(&shared),
        runtime: Arc::new(Mutex::new(None)),
        runtime_activity: runtime_activity.clone(),
        memory_runtime: Arc::clone(
            app.state::<Arc<crate::memory::ApplicationMemoryRuntime>>()
                .inner(),
        ),
        active: Arc::clone(&active),
        run_id: run_id.clone(),
        tokens: TokenSet {
            access_token: "token".into(),
            refresh_token: None,
            expires_at: None,
            subject: Some("owner".into()),
        },
        grant: ChatGrant {
            native_access_token: None,
            expires_at: None,
            workspace: "workspace-a".into(),
            gateway_url: "https://gateway.invalid".into(),
            virtual_key: "virtual-key".into(),
            model: None,
            minimum_cacheable_prefix_characters: 8_192,
            receipt_url,
        },
        cancelled,
        transport,
        adapter,
        permission_answers,
        resume: ResumeContext {
            events: existing,
            locator,
        },
        attempt: sender,
    });
    assert_eq!(receiver.recv().unwrap(), Ok(()));
    receipt_server.join().unwrap();
    for key in ["MUNIMENT_PI_ROOT", "PI_RESUME_STUB_MEMORY_QUERY"] {
        std::env::remove_var(key);
    }

    let events = shared.lock().unwrap().journal.events(&run_id).unwrap();
    assert_eq!(events.last().unwrap().event_type, "run.completed");
    let recalls: Vec<_> = events
        .iter()
        .filter(|event| event.event_type == "memory.recalled")
        .collect();
    assert_eq!(recalls.len(), 1);
    let EventPayload::Inline { payload_json } = &recalls[0].payload else {
        panic!("memory recall must use an inline payload");
    };
    assert!(payload_json["files"]
        .as_array()
        .unwrap()
        .contains(&json!("memory/fact.md")));
    assert_eq!(payload_json["thread"], json!(thread_id));
    assert_eq!(payload_json["character_budget"], json!(8_192));
    assert!(app
        .state::<Arc<crate::memory::ApplicationMemoryRuntime>>()
        .dispatch_tool_call(&run_id, "memory-search", br#"{"query":"saffron"}"#)
        .is_err());

    std::fs::remove_file(sessions.join(session_name)).unwrap();
    drop(shared);
    std::fs::remove_dir_all(directory).unwrap();
}

fn expired_run_event(run: u64, seq: u64, kind: &str) -> EventEnvelope {
    EventEnvelope {
        event_id: format!("0190b100-0000-7000-8000-{run:06}{seq:06}"),
        run_id: format!("0190b000-0000-7000-8000-{run:012}"),
        run_seq: seq,
        event_type: kind.into(),
        event_version: 1,
        envelope_version: 1,
        recorded_at: "2020-01-01T00:00:00Z".into(),
        occurred_at: None,
        correlation_id: None,
        causation_id: None,
        payload: EventPayload::Inline {
            payload_json: json!({}),
        },
        provenance: desktop_provenance(Some("owner")),
        extra: Default::default(),
    }
}

fn append_expired_run(storage: &SharedStorage, run: u64) {
    storage
        .lock()
        .unwrap()
        .journal
        .append_batch(
            0,
            &[
                expired_run_event(run, 1, "run.started"),
                expired_run_event(run, 2, "run.completed"),
            ],
        )
        .unwrap();
}

fn wait_for_empty_journal(storage: &SharedStorage) {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        if storage
            .lock()
            .unwrap()
            .journal
            .run_ids()
            .unwrap()
            .is_empty()
        {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the retention schedule did not check the journal"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn desktop_prompt_persistence_requires_the_runtime() {
    assert_eq!(
        super::resume::protect_prompt("run", "prompt", Some("owner")).unwrap_err(),
        auth::background_service_error()
    );
    assert!(matches!(
        super::run_preparation::fetch_grant("access"),
        Err(FetchGrantError::Unavailable)
    ));
}

#[test]
fn an_uninstalled_retention_trigger_takes_no_check() {
    assert!(!RetentionTrigger::default().check_now());
    let (trigger, checks) = RetentionTrigger::for_test();
    assert!(trigger.check_now());
    assert!(checks.recv().is_ok());
}

#[test]
fn the_retention_schedule_checks_again_when_a_save_triggers_it() {
    let directory =
        std::env::temp_dir().join(format!("muniment-retention-schedule-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&directory).unwrap();
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let config_directory = directory.join("config");
    muniment_core::retention_record::write_retention_choice(
        &config_directory,
        muniment_core::retention_record::RetentionChoice::DeleteAfter30Days,
    )
    .unwrap();
    let storage: SharedStorage = Arc::new(Mutex::new(ChatStorage {
        journal: RunJournal::open(directory.join("runs.sqlite3")).unwrap(),
        cas: LocalCas::open(&directory.join("cas")).unwrap(),
    }));
    append_expired_run(&storage, 1);

    let trigger = start_retention_schedule(config_directory, Arc::clone(&storage), |deleted_run| {
        muniment_core::chat_prompt::delete_prompt(
            &deleted_run.run_id,
            deleted_run.subject.as_deref(),
        )
        .map_err(|_| RetentionError::BeforeDelete)
    });
    wait_for_empty_journal(&storage);

    append_expired_run(&storage, 2);
    trigger.send(()).unwrap();
    wait_for_empty_journal(&storage);

    drop(trigger);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn file_panel_reads_text_and_rejects_binary_large_missing_and_directory() {
    let temp = std::env::temp_dir().join(format!("muniment-file-panel-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&temp).unwrap();
    let path = temp.as_path().join("preview.rs");
    std::fs::write(&path, "fn main() {}\n").unwrap();
    assert_eq!(tauri::async_runtime::block_on(chat_file_content(path.clone())).unwrap(), "fn main() {}\n");
    std::fs::write(&path, [0, 1, 2]).unwrap();
    assert!(tauri::async_runtime::block_on(chat_file_content(path.clone())).unwrap_err().contains("binary"));
    std::fs::write(&path, vec![b'a'; 512 * 1024 + 1]).unwrap();
    assert!(tauri::async_runtime::block_on(chat_file_content(path)).unwrap_err().contains("too large"));
    assert!(tauri::async_runtime::block_on(chat_file_content(temp.as_path().to_owned())).is_err());
    assert!(tauri::async_runtime::block_on(chat_file_content(temp.as_path().join("missing"))).is_err());
    std::fs::remove_dir_all(temp).unwrap();
}
