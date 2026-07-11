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

use muniment_core::auth::{self, AuthError, AuthStatus, OidcConfig, TokenStore};

use keyring_store::KeyringTokenStore;

/// Default OIDC issuer: the muniment-cloud control plane.
const DEFAULT_ISSUER: &str = "https://api.muniment.ai";
/// Placeholder until the cloud team registers the desktop client — see
/// docs/auth.md "Client registration (cloud side)".
const DEFAULT_CLIENT_ID: &str = "muniment-desktop";
/// `offline_access` asks the control plane for a refresh token.
const SCOPES: &str = "openid profile email offline_access";
/// How long the loopback listener waits for the user to finish in the
/// browser before the sign-in attempt is abandoned.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);
/// Renew shortly before expiry so callers do not receive a nearly-dead token.
const REFRESH_SKEW: Duration = Duration::from_secs(60);

/// The one place issuer and client_id are decided. Env overrides exist for
/// development against a non-default control plane (`MUNIMENT_ISSUER`,
/// `MUNIMENT_CLIENT_ID`).
fn oidc_config() -> OidcConfig {
    OidcConfig {
        issuer: std::env::var("MUNIMENT_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.into()),
        client_id: std::env::var("MUNIMENT_CLIENT_ID").unwrap_or_else(|_| DEFAULT_CLIENT_ID.into()),
        scopes: SCOPES.into(),
    }
}

/// Managed by Tauri; shared across the `auth_*` commands.
pub struct AuthState {
    store: Arc<dyn TokenStore>,
    sign_in_running: Arc<AtomicBool>,
}

impl AuthState {
    pub fn new() -> Self {
        AuthState {
            store: Arc::new(KeyringTokenStore::new()),
            sign_in_running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn store(&self) -> Arc<dyn TokenStore> {
        self.store.clone()
    }
}

pub(crate) fn config() -> OidcConfig {
    oidc_config()
}

pub(crate) fn refresh_skew() -> Duration {
    REFRESH_SKEW
}

/// Run the browser sign-in flow, persist the tokens, and report the new
/// status. Concurrent invocations are rejected while one is in flight.
#[tauri::command]
pub async fn auth_sign_in(state: tauri::State<'_, AuthState>) -> Result<AuthStatus, String> {
    if state.sign_in_running.swap(true, Ordering::SeqCst) {
        return Err("a sign-in is already in progress".into());
    }
    let store = state.store.clone();
    let running = state.sign_in_running.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let result = sign_in_blocking(store.as_ref());
        running.store(false, Ordering::SeqCst);
        result
    })
    .await
    .map_err(|e| format!("sign-in task failed: {e}"))?;
    outcome.map_err(|e| e.to_string())
}

fn sign_in_blocking(store: &dyn TokenStore) -> Result<AuthStatus, AuthError> {
    let cfg = oidc_config();
    let tokens = auth::run_sign_in(&cfg, open_in_browser, SIGN_IN_TIMEOUT)?;
    store.save(&tokens)?;
    auth::status(store)
}

/// Signed-in subject/expiry from the stored tokens; no network.
#[tauri::command]
pub async fn auth_status(state: tauri::State<'_, AuthState>) -> Result<AuthStatus, String> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || auth::status(store.as_ref()))
        .await
        .map_err(|e| format!("status task failed: {e}"))?
        .map_err(|e| e.to_string())
}

/// Return session status after renewing expired or nearly-expired tokens.
#[tauri::command]
pub async fn auth_ensure_fresh(state: tauri::State<'_, AuthState>) -> Result<AuthStatus, String> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        auth::ensure_fresh(store.as_ref(), &oidc_config(), now, REFRESH_SKEW)
    })
    .await
    .map_err(|e| format!("session refresh task failed: {e}"))?
    .map_err(|e| e.to_string())
}

/// Clear stored tokens; best-effort revocation when discovery advertises a
/// revocation endpoint.
#[tauri::command]
pub async fn auth_sign_out(state: tauri::State<'_, AuthState>) -> Result<AuthStatus, String> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        auth::sign_out(store.as_ref(), &oidc_config())?;
        auth::status(store.as_ref())
    })
    .await
    .map_err(|e| format!("sign-out task failed: {e}"))?
    .map_err(|e| e.to_string())
}

/// Hand the authorization URL to the default browser — RFC 8252 §7.2 wants
/// the system browser, not an embedded webview.
fn open_in_browser(url: &str) -> Result<(), AuthError> {
    spawn_browser(url).map_err(|e| AuthError::Config(format!("cannot open system browser: {e}")))
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
