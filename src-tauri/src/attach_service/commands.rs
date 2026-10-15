use super::*;

#[tauri::command]
pub fn attach_companions(
    state: tauri::State<'_, AttachCompanionState>,
) -> Result<Vec<AuthorizedCompanion>, ProtocolError> {
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    return match state.desktop_client_session() {
        DesktopClientSession::NoSupervisor | DesktopClientSession::Disconnected => {
            Err(ProtocolError::persistence_failed())
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
    #[cfg(any(target_os = "linux", target_os = "macos"))]
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
        DesktopClientSession::NoSupervisor | DesktopClientSession::Disconnected => {
            Err(ProtocolError::persistence_failed())
        }
        DesktopClientSession::Connected(client) => client
            .revoke_companion(&client_identity)
            .map(|_| ())
            .map_err(companion_client_error),
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

pub type AttachApprovalState = ApprovalCoordinator;

#[derive(serde::Serialize)]
pub(super) struct AttachPairingRequest {
    pub(super) challenge: String,
    pub(super) claimed_kind: String,
    pub(super) claimed_version: String,
    pub(super) workspace: String,
    pub(super) scopes: std::collections::BTreeSet<String>,
    pub(super) deadline_ms: u64,
}

#[tauri::command]
pub fn attach_pairing_decide(
    state: tauri::State<'_, AttachApprovalState>,
    challenge: String,
    approve: bool,
) {
    state.decide(&challenge, approve);
}
