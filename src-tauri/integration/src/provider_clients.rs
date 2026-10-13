//! The desktop's third-party OAuth clients for provider sign-in.

use muniment_core::model_router::native_auth::{OAuthClient, ProviderClients};

/// Antigravity signs in with a Google account through the client its IDE
/// presents.
pub const ANTIGRAVITY_CLIENT_ID: &str =
    "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
pub const ANTIGRAVITY_CLIENT_SECRET: &str = "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";

/// The desktop's provider clients. The runtime host hands them to the
/// runtime, which registers them before any sign-in or refresh.
pub fn provider_clients() -> ProviderClients {
    ProviderClients {
        antigravity: Some(OAuthClient {
            id: ANTIGRAVITY_CLIENT_ID.into(),
            secret: ANTIGRAVITY_CLIENT_SECRET.into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use muniment_core::model_router::native_auth::{antigravity_auth_url, set_provider_clients};

    #[test]
    fn registration_supplies_the_antigravity_client() {
        set_provider_clients(super::provider_clients());
        let url = antigravity_auth_url("state", "http://127.0.0.1:51121/oauth-callback").unwrap();
        assert!(url.contains(super::ANTIGRAVITY_CLIENT_ID));
        assert!(!url.contains(super::ANTIGRAVITY_CLIENT_SECRET));
    }
}
