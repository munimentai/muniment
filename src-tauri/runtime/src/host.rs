//! The host value the desktop runtime runs the core runtime with.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use muniment_core::chat_launch::{ChatGrant, FetchGrantError, FetchReceiptError};
use muniment_core::sidecar::pi_chat::Receipt;
use muniment_desktop_integration::auth::api_base_url;
use muniment_desktop_integration::chat_grant;
use muniment_runtime_core::host::{ChatGrantService, KeyringCredentialStore};
use muniment_runtime_core::RuntimeHost;

use crate::account::DesktopAccount;

/// The reader sidecar's file name beside the runtime.
const READER_NAME: &str = if cfg!(windows) {
    "muniment-reader.exe"
} else {
    "muniment-reader"
};

/// The desktop's cloud account and grants, the platform keychain, the
/// installed desktop executable and the reader sidecar beside this binary.
pub fn desktop_host() -> RuntimeHost {
    let runtime_executable = std::env::current_exe().ok();
    RuntimeHost {
        account: Arc::new(DesktopAccount::default()),
        chat_grants: Arc::new(DesktopChatGrants),
        credentials: Arc::new(KeyringCredentialStore),
        expected_desktop_executable: crate::installed_desktop_executable(),
        reader_executable: runtime_executable.as_deref().and_then(reader_beside),
    }
}

/// The reader sidecar the installer stages beside the runtime executable.
pub fn reader_beside(runtime_executable: &Path) -> Option<PathBuf> {
    Some(runtime_executable.parent()?.join(READER_NAME))
}

/// Cloud chat grants from the keychain session's installation.
pub struct DesktopChatGrants;

impl ChatGrantService for DesktopChatGrants {
    fn fetch_grant(&self, access_token: &str) -> Result<ChatGrant, FetchGrantError> {
        chat_grant::fetch_native_grant(&api_base_url(), access_token)
    }

    fn inspect_session(&self, access_token: &str) -> Result<String, FetchGrantError> {
        chat_grant::inspect_native_chat_session(access_token)
    }

    fn fetch_receipt(
        &self,
        grant: &ChatGrant,
        access_token: &str,
        run_id: &str,
    ) -> Result<Receipt, FetchReceiptError> {
        chat_grant::fetch_receipt(&grant.receipt_url, access_token, run_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reader_sits_beside_the_runtime() {
        assert_eq!(
            reader_beside(Path::new("/usr/lib/muniment/muniment-runtime")),
            Some(Path::new("/usr/lib/muniment").join(READER_NAME))
        );
    }
}
