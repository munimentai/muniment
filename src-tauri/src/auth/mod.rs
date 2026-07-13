//! Tauri-side auth surface: the `auth_*` commands, client configuration,
//! and the keychain-backed token store. All protocol logic lives in
//! `muniment_core::auth` (see docs/auth.md), which keeps it testable
//! without the GUI stack; this module only wires it to the webview.
//!
//! Nothing here logs or returns token material: commands hand the webview
//! an `AuthStatus` (signed-in flag, subject, expiry) and error strings that
//! `muniment_core::auth::AuthError` guarantees are token-free.

mod keyring_store;

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use muniment_core::auth::{
    self, AuthStatus, BrowserOpenError, NativeCredentialStore, UreqAuthorizationTransport,
    UreqRegistrationTransport, UreqSessionTransport, UreqTokenTransport,
};

use keyring_store::KeyringNativeCredentialStore;

/// Default OIDC issuer: the muniment-cloud control plane.
const DEFAULT_ISSUER: &str = "https://api.muniment.ai";
/// How long the loopback listener waits for the user to finish in the
/// browser before the sign-in attempt is abandoned.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);
/// Renew shortly before expiry so callers do not receive a nearly-dead token.
const REFRESH_SKEW: Duration = Duration::from_secs(60);

fn api_base_url() -> String {
    std::env::var("MUNIMENT_API_BASE_URL")
        .or_else(|_| std::env::var("MUNIMENT_ISSUER"))
        .unwrap_or_else(|_| DEFAULT_ISSUER.into())
}

/// Managed by Tauri; shared across the `auth_*` commands.
pub struct AuthState {
    native_store: Arc<KeyringNativeCredentialStore>,
    sign_in_running: Arc<AtomicBool>,
}

pub(crate) fn fresh_tokens(state: &AuthState) -> Result<muniment_core::auth::TokenSet, String> {
    let result = ensure_native_session(state.native_store.as_ref())?;
    result
        .into_credentials()
        .map(|credentials| credentials.tokens)
        .ok_or_else(|| "Sign in before sending a message.".into())
}

pub(crate) async fn fresh_tokens_async(
    state: &AuthState,
) -> Result<muniment_core::auth::TokenSet, String> {
    let store = state.native_store.clone();
    tauri::async_runtime::spawn_blocking(move || ensure_native_session(store.as_ref()))
        .await
        .map_err(|_| "Sign in before sending a message.".to_string())??
        .into_credentials()
        .map(|credentials| credentials.tokens)
        .ok_or_else(|| "Sign in before sending a message.".into())
}

impl AuthState {
    pub fn new() -> Self {
        AuthState {
            native_store: Arc::new(KeyringNativeCredentialStore::new()),
            sign_in_running: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// Run the browser sign-in flow, persist the tokens, and report the new
/// status. Concurrent invocations are rejected while one is in flight.
#[tauri::command]
pub async fn auth_sign_in(state: tauri::State<'_, AuthState>) -> Result<AuthStatus, String> {
    let permit = SignInPermit::acquire(state.sign_in_running.clone())
        .ok_or_else(|| "a sign-in is already in progress".to_string())?;
    let store = state.native_store.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        sign_in_blocking(store.as_ref())
    })
    .await
    .map_err(|e| format!("sign-in task failed: {e}"))?;
    outcome.map_err(|e| e.to_string())
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
    )
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
    tauri::async_runtime::spawn_blocking(move || auth::native_status(store.as_ref()))
        .await
        .map_err(|e| format!("status task failed: {e}"))?
        .map_err(|e| e.to_string())
}

/// Return session status after renewing expired or nearly-expired tokens.
#[tauri::command]
pub async fn auth_ensure_fresh(state: tauri::State<'_, AuthState>) -> Result<AuthStatus, String> {
    let store = state.native_store.clone();
    tauri::async_runtime::spawn_blocking(move || ensure_native_session(store.as_ref()))
        .await
        .map_err(|e| format!("session refresh task failed: {e}"))?
        .map(|result| result.status)
}

/// Fetch the authoritative native session and expose only its typed,
/// display-only entitlement projection.
#[tauri::command]
pub async fn auth_entitlement_snapshot(
    state: tauri::State<'_, AuthState>,
) -> Result<auth::EntitlementSnapshotView, String> {
    let store = state.native_store.clone();
    tauri::async_runtime::spawn_blocking(move || ensure_native_session(store.as_ref()))
        .await
        .map_err(|e| format!("access task failed: {e}"))??
        .entitlement_snapshot
        .ok_or_else(|| "Sign in to view your access.".into())
}

fn ensure_native_session(
    store: &KeyringNativeCredentialStore,
) -> Result<auth::FreshNativeSession, String> {
    let timeout = Duration::from_secs(30);
    auth::ensure_fresh_native_session(
        store,
        &UreqTokenTransport::new(timeout),
        &UreqSessionTransport::new(timeout),
        &api_base_url(),
        unix_time(),
        REFRESH_SKEW,
    )
    .map_err(|error| error.to_string())
}

/// Clear the local native session while preserving the installation identity.
#[tauri::command]
pub async fn auth_sign_out(state: tauri::State<'_, AuthState>) -> Result<AuthStatus, String> {
    let store = state.native_store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.clear_session().map_err(|error| error.to_string())?;
        auth::native_status(store.as_ref()).map_err(|error| error.to_string())
    })
    .await
    .map_err(|e| format!("sign-out task failed: {e}"))?
    .map_err(|e| e.to_string())
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

    #[test]
    fn concurrent_sign_in_is_rejected_and_guard_releases_on_drop() {
        let running = Arc::new(AtomicBool::new(false));
        let first = SignInPermit::acquire(running.clone()).unwrap();
        assert!(SignInPermit::acquire(running.clone()).is_none());
        drop(first);
        assert!(SignInPermit::acquire(running).is_some());
    }
}
