//! The desktop's cloud account: native sign-in, the keychain session,
//! devices and phone pairing.

#[cfg(test)]
mod pairing_tests;

pub use muniment_core::attach::EntitlementSnapshotResult;
use muniment_core::attach::{ProtocolError, RuntimeActivityRegistry};
use muniment_desktop_integration::auth::{
    api_base_url, create_pairing_challenge, ensure_native_session as ensure_core_native_session,
    list_native_devices, native_status, read_pairing, revoke_pairing, run_native_sign_in_while,
    sign_out_native_session, AuthStatus, BrowserOpenError, BrowserOpener,
    EntitlementSnapshotTracker, FreshNativeSession, FreshNativeSessionError,
    KeyringNativeCredentialStore, NativeDeviceList, NativeDeviceListError, NativeSignInError,
    NativeTokenError, PairId, PairingChallengeView, PairingError, PairingRevokeView,
    PairingStatusView, UreqAuthorizationTransport, UreqNativeDeviceListTransport,
    UreqPairingTransport, UreqRegistrationTransport, UreqRevocationTransport, UreqTokenTransport,
    PAIR_FILE_NAME,
};
use muniment_runtime_core::host::{AccountService, AccountSession, LinkOpener};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The account the desktop runtime serves: the keychain session and the
/// cloud's native endpoints. It tracks the entitlement snapshot version
/// across reads.
#[derive(Default)]
pub struct DesktopAccount {
    tracker: EntitlementSnapshotTracker,
}

impl AccountService for DesktopAccount {
    fn session(
        &self,
        runtime_activity: &RuntimeActivityRegistry,
    ) -> Result<AccountSession, String> {
        let session = ensure_native_session(runtime_activity).map_err(|error| error.to_string())?;
        let entitlement_snapshot = session.entitlement_snapshot.clone();
        Ok(AccountSession {
            tokens: session
                .into_credentials()
                .map(|credentials| credentials.tokens),
            entitlement_snapshot,
        })
    }

    fn session_status(&self) -> Result<AuthStatus, ProtocolError> {
        session_status().map_err(|_| ProtocolError::persistence_failed())
    }

    fn entitlement_snapshot(
        &self,
        runtime_activity: &RuntimeActivityRegistry,
    ) -> Result<EntitlementSnapshotResult, ProtocolError> {
        entitlement_snapshot(&self.tracker, runtime_activity)
            .map_err(|_| ProtocolError::persistence_failed())
    }

    fn sign_in(
        &self,
        active: &dyn Fn() -> bool,
        open_link: &LinkOpener,
        runtime_activity: &RuntimeActivityRegistry,
    ) -> Result<AuthStatus, ProtocolError> {
        let browser = |url: &str| open_link(url).map_err(|_| BrowserOpenError);
        sign_in(active, &browser, &self.tracker, runtime_activity).map_err(sign_in_protocol_error)
    }

    fn sign_out(
        &self,
        runtime_activity: &RuntimeActivityRegistry,
    ) -> Result<AuthStatus, ProtocolError> {
        sign_out(&self.tracker, runtime_activity).map_err(|_| ProtocolError::persistence_failed())
    }

    fn cloud_api_base(&self) -> Option<String> {
        Some(api_base_url())
    }

    fn list_devices(&self, access_token: &str) -> Result<NativeDeviceList, ProtocolError> {
        list_devices(access_token).map_err(device_list_protocol_error)
    }

    fn pairing_status(
        &self,
        access_token: &str,
        profile_directory: &Path,
    ) -> Result<PairingStatusView, ProtocolError> {
        pairing_status(access_token, profile_directory).map_err(pairing_protocol_error)
    }

    fn create_pairing_challenge(
        &self,
        access_token: &str,
    ) -> Result<PairingChallengeView, ProtocolError> {
        request_pairing_challenge(access_token).map_err(pairing_protocol_error)
    }

    fn revoke_pairing(
        &self,
        access_token: &str,
        pair_id: PairId,
        profile_directory: &Path,
    ) -> Result<PairingRevokeView, ProtocolError> {
        revoke_pairing_pair(access_token, pair_id, profile_directory)
            .map_err(pairing_protocol_error)
    }
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
    pair_id: PairId,
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

fn sign_in_protocol_error(error: NativeSignInError) -> ProtocolError {
    match error {
        NativeSignInError::Authorization(_) => {
            ProtocolError::authorization_failed(error.to_string())
        }
        _ => ProtocolError::persistence_failed_with_reason(error.to_string()),
    }
}

fn device_list_protocol_error(error: NativeDeviceListError) -> ProtocolError {
    match error {
        NativeDeviceListError::CredentialsMissing
        | NativeDeviceListError::HttpStatus(401 | 403) => ProtocolError::unauthorized(),
        NativeDeviceListError::Config(_)
        | NativeDeviceListError::Transport(_)
        | NativeDeviceListError::HttpStatus(_)
        | NativeDeviceListError::MalformedResponse(_) => ProtocolError::persistence_failed(),
    }
}

fn pairing_protocol_error(error: PairingError) -> ProtocolError {
    match error {
        PairingError::CredentialsMissing | PairingError::HttpStatus(401 | 403) => {
            ProtocolError::unauthorized()
        }
        PairingError::HttpStatus(409 | 429) => {
            ProtocolError::persistence_failed_with_reason(error.to_string())
        }
        PairingError::Config(_)
        | PairingError::Transport(_)
        | PairingError::HttpStatus(_)
        | PairingError::MalformedResponse(_) => ProtocolError::persistence_failed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_in_authorization_failure_keeps_its_code_in_the_rpc() {
        let error = sign_in_protocol_error(NativeSignInError::Authorization(
            "HttpStatus status=400 error_code=invalid_device_proof cf_ray=0123456789abcdef-IAD"
                .into(),
        ));
        assert_eq!(
            error.code(),
            muniment_core::attach::ErrorCode::AuthorizationFailed
        );
        assert_eq!(error.details(), Some(&muniment_core::attach::ErrorDetails::RequestReason {
            reason: "native authorization failed: HttpStatus status=400 error_code=invalid_device_proof cf_ray=0123456789abcdef-IAD".into(),
        }));
        assert!(!error.retryable());
    }
}
