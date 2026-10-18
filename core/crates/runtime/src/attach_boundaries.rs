//! Runtime-owned boundaries for desktop attach reads.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
#[cfg(any(unix, target_os = "windows"))]
use std::sync::atomic::Ordering;
#[cfg(any(unix, target_os = "windows"))]
use std::sync::mpsc;
use std::sync::Arc;
#[cfg(any(unix, target_os = "windows"))]
use std::time::Duration;

use muniment_core::account::TokenSet;
use muniment_core::active_run::RunRegistry;
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
use muniment_core::cas::ContentHash;
use muniment_core::chat_launch::{ChatGrant, FetchGrantError};
use muniment_core::chat_resume::{clear_active_run, install_active_run};
use muniment_core::chat_view::{chat_attachments, ChatAttachment, SelectedFile};
use muniment_core::engine::ThreadContexts;
#[cfg(any(unix, target_os = "windows"))]
use muniment_core::engine::{AgentEngine, CompactionSettings, PiRpcEngine, ThreadContextSnapshot};
use muniment_core::journal::reducer::ChatProjector;
#[cfg(any(unix, target_os = "windows"))]
use muniment_core::journal::thread_mutation::{
    append_thread_delete_now, append_thread_rename_now, create_thread_now, ThreadMutationError,
};
use muniment_core::journal::Provenance;
use muniment_core::memory_index::ModelMemoryCapability;
use muniment_core::memory_runtime::ApplicationMemoryRuntime;
#[cfg(any(unix, target_os = "windows"))]
use muniment_core::permission_gate::{ChatPermissionAnswer, PermissionMode};
use muniment_core::pi_execution::PiProcesses;
use muniment_core::run_events::{ChatStorage, SharedStorage};
use muniment_core::run_preparation::{
    append_prepared_run_persistence_failure, prepare_opened_run_with_prompt_storage,
    OpenSelectedFile, SessionThreadStart,
};
#[cfg(any(unix, target_os = "windows"))]
use muniment_core::run_start::{
    accepted_time_now, new_run_id, AttachPromptAccepted, AttachResumeAccepted, RunAttachBoundaries,
};
use muniment_core::run_start::{ActiveRun, RunStartBoundaries, RunStartError, RunStartLaunch};
use muniment_core::serde_json;
use muniment_core::session_thread::SessionThread;
use muniment_core::sidecar::pi_install::{PiArtifactDescriptor, PI_SELECTED_ARTIFACT};

use crate::host::{LinkOpenError, LinkOpener, RuntimeHost};
use crate::models::ModelService;
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
    active: RunRegistry,
    profile_directory: PathBuf,
    config_directory: PathBuf,
    runtime: PiProcesses,
    memory_runtime: Arc<ApplicationMemoryRuntime>,
    runtime_activity: RuntimeActivityRegistry,
    host: RuntimeHost,
    session_thread: Arc<SessionThread>,
    approval: SignedWorkspaceApproval,
    companion_registry: CompanionRegistry,
    #[cfg(any(unix, target_os = "windows"))]
    approvals: ApprovalCoordinator,
    sign_in_running: Arc<AtomicBool>,
    browser_opener: Arc<LinkOpener>,
    chat_events: RuntimeChatEventBroadcast,
    pi_artifact: Option<PiArtifactDescriptor>,
    records: Arc<service::RecordRegistry>,
    contexts: ThreadContexts,
    models: Arc<ModelService>,
    swarm: Option<muniment_core::swarm::Swarm>,
    /// Whether an action on another member's item goes to that member. An
    /// action that member forwarded here runs here.
    routes_to_owners: bool,
}

