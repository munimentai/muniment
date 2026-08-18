//! Runtime-owned boundaries for desktop attach reads.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use muniment_core::active_run::{ChatDelivery, ChatQueueRequest};
use muniment_core::attach::linux::{
    EntitlementSnapshotResult, RunStreamPage, ThreadListPage, ThreadListRequest, ThreadListService,
    ThreadOpenPage, ThreadOpenRequest,
};
use muniment_core::attach::ProtocolError;
use muniment_core::attach::{
    CompanionRecord, CompanionRegistry, RuntimeActivityGuard, RuntimeActivityRegistry,
    SignedWorkspaceApproval,
};
use muniment_core::auth::{
    BrowserOpenError, BrowserOpener, EntitlementSnapshotTracker, NativeDeviceListError, TokenSet,
};
use muniment_core::chat_grant::{ChatGrant, FetchGrantError};
use muniment_core::chat_resume::{clear_active_run, install_active_run};
use muniment_core::chat_view::{chat_attachments, ChatAttachment, SelectedFile};
use muniment_core::journal::reducer::ChatProjector;
use muniment_core::journal::thread_mutation::{
    append_thread_delete_now, append_thread_rename_now, create_thread_now, ThreadMutationError,
};
use muniment_core::journal::Provenance;
use muniment_core::memory_index::ModelMemoryCapability;
use muniment_core::memory_runtime::ApplicationMemoryRuntime;
use muniment_core::permission_gate::ChatPermissionAnswer;
use muniment_core::pi_execution::PiRuntime;
use muniment_core::run_events::SharedStorage;
use muniment_core::run_preparation::{
    append_prepared_run_persistence_failure, prepare_new_run_in_thread_after_validation,
    prepare_new_run_with_session_thread, OpenSelectedFile, SessionThreadStart,
};
use muniment_core::run_start::{
    accepted_time_now, new_run_id, ActiveRun, AttachPromptAccepted, AttachResumeAccepted,
    RunAttachBoundaries, RunStartBoundaries, RunStartError, RunStartLaunch,
};
use muniment_core::session_thread::SessionThread;
use muniment_core::sidecar::pi_install::PiArtifactDescriptor;

use crate::service::{self, ConfigureRunError};
use crate::{RuntimeChatEventBroadcast, RuntimeChatEventSink};

const ATTACH_PERMISSION_COMMIT_TIMEOUT: Duration = Duration::from_secs(2);

/// Supplies attach reads from runtime-owned state.
pub struct RuntimeAttachBoundaries {
    storage: SharedStorage,
    active: Arc<Mutex<Option<ActiveRun>>>,
    profile_directory: PathBuf,
    config_directory: PathBuf,
    runtime: Arc<Mutex<Option<PiRuntime>>>,
    memory_runtime: Arc<ApplicationMemoryRuntime>,
    runtime_activity: RuntimeActivityRegistry,
    entitlement_tracker: Arc<EntitlementSnapshotTracker>,
    session_thread: Arc<SessionThread>,
    approval: SignedWorkspaceApproval,
    companion_registry: CompanionRegistry,
    sign_in_running: Arc<AtomicBool>,
    browser_opener: Arc<dyn BrowserOpener>,
    chat_events: RuntimeChatEventBroadcast,
    pi_artifact: Option<PiArtifactDescriptor>,
}

