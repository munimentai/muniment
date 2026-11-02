//! What the host supplies to the runtime.
//!
//! The runtime names no desktop module. Cloud sign-in, chat grants, credential
//! storage, the OAuth clients of provider sign-in, the page a sign-in's
//! browser lands on and the paths of the host's own executables reach it
//! through one [`RuntimeHost`] value. Each default answers that the cloud is
//! unavailable, so a host without cloud accounts runs locally.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use muniment_core::account::{
    AuthStatus, EntitlementSnapshotView, NativeDeviceList, PairId, PairingChallengeView,
    PairingRevokeView, PairingStatusView, TokenSet,
};
use muniment_core::attach::{EntitlementSnapshotResult, ProtocolError, RuntimeActivityRegistry};
use muniment_core::chat_launch::{ChatGrant, FetchGrantError, FetchReceiptError};
#[cfg(feature = "keyring")]
use muniment_core::keyring::Entry;
pub use muniment_core::model_router::credentials::{
    credential_variable, CredentialRefusal, CredentialStore, FileCredentialStore,
    CREDENTIALS_DIRECTORY_NAME, VARIABLE_PREFIX as CREDENTIAL_VARIABLE_PREFIX,
};
use muniment_core::model_router::native_auth::ProviderClients;
use muniment_core::relay_encryption::RelayKeys;
use muniment_core::sidecar::pi_chat::Receipt;

/// The values and services one runtime process runs with.
#[derive(Clone)]
pub struct RuntimeHost {
    /// Cloud sign-in, session, device and pairing calls.
    pub account: Arc<dyn AccountService>,
    /// Cloud chat grants and receipts.
    pub chat_grants: Arc<dyn ChatGrantService>,
    /// Where the runtime keeps reader source secrets.
    pub credentials: Arc<dyn CredentialStore>,
    /// The desktop executable a same-user peer runs to reach the desktop
    /// attach routes. `None` serves companion sessions only.
    pub expected_desktop_executable: Option<PathBuf>,
    /// The reader sidecar, `muniment-reader`, that network sources run in.
    /// `MUNIMENT_READER_BIN` overrides it. `None` leaves network sources
    /// without a reader.
    pub reader_executable: Option<PathBuf>,
    /// The version the runtime announces on attach and records in the journal:
    /// the host binary's package version.
    pub runtime_version: &'static str,
    /// The OAuth clients the router's own sign-ins and refreshes use. A
    /// provider without one falls back to its environment variables.
    pub provider_clients: ProviderClients,
    /// The page a provider sign-in's browser lands on.
    pub sign_in_page: Arc<SignInPage>,
    /// The executable that answers `secret get <provider>` from
    /// `credentials`, which Pi runs for an endpoint key. `None` leaves
    /// endpoint keys in Pi's `models.json`.
    pub secret_executable: Option<PathBuf>,
}

impl Default for RuntimeHost {
    /// No cloud account, the platform keyring, no desktop peer, no reader
    /// sidecar, this crate's version, no provider clients, a plain sign-in
    /// page and endpoint keys in `models.json`. Without the `keyring` feature the credential store is
    /// [`state_root_credentials`].
    fn default() -> Self {
        Self {
            account: Arc::new(NoCloud),
            chat_grants: Arc::new(NoCloud),
            #[cfg(feature = "keyring")]
            credentials: platform_credentials(),
            #[cfg(not(feature = "keyring"))]
            credentials: Arc::new(state_root_credentials()),
            expected_desktop_executable: None,
            reader_executable: None,
            runtime_version: env!("CARGO_PKG_VERSION"),
            provider_clients: ProviderClients::default(),
            sign_in_page: Arc::new(plain_sign_in_page),
            secret_executable: None,
        }
    }
}

/// Renders the page a provider sign-in's browser lands on: `Some` with the
/// provider's name after a redirect that carried a code, `None` without one.
pub type SignInPage = dyn Fn(Option<&str>) -> String + Send + Sync;

