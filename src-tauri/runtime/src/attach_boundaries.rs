//! Runtime-owned boundaries for desktop attach reads.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use muniment_core::active_run::queue_permission_answer_with_commit;
use muniment_core::attach::linux::{
    EntitlementSnapshotResult, RunStreamPage, ThreadListPage, ThreadListRequest, ThreadListService,
    ThreadOpenPage, ThreadOpenRequest,
};
use muniment_core::attach::ProtocolError;
use muniment_core::attach::{
    RuntimeActivityGuard, RuntimeActivityRegistry, SignedWorkspaceApproval,
};
use muniment_core::auth::{EntitlementSnapshotTracker, NativeDeviceListError, TokenSet};
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
    ActiveRun, RunAttachBoundaries, RunStartBoundaries, RunStartError, RunStartLaunch,
};
use muniment_core::session_thread::SessionThread;

use crate::service::{self, ConfigureRunError};
use crate::RuntimeChatEventSink;

/// Supplies attach reads from runtime-owned state.
pub struct RuntimeAttachBoundaries {
    storage: SharedStorage,
    active: Arc<Mutex<Option<ActiveRun>>>,
    profile_directory: PathBuf,
    #[allow(dead_code)]
    config_directory: PathBuf,
    runtime: Arc<Mutex<Option<PiRuntime>>>,
    memory_runtime: Arc<ApplicationMemoryRuntime>,
    runtime_activity: RuntimeActivityRegistry,
    entitlement_tracker: Arc<EntitlementSnapshotTracker>,
    session_thread: Arc<SessionThread>,
    approval: SignedWorkspaceApproval,
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
        }
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
        std::thread::spawn(move || {
            muniment_core::chat_coordinate::coordinate(
                RuntimeChatEventSink::new(&profile_directory, None, Arc::clone(&memory_runtime)),
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

impl RunAttachBoundaries for RuntimeAttachBoundaries {
    fn session_status(&self) -> Result<muniment_core::auth::AuthStatus, ProtocolError> {
        service::session_status().map_err(|_| ProtocolError::persistence_failed())
    }

    fn entitlement_snapshot(&self) -> Result<EntitlementSnapshotResult, ProtocolError> {
        service::entitlement_snapshot(&self.entitlement_tracker, &self.runtime_activity)
            .map_err(|_| ProtocolError::persistence_failed())
    }

    fn sign_out(
        &self,
        _provenance: Provenance,
    ) -> Result<muniment_core::auth::AuthStatus, ProtocolError> {
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

    fn queue_attach_permission_answer(
        &self,
        workspace: &str,
        run_id: &str,
        gate_id: &str,
        answer: ChatPermissionAnswer,
    ) -> Result<std::sync::mpsc::Receiver<Option<u64>>, RunStartError> {
        queue_permission_answer_with_commit(
            &self.active,
            Some(workspace),
            run_id.to_owned(),
            gate_id.to_owned(),
            answer,
        )
        .map_err(RunStartError::InvalidRequest)
    }
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
