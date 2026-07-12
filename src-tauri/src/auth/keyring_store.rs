//! Auth persistence backed by the platform keychain via the `keyring` crate:
//! macOS Keychain, Windows Credential Manager, Secret Service on Linux.
//! Native installation and session state share one record; the legacy OIDC
//! store remains until native command wiring replaces it in the next slice.

use keyring::Entry;
use muniment_core::auth::{
    AuthError, InstallationRecord, InstallationStore, NativeCredentialStore, NativeCredentials,
    NativeRegistrationError, NativeTokenError, TokenSet, TokenStore,
};
use serde::{Deserialize, Serialize};

const SERVICE: &str = "ai.muniment.desktop";
const USER: &str = "oidc-tokens";
const INSTALLATION_USER: &str = "native-installation";
const NATIVE_CREDENTIAL_USER: &str = "native-credentials";
const NATIVE_RECORD_VERSION: u8 = 1;

/// Installation and session are encoded into one keychain value. Replacing a
/// keychain value is the only publish operation, so readers cannot observe a
/// rotated challenge paired with the previous token set.
#[derive(Serialize, Deserialize)]
struct NativeKeychainRecord {
    version: u8,
    installation: InstallationRecord,
    session: Option<NativeKeychainSession>,
}

#[derive(Serialize, Deserialize)]
struct NativeKeychainSession {
    tokens: TokenSet,
    refresh_expires_at: u64,
}

trait KeychainBackend: Send + Sync {
    fn get(&self, user: &str) -> Result<Option<String>, String>;
    fn set(&self, user: &str, value: &str) -> Result<(), String>;
    fn delete(&self, user: &str) -> Result<(), String>;
}

struct PlatformKeychain;

impl KeychainBackend for PlatformKeychain {
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

pub struct KeyringNativeCredentialStore {
    backend: Box<dyn KeychainBackend>,
}

impl KeyringNativeCredentialStore {
    pub fn new() -> Self {
        Self {
            backend: Box::new(PlatformKeychain),
        }
    }
}

impl KeyringNativeCredentialStore {
    fn save_record(&self, record: &NativeKeychainRecord) -> Result<(), String> {
        let json = serde_json::to_string(record)
            .map_err(|_| "could not serialize native credential record".to_owned())?;
        self.backend.set(NATIVE_CREDENTIAL_USER, &json)
    }

    fn load_record(&self) -> Result<Option<NativeKeychainRecord>, String> {
        if let Some(json) = self.backend.get(NATIVE_CREDENTIAL_USER)? {
            let record: NativeKeychainRecord = serde_json::from_str(&json)
                .map_err(|_| "stored native credential record is unreadable".to_owned())?;
            if record.version != NATIVE_RECORD_VERSION {
                return Err("stored native credential record has an unsupported version".into());
            }
            return Ok(Some(record));
        }
        self.migrate_split_record()
    }

    fn migrate_split_record(&self) -> Result<Option<NativeKeychainRecord>, String> {
        let Some(installation_json) = self.backend.get(INSTALLATION_USER)? else {
            return Ok(None);
        };
        let installation: InstallationRecord = serde_json::from_str(&installation_json)
            .map_err(|_| "stored installation is unreadable".to_owned())?;
        let session = self
            .backend
            .get(USER)?
            .map(|json| {
                let tokens: TokenSet = serde_json::from_str(&json)
                    .map_err(|_| "stored session is unreadable".to_owned())?;
                // The legacy token record did not retain the refresh expiry.
                // Conservatively prevent refresh beyond the known access expiry.
                let refresh_expires_at = tokens.expires_at.unwrap_or(0);
                Ok::<NativeKeychainSession, String>(NativeKeychainSession {
                    tokens,
                    refresh_expires_at,
                })
            })
            .transpose()?;
        let record = NativeKeychainRecord {
            version: NATIVE_RECORD_VERSION,
            installation,
            session,
        };
        self.save_record(&record)?;
        // Cleanup follows publication. A cleanup failure must not invalidate
        // the already atomically published coherent record.
        let _ = self.backend.delete(INSTALLATION_USER);
        let _ = self.backend.delete(USER);
        Ok(Some(record))
    }
}

impl InstallationStore for KeyringNativeCredentialStore {
    fn save(&self, installation: &InstallationRecord) -> Result<(), NativeRegistrationError> {
        self.save_record(&NativeKeychainRecord {
            version: NATIVE_RECORD_VERSION,
            installation: installation.clone(),
            session: None,
        })
        .map_err(NativeRegistrationError::Persistence)
    }

    fn load(&self) -> Result<Option<InstallationRecord>, NativeRegistrationError> {
        self.load_record()
            .map(|record| record.map(|value| value.installation))
            .map_err(NativeRegistrationError::Persistence)
    }
}

impl NativeCredentialStore for KeyringNativeCredentialStore {
    fn load_installation(&self) -> Result<Option<InstallationRecord>, NativeTokenError> {
        self.load_record()
            .map(|record| record.map(|value| value.installation))
            .map_err(NativeTokenError::Persistence)
    }

