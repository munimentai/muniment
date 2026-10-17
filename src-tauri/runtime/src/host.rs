//! The host value the desktop runtime runs the core runtime with.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use muniment_core::chat_launch::{ChatGrant, FetchGrantError, FetchReceiptError};
use muniment_core::sidecar::pi_chat::Receipt;
use muniment_desktop_integration::auth::api_base_url;
use muniment_desktop_integration::chat_grant;
use muniment_runtime_core::host::{platform_credentials, ChatGrantService};
use muniment_runtime_core::RuntimeHost;

use crate::account::DesktopAccount;
use crate::sign_in_page::sign_in_page;

/// The reader sidecar's file name beside the runtime.
const READER_NAME: &str = if cfg!(windows) {
    "muniment-reader.exe"
} else {
    "muniment-reader"
};

/// The desktop's cloud account and grants, the platform keychain (a file
/// store in a temporary directory in a test run), the
/// installed desktop executable, the reader sidecar beside this binary, this
/// binary's package version, which the runtime announces on attach, the
/// desktop's provider OAuth clients and Muniment's sign-in page.
pub fn desktop_host() -> RuntimeHost {
    let runtime_executable = std::env::current_exe().ok();
    RuntimeHost {
        account: Arc::new(DesktopAccount::default()),
        chat_grants: Arc::new(DesktopChatGrants),
        credentials: platform_credentials(),
        expected_desktop_executable: crate::installed_desktop_executable(),
        reader_executable: runtime_executable.as_deref().and_then(reader_beside),
        runtime_version: env!("CARGO_PKG_VERSION"),
        provider_clients: muniment_desktop_integration::provider_clients::provider_clients(),
        sign_in_page: Arc::new(sign_in_page),
    }
}

/// Opens remote access for this runtime and starts its remote listener as
/// the stored settings direct. The listener stays off until the owner turns
/// pairing on, and the local desktop's welcome then names this runtime. It
/// then resumes the runs the last update interrupted and watches for the
/// app's update drain requests.
pub fn start_remote_route(state: &Arc<muniment_runtime_core::RuntimeAttachState>) {
    use muniment_core::attach::{
        InstallKind, RemoteAccessOptions, RemoteRouteOptions, RemoteSettingsOverrides,
    };
    let options = RemoteRouteOptions {
        access: RemoteAccessOptions {
            // Paired devices list this computer under the name it pairs with.
            runtime_name: Some(muniment_desktop_integration::computer_name::computer_name()),
            runtime_version: env!("MUNIMENT_APP_VERSION").to_owned(),
            install_kind: InstallKind::Desktop,
        },
        overrides: RemoteSettingsOverrides::default(),
    };
    if let Err(error) = state.start_remote_route(options) {
        eprintln!("muniment-runtime: {error}");
        return;
    }
    // The desktop app installs updates. Runs an update interrupted resume
    // now, and the app's drain requests arrive beside the update status.
    state.resume_after_update();
    if let Ok(profile) = crate::profile_directory() {
        crate::update_drain::start(state, profile.join("remote"));
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
    fn the_runtime_announces_this_binary_version() {
        assert_eq!(desktop_host().runtime_version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn the_reader_sits_beside_the_runtime() {
        assert_eq!(
            reader_beside(Path::new("/usr/lib/muniment/muniment-runtime")),
            Some(Path::new("/usr/lib/muniment").join(READER_NAME))
        );
    }
}
