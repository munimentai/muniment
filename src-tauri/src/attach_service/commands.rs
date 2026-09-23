use super::*;

#[tauri::command]
pub fn attach_companions(
    state: tauri::State<'_, AttachCompanionState>,
) -> Result<Vec<AuthorizedCompanion>, ProtocolError> {
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    return match state.desktop_client_session() {
        DesktopClientSession::NoSupervisor => {
            #[cfg(target_os = "linux")]
            {
                state.listener()?.list_companions()
            }
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            {
                Err(ProtocolError::persistence_failed())
            }
        }
        DesktopClientSession::Connected(client) => {
            #[derive(serde::Deserialize)]
            struct CompanionList {
                companions: Vec<AuthorizedCompanion>,
            }

            let response = client.list_companions().map_err(companion_client_error)?;
            serde_json::from_value::<CompanionList>(response)
                .map(|response| response.companions)
                .map_err(|_| ProtocolError::persistence_failed())
        }
        DesktopClientSession::Disconnected => Err(ProtocolError::persistence_failed()),
    };

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = state;
        Ok(Vec::new())
    }
}

#[tauri::command]
pub fn attach_listener_status(
    state: tauri::State<'_, AttachCompanionState>,
) -> AttachListenerStatus {
    #[cfg(any(unix, target_os = "windows"))]
    return state.listener_status();

    #[cfg(not(any(unix, target_os = "windows")))]
    {
        let _ = state;
        AttachListenerStatus {
            started: true,
            failure: None,
            pending: false,
            stopped: false,
            presenting: false,
            connected: false,
            chat_events_connected: false,
            supervisor_running: false,
            runtime_upgrade_pending: false,
        }
    }
}

#[cfg(any(unix, target_os = "windows"))]
pub(super) fn observe_desktop_client_connection<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    connected: bool,
) {
    app.state::<AttachCompanionState>()
        .record_connected(connected);
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    eprintln!("desktop runtime client connected={connected}");
    #[cfg(target_os = "linux")]
    eprintln!("Desktop client connected: {connected}.");
    let status = app.state::<AttachCompanionState>().listener_status();
    let _ = app.emit("desktop-client-status-changed", status);
}

#[cfg(any(unix, target_os = "windows"))]
pub(super) fn observe_chat_event_subscription<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    connected: bool,
) {
    app.state::<AttachCompanionState>()
        .record_chat_events_connected(connected);
    #[cfg(target_os = "linux")]
    eprintln!("Chat events connected: {connected}.");
    let status = app.state::<AttachCompanionState>().listener_status();
    let _ = app.emit("desktop-client-status-changed", status);
}