    fn save_credentials(&self, credentials: &NativeCredentials) -> Result<(), NativeTokenError> {
        self.save_record(&NativeKeychainRecord {
            version: NATIVE_RECORD_VERSION,
            installation: credentials.installation.clone(),
            session: Some(NativeKeychainSession {
                tokens: credentials.tokens.clone(),
                refresh_expires_at: credentials.refresh_expires_at,
            }),
        })
        .map_err(NativeTokenError::Persistence)
    }

    fn load_credentials(&self) -> Result<Option<NativeCredentials>, NativeTokenError> {
        self.load_record()
            .map(|record| {
                record.and_then(|value| {
                    value.session.map(|session| NativeCredentials {
                        installation: value.installation,
                        tokens: session.tokens,
                        refresh_expires_at: session.refresh_expires_at,
                    })
                })
            })
            .map_err(NativeTokenError::Persistence)
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

/// keyring errors describe the platform store, never secret values, so
/// their Display form is safe to propagate.
fn store_err(e: keyring::Error) -> AuthError {
    AuthError::Store(e.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use uuid::Uuid;

    use super::*;

    #[derive(Clone, Default)]
    struct MemoryKeychain {
        values: Arc<Mutex<HashMap<String, String>>>,
        writes: Arc<Mutex<Vec<String>>>,
        fail_writes: bool,
    }

    impl MemoryKeychain {
        fn with_split(installation: &InstallationRecord, tokens: &TokenSet) -> Self {
            Self {
                values: Arc::new(Mutex::new(HashMap::from([
                    (
                        INSTALLATION_USER.into(),
                        serde_json::to_string(installation).unwrap(),
                    ),
                    (USER.into(), serde_json::to_string(tokens).unwrap()),
                ]))),
                ..Self::default()
            }
        }
    }

    impl KeychainBackend for MemoryKeychain {
        fn get(&self, user: &str) -> Result<Option<String>, String> {
            Ok(self.values.lock().unwrap().get(user).cloned())
        }

        fn set(&self, user: &str, value: &str) -> Result<(), String> {
            if self.fail_writes {
                return Err("write failed".into());
            }
            self.values
                .lock()
                .unwrap()
                .insert(user.into(), value.into());
            self.writes.lock().unwrap().push(user.into());
            Ok(())
        }

        fn delete(&self, user: &str) -> Result<(), String> {
            self.values.lock().unwrap().remove(user);
            Ok(())
        }
    }

    fn installation(challenge: &str) -> InstallationRecord {
        InstallationRecord {
            private_key: [7; 32],
            device_id: Uuid::parse_str("10000000-0000-4000-8000-000000000001").unwrap(),
            registration_token: "registration-secret".into(),
            device_challenge: challenge.into(),
            registration_expires_at: 1_000,
        }
    }

    fn tokens() -> TokenSet {
        TokenSet {
            access_token: "access-secret".into(),
            refresh_token: Some("refresh-secret".into()),
            expires_at: Some(2_000),
            subject: Some("user-id".into()),
        }
    }

    #[test]
    fn credentials_replace_one_coherent_keychain_value() {
        let backend = MemoryKeychain::default();
        let store = KeyringNativeCredentialStore {
            backend: Box::new(backend.clone()),
        };
        let credentials = NativeCredentials {
            installation: installation("rotated-challenge"),
            tokens: tokens(),
            refresh_expires_at: 9_000,
        };

        store.save_credentials(&credentials).unwrap();

        assert_eq!(*backend.writes.lock().unwrap(), [NATIVE_CREDENTIAL_USER]);
        let loaded = store.load_credentials().unwrap().unwrap();
        assert_eq!(loaded.installation.device_challenge, "rotated-challenge");
        assert_eq!(loaded.tokens.access_token, "access-secret");
        assert_eq!(loaded.refresh_expires_at, 9_000);
    }

    #[test]
    fn split_installation_and_session_migrate_after_coherent_publish() {
        let backend = MemoryKeychain::with_split(&installation("legacy-challenge"), &tokens());
        let store = KeyringNativeCredentialStore {
            backend: Box::new(backend.clone()),
        };

        let loaded = store.load_credentials().unwrap().unwrap();

        assert_eq!(loaded.installation.device_challenge, "legacy-challenge");
        assert_eq!(loaded.tokens.access_token, "access-secret");
        assert_eq!(loaded.refresh_expires_at, 2_000);
        let values = backend.values.lock().unwrap();
        assert!(values.contains_key(NATIVE_CREDENTIAL_USER));
        assert!(!values.contains_key(INSTALLATION_USER));
        assert!(!values.contains_key(USER));
    }

    #[test]
    fn failed_migration_publish_keeps_both_split_values() {
        let mut backend = MemoryKeychain::with_split(&installation("legacy-challenge"), &tokens());
        backend.fail_writes = true;
        let store = KeyringNativeCredentialStore {
            backend: Box::new(backend.clone()),
        };

        assert!(store.load_credentials().is_err());

        let values = backend.values.lock().unwrap();
        assert!(values.contains_key(INSTALLATION_USER));
        assert!(values.contains_key(USER));
        assert!(!values.contains_key(NATIVE_CREDENTIAL_USER));
    }
}
