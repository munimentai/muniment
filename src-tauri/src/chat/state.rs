use super::resume::protect_prompt;
#[cfg(target_os = "linux")]
use super::run_preparation::desktop_provenance;
use super::run_preparation::{
    fetch_grant, map_fetch_grant_error, prepare_new_run_in_thread_after_validation,
    prepare_new_run_with_session_thread_after_validation, validate_grant,
};
use super::*;

pub(super) fn desktop_pi_workspace(grant: &ChatGrant) -> Result<PathBuf, PiLaunchError> {
    if grant.is_local() {
        std::env::current_dir().map_err(|_| PiLaunchError::UnavailableAgentDirectory)
    } else {
        Ok(PathBuf::from(&grant.workspace))
    }
}

pub(crate) struct TauriChatEventSink<R: tauri::Runtime> {
    app: tauri::AppHandle<R>,
    memory_runtime: Arc<crate::memory::ApplicationMemoryRuntime>,
    workspace: Result<PathBuf, PiLaunchError>,
}

impl<R: tauri::Runtime> TauriChatEventSink<R> {
    pub(super) fn new(
        app: tauri::AppHandle<R>,
        memory_runtime: Arc<crate::memory::ApplicationMemoryRuntime>,
        workspace: Result<PathBuf, PiLaunchError>,
    ) -> Self {
        Self {
            app,
            memory_runtime,
            workspace,
        }
    }
}

impl<R: tauri::Runtime> ChatEventSink for TauriChatEventSink<R> {
    fn provenance(&self) -> (&str, &str) {
        ("muniment-desktop", env!("CARGO_PKG_VERSION"))
    }

    fn deliver(&self, event: ChatEvent) -> Result<(), ()> {
        self.app.emit("chat-event", event).map_err(|_| ())
    }
}

impl<R: tauri::Runtime> PiLaunchBoundaries for TauriChatEventSink<R> {
    fn pi_session_root(&self) -> Result<PathBuf, PiLaunchError> {
        self.app
            .path()
            .app_data_dir()
            .map(|path| ChatProfile::new(path).pi_session_root())
            .map_err(|_| PiLaunchError::UnavailableSessionRoot)
    }

    fn memory_agent_extension_path(&self) -> Option<PathBuf> {
        Some(self.memory_runtime.agent_extension_path())
    }

    fn pi_workspace_directory(&self) -> Result<Option<PathBuf>, PiLaunchError> {
        self.workspace.clone().map(Some)
    }

    fn pi_agent_directory(&self, _workspace: &Path) -> Result<Option<PathBuf>, PiLaunchError> {
        let bundled = self
            .app
            .path()
            .resource_dir()
            .map_err(|_| PiLaunchError::UnavailableAgentDirectory)?
            .join("pi-agent");
        if !bundled.is_dir() {
            return Err(PiLaunchError::UnavailableAgentDirectory);
        }
        let destination = self
            .app
            .path()
            .app_config_dir()
            .map_err(|_| PiLaunchError::UnavailableAgentDirectory)?
            .join("pi-agent");
        muniment_core::pi_launch::prepare_pi_agent_directory(
            &bundled,
            &destination,
            self.workspace.as_deref().map_err(|error| *error)?,
        )
        .map(Some)
    }
}

pub struct ChatState {
    #[cfg(any(unix, target_os = "windows"))]
    pub(crate) storage: DeferredStorage,
    pub(super) active: Arc<Mutex<Option<ActiveRun>>>,
    pub(super) runtime: Arc<Mutex<Option<PiRuntime>>>,
    pub(crate) session_thread: SessionThread,
    pub(crate) runtime_activity: RuntimeActivityRegistry,
    pub(crate) retention_trigger: RetentionTrigger,
}

/// Wakes the desktop retention schedule between its timed checks.
#[derive(Default)]
pub(crate) struct RetentionTrigger(OnceLock<Sender<()>>);

impl RetentionTrigger {
    fn install(&self, trigger: Sender<()>) {
        let _ = self.0.set(trigger);
    }

    /// Asks the schedule to check now. Reports whether the schedule took it.
    pub(crate) fn check_now(&self) -> bool {
        self.0.get().is_some_and(|trigger| trigger.send(()).is_ok())
    }

    /// Builds a trigger that a test reads instead of a schedule.
    #[cfg(test)]
    pub(crate) fn for_test() -> (Self, std::sync::mpsc::Receiver<()>) {
        let (trigger, checks) = channel();
        let installed = Self::default();
        installed.install(trigger);
        (installed, checks)
    }
}

