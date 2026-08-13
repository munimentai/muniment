//! Native session service operations.

use muniment_core::attach::RuntimeActivityRegistry;
use muniment_core::auth::{
    api_base_url, ensure_native_session as ensure_core_native_session, list_native_devices,
    native_status, sign_out_native_session, AuthStatus, EntitlementSnapshotTracker,
    EntitlementSnapshotView, FreshNativeSession, FreshNativeSessionError,
    KeyringNativeCredentialStore, NativeDeviceList, NativeDeviceListError, NativeTokenError,
    UreqNativeDeviceListTransport, UreqRevocationTransport,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntitlementSnapshotResult {
    pub snapshot: EntitlementSnapshotView,
    pub changed_snapshot_version: Option<u64>,
}

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

/// Returns a fresh native session from the platform credential store.
pub fn ensure_native_session(
    runtime_activity: &RuntimeActivityRegistry,
) -> Result<FreshNativeSession, FreshNativeSessionError> {
    let _activity = runtime_activity.mark_session_refresh();
    let now_unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    ensure_core_native_session(
        &KeyringNativeCredentialStore::new(),
        &api_base_url(),
        now_unix_seconds,
    )
}

/// Reads the native session status locally, so this call takes no activity mark.
pub fn session_status() -> Result<AuthStatus, FreshNativeSessionError> {
    let now_unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
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
    let now_unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
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
