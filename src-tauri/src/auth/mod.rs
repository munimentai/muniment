//! The desktop sends auth commands to the runtime over the attach socket.
//! Only the runtime reads credentials, renews tokens, and requests chat grants.
//!
//! Commands return display-only session state and errors without token material.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
#[cfg(test)]
use std::time::Duration;

use muniment_core::attach::{RuntimeActivityGuard, RuntimeActivityRegistry};
use muniment_core::auth::{self, AuthStatus, EntitlementSnapshotTracker};
#[cfg(any(unix, target_os = "windows"))]
use serde::Deserialize;
use serde::Serialize;
use tauri::{Emitter, Manager};

#[cfg(any(unix, target_os = "windows"))]
use crate::attach_service::{AttachCompanionState, DesktopClientSession};
#[cfg(any(unix, target_os = "windows"))]
use muniment_core::attach::ClientError;
#[cfg(any(unix, target_os = "windows"))]
use muniment_core::attach::DesktopClientHolder;

/// Managed by Tauri; shared across the `auth_*` commands.
pub struct AuthState {
    sign_in_running: Arc<AtomicBool>,
    entitlement_snapshot_tracker: EntitlementSnapshotTracker,
    runtime_activity: RuntimeActivityRegistry,
    #[cfg(test)]
    test_tokens: Option<muniment_core::auth::TokenSet>,
}

#[derive(Clone, Copy, Serialize)]
struct EntitlementChanged {
    snapshot_version: u64,
}

#[cfg(any(unix, target_os = "windows"))]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntitlementSnapshotResponse {
    snapshot: auth::EntitlementSnapshotView,
    changed_snapshot_version: Option<u64>,
}

pub(crate) fn fresh_tokens<R: tauri::Runtime>(
    state: &AuthState,
    app: &tauri::AppHandle<R>,
) -> Result<muniment_core::auth::TokenSet, String> {
    #[cfg(test)]
    if let Some(tokens) = &state.test_tokens {
        return Ok(tokens.clone());
    }
    let status = state.marked_refresh_blocking(|| {
        runtime_status(app.state::<AttachCompanionState>().desktop_client_session())
    })?;
    status_projection(status)
}

pub(crate) async fn fresh_tokens_async<R: tauri::Runtime>(
    state: &AuthState,
    app: &tauri::AppHandle<R>,
) -> Result<muniment_core::auth::TokenSet, String> {
    let session = app.state::<AttachCompanionState>().desktop_client_session();
    let status = state
        .marked_refresh(move || runtime_status(session))
        .await
        .map_err(|_| runtime_task_error())??;
    status_projection(status)
}

impl AuthState {
    pub fn new(runtime_activity: RuntimeActivityRegistry) -> Self {
        AuthState {
            sign_in_running: Arc::new(AtomicBool::new(false)),
            entitlement_snapshot_tracker: EntitlementSnapshotTracker::new(),
            runtime_activity,
            #[cfg(test)]
            test_tokens: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_test_tokens(
        runtime_activity: RuntimeActivityRegistry,
        tokens: muniment_core::auth::TokenSet,
    ) -> Self {
        let mut state = Self::new(runtime_activity);
        state.test_tokens = Some(tokens);
        state
    }

    /// Marks a sign-in or a sign-out until the returned guard drops.
    fn mark_authentication_operation(&self) -> RuntimeActivityGuard {
        self.runtime_activity.mark_authentication_operation()
    }

    /// Marks a native session refresh until the returned guard drops.
    fn mark_session_refresh(&self) -> RuntimeActivityGuard {
        self.runtime_activity.mark_session_refresh()
    }

    /// Runs a session request on the blocking pool under the refresh mark.
    async fn marked_refresh<T: Send + 'static>(
        &self,
        step: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) -> Result<Result<T, String>, tauri::Error> {
        marked_blocking(self.mark_session_refresh(), step).await
    }

    /// The blocking twin of [`AuthState::marked_refresh`], for the one call
    /// site that already runs on the blocking pool.
    fn marked_refresh_blocking<T>(
        &self,
        step: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        while_marked(self.mark_session_refresh(), step)
    }
}

/// Runs `step` while `mark` holds its runtime activity blocker, so
/// `evaluate_quiesce` reports the auth lane as busy until `step` returns.
fn while_marked<T>(mark: RuntimeActivityGuard, step: impl FnOnce() -> T) -> T {
    let _mark = mark;
    step()
}

/// Runs `step` on the blocking pool under the same mark. The mark covers the
/// whole operation, because the caller takes it before the step starts.
async fn marked_blocking<T: Send + 'static>(
    mark: RuntimeActivityGuard,
    step: impl FnOnce() -> T + Send + 'static,
) -> Result<T, tauri::Error> {
    tauri::async_runtime::spawn_blocking(move || while_marked(mark, step)).await
}

