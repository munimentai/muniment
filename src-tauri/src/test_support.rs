use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

#[cfg(target_os = "linux")]
use muniment_core::attach::linux::{
    RunStreamPage, ThreadListPage, ThreadListRequest, ThreadListService, ThreadOpenPage,
    ThreadOpenRequest,
};
#[cfg(target_os = "linux")]
use muniment_core::attach::ProtocolError;
use muniment_core::attach::{RuntimeActivityGuard, RuntimeActivityRegistry};
use muniment_core::auth::TokenSet;
use muniment_core::chat_grant::ChatGrant;
use muniment_core::journal::reducer::ChatProjector;
#[cfg(target_os = "linux")]
use muniment_core::journal::RunJournal;
use muniment_core::journal::{EventEnvelope, JournalCommitHint, Provenance};
use serde_json::json;
use serde_json::Value;

use crate::chat::{
    attachment_error, chat_attachments, event_envelope, ActiveRun, ChatAttachment,
    ChatPermissionAnswer, RunStartBoundaries, RunStartError, RunStartLaunch, SelectedFile,
};

pub(crate) fn append_test_event(
    journal: &mut muniment_core::journal::RunJournal,
    run_id: &str,
    seq: u64,
    kind: &str,
    payload: Value,
    subject: Option<&str>,
) {
    journal
        .append(
            seq - 1,
            &crate::chat::event_envelope(run_id, seq, kind, payload, subject),
        )
        .unwrap();
}

pub(crate) struct FakeRunStartBoundaries {
    pub(crate) active: bool,
    pub(crate) granted_workspaces: Vec<String>,
    pub(crate) auth_calls: AtomicUsize,
    pub(crate) prompt_protection_calls: AtomicUsize,
    pub(crate) prepare_calls: AtomicUsize,
    pub(crate) launch_calls: AtomicUsize,
    pub(crate) launched_run: Mutex<Option<String>>,
    pub(crate) auth_error: Option<String>,
    pub(crate) configure_error: Option<String>,
    pub(crate) protect_error: Option<String>,
    pub(crate) install_error: Option<String>,
    pub(crate) prepare_error: Option<String>,
    pub(crate) thread_id_error: Option<String>,
    pub(crate) memory_error: Option<String>,
    pub(crate) projection_error: Option<String>,
    pub(crate) prepared_provenance: Mutex<Option<Provenance>>,
    pub(crate) journaled_events: Mutex<BTreeMap<String, Vec<EventEnvelope>>>,
    pub(crate) clear_calls: AtomicUsize,
    pub(crate) cancel_calls: AtomicUsize,
    pub(crate) active_run: Mutex<Option<(String, String)>>,
    pub(crate) runtime_activity: RuntimeActivityRegistry,
    #[cfg(target_os = "linux")]
    pub(crate) queued_permission_answers: Mutex<Vec<(String, ChatPermissionAnswer)>>,
    #[cfg(target_os = "linux")]
    pub(crate) permission_auto_commit: bool,
    #[cfg(target_os = "linux")]
    pub(crate) permission_competing_answer: bool,
    #[cfg(target_os = "linux")]
    pub(crate) permission_resolution_sender:
        Mutex<Option<std::sync::mpsc::SyncSender<Option<u64>>>>,
    #[cfg(target_os = "linux")]
    pub(crate) permission_commit_sender:
        Mutex<Option<std::sync::mpsc::SyncSender<JournalCommitHint>>>,
    #[cfg(target_os = "linux")]
    pub(crate) journal: Mutex<RunJournal>,
}

