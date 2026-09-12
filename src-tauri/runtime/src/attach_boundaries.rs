//! Runtime-owned boundaries for desktop attach reads.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::AtomicBool;
#[cfg(any(unix, target_os = "windows"))]
use std::sync::atomic::Ordering;
#[cfg(any(unix, target_os = "windows"))]
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
#[cfg(any(unix, target_os = "windows"))]
use std::time::Duration;

#[cfg(any(unix, target_os = "windows"))]
use muniment_core::active_run::{ChatDelivery, ChatQueueRequest};
#[cfg(any(unix, target_os = "windows"))]
use muniment_core::attach::desktop_service_message::{ArtifactFetchResult, RunStreamPage};
#[cfg(any(unix, target_os = "windows"))]
use muniment_core::attach::live_connections::LiveConnectionRegistry;
#[cfg(any(unix, target_os = "windows"))]
use muniment_core::attach::thread_service::{
    ThreadListPage, ThreadListRequest, ThreadListService, ThreadOpenPage, ThreadOpenRequest,
};
#[cfg(any(unix, target_os = "windows"))]
use muniment_core::attach::ProtocolError;
#[cfg(any(unix, target_os = "windows"))]
use muniment_core::attach::{
    bounded_claim, ApprovalCoordinator, ApprovalDecision, ApprovalRequest,
};
#[cfg(any(unix, target_os = "windows"))]
use muniment_core::attach::{CompanionRecord, EntitlementSnapshotResult};
use muniment_core::attach::{
    CompanionRegistry, RuntimeActivityGuard, RuntimeActivityRegistry, SignedWorkspaceApproval,
};
#[cfg(any(unix, target_os = "windows"))]
use muniment_core::auth::NativeDeviceListError;
use muniment_core::auth::{BrowserOpenError, BrowserOpener, EntitlementSnapshotTracker, TokenSet};
#[cfg(any(unix, target_os = "windows"))]
use muniment_core::cas::ContentHash;
use muniment_core::chat_grant::{ChatGrant, FetchGrantError};
use muniment_core::chat_resume::{clear_active_run, install_active_run};
use muniment_core::chat_view::{chat_attachments, ChatAttachment, SelectedFile};
use muniment_core::journal::reducer::ChatProjector;
#[cfg(any(unix, target_os = "windows"))]
use muniment_core::journal::thread_mutation::{
    append_thread_delete_now, append_thread_rename_now, create_thread_now, ThreadMutationError,
};
use muniment_core::journal::Provenance;
use muniment_core::memory_index::ModelMemoryCapability;
use muniment_core::memory_runtime::ApplicationMemoryRuntime;
#[cfg(any(unix, target_os = "windows"))]
use muniment_core::permission_gate::ChatPermissionAnswer;
use muniment_core::pi_execution::PiRuntime;
use muniment_core::run_events::SharedStorage;
use muniment_core::run_preparation::{
    append_prepared_run_persistence_failure, prepare_opened_run_with_prompt_storage,
    OpenSelectedFile, SessionThreadStart,
};
#[cfg(any(unix, target_os = "windows"))]
use muniment_core::run_start::{
    accepted_time_now, new_run_id, AttachPromptAccepted, AttachResumeAccepted, RunAttachBoundaries,
};
use muniment_core::run_start::{ActiveRun, RunStartBoundaries, RunStartError, RunStartLaunch};
use muniment_core::session_thread::SessionThread;
use muniment_core::sidecar::pi_install::{PiArtifactDescriptor, PI_SELECTED_ARTIFACT};

use crate::service::{self, ConfigureRunError};
#[cfg(any(unix, target_os = "windows"))]
use crate::RuntimeChatEventTarget;
use crate::{RuntimeChatEventBroadcast, RuntimeChatEventSink};

#[cfg(any(unix, target_os = "windows"))]
const ATTACH_PERMISSION_COMMIT_TIMEOUT: Duration = Duration::from_secs(2);

