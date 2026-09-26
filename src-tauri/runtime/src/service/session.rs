//! Native session service operations.

pub use muniment_core::attach::EntitlementSnapshotResult;
use muniment_core::attach::RuntimeActivityRegistry;
use muniment_core::auth::{
    api_base_url, create_pairing_challenge, ensure_native_session as ensure_core_native_session,
    list_native_devices, native_status, read_pairing, revoke_pairing, run_native_sign_in_while,
    sign_out_native_session, AuthStatus, BrowserOpener, EntitlementSnapshotTracker,
    FreshNativeSession, FreshNativeSessionError, KeyringNativeCredentialStore, NativeDeviceList,
    NativeDeviceListError, NativeSignInError, NativeTokenError, PairingChallengeView, PairingError,
    PairingRevokeView, PairingStatusView, UreqAuthorizationTransport,
    UreqNativeDeviceListTransport, UreqPairingTransport, UreqRegistrationTransport,
    UreqRevocationTransport, UreqTokenTransport, PAIR_FILE_NAME,
};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntitlementSnapshotError {
    Session(FreshNativeSessionError),
    Missing,
}

impl std::fmt::Display for EntitlementSnapshotError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Session(error) => write!(formatter, "{error}"),
            Self::Missing => formatter.write_str("native session has no entitlement snapshot"),
        }
    }
}

impl std::error::Error for EntitlementSnapshotError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignOutError {
    LocalClear(NativeTokenError),
    Status(FreshNativeSessionError),
}

impl std::fmt::Display for SignOutError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LocalClear(error) => write!(formatter, "{error}"),
            Self::Status(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for SignOutError {}

/// Runs the native browser sign-in flow and stores the resulting session.
pub fn sign_in(
    active: &dyn Fn() -> bool,
    browser: &dyn BrowserOpener,
    tracker: &EntitlementSnapshotTracker,
    runtime_activity: &RuntimeActivityRegistry,
) -> Result<AuthStatus, NativeSignInError> {
    let _activity = runtime_activity.mark_authentication_operation();
    let network_timeout = Duration::from_secs(30);
    let status = run_native_sign_in_while(
        &KeyringNativeCredentialStore::new(),
        &UreqRegistrationTransport::new(network_timeout),
        &UreqAuthorizationTransport::new(network_timeout),
        &UreqTokenTransport::new(network_timeout),
        browser,
        &api_base_url(),
        &unix_time,
        Duration::from_secs(300),
        &|delay| {
            let deadline = std::time::Instant::now() + delay;
            while active() && std::time::Instant::now() < deadline {
                std::thread::sleep(
                    Duration::from_millis(25)
                        .min(deadline.saturating_duration_since(std::time::Instant::now())),
                );
            }
        },
        active,
    )?;
    tracker.clear();
    Ok(status)
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// Returns a fresh native session from the platform credential store.
pub fn ensure_native_session(
    runtime_activity: &RuntimeActivityRegistry,
) -> Result<FreshNativeSession, FreshNativeSessionError> {
    let _activity = runtime_activity.mark_session_refresh();
    let now_unix_seconds = unix_time();
    ensure_core_native_session(
        &KeyringNativeCredentialStore::new(),
        &api_base_url(),
        now_unix_seconds,
    )
}

/// Reads the native session status locally, so this call takes no activity mark.
pub fn session_status() -> Result<AuthStatus, FreshNativeSessionError> {
    let now_unix_seconds = unix_time();
    native_status(&KeyringNativeCredentialStore::new(), now_unix_seconds)
}

/// Revokes the server session and clears the local native session.
pub fn sign_out(
    tracker: &EntitlementSnapshotTracker,
    runtime_activity: &RuntimeActivityRegistry,
) -> Result<AuthStatus, SignOutError> {
    let _activity = runtime_activity.mark_authentication_operation();
    let store = KeyringNativeCredentialStore::new();
    sign_out_native_session(
        &store,
        &UreqRevocationTransport::new(Duration::from_secs(2)),
        &api_base_url(),
    )
    .map_err(SignOutError::LocalClear)?;
    let now_unix_seconds = unix_time();
    let status = native_status(&store, now_unix_seconds).map_err(SignOutError::Status)?;
    tracker.clear();
    Ok(status)
}

/// Returns the display-only entitlement projection and reports a version change.
pub fn entitlement_snapshot(
    tracker: &EntitlementSnapshotTracker,
    runtime_activity: &RuntimeActivityRegistry,
) -> Result<EntitlementSnapshotResult, EntitlementSnapshotError> {
    let session =
        ensure_native_session(runtime_activity).map_err(EntitlementSnapshotError::Session)?;
    let next = session
        .entitlement_snapshot
        .as_ref()
        .map(|snapshot| snapshot.snapshot_version);
    let changed_snapshot_version = tracker.observe(next);
    let snapshot = session
        .entitlement_snapshot
        .ok_or(EntitlementSnapshotError::Missing)?;
    Ok(EntitlementSnapshotResult {
        snapshot,
        changed_snapshot_version,
    })
}

/// Lists display-only metadata for one account's native installations.
pub fn list_devices(access_token: &str) -> Result<NativeDeviceList, NativeDeviceListError> {
    list_native_devices(
        &UreqNativeDeviceListTransport::new(Duration::from_secs(30)),
        &api_base_url(),
        access_token,
    )
}

fn pairing_transport() -> UreqPairingTransport {
    UreqPairingTransport::new(Duration::from_secs(30))
}

fn pair_store(profile_directory: &Path) -> std::path::PathBuf {
    profile_directory.join(PAIR_FILE_NAME)
}

/// Requests a QR pairing challenge for the signed-in desktop installation.
pub fn request_pairing_challenge(access_token: &str) -> Result<PairingChallengeView, PairingError> {
    create_pairing_challenge(&pairing_transport(), &api_base_url(), access_token)
}

/// Reads the current pair and stores or clears the local peer identity.
pub fn pairing_status(
    access_token: &str,
    profile_directory: &Path,
) -> Result<PairingStatusView, PairingError> {
    read_pairing(
        &pairing_transport(),
        &api_base_url(),
        access_token,
        &pair_store(profile_directory),
    )
}

/// Revokes the named pair and clears matching stored peer identity.
pub fn revoke_pairing_pair(
    access_token: &str,
    pair_id: uuid::Uuid,
    profile_directory: &Path,
) -> Result<PairingRevokeView, PairingError> {
    revoke_pairing(
        &pairing_transport(),
        &api_base_url(),
        access_token,
        pair_id,
        &pair_store(profile_directory),
    )
}
