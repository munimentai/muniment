use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

#[cfg(target_os = "linux")]
use muniment_core::attach::linux::{
    ThreadListPage, ThreadListRequest, ThreadListService, ThreadOpenPage, ThreadOpenRequest,
};
#[cfg(target_os = "linux")]
use muniment_core::attach::ProtocolError;
use muniment_core::auth::TokenSet;
use muniment_core::journal::reducer::ChatProjector;
#[cfg(target_os = "linux")]
use muniment_core::journal::RunJournal;
use muniment_core::journal::{EventEnvelope, Provenance};
use serde_json::json;
use serde_json::Value;

use crate::chat::{
    attachment_error, chat_attachments, event_envelope, ActiveRun, ChatAttachment, ChatGrant,
    RunStartBoundaries, RunStartError, RunStartLaunch, SelectedFile,
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
    pub(crate) install_error: Option<String>,
    pub(crate) prepare_error: Option<String>,
    pub(crate) projection_error: Option<String>,
    pub(crate) prepared_provenance: Mutex<Option<Provenance>>,
    pub(crate) journaled_events: Mutex<BTreeMap<String, Vec<EventEnvelope>>>,
    pub(crate) clear_calls: AtomicUsize,
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
            install_error: None,
            prepare_error: None,
            projection_error: None,
            prepared_provenance: Mutex::new(None),
            journaled_events: Mutex::new(BTreeMap::new()),
            clear_calls: AtomicUsize::new(0),
            #[cfg(target_os = "linux")]
            journal: Mutex::new(RunJournal::open(":memory:").unwrap()),
        }
    }
}

impl RunStartBoundaries for FakeRunStartBoundaries {
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
        self.prompt_protection_calls.fetch_add(1, Ordering::SeqCst);
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
            receipt_url: "https://receipt.invalid".into(),
        })
    }

    fn install_active_run(&self, _run: ActiveRun) -> Result<(), RunStartError> {
        if let Some(error) = &self.install_error {
            return Err(RunStartError::InvalidRequest(error.clone()));
        }
        Ok(())
    }

    fn prepare_run(
        &self,
        run_id: &str,
        _grant: &ChatGrant,
        tokens: &TokenSet,
        _files: Vec<SelectedFile>,
        provenance: Option<Provenance>,
        _thread_id: Option<&str>,
    ) -> Result<(u64, ChatProjector), RunStartError> {
        self.prepare_calls.fetch_add(1, Ordering::SeqCst);
        *self.prepared_provenance.lock().unwrap() = provenance;
        if let Some(error) = &self.prepare_error {
            return Err(RunStartError::Persistence(error.clone()));
        }
        let mut projector = ChatProjector::new();
        let started = event_envelope(
            run_id,
            1,
            "run.started",
            json!({}),
            tokens.subject.as_deref(),
        );
        projector.apply(&started).unwrap();
        self.journaled_events
            .lock()
            .unwrap()
            .insert(run_id.to_owned(), vec![started]);
        Ok((1, projector))
    }

    fn run_thread_id(&self, _run_id: &str) -> Result<String, RunStartError> {
        Ok("0190a100-0000-7000-8000-000000000002".into())
    }

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

    fn launch(&self, launch: RunStartLaunch) {
        self.launch_calls.fetch_add(1, Ordering::SeqCst);
        *self.launched_run.lock().unwrap() = Some(launch.run_id);
    }
}
