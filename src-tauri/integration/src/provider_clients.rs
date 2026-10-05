//! The desktop's third-party OAuth clients for provider sign-in.

use muniment_core::model_router::native_auth::{
    set_provider_clients, OAuthClient, ProviderClients,
};

/// Antigravity signs in with a Google account through the client its IDE
/// presents.
pub const ANTIGRAVITY_CLIENT_ID: &str =
    "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
pub const ANTIGRAVITY_CLIENT_SECRET: &str = "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";

/// Registers the desktop's provider clients with the router for this process.
/// The shell and the runtime call it at startup, before any sign-in or refresh.
pub fn register_provider_clients() {
    set_provider_clients(ProviderClients {
        antigravity: Some(OAuthClient {
            id: ANTIGRAVITY_CLIENT_ID.into(),
            secret: ANTIGRAVITY_CLIENT_SECRET.into(),
        }),
    });
}

#[cfg(test)]
mod tests {
    use muniment_core::model_router::native_auth::antigravity_auth_url;

    #[test]
    fn registration_supplies_the_antigravity_client() {
        super::register_provider_clients();
        let url = antigravity_auth_url("state", "http://127.0.0.1:51121/oauth-callback").unwrap();
        assert!(url.contains(super::ANTIGRAVITY_CLIENT_ID));
        assert!(!url.contains(super::ANTIGRAVITY_CLIENT_SECRET));
    }
}