/// Supplies attach reads from runtime-owned state.
// Four fields serve only the `RunAttachBoundaries` impl below.
#[cfg_attr(not(any(unix, target_os = "windows")), allow(dead_code))]
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
    #[cfg(any(unix, target_os = "windows"))]
    approvals: ApprovalCoordinator,
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
            approval.clone(),
            session_thread,
            companion_registry,
            #[cfg(any(unix, target_os = "windows"))]
            ApprovalCoordinator::default(),
            Arc::new(AtomicBool::new(false)),
            RuntimeChatEventBroadcast::new(approval),
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
        #[cfg(any(unix, target_os = "windows"))] approvals: ApprovalCoordinator,
        sign_in_running: Arc<AtomicBool>,
        chat_events: RuntimeChatEventBroadcast,
    ) -> Self {
        let chat_events = chat_events.with_config_directory(config_directory.clone());
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
            #[cfg(any(unix, target_os = "windows"))]
            approvals,
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

    fn local_mode(&self) -> bool {
        muniment_core::local_mode::is_local_mode(&self.config_directory)
    }

    /// Returns the approval state shared with the attach listener.
    pub fn signed_workspace_approval(&self) -> SignedWorkspaceApproval {
        self.approval.clone()
    }

    /// Returns the approval coordinator shared by this activation.
    #[cfg(any(unix, target_os = "windows"))]
    pub fn approval_coordinator(&self) -> ApprovalCoordinator {
        self.approvals.clone()
    }

    /// Returns the live connection registry shared by this activation.
    #[cfg(any(unix, target_os = "windows"))]
    pub fn live_connections(&self) -> LiveConnectionRegistry {
        self.companion_registry.live_connections()
    }

    /// Prompts the claimed presenter for one companion pairing decision.
    #[cfg(any(unix, target_os = "windows"))]
    pub fn request_approval(
        &self,
        challenge: &str,
        claimed_kind: &str,
        claimed_version: &str,
        remaining: Duration,
    ) -> ApprovalDecision {
        request_approval(
            &self.approval,
            &self.approvals,
            challenge,
            claimed_kind,
            claimed_version,
            remaining,
        )
    }
}

