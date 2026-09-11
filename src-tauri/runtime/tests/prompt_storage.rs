use std::any::Any;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use muniment_core::attach::RuntimeActivityRegistry;
use muniment_core::journal::reducer::project_chat_fragment;
use muniment_core::session_thread::SessionThread;
use muniment_runtime::{
    accept_prompt, apply_retention, drive_prompt, open_profile_storage, thread_page,
    RuntimeChatEventTarget,
};

mod common;
use common::{local_grant, stage_pi_stub, TemporaryProfile};

static ENVIRONMENT: Mutex<()> = Mutex::new(());
static WRITES: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Debug)]
struct RefusedKeyring {
    code: i32,
    message: &'static str,
    refuse_entry: bool,
}

impl std::fmt::Display for RefusedKeyring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for RefusedKeyring {}

impl RefusedKeyring {
    fn error(&self) -> keyring::Error {
        keyring::Error::PlatformFailure(Box::new(self.clone()))
    }
}

impl keyring::credential::CredentialBuilderApi for RefusedKeyring {
    fn build(
        &self,
        _target: Option<&str>,
        _service: &str,
        _user: &str,
    ) -> keyring::Result<Box<keyring::Credential>> {
        if self.refuse_entry {
            WRITES.fetch_add(1, Ordering::SeqCst);
            return Err(self.error());
        }
        Ok(Box::new(self.clone()))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl keyring::credential::CredentialApi for RefusedKeyring {
    fn set_secret(&self, _secret: &[u8]) -> keyring::Result<()> {
        WRITES.fetch_add(1, Ordering::SeqCst);
        Err(self.error())
    }

    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        panic!("The runtime must not read a prompt that the keyring refused.");
    }

    fn delete_credential(&self) -> keyring::Result<()> {
        panic!("Retention must not delete a prompt that the keyring refused.");
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[test]
fn refused_keyring_starts_local_runs_and_preserves_the_notice_after_reopen() {
    let _environment = ENVIRONMENT.lock().unwrap();
    let temporary = TemporaryProfile::new("refused-prompt-store", true);
    let descriptor = stage_pi_stub(&temporary.root);
    let storage = open_profile_storage(&temporary.profile).unwrap();
    let runtime = Arc::new(Mutex::new(None));
    let activity = RuntimeActivityRegistry::new();
    let active = Arc::new(Mutex::new(None));
    let session_thread = SessionThread::default();
    let mut thread_id = None;
    let captured = temporary.root.join("captured-prompts");
    std::env::set_var("PI_RESUME_STUB_PROMPTS", &captured);
    WRITES.store(0, Ordering::SeqCst);

    for (index, code, message, refuse_entry) in [
        (1, -25307, "A default keychain could not be found.", false),
        (2, -25308, "User interaction is not allowed.", false),
        (3, -25307, "A default keychain could not be found.", true),
    ] {
        keyring::set_default_credential_builder(Box::new(RefusedKeyring {
            code,
            message,
            refuse_entry,
        }));
        let run_id = format!("018f0000-0000-7000-8000-{index:012}");
        let prompt = format!("private prompt {index}");
        let (sender, events) = mpsc::channel();
        let (accepted, launch) = accept_prompt(
            &temporary.profile,
            Arc::clone(&storage),
            Arc::clone(&runtime),
            &activity,
            &temporary.config,
            run_id.clone(),
            prompt.clone(),
            thread_id.clone(),
            &session_thread,
            false,
            String::new(),
            None,
            Vec::new(),
            local_grant(),
            Arc::clone(&active),
            RuntimeChatEventTarget::Subscriber(Some(sender)),
            Some(descriptor),
        )
        .unwrap();
        if let Some(thread_id) = &thread_id {
            assert_eq!(&accepted.thread_id, thread_id);
        }
        thread_id = Some(accepted.thread_id);
        assert_eq!(accepted.committed_seq, 1);
        assert_eq!(active.lock().unwrap().as_ref().unwrap().id, run_id);
        let journal_events = storage.lock().unwrap().journal.events(&run_id).unwrap();
        assert_eq!(journal_events[0].event_type, "run.started");
        assert_eq!(journal_events.len(), 1);
        let notice = project_chat_fragment(&journal_events)
            .unwrap()
            .prompt_storage_notice
            .unwrap();
        assert!(notice.contains("runtime memory"));
        assert!(notice.contains(&code.to_string()));
        assert!(notice.contains(message));
        assert!(!notice.contains("Conversation history is unavailable."));
        assert!(!serde_json::to_string(&journal_events)
            .unwrap()
            .contains(&prompt));

        // The notice must not turn an accepted run into a failed reply.
        drive_prompt(launch);
        let delivered: Vec<_> = events.try_iter().collect();
        assert!(delivered.iter().any(|event| !event.text.is_empty()));
        assert!(delivered
            .iter()
            .all(|event| event.prompt_storage_notice.as_deref() == Some(&notice)));
        assert_eq!(delivered.last().unwrap().phase, "complete");
        assert!(active.lock().unwrap().is_none());
        assert!(!activity.snapshot().active_run);
        assert!(std::fs::read_to_string(&captured)
            .unwrap()
            .contains(&prompt));
    }
    assert_eq!(WRITES.load(Ordering::SeqCst), 3);
    drop(storage);
    let reopened = open_profile_storage(&temporary.profile).unwrap();
    let page = thread_page(
        &temporary.profile,
        Arc::clone(&reopened),
        None,
        thread_id.unwrap(),
        10,
        None,
    )
    .unwrap();
    assert_eq!(page.entries.len(), 3);
    for entry in page.entries {
        assert!(entry.prompt.is_none());
        assert!(entry
            .prompt_storage_notice
            .unwrap()
            .contains("runtime memory"));
        assert!(!entry.text.is_empty());
    }
    let retention = apply_retention(reopened, 0).unwrap();
    assert_eq!(retention.deleted_runs.len(), 3);
    assert!(retention.deleted_runs.iter().all(|run| !run.prompt_stored));
    std::env::remove_var("MUNIMENT_PI_ROOT");
    std::env::remove_var("PI_RESUME_STUB_PROMPTS");
}

#[test]
fn attach_submit_sends_without_a_default_keychain() {
    use muniment_core::run_start::{RunAttachBoundaries, RunStartBoundaries};
    use std::time::{Duration, Instant};

    let _environment = ENVIRONMENT.lock().unwrap();
    keyring::set_default_credential_builder(Box::new(RefusedKeyring {
        code: -25307,
        message: "A default keychain could not be found.",
        refuse_entry: false,
    }));
    let temporary = TemporaryProfile::new("refused-prompt-submit", true);
    std::fs::write(
        temporary
            .config
            .join(muniment_core::local_mode::LOCAL_MODE_MARKER),
        "1",
    )
    .unwrap();
    let descriptor = stage_pi_stub(&temporary.root);
    let state =
        muniment_runtime::RuntimeAttachState::open(&temporary.profile, &temporary.config).unwrap();
    state.signed_workspace_approval().record("local".into());
    let boundaries = state.boundaries().with_pi_artifact(descriptor);
    let events = boundaries.subscribe_chat_events().unwrap();
    let accepted = boundaries
        .submit_run("local", "Send this prompt".into(), Vec::new(), None)
        .unwrap();
    assert!(!accepted.run_id.is_empty());
    assert_eq!(accepted.committed_seq, 1);
    let mut saw_reply = false;
    loop {
        let event = events.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(event.run_id, accepted.run_id);
        let notice = event.prompt_storage_notice.unwrap();
        assert!(notice.contains("-25307"));
        assert!(notice.contains("A default keychain could not be found."));
        saw_reply |= !event.text.is_empty();
        assert_ne!(event.phase, "failed");
        if event.phase == "complete" {
            break;
        }
    }
    assert!(saw_reply);
    let deadline = Instant::now() + Duration::from_secs(5);
    while boundaries.active_run_exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let storage = open_profile_storage(&temporary.profile).unwrap();
    let journal_events = storage
        .lock()
        .unwrap()
        .journal
        .events(&accepted.run_id)
        .unwrap();
    assert_eq!(journal_events.first().unwrap().event_type, "run.started");
    assert_eq!(journal_events.last().unwrap().event_type, "run.completed");
    std::env::remove_var("MUNIMENT_PI_ROOT");
}

#[test]
fn an_attachment_failure_after_a_keyring_failure_keeps_both_causes() {
    let _environment = ENVIRONMENT.lock().unwrap();
    keyring::set_default_credential_builder(Box::new(RefusedKeyring {
        code: -25308,
        message: "User interaction is not allowed.",
        refuse_entry: false,
    }));
    let temporary = TemporaryProfile::new("refused-prompt-attachment", true);
    let storage = open_profile_storage(&temporary.profile).unwrap();
    let active = Arc::new(Mutex::new(None));
    let activity = RuntimeActivityRegistry::new();
    let path = temporary.root.join("attachment.txt");
    std::fs::write(&path, b"content").unwrap();
    let run_id = "018f0000-0000-7000-8000-000000000098";
    let result = accept_prompt(
        &temporary.profile,
        Arc::clone(&storage),
        Arc::new(Mutex::new(None)),
        &activity,
        &temporary.config,
        run_id.into(),
        "private prompt".into(),
        None,
        &SessionThread::default(),
        false,
        String::new(),
        None,
        vec![muniment_core::run_preparation::OpenSelectedFile {
            file: std::fs::File::open(path).unwrap(),
            display_name: "attachment.txt".into(),
            byte_length: 100,
        }],
        local_grant(),
        Arc::clone(&active),
        RuntimeChatEventTarget::Subscriber(None),
        None,
    );
    let error = result.err().unwrap();
    assert!(error.contains("-25308"));
    assert!(error.contains("User interaction is not allowed."));
    assert!(error.contains(&muniment_core::pi_execution::attachment_error()));
    assert!(!error.contains("private prompt"));
    let events = storage.lock().unwrap().journal.events(run_id).unwrap();
    assert_eq!(events.first().unwrap().event_type, "run.started");
    assert_eq!(events.last().unwrap().event_type, "run.failed");
    let notice = project_chat_fragment(&events)
        .unwrap()
        .prompt_storage_notice
        .unwrap();
    assert!(notice.contains("-25308"));
    assert!(notice.contains("User interaction is not allowed."));
    assert!(!serde_json::to_string(&events)
        .unwrap()
        .contains("private prompt"));
    let thread_id = storage
        .lock()
        .unwrap()
        .journal
        .run_thread_id(run_id)
        .unwrap()
        .unwrap();
    drop(storage);
    let reopened = open_profile_storage(&temporary.profile).unwrap();
    let page = thread_page(
        &temporary.profile,
        Arc::clone(&reopened),
        None,
        thread_id,
        10,
        None,
    )
    .unwrap();
    assert_eq!(page.entries.len(), 1);
    assert!(page.entries[0].prompt.is_none());
    assert_eq!(page.entries[0].phase, "failed");
    assert_eq!(
        page.entries[0].prompt_storage_notice.as_deref(),
        Some(notice.as_str())
    );
    let retention = apply_retention(reopened, 0).unwrap();
    assert_eq!(retention.deleted_runs.len(), 1);
    assert_eq!(retention.deleted_runs[0].run_id, run_id);
    assert!(!retention.deleted_runs[0].prompt_stored);
    assert!(active.lock().unwrap().is_none());
    assert!(!activity.snapshot().active_run);
}

#[test]
fn an_invalid_thread_does_not_touch_the_prompt_store() {
    let _environment = ENVIRONMENT.lock().unwrap();
    keyring::set_default_credential_builder(Box::new(RefusedKeyring {
        code: -25307,
        message: "A default keychain could not be found.",
        refuse_entry: false,
    }));
    WRITES.store(0, Ordering::SeqCst);
    let temporary = TemporaryProfile::new("refused-prompt-invalid-thread", true);
    let storage = open_profile_storage(&temporary.profile).unwrap();
    let active = Arc::new(Mutex::new(None));
    let activity = RuntimeActivityRegistry::new();
    let run_id = "018f0000-0000-7000-8000-000000000099";
    let result = accept_prompt(
        &temporary.profile,
        Arc::clone(&storage),
        Arc::new(Mutex::new(None)),
        &activity,
        &temporary.config,
        run_id.into(),
        "private prompt".into(),
        Some("unknown-thread".into()),
        &SessionThread::default(),
        false,
        String::new(),
        None,
        Vec::new(),
        local_grant(),
        Arc::clone(&active),
        RuntimeChatEventTarget::Subscriber(None),
        None,
    );
    assert_eq!(result.err().unwrap(), "thread_not_found");
    assert_eq!(WRITES.load(Ordering::SeqCst), 0);
    assert!(storage
        .lock()
        .unwrap()
        .journal
        .events(run_id)
        .unwrap()
        .is_empty());
    assert!(active.lock().unwrap().is_none());
    assert!(!activity.snapshot().active_run);
}