impl RuntimeAttachBoundaries {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        storage: SharedStorage,
        active: Arc<Mutex<Option<ActiveRun>>>,
        profile_directory: PathBuf,
        config_directory: PathBuf,
        runtime: Arc<Mutex<Option<PiRuntime>>>,
        memory_runtime: Arc<ApplicationMemoryRuntime>,
        runtime_activity: RuntimeActivityRegistry,
        entitlement_tracker: Arc<EntitlementSnapshotTracker>,
        approval: SignedWorkspaceApproval,
        session_thread: Arc<SessionThread>,
        companion_registry: CompanionRegistry,
    ) -> Self {
        Self::new_with_sign_in(
            storage,
            active,
            profile_directory,
            config_directory,
            runtime,
            memory_runtime,
            runtime_activity,
            entitlement_tracker,
            approval,
            session_thread,
            companion_registry,
            Arc::new(AtomicBool::new(false)),
            RuntimeChatEventBroadcast::default(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_with_sign_in(
        storage: SharedStorage,
        active: Arc<Mutex<Option<ActiveRun>>>,
        profile_directory: PathBuf,
        config_directory: PathBuf,
        runtime: Arc<Mutex<Option<PiRuntime>>>,
        memory_runtime: Arc<ApplicationMemoryRuntime>,
        runtime_activity: RuntimeActivityRegistry,
        entitlement_tracker: Arc<EntitlementSnapshotTracker>,
        approval: SignedWorkspaceApproval,
        session_thread: Arc<SessionThread>,
        companion_registry: CompanionRegistry,
        sign_in_running: Arc<AtomicBool>,
        chat_events: RuntimeChatEventBroadcast,
    ) -> Self {
        Self {
            storage,
            active,
            profile_directory,
            config_directory,
            runtime,
            memory_runtime,
            runtime_activity,
            entitlement_tracker,
            session_thread,
            approval,
            companion_registry,
            sign_in_running,
            browser_opener: Arc::new(open_browser),
            chat_events,
            pi_artifact: None,
        }
    }

    /// Replaces the production browser opener for tests and alternate hosts.
    pub fn with_browser_opener(mut self, browser_opener: Arc<dyn BrowserOpener>) -> Self {
        self.browser_opener = browser_opener;
        self
    }

    /// Replaces the production Pi artifact for tests and alternate hosts.
    pub fn with_pi_artifact(mut self, pi_artifact: PiArtifactDescriptor) -> Self {
        self.pi_artifact = Some(pi_artifact);
        self
    }

    pub fn clear_workspace(&self) {
        self.approval.clear();
    }

    /// Returns the approval state shared with the attach listener.
    pub fn signed_workspace_approval(&self) -> SignedWorkspaceApproval {
        self.approval.clone()
    }
}

impl RunStartBoundaries for RuntimeAttachBoundaries {
    fn mark_active_run(&self) -> RuntimeActivityGuard {
        self.runtime_activity.mark_active_run()
    }

    fn active_run_exists(&self) -> bool {
        self.active
            .lock()
            .map(|active| active.is_some())
            .unwrap_or(true)
    }

    fn fresh_tokens(&self) -> Result<TokenSet, RunStartError> {
        service::ensure_native_session(&self.runtime_activity)
            .map_err(|error| RunStartError::Unauthorized(error.to_string()))?
            .into_credentials()
            .map(|credentials| credentials.tokens)
            .ok_or_else(|| RunStartError::Unauthorized("Sign in to send a message.".into()))
    }

    fn configure_run(
        &self,
        _run_id: &str,
        _prompt: &str,
        tokens: &TokenSet,
        requested_workspace: Option<&str>,
    ) -> Result<ChatGrant, RunStartError> {
        let grant =
            service::configure_run(&tokens.access_token, requested_workspace).map_err(|error| {
                match error {
                    ConfigureRunError::Grant(FetchGrantError::Unauthorized) => {
                        RunStartError::Unauthorized("The capability is not authorized.".into())
                    }
                    ConfigureRunError::Grant(FetchGrantError::Unavailable) => {
                        RunStartError::Persistence(
                            "Chat configuration is temporarily unavailable.".into(),
                        )
                    }
                    ConfigureRunError::Grant(FetchGrantError::InvalidResponse) => {
                        RunStartError::Persistence(
                            "The chat configuration response was invalid.".into(),
                        )
                    }
                    ConfigureRunError::Unauthorized => {
                        RunStartError::Unauthorized("The capability is not authorized.".into())
                    }
                }
            })?;
        self.approval.record(grant.workspace.clone());
        Ok(grant)
    }

    #[cfg(target_os = "linux")]
    fn attach_approval(&self) -> Option<muniment_core::attach::Approval> {
        self.approval.approval()
    }

    fn install_active_run(&self, run: ActiveRun) -> Result<(), RunStartError> {
        install_active_run(&self.active, run).map_err(RunStartError::InvalidRequest)
    }

    fn prepare_run(
        &self,
        run_id: &str,
        prompt: &str,
        grant: &ChatGrant,
        tokens: &TokenSet,
        files: Vec<SelectedFile>,
        provenance: Option<Provenance>,
        thread_id: Option<&str>,
    ) -> Result<(u64, ChatProjector), RunStartError> {
        let files = open_selected_files(files)?;
        let protect = || {
            muniment_core::chat_prompt::store_prompt(run_id, prompt, tokens.subject.as_deref())
                .map_err(|_| "Conversation history is unavailable.".to_string())
        };
        let result = match thread_id {
            Some(thread_id) => prepare_new_run_in_thread_after_validation(
                &self.storage,
                run_id,
                &grant.workspace,
                tokens.subject.as_deref(),
                files,
                provenance,
                thread_id,
                "muniment-runtime",
                env!("CARGO_PKG_VERSION"),
                protect,
            ),
            None => prepare_new_run_with_session_thread(
                &self.storage,
                SessionThreadStart {
                    tracker: &self.session_thread,
                    continue_existing: true,
                },
                run_id,
                &grant.workspace,
                tokens.subject.as_deref(),
                files,
                provenance,
                "muniment-runtime",
                env!("CARGO_PKG_VERSION"),
                protect,
            ),
        };
        result.map_err(|error| {
            if error == "thread_not_found" {
                RunStartError::ThreadNotFound
            } else {
                RunStartError::Persistence(error)
            }
        })
    }

    fn project_attachments(
        &self,
        projector: &ChatProjector,
    ) -> Result<Vec<ChatAttachment>, RunStartError> {
        projector
            .projection()
            .map(|projection| chat_attachments(&projection.attachments))
            .map_err(|error| RunStartError::Persistence(error.to_string()))
    }

    fn run_thread_id(&self, run_id: &str) -> Result<String, RunStartError> {
        self.storage
            .lock()
            .map_err(|_| persistence_error())?
            .journal
            .run_thread_id(run_id)
            .map_err(|_| persistence_error())?
            .ok_or_else(persistence_error)
    }

    fn open_memory_session(
        &self,
        run_id: &str,
        thread_id: &str,
        minimum: usize,
    ) -> Result<(), RunStartError> {
        self.memory_runtime
            .open_session(
                run_id,
                thread_id,
                ModelMemoryCapability {
                    minimum_cacheable_prefix_characters: minimum,
                },
            )
            .map_err(|_| persistence_error())
    }

    fn close_memory_session(&self, run_id: &str) {
        self.memory_runtime.close_session(run_id);
    }

    fn fail_prepared_run(&self, launch: &RunStartLaunch) -> Result<(), RunStartError> {
        append_prepared_run_persistence_failure(
            &mut self
                .storage
                .lock()
                .map_err(|_| persistence_error())?
                .journal,
            &launch.run_id,
            launch.prepared.0,
            launch.tokens.subject.as_deref(),
            "muniment-runtime",
            env!("CARGO_PKG_VERSION"),
        )
        .map_err(|_| persistence_error())
    }

    fn cancel_run(&self, workspace: &str, run_id: &str) -> Result<(), RunStartError> {
        service::cancel_run(
            Arc::clone(&self.active),
            workspace.to_owned(),
            run_id.to_owned(),
        )
        .map_err(RunStartError::InvalidRequest)
    }

    fn clear_active_run(&self, run_id: &str) {
        clear_active_run(&self.active, run_id);
    }

    fn launch(&self, launch: RunStartLaunch) {
        let profile_directory = self.profile_directory.clone();
        let storage = Arc::clone(&self.storage);
        let runtime = Arc::clone(&self.runtime);
        let runtime_activity = self.runtime_activity.clone();
        let memory_runtime = Arc::clone(&self.memory_runtime);
        let active = Arc::clone(&self.active);
        let chat_events = self.chat_events.clone();
        std::thread::spawn(move || {
            muniment_core::chat_coordinate::coordinate(
                RuntimeChatEventSink::new(
                    &profile_directory,
                    chat_events,
                    Arc::clone(&memory_runtime),
                ),
                storage,
                runtime,
                runtime_activity,
                Arc::clone(&memory_runtime),
                launch.run_id.clone(),
                launch.prompt,
                launch.tokens.access_token,
                launch.tokens.subject,
                launch.grant,
                launch.cancelled,
                launch.transport,
                launch.adapter,
                launch.permission_answers,
                None,
                None,
                Some(launch.prepared),
            );
            memory_runtime.close_session(&launch.run_id);
            clear_active_run(&active, &launch.run_id);
        });
    }
}

fn open_selected_files(files: Vec<SelectedFile>) -> Result<Vec<OpenSelectedFile>, RunStartError> {
    files
        .into_iter()
        .map(|selected| {
            let file = std::fs::File::open(&selected.path).map_err(|_| persistence_error())?;
            let metadata = file.metadata().map_err(|_| persistence_error())?;
            if !metadata.is_file() {
                return Err(persistence_error());
            }
            let display_name = selected
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .ok_or_else(persistence_error)?
                .to_owned();
            Ok(OpenSelectedFile {
                file,
                display_name,
                byte_length: metadata.len(),
            })
        })
        .collect()
}

fn persistence_error() -> RunStartError {
    RunStartError::Persistence("Conversation history is unavailable.".into())
}

fn run_service_error(error: String) -> RunStartError {
    if error == "thread_not_found" {
        RunStartError::ThreadNotFound
    } else {
        RunStartError::InvalidRequest(error)
    }
}

impl RunAttachBoundaries for RuntimeAttachBoundaries {
    fn submit_run(
        &self,
        workspace: &str,
        text: String,
        files: Vec<SelectedFile>,
        thread_id: Option<String>,
    ) -> Result<AttachPromptAccepted, RunStartError> {
        let prompt = text.trim().to_owned();
        if prompt.is_empty() {
            return Err(RunStartError::InvalidRequest(
                "Enter a message before sending.".into(),
            ));
        }
        if self.active_run_exists() {
            return Err(RunStartError::InvalidRequest(
                "A reply is already in progress.".into(),
            ));
        }
        let tokens = self.fresh_tokens()?;
        let run_id = new_run_id();
        let grant = self.configure_run(
            &run_id,
            &prompt,
            &tokens,
            (!workspace.is_empty()).then_some(workspace),
        )?;
        let files = open_selected_files(files)?;
        let (accepted, launch) = service::accept_prompt(
            &self.profile_directory,
            Arc::clone(&self.storage),
            Arc::clone(&self.runtime),
            &self.runtime_activity,
            &self.config_directory,
            run_id,
            prompt,
            thread_id,
            &self.session_thread,
            false,
            tokens.access_token,
            tokens.subject,
            files,
            grant,
            Arc::clone(&self.active),
            None,
            self.pi_artifact,
        )
        .map_err(run_service_error)?;
        std::thread::spawn(move || service::drive_prompt(launch));
        Ok(AttachPromptAccepted {
            run_id: accepted.run_id,
            thread_id: accepted.thread_id,
            attachments: accepted.attachments,
            committed_seq: accepted.committed_seq,
            accepted_at: accepted.accepted_at,
        })
    }

    fn resume_run(
        &self,
        workspace: &str,
        run_id: &str,
    ) -> Result<AttachResumeAccepted, RunStartError> {
        if self.active_run_exists() {
            return Err(RunStartError::InvalidRequest(
                "A reply is already in progress.".into(),
            ));
        }
        let belongs_to_workspace = self
            .storage
            .lock()
            .map_err(|_| persistence_error())?
            .journal
            .run_belongs_to_workspace(run_id, workspace)
            .map_err(|_| persistence_error())?;
        if !belongs_to_workspace {
            return Err(RunStartError::InvalidRequest(
                "The run was not found or is inaccessible.".into(),
            ));
        }
        let tokens = self.fresh_tokens()?;
        let grant = self.configure_run(run_id, "", &tokens, Some(workspace))?;
        service::resume_run(
            &self.profile_directory,
            Arc::clone(&self.storage),
            Arc::clone(&self.runtime),
            &self.runtime_activity,
            &self.config_directory,
            run_id.to_owned(),
            tokens.access_token,
            tokens.subject,
            grant,
            Arc::clone(&self.active),
            None,
            self.pi_artifact,
        )
        .map_err(run_service_error)?;
        let mut storage = self.storage.lock().map_err(|_| persistence_error())?;
        let thread_id = storage
            .journal
            .run_thread_id(run_id)
            .map_err(|_| persistence_error())?
            .ok_or(RunStartError::ThreadNotFound)?;
        let committed_seq = storage
            .journal
            .events(run_id)
            .map_err(|_| persistence_error())?
            .last()
            .map(|event| event.run_seq)
            .ok_or_else(persistence_error)?;
        Ok(AttachResumeAccepted {
            run_id: run_id.to_owned(),
            thread_id,
            committed_seq,
            accepted_at: accepted_time_now(),
        })
    }

    fn queue_attach_message(
        &self,
        workspace: &str,
        run_id: &str,
        delivery: ChatDelivery,
        message: &str,
    ) -> Result<(), RunStartError> {
        service::queue_run_message(
            Arc::clone(&self.active),
            ChatQueueRequest {
                run_id: run_id.to_owned(),
                workspace: Some(workspace.to_owned()),
                delivery,
                message: message.to_owned(),
            },
        )
        .map_err(RunStartError::InvalidRequest)
    }
    fn session_status(&self) -> Result<muniment_core::auth::AuthStatus, ProtocolError> {
        service::session_status().map_err(|_| ProtocolError::persistence_failed())
    }

    fn entitlement_snapshot(&self) -> Result<EntitlementSnapshotResult, ProtocolError> {
        service::entitlement_snapshot(&self.entitlement_tracker, &self.runtime_activity)
            .map_err(|_| ProtocolError::persistence_failed())
    }

    fn sign_in(
        &self,
        _provenance: Provenance,
    ) -> Result<muniment_core::auth::AuthStatus, ProtocolError> {
        let _permit = SignInPermit::acquire(Arc::clone(&self.sign_in_running))
            .ok_or_else(ProtocolError::invalid_request)?;
        service::sign_in(
            self.browser_opener.as_ref(),
            &self.entitlement_tracker,
            &self.runtime_activity,
        )
        .map_err(|_| ProtocolError::persistence_failed())
    }

    fn sign_out(
        &self,
        _provenance: Provenance,
    ) -> Result<muniment_core::auth::AuthStatus, ProtocolError> {
        self.clear_workspace();
        service::sign_out(&self.entitlement_tracker, &self.runtime_activity)
            .map_err(|_| ProtocolError::persistence_failed())
    }

    fn list_devices(&self) -> Result<muniment_core::auth::NativeDeviceList, ProtocolError> {
        let access_token = self
            .fresh_tokens()
            .map_err(|error| error.protocol_error())?
            .access_token;
        service::list_devices(&access_token).map_err(device_list_protocol_error)
    }

    fn list_companions(&self) -> Result<Vec<CompanionRecord>, ProtocolError> {
        service::list_companions(&self.companion_registry)
    }

    fn revoke_companion(&self, client_identity: &str) -> Result<(), ProtocolError> {
        service::revoke_companion(&self.companion_registry, client_identity)
    }

    fn list_threads(
        &self,
        workspace: &str,
        request: ThreadListRequest,
    ) -> Result<ThreadListPage, ProtocolError> {
        let mut storage = self
            .storage
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        ThreadListService::list_threads(&mut storage.journal, workspace, request)
    }

    fn open_thread(
        &self,
        workspace: &str,
        request: ThreadOpenRequest,
    ) -> Result<ThreadOpenPage, ProtocolError> {
        let mut storage = self
            .storage
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        ThreadListService::open_thread(&mut storage.journal, workspace, request)
    }

    fn thread_summaries(
        &self,
        request: ThreadListRequest,
    ) -> Result<muniment_core::journal::thread_summaries::ThreadSummaryPage, ProtocolError> {
        let subject = self
            .fresh_tokens()
            .map_err(|error| error.protocol_error())?
            .subject;
        service::thread_summaries(
            Arc::clone(&self.storage),
            subject,
            usize::from(request.limit),
            request.cursor,
        )
        .map_err(|_| ProtocolError::persistence_failed())
    }

    fn select_thread(&self, thread_id: &str) -> Result<bool, ProtocolError> {
        let subject = self
            .fresh_tokens()
            .map_err(|error| error.protocol_error())?
            .subject;
        service::select_thread(Arc::clone(&self.storage), subject, thread_id.to_owned())
            .map_err(|_| ProtocolError::persistence_failed())
    }

    fn recheck_retention(&self) -> Result<(), ProtocolError> {
        crate::attach_state::apply_recorded_retention(&self.config_directory, &self.storage)
            .map_err(|_| ProtocolError::persistence_failed())
    }

    fn thread_history(
        &self,
        request: ThreadOpenRequest,
    ) -> Result<muniment_core::thread_history::ChatThreadOpenPage, ProtocolError> {
        let subject = self
            .fresh_tokens()
            .map_err(|error| error.protocol_error())?
            .subject;
        service::thread_page(
            &self.profile_directory,
            Arc::clone(&self.storage),
            subject,
            request.thread_id,
            usize::from(request.limit),
            request.cursor,
        )
        .map_err(|_| ProtocolError::persistence_failed())
    }

    fn create_thread(
        &self,
        workspace: &str,
        mut provenance: Provenance,
    ) -> Result<String, ProtocolError> {
        provenance.source = "muniment-runtime".into();
        provenance.source_version = env!("CARGO_PKG_VERSION").into();
        let mut storage = self
            .storage
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        create_thread_now(&mut storage.journal, workspace, provenance)
            .map_err(|_| ProtocolError::persistence_failed())
    }

    fn rename_thread(
        &self,
        thread_id: &str,
        title: &str,
        mut provenance: Provenance,
    ) -> Result<(), ProtocolError> {
        let subject = self
            .fresh_tokens()
            .map_err(|error| error.protocol_error())?
            .subject;
        provenance.source = "muniment-runtime".into();
        provenance.source_version = env!("CARGO_PKG_VERSION").into();
        let mut storage = self
            .storage
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        append_thread_rename_now(
            &mut storage.journal,
            subject.as_deref(),
            thread_id,
            title,
            &provenance,
        )
        .map_err(thread_mutation_protocol_error)
    }

    fn delete_thread(
        &self,
        thread_id: &str,
        mut provenance: Provenance,
    ) -> Result<(), ProtocolError> {
        let subject = self
            .fresh_tokens()
            .map_err(|error| error.protocol_error())?
            .subject;
        provenance.source = "muniment-runtime".into();
        provenance.source_version = env!("CARGO_PKG_VERSION").into();
        let mut storage = self
            .storage
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        append_thread_delete_now(
            &mut storage.journal,
            subject.as_deref(),
            thread_id,
            &provenance,
        )
        .map_err(thread_mutation_protocol_error)
    }

    fn stream_run(
        &self,
        workspace: &str,
        run_id: &str,
        after_run_seq: u64,
    ) -> Result<RunStreamPage, ProtocolError> {
        let mut storage = self
            .storage
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        ThreadListService::stream_run(&mut storage.journal, workspace, run_id, after_run_seq)
    }

    fn subscribe_run_commits(
        &self,
        run_id: &str,
    ) -> Result<muniment_core::journal::CommitSubscription, ProtocolError> {
        let mut storage = self
            .storage
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        if storage
            .journal
            .run_thread_id(run_id)
            .map_err(|_| ProtocolError::persistence_failed())?
            .is_none()
        {
            return Err(ProtocolError::thread_not_found());
        }
        storage
            .journal
            .subscribe_commits(run_id)
            .map_err(|_| ProtocolError::persistence_failed())
    }

    fn subscribe_chat_events(
        &self,
    ) -> Result<muniment_core::run_events::ChatEventSubscription, ProtocolError> {
        Ok(self.chat_events.subscribe())
    }

    fn queue_attach_permission_answer(
        &self,
        workspace: &str,
        run_id: &str,
        gate_id: &str,
        answer: ChatPermissionAnswer,
    ) -> Result<std::sync::mpsc::Receiver<Option<u64>>, RunStartError> {
        let committed_seq = service::answer_permission(
            Arc::clone(&self.active),
            workspace.to_owned(),
            run_id.to_owned(),
            gate_id.to_owned(),
            answer,
            ATTACH_PERMISSION_COMMIT_TIMEOUT,
        )
        .map_err(RunStartError::InvalidRequest)?;
        let (committed, resolved) = mpsc::channel();
        committed
            .send(Some(committed_seq))
            .map_err(|_| RunStartError::Persistence("The permission answer was lost.".into()))?;
        Ok(resolved)
    }
}

struct SignInPermit(Arc<AtomicBool>);

impl SignInPermit {
    fn acquire(running: Arc<AtomicBool>) -> Option<Self> {
        running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok()
            .map(|_| Self(running))
    }
}

impl Drop for SignInPermit {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn open_browser(url: &str) -> Result<(), BrowserOpenError> {
    Command::new("xdg-open")
        .arg(url)
        .spawn()
        .map(drop)
        .map_err(|_| BrowserOpenError)
}

fn thread_mutation_protocol_error(error: ThreadMutationError) -> ProtocolError {
    match error {
        ThreadMutationError::NotOwned => ProtocolError::thread_not_found(),
        ThreadMutationError::Ownership(_) | ThreadMutationError::Journal(_) => {
            ProtocolError::persistence_failed()
        }
    }
}

fn device_list_protocol_error(error: NativeDeviceListError) -> ProtocolError {
    match error {
        NativeDeviceListError::CredentialsMissing
        | NativeDeviceListError::HttpStatus(401 | 403) => ProtocolError::unauthorized(),
        NativeDeviceListError::Config(_)
        | NativeDeviceListError::Transport(_)
        | NativeDeviceListError::HttpStatus(_)
        | NativeDeviceListError::MalformedResponse(_) => ProtocolError::persistence_failed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_in_permit_clears_when_an_attempt_ends() {
        let running = Arc::new(AtomicBool::new(false));
        let first = SignInPermit::acquire(Arc::clone(&running)).unwrap();
        assert!(SignInPermit::acquire(Arc::clone(&running)).is_none());
        drop(first);
        assert!(SignInPermit::acquire(running).is_some());
    }
}