#[cfg(any(unix, target_os = "windows"))]
pub(crate) fn request_approval(
    approval: &SignedWorkspaceApproval,
    approvals: &ApprovalCoordinator,
    challenge: &str,
    claimed_kind: &str,
    claimed_version: &str,
    remaining: Duration,
) -> ApprovalDecision {
    let Some(recorded) = approval.approval() else {
        return ApprovalDecision::Deny;
    };
    let approved = approvals.request(
        ApprovalRequest {
            challenge: challenge.to_owned(),
            claimed_kind: bounded_claim(claimed_kind),
            claimed_version: bounded_claim(claimed_version),
            workspace: recorded.workspace.clone(),
            scopes: recorded.scopes.clone(),
        },
        remaining,
    );
    if !approved {
        return ApprovalDecision::Deny;
    }
    match approval.approval() {
        Some(current) if current.workspace == recorded.workspace => {
            ApprovalDecision::Approve(current)
        }
        _ => ApprovalDecision::Deny,
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
        if self.local_mode() {
            return Ok(TokenSet {
                access_token: String::new(),
                refresh_token: None,
                expires_at: None,
                subject: None,
            });
        }
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
        if self.local_mode() {
            return Ok(ChatGrant::local());
        }
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
        let mut notice = None;
        prepare_opened_run_with_prompt_storage(
            &self.storage,
            SessionThreadStart {
                tracker: &self.session_thread,
                continue_existing: thread_id.is_none(),
            },
            run_id,
            &grant.workspace,
            tokens.subject.as_deref(),
            files,
            provenance,
            thread_id,
            "muniment-runtime",
            env!("CARGO_PKG_VERSION"),
            || {
                notice = service::prompt_storage::store_prompt_or_notice(
                    run_id,
                    prompt,
                    tokens.subject.as_deref(),
                );
                Ok(notice.clone())
            },
        )
        .map_err(|error| match notice {
            Some(notice) => format!("{notice} {error}"),
            None => error,
        })
        .map_err(|error| {
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
            .map_err(|error| persistence_error("lock", error.to_string()))?
            .journal
            .run_thread_id(run_id)
            .map_err(|error| persistence_error("journal", error))?
            .ok_or_else(|| {
                persistence_error(
                    "journal",
                    format!("The journal has no thread for run {run_id}."),
                )
            })
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
            .map_err(|error| persistence_error("memory session", error))
    }

    fn close_memory_session(&self, run_id: &str) {
        self.memory_runtime.close_session(run_id);
    }

    fn fail_prepared_run(&self, launch: &RunStartLaunch) -> Result<(), RunStartError> {
        append_prepared_run_persistence_failure(
            &mut self
                .storage
                .lock()
                .map_err(|error| persistence_error("lock", error.to_string()))?
                .journal,
            &launch.run_id,
            launch.prepared.0,
            launch.tokens.subject.as_deref(),
            "muniment-runtime",
            env!("CARGO_PKG_VERSION"),
        )
        .map_err(|error| persistence_error("journal", error))
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
        let thread_id = match self.run_thread_id(&launch.run_id) {
            Ok(thread_id) => thread_id,
            Err(_) => {
                self.close_memory_session(&launch.run_id);
                self.clear_active_run(&launch.run_id);
                return;
            }
        };
        let profile_directory = self.profile_directory.clone();
        let storage = Arc::clone(&self.storage);
        let runtime = Arc::clone(&self.runtime);
        let runtime_activity = self.runtime_activity.clone();
        let memory_runtime = Arc::clone(&self.memory_runtime);
        let active = Arc::clone(&self.active);
        let chat_events = self.chat_events.clone();
        let pi_artifact = self.pi_artifact.unwrap_or(PI_SELECTED_ARTIFACT);
        std::thread::spawn(move || {
            muniment_core::chat_coordinate::coordinate(
                RuntimeChatEventSink::new(
                    &profile_directory,
                    chat_events,
                    Arc::clone(&memory_runtime),
                    thread_id,
                    launch.grant.workspace.clone(),
                )
                .with_pi_artifact(pi_artifact),
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
            let file = std::fs::File::open(&selected.path)
                .map_err(|error| persistence_error("attachment", error))?;
            let metadata = file
                .metadata()
                .map_err(|error| persistence_error("attachment", error))?;
            if !metadata.is_file() {
                return Err(persistence_error(
                    "attachment",
                    "The selected path is not a file.",
                ));
            }
            let display_name = selected
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .ok_or_else(|| persistence_error("attachment", "The file name is invalid."))?
                .to_owned();
            Ok(OpenSelectedFile {
                file,
                display_name,
                byte_length: metadata.len(),
            })
        })
        .collect()
}

fn persistence_error(context: &str, error: impl std::fmt::Debug) -> RunStartError {
    RunStartError::Persistence(format!("Conversation history {context} failed: {error:?}"))
}

#[cfg(any(unix, target_os = "windows"))]
fn run_service_error(error: String) -> RunStartError {
    if error == "thread_not_found" {
        RunStartError::ThreadNotFound
    } else {
        RunStartError::InvalidRequest(error)
    }
}

#[cfg(any(unix, target_os = "windows"))]
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
            RuntimeChatEventTarget::Broadcast(self.chat_events.clone()),
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
        let workspace = if self.local_mode() {
            "local"
        } else {
            workspace
        };
        let belongs_to_workspace = self
            .storage
            .lock()
            .map_err(|error| persistence_error("lock", error.to_string()))?
            .journal
            .run_belongs_to_workspace(run_id, workspace)
            .map_err(|error| persistence_error("journal", error))?;
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
            RuntimeChatEventTarget::Broadcast(self.chat_events.clone()),
            self.pi_artifact,
        )
        .map_err(run_service_error)?;
        let mut storage = self
            .storage
            .lock()
            .map_err(|error| persistence_error("lock", error.to_string()))?;
        let thread_id = storage
            .journal
            .run_thread_id(run_id)
            .map_err(|error| persistence_error("journal", error))?
            .ok_or(RunStartError::ThreadNotFound)?;
        let committed_seq = storage
            .journal
            .events(run_id)
            .map_err(|error| persistence_error("journal", error))?
            .last()
            .map(|event| event.run_seq)
            .ok_or_else(|| {
                persistence_error(
                    "journal",
                    format!("The journal has no events for run {run_id}."),
                )
            })?;
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
        let workspace = if self.local_mode() {
            "local"
        } else {
            workspace
        };
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
        let marker = self
            .config_directory
            .join(muniment_core::local_mode::LOCAL_MODE_MARKER);
        match std::fs::remove_file(marker) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(ProtocolError::persistence_failed_with_reason(format!(
                    "The runtime could not remove the local mode marker: kind={:?} os_code={:?}.",
                    error.kind(),
                    error.raw_os_error(),
                )))
            }
        }
        let _permit = SignInPermit::acquire(Arc::clone(&self.sign_in_running))
            .ok_or_else(ProtocolError::invalid_request)?;
        service::sign_in(
            self.browser_opener.as_ref(),
            &self.entitlement_tracker,
            &self.runtime_activity,
        )
        .map_err(|error| ProtocolError::persistence_failed_with_reason(error.to_string()))
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
        .map_err(ProtocolError::persistence_failed_with_reason)
    }

    fn select_thread(&self, thread_id: &str) -> Result<bool, ProtocolError> {
        let subject = self
            .fresh_tokens()
            .map_err(|error| error.protocol_error())?
            .subject;
        service::select_thread(Arc::clone(&self.storage), subject, thread_id.to_owned())
            .map_err(ProtocolError::persistence_failed_with_reason)
    }

    fn recheck_retention(&self) -> Result<(), ProtocolError> {
        crate::attach_state::apply_recorded_retention(&self.config_directory, &self.storage)
            .map_err(ProtocolError::persistence_failed_with_reason)
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
        .map_err(ProtocolError::persistence_failed_with_reason)
    }

    fn create_thread(
        &self,
        workspace: &str,
        mut provenance: Provenance,
    ) -> Result<String, ProtocolError> {
        provenance.source = "muniment-runtime".into();
        provenance.source_version = env!("CARGO_PKG_VERSION").into();
        let mut storage = self.storage.lock().map_err(|error| {
            persistence_error("lock", error.to_string()).desktop_protocol_error()
        })?;
        create_thread_now(&mut storage.journal, workspace, provenance)
            .map_err(|error| persistence_error("journal", error).desktop_protocol_error())
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
        let mut storage = self.storage.lock().map_err(|error| {
            persistence_error("lock", error.to_string()).desktop_protocol_error()
        })?;
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
        let mut storage = self.storage.lock().map_err(|error| {
            persistence_error("lock", error.to_string()).desktop_protocol_error()
        })?;
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

    fn fetch_artifact(
        &self,
        workspace: &str,
        artifact_id: &muniment_core::attach::Id,
    ) -> Result<ArtifactFetchResult, ProtocolError> {
        let mut storage = self
            .storage
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        let reference = storage
            .journal
            .workspace_artifact(workspace, artifact_id.as_str())
            .map_err(|_| ProtocolError::persistence_failed())?
            .ok_or_else(ProtocolError::invalid_request)?;
        let hash: ContentHash = reference
            .sha256
            .parse()
            .map_err(|_| ProtocolError::invalid_request())?;
        let object = storage
            .cas
            .open_object(&hash)
            .map_err(|_| ProtocolError::invalid_request())?
            .ok_or_else(ProtocolError::invalid_request)?;
        let total_bytes = object
            .metadata()
            .map_err(|_| ProtocolError::invalid_request())?
            .len();
        if total_bytes != reference.byte_length || storage.cas.verify(&hash).is_err() {
            return Err(ProtocolError::invalid_request());
        }
        Ok(ArtifactFetchResult {
            total_bytes,
            sha256: reference.sha256,
        })
    }

    fn read_artifact_range(
        &self,
        workspace: &str,
        artifact_id: &muniment_core::attach::Id,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, ProtocolError> {
        let mut storage = self
            .storage
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        let reference = storage
            .journal
            .workspace_artifact(workspace, artifact_id.as_str())
            .map_err(|_| ProtocolError::persistence_failed())?
            .ok_or_else(ProtocolError::invalid_request)?;
        let hash: ContentHash = reference
            .sha256
            .parse()
            .map_err(|_| ProtocolError::invalid_request())?;
        storage
            .cas
            .read_range(&hash, offset, length)
            .map_err(|_| ProtocolError::persistence_failed())
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

    #[cfg(target_os = "linux")]
    fn record_chat_delivery_failure(&self, run_id: &str, cause: &str) {
        let workspace = if self.local_mode() {
            "local".to_owned()
        } else if let Some(approval) = self.approval.approval() {
            approval.workspace
        } else {
            return;
        };
        let Ok(storage) = self.storage.lock() else {
            return;
        };
        if !matches!(
            storage.journal.run_belongs_to_workspace(run_id, &workspace),
            Ok(true)
        ) {
            return;
        }
        let Ok(Some(thread_id)) = storage.journal.run_thread_id(run_id) else {
            return;
        };
        drop(storage);
        self.chat_events.record_delivery_failure(
            workspace,
            muniment_core::run_events::ChatEvent {
                run_id: run_id.to_owned(),
                thread_id: Some(thread_id),
                phase: "delivery-failed".into(),
                text: String::new(),
                prompt_storage_notice: None,
                failure_reason: Some(cause.to_owned()),
                receipt: None,
                tool_activity: Vec::new(),
                attachments: Vec::new(),
                recalls: Vec::new(),
                applied_diffs: Vec::new(),
                pending_permission: None,
            },
        );
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

#[cfg(any(unix, target_os = "windows"))]
struct SignInPermit(Arc<AtomicBool>);

#[cfg(any(unix, target_os = "windows"))]
impl SignInPermit {
    fn acquire(running: Arc<AtomicBool>) -> Option<Self> {
        running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok()
            .map(|_| Self(running))
    }
}

#[cfg(any(unix, target_os = "windows"))]
impl Drop for SignInPermit {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn open_browser(url: &str) -> Result<(), BrowserOpenError> {
    browser_command(url)
        .spawn()
        .map(drop)
        .map_err(|_| BrowserOpenError)
}

#[cfg(target_os = "macos")]
fn browser_command(url: &str) -> Command {
    let mut command = Command::new("open");
    command.arg(url);
    command
}

#[cfg(target_os = "windows")]
fn browser_command(url: &str) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // rundll32 takes the URL as a plain argument, so cmd.exe never parses the
    // `&` in the query string.
    let mut command = Command::new("rundll32");
    command
        .args(["url.dll,FileProtocolHandler", url])
        .creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(all(unix, not(target_os = "macos")))]
fn browser_command(url: &str) -> Command {
    browser_command_with_browser(url, std::env::var_os("BROWSER").as_deref())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn browser_command_with_browser(url: &str, browser: Option<&std::ffi::OsStr>) -> Command {
    let program = browser
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| std::ffi::OsStr::new("xdg-open"));
    let mut command = Command::new(program);
    command.arg(url);
    command
}

#[cfg(any(unix, target_os = "windows"))]
fn thread_mutation_protocol_error(error: ThreadMutationError) -> ProtocolError {
    match error {
        ThreadMutationError::NotOwned => ProtocolError::thread_not_found(),
        ThreadMutationError::Ownership(_) | ThreadMutationError::Journal(_) => {
            persistence_error("journal", error).desktop_protocol_error()
        }
    }
}

#[cfg(any(unix, target_os = "windows"))]
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

    #[cfg(unix)]
    #[test]
    fn presenter_disconnect_releases_the_runtime_claim() {
        use muniment_core::attach::{serve_approval_presenter, ApprovalPresenterConnection};
        use std::os::unix::net::UnixStream;

        let coordinator = ApprovalCoordinator::default();
        for _ in 0..2 {
            let (runtime, desktop) = UnixStream::pair().unwrap();
            let session = serve_approval_presenter(
                coordinator.clone(),
                ApprovalPresenterConnection::new(runtime, "presenter"),
            )
            .unwrap();
            assert!(coordinator.claim_presenter(|_| true).is_none());
            let (sender, closed) = std::sync::mpsc::channel();
            let waiter = std::thread::spawn(move || {
                session.wait_until_closed();
                drop(session);
                sender.send(()).unwrap();
            });
            drop(desktop);
            closed.recv_timeout(Duration::from_secs(2)).unwrap();
            waiter.join().unwrap();
        }
    }

    #[test]
    fn persistence_rejection_keeps_the_cause_for_the_desktop_owner() {
        let cause = std::io::Error::other("the journal is locked");
        let error = persistence_error("journal", cause);
        let reason = "Conversation history journal failed: Custom { kind: Other, error: \"the journal is locked\" }";
        let desktop = error.desktop_protocol_error();
        assert_eq!(
            desktop,
            ProtocolError::persistence_failed_with_reason(reason)
        );
        assert_eq!(
            desktop.to_string(),
            format!("code=\"persistence_failed\" reason={reason:?}")
        );
        assert_eq!(error.protocol_error(), ProtocolError::persistence_failed());
    }

    #[test]
    fn run_rejection_keeps_the_runtime_reason_on_the_wire() {
        use muniment_core::attach::{
            decode_frame, encode_frame, Envelope, ErrorDetails, ErrorEnvelope, Failure, Id,
            Protocol,
        };

        for reason in [
            "Enter a message before sending.",
            "A reply is already in progress.",
            "Conversation history is unavailable.",
            "A reason with \"quotes\" and a newline.\nNext line.",
        ] {
            let error = run_service_error(reason.into()).desktop_protocol_error();
            let frame = encode_frame(&ErrorEnvelope {
                protocol: Protocol,
                request_id: Some(Id::new("018f0000-0000-7000-8000-000000000201").unwrap()),
                ok: Failure,
                error: error.clone(),
            })
            .unwrap();
            let (Envelope::Error(envelope), _) = decode_frame::<Envelope>(&frame).unwrap().unwrap()
            else {
                panic!("expected a run rejection");
            };
            assert_eq!(envelope.error, error);
            assert_eq!(
                envelope.error.details(),
                Some(&ErrorDetails::RequestReason {
                    reason: reason.into()
                })
            );
            assert_eq!(
                envelope.error.to_string(),
                format!("code=\"invalid_request\" reason={reason:?}")
            );
            assert!(!envelope.error.to_string().contains('\n'));
        }
        assert_eq!(
            run_service_error("thread_not_found".into()).protocol_error(),
            ProtocolError::thread_not_found()
        );
    }

    #[cfg(unix)]
    #[test]
    fn recorded_workspace_approval_reaches_the_claimed_presenter() {
        let approval = SignedWorkspaceApproval::default();
        approval.record("workspace-a".into());
        let approvals = ApprovalCoordinator::default();
        let decider = approvals.clone();
        approvals.register_presenter(move |request| decider.decide(&request.challenge, true));

        let decision = request_approval(
            &approval,
            &approvals,
            "challenge-a",
            "editor-extension",
            "1.0.0",
            Duration::from_secs(1),
        );

        assert!(matches!(decision, ApprovalDecision::Approve(_)));
    }

    #[cfg(unix)]
    #[test]
    fn missing_workspace_approval_denies_without_prompting() {
        let prompted = Arc::new(AtomicBool::new(false));
        let presenter_prompted = Arc::clone(&prompted);
        let approvals = ApprovalCoordinator::default();
        approvals.register_presenter(move |_| {
            presenter_prompted.store(true, Ordering::Release);
            true
        });

        assert_eq!(
            request_approval(
                &SignedWorkspaceApproval::default(),
                &approvals,
                "challenge-a",
                "editor-extension",
                "1.0.0",
                Duration::from_secs(1),
            ),
            ApprovalDecision::Deny
        );
        assert!(!prompted.load(Ordering::Acquire));
    }

    #[cfg(any(unix, target_os = "windows"))]
    #[test]
    fn sign_in_permit_clears_when_an_attempt_ends() {
        let running = Arc::new(AtomicBool::new(false));
        let first = SignInPermit::acquire(Arc::clone(&running)).unwrap();
        assert!(SignInPermit::acquire(Arc::clone(&running)).is_none());
        drop(first);
        assert!(SignInPermit::acquire(running).is_some());
    }

    #[test]
    fn the_default_browser_command_matches_the_host_platform() {
        let url = "https://example.test/sign-in?a=1&b=2";
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let command = browser_command(url);
        #[cfg(all(unix, not(target_os = "macos")))]
        let command = browser_command_with_browser(url, None);

        #[cfg(target_os = "macos")]
        let expected_program = "open";
        #[cfg(target_os = "windows")]
        let expected_program = "rundll32";
        #[cfg(all(unix, not(target_os = "macos")))]
        let expected_program = "xdg-open";

        assert_eq!(command.get_program(), expected_program);
        let arguments: Vec<String> = command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect();
        assert_eq!(arguments.last().map(String::as_str), Some(url));
        #[cfg(target_os = "windows")]
        assert_eq!(
            arguments.first().map(String::as_str),
            Some("url.dll,FileProtocolHandler")
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn linux_browser_command_uses_the_configured_browser() {
        let url = "https://example.test/sign-in?a=1&b=2";
        let command = browser_command_with_browser(
            url,
            Some(std::ffi::OsStr::new("/tmp/configured-browser")),
        );

        assert_eq!(command.get_program(), "/tmp/configured-browser");
        assert_eq!(
            command
                .get_args()
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            [url]
        );
        assert_eq!(
            browser_command_with_browser(url, Some(std::ffi::OsStr::new(""))).get_program(),
            "xdg-open"
        );
    }
}
