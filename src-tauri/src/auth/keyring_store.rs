//! [`TokenStore`] backed by the platform keychain via the `keyring` crate:
//! macOS Keychain, Windows Credential Manager, Secret Service on Linux.
//! One entry holds the whole `TokenSet` as JSON — nothing token-shaped ever
//! touches disk in plaintext.

use keyring::Entry;
use muniment_core::auth::{AuthError, TokenSet, TokenStore};

const SERVICE: &str = "ai.muniment.desktop";
const USER: &str = "oidc-tokens";

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