/// A sign-in page with no branding.
pub fn plain_sign_in_page(provider: Option<&str>) -> String {
    let (heading, body) = match provider {
        Some(provider) => (
            "Signed in".to_owned(),
            format!("{provider} is connected. This window can close."),
        ),
        None => (
            "Sign-in did not finish".to_owned(),
            "No code reached Muniment from this page. Return and try again.".to_owned(),
        ),
    };
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>Muniment</title>\n</head>\n<body>\n<h1>{heading}</h1>\n<p>{body}</p>\n</body>\n</html>\n"
    )
}

/// The session an account answers.
pub struct AccountSession {
    /// The bearer tokens, or `None` when no account is signed in.
    pub tokens: Option<TokenSet>,
    /// The display-only entitlement projection of the session.
    pub entitlement_snapshot: Option<EntitlementSnapshotView>,
}

/// The browser could not open a sign-in link.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinkOpenError;

impl std::fmt::Display for LinkOpenError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("the browser could not open the link")
    }
}

impl std::error::Error for LinkOpenError {}

/// Shows the user a sign-in link.
pub type LinkOpener = dyn Fn(&str) -> Result<(), LinkOpenError> + Send + Sync;

/// Cloud account calls. The runtime holds the activity mark a call needs and
/// passes the registry for the host to mark its own operations.
pub trait AccountService: Send + Sync {
    /// Answers the signed-in session, refreshed when it is near expiry. The
    /// error names why no session could be read.
    fn session(
        &self,
        runtime_activity: &RuntimeActivityRegistry,
    ) -> Result<AccountSession, String> {
        let _ = runtime_activity;
        Ok(AccountSession {
            tokens: None,
            entitlement_snapshot: None,
        })
    }

    /// Reads the session status without a network call.
    fn session_status(&self) -> Result<AuthStatus, ProtocolError> {
        Ok(AuthStatus {
            signed_in: false,
            subject: None,
            expires_at: None,
        })
    }

    /// Answers the entitlement projection and whether its version changed.
    fn entitlement_snapshot(
        &self,
        runtime_activity: &RuntimeActivityRegistry,
    ) -> Result<EntitlementSnapshotResult, ProtocolError> {
        let _ = runtime_activity;
        Err(ProtocolError::persistence_failed())
    }

    /// Runs the browser sign-in while `active` answers true. `open_link`
    /// shows the user the sign-in link.
    fn sign_in(
        &self,
        active: &dyn Fn() -> bool,
        open_link: &LinkOpener,
        runtime_activity: &RuntimeActivityRegistry,
    ) -> Result<AuthStatus, ProtocolError> {
        let _ = (active, open_link, runtime_activity);
        Err(ProtocolError::unsupported_operation())
    }

    /// Revokes the session and clears it locally.
    fn sign_out(
        &self,
        runtime_activity: &RuntimeActivityRegistry,
    ) -> Result<AuthStatus, ProtocolError> {
        let _ = runtime_activity;
        Err(ProtocolError::unsupported_operation())
    }

    /// The base URL of the cloud API this host signs in to, or none for a
    /// host without a cloud account.
    fn cloud_api_base(&self) -> Option<String> {
        None
    }

    /// Lists the account's native installations.
    fn list_devices(&self, access_token: &str) -> Result<NativeDeviceList, ProtocolError> {
        let _ = access_token;
        Err(ProtocolError::unsupported_operation())
    }

    /// Reads the current phone pair and keeps its peer identity under the profile.
    fn pairing_status(
        &self,
        access_token: &str,
        profile_directory: &Path,
    ) -> Result<PairingStatusView, ProtocolError> {
        let _ = (access_token, profile_directory);
        Err(ProtocolError::unsupported_operation())
    }

    /// Requests a phone pairing challenge.
    fn create_pairing_challenge(
        &self,
        access_token: &str,
    ) -> Result<PairingChallengeView, ProtocolError> {
        let _ = access_token;
        Err(ProtocolError::unsupported_operation())
    }