impl FakeRunStartBoundaries {
    pub(crate) fn accepting() -> Self {
        Self {
            active: false,
            granted_workspaces: vec!["workspace-a".into()],
            auth_calls: AtomicUsize::new(0),
            prompt_protection_calls: AtomicUsize::new(0),
            prepare_calls: AtomicUsize::new(0),
            launch_calls: AtomicUsize::new(0),
            launched_run: Mutex::new(None),
            auth_error: None,
            configure_error: None,
            protect_error: None,
            install_error: None,
            prepare_error: None,
            thread_id_error: None,
            memory_error: None,
            projection_error: None,
            prepared_provenance: Mutex::new(None),
            journaled_events: Mutex::new(BTreeMap::new()),
            clear_calls: AtomicUsize::new(0),
            cancel_calls: AtomicUsize::new(0),
            active_run: Mutex::new(None),
            runtime_activity: RuntimeActivityRegistry::new(),
            #[cfg(target_os = "linux")]
            queued_permission_answers: Mutex::new(Vec::new()),
            #[cfg(target_os = "linux")]
            permission_auto_commit: true,
            #[cfg(target_os = "linux")]
            permission_competing_answer: false,
            #[cfg(target_os = "linux")]
            permission_resolution_sender: Mutex::new(None),
            #[cfg(target_os = "linux")]
            permission_commit_sender: Mutex::new(None),
            #[cfg(target_os = "linux")]
            journal: Mutex::new(RunJournal::open(":memory:").unwrap()),
        }
    }
}

impl RunStartBoundaries for FakeRunStartBoundaries {
    fn mark_active_run(&self) -> RuntimeActivityGuard {
        self.runtime_activity.mark_active_run()
    }

    #[cfg(target_os = "linux")]
    fn list_threads(
        &self,
        workspace: &str,
        request: ThreadListRequest,
    ) -> Result<ThreadListPage, ProtocolError> {
        let mut journal = self
            .journal
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        ThreadListService::list_threads(&mut *journal, workspace, request)
    }

    #[cfg(target_os = "linux")]
    fn open_thread(
        &self,
        workspace: &str,
        request: ThreadOpenRequest,
    ) -> Result<ThreadOpenPage, ProtocolError> {
        let mut journal = self
            .journal
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        ThreadListService::open_thread(&mut *journal, workspace, request)
    }

    #[cfg(target_os = "linux")]
    fn create_thread(
        &self,
        workspace: &str,
        provenance: Provenance,
    ) -> Result<String, ProtocolError> {
        self.journal
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?
            .create_thread(workspace, "2026-01-01T00:00:00Z", provenance)
            .map_err(|_| ProtocolError::persistence_failed())
    }

    #[cfg(target_os = "linux")]
    fn stream_run(
        &self,
        workspace: &str,
        run_id: &str,
        after_run_seq: u64,
    ) -> Result<RunStreamPage, ProtocolError> {
        ThreadListService::stream_run(
            &mut *self
                .journal
                .lock()
                .map_err(|_| ProtocolError::persistence_failed())?,
            workspace,
            run_id,
            after_run_seq,
        )
    }

    #[cfg(target_os = "linux")]
    fn subscribe_run_commits(
        &self,
        run_id: &str,
    ) -> Result<(u64, std::sync::mpsc::Receiver<JournalCommitHint>), ProtocolError> {
        let high_water = self
            .journal
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?
            .run_event_types()
            .map_err(|_| ProtocolError::persistence_failed())?
            .into_iter()
            .filter(|event| event.run_id == run_id)
            .last()
            .map_or(0, |event| event.run_seq);
        let (sender, receiver) = std::sync::mpsc::sync_channel(4);
        *self.permission_commit_sender.lock().unwrap() = Some(sender);
        Ok((high_water, receiver))
    }