/// Asks the runtime to sign in and returns its status.
/// Rejects concurrent sign-in requests.
#[tauri::command]
#[cfg(any(unix, target_os = "windows"))]
pub async fn auth_sign_in(
    state: tauri::State<'_, AuthState>,
    attach_state: tauri::State<'_, AttachCompanionState>,
) -> Result<AuthStatus, String> {
    let started = native_auth_command_start();
    let result = sign_in_for_session(&state, attach_state.desktop_client_session(), |client| {
        let mut authorization_failure = None;
        let response = native_auth_runtime_result(
            || {
                client
                    .sign_in_with_diagnostics()
                    .map_err(|(error, failure)| {
                        authorization_failure = failure;
                        error
                    })
            },
            |line| eprintln!("{line}"),
        )
        .map_err(|error| {
            authorization_failure
                .map(|failure| failure.to_string())
                .unwrap_or_else(|| desktop_client_error(error))
        })?;
        decode_sign_in_status(response)
    })
    .await;
    native_auth_command_result(result, started)
}

fn native_auth_command_start() -> std::time::Instant {
    let started = std::time::Instant::now();
    eprintln!("muniment-desktop: native-auth start method=COMMAND path=auth_sign_in");
    started
}

fn native_auth_command_result(
    result: Result<AuthStatus, String>,
    started: std::time::Instant,
) -> Result<AuthStatus, String> {
    let outcome = if result.is_ok() {
        "status=ok"
    } else {
        "error=SignIn"
    };
    eprintln!("muniment-desktop: native-auth end method=COMMAND path=auth_sign_in {outcome} elapsed_ms={}", started.elapsed().as_millis());
    result
}

#[cfg(any(unix, target_os = "windows"))]
fn native_auth_runtime_result(
    step: impl FnOnce() -> Result<serde_json::Value, ClientError>,
    mut log: impl FnMut(&str),
) -> Result<serde_json::Value, ClientError> {
    let started = std::time::Instant::now();
    log("muniment-desktop: native-auth start method=RPC path=session.sign_in");
    let result = step();
    log(&native_auth_runtime_after_line(
        &result,
        started.elapsed().as_millis(),
    ));
    result
}

#[cfg(any(unix, target_os = "windows"))]
fn native_auth_runtime_after_line(
    result: &Result<serde_json::Value, ClientError>,
    elapsed_ms: u128,
) -> String {
    // ClientError contains only fixed variants. The response can contain user data.
    let outcome = match result {
        Ok(_) => "status=ok".to_owned(),
        Err(error) => format!("error={error:?}"),
    };
    format!("muniment-desktop: native-auth end method=RPC path=session.sign_in {outcome} elapsed_ms={elapsed_ms}")
}

#[cfg(any(unix, target_os = "windows"))]
fn decode_sign_in_status(response: serde_json::Value) -> Result<AuthStatus, String> {
    let status = response
        .get("status")
        .cloned()
        .ok_or_else(runtime_response_error)?;
    serde_json::from_value(status).map_err(|_| runtime_response_error())
}

#[cfg(any(unix, target_os = "windows"))]
async fn sign_in_for_session(
    state: &AuthState,
    session: DesktopClientSession,
    connected_step: impl FnOnce(DesktopClientHolder) -> Result<AuthStatus, String> + Send + 'static,
) -> Result<AuthStatus, String> {
    match session {
        DesktopClientSession::Connected(client) => {
            sign_in_marked(state, move || connected_step(client)).await
        }
        DesktopClientSession::NoSupervisor | DesktopClientSession::Disconnected => {
            eprintln!("desktop native-auth response: runtime unavailable");
            Err(background_service_error())
        }
    }
}

