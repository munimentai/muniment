use std::io::Write;
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use muniment_core::auth::{
    run_native_browser_authorization, AuthorizationTransport, InstallationRecord,
    InstallationStore, NativeAuthorizationError, NativeAuthorizationRequest,
    NativeAuthorizationResponse, NativeBrowserAuthorizationError, NativeRegistrationError,
    PkcePair,
};

const CONTINUATION: &str =
    "https://api.muniment.ai/v1/auth/native/authorize/opaque?attempt=server-secret";

struct MemoryStore(Mutex<Option<InstallationRecord>>);

impl InstallationStore for MemoryStore {
    fn save(&self, value: &InstallationRecord) -> Result<(), NativeRegistrationError> {
        *self.0.lock().unwrap() = Some(value.clone());
        Ok(())
    }

    fn load(&self) -> Result<Option<InstallationRecord>, NativeRegistrationError> {
        Ok(self.0.lock().unwrap().clone())
    }
}

#[derive(Default)]
struct CaptureTransport {
    request: Mutex<Option<NativeAuthorizationRequest>>,
}

impl AuthorizationTransport for CaptureTransport {
    fn authorize(
        &self,
        _url: &str,
        request: &NativeAuthorizationRequest,
    ) -> Result<NativeAuthorizationResponse, NativeAuthorizationError> {
        *self.request.lock().unwrap() = Some(request.clone());
        Ok(NativeAuthorizationResponse {
            authorization_url: CONTINUATION.into(),
            device_challenge: URL_SAFE_NO_PAD.encode([4; 32]),
        })
    }
}

fn store() -> MemoryStore {
    MemoryStore(Mutex::new(Some(InstallationRecord {
        private_key: [1; 32],
        device_id: "10000000-0000-4000-8000-000000000001".parse().unwrap(),
        registration_token: "registration-secret".into(),
        device_challenge: URL_SAFE_NO_PAD.encode([2; 32]),
        registration_expires_at: 2_000,
    })))
}

fn callback(redirect_uri: &str, query: &str) {
    let url = url::Url::parse(redirect_uri).unwrap();
    let port = url.port().unwrap();
    let target = format!("{}?{query}", url.path());
    std::thread::spawn(move || {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(
            stream,
            "GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
    });
}

#[test]
fn completes_verified_loopback_leg_with_exact_authorize_values() {
    let store = store();
    let transport = Arc::new(CaptureTransport::default());
    let browser_transport = transport.clone();
    let opened = Arc::new(Mutex::new(None));
    let browser_opened = opened.clone();

    let result = run_native_browser_authorization(
        &store,
        transport.as_ref(),
        "https://api.muniment.ai",
        None,
        1_000,
        move |url: &str| {
            *browser_opened.lock().unwrap() = Some(url.to_owned());
            let request = browser_transport.request.lock().unwrap().clone().unwrap();
            callback(
                &request.redirect_uri,
                &format!("code=authorization-secret&state={}", request.state),
            );
            Ok::<_, ()>(())
        },
        Duration::from_secs(2),
    )
    .unwrap();

    let request = transport.request.lock().unwrap().clone().unwrap();
    let redirect = url::Url::parse(&request.redirect_uri).unwrap();
    assert_eq!(redirect.host_str(), Some("127.0.0.1"));
    assert_ne!(redirect.port(), Some(0));
    assert_eq!(redirect.path(), "/callback");
    assert_eq!(request.code_challenge_method, "S256");
    assert_eq!(
        request.code_challenge,
        PkcePair::from_verifier(result.pkce_verifier.clone()).challenge
    );
    assert!(!request.state.is_empty());
    assert_eq!(opened.lock().unwrap().as_deref(), Some(CONTINUATION));
    assert_eq!(result.authorization_code, "authorization-secret");
    assert_eq!(result.redirect_uri, request.redirect_uri);
    assert_eq!(result.device_id, request.device_id);

    let debug = format!("{result:?} {request:?}");
    for secret in [
        "authorization-secret",
        result.pkce_verifier.as_str(),
        request.state.as_str(),
        "registration-secret",
        "server-secret",
    ] {
        assert!(!debug.contains(secret));
    }
}

fn callback_failure(
    query: impl FnOnce(&NativeAuthorizationRequest) -> String,
) -> NativeBrowserAuthorizationError {
    let store = store();
    let transport = CaptureTransport::default();
    run_native_browser_authorization(
        &store,
        &transport,
        "https://api.muniment.ai",
        None,
        1_000,
        |_url: &str| {
            let request = transport.request.lock().unwrap().clone().unwrap();
            callback(&request.redirect_uri, &query(&request));
            Ok::<_, ()>(())
        },
        Duration::from_secs(2),
    )
    .unwrap_err()
}

#[test]
fn rejects_mismatched_state_and_provider_error_without_leaking_callback_values() {
    let mismatch = callback_failure(|_| "code=code-secret&state=wrong-secret".into());
    assert_eq!(mismatch, NativeBrowserAuthorizationError::StateMismatch);

    let denied = callback_failure(|request| {
        format!(
            "error=access_denied&error_description=provider-secret&state={}",
            request.state
        )
    });
    assert_eq!(denied, NativeBrowserAuthorizationError::ProviderError);
    assert!(!denied.to_string().contains("provider-secret"));
}

#[test]
fn timeout_and_browser_launch_failure_are_bounded() {
    let timeout = run_native_browser_authorization(
        &store(),
        &CaptureTransport::default(),
        "https://api.muniment.ai",
        None,
        1_000,
        |_url| Ok::<_, ()>(()),
        Duration::from_millis(50),
    )
    .unwrap_err();
    assert_eq!(timeout, NativeBrowserAuthorizationError::Timeout);

    let browser = run_native_browser_authorization(
        &store(),
        &CaptureTransport::default(),
        "https://api.muniment.ai",
        None,
        1_000,
        |_url| Err("browser failure containing server-secret"),
        Duration::from_secs(1),
    )
    .unwrap_err();
    assert_eq!(browser, NativeBrowserAuthorizationError::BrowserLaunch);
    assert!(!browser.to_string().contains("server-secret"));
}