impl RuntimeAttachBoundaries {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        storage: SharedStorage,
        active: RunRegistry,
        profile_directory: PathBuf,
        config_directory: PathBuf,
        runtime: PiProcesses,
        memory_runtime: Arc<ApplicationMemoryRuntime>,
        runtime_activity: RuntimeActivityRegistry,
        host: RuntimeHost,
        approval: SignedWorkspaceApproval,
        session_thread: Arc<SessionThread>,
        companion_registry: CompanionRegistry,
    ) -> Self {
        let models = Arc::new(ModelService::new(
            &profile_directory,
            Arc::clone(&host.sign_in_page),
        ));
        Self::new_with_sign_in(
            storage,
            active,
            profile_directory,
            config_directory,
            runtime,
            memory_runtime,
            runtime_activity,
            host,
            approval.clone(),
            session_thread,
            companion_registry,
            #[cfg(any(unix, target_os = "windows"))]
            ApprovalCoordinator::default(),
            Arc::new(AtomicBool::new(false)),
            RuntimeChatEventBroadcast::new(approval),
            models,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_with_sign_in(
        storage: SharedStorage,
        active: RunRegistry,
        profile_directory: PathBuf,
        config_directory: PathBuf,
        runtime: PiProcesses,
        memory_runtime: Arc<ApplicationMemoryRuntime>,
        runtime_activity: RuntimeActivityRegistry,
        host: RuntimeHost,
        approval: SignedWorkspaceApproval,
        session_thread: Arc<SessionThread>,
        companion_registry: CompanionRegistry,
        #[cfg(any(unix, target_os = "windows"))] approvals: ApprovalCoordinator,
        sign_in_running: Arc<AtomicBool>,
        chat_events: RuntimeChatEventBroadcast,
        models: Arc<ModelService>,
    ) -> Self {
        let chat_events = chat_events.with_config_directory(config_directory.clone());
        let records = Arc::new(service::RecordRegistry::with_host(
            &profile_directory,
            host.clone(),
        ));
        Self {
            storage,
            active,
            profile_directory,
            config_directory,
            runtime,
            memory_runtime,
            runtime_activity,
            host,
            session_thread,
            approval,
            companion_registry,
            #[cfg(any(unix, target_os = "windows"))]
            approvals,
            sign_in_running,
            browser_opener: Arc::new(sign_in_link_opener(chat_events.clone(), open_browser)),
            chat_events,
            pi_artifact: None,
            records,
            contexts: ThreadContexts::new(),
            models,
            swarm: None,
            routes_to_owners: true,
        }
    }

    /// Answers the permission operations: the modes and settings from the
    /// profile, and a thread's mode and plan from the journal and its live
    /// run.
    #[cfg(any(unix, target_os = "windows"))]
    fn permission_operation(
        &self,
        operation: muniment_core::attach::Operation,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, ProtocolError> {
        use muniment_core::attach::Operation;
        if matches!(
            operation,
            Operation::PermissionModes
                | Operation::PermissionSettings
                | Operation::PermissionSettingsSave
        ) {
            return muniment_core::permission_gate::settings::operation(
                &self.profile_directory,
                operation.as_str(),
                body,
            )
            .map_err(ProtocolError::invalid_request_with_reason);
        }
        let replica_of = |thread: &str| {
            self.swarm
                .as_ref()
                .and_then(|swarm| swarm.is_replica_thread(thread))
        };
        let scope = muniment_core::permission_gate::threads::ThreadScope {
            profile: &self.profile_directory,
            storage: &self.storage,
            active: &self.active,
            provenance: service::runtime_provenance(self.host.runtime_version),
            replica_of: &replica_of,
        };
        muniment_core::permission_gate::threads::operation(&scope, operation.as_str(), body)
            .map_err(ProtocolError::invalid_request_with_reason)
    }

    /// Answers swarm operations through `swarm`.
    pub fn with_swarm(mut self, swarm: muniment_core::swarm::Swarm) -> Self {
        self.swarm = Some(swarm);
        self
    }

    /// Runs every action here, as for an action another member forwarded.
    pub(crate) fn without_routing(mut self) -> Self {
        self.routes_to_owners = false;
        self
    }

    #[cfg(any(unix, target_os = "windows"))]
    fn swarm(&self) -> Result<&muniment_core::swarm::Swarm, ProtocolError> {
        self.swarm
            .as_ref()
            .ok_or_else(ProtocolError::unsupported_operation)
    }

    /// The swarm member that runs a thread, and whether it is another one.
    fn thread_owner(&self, thread_id: &str) -> (Option<String>, bool) {
        let Some(swarm) = &self.swarm else {
            return (None, false);
        };
        let owner = swarm.thread_owner(thread_id);
        let replica = owner.is_some() && owner != swarm.runtime_id();
        (owner, replica)
    }

    /// Replaces the production browser opener for tests and alternate hosts.
    pub fn with_browser_opener(mut self, browser_opener: Arc<LinkOpener>) -> Self {
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

    /// The engine adapter for this profile's conversations and settings.
    #[cfg(any(unix, target_os = "windows"))]
    fn engine(&self) -> PiRpcEngine {
        let profile = muniment_core::chat_profile::ChatProfile::new(&self.profile_directory);
        PiRpcEngine::new(Some(profile.pi_session_root())).with_agent_directory(
            muniment_core::state_root::agent_directory(&self.profile_directory),
        )
    }

    /// Returns the approval state shared with the attach listener. Local mode
    /// has no cloud grant to record a workspace from, so the local workspace
    /// stands recorded whenever the marker is present.
    pub fn signed_workspace_approval(&self) -> SignedWorkspaceApproval {
        record_local_workspace_approval(&self.approval, &self.config_directory);
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

/// Records the local workspace as the owner's approval when local mode is on
/// and no cloud workspace is recorded, so a companion pairing reaches the
/// presenter instead of a denial before it.
pub(crate) fn record_local_workspace_approval(
    approval: &SignedWorkspaceApproval,
    config_directory: &Path,
) {
    if approval.approval().is_none() && muniment_core::local_mode::is_local_mode(config_directory) {
        approval.record(ChatGrant::local().workspace);
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
        !self.active.is_empty()
    }

    fn admit_run(&self, thread: Option<&str>) -> Result<(), RunStartError> {
        self.active
            .admits(thread)
            .map_err(RunStartError::InvalidRequest)
    }

    fn bind_run_thread(&self, run_id: &str, thread: &str) -> Result<(), RunStartError> {
        self.active
            .bind_thread(run_id, thread)
            .map_err(RunStartError::InvalidRequest)
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
        self.host
            .account
            .session(&self.runtime_activity)
            .map_err(RunStartError::Unauthorized)?
            .tokens
            .ok_or_else(|| RunStartError::Unauthorized("Sign in to send a message.".into()))
    }

    fn configure_run(
        &self,
        run_id: &str,
        _prompt: &str,
        tokens: &TokenSet,
        requested_workspace: Option<&str>,
    ) -> Result<ChatGrant, RunStartError> {
        if self.local_mode() {
            let grant = ChatGrant::local();
            self.approval.record(grant.workspace.clone());
            return Ok(grant);
        }
        let grant = service::configure_run(
            self.host.chat_grants.as_ref(),
            &tokens.access_token,
            requested_workspace,
        )
        .map_err(|error| match error {
            ConfigureRunError::Grant(error @ FetchGrantError::NotEntitled { .. }) => {
                eprintln!(
                    "muniment-runtime: grant refused run_id={run_id:?} code=chat_not_entitled"
                );
                RunStartError::Unauthorized(error.into_message())
            }
            ConfigureRunError::Grant(FetchGrantError::Unauthorized) => {
                RunStartError::Unauthorized("The capability is not authorized.".into())
            }
            ConfigureRunError::Grant(FetchGrantError::Unavailable) => {
                RunStartError::Persistence("Chat configuration is temporarily unavailable.".into())
            }
            ConfigureRunError::Grant(FetchGrantError::InvalidResponse) => {
                RunStartError::Persistence("The chat configuration response was invalid.".into())
            }
            ConfigureRunError::Unauthorized => {
                RunStartError::Unauthorized("The capability is not authorized.".into())
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
            self.host.runtime_version,
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
            self.host.runtime_version,
        )
        .map_err(|error| persistence_error("journal", error))
    }

    fn cancel_run(&self, workspace: &str, run_id: &str) -> Result<(), RunStartError> {
        service::cancel_run(self.active.clone(), workspace.to_owned(), run_id.to_owned())
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
        let runtime = self.runtime.slot(&thread_id);
        let runtime_activity = self.runtime_activity.clone();
        let memory_runtime = Arc::clone(&self.memory_runtime);
        let active = self.active.clone();
        let chat_events = self.chat_events.clone();
        let pi_artifact = self.pi_artifact.unwrap_or(PI_SELECTED_ARTIFACT);
        let host = self.host.clone();
        let earlier_models = earlier_thread_models(&storage, &thread_id, &launch.run_id);
        let contexts = self.contexts.clone();
        let context_thread = thread_id.clone();
        std::thread::spawn(move || {
            // The runtime's storage handle keeps this journal's coordination
            // live and validated, so this open skips the full validation scan
            // and costs one connection. Retrieval then runs off the storage lock.
            let project_context = {
                muniment_core::journal::RunJournal::open(profile_directory.join("runs.sqlite3"))
                    .ok()
                    .and_then(|mut journal| {
                        muniment_core::project_context::retrieve(
                            &profile_directory,
                            &mut journal,
                            &thread_id,
                            &launch.grant.workspace,
                            launch.tokens.subject.as_deref(),
                            &launch.prompt,
                        )
                        .ok()
                    })
                    .unwrap_or_default()
            };
            muniment_core::chat_coordinate::coordinate(
                RuntimeChatEventSink::new(
                    &profile_directory,
                    chat_events,
                    Arc::clone(&memory_runtime),
                    thread_id,
                    launch.grant.workspace.clone(),
                )
                .with_host(&host)
                .with_pi_artifact(pi_artifact)
                .with_earlier_models(earlier_models)
                .with_project_context(project_context)
                .with_run_id(&launch.run_id),
                storage,
                runtime,
                runtime_activity,
                Arc::clone(&memory_runtime),
                launch.run_id.clone(),
                launch.prompt,
                launch.thinking_level,
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
            contexts.invalidate(&context_thread);
        });
    }
}

/// The models that answered this thread's earlier runs, from their receipts,
/// oldest first. A run without a receipt, or one that cannot be read, adds
/// nothing: the facts never guess.
fn earlier_thread_models(
    storage: &SharedStorage,
    thread_id: &str,
    current_run_id: &str,
) -> Vec<String> {
    const RUNS_READ: usize = 100;
    let Ok(mut storage) = storage.lock() else {
        return Vec::new();
    };
    storage
        .journal
        .thread_receipt_models(thread_id, current_run_id, RUNS_READ)
        .unwrap_or_default()
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
impl RuntimeAttachBoundaries {
    /// Resumes an interrupted run, and its `run.resumed` event carries
    /// `notice` into the thread.
    pub fn resume_run_with_notice(
        &self,
        workspace: &str,
        run_id: &str,
        notice: Option<String>,
    ) -> Result<AttachResumeAccepted, RunStartError> {
        let workspace = if self.local_mode() {
            "local"
        } else {
            workspace
        };
        let (belongs_to_workspace, thread) = {
            let storage = self
                .storage
                .lock()
                .map_err(|error| persistence_error("lock", error.to_string()))?;
            (
                storage
                    .journal
                    .run_belongs_to_workspace(run_id, workspace)
                    .map_err(|error| persistence_error("journal", error))?,
                storage
                    .journal
                    .run_thread_id(run_id)
                    .map_err(|error| persistence_error("journal", error))?,
            )
        };
        if !belongs_to_workspace {
            return Err(RunStartError::InvalidRequest(
                "The run was not found or is inaccessible.".into(),
            ));
        }
        self.admit_run(thread.as_deref())?;
        let tokens = self.fresh_tokens()?;
        let grant = self.configure_run(run_id, "", &tokens, Some(workspace))?;
        service::resume_run_with_notice(
            &self.host,
            &self.profile_directory,
            Arc::clone(&self.storage),
            self.runtime.clone(),
            &self.runtime_activity,
            &self.config_directory,
            run_id.to_owned(),
            tokens.access_token,
            tokens.subject,
            grant,
            self.active.clone(),
            RuntimeChatEventTarget::Broadcast(self.chat_events.clone()),
            self.pi_artifact,
            notice,
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
}

#[cfg(any(unix, target_os = "windows"))]
impl RuntimeAttachBoundaries {
    /// The mode a thread's first message starts it in when the message picks
    /// none: the default thread mode, for a new thread or one created for
    /// this message. An agent's thread follows its agent, and a thread with
    /// runs or a mode keeps what it has.
    fn first_message_mode(&self, thread: Option<&str>) -> Option<PermissionMode> {
        let default =
            || muniment_core::permission_gate::settings::new_thread_mode(&self.profile_directory);
        let Some(thread) = thread else {
            return Some(default());
        };
        if !matches!(
            muniment_core::agents::thread_agent(&self.profile_directory, thread),
            Ok(None)
        ) {
            return None;
        }
        let mut storage = self.storage.lock().ok()?;
        let unstarted = storage
            .journal
            .thread_run_ids(thread, 1, None)
            .ok()?
            .run_ids
            .is_empty();
        let unset = storage
            .journal
            .thread_permission_mode(thread)
            .ok()?
            .is_none();
        (unstarted && unset).then(default)
    }

    /// Accepts a prompt as the run `run_id` and drives it on its own thread.
    /// The agent run queue names the run, so its record and the journal share
    /// one id. `permission_mode` becomes the thread's mode before the run
    /// starts.
    #[allow(clippy::too_many_arguments)]
    pub fn submit_run_with_id(
        &self,
        run_id: String,
        workspace: &str,
        text: String,
        files: Vec<SelectedFile>,
        thread_id: Option<String>,
        thinking_level: Option<String>,
        permission_mode: Option<PermissionMode>,
    ) -> Result<AttachPromptAccepted, RunStartError> {
        let prompt = text.trim().to_owned();
        if prompt.is_empty() {
            return Err(RunStartError::InvalidRequest(
                "Enter a message before sending.".into(),
            ));
        }
        // The primary of a replicated agent or group thread writes its runs.
        if let (Some(thread), Some(swarm)) = (thread_id.as_deref(), &self.swarm) {
            if swarm.is_replica_thread(thread).is_some() {
                return Err(RunStartError::InvalidRequest(
                    "Another machine runs this thread. Post the message with thread.post.".into(),
                ));
            }
        }
        if let Some(reason) = permission_mode.and_then(|mode| {
            muniment_core::permission_gate::settings::load(&self.profile_directory)
                .unavailable(mode)
        }) {
            return Err(RunStartError::InvalidRequest(reason.into()));
        }
        let permission_mode =
            permission_mode.or_else(|| self.first_message_mode(thread_id.as_deref()));
        self.admit_run(thread_id.as_deref())?;
        let tokens = self.fresh_tokens()?;
        let grant = self.configure_run(
            &run_id,
            &prompt,
            &tokens,
            (!workspace.is_empty()).then_some(workspace),
        )?;
        let files = open_selected_files(files)?;
        let (accepted, launch) = service::accept_prompt(
            &self.host,
            &self.profile_directory,
            Arc::clone(&self.storage),
            self.runtime.clone(),
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
            self.active.clone(),
            RuntimeChatEventTarget::Broadcast(self.chat_events.clone()),
            self.pi_artifact,
        )
        .map_err(run_service_error)?;
        let launch = launch
            .with_permission_mode(permission_mode)
            .map_err(RunStartError::Persistence)?;
        let contexts = self.contexts.clone();
        let context_thread = accepted.thread_id.clone();
        std::thread::spawn(move || {
            service::drive_prompt(launch.with_thinking_level(thinking_level));
            contexts.invalidate(&context_thread);
        });
        Ok(AttachPromptAccepted {
            run_id: accepted.run_id,
            thread_id: accepted.thread_id,
            attachments: accepted.attachments,
            committed_seq: accepted.committed_seq,
            accepted_at: accepted.accepted_at,
        })
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
        thinking_level: Option<String>,
        permission_mode: Option<PermissionMode>,
    ) -> Result<AttachPromptAccepted, RunStartError> {
        self.submit_run_with_id(
            new_run_id(),
            workspace,
            text,
            files,
            thread_id,
            thinking_level,
            permission_mode,
        )
    }

    fn resume_run(
        &self,
        workspace: &str,
        run_id: &str,
    ) -> Result<AttachResumeAccepted, RunStartError> {
        self.resume_run_with_notice(workspace, run_id, None)
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
            self.active.clone(),
            ChatQueueRequest {
                run_id: run_id.to_owned(),
                workspace: Some(workspace.to_owned()),
                delivery,
                message: message.to_owned(),
            },
        )
        .map_err(RunStartError::InvalidRequest)
    }

    fn thread_context_usage(
        &self,
        workspace: &str,
        thread_id: &str,
    ) -> Result<ThreadContextSnapshot, ProtocolError> {
        let tokens = self
            .fresh_tokens()
            .map_err(|error| error.protocol_error())?;
        let local = self.local_mode();
        let workspace = if local || workspace.is_empty() {
            ChatGrant::local().workspace
        } else {
            workspace.to_owned()
        };
        let app = RuntimeChatEventSink::new(
            &self.profile_directory,
            self.chat_events.clone(),
            Arc::clone(&self.memory_runtime),
            thread_id.to_owned(),
            workspace.clone(),
        )
        .with_host(&self.host)
        .with_pi_artifact(self.pi_artifact.unwrap_or(PI_SELECTED_ARTIFACT));
        let chat_grants = Arc::clone(&self.host.chat_grants);
        let storage = Arc::clone(&self.storage);
        let processes = self.runtime.clone();
        let active = self.active.clone();
        let thread = thread_id.to_owned();
        Ok(self.contexts.read(thread_id, move || {
            muniment_core::pi_execution::thread_context_usage(
                &app,
                &storage,
                &processes,
                &active,
                &thread,
                tokens.subject.as_deref(),
                || {
                    if local {
                        return Ok(ChatGrant::local());
                    }
                    service::configure_run(
                        chat_grants.as_ref(),
                        &tokens.access_token,
                        Some(&workspace),
                    )
                    .map_err(|_| "Chat configuration is temporarily unavailable.".to_string())
                },
            )
        }))
    }

    fn compact_thread(
        &self,
        workspace: &str,
        thread_id: &str,
        instructions: Option<&str>,
    ) -> Result<String, ProtocolError> {
        if self.contexts.compacting(thread_id) {
            return Err(ProtocolError::invalid_request_with_reason(
                "This thread is already compacting.",
            ));
        }
        if self.active.thread_run(thread_id).is_some() {
            return Err(ProtocolError::invalid_request_with_reason(
                "This thread is replying. Compact it after the reply finishes.",
            ));
        }
        self.active
            .admits(Some(thread_id))
            .map_err(ProtocolError::invalid_request_with_reason)?;
        let tokens = self
            .fresh_tokens()
            .map_err(|error| error.protocol_error())?;
        let session_root = muniment_core::chat_profile::ChatProfile::new(&self.profile_directory)
            .pi_session_root();
        match muniment_core::pi_execution::thread_session(
            &self.storage,
            thread_id,
            tokens.subject.as_deref(),
            &session_root,
        ) {
            Ok(Some(_)) => {}
            Ok(None) => {
                return Err(ProtocolError::invalid_request_with_reason(
                    "This thread has no conversation to compact yet.",
                ))
            }
            Err(error) => return Err(ProtocolError::invalid_request_with_reason(error)),
        }
        let run_id = new_run_id();
        let grant = self
            .configure_run(
                &run_id,
                "",
                &tokens,
                (!workspace.is_empty()).then_some(workspace),
            )
            .map_err(|error| error.protocol_error())?;
        self.contexts
            .begin_compaction(thread_id, &run_id)
            .map_err(ProtocolError::invalid_request_with_reason)?;
        let launch = service::accept_compaction(
            &self.host,
            &self.profile_directory,
            Arc::clone(&self.storage),
            self.runtime.clone(),
            &self.runtime_activity,
            &self.config_directory,
            run_id.clone(),
            thread_id.to_owned(),
            instructions.map(str::to_owned),
            &self.session_thread,
            tokens.access_token,
            tokens.subject,
            grant,
            self.active.clone(),
            RuntimeChatEventTarget::Broadcast(self.chat_events.clone()),
            self.pi_artifact,
        )
        .map_err(|error| {
            self.contexts
                .finish_compaction(thread_id, &run_id, Err(error.clone()));
            ProtocolError::invalid_request_with_reason(error)
        })?;
        let contexts = self.contexts.clone();
        let thread = thread_id.to_owned();
        let started = run_id.clone();
        std::thread::spawn(move || {
            let result = service::drive_compaction(launch);
            contexts.finish_compaction(&thread, &started, result);
        });
        Ok(run_id)
    }

    fn context_settings(&self) -> Result<CompactionSettings, ProtocolError> {
        self.engine()
            .compaction_settings()
            .map_err(ProtocolError::persistence_failed_with_reason)
    }

    fn save_context_settings(
        &self,
        settings: CompactionSettings,
    ) -> Result<CompactionSettings, ProtocolError> {
        settings
            .validate()
            .map_err(ProtocolError::invalid_request_with_reason)?;
        self.engine()
            .save_compaction_settings(settings)
            .map_err(ProtocolError::persistence_failed_with_reason)
    }

    fn session_status(&self) -> Result<muniment_core::account::AuthStatus, ProtocolError> {
        // Local mode holds no cloud session, so its status read never opens the keychain.
        if self.local_mode() {
            return Ok(muniment_core::account::AuthStatus {
                signed_in: false,
                subject: None,
                expires_at: None,
            });
        }
        self.host.account.session_status()
    }

    fn entitlement_snapshot(&self) -> Result<EntitlementSnapshotResult, ProtocolError> {
        if self.local_mode() {
            return Err(ProtocolError::persistence_failed());
        }
        self.host
            .account
            .entitlement_snapshot(&self.runtime_activity)
    }

    fn sign_in(
        &self,
        _provenance: Provenance,
    ) -> Result<muniment_core::account::AuthStatus, ProtocolError> {
        // The desktop leaves local mode before requesting sign-in. Returning to
        // local mode cancels the browser wait without another socket request.
        let _permit = SignInPermit::acquire(Arc::clone(&self.sign_in_running))
            .ok_or_else(ProtocolError::invalid_request)?;
        self.host.account.sign_in(
            &|| !self.local_mode(),
            self.browser_opener.as_ref(),
            &self.runtime_activity,
        )
    }

    fn sign_out(
        &self,
        _provenance: Provenance,
    ) -> Result<muniment_core::account::AuthStatus, ProtocolError> {
        self.clear_workspace();
        // A companion approved under this account keeps no live connection past sign-out.
        let revoked = self.companion_registry.revoke_live_connections();
        muniment_core::runtime_eprintln!(
            "muniment-runtime: sign-out revoked live attach connections count={revoked}"
        );
        self.host.account.sign_out(&self.runtime_activity)
    }

    fn approval_subject(&self) -> Option<String> {
        if self.local_mode() {
            return Some(muniment_core::attach::LOCAL_APPROVAL_SUBJECT.to_owned());
        }
        // The session read names the account and organization that approve now.
        let snapshot = self
            .host
            .account
            .session(&self.runtime_activity)
            .ok()?
            .entitlement_snapshot?;
        muniment_core::attach::account_approval_subject(&snapshot.org_id, &snapshot.user_id)
    }

    fn cloud_session(&self) -> Result<muniment_core::shared_models::CloudSession, ProtocolError> {
        crate::host::cloud_session(&self.host, &self.runtime_activity).ok_or_else(|| {
            ProtocolError::invalid_request_with_reason("Sign in to MuniCloud first.")
        })
    }

    fn list_devices(&self) -> Result<muniment_core::account::NativeDeviceList, ProtocolError> {
        let access_token = self
            .fresh_tokens()
            .map_err(|error| error.protocol_error())?
            .access_token;
        self.host.account.list_devices(&access_token)
    }

    fn pairing_status(&self) -> Result<muniment_core::account::PairingStatusView, ProtocolError> {
        let access_token = self
            .fresh_tokens()
            .map_err(|error| error.protocol_error())?
            .access_token;
        self.host
            .account
            .pairing_status(&access_token, &self.profile_directory)
    }

    fn create_pairing_challenge(
        &self,
        _provenance: Provenance,
    ) -> Result<muniment_core::account::PairingChallengeView, ProtocolError> {
        let access_token = self
            .fresh_tokens()
            .map_err(|error| error.protocol_error())?
            .access_token;
        self.host.account.create_pairing_challenge(&access_token)
    }

    fn revoke_pairing(
        &self,
        pair_id: muniment_core::account::PairId,
        _provenance: Provenance,
    ) -> Result<muniment_core::account::PairingRevokeView, ProtocolError> {
        let access_token = self
            .fresh_tokens()
            .map_err(|error| error.protocol_error())?
            .access_token;
        self.host
            .account
            .revoke_pairing(&access_token, pair_id, &self.profile_directory)
    }

    fn list_companions(&self) -> Result<Vec<CompanionRecord>, ProtocolError> {
        service::list_companions(&self.companion_registry)
    }

    fn revoke_companion(&self, client_identity: &str) -> Result<(), ProtocolError> {
        service::revoke_companion(&self.companion_registry, client_identity)
    }

    fn list_companies(&self) -> Result<Vec<muniment_core::record::CompanySummary>, ProtocolError> {
        service::list_companies(&self.profile_directory)
    }

    fn create_company(
        &self,
        name: &str,
    ) -> Result<muniment_core::record::CompanySummary, ProtocolError> {
        service::create_company(&self.profile_directory, name)
    }

    fn select_company(
        &self,
        company_id: &str,
    ) -> Result<muniment_core::record::CompanySummary, ProtocolError> {
        service::select_company(&self.profile_directory, company_id)
    }

    fn rename_company(
        &self,
        company_id: &str,
        name: &str,
    ) -> Result<muniment_core::record::CompanySummary, ProtocolError> {
        service::rename_company(&self.profile_directory, company_id, name)
    }

    fn delete_company(
        &self,
        company_id: &str,
    ) -> Result<muniment_core::record::CompanySummary, ProtocolError> {
        self.records.close(company_id);
        service::delete_company(&self.profile_directory, company_id)
    }

    fn record_sql(
        &self,
        actor: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, ProtocolError> {
        self.records.sql(actor, body)
    }

    fn record_propose(
        &self,
        actor: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, ProtocolError> {
        self.records.propose(actor, body)
    }

    fn record_commit(
        &self,
        actor: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, ProtocolError> {
        self.records.commit(actor, body)
    }

    fn record_kinds(
        &self,
        actor: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, ProtocolError> {
        self.records.kinds(actor, body)
    }

    fn record_query(
        &self,
        actor: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, ProtocolError> {
        self.records.query(actor, body)
    }

    fn record_entity(
        &self,
        actor: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, ProtocolError> {
        self.records.entity(actor, body)
    }

    fn record_report(
        &self,
        actor: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, ProtocolError> {
        self.records.report(actor, body)
    }

    fn reader_describe(
        &self,
        actor: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, ProtocolError> {
        self.records.reader_describe(actor, body)
    }

    fn reader_run(
        &self,
        actor: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, ProtocolError> {
        self.records.reader_run(actor, body)
    }

    fn reader_queue(
        &self,
        actor: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, ProtocolError> {
        self.records.reader_queue(actor, body)
    }

    fn reader_objects(
        &self,
        actor: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, ProtocolError> {
        self.records.reader_objects(actor, body)
    }

    fn reader_connect(
        &self,
        actor: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, ProtocolError> {
        self.records.reader_connect(actor, body)
    }

    fn list_threads(
        &self,
        workspace: &str,
        request: ThreadListRequest,
    ) -> Result<ThreadListPage, ProtocolError> {
        let mut page = {
            let mut storage = self
                .storage
                .lock()
                .map_err(|_| ProtocolError::persistence_failed())?;
            ThreadListService::list_threads(&mut storage.journal, workspace, request)?
        };
        for thread in &mut page.threads {
            (thread.owner_runtime, thread.replica) = self.thread_owner(&thread.thread_id);
        }
        Ok(page)
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
        let mut page = service::thread_summaries(
            Arc::clone(&self.storage),
            subject,
            usize::from(request.limit),
            request.cursor,
        )
        .map_err(ProtocolError::persistence_failed_with_reason)?;
        for summary in &mut page.summaries {
            (summary.owner_runtime, summary.replica) = self.thread_owner(&summary.thread_id);
        }
        Ok(page)
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
        if let Some(swarm) = &self.swarm {
            swarm.want_thread_blobs(&request.thread_id);
        }
        service::thread_page(
            &self.profile_directory,
            Arc::clone(&self.storage),
            &self.active,
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
        provenance.actor_id = self
            .fresh_tokens()
            .map_err(|error| error.protocol_error())?
            .subject;
        provenance.source = "muniment-runtime".into();
        provenance.source_version = self.host.runtime_version.into();
        let mut storage = self.storage.lock().map_err(|error| {
            persistence_error("lock", error.to_string()).desktop_protocol_error()
        })?;
        // The desktop can connect before its first grant records a workspace.
        // Cloud and local chat grants both start in the local workspace.
        let default_workspace = ChatGrant::local().workspace;
        let workspace = if workspace.is_empty() {
            &default_workspace
        } else {
            workspace
        };
        create_thread_now(&mut storage.journal, workspace, provenance)
            .map_err(|error| persistence_error("journal", error).desktop_protocol_error())
    }

    fn create_thread_with_id(
        &self,
        thread_id: &str,
        workspace: &str,
        mut provenance: Provenance,
    ) -> Result<String, ProtocolError> {
        provenance.actor_id = self
            .fresh_tokens()
            .map_err(|error| error.protocol_error())?
            .subject;
        provenance.source = "muniment-runtime".into();
        provenance.source_version = self.host.runtime_version.into();
        let mut storage = self.storage.lock().map_err(|error| {
            persistence_error("lock", error.to_string()).desktop_protocol_error()
        })?;
        let default_workspace = ChatGrant::local().workspace;
        let workspace = if workspace.is_empty() {
            &default_workspace
        } else {
            workspace
        };
        let now = muniment_core::chrono::Utc::now()
            .to_rfc3339_opts(muniment_core::chrono::SecondsFormat::AutoSi, true);
        storage
            .journal
            .create_thread_with_id(thread_id, workspace, &now, provenance)
            .map_err(|error| persistence_error("journal", error).desktop_protocol_error())?;
        Ok(thread_id.to_owned())
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
        provenance.source_version = self.host.runtime_version.into();
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
        provenance.source_version = self.host.runtime_version.into();
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
                prompt_accepted: false,
                turn_started: false,
                routing_stage: None,
                prompt_storage_notice: None,
                resume_notice: None,
                failure_reason: Some(cause.to_owned()),
                receipt: None,
                tool_activity: Vec::new(),
                attachments: Vec::new(),
                recalls: Vec::new(),
                applied_diffs: Vec::new(),
                pending_permission: None,
                delta: None,
                seq: None,
            },
        );
    }

    fn subscribe_chat_events(
        &self,
    ) -> Result<muniment_core::run_events::ChatEventSubscription, ProtocolError> {
        Ok(self.chat_events.subscribe())
    }

    fn subscribe_chat_events_after(
        &self,
        cursor: Option<u64>,
    ) -> Result<muniment_core::run_events::ChatEventSubscription, ProtocolError> {
        Ok(self.chat_events.subscribe_after(cursor))
    }

    fn run_detail(
        &self,
        request: muniment_core::attach::desktop_service_message::RunDetailRequest,
    ) -> Result<muniment_core::thread_history::RunDetail, ProtocolError> {
        use muniment_core::thread_history::{run_detail, RunDetailError};
        let subject = self
            .fresh_tokens()
            .map_err(|error| error.protocol_error())?
            .subject;
        let mut storage = self.storage.lock().map_err(|error| {
            persistence_error("lock", error.to_string()).desktop_protocol_error()
        })?;
        let ChatStorage { journal, cas } = &mut *storage;
        run_detail(
            journal,
            cas,
            subject.as_deref(),
            &request.run_id,
            &request.effect_id,
            request.kind,
        )
        .map_err(|error| match error {
            RunDetailError::RunNotOwned | RunDetailError::NotFound => {
                ProtocolError::thread_not_found()
            }
            error => ProtocolError::persistence_failed_with_reason(format!("{error:?}")),
        })
    }

    fn models_request(
        &self,
        request: muniment_core::attach::ModelRequest,
        idempotency_key: Option<&muniment_core::attach::Id>,
    ) -> Result<serde_json::Value, ProtocolError> {
        self.models.request(request, idempotency_key)
    }

    fn subscribe_model_events(&self) -> Result<mpsc::Receiver<serde_json::Value>, ProtocolError> {
        Ok(self.models.subscribe())
    }

    fn agent_operation(
        &self,
        operation: muniment_core::attach::Operation,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, ProtocolError> {
        if operation.is_permission_operation() {
            return self.permission_operation(operation, body);
        }
        if operation.is_preferences_operation() {
            return muniment_core::preferences::operation(
                &self.profile_directory,
                operation.as_str(),
                body,
            )
            .map_err(ProtocolError::invalid_request_with_reason);
        }
        if operation == muniment_core::attach::Operation::UsageSummary {
            return muniment_core::usage_report::operation(
                &self.profile_directory,
                |since| {
                    let mut storage = self
                        .storage
                        .lock()
                        .map_err(|_| "Usage could not be read from the run history.".to_string())?;
                    muniment_core::usage_report::receipts(&mut storage.journal, since)
                },
                body,
                muniment_core::chrono::Utc::now().timestamp_millis(),
            )
            .map_err(ProtocolError::invalid_request_with_reason);
        }
        if operation == muniment_core::attach::Operation::NotificationsList {
            return muniment_core::notifications::operation(
                &self.profile_directory,
                body,
                muniment_core::chrono::Utc::now().timestamp_millis(),
            )
            .map_err(ProtocolError::invalid_request_with_reason);
        }
        if operation == muniment_core::attach::Operation::AgentToolCatalog {
            return muniment_core::agents::operation(
                &self.profile_directory,
                operation.as_str(),
                body,
            )
            .map_err(ProtocolError::invalid_request_with_reason);
        }
        self.require_home()?;
        if operation == muniment_core::attach::Operation::AgentThreads {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Body {
                id: String,
            }
            let body: Body =
                serde_json::from_value(body).map_err(|_| ProtocolError::invalid_request())?;
            let mut storage = self.storage.lock().map_err(|error| {
                persistence_error("lock", error.to_string()).desktop_protocol_error()
            })?;
            return muniment_core::agents::threads(
                &self.profile_directory,
                &mut storage.journal,
                &body.id,
            )
            .map_err(ProtocolError::invalid_request_with_reason);
        }
        let answer =
            muniment_core::agents::operation(&self.profile_directory, operation.as_str(), body)
                .map_err(ProtocolError::invalid_request_with_reason)?;
        // A saved agent's routine proposals are refined by a cheap model call
        // in the background; the deterministic ones are already saved.
        if matches!(operation.as_str(), "agent.save" | "agent.import_commit") {
            if let Some(id) = answer["agent"]["id"].as_str() {
                crate::agent_schedule::refine_proposals(&self.profile_directory, id);
            }
        }
        Ok(answer)
    }

    fn subscribe_agent_runs(&self) -> Result<mpsc::Receiver<serde_json::Value>, ProtocolError> {
        Ok(muniment_core::agent_runs::subscribe(
            &self.profile_directory,
        ))
    }

    fn swarm_operation(
        &self,
        operation: muniment_core::attach::Operation,
        body: serde_json::Value,
        client: Option<&str>,
    ) -> Result<serde_json::Value, ProtocolError> {
        let before = (operation == muniment_core::attach::Operation::ThreadHistoryFetch)
            .then(|| body["before"].as_str().map(str::to_owned));
        let mut answer = self
            .swarm()?
            .operation(operation, body, client)
            .map_err(ProtocolError::invalid_request_with_reason)?;
        // A fetched history that is held answers its page at once.
        if let Some(before) = before {
            if answer["status"] == "held" {
                let thread_id = answer["thread_id"].as_str().unwrap_or_default().to_owned();
                let page = self.thread_history(ThreadOpenRequest {
                    thread_id,
                    limit: 20,
                    cursor: before,
                })?;
                answer["page"] =
                    serde_json::to_value(page).map_err(|_| ProtocolError::persistence_failed())?;
            }
        }
        Ok(answer)
    }

    fn swarm_exchange(
        &self,
        device_id: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, ProtocolError> {
        self.swarm()?
            .exchange(device_id, body)
            .map_err(ProtocolError::invalid_request_with_reason)
    }

    fn subscribe_swarm_events(
        &self,
    ) -> Result<mpsc::Receiver<muniment_core::swarm::ClientEvent>, ProtocolError> {
        Ok(self.swarm()?.subscribe())
    }

    fn note_client_activity(&self) {
        if let Some(swarm) = &self.swarm {
            swarm.note_client_activity();
        }
    }

    fn fleet_route(
        &self,
        operation: muniment_core::attach::Operation,
        body: &serde_json::Value,
        actor: &muniment_core::swarm::FleetActor,
        idempotency_key: Option<&str>,
    ) -> Result<Option<serde_json::Value>, ProtocolError> {
        let Some(swarm) = self.swarm.as_ref().filter(|_| self.routes_to_owners) else {
            return Ok(None);
        };
        swarm
            .route(operation, body, actor, idempotency_key)
            .map_err(ProtocolError::invalid_request_with_reason)
    }

    fn hook_operation(
        &self,
        operation: muniment_core::attach::Operation,
        body: serde_json::Value,
        endpoints: muniment_core::agent_hooks::HookEndpoints,
    ) -> Result<serde_json::Value, ProtocolError> {
        self.require_home()?;
        muniment_core::agent_hooks::operation(
            &self.profile_directory,
            &HookSecretStore(self.host.credentials.as_ref()),
            &endpoints,
            operation.as_str(),
            body,
        )
        .map_err(ProtocolError::invalid_request_with_reason)
    }

    fn receive_webhook(
        &self,
        runtime_id: Option<&str>,
        hook_id: &str,
        headers: &[(String, String)],
        body: &[u8],
    ) -> Result<String, muniment_core::agent_hooks::Refusal> {
        if self.require_home().is_err() {
            return Err(muniment_core::agent_hooks::Refusal::NOT_FOUND);
        }
        muniment_core::agent_hooks::receive(
            &self.profile_directory,
            &HookSecretStore(self.host.credentials.as_ref()),
            &muniment_core::agent_hooks::Delivery {
                hook_id,
                headers,
                body,
            },
            &|agent| muniment_core::agents::runs_routines_here(agent, runtime_id),
        )
        .map(|run| run.id)
    }

    fn queue_attach_permission_answer(
        &self,
        workspace: &str,
        run_id: &str,
        gate_id: &str,
        answer: ChatPermissionAnswer,
    ) -> Result<std::sync::mpsc::Receiver<Option<u64>>, RunStartError> {
        let committed_seq = service::answer_permission(
            self.active.clone(),
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

/// Hook secrets in the runtime's credential store, one entry per hook id.
#[cfg(any(unix, target_os = "windows"))]
struct HookSecretStore<'a>(&'a dyn crate::host::CredentialStore);

#[cfg(any(unix, target_os = "windows"))]
impl muniment_core::agent_hooks::HookSecrets for HookSecretStore<'_> {
    fn read(&self, hook_id: &str) -> Option<String> {
        self.0
            .read(muniment_core::agent_hooks::SECRET_SERVICE, hook_id)
            .filter(|secret| !secret.is_empty())
    }

    fn write(&self, hook_id: &str, secret: &str) -> Result<(), String> {
        self.0
            .write(muniment_core::agent_hooks::SECRET_SERVICE, hook_id, secret)
    }
}

#[cfg(any(unix, target_os = "windows"))]
impl RuntimeAttachBoundaries {
    fn require_home(&self) -> Result<(), ProtocolError> {
        if muniment_core::home::configured_home(&self.profile_directory)
            .ok()
            .flatten()
            .is_none()
        {
            return Err(ProtocolError::invalid_request_with_reason(
                "Choose a Home folder before managing agents.",
            ));
        }
        Ok(())
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

/// The run id and phase of the chat event that carries the sign-in link.
pub const SIGN_IN_LINK_RUN_ID: &str = "sign-in";
pub const SIGN_IN_LINK_PHASE: &str = "sign-in-link";

/// Announces the sign-in link on the chat-event stream before the browser
/// opens, so the desktop shows the link when no browser answers.
fn sign_in_link_opener(
    chat_events: RuntimeChatEventBroadcast,
    open: impl Fn(&str) -> Result<(), LinkOpenError> + Send + Sync,
) -> impl Fn(&str) -> Result<(), LinkOpenError> + Send + Sync {
    move |url: &str| {
        chat_events.announce(sign_in_link_event(url));
        open(url)
    }
}

fn sign_in_link_event(url: &str) -> muniment_core::run_events::ChatEvent {
    muniment_core::run_events::ChatEvent {
        run_id: SIGN_IN_LINK_RUN_ID.to_owned(),
        thread_id: None,
        phase: SIGN_IN_LINK_PHASE.into(),
        text: url.to_owned(),
        prompt_accepted: false,
        turn_started: false,
        routing_stage: None,
        prompt_storage_notice: None,
        resume_notice: None,
        failure_reason: None,
        receipt: None,
        tool_activity: Vec::new(),
        attachments: Vec::new(),
        recalls: Vec::new(),
        applied_diffs: Vec::new(),
        pending_permission: None,
        delta: None,
        seq: None,
    }
}

fn open_browser(url: &str) -> Result<(), LinkOpenError> {
    browser_command(url)
        .spawn()
        .map(drop)
        .map_err(|_| LinkOpenError)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_in_link_reaches_every_subscriber_before_the_browser_opens() {
        let broadcast = RuntimeChatEventBroadcast::default();
        let subscription = broadcast.subscribe();
        let opened = std::sync::Mutex::new(Vec::new());
        let opener = sign_in_link_opener(broadcast.clone(), |url: &str| {
            opened.lock().unwrap().push(url.to_owned());
            Ok(())
        });
        let url = "https://muniment.ai/authorize?state=abc";

        opener(url).unwrap();

        let event = subscription.try_recv().unwrap();
        assert_eq!(event.run_id, SIGN_IN_LINK_RUN_ID);
        assert_eq!(event.phase, SIGN_IN_LINK_PHASE);
        assert_eq!(event.text, url);
        assert_eq!(event.thread_id, None);
        assert_eq!(opened.lock().unwrap().as_slice(), [url.to_owned()]);
    }

    #[test]
    fn sign_in_link_opener_reports_the_browser_failure() {
        let broadcast = RuntimeChatEventBroadcast::default();
        let subscription = broadcast.subscribe();
        let opener = sign_in_link_opener(broadcast.clone(), |_: &str| Err(LinkOpenError));

        assert!(opener("https://muniment.ai/authorize").is_err());
        assert_eq!(
            subscription.try_recv().unwrap().text,
            "https://muniment.ai/authorize"
        );
    }

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
    fn grant_refusal_keeps_the_reason_for_the_desktop_and_redacts_companions() {
        let reason = FetchGrantError::NotEntitled {
            message: "No chat model is currently available for this account.".into(),
        }
        .into_message();
        let error = RunStartError::Unauthorized(reason.clone());
        let desktop = error.desktop_protocol_error();
        assert_eq!(
            desktop.code(),
            muniment_core::attach::ErrorCode::Unauthorized
        );
        assert_eq!(desktop, ProtocolError::unauthorized_with_reason(reason));
        assert_eq!(error.protocol_error(), ProtocolError::unauthorized());
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

    #[test]
    fn local_mode_records_the_local_workspace_approval_once() {
        let root = std::env::temp_dir().join(format!(
            "muniment-local-approval-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let approval = SignedWorkspaceApproval::default();

        record_local_workspace_approval(&approval, &root);
        assert!(approval.approval().is_none());

        std::fs::write(root.join(muniment_core::local_mode::LOCAL_MODE_MARKER), []).unwrap();
        record_local_workspace_approval(&approval, &root);
        assert_eq!(approval.approval().unwrap().workspace, "local");

        approval.record("workspace-a".into());
        record_local_workspace_approval(&approval, &root);
        assert_eq!(approval.approval().unwrap().workspace, "workspace-a");
        std::fs::remove_dir_all(root).unwrap();
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