#[tauri::command]
pub fn attach_revoke_companion(
    state: tauri::State<'_, AttachCompanionState>,
    client_identity: String,
) -> Result<(), ProtocolError> {
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    return match state.desktop_client_session() {
        DesktopClientSession::NoSupervisor => {
            #[cfg(target_os = "linux")]
            {
                state.listener()?.revoke_companion(&client_identity)
            }
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            {
                Err(ProtocolError::persistence_failed())
            }
        }
        DesktopClientSession::Connected(client) => client
            .revoke_companion(&client_identity)
            .map(|_| ())
            .map_err(companion_client_error),
        DesktopClientSession::Disconnected => Err(ProtocolError::persistence_failed()),
    };

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = (state, client_identity);
        Err(ProtocolError::unsupported_operation())
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
pub(super) fn companion_client_error(error: ClientError) -> ProtocolError {
    match error {
        ClientError::DesktopBusy => ProtocolError::desktop_busy(),
        _ => ProtocolError::persistence_failed(),
    }
}

#[cfg(target_os = "linux")]
impl AttachListenerState {
    #[cfg(test)]
    pub(super) fn load(credential_path: &std::path::Path) -> Result<Self, ProtocolError> {
        Self::load_with_approval(credential_path, SignedWorkspaceApproval::default())
    }

    pub(super) fn load_with_approval(
        credential_path: &std::path::Path,
        approval: SignedWorkspaceApproval,
    ) -> Result<Self, ProtocolError> {
        let client_credentials = Arc::new(Mutex::new(load_client_credentials(credential_path)?));
        let companion_registry = CompanionRegistry::new(
            client_credentials.clone(),
            credential_path,
            LiveConnectionRegistry::default(),
        );
        Ok(Self {
            workspace_contexts: Arc::new(Mutex::new(WorkspaceContextMap::default())),
            client_credentials,
            companion_registry,
            approval,
        })
    }

    pub(super) fn approval(&self) -> Option<Approval> {
        self.approval.approval()
    }

    pub(crate) fn revoke_companion(&self, client_identity: &str) -> Result<(), ProtocolError> {
        self.companion_registry.revoke(client_identity)
    }

    pub(crate) fn list_companions(&self) -> Result<Vec<AuthorizedCompanion>, ProtocolError> {
        self.companion_registry.list().map(|companions| {
            companions
                .into_iter()
                .map(|companion| AuthorizedCompanion {
                    identity: companion.identity,
                    claimed_kind: companion.claimed_kind,
                    claimed_version: companion.claimed_version,
                    approved_at: companion.approved_at,
                })
                .collect()
        })
    }
}

pub type AttachApprovalState = ApprovalCoordinator;

#[derive(serde::Serialize)]
pub(super) struct AttachPairingRequest {
    pub(super) challenge: String,
    pub(super) claimed_kind: String,
    pub(super) claimed_version: String,
    pub(super) workspace: String,
    pub(super) scopes: std::collections::BTreeSet<String>,
}

#[tauri::command]
pub fn attach_pairing_decide(
    state: tauri::State<'_, AttachApprovalState>,
    challenge: String,
    approve: bool,
) {
    state.decide(&challenge, approve);
}

#[cfg(target_os = "linux")]
pub(super) fn resolve_attach_home(
    config: &Path,
    documents: Option<PathBuf>,
    home: Option<PathBuf>,
) -> muniment_core::attach::AttachHome {
    muniment_core::attach::AttachHome::configured(config.to_path_buf(), documents, home)
}

#[cfg(target_os = "linux")]
pub(super) trait TauriDesktopAttachService<R: tauri::Runtime> {
    fn new(
        app: tauri::AppHandle<R>,
        workspace_contexts: WorkspaceContexts,
        client_credentials: Arc<Mutex<HashMap<String, ClientCredential>>>,
    ) -> Result<Self, ProtocolError>
    where
        Self: Sized;
}

#[cfg(target_os = "linux")]
impl<R: tauri::Runtime> TauriDesktopAttachService<R>
    for DesktopAttachService<TauriRunStartBoundaries<R>>
{
    fn new(
        app: tauri::AppHandle<R>,
        workspace_contexts: WorkspaceContexts,
        client_credentials: Arc<Mutex<HashMap<String, ClientCredential>>>,
    ) -> Result<Self, ProtocolError> {
        let drain_state = app
            .state::<muniment_core::attach::DrainState>()
            .inner()
            .clone();
        let config = muniment_runtime::profile_directory()
            .map_err(|_| ProtocolError::persistence_failed())?;
        let home = resolve_attach_home(
            &config,
            app.path().document_dir().ok(),
            app.path().home_dir().ok(),
        );
        let idempotency = IdempotencyStore::open(config.join("attach-idempotency.sqlite3"))?;
        let credential_path = config.join(COMPANION_CREDENTIAL_FILE_NAME);
        Ok(Self {
            boundaries: TauriRunStartBoundaries {
                app,
                continue_session_thread: false,
            },
            idempotency,
            home,
            workspace_contexts,
            client_credentials,
            credential_path: Some(credential_path),
            client_identity: None,
            drain_state,
        })
    }
}

#[cfg(target_os = "linux")]
pub(super) fn request_attach_pairing_approval(
    approval_state: &AttachListenerState,
    approvals: &AttachApprovalState,
    challenge: &str,
    claimed_kind: &str,
    claimed_version: &str,
    remaining: Duration,
) -> muniment_core::attach::linux::ApprovalDecision {
    let Some(approval) = approval_state.approval() else {
        return muniment_core::attach::linux::ApprovalDecision::Deny;
    };
    let approved = approvals.request(
        ApprovalRequest {
            challenge: challenge.to_owned(),
            claimed_kind: bounded_claim(claimed_kind),
            claimed_version: bounded_claim(claimed_version),
            workspace: approval.workspace.clone(),
            scopes: approval.scopes.clone(),
        },
        remaining,
    );
    if !approved {
        return muniment_core::attach::linux::ApprovalDecision::Deny;
    }
    match approval_state.approval() {
        Some(current) if current.workspace == approval.workspace => {
            muniment_core::attach::linux::ApprovalDecision::Approve(current)
        }
        _ => muniment_core::attach::linux::ApprovalDecision::Deny,
    }
}
