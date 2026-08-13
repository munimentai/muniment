//! Workspace and companion service operations.

#[cfg(target_os = "linux")]
use muniment_core::attach::linux::LiveConnectionRegistry;
#[cfg(target_os = "linux")]
use muniment_core::attach::{
    load_client_credentials, CompanionRecord, CompanionRegistry, COMPANION_CREDENTIAL_FILE_NAME,
};
use muniment_core::attach::{
    ProtocolError, WorkspaceContextMap, WorkspaceOnboardRequest, WorkspaceOnboarded,
};
use muniment_core::chat_profile::{ChatProfile, ChatProfileError};
use muniment_core::journal::reconciliation::reconcile_interrupted_runs;
use muniment_core::run_events::{ChatStorage, SharedStorage};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::runtime_provenance;

/// Creates the cross-project home scaffold.
pub fn ensure_home(home: impl AsRef<Path>) -> Result<(), ProtocolError> {
    muniment_core::ensure_cross_project_home(home.as_ref())
        .map_err(|_| ProtocolError::persistence_failed())
}

/// Creates a companion workspace scaffold and records its authorized directories.
pub fn onboard_workspace(
    workspace_contexts: Arc<Mutex<WorkspaceContextMap>>,
    client_identity: &str,
    session_workspace: &str,
    request: WorkspaceOnboardRequest,
) -> Result<WorkspaceOnboarded, ProtocolError> {
    let opened = PathBuf::from(&request.opened_directory);
    let memory = PathBuf::from(&request.memory_location);
    if !opened.is_absolute() || !memory.is_absolute() {
        return Err(ProtocolError::invalid_request());
    }
    let instructions = muniment_core::onboard_companion_workspace(&opened, &memory)
        .map_err(|_| ProtocolError::persistence_failed())?;
    let opened_canonical = opened
        .canonicalize()
        .map_err(|_| ProtocolError::persistence_failed())?;
    let memory_canonical = memory
        .canonicalize()
        .map_err(|_| ProtocolError::persistence_failed())?;
    let mut contexts = workspace_contexts
        .lock()
        .map_err(|_| ProtocolError::persistence_failed())?;
    contexts.record(
        client_identity,
        session_workspace,
        opened_canonical,
        instructions.clone(),
    );
    contexts.record(
        client_identity,
        session_workspace,
        memory_canonical,
        instructions.clone(),
    );
    Ok(WorkspaceOnboarded {
        opened_directory: opened.to_string_lossy().into_owned(),
        memory_location: memory.to_string_lossy().into_owned(),
        instructions,
    })
}

/// Opens profile storage after the ADR 0009 instance-lock cutover gate transfers ownership.
pub fn open_profile_storage(
    profile_directory: impl AsRef<Path>,
) -> Result<SharedStorage, ChatProfileError> {
    let profile = ChatProfile::new(profile_directory.as_ref());
    let (mut journal, cas) = profile.open_storage()?;
    reconcile_interrupted_runs(&mut journal, &runtime_provenance());
    Ok(Arc::new(Mutex::new(ChatStorage { journal, cas })))
}

/// Opens the shared companion registry under a configuration directory.
#[cfg(target_os = "linux")]
pub fn open_companion_registry(
    config_directory: impl AsRef<Path>,
) -> Result<CompanionRegistry, ProtocolError> {
    let credential_path = config_directory
        .as_ref()
        .join(COMPANION_CREDENTIAL_FILE_NAME);
    let credentials = Arc::new(Mutex::new(load_client_credentials(&credential_path)?));
    Ok(CompanionRegistry::new(
        credentials,
        credential_path,
        LiveConnectionRegistry::default(),
    ))
}

/// Lists authorized companions by identity without their secret credentials.
#[cfg(target_os = "linux")]
pub fn list_companions(
    registry: &CompanionRegistry,
) -> Result<Vec<CompanionRecord>, ProtocolError> {
    registry.list()
}

/// Revokes one authorized companion.
#[cfg(target_os = "linux")]
pub fn revoke_companion(registry: &CompanionRegistry, identity: &str) -> Result<(), ProtocolError> {
    registry.revoke(identity)
}