    /// Revokes one phone pair and clears its peer identity under the profile.
    fn revoke_pairing(
        &self,
        access_token: &str,
        pair_id: PairId,
        profile_directory: &Path,
    ) -> Result<PairingRevokeView, ProtocolError> {
        let _ = (access_token, pair_id, profile_directory);
        Err(ProtocolError::unsupported_operation())
    }

    /// The relay keys of the phone pair stored under the profile, read
    /// without a network call, or none. A host that answers keys gets a
    /// relay publisher while an account is signed in.
    fn relay_keys(&self, profile_directory: &Path) -> Option<RelayKeys> {
        let _ = profile_directory;
        None
    }
}

/// Cloud chat grant calls.
pub trait ChatGrantService: Send + Sync {
    /// Fetches a chat grant for the signed-in account.
    fn fetch_grant(&self, access_token: &str) -> Result<ChatGrant, FetchGrantError> {
        let _ = access_token;
        Err(FetchGrantError::Unavailable)
    }

    /// Inspects the cloud session and answers the bearer to continue with.
    fn inspect_session(&self, access_token: &str) -> Result<String, FetchGrantError> {
        let _ = access_token;
        Err(FetchGrantError::Unavailable)
    }

    /// Fetches the receipt for a finished run on a cloud grant.
    fn fetch_receipt(
        &self,
        grant: &ChatGrant,
        access_token: &str,
        run_id: &str,
    ) -> Result<Receipt, FetchReceiptError> {
        let _ = (grant, access_token, run_id);
        Err(FetchReceiptError)
    }
}

/// The cloud API and the signed-in account's bearer token, or none when the
/// host has no cloud or no one is signed in.
pub fn cloud_session(
    host: &RuntimeHost,
    runtime_activity: &RuntimeActivityRegistry,
) -> Option<muniment_core::shared_models::CloudSession> {
    let base_url = host.account.cloud_api_base()?;
    let bearer = host
        .account
        .session(runtime_activity)
        .ok()?
        .tokens?
        .access_token;
    (!bearer.is_empty()).then_some(muniment_core::shared_models::CloudSession { base_url, bearer })
}

/// A host with no cloud account.
pub struct NoCloud;

impl AccountService for NoCloud {}

impl ChatGrantService for NoCloud {}

/// The platform keychain, or in a test run, a test build or a process whose
/// state lives in a test root, a file store in a temporary directory, so no
/// test and no copy of a test binary reaches the owner's real keychain.
#[cfg(feature = "keyring")]
pub fn platform_credentials() -> Arc<dyn CredentialStore> {
    if muniment_core::state_root::keychain_allowed() {
        // One store per process, so every host shares what it remembers of
        // the keychain's refusals.
        // The runtime's start forgets the declines an earlier start shared.
        static KEYCHAIN: std::sync::OnceLock<Arc<KeyringCredentialStore>> =
            std::sync::OnceLock::new();
        KEYCHAIN
            .get_or_init(|| {
                let store = KeyringCredentialStore::sharing_declines();
                if let Some(shared) = &store.shared {
                    shared.clear();
                }
                Arc::new(store)
            })
            .clone()
    } else {
        Arc::new(FileCredentialStore::new(
            muniment_core::state_root::test_credentials_directory(),
        ))
    }
}

/// The store the `secret get` command reads when no runtime answers it on
/// its socket: the platform keychain, holding back a service the runtime or
/// an earlier command saw the keychain decline, so a declined endpoint key
/// raises the prompt once until the person retries or the runtime starts
/// again. In a test run it is [`platform_credentials`]'s file store.
#[cfg(feature = "keyring")]
pub fn secret_command_credentials() -> Arc<dyn CredentialStore> {
    if muniment_core::state_root::keychain_allowed() {
        Arc::new(KeyringCredentialStore::sharing_declines())
    } else {
        platform_credentials()
    }
}

