//! Shared state for runtime attach connections.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use muniment_core::attach::{
    ApprovalCoordinator, CompanionRegistry, DesktopAttachService, ProtocolError,
    RuntimeActivityRegistry, SignedWorkspaceApproval,
};
use muniment_core::auth::EntitlementSnapshotTracker;
use muniment_core::memory_runtime::ApplicationMemoryRuntime;
use muniment_core::pi_execution::PiRuntime;
use muniment_core::run_start::ActiveRun;
use muniment_core::session_thread::SessionThread;

use crate::service::{open_companion_registry, open_profile_storage};
use crate::{
    compose_attach_service, installed_desktop_executable, AttachListenerInputs,
    RuntimeAttachBoundaries,
};

/// Owns the state shared by all runtime attach connections.
pub struct RuntimeAttachState {
    profile_directory: PathBuf,
    config_directory: PathBuf,
    storage: muniment_core::run_events::SharedStorage,
    active: Arc<Mutex<Option<ActiveRun>>>,
    runtime: Arc<Mutex<Option<PiRuntime>>>,
    memory_runtime: Arc<ApplicationMemoryRuntime>,
    runtime_activity: RuntimeActivityRegistry,
    entitlement_tracker: Arc<EntitlementSnapshotTracker>,
    approval: SignedWorkspaceApproval,
    session_thread: Arc<SessionThread>,
    companion_registry: CompanionRegistry,
    approvals: ApprovalCoordinator,
}

impl RuntimeAttachState {
    /// Opens the profile storage and companion registry for the runtime.
    pub fn open(
        profile_directory: impl AsRef<Path>,
        config_directory: impl AsRef<Path>,
    ) -> Result<Self, ProtocolError> {
        let profile_directory = profile_directory.as_ref().to_path_buf();
        let config_directory = config_directory.as_ref().to_path_buf();
        let storage = open_profile_storage(&profile_directory)
            .map_err(|_| ProtocolError::persistence_failed())?;
        let companion_registry = open_companion_registry(&profile_directory)?;

        Ok(Self {
            memory_runtime: Arc::new(ApplicationMemoryRuntime::new(
                config_directory.clone(),
                profile_directory.join("memory"),
            )),
            profile_directory,
            config_directory,
            storage,
            active: Arc::new(Mutex::new(None)),
            runtime: Arc::new(Mutex::new(None)),
            runtime_activity: RuntimeActivityRegistry::new(),
            entitlement_tracker: Arc::new(EntitlementSnapshotTracker::new()),
            approval: SignedWorkspaceApproval::default(),
            session_thread: Arc::new(SessionThread::default()),
            companion_registry,
            approvals: ApprovalCoordinator::default(),
        })
    }

    /// Builds boundaries that share the runtime state.
    pub fn boundaries(&self) -> RuntimeAttachBoundaries {
        RuntimeAttachBoundaries::new(
            Arc::clone(&self.storage),
            Arc::clone(&self.active),
            self.profile_directory.clone(),
            self.config_directory.clone(),
            Arc::clone(&self.runtime),
            Arc::clone(&self.memory_runtime),
            self.runtime_activity.clone(),
            Arc::clone(&self.entitlement_tracker),
            self.approval.clone(),
            Arc::clone(&self.session_thread),
            self.companion_registry.clone(),
        )
    }

    /// Composes a service for one companion connection.
    pub fn attach_service(
        &self,
    ) -> Result<DesktopAttachService<RuntimeAttachBoundaries>, ProtocolError> {
        compose_attach_service(
            self.boundaries(),
            &self.companion_registry,
            &self.profile_directory,
            &self.config_directory,
        )
    }

    /// Builds listener inputs that share the runtime attach state.
    pub fn attach_listener_inputs(&self) -> AttachListenerInputs<'_> {
        AttachListenerInputs {
            companion_registry: &self.companion_registry,
            approval: self.approval.clone(),
            approvals: self.approvals.clone(),
            expected_desktop_executable: installed_desktop_executable(),
        }
    }

    /// Returns the companion registry shared with attach services.
    pub fn companion_registry(&self) -> &CompanionRegistry {
        &self.companion_registry
    }

    /// Returns the workspace approval shared with attach boundaries.
    pub fn signed_workspace_approval(&self) -> SignedWorkspaceApproval {
        self.approval.clone()
    }
}
