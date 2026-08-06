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

use muniment_core::auth::{
    self, AuthStatus, BrowserOpenError, EntitlementSnapshotTracker, KeyringNativeCredentialStore,
    NativeCredentialStore, UreqAuthorizationTransport, UreqNativeDeviceListTransport,
    UreqRegistrationTransport, UreqRevocationTransport, UreqTokenTransport,
};
use serde::Serialize;
use tauri::Emitter;

/// Default OIDC issuer: the muniment-cloud control plane.
const DEFAULT_ISSUER: &str = "https://api.muniment.ai";
/// How long the loopback listener waits for the user to finish in the
/// browser before the sign-in attempt is abandoned.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);
fn api_base_url() -> String {
    std::env::var("MUNIMENT_API_BASE_URL")
        .or_else(|_| std::env::var("MUNIMENT_ISSUER"))
        .unwrap_or_else(|_| DEFAULT_ISSUER.into())
}

/// Managed by Tauri; shared across the `auth_*` commands.
pub struct AuthState {
    native_store: Arc<KeyringNativeCredentialStore>,
    sign_in_running: Arc<AtomicBool>,
    entitlement_snapshot_tracker: EntitlementSnapshotTracker,
}

#[derive(Clone, Copy, Serialize)]
struct EntitlementChanged {
    snapshot_version: u64,
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
    let result = ensure_native_session(state.native_store.as_ref())?;
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
    let store = state.native_store.clone();
    let result =
        tauri::async_runtime::spawn_blocking(move || ensure_native_session(store.as_ref()))
            .await
            .map_err(|_| "Sign in before sending a message.".to_string())??;
    observe_snapshot(state, app, &result)?;
    result
        .into_credentials()
        .map(|credentials| credentials.tokens)
        .ok_or_else(|| "Sign in before sending a message.".into())
}

impl AuthState {
    pub fn new() -> Self {
        AuthState {
            native_store: Arc::new(KeyringNativeCredentialStore::new()),
            sign_in_running: Arc::new(AtomicBool::new(false)),
            entitlement_snapshot_tracker: EntitlementSnapshotTracker::new(),
        }
    }
}

/// Run the browser sign-in flow, persist the tokens, and report the new
/// status. Concurrent invocations are rejected while one is in flight.
#[tauri::command]
pub async fn auth_sign_in(
    app: tauri::AppHandle,
    state: tauri::State<'_, AuthState>,
) -> Result<AuthStatus, String> {
    let permit = SignInPermit::acquire(state.sign_in_running.clone())
        .ok_or_else(|| "a sign-in is already in progress".to_string())?;
    let store = state.native_store.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        sign_in_blocking(store.as_ref(), &app)
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
        &api_base_url(),
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
#[tauri::command]
pub async fn auth_entitlement_snapshot(
    app: tauri::AppHandle,
    state: tauri::State<'_, AuthState>,
) -> Result<auth::EntitlementSnapshotView, String> {
    let store = state.native_store.clone();
    let result =
        tauri::async_runtime::spawn_blocking(move || ensure_native_session(store.as_ref()))
            .await
            .map_err(|e| format!("access task failed: {e}"))??;
    observe_snapshot(&state, &app, &result)?;
    result
        .entitlement_snapshot
        .ok_or_else(|| "Sign in to view your access.".into())
}

/// List display-only metadata for this account's native installations.
#[tauri::command]
pub async fn auth_devices(
    app: tauri::AppHandle,
    state: tauri::State<'_, AuthState>,
) -> Result<Vec<auth::NativeDevice>, String> {
    let store = state.native_store.clone();
    let session =
        tauri::async_runtime::spawn_blocking(move || ensure_native_session(store.as_ref()))
            .await
            .map_err(|_| device_list_error())?
            .map_err(|_| device_list_error())?;
    observe_snapshot(&state, &app, &session).map_err(|_| device_list_error())?;
    let credentials = session.into_credentials().ok_or_else(device_list_error)?;
    tauri::async_runtime::spawn_blocking(move || {
        list_devices(
            Some(&credentials.tokens.access_token),
            &UreqNativeDeviceListTransport::new(Duration::from_secs(30)),
            &api_base_url(),
        )
    })
    .await
    .map_err(|_| device_list_error())?
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
    store: &KeyringNativeCredentialStore,
) -> Result<auth::FreshNativeSession, String> {
    auth::ensure_native_session(store, &api_base_url(), unix_time())
        .map_err(|error| error.to_string())
}

/// Clear the local native session while preserving the installation identity.
#[tauri::command]
pub async fn auth_sign_out(
    state: tauri::State<'_, AuthState>,
    attach_state: tauri::State<'_, crate::attach_service::AttachCompanionState>,
) -> Result<AuthStatus, String> {
    attach_state.clear_workspace();
    let store = state.native_store.clone();
    let status = tauri::async_runtime::spawn_blocking(move || {
        auth::sign_out_native_session(
            store.as_ref(),
            &UreqRevocationTransport::new(Duration::from_secs(2)),
            &api_base_url(),
        )
        .map_err(|error| error.to_string())?;
        auth::native_status(store.as_ref(), unix_time()).map_err(|error| error.to_string())
    })
    .await
    .map_err(|e| format!("sign-out task failed: {e}"))?
    .map_err(|e| e.to_string())?;
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
}
