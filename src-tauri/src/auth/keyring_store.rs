//! Auth persistence backed by the platform keychain via the `keyring` crate:
//! macOS Keychain, Windows Credential Manager, and the Linux kernel keyring.

use keyring::Entry;
use muniment_core::auth::{
    AuthError, CoherentNativeCredentialStore, InstallationRecord, InstallationStore,
    NativeCredentialBackend, NativeCredentialKeys, NativeCredentialStore, NativeCredentials,
    NativeRegistrationError, NativeTokenError, TokenSet, TokenStore,
};

const SERVICE: &str = "ai.muniment.desktop";
const USER: &str = "oidc-tokens";
const NATIVE_KEYS: NativeCredentialKeys = NativeCredentialKeys {
    record: "native-credentials",
    legacy_installation: "native-installation",
};

struct PlatformKeychain;

impl NativeCredentialBackend for PlatformKeychain {
    fn get(&self, user: &str) -> Result<Option<String>, String> {
        match Entry::new(SERVICE, user)
            .map_err(|error| error.to_string())?
            .get_password()
        {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    fn set(&self, user: &str, value: &str) -> Result<(), String> {
        Entry::new(SERVICE, user)
            .map_err(|error| error.to_string())?
            .set_password(value)
            .map_err(|error| error.to_string())
    }

    fn delete(&self, user: &str) -> Result<(), String> {
        match Entry::new(SERVICE, user)
            .map_err(|error| error.to_string())?
            .delete_credential()
        {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

pub struct KeyringNativeCredentialStore(CoherentNativeCredentialStore<PlatformKeychain>);

impl KeyringNativeCredentialStore {
    pub fn new() -> Self {
        Self(CoherentNativeCredentialStore::new(
            PlatformKeychain,
            NATIVE_KEYS,
        ))
    }
}

impl InstallationStore for KeyringNativeCredentialStore {
    fn save(&self, installation: &InstallationRecord) -> Result<(), NativeRegistrationError> {
        self.0.save(installation)
    }

    fn load(&self) -> Result<Option<InstallationRecord>, NativeRegistrationError> {
        self.0.load()
    }
}

impl NativeCredentialStore for KeyringNativeCredentialStore {
    fn load_installation(&self) -> Result<Option<InstallationRecord>, NativeTokenError> {
        self.0.load_installation()
    }

    fn save_credentials(&self, credentials: &NativeCredentials) -> Result<(), NativeTokenError> {
        self.0.save_credentials(credentials)
    }

    fn load_credentials(&self) -> Result<Option<NativeCredentials>, NativeTokenError> {
        self.0.load_credentials()
    }

    fn clear_session(&self) -> Result<(), NativeTokenError> {
        self.0.clear_session()
    }
}

pub struct KeyringTokenStore;

impl KeyringTokenStore {
    pub fn new() -> Self {
        KeyringTokenStore
    }

    fn entry() -> Result<Entry, AuthError> {
        Entry::new(SERVICE, USER).map_err(store_err)
    }
}

impl TokenStore for KeyringTokenStore {
    fn save(&self, tokens: &TokenSet) -> Result<(), AuthError> {
        let json = serde_json::to_string(tokens)
            .map_err(|e| AuthError::Store(format!("serializing tokens: {e}")))?;
        Self::entry()?.set_password(&json).map_err(store_err)
    }

    fn load(&self) -> Result<Option<TokenSet>, AuthError> {
        match Self::entry()?.get_password() {
            Ok(json) => serde_json::from_str(&json)
                .map(Some)
                .map_err(|e| AuthError::Store(format!("stored tokens unreadable: {e}"))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(store_err(e)),
        }
    }

    fn clear(&self) -> Result<(), AuthError> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(store_err(e)),
        }
    }
}

fn store_err(e: keyring::Error) -> AuthError {
    AuthError::Store(e.to_string())
}