/// Takes the sign-in permit, then runs `step` under an authentication-operation
/// mark that covers the whole blocking call.
async fn sign_in_marked(
    state: &AuthState,
    step: impl FnOnce() -> Result<AuthStatus, String> + Send + 'static,
) -> Result<AuthStatus, String> {
    let permit = SignInPermit::acquire(state.sign_in_running.clone())
        .ok_or_else(|| "a sign-in is already in progress".to_string())?;
    let outcome = marked_blocking(state.mark_authentication_operation(), move || {
        let _permit = permit;
        step()
    })
    .await
    .map_err(|e| format!("sign-in task failed: {e}"))?;
    let status = outcome?;
    state.entitlement_snapshot_tracker.clear();
    Ok(status)
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

// Legacy local run interfaces accept TokenSet. The shell supplies display fields only.
// The runtime keeps every bearer and refresh token.
fn status_projection(status: AuthStatus) -> Result<auth::TokenSet, String> {
    if !status.signed_in {
        return Err("Sign in before sending a message.".into());
    }
    Ok(auth::TokenSet {
        access_token: String::new(),
        refresh_token: None,
        expires_at: status.expires_at,
        subject: status.subject,
    })
}

fn runtime_status(session: DesktopClientSession) -> Result<AuthStatus, String> {
    let client = connected_client(session)?;
    serde_json::from_value(client.session_status().map_err(desktop_client_error)?)
        .map_err(|_| runtime_response_error())
}

/// Asks the runtime for session status without a cloud request.
#[tauri::command]
#[cfg(any(unix, target_os = "windows"))]
pub async fn auth_status(
    attach_state: tauri::State<'_, AttachCompanionState>,
) -> Result<AuthStatus, String> {
    status_for_session(attach_state.desktop_client_session(), |client| {
        serde_json::from_value(client.session_status().map_err(desktop_client_error)?)
            .map_err(|_| runtime_response_error())
    })
    .await
}

#[cfg(any(unix, target_os = "windows"))]
async fn status_for_session(
    session: DesktopClientSession,
    connected_step: impl FnOnce(DesktopClientHolder) -> Result<AuthStatus, String> + Send + 'static,
) -> Result<AuthStatus, String> {
    let step: Box<dyn FnOnce() -> Result<AuthStatus, String> + Send> = match session {
        DesktopClientSession::Connected(client) => Box::new(move || connected_step(client)),
        DesktopClientSession::NoSupervisor | DesktopClientSession::Disconnected => {
            return Err(background_service_error())
        }
    };
    tauri::async_runtime::spawn_blocking(step)
        .await
        .map_err(|error| format!("status task failed: {error}"))?
}

/// Fetch the authoritative native session and expose only its typed,
/// display-only entitlement projection.
#[cfg(any(unix, target_os = "windows"))]
#[tauri::command]
pub async fn auth_entitlement_snapshot(
    app: tauri::AppHandle,
    state: tauri::State<'_, AuthState>,
    attach_state: tauri::State<'_, AttachCompanionState>,
) -> Result<auth::EntitlementSnapshotView, String> {
    auth_entitlement_snapshot_with_state(app, state, attach_state).await
}

async fn auth_entitlement_snapshot_with_state<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AuthState>,
    #[cfg(any(unix, target_os = "windows"))] attach_state: tauri::State<'_, AttachCompanionState>,
) -> Result<auth::EntitlementSnapshotView, String> {
    let client = connected_client(attach_state.desktop_client_session())?;
    let response = state
        .marked_refresh(move || client.entitlement_snapshot().map_err(desktop_client_error))
        .await
        .map_err(|_| runtime_task_error())??;
    let response: EntitlementSnapshotResponse =
        serde_json::from_value(response).map_err(|_| runtime_response_error())?;
    if let Some(snapshot_version) = response.changed_snapshot_version {
        app.emit(
            "entitlement-changed",
            EntitlementChanged { snapshot_version },
        )
        .map_err(|_| "Muniment could not show the access update. Try again.".to_string())?;
    }
    Ok(response.snapshot)
}

/// List display-only metadata for this account's native installations.
#[cfg(any(unix, target_os = "windows"))]
#[tauri::command]
pub async fn auth_devices(
    app: tauri::AppHandle,
    state: tauri::State<'_, AuthState>,
    attach_state: tauri::State<'_, AttachCompanionState>,
) -> Result<Vec<auth::NativeDevice>, String> {
    auth_devices_with_state(app, state, attach_state).await
}

async fn auth_devices_with_state<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AuthState>,
    #[cfg(any(unix, target_os = "windows"))] attach_state: tauri::State<'_, AttachCompanionState>,
) -> Result<Vec<auth::NativeDevice>, String> {
    let _ = app;
    let client = connected_client(attach_state.desktop_client_session())?;
    let response = state
        .marked_refresh(move || client.list_devices().map_err(desktop_client_error))
        .await
        .map_err(|_| runtime_task_error())??;
    decode_devices(response)
}

#[cfg(any(unix, target_os = "windows"))]
pub(crate) fn background_service_error() -> String {
    "Muniment cannot reach its background service.".to_string()
}

#[cfg(any(unix, target_os = "windows"))]
pub(crate) fn runtime_update_pending_error() -> String {
    "A runtime update is pending. Muniment will start new runs after the update.".to_string()
}

#[cfg(any(unix, target_os = "windows"))]
pub(crate) fn desktop_client_error(error: ClientError) -> String {
    match error {
        ClientError::DesktopBusy => "Muniment is busy with another request. Try again.".to_string(),
        ClientError::RuntimeUpgradePending => runtime_update_pending_error(),
        ClientError::AuthorizationExpired => {
            "The runtime refused the request as unauthorized. Enter local mode or sign in, then retry."
                .to_string()
        }
        ClientError::DesktopUnavailable | ClientError::ConnectionClosed => background_service_error(),
        ClientError::Timeout => "The background service did not answer in time. Try again.".into(),
        ClientError::ThreadNotFound => "Muniment cannot find this thread. Open another thread.".into(),
        ClientError::RequestRejected => "The background service refused the request. Try again.".into(),
        ClientError::DesktopFailed => "The background service could not complete the request. Try again.".into(),
        ClientError::CapabilityRevoked => "The background service revoked access. Restart Muniment.".into(),
        ClientError::ProtocolIncompatible => "Muniment and its background service use different protocols. Restart Muniment.".into(),
        ClientError::MalformedFrame | ClientError::PayloadTooLarge | ClientError::UnexpectedMessage => runtime_response_error(),
        ClientError::UnsupportedPlatform | ClientError::RuntimeDirectoryMissing | ClientError::RuntimeDirectoryRelative
        | ClientError::RandomnessUnavailable | ClientError::WriterFailed =>
            "Muniment could not prepare the request. Restart Muniment.".into(),
    }
}