/// The folder under the state root where the runtime and the commands it
/// starts share the keychain's declines.
pub const KEYCHAIN_DECLINES_DIRECTORY_NAME: &str = "keychain-declines";

/// The platform keychain through the system keyring. A test run that opens
/// it panics unless it is marked as a keychain test.
///
/// The store remembers each entry the keychain refused. A declined prompt
/// holds until the person retries or the process ends, and any other failure
/// holds for a backoff, so a refused entry never raises the prompt in a loop.
/// A write never replaces an entry whose read the keychain refused. A store
/// that shares its declines also holds back a service another process saw
/// the keychain decline.
#[cfg(feature = "keyring")]
#[derive(Default)]
pub struct KeyringCredentialStore {
    refusals: muniment_core::model_router::credentials::RefusalMemory,
    shared: Option<muniment_core::model_router::credentials::SharedDeclines>,
}

#[cfg(feature = "keyring")]
impl KeyringCredentialStore {
    /// A store that shares its declines under the state root.
    fn sharing_declines() -> Self {
        Self::sharing_declines_in(
            muniment_core::state_root::state_directory()
                .map(|state| state.join(KEYCHAIN_DECLINES_DIRECTORY_NAME)),
        )
    }

    fn sharing_declines_in(directory: Option<PathBuf>) -> Self {
        Self {
            refusals: Default::default(),
            shared: directory.map(muniment_core::model_router::credentials::SharedDeclines::new),
        }
    }

    fn ask(
        &self,
        service: &str,
        name: &str,
        get: impl FnOnce() -> Result<String, muniment_core::keyring::Error>,
    ) -> Result<Option<String>, CredentialRefusal> {
        if let Some(refusal) = self.refusals.held(service, name, std::time::Instant::now()) {
            return Err(refusal);
        }
        if let Some(refusal) = self.shared.as_ref().and_then(|shared| shared.held(service)) {
            return Err(refusal);
        }
        match get() {
            Ok(secret) => {
                self.refusals.answered(service, name);
                Ok(Some(secret))
            }
            Err(muniment_core::keyring::Error::NoEntry) => {
                self.refusals.answered(service, name);
                Ok(None)
            }
            Err(error) => {
                let refusal = muniment_core::keychain_refusal::keyring_refusal(&error);
                muniment_core::runtime_eprintln!(
                    "muniment-runtime: the keychain refused {service}: {}",
                    refusal.message()
                );
                self.refusals
                    .note(service, name, &refusal, std::time::Instant::now());
                if let Some(shared) = &self.shared {
                    shared.note(service, &refusal);
                }
                Err(refusal)
            }
        }
    }

    /// Deletes an entry through `delete`, which never reads its secret, so
    /// a removal raises no prompt for a key the person declined.
    fn delete(
        &self,
        service: &str,
        name: &str,
        delete: impl FnOnce() -> Result<(), muniment_core::keyring::Error>,
    ) -> Result<(), String> {
        match delete() {
            Ok(()) | Err(muniment_core::keyring::Error::NoEntry) => {
                self.refusals.answered(service, name);
                Ok(())
            }
            Err(error) => Err(format!(
                "The keychain kept the secret: {}.",
                error.to_string().trim_end_matches('.')
            )),
        }
    }

    fn put(
        &self,
        service: &str,
        name: &str,
        set: impl FnOnce() -> Result<(), muniment_core::keyring::Error>,
    ) -> Result<(), String> {
        if self.refusals.refused(service, name) {
            return Err(
                "The keychain refused this secret, so it keeps the one it holds. Choose Try again in Settings → Machines."
                    .into(),
            );
        }
        set().map_err(|error| {
            format!(
                "The keychain refused the secret: {}.",
                error.to_string().trim_end_matches('.')
            )
        })
    }
}

#[cfg(feature = "keyring")]
impl CredentialStore for KeyringCredentialStore {
    fn read(&self, service: &str, name: &str) -> Option<String> {
        self.lookup(service, name).ok().flatten()
    }