    #[cfg(target_os = "linux")]
    fn queue_attach_permission_answer(
        &self,
        workspace: &str,
        run_id: &str,
        gate_id: &str,
        answer: ChatPermissionAnswer,
    ) -> Result<std::sync::mpsc::Receiver<Option<u64>>, RunStartError> {
        if !self
            .active_run
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|active| active.0 == run_id && active.1 == workspace)
        {
            return Err(RunStartError::InvalidRequest(
                "That reply is no longer active.".into(),
            ));
        }
        let (resolved_sender, resolved_receiver) = std::sync::mpsc::sync_channel(1);
        self.queued_permission_answers
            .lock()
            .unwrap()
            .push((gate_id.to_owned(), answer.clone()));
        if !self.permission_auto_commit {
            *self.permission_resolution_sender.lock().unwrap() = Some(resolved_sender);
            return Ok(resolved_receiver);
        }
        let mut journal = self.journal.lock().unwrap();
        let seq = journal
            .run_event_types()
            .unwrap()
            .into_iter()
            .filter(|event| event.run_id == run_id)
            .last()
            .unwrap()
            .run_seq
            + 1;
        journal
            .append(
                seq - 1,
                &event_envelope(
                    run_id,
                    seq,
                    "permission.resolved",
                    json!({
                        "gate_id": gate_id,
                        "decision": if self.permission_competing_answer {
                            ChatPermissionAnswer::Confirm(!matches!(answer, ChatPermissionAnswer::Confirm(true))).decision()
                        } else {
                            answer.decision()
                        }
                    }),
                    None,
                ),
            )
            .unwrap();
        drop(journal);
        if let Some(sender) = self.permission_commit_sender.lock().unwrap().as_ref() {
            sender
                .send(JournalCommitHint {
                    run_id: run_id.to_owned(),
                    run_seq: seq,
                })
                .unwrap();
        }
        resolved_sender
            .send((!self.permission_competing_answer).then_some(seq))
            .unwrap();
        Ok(resolved_receiver)
    }

    fn active_run_exists(&self) -> bool {
        self.active
    }

    fn fresh_tokens(&self) -> Result<TokenSet, RunStartError> {
        self.auth_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = &self.auth_error {
            return Err(RunStartError::Unauthorized(error.clone()));
        }
        Ok(TokenSet {
            access_token: "access-token".into(),
            refresh_token: None,
            expires_at: None,
            subject: Some("owner".into()),
        })
    }

    fn configure_run(
        &self,
        _run_id: &str,
        _prompt: &str,
        _tokens: &TokenSet,
        requested_workspace: Option<&str>,
    ) -> Result<ChatGrant, RunStartError> {
        if requested_workspace
            .is_some_and(|workspace| !self.granted_workspaces.iter().any(|g| g == workspace))
        {
            return Err(RunStartError::Unauthorized(
                "sensitive workspace detail".into(),
            ));
        }
        if let Some(error) = &self.configure_error {
            return Err(RunStartError::Persistence(error.clone()));
        }
        // Model the gateway granting exactly the workspace that was
        // requested (and authorized), so the capability check
        // `requested == grant.workspace` holds for every granted workspace.
        let workspace = requested_workspace
            .map(str::to_owned)
            .or_else(|| self.granted_workspaces.first().cloned())
            .unwrap_or_default();
        Ok(ChatGrant {
            workspace,
            gateway_url: "https://gateway.invalid".into(),
            virtual_key: "virtual-key".into(),
            model: None,
            minimum_cacheable_prefix_characters: 8_192,
            receipt_url: "https://receipt.invalid".into(),
        })
    }

    fn install_active_run(&self, run: ActiveRun) -> Result<(), RunStartError> {
        if let Some(error) = &self.install_error {
            return Err(RunStartError::InvalidRequest(error.clone()));
        }
        *self.active_run.lock().unwrap() = Some((run.id, run.workspace));
        Ok(())
    }

    fn prepare_run(
        &self,
        run_id: &str,
        _prompt: &str,
        grant: &ChatGrant,
        tokens: &TokenSet,
        _files: Vec<SelectedFile>,
        provenance: Option<Provenance>,
        thread_id: Option<&str>,
    ) -> Result<(u64, ChatProjector), RunStartError> {
        self.prepare_calls.fetch_add(1, Ordering::SeqCst);
        *self.prepared_provenance.lock().unwrap() = provenance.clone();
        if let Some(error) = &self.prepare_error {
            return Err(RunStartError::Persistence(error.clone()));
        }
        let mut projector = ChatProjector::new();
        let mut started = event_envelope(
            run_id,
            1,
            "run.started",
            json!({}),
            tokens.subject.as_deref(),
        );
        if let Some(provenance) = provenance {
            started.provenance = Provenance {
                actor_id: provenance
                    .actor_id
                    .or_else(|| tokens.subject.as_deref().map(str::to_owned)),
                ..provenance
            };
        }
        projector.apply(&started).unwrap();
        let mut events = vec![started.clone()];
        let mut committed_seq = 1;
        #[cfg(target_os = "linux")]
        if let Some(thread_id) = thread_id {
            let mut journal = self
                .journal
                .lock()
                .map_err(|_| RunStartError::Persistence(attachment_error()))?;
            let protect = || {
                self.prompt_protection_calls.fetch_add(1, Ordering::SeqCst);
                self.protect_error
                    .as_ref()
                    .map_or(Ok(()), |error| Err(error.clone()))
            };
            let protection = journal
                .append_new_run_in_thread_after_validation(
                    &grant.workspace,
                    thread_id,
                    &started,
                    protect,
                )
                .map_err(|error| {
                    if matches!(
                        error,
                        muniment_core::journal::JournalError::InvalidEnvelope(_)
                    ) {
                        RunStartError::ThreadNotFound
                    } else {
                        RunStartError::Persistence(attachment_error())
                    }
                })?;
            protection.map_err(RunStartError::Persistence)?;
            let prompt = event_envelope(
                run_id,
                2,
                "user.prompt.submitted",
                json!({"prompt": "hello"}),
                tokens.subject.as_deref(),
            );
            projector.apply(&prompt).unwrap();
            journal
                .append(1, &prompt)
                .map_err(|_| RunStartError::Persistence(attachment_error()))?;
            events.push(prompt);
            committed_seq = 2;
        } else {
            self.prompt_protection_calls.fetch_add(1, Ordering::SeqCst);
            if let Some(error) = &self.protect_error {
                return Err(RunStartError::Persistence(error.clone()));
            }
        }
        self.journaled_events
            .lock()
            .unwrap()
            .insert(run_id.to_owned(), events);
        Ok((committed_seq, projector))
    }

    fn run_thread_id(&self, run_id: &str) -> Result<String, RunStartError> {
        if let Some(error) = &self.thread_id_error {
            return Err(RunStartError::Persistence(error.clone()));
        }
        #[cfg(target_os = "linux")]
        if let Some(thread_id) = self
            .journal
            .lock()
            .map_err(|_| RunStartError::Persistence(attachment_error()))?
            .run_thread_id(run_id)
            .map_err(|_| RunStartError::Persistence(attachment_error()))?
        {
            return Ok(thread_id);
        }
        Ok("0190a100-0000-7000-8000-000000000002".into())
    }

    fn open_memory_session(
        &self,
        _run_id: &str,
        _thread_id: &str,
        _minimum_cacheable_prefix_characters: usize,
    ) -> Result<(), RunStartError> {
        self.memory_error.as_ref().map_or(Ok(()), |error| {
            Err(RunStartError::Persistence(error.clone()))
        })
    }

    fn close_memory_session(&self, _run_id: &str) {}

    fn project_attachments(
        &self,
        projector: &ChatProjector,
    ) -> Result<Vec<ChatAttachment>, RunStartError> {
        if let Some(error) = &self.projection_error {
            return Err(RunStartError::Persistence(error.clone()));
        }
        projector
            .projection()
            .map(|projection| chat_attachments(&projection.attachments))
            .map_err(|_| RunStartError::Persistence(attachment_error()))
    }

    fn fail_prepared_run(&self, launch: &RunStartLaunch) -> Result<(), RunStartError> {
        self.journaled_events
            .lock()
            .unwrap()
            .get_mut(&launch.run_id)
            .unwrap()
            .push(event_envelope(
                &launch.run_id,
                launch.prepared.0 + 1,
                "run.failed",
                json!({"reason": "persistence"}),
                launch.tokens.subject.as_deref(),
            ));
        Ok(())
    }

    fn clear_active_run(&self, _run_id: &str) {
        self.clear_calls.fetch_add(1, Ordering::SeqCst);
    }

    fn cancel_run(&self, workspace: &str, run_id: &str) -> Result<(), RunStartError> {
        let active = self.active_run.lock().unwrap();
        if active
            .as_ref()
            .is_none_or(|(id, active_workspace)| id != run_id || active_workspace != workspace)
        {
            return Err(RunStartError::InvalidRequest(
                "That reply is no longer active.".into(),
            ));
        }
        self.cancel_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn launch(&self, launch: RunStartLaunch) {
        self.launch_calls.fetch_add(1, Ordering::SeqCst);
        *self.launched_run.lock().unwrap() = Some(launch.run_id);
    }
}
