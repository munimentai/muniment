//! Tauri-side auth surface: the `auth_*` commands, client configuration,
//! and the keychain-backed token store. All protocol logic lives in
//! `muniment_core::auth` (see docs/auth.md), which keeps it testable
//! without the GUI stack; this module only wires it to the webview.
//!
//! Nothing here logs or returns token material: commands hand the webview
//! an `AuthStatus` (signed-in flag, subject, expiry) and error strings that
//! `muniment_core::auth::AuthError` guarantees are token-free.

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use muniment_core::attach::{RuntimeActivityGuard, RuntimeActivityRegistry};
use muniment_core::auth::{
    self, AuthStatus, BrowserOpenError, EntitlementSnapshotTracker, KeyringNativeCredentialStore,
    NativeCredentialStore, UreqAuthorizationTransport, UreqNativeDeviceListTransport,
    UreqRegistrationTransport, UreqRevocationTransport, UreqTokenTransport,
};
#[cfg(target_os = "linux")]
use serde::Deserialize;
use serde::Serialize;
use tauri::Emitter;

#[cfg(target_os = "linux")]
use crate::attach_service::{AttachCompanionState, DesktopClientSession};
#[cfg(target_os = "linux")]
use muniment_core::attach::DesktopClientHolder;

/// How long the loopback listener waits for the user to finish in the
/// browser before the sign-in attempt is abandoned.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);

/// Managed by Tauri; shared across the `auth_*` commands.
pub struct AuthState {
    native_store: Arc<KeyringNativeCredentialStore>,
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

#[cfg(target_os = "linux")]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntitlementSnapshotResponse {
    snapshot: auth::EntitlementSnapshotView,
    changed_snapshot_version: Option<u64>,
}

fn observe_snapshot<R: tauri::Runtime>(
    state: &AuthState,
    app: &tauri::AppHandle<R>,
    session: &auth::FreshNativeSession,
) -> Result<(), String> {
    let next = session
        .entitlement_snapshot
        .as_ref()
        .map(|snapshot| snapshot.snapshot_version);
    if let Some(snapshot_version) = state.entitlement_snapshot_tracker.observe(next) {
        app.emit(
            "entitlement-changed",
            EntitlementChanged { snapshot_version },
        )
        .map_err(|error| format!("entitlement event failed: {error}"))?;
    }
    Ok(())
}

pub(crate) fn fresh_tokens<R: tauri::Runtime>(
    state: &AuthState,
    app: &tauri::AppHandle<R>,
) -> Result<muniment_core::auth::TokenSet, String> {
    #[cfg(test)]
    if let Some(tokens) = &state.test_tokens {
        return Ok(tokens.clone());
    }
    let result = state.marked_refresh_blocking(state.native_store.as_ref())?;
    observe_snapshot(state, app, &result)?;
    result
        .into_credentials()
        .map(|credentials| credentials.tokens)
        .ok_or_else(|| "Sign in before sending a message.".into())
}

pub(crate) async fn fresh_tokens_async<R: tauri::Runtime>(
    state: &AuthState,
    app: &tauri::AppHandle<R>,
) -> Result<muniment_core::auth::TokenSet, String> {
    let result = state
        .marked_refresh(state.native_store.clone())
        .await
        .map_err(|_| "Sign in before sending a message.".to_string())??;
    observe_snapshot(state, app, &result)?;
    result
        .into_credentials()
        .map(|credentials| credentials.tokens)
        .ok_or_else(|| "Sign in before sending a message.".into())
}