    fn lookup(&self, service: &str, name: &str) -> Result<Option<String>, CredentialRefusal> {
        muniment_core::state_root::guard_keychain(service);
        self.ask(service, name, || {
            Entry::new(service, name).and_then(|entry| entry.get_password())
        })
    }

    fn write(&self, service: &str, name: &str, secret: &str) -> Result<(), String> {
        muniment_core::state_root::guard_keychain(service);
        self.put(service, name, || {
            Entry::new(service, name).and_then(|entry| entry.set_password(secret))
        })
    }

    fn remove(&self, service: &str, name: &str) -> Result<(), String> {
        muniment_core::state_root::guard_keychain(service);
        self.delete(service, name, || {
            muniment_core::keychain_refusal::delete_without_reading(service, name)
        })
    }

    fn refusals(&self) -> Vec<muniment_core::model_router::credentials::RefusedEntry> {
        let mut refused = self.refusals.list();
        if let Some(shared) = &self.shared {
            refused.extend(shared.list());
        }
        refused
    }

    fn retry_refused(&self) {
        self.refusals.clear();
        if let Some(shared) = &self.shared {
            shared.clear();
        }
    }
}

/// The file credential store under the `credentials` directory of the state
/// root. When no state root resolves, the store reads the environment alone and
/// refuses every write.
pub fn state_root_credentials() -> FileCredentialStore {
    match muniment_core::state_root::state_directory() {
        Some(state) => FileCredentialStore::new(state.join(CREDENTIALS_DIRECTORY_NAME)),
        None => FileCredentialStore::environment_only(),
    }
}

#[cfg(all(test, feature = "keyring"))]
mod keyring_store_tests {
    use super::*;
    use std::cell::Cell;

    fn declined() -> muniment_core::keyring::Error {
        muniment_core::keyring::Error::NoStorageAccess(Box::new(std::io::Error::other("declined")))
    }

    #[cfg(target_os = "macos")]
    fn user_canceled() -> muniment_core::keyring::Error {
        muniment_core::keychain_refusal::status_error(-128)
    }

    #[cfg(not(target_os = "macos"))]
    fn user_canceled() -> muniment_core::keyring::Error {
        declined()
    }

    #[test]
    fn a_refused_entry_asks_the_keychain_once_and_is_never_written_over() {
        let store = KeyringCredentialStore::default();
        let asked = Cell::new(0);
        let get = || {
            asked.set(asked.get() + 1);
            Err(user_canceled())
        };
        assert!(store.ask("ai.muniment.hooks", "hook-1", get).is_err());
        for _ in 0..10 {
            assert!(store
                .ask("ai.muniment.hooks", "hook-1", || {
                    asked.set(asked.get() + 1);
                    Err(user_canceled())
                })
                .is_err());
        }
        assert_eq!(asked.get(), 1);
        let wrote = Cell::new(false);
        assert!(store
            .put("ai.muniment.hooks", "hook-1", || {
                wrote.set(true);
                Ok(())
            })
            .is_err());
        assert!(!wrote.get());
        assert_eq!(store.refusals().len(), 1);
        // The decline holds back the service's other entries, and no other
        // service.
        assert!(store
            .ask("ai.muniment.hooks", "hook-2", || panic!("held back"))
            .is_err());
        assert_eq!(
            store.ask("ai.muniment.reader", "source", || {
                Err(muniment_core::keyring::Error::NoEntry)
            }),
            Ok(None)
        );
        // A retry asks once more, and an answer clears the refusal.
        store.retry_refused();
        assert_eq!(
            store.ask("ai.muniment.hooks", "hook-1", || Ok("secret".into())),
            Ok(Some("secret".into()))
        );
        assert!(store.refusals().is_empty());
        assert!(store.put("ai.muniment.hooks", "hook-1", || Ok(())).is_ok());
        let _ = declined();
    }