#[cfg(any(unix, target_os = "windows"))]
pub(crate) struct DeferredStorage(pub(super) OnceLock<SharedStorage>);

#[cfg(any(unix, target_os = "windows"))]
impl DeferredStorage {
    fn new() -> Self {
        Self(OnceLock::new())
    }

    pub(crate) fn lock(&self) -> Result<std::sync::MutexGuard<'_, ChatStorage>, String> {
        self.0
            .get()
            .ok_or_else(auth::background_service_error)?
            .lock()
            .map_err(|_| "The chat storage lock is poisoned.".into())
    }

    pub(super) fn get(&self) -> Option<&SharedStorage> {
        self.0.get()
    }

    fn set(&self, storage: SharedStorage) -> Result<(), SharedStorage> {
        self.0.set(storage)
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) struct TauriRunStartBoundaries<R: tauri::Runtime> {
    pub(crate) app: tauri::AppHandle<R>,
    pub(crate) continue_session_thread: bool,
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
impl<R: tauri::Runtime> TauriRunStartBoundaries<R> {
    fn state(&self) -> tauri::State<'_, ChatState> {
        self.app.state::<ChatState>()
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
impl<R: tauri::Runtime> RunAttachBoundaries for TauriRunStartBoundaries<R> {
    #[cfg(target_os = "linux")]
    fn queue_attach_message(
        &self,
        workspace: &str,
        run_id: &str,
        delivery: ChatDelivery,
        message: &str,
    ) -> Result<(), RunStartError> {
        queue_message(
            &self.state().active,
            ChatQueueRequest {
                run_id: run_id.to_owned(),
                workspace: Some(workspace.to_owned()),
                delivery,
                message: message.to_owned(),
            },
        )
        .map_err(RunStartError::InvalidRequest)
    }
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    fn list_threads(
        &self,
        workspace: &str,
        request: ThreadListRequest,
    ) -> Result<ThreadListPage, ProtocolError> {
        let state = self.state();
        let mut storage = state
            .storage
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        ThreadListService::list_threads(&mut storage.journal, workspace, request)
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    fn open_thread(
        &self,
        workspace: &str,
        request: ThreadOpenRequest,
    ) -> Result<ThreadOpenPage, ProtocolError> {
        let state = self.state();
        let mut storage = state
            .storage
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        ThreadListService::open_thread(&mut storage.journal, workspace, request)
    }

    #[cfg(target_os = "linux")]
    fn select_thread(&self, thread_id: &str) -> Result<bool, ProtocolError> {
        let subject = if crate::local_mode::is_active(&self.app)
            .map_err(|_| ProtocolError::persistence_failed())?
        {
            None
        } else {
            auth::fresh_tokens(&self.app.state::<auth::AuthState>(), &self.app)
                .map_err(|_| ProtocolError::unauthorized())?
                .subject
        };
        let state = self.state();
        let mut storage = state
            .storage
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        subject_owns_first_run(&mut storage.journal, thread_id, subject.as_deref())
            .map_err(|_| ProtocolError::thread_not_found())
    }

    #[cfg(target_os = "linux")]
    fn create_thread(
        &self,
        workspace: &str,
        provenance: Provenance,
    ) -> Result<String, ProtocolError> {
        let state = self.state();
        let mut storage = state
            .storage
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        create_thread_now(&mut storage.journal, workspace, provenance)
            .map_err(|_| ProtocolError::persistence_failed())
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    fn stream_run(
        &self,
        workspace: &str,
        run_id: &str,
        after_run_seq: u64,
    ) -> Result<muniment_core::attach::desktop_service_message::RunStreamPage, ProtocolError> {
        let state = self.state();
        let mut storage = state
            .storage
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        ThreadListService::stream_run(&mut storage.journal, workspace, run_id, after_run_seq)
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    fn subscribe_run_commits(
        &self,
        run_id: &str,
    ) -> Result<muniment_core::journal::CommitSubscription, ProtocolError> {
        self.state()
            .storage
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?
            .journal
            .subscribe_commits(run_id)
            .map_err(|_| ProtocolError::persistence_failed())
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    fn queue_attach_permission_answer(
        &self,
        workspace: &str,
        run_id: &str,
        gate_id: &str,
        answer: ChatPermissionAnswer,
    ) -> Result<std::sync::mpsc::Receiver<Option<u64>>, RunStartError> {
        let state = self.state();
        queue_permission_answer_with_commit(
            &state.active,
            Some(workspace),
            run_id.to_owned(),
            gate_id.to_owned(),
            answer,
        )
        .map_err(RunStartError::InvalidRequest)
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
impl<R: tauri::Runtime> RunStartBoundaries for TauriRunStartBoundaries<R> {
    fn mark_active_run(&self) -> RuntimeActivityGuard {
        self.state().runtime_activity.mark_active_run()
    }

    #[cfg(target_os = "linux")]
    fn control_migration(
        &self,
        request: muniment_core::attach::linux::MigrationControlRequest,
        peer_pid: u32,
    ) -> Result<(), ProtocolError> {
        crate::attach_service::control_desktop_migration(&self.app, request, peer_pid)
    }

    fn active_run_exists(&self) -> bool {
        self.state()
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
    }

    fn cancel_run(&self, workspace: &str, run_id: &str) -> Result<(), RunStartError> {
        cancel_active_run(&self.state().active, run_id, Some(workspace))
            .map_err(RunStartError::InvalidRequest)
    }

    fn fresh_tokens(&self) -> Result<TokenSet, RunStartError> {
        if crate::local_mode::is_active(&self.app).map_err(RunStartError::Persistence)? {
            return Ok(TokenSet {
                access_token: String::new(),
                refresh_token: None,
                expires_at: None,
                subject: None,
            });
        }
        auth::fresh_tokens(&self.app.state::<auth::AuthState>(), &self.app)
            .map_err(RunStartError::Unauthorized)
    }

    fn configure_run(
        &self,
        _run_id: &str,
        _prompt: &str,
        tokens: &TokenSet,
        requested_workspace: Option<&str>,
    ) -> Result<ChatGrant, RunStartError> {
        if crate::local_mode::is_active(&self.app).map_err(RunStartError::Persistence)? {
            return Ok(ChatGrant::local());
        }
        let grant = fetch_grant(&tokens.access_token).map_err(map_fetch_grant_error)?;
        validate_grant(&grant).map_err(RunStartError::Persistence)?;
        self.app
            .state::<crate::attach_service::AttachCompanionState>()
            .record_workspace(grant.workspace.clone());
        if !grant_authorizes_workspace(&grant, requested_workspace) {
            return Err(RunStartError::Unauthorized(
                "The capability is not authorized.".into(),
            ));
        }
        Ok(grant)
    }

    #[cfg(target_os = "linux")]
    fn attach_approval(&self) -> Option<muniment_core::attach::Approval> {
        self.app
            .state::<crate::attach_service::AttachCompanionState>()
            .approval()
    }

    fn install_active_run(&self, run: ActiveRun) -> Result<(), RunStartError> {
        install_active_run(&self.state().active, run).map_err(RunStartError::InvalidRequest)
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
        let state = self.state();
        let storage = state.storage().map_err(RunStartError::Persistence)?;
        let result = match thread_id {
            Some(thread_id) => prepare_new_run_in_thread_after_validation(
                storage,
                run_id,
                &grant.workspace,
                tokens.subject.as_deref(),
                files,
                provenance,
                thread_id,
                || protect_prompt(run_id, prompt, tokens.subject.as_deref()),
            ),
            None => prepare_new_run_with_session_thread_after_validation(
                storage,
                SessionThreadStart {
                    tracker: &state.session_thread,
                    continue_existing: self.continue_session_thread,
                },
                run_id,
                &grant.workspace,
                tokens.subject.as_deref(),
                files,
                provenance,
                || protect_prompt(run_id, prompt, tokens.subject.as_deref()),
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
            .map_err(|_| RunStartError::Persistence(attachment_error()))
    }

    fn run_thread_id(&self, run_id: &str) -> Result<String, RunStartError> {
        self.state()
            .storage
            .lock()
            .map_err(|_| RunStartError::Persistence(attachment_error()))?
            .journal
            .run_thread_id(run_id)
            .map_err(|_| RunStartError::Persistence(attachment_error()))?
            .ok_or_else(|| RunStartError::Persistence(attachment_error()))
    }

    fn open_memory_session(
        &self,
        run_id: &str,
        thread_id: &str,
        minimum_cacheable_prefix_characters: usize,
    ) -> Result<(), RunStartError> {
        self.app
            .state::<Arc<crate::memory::ApplicationMemoryRuntime>>()
            .open_session(
                run_id,
                thread_id,
                ModelMemoryCapability {
                    minimum_cacheable_prefix_characters,
                },
            )
            .map_err(|_| RunStartError::Persistence(attachment_error()))
    }

    fn close_memory_session(&self, run_id: &str) {
        self.app
            .state::<Arc<crate::memory::ApplicationMemoryRuntime>>()
            .close_session(run_id);
    }

    fn fail_prepared_run(&self, launch: &RunStartLaunch) -> Result<(), RunStartError> {
        core_run_preparation::append_prepared_run_persistence_failure(
            &mut self
                .state()
                .storage
                .lock()
                .map_err(|_| RunStartError::Persistence(attachment_error()))?
                .journal,
            &launch.run_id,
            launch.prepared.0,
            launch.tokens.subject.as_deref(),
            "muniment-desktop",
            env!("CARGO_PKG_VERSION"),
        )
        .map_err(|_| RunStartError::Persistence(attachment_error()))
    }

    fn clear_active_run(&self, run_id: &str) {
        clear_active_run(&self.state().active, run_id);
    }

    fn launch(&self, launch: RunStartLaunch) {
        let app = self.app.clone();
        let state = self.state();
        let storage = Arc::clone(state.storage().expect("The listener owns no chat storage."));
        let runtime = Arc::clone(&state.runtime);
        let runtime_activity = state.runtime_activity.clone();
        let memory_runtime = Arc::clone(
            self.app
                .state::<Arc<crate::memory::ApplicationMemoryRuntime>>()
                .inner(),
        );
        tauri::async_runtime::spawn_blocking(move || {
            let workspace = desktop_pi_workspace(&launch.grant);
            let sink = TauriChatEventSink::new(app.clone(), Arc::clone(&memory_runtime), workspace);
            coordinate(
                sink,
                storage,
                runtime,
                runtime_activity,
                memory_runtime,
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
            if let Some(state) = app.try_state::<ChatState>() {
                app.state::<Arc<crate::memory::ApplicationMemoryRuntime>>()
                    .close_session(&launch.run_id);
                clear_active_run(&state.active, &launch.run_id);
            }
        });
    }
}

impl ChatState {
    #[cfg(any(unix, target_os = "windows"))]
    pub fn new(runtime_activity: RuntimeActivityRegistry) -> Self {
        Self::new_runtime_owned(runtime_activity)
    }

    #[cfg(any(unix, target_os = "windows"))]
    pub(super) fn new_runtime_owned(runtime_activity: RuntimeActivityRegistry) -> Self {
        Self {
            storage: DeferredStorage::new(),
            active: Arc::new(Mutex::new(None)),
            runtime: Arc::new(Mutex::new(None)),
            session_thread: SessionThread::default(),
            runtime_activity,
            retention_trigger: RetentionTrigger::default(),
        }
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn open_storage<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.storage.get().is_some() {
            return Ok(());
        }
        let directory = app.path().app_data_dir()?;
        let config_directory = app.path().app_config_dir()?;
        let profile = ChatProfile::new(directory);
        let (mut journal, cas) = profile.open_storage()?;
        reconcile_interrupted_runs(&mut journal, &desktop_provenance(None));
        let storage = Arc::new(Mutex::new(ChatStorage { journal, cas }));
        self.storage
            .set(Arc::clone(&storage))
            .map_err(|_| "Chat storage is already open.")?;
        self.retention_trigger
            .install(start_retention_schedule(config_directory, storage));
        Ok(())
    }

    #[cfg(any(unix, target_os = "windows"))]
    pub(crate) fn storage(&self) -> Result<&SharedStorage, String> {
        self.storage
            .get()
            .ok_or_else(auth::background_service_error)
    }
}

/// Starts the schedule and returns the trigger for an immediate check.
pub(super) fn start_retention_schedule(
    config_directory: PathBuf,
    storage: SharedStorage,
) -> Sender<()> {
    let (trigger, checks) = channel();
    std::thread::spawn(move || {
        muniment_core::retention_record::run_recorded_retention_checks(
            &config_directory,
            |interval| match checks.recv_timeout(interval) {
                Ok(()) => {
                    while checks.try_recv().is_ok() {}
                    true
                }
                Err(RecvTimeoutError::Timeout) => true,
                Err(RecvTimeoutError::Disconnected) => false,
            },
            |max_age_seconds| {
                let mut storage = storage.lock().map_err(|_| ())?;
                let ChatStorage { journal, cas } = &mut *storage;
                apply_retention_now_with(journal, Some(cas), max_age_seconds, |deleted_run| {
                    muniment_core::chat_prompt::delete_prompt(
                        &deleted_run.run_id,
                        deleted_run.subject.as_deref(),
                    )
                    .map_err(|_| RetentionError::BeforeDelete)
                })
                .map(|_| ())
                .map_err(|_| ())
            },
        );
    });
    trigger
}