#[cfg(any(unix, target_os = "windows"))]
pub(crate) fn desktop_request_error(
    error: ClientError,
    response: Option<&muniment_core::attach::ProtocolError>,
) -> String {
    if let Some(muniment_core::attach::ErrorDetails::RequestReason { reason }) =
        response.and_then(|response| response.details())
    {
        return reason.clone();
    }
    desktop_client_error(error)
}

fn runtime_response_error() -> String {
    "The background service returned an invalid response. Restart Muniment.".into()
}

fn runtime_task_error() -> String {
    "Muniment could not complete the background service request. Try again.".into()
}

fn decode_devices(response: serde_json::Value) -> Result<Vec<auth::NativeDevice>, String> {
    serde_json::from_value::<auth::NativeDeviceList>(response)
        .map(|list| list.devices)
        .map_err(|_| runtime_response_error())
}

fn connected_client(session: DesktopClientSession) -> Result<DesktopClientHolder, String> {
    match session {
        DesktopClientSession::Connected(client) => Ok(client),
        DesktopClientSession::NoSupervisor | DesktopClientSession::Disconnected => {
            Err(background_service_error())
        }
    }
}

/// Clear the local native session while preserving the installation identity.
#[tauri::command]
pub async fn auth_sign_out(
    state: tauri::State<'_, AuthState>,
    attach_state: tauri::State<'_, crate::attach_service::AttachCompanionState>,
) -> Result<AuthStatus, String> {
    sign_out_for_session(
        &state,
        attach_state.desktop_client_session(),
        || attach_state.clear_workspace(),
        |client| {
            let response = client.sign_out().map_err(desktop_client_error)?;
            decode_sign_out_status(response)
        },
    )
    .await
}

#[cfg(any(unix, target_os = "windows"))]
fn decode_sign_out_status(response: serde_json::Value) -> Result<AuthStatus, String> {
    decode_sign_in_status(response)
}

#[cfg(any(unix, target_os = "windows"))]
async fn sign_out_for_session(
    state: &AuthState,
    session: DesktopClientSession,
    clear_workspace: impl FnOnce(),
    connected_step: impl FnOnce(DesktopClientHolder) -> Result<AuthStatus, String> + Send + 'static,
) -> Result<AuthStatus, String> {
    match session {
        DesktopClientSession::Connected(client) => {
            sign_out_marked(state, clear_workspace, move || connected_step(client)).await
        }
        DesktopClientSession::NoSupervisor | DesktopClientSession::Disconnected => {
            Err(background_service_error())
        }
    }
}