impl AuthState {
    pub fn new(runtime_activity: RuntimeActivityRegistry) -> Self {
        AuthState {
            native_store: Arc::new(KeyringNativeCredentialStore::new()),
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

    /// Refreshes the native session on the blocking pool. The mark covers the
    /// whole call, so every async refresh call site goes through here.
    async fn marked_refresh(
        &self,
        store: Arc<dyn NativeCredentialStore>,
    ) -> Result<Result<auth::FreshNativeSession, String>, tauri::Error> {
        marked_blocking(self.mark_session_refresh(), move || {
            ensure_native_session(store.as_ref())
        })
        .await
    }

    /// The blocking twin of [`AuthState::marked_refresh`], for the one call
    /// site that already runs on the blocking pool.
    fn marked_refresh_blocking(
        &self,
        store: &dyn NativeCredentialStore,
    ) -> Result<auth::FreshNativeSession, String> {
        while_marked(self.mark_session_refresh(), || ensure_native_session(store))
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

/// Run the browser sign-in flow, persist the tokens, and report the new
/// status. Concurrent invocations are rejected while one is in flight.
#[tauri::command]
pub async fn auth_sign_in(
    app: tauri::AppHandle,
    state: tauri::State<'_, AuthState>,
) -> Result<AuthStatus, String> {
    let store = state.native_store.clone();
    sign_in_marked(&state, move || sign_in_blocking(store.as_ref(), &app)).await
}

/// Takes the sign-in permit, then runs `step` under an authentication-operation
/// mark that covers the whole blocking call.
async fn sign_in_marked(
    state: &AuthState,
    step: impl FnOnce() -> Result<AuthStatus, auth::NativeSignInError> + Send + 'static,
) -> Result<AuthStatus, String> {
    let permit = SignInPermit::acquire(state.sign_in_running.clone())
        .ok_or_else(|| "a sign-in is already in progress".to_string())?;
    let outcome = marked_blocking(state.mark_authentication_operation(), move || {
        let _permit = permit;
        step()
    })
    .await
    .map_err(|e| format!("sign-in task failed: {e}"))?;
    let status = outcome.map_err(|e| e.to_string())?;
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

fn sign_in_blocking(
    store: &KeyringNativeCredentialStore,
    app: &tauri::AppHandle,
) -> Result<AuthStatus, auth::NativeSignInError> {
    let network_timeout = Duration::from_secs(30);
    auth::run_native_sign_in(
        store,
        &UreqRegistrationTransport::new(network_timeout),
        &UreqAuthorizationTransport::new(network_timeout),
        &UreqTokenTransport::new(network_timeout),
        &|url: &str| spawn_browser(url).map_err(|_| BrowserOpenError),
        &auth::api_base_url(),
        &unix_time,
        SIGN_IN_TIMEOUT,
        &|delay| {
            let _ = app.emit(
                "auth-registration-retry",
                RegistrationRetryStatus {
                    delay_seconds: delay.as_secs(),
                },
            );
            std::thread::sleep(delay);
        },
    )
}

#[derive(Clone, Copy, Serialize)]
struct RegistrationRetryStatus {
    delay_seconds: u64,
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// Signed-in subject/expiry from the stored tokens; no network.
#[tauri::command]
pub async fn auth_status(state: tauri::State<'_, AuthState>) -> Result<AuthStatus, String> {
    let store = state.native_store.clone();
    tauri::async_runtime::spawn_blocking(move || auth::native_status(store.as_ref(), unix_time()))
        .await
        .map_err(|e| format!("status task failed: {e}"))?
        .map_err(|e| e.to_string())
}

/// Fetch the authoritative native session and expose only its typed,
/// display-only entitlement projection.
#[cfg(target_os = "linux")]
#[tauri::command]
pub async fn auth_entitlement_snapshot(
    app: tauri::AppHandle,
    state: tauri::State<'_, AuthState>,
    attach_state: tauri::State<'_, AttachCompanionState>,
) -> Result<auth::EntitlementSnapshotView, String> {
    auth_entitlement_snapshot_with_state(app, state, attach_state).await
}

#[cfg(not(target_os = "linux"))]
#[tauri::command]
pub async fn auth_entitlement_snapshot(
    app: tauri::AppHandle,
    state: tauri::State<'_, AuthState>,
) -> Result<auth::EntitlementSnapshotView, String> {
    auth_entitlement_snapshot_with_state(app, state).await
}

async fn auth_entitlement_snapshot_with_state<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AuthState>,
    #[cfg(target_os = "linux")] attach_state: tauri::State<'_, AttachCompanionState>,
) -> Result<auth::EntitlementSnapshotView, String> {
    #[cfg(target_os = "linux")]
    match attach_state.desktop_client_session() {
        DesktopClientSession::Connected(client) => {
            let response: EntitlementSnapshotResponse = serde_json::from_value(
                client
                    .entitlement_snapshot()
                    .map_err(|_| background_service_error())?,
            )
            .map_err(|_| background_service_error())?;
            if let Some(snapshot_version) = response.changed_snapshot_version {
                app.emit(
                    "entitlement-changed",
                    EntitlementChanged { snapshot_version },
                )
                .map_err(|_| background_service_error())?;
            }
            return Ok(response.snapshot);
        }
        DesktopClientSession::Disconnected => return Err(background_service_error()),
        DesktopClientSession::NoSupervisor => {}
    }
    let result = state
        .marked_refresh(state.native_store.clone())
        .await
        .map_err(|e| format!("access task failed: {e}"))??;
    observe_snapshot(&state, &app, &result)?;
    result
        .entitlement_snapshot
        .ok_or_else(|| "Sign in to view your access.".into())
}

/// List display-only metadata for this account's native installations.
#[cfg(target_os = "linux")]
#[tauri::command]
pub async fn auth_devices(
    app: tauri::AppHandle,
    state: tauri::State<'_, AuthState>,
    attach_state: tauri::State<'_, AttachCompanionState>,
) -> Result<Vec<auth::NativeDevice>, String> {
    auth_devices_with_state(app, state, attach_state).await
}

#[cfg(not(target_os = "linux"))]
#[tauri::command]
pub async fn auth_devices(
    app: tauri::AppHandle,
    state: tauri::State<'_, AuthState>,
) -> Result<Vec<auth::NativeDevice>, String> {
    auth_devices_with_state(app, state).await
}

async fn auth_devices_with_state<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AuthState>,
    #[cfg(target_os = "linux")] attach_state: tauri::State<'_, AttachCompanionState>,
) -> Result<Vec<auth::NativeDevice>, String> {
    #[cfg(target_os = "linux")]
    match attach_state.desktop_client_session() {
        DesktopClientSession::Connected(client) => {
            let response: auth::NativeDeviceList = serde_json::from_value(
                client
                    .list_devices()
                    .map_err(|_| background_service_error())?,
            )
            .map_err(|_| background_service_error())?;
            return Ok(response.devices);
        }
        DesktopClientSession::Disconnected => return Err(background_service_error()),
        DesktopClientSession::NoSupervisor => {}
    }
    let session = state
        .marked_refresh(state.native_store.clone())
        .await
        .map_err(|_| device_list_error())?
        .map_err(|_| device_list_error())?;
    observe_snapshot(&state, &app, &session).map_err(|_| device_list_error())?;
    let credentials = session.into_credentials().ok_or_else(device_list_error)?;
    tauri::async_runtime::spawn_blocking(move || {
        list_devices(
            Some(&credentials.tokens.access_token),
            &UreqNativeDeviceListTransport::new(Duration::from_secs(30)),
            &auth::api_base_url(),
        )
    })
    .await
    .map_err(|_| device_list_error())?
}

#[cfg(target_os = "linux")]
fn background_service_error() -> String {
    "Muniment cannot reach its background service.".to_string()
}

fn list_devices(
    access_token: Option<&str>,
    transport: &dyn auth::NativeDeviceListTransport,
    base_url: &str,
) -> Result<Vec<auth::NativeDevice>, String> {
    let access_token = access_token.ok_or_else(device_list_error)?;
    auth::list_native_devices(transport, base_url, access_token)
        .map(|list| list.devices)
        .map_err(|_| device_list_error())
}

fn device_list_error() -> String {
    "Your devices could not be loaded.".to_string()
}

fn ensure_native_session(
    store: &dyn NativeCredentialStore,
) -> Result<auth::FreshNativeSession, String> {
    auth::ensure_native_session(store, &auth::api_base_url(), unix_time())
        .map_err(|error| error.to_string())
}

/// Clear the local native session while preserving the installation identity.
#[tauri::command]
pub async fn auth_sign_out(
    state: tauri::State<'_, AuthState>,
    attach_state: tauri::State<'_, crate::attach_service::AttachCompanionState>,
) -> Result<AuthStatus, String> {
    let store = state.native_store.clone();

    #[cfg(target_os = "linux")]
    return sign_out_for_session(
        &state,
        attach_state.desktop_client_session(),
        || attach_state.clear_workspace(),
        move || local_sign_out(store),
        |client| {
            let response = client
                .sign_out()
                .map_err(|_| "Muniment cannot reach its background service.".to_string())?;
            decode_sign_out_status(response)
        },
    )
    .await;

    #[cfg(not(target_os = "linux"))]
    sign_out_marked(
        &state,
        || attach_state.clear_workspace(),
        move || local_sign_out(store),
    )
    .await
}

#[cfg(target_os = "linux")]
fn decode_sign_out_status(response: serde_json::Value) -> Result<AuthStatus, String> {
    let status = response
        .get("status")
        .cloned()
        .ok_or_else(|| "missing status field".to_string())?;
    serde_json::from_value(status).map_err(|error| error.to_string())
}

fn local_sign_out(store: Arc<KeyringNativeCredentialStore>) -> Result<AuthStatus, String> {
    auth::sign_out_native_session(
        store.as_ref(),
        &UreqRevocationTransport::new(Duration::from_secs(2)),
        &auth::api_base_url(),
    )
    .map_err(|error| error.to_string())?;
    auth::native_status(store.as_ref(), unix_time()).map_err(|error| error.to_string())
}

#[cfg(target_os = "linux")]
async fn sign_out_for_session(
    state: &AuthState,
    session: DesktopClientSession,
    clear_workspace: impl FnOnce(),
    local_step: impl FnOnce() -> Result<AuthStatus, String> + Send + 'static,
    connected_step: impl FnOnce(DesktopClientHolder) -> Result<AuthStatus, String> + Send + 'static,
) -> Result<AuthStatus, String> {
    match session {
        DesktopClientSession::NoSupervisor => {
            sign_out_marked(state, clear_workspace, local_step).await
        }
        DesktopClientSession::Connected(client) => {
            sign_out_marked(state, clear_workspace, move || connected_step(client)).await
        }
        DesktopClientSession::Disconnected => {
            Err("Muniment cannot reach its background service.".into())
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

#[cfg(target_os = "macos")]
fn spawn_browser(url: &str) -> std::io::Result<()> {
    Command::new("open").arg(url).spawn().map(drop)
}

#[cfg(target_os = "windows")]
fn spawn_browser(url: &str) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // rundll32 takes the URL as a plain argument, sidestepping cmd.exe's
    // parsing of `&` in the query string.
    Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(drop)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn spawn_browser(url: &str) -> std::io::Result<()> {
    Command::new("xdg-open").arg(url).spawn().map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A signed-out store that records the session-refresh mark it sees
    /// while `ensure_native_session` reads it.
    struct ObservingCredentialStore {
        runtime_activity: RuntimeActivityRegistry,
        marked_during_load: AtomicBool,
    }

    impl ObservingCredentialStore {
        fn new(runtime_activity: RuntimeActivityRegistry) -> Self {
            Self {
                runtime_activity,
                marked_during_load: AtomicBool::new(false),
            }
        }
    }

    impl NativeCredentialStore for ObservingCredentialStore {
        fn load_installation(
            &self,
        ) -> Result<Option<auth::InstallationRecord>, auth::NativeTokenError> {
            Ok(None)
        }

        fn save_credentials(
            &self,
            _: &auth::NativeCredentials,
        ) -> Result<(), auth::NativeTokenError> {
            Ok(())
        }

        fn load_credentials(
            &self,
        ) -> Result<Option<auth::NativeCredentials>, auth::NativeTokenError> {
            self.marked_during_load.store(
                self.runtime_activity.snapshot().session_refresh,
                Ordering::SeqCst,
            );
            Ok(None)
        }

        fn clear_session(&self) -> Result<(), auth::NativeTokenError> {
            Ok(())
        }
    }

    struct FailingDeviceTransport;

    impl auth::NativeDeviceListTransport for FailingDeviceTransport {
        fn list(
            &self,
            _: &str,
            _: &auth::NativeDeviceListRequest,
        ) -> Result<auth::NativeDeviceList, auth::NativeDeviceListError> {
            Err(auth::NativeDeviceListError::Transport(
                "backend-secret".into(),
            ))
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

    #[cfg(target_os = "linux")]
    #[test]
    fn sign_out_handles_each_desktop_client_session() {
        use std::sync::atomic::AtomicUsize;

        fn steps() -> (
            Arc<AtomicUsize>,
            impl FnOnce(),
            impl FnOnce() -> Result<AuthStatus, String> + Send + 'static,
            impl FnOnce(DesktopClientHolder) -> Result<AuthStatus, String> + Send + 'static,
        ) {
            let calls = Arc::new(AtomicUsize::new(0));
            let clear_calls = calls.clone();
            let local_calls = calls.clone();
            let connected_calls = calls.clone();
            (
                calls,
                move || assert_eq!(clear_calls.fetch_add(1, Ordering::SeqCst), 0),
                move || {
                    assert_eq!(local_calls.fetch_add(1, Ordering::SeqCst), 1);
                    Ok(signed_out_status())
                },
                move |_| {
                    assert_eq!(connected_calls.fetch_add(1, Ordering::SeqCst), 1);
                    Ok(signed_out_status())
                },
            )
        }

        let state = AuthState::new(RuntimeActivityRegistry::new());
        let (calls, clear, local, connected) = steps();
        let status = tauri::async_runtime::block_on(sign_out_for_session(
            &state,
            DesktopClientSession::NoSupervisor,
            clear,
            local,
            connected,
        ))
        .unwrap();
        assert!(!status.signed_in);
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        let (calls, clear, local, connected) = steps();
        let status = tauri::async_runtime::block_on(sign_out_for_session(
            &state,
            DesktopClientSession::Connected(DesktopClientHolder::new()),
            clear,
            local,
            connected,
        ))
        .unwrap();
        assert!(!status.signed_in);
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        let (calls, clear, local, connected) = steps();
        let error = tauri::async_runtime::block_on(sign_out_for_session(
            &state,
            DesktopClientSession::Disconnected,
            clear,
            local,
            connected,
        ))
        .unwrap_err();
        assert_eq!(error, "Muniment cannot reach its background service.");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_session_refresh_marks_the_refresh_until_it_returns() {
        let runtime_activity = RuntimeActivityRegistry::new();
        let state = AuthState::new(runtime_activity.clone());
        let store = Arc::new(ObservingCredentialStore::new(runtime_activity.clone()));

        let session = tauri::async_runtime::block_on(state.marked_refresh(store.clone()))
            .unwrap()
            .unwrap();

        assert!(!session.status.signed_in);
        assert!(store.marked_during_load.load(Ordering::SeqCst));
        assert!(!runtime_activity.snapshot().session_refresh);
    }

    #[test]
    fn a_blocking_session_refresh_marks_the_refresh_until_it_returns() {
        let runtime_activity = RuntimeActivityRegistry::new();
        let state = AuthState::new(runtime_activity.clone());
        let store = ObservingCredentialStore::new(runtime_activity.clone());

        let session = state.marked_refresh_blocking(&store).unwrap();

        assert!(!session.status.signed_in);
        assert!(store.marked_during_load.load(Ordering::SeqCst));
        assert!(!runtime_activity.snapshot().session_refresh);
    }

    #[test]
    fn a_failed_authentication_operation_clears_its_mark() {
        let runtime_activity = RuntimeActivityRegistry::new();
        let state = AuthState::new(runtime_activity.clone());

        let outcome = tauri::async_runtime::block_on(sign_in_marked(&state, || {
            Err(auth::NativeSignInError::TokenExchange)
        }));

        assert!(outcome.is_err());
        assert!(!runtime_activity.snapshot().authentication_operation);
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
        let error =
            list_devices(None, &FailingDeviceTransport, "https://api.muniment.ai").unwrap_err();
        assert_eq!(error, "Your devices could not be loaded.");
    }

    #[test]
    fn device_listing_redacts_client_errors() {
        let error = list_devices(
            Some("access-secret"),
            &FailingDeviceTransport,
            "https://api.muniment.ai",
        )
        .unwrap_err();
        assert_eq!(error, "Your devices could not be loaded.");
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
        use uuid::Uuid;

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
        assert_ne!(
            entitlement(&local_app).unwrap_err(),
            background_service_error()
        );
        assert_eq!(devices(&local_app).unwrap_err(), device_list_error());

        let endpoint = std::env::temp_dir().join(format!("muniment-auth-{}.sock", Uuid::now_v7()));
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
                        "groups": []
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
        assert_eq!(
            event_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            json!({"snapshot_version": 7}).to_string()
        );
        assert_eq!(
            server.join().unwrap(),
            [json!("entitlement.snapshot"), json!("device.list")]
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