    #[test]
    fn a_removal_never_reads_the_secret_and_clears_a_held_decline() {
        let store = KeyringCredentialStore::default();
        assert!(store
            .ask("ai.muniment.endpoints", "custom-live27", || Err(
                user_canceled()
            ))
            .is_err());
        // The delete runs with the decline held, and it reads no secret.
        let deleted = Cell::new(false);
        assert_eq!(
            store.delete("ai.muniment.endpoints", "custom-live27", || {
                deleted.set(true);
                Ok(())
            }),
            Ok(())
        );
        assert!(deleted.get());
        assert!(store.refusals.list().is_empty());
        assert_eq!(
            store.delete("ai.muniment.endpoints", "custom-gone", || {
                Err(muniment_core::keyring::Error::NoEntry)
            }),
            Ok(())
        );
        let kept = store
            .delete("ai.muniment.endpoints", "custom-live27", || {
                Err(user_canceled())
            })
            .unwrap_err();
        assert!(kept.starts_with("The keychain kept the secret: "), "{kept}");
        assert!(!kept.ends_with(".."), "{kept}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_declined_endpoint_key_asks_the_keychain_once_across_secret_commands_until_a_retry() {
        // Each `secret get` Pi runs is a new process with a new store, and
        // they share the runtime's folder of declines, a temporary one here.
        let directory = muniment_core::state_root::test_root()
            .join(format!("keychain-declines-{}", uuid::Uuid::new_v4()));
        let runtime = KeyringCredentialStore::sharing_declines_in(Some(directory.clone()));
        let command = || KeyringCredentialStore::sharing_declines_in(Some(directory.clone()));
        let asked = Cell::new(0);
        let ask = |answer: fn() -> Result<String, muniment_core::keyring::Error>| {
            command().ask(
                crate::models::endpoint_secrets::SERVICE,
                "custom-proxy",
                || {
                    asked.set(asked.get() + 1);
                    answer()
                },
            )
        };
        for _ in 0..10 {
            assert!(ask(|| Err(user_canceled())).is_err());
        }
        assert_eq!(asked.get(), 1);
        // The runtime names the decline for Settings, and holds the service
        // back itself.
        let refused = runtime.refusals();
        assert_eq!(refused.len(), 1);
        assert_eq!(refused[0].service, crate::models::endpoint_secrets::SERVICE);
        assert!(matches!(refused[0].refusal, CredentialRefusal::Declined(_)));
        assert!(runtime
            .ask(crate::models::endpoint_secrets::SERVICE, "other", || {
                panic!("held back")
            })
            .is_err());
        // A retry asks once more, and the answer reaches Pi.
        runtime.retry_refused();
        assert_eq!(ask(|| Ok("sk-key".into())), Ok(Some("sk-key".into())));
        assert_eq!(asked.get(), 2);
        assert!(runtime.refusals().is_empty());
        // A new decline holds again, until the runtime's next start.
        assert!(ask(|| Err(user_canceled())).is_err());
        assert!(ask(|| Err(user_canceled())).is_err());
        assert_eq!(asked.get(), 3);
        KeyringCredentialStore::sharing_declines_in(Some(directory.clone()))
            .shared
            .unwrap()
            .clear();
        assert_eq!(ask(|| Ok("sk-key".into())), Ok(Some("sk-key".into())));
        assert_eq!(asked.get(), 4);
        let _ = std::fs::remove_dir_all(directory);
    }
}

#[cfg(all(test, feature = "keyring"))]
mod test_build_store_tests {
    use super::*;

    #[test]
    fn a_test_build_keeps_secrets_in_files_and_never_in_the_keychain() {
        assert!(muniment_core::state_root::is_test_build());
        let store = platform_credentials();
        let name = format!("entry-{}", uuid::Uuid::new_v4());
        store.write("test.build", &name, "sk_test").unwrap();
        assert_eq!(store.read("test.build", &name).as_deref(), Some("sk_test"));
        assert!(muniment_core::state_root::test_credentials_directory()
            .join("test.build")
            .join(&name)
            .is_file());
        store.remove("test.build", &name).unwrap();
    }
}