/// Takes the authentication-operation mark before `clear_workspace` mutates
/// any state, and holds it until the whole sign-out returns.
async fn sign_out_marked(
    state: &AuthState,
    clear_workspace: impl FnOnce(),
    step: impl FnOnce() -> Result<AuthStatus, String> + Send + 'static,
) -> Result<AuthStatus, String> {
    let _mark = state.mark_authentication_operation();
    clear_workspace();
    let status = tauri::async_runtime::spawn_blocking(step)
        .await
        .map_err(|e| format!("sign-out task failed: {e}"))??;
    state.entitlement_snapshot_tracker.clear();
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grant_refusal_keeps_the_cloud_code_and_message() {
        let reason = "chat_not_entitled: No chat model is currently available for this account.";
        let error = muniment_core::attach::ProtocolError::unauthorized_with_reason(reason);
        assert_eq!(
            desktop_request_error(ClientError::AuthorizationExpired, Some(&error)),
            reason
        );
        assert_eq!(
            desktop_request_error(ClientError::DesktopUnavailable, None),
            background_service_error()
        );
    }

    #[test]
    fn only_transport_loss_reports_an_unreachable_service() {
        for error in [
            ClientError::UnsupportedPlatform,
            ClientError::AuthorizationExpired,
            ClientError::ThreadNotFound,
            ClientError::RequestRejected,
            ClientError::DesktopFailed,
            ClientError::RuntimeDirectoryMissing,
            ClientError::RuntimeDirectoryRelative,
            ClientError::DesktopBusy,
            ClientError::Timeout,
            ClientError::MalformedFrame,
            ClientError::PayloadTooLarge,
            ClientError::UnexpectedMessage,
            ClientError::CapabilityRevoked,
            ClientError::ProtocolIncompatible,
            ClientError::RuntimeUpgradePending,
            ClientError::RandomnessUnavailable,
            ClientError::WriterFailed,
        ] {
            assert_ne!(
                desktop_client_error(error),
                background_service_error(),
                "{error:?}"
            );
        }
        for error in [
            ClientError::DesktopUnavailable,
            ClientError::ConnectionClosed,
        ] {
            assert_eq!(desktop_client_error(error), background_service_error());
        }
    }

    #[test]
    fn concurrent_sign_in_is_rejected_and_guard_releases_on_drop() {
        let running = Arc::new(AtomicBool::new(false));
        let first = SignInPermit::acquire(running.clone()).unwrap();
        assert!(SignInPermit::acquire(running.clone()).is_none());
        drop(first);
        assert!(SignInPermit::acquire(running).is_some());
    }

    fn signed_out_status() -> AuthStatus {
        AuthStatus {
            signed_in: false,
            subject: None,
            expires_at: None,
        }
    }

    /// Records the authentication mark the registry reports when it runs.
    fn record_authentication_mark(
        runtime_activity: &RuntimeActivityRegistry,
        recorder: &Arc<AtomicBool>,
    ) -> impl FnOnce() + Send + 'static {
        let runtime_activity = runtime_activity.clone();
        let recorder = recorder.clone();
        move || {
            recorder.store(
                runtime_activity.snapshot().authentication_operation,
                Ordering::SeqCst,
            )
        }
    }

    #[test]
    fn a_sign_in_marks_the_authentication_operation_until_it_returns() {
        let runtime_activity = RuntimeActivityRegistry::new();
        let state = AuthState::new(runtime_activity.clone());
        let marked_during_sign_in = Arc::new(AtomicBool::new(false));
        let observe = record_authentication_mark(&runtime_activity, &marked_during_sign_in);

        let status = tauri::async_runtime::block_on(sign_in_marked(&state, move || {
            observe();
            Ok(signed_out_status())
        }))
        .unwrap();

        assert!(!status.signed_in);
        assert!(marked_during_sign_in.load(Ordering::SeqCst));
        assert!(!runtime_activity.snapshot().authentication_operation);
    }

    #[test]
    fn a_sign_out_marks_the_authentication_operation_before_it_clears_the_workspace() {
        let runtime_activity = RuntimeActivityRegistry::new();
        let state = AuthState::new(runtime_activity.clone());
        let marked_at_clear = Arc::new(AtomicBool::new(false));
        let marked_during_sign_out = Arc::new(AtomicBool::new(false));
        let observe_clear = record_authentication_mark(&runtime_activity, &marked_at_clear);
        let observe_step = record_authentication_mark(&runtime_activity, &marked_during_sign_out);

        let status =
            tauri::async_runtime::block_on(sign_out_marked(&state, observe_clear, move || {
                observe_step();
                Ok(signed_out_status())
            }))
            .unwrap();

        assert!(!status.signed_in);
        assert!(marked_at_clear.load(Ordering::SeqCst));
        assert!(marked_during_sign_out.load(Ordering::SeqCst));
        assert!(!runtime_activity.snapshot().authentication_operation);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn connected_sign_out_decodes_the_wrapped_status() {
        let status = decode_sign_out_status(serde_json::json!({
            "status": {
                "signed_in": false,
                "subject": null,
                "expires_at": null
            }
        }))
        .unwrap();

        assert!(!status.signed_in);
        assert_eq!(status.subject, None);
        assert_eq!(status.expires_at, None);
    }

    #[cfg(unix)]
    #[test]
    fn native_auth_runtime_result_logs_success_error_and_timeout() {
        let response = serde_json::json!({"status": {"signed_in": true}});
        let mut success_log = Vec::new();
        assert_eq!(
            native_auth_runtime_result(
                || Ok(response.clone()),
                |line| { success_log.push(line.to_owned()) }
            ),
            Ok(response)
        );
        assert_eq!(success_log.len(), 2);
        assert_eq!(
            success_log[0],
            "muniment-desktop: native-auth start method=RPC path=session.sign_in"
        );
        assert!(success_log[1].starts_with("muniment-desktop: native-auth end method=RPC path=session.sign_in status=ok elapsed_ms="));
        assert_eq!(native_auth_runtime_after_line(&Ok(serde_json::json!({"subject": "secret-user", "token": "secret-token"})), 17),
            "muniment-desktop: native-auth end method=RPC path=session.sign_in status=ok elapsed_ms=17");

        for error in [
            ClientError::ConnectionClosed,
            ClientError::Timeout,
            ClientError::DesktopUnavailable,
            ClientError::RequestRejected,
            ClientError::MalformedFrame,
        ] {
            let mut log = Vec::new();
            assert_eq!(
                native_auth_runtime_result(|| Err(error), |line| log.push(line.to_owned())),
                Err(error)
            );
            assert_eq!(log.len(), 2);
            assert_eq!(log[0], success_log[0]);
            let prefix = format!("muniment-desktop: native-auth end method=RPC path=session.sign_in error={error:?} elapsed_ms=");
            log[1]
                .strip_prefix(&prefix)
                .unwrap()
                .parse::<u128>()
                .unwrap();
            assert_eq!(
                native_auth_runtime_after_line(&Err(error), 0),
                format!("{prefix}0")
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn connected_sign_in_decodes_the_wrapped_status() {
        let status = decode_sign_in_status(serde_json::json!({
            "status": {
                "signed_in": true,
                "subject": "account-1",
                "expires_at": 42
            }
        }))
        .unwrap();

        assert!(status.signed_in);
        assert_eq!(status.subject.as_deref(), Some("account-1"));
        assert_eq!(status.expires_at, Some(42));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn status_handles_each_desktop_client_session() {
        for session in [
            DesktopClientSession::NoSupervisor,
            DesktopClientSession::Disconnected,
        ] {
            let error = tauri::async_runtime::block_on(status_for_session(session, |_| {
                panic!("An unavailable runtime must not receive a request.")
            }))
            .unwrap_err();
            assert_eq!(error, background_service_error());
        }
        let calls = Arc::new(AtomicBool::new(false));
        let observed = calls.clone();
        let status = tauri::async_runtime::block_on(status_for_session(
            DesktopClientSession::Connected(DesktopClientHolder::new()),
            move |_| {
                observed.store(true, Ordering::SeqCst);
                Ok(signed_out_status())
            },
        ))
        .unwrap();
        assert!(!status.signed_in);
        assert!(calls.load(Ordering::SeqCst));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sign_in_handles_each_desktop_client_session() {
        let state = AuthState::new(RuntimeActivityRegistry::new());
        state.entitlement_snapshot_tracker.observe(Some(1));
        for session in [
            DesktopClientSession::NoSupervisor,
            DesktopClientSession::Disconnected,
        ] {
            let error =
                tauri::async_runtime::block_on(sign_in_for_session(&state, session, |_| {
                    panic!("An unavailable runtime must not receive a request.")
                }))
                .unwrap_err();
            assert_eq!(error, background_service_error());
            assert_eq!(state.entitlement_snapshot_tracker.observe(Some(1)), None);
        }
        let calls = Arc::new(AtomicBool::new(false));
        let observed = calls.clone();
        tauri::async_runtime::block_on(sign_in_for_session(
            &state,
            DesktopClientSession::Connected(DesktopClientHolder::new()),
            move |_| {
                observed.store(true, Ordering::SeqCst);
                Ok(signed_out_status())
            },
        ))
        .unwrap();
        assert!(calls.load(Ordering::SeqCst));
        assert_eq!(state.entitlement_snapshot_tracker.observe(Some(2)), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sign_in_permit_rejects_the_served_route_and_missing_runtime() {
        let state = AuthState::new(RuntimeActivityRegistry::new());
        let _permit = SignInPermit::acquire(state.sign_in_running.clone()).unwrap();
        for (session, expected) in [
            (
                DesktopClientSession::NoSupervisor,
                background_service_error(),
            ),
            (
                DesktopClientSession::Disconnected,
                background_service_error(),
            ),
            (
                DesktopClientSession::Connected(DesktopClientHolder::new()),
                "a sign-in is already in progress".into(),
            ),
        ] {
            let error =
                tauri::async_runtime::block_on(sign_in_for_session(&state, session, |_| {
                    panic!("The rejected sign-in must not run.")
                }))
                .unwrap_err();
            assert_eq!(error, expected);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sign_out_handles_each_desktop_client_session() {
        use std::sync::atomic::AtomicUsize;
        let state = AuthState::new(RuntimeActivityRegistry::new());
        for session in [
            DesktopClientSession::NoSupervisor,
            DesktopClientSession::Disconnected,
        ] {
            let error = tauri::async_runtime::block_on(sign_out_for_session(
                &state,
                session,
                || panic!("A rejected sign-out must not clear the workspace."),
                |_| panic!("An unavailable runtime must not receive a request."),
            ))
            .unwrap_err();
            assert_eq!(error, background_service_error());
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let clear_calls = calls.clone();
        let connected_calls = calls.clone();
        let status = tauri::async_runtime::block_on(sign_out_for_session(
            &state,
            DesktopClientSession::Connected(DesktopClientHolder::new()),
            move || assert_eq!(clear_calls.fetch_add(1, Ordering::SeqCst), 0),
            move |_| {
                assert_eq!(connected_calls.fetch_add(1, Ordering::SeqCst), 1);
                Ok(signed_out_status())
            },
        ))
        .unwrap();
        assert!(!status.signed_in);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_session_refresh_marks_the_refresh_until_it_returns() {
        let runtime_activity = RuntimeActivityRegistry::new();
        let state = AuthState::new(runtime_activity.clone());
        let observed = runtime_activity.clone();
        let session = tauri::async_runtime::block_on(state.marked_refresh(move || {
            assert!(observed.snapshot().session_refresh);
            Ok(signed_out_status())
        }))
        .unwrap()
        .unwrap();
        assert!(!session.signed_in);
        assert!(!runtime_activity.snapshot().session_refresh);
    }

    #[test]
    fn a_blocking_session_refresh_marks_the_refresh_until_it_returns() {
        let runtime_activity = RuntimeActivityRegistry::new();
        let state = AuthState::new(runtime_activity.clone());
        let session = state
            .marked_refresh_blocking(|| {
                assert!(runtime_activity.snapshot().session_refresh);
                Ok(signed_out_status())
            })
            .unwrap();
        assert!(!session.signed_in);
        assert!(!runtime_activity.snapshot().session_refresh);
    }

    #[test]
    fn a_failed_authentication_operation_clears_its_mark() {
        let runtime_activity = RuntimeActivityRegistry::new();
        let state = AuthState::new(runtime_activity.clone());

        let outcome = tauri::async_runtime::block_on(sign_in_marked(&state, || {
            Err(auth::NativeSignInError::TokenExchange.to_string())
        }));

        assert!(outcome.is_err());
        assert!(!runtime_activity.snapshot().authentication_operation);
    }

    #[test]
    fn malformed_status_is_redacted_and_signed_out_has_no_projection() {
        for response in [
            serde_json::json!({}),
            serde_json::json!({"status": {"signed_in": "secret"}}),
        ] {
            assert_eq!(
                decode_sign_in_status(response.clone()).unwrap_err(),
                runtime_response_error()
            );
            assert_eq!(
                decode_sign_out_status(response).unwrap_err(),
                runtime_response_error()
            );
        }
        assert!(status_projection(signed_out_status()).is_err());
    }

    #[test]
    fn a_failed_session_refresh_releases_its_activity_mark() {
        let activity = RuntimeActivityRegistry::new();
        let state = AuthState::new(activity.clone());
        let result = tauri::async_runtime::block_on(
            state.marked_refresh(|| Err::<(), _>(background_service_error())),
        )
        .unwrap();
        assert_eq!(result, Err(background_service_error()));
        assert!(!activity.snapshot().session_refresh);
    }

    #[test]
    fn entitlement_changed_payload_contains_only_the_version() {
        let payload = serde_json::to_value(EntitlementChanged {
            snapshot_version: 42,
        })
        .unwrap();
        assert_eq!(payload, serde_json::json!({"snapshot_version": 42}));
    }

    #[test]
    fn device_listing_rejects_a_missing_fresh_session_without_calling_the_client() {
        assert_eq!(
            connected_client(DesktopClientSession::NoSupervisor).err(),
            Some(background_service_error())
        );
        assert_eq!(
            connected_client(DesktopClientSession::Disconnected).err(),
            Some(background_service_error())
        );
    }

    #[test]
    fn device_listing_redacts_client_errors() {
        let error = decode_devices(serde_json::json!({"devices": "backend-secret"})).unwrap_err();
        assert_eq!(error, runtime_response_error());
        assert!(!error.contains("secret"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn auth_reads_cover_each_desktop_client_state() {
        use muniment_core::attach::{
            encode_frame, reconnect_welcome, serve_desktop_client_at, Id, Protocol, Response,
            Success,
        };
        use serde_json::{json, Value};
        use std::io::{Read, Write};
        use std::os::unix::net::UnixListener;
        use std::sync::mpsc;
        use tauri::{Listener, Manager};

        fn read_value(stream: &mut impl Read) -> Value {
            let mut length = [0; 4];
            stream.read_exact(&mut length).unwrap();
            let mut payload = vec![0; u32::from_be_bytes(length) as usize];
            stream.read_exact(&mut payload).unwrap();
            serde_json::from_slice(&payload).unwrap()
        }

        fn app_with_state(
            attach_state: AttachCompanionState,
        ) -> tauri::App<tauri::test::MockRuntime> {
            let app = tauri::test::mock_app();
            app.manage(AuthState::new(RuntimeActivityRegistry::new()));
            app.manage(attach_state);
            app
        }

        fn entitlement(
            app: &tauri::App<tauri::test::MockRuntime>,
        ) -> Result<auth::EntitlementSnapshotView, String> {
            tauri::async_runtime::block_on(auth_entitlement_snapshot_with_state(
                app.handle().clone(),
                app.state(),
                app.state(),
            ))
        }

        fn devices(
            app: &tauri::App<tauri::test::MockRuntime>,
        ) -> Result<Vec<auth::NativeDevice>, String> {
            tauri::async_runtime::block_on(auth_devices_with_state(
                app.handle().clone(),
                app.state(),
                app.state(),
            ))
        }

        let local_app = app_with_state(AttachCompanionState::default());
        assert_eq!(
            entitlement(&local_app).unwrap_err(),
            background_service_error()
        );
        assert_eq!(devices(&local_app).unwrap_err(), background_service_error());

        let endpoint = crate::test_support::socket_temp_path();
        let listener = UnixListener::bind(&endpoint).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            assert_eq!(read_value(&mut stream)["client"]["kind"], "desktop-client");
            stream
                .write_all(
                    &encode_frame(&reconnect_welcome(1, "0.0.1", "11".repeat(16), "")).unwrap(),
                )
                .unwrap();
            stream
                .write_all(
                    &encode_frame(&json!({
                        "profile_id": "profile-1",
                        "capability": "33".repeat(32),
                        "expires_at": 60,
                        "idle_timeout_seconds": 30,
                        "workspace_scopes": {"/work/signed": ["threads:read"]}
                    }))
                    .unwrap(),
                )
                .unwrap();
            let bodies = [
                json!({
                    "snapshot": {
                        "snapshot_version": 7,
                        "org_id": "20000000-0000-4000-8000-000000000002",
                        "user_id": "30000000-0000-4000-8000-000000000003",
                        "role": "owner",
                        "user_display_name": "User",
                        "organization_display_name": "Muniment",
                        "capabilities": [],
                        "grants": []
                    },
                    "changed_snapshot_version": 7
                }),
                json!({"devices": [{
                    "device_id": "10000000-0000-4000-8000-000000000001",
                    "client_id": "desktop-1",
                    "client_role": "desktop",
                    "platform": "desktop",
                    "created_at": "2026-01-01T00:00:00Z",
                    "revoked_at": null,
                    "last_active_at": "2026-01-02T00:00:00Z",
                    "current": true
                }]}),
                json!({"signed_in": true, "subject": "owner", "expires_at": 42}),
                json!({"signed_in": true, "subject": "owner", "expires_at": 900}),
            ];
            let mut operations = Vec::new();
            for body in bodies {
                let request = read_value(&mut stream);
                operations.push(request["operation"].clone());
                let request_id = Id::new(request["request_id"].as_str().unwrap()).unwrap();
                stream
                    .write_all(
                        &encode_frame(&Response {
                            protocol: Protocol,
                            request_id,
                            ok: Success,
                            body,
                        })
                        .unwrap(),
                    )
                    .unwrap();
            }
            operations
        });
        let live_state = AttachCompanionState::default();
        let client_endpoint = endpoint.clone();
        let (connected_tx, connected_rx) = mpsc::channel();
        live_state.set_desktop_client_for_test(false, move |stop, holder| {
            std::thread::spawn(move || {
                let mut connected_tx = Some(connected_tx);
                serve_desktop_client_at(
                    &client_endpoint,
                    "0.0.1",
                    Duration::from_secs(1),
                    Duration::from_millis(10),
                    stop,
                    holder,
                    move |connected| {
                        if connected {
                            connected_tx.take().unwrap().send(()).unwrap();
                        }
                    },
                )
            })
        });
        connected_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        live_state.set_desktop_client_for_test(true, |_, _| std::thread::spawn(|| {}));
        let live_app = app_with_state(live_state);
        let (event_tx, event_rx) = mpsc::channel();
        live_app.listen("entitlement-changed", move |event| {
            event_tx.send(event.payload().to_string()).unwrap();
        });
        assert_eq!(entitlement(&live_app).unwrap().snapshot_version, 7);
        assert_eq!(devices(&live_app).unwrap().len(), 1);
        for expiry in [42, 900] {
            let projection = tauri::async_runtime::block_on(fresh_tokens_async(
                &live_app.state::<AuthState>(),
                live_app.handle(),
            ))
            .unwrap();
            assert_eq!(projection.subject.as_deref(), Some("owner"));
            assert_eq!(projection.expires_at, Some(expiry));
            assert!(projection.access_token.is_empty());
            assert!(projection.refresh_token.is_none());
        }
        assert_eq!(
            event_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            json!({"snapshot_version": 7}).to_string()
        );
        assert_eq!(
            server.join().unwrap(),
            [
                json!("entitlement.snapshot"),
                json!("device.list"),
                json!("session.status"),
                json!("session.status")
            ]
        );
        live_app
            .state::<AttachCompanionState>()
            .stop_desktop_client_for_test();

        let disconnected_state = AttachCompanionState::default();
        disconnected_state.set_desktop_client_for_test(false, |_, _| std::thread::spawn(|| {}));
        let disconnected_app = app_with_state(disconnected_state);
        assert_eq!(
            entitlement(&disconnected_app).unwrap_err(),
            background_service_error()
        );
        assert_eq!(
            devices(&disconnected_app).unwrap_err(),
            background_service_error()
        );
        disconnected_app
            .state::<AttachCompanionState>()
            .stop_desktop_client_for_test();
    }
}
