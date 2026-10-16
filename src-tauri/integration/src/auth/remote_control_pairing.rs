//! Desktop half of Municloud QR pairing, revision 2. The QR payload is the
//! server `qr` object plus the pairing secret, and a completed pair yields the
//! relay keys of the relay encryption contract.

use std::fmt;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Utc};
use qrcode::QrCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use muniment_core::atomic_file;
use muniment_core::relay_encryption::{
    AuthorizedAgreement, DesktopAgreement, RelayCryptoError, RelayKeys,
};

use super::NativeCredentialBackend;

pub use muniment_core::account::{
    AuthorizedPhone, PairId, PairingChallengeView, PairingRevokeView, PairingStatusView,
    RemoteControlState,
};

pub const CONTRACT_VERSION: &str = muniment_core::relay_encryption::PAIRING_CONTRACT_VERSION;
/// The first revision, whose pairs carry no agreement keys.
pub const LEGACY_CONTRACT_VERSION: &str = "muniment.remote-control-pairing/1";
pub const CHALLENGE_PATH: &str = "/v1/remote-control/pairing/challenges";
pub const PAIRING_PATH: &str = "/v1/remote-control/pairing";
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);
pub const REPLACEMENT_INTERVAL: Duration = Duration::from_secs(10);
pub const PAIR_FILE_NAME: &str = "remote-control-pair.json";

const MAX_RESPONSE_BYTES: u64 = 16 * 1024;
const KEY_LENGTH: usize = 43;

#[derive(Clone)]
pub struct PairingHttpRequest {
    authorization: String,
}

impl fmt::Debug for PairingHttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PairingHttpRequest")
            .field("authorization", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairingQr {
    pub contract_version: String,
    pub desktop_device_id: Uuid,
    pub desktop_public_key: String,
    pub desktop_agreement_key: String,
    pub challenge: String,
}

impl fmt::Debug for PairingQr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PairingQr")
            .field("contract_version", &self.contract_version)
            .field("desktop_device_id", &self.desktop_device_id)
            .field("desktop_public_key", &"<redacted>")
            .field("desktop_agreement_key", &"<redacted>")
            .field("challenge", &"<redacted>")
            .finish()
    }
}

/// The QR object the desktop shows: the server `qr` and the pairing secret.
#[derive(Serialize)]
struct DisplayedQr<'a> {
    contract_version: &'a str,
    desktop_device_id: Uuid,
    desktop_public_key: &'a str,
    desktop_agreement_key: &'a str,
    challenge: &'a str,
    pairing_secret: &'a str,
}

/// This desktop installation as the cloud names it.
#[derive(Clone, PartialEq, Eq)]
pub struct DesktopIdentity {
    pub device_id: Uuid,
    /// The installation signing public key, base64url.
    pub public_key: String,
}

impl fmt::Debug for DesktopIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DesktopIdentity")
            .field("device_id", &self.device_id)
            .finish()
    }
}

impl DesktopIdentity {
    pub fn from_installation(installation: &super::InstallationRecord) -> Self {
        use base64::Engine as _;
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&installation.private_key);
        Self {
            device_id: installation.device_id,
            public_key: base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(signing_key.verifying_key().as_bytes()),
        }
    }
}

/// The agreement material of the challenge on screen, kept until the phone
/// completes the pair or the code expires.
pub struct PendingAgreement {
    agreement: DesktopAgreement,
    desktop_device_id: Uuid,
    expires_at: DateTime<Utc>,
}

impl fmt::Debug for PendingAgreement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingAgreement")
            .field("desktop_device_id", &self.desktop_device_id)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairingChallengeResponse {
    pub contract_version: String,
    pub expires_at: DateTime<Utc>,
    pub qr: PairingQr,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelayPair {
    pub pair_id: PairId,
    pub desktop_device_id: Uuid,
    pub mobile_device_id: Uuid,
    pub desktop_public_key: String,
    pub mobile_public_key: String,
    /// The agreement keys and proof of a revision 2 pair. A revision 1 pair
    /// has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop_agreement_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mobile_agreement_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mobile_agreement_proof: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl fmt::Debug for RelayPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RelayPair")
            .field("pair_id", &self.pair_id)
            .field("desktop_device_id", &self.desktop_device_id)
            .field("mobile_device_id", &self.mobile_device_id)
            .field("desktop_public_key", &"<redacted>")
            .field("mobile_public_key", &"<redacted>")
            .field("agreement", &self.desktop_agreement_key.is_some())
            .field("created_at", &self.created_at)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairingStatusResponse {
    pub contract_version: String,
    #[serde(deserialize_with = "required_pair")]
    pub pair: Option<RelayPair>,
}

fn required_pair<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<RelayPair>, D::Error> {
    Option::deserialize(deserializer)
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairingRevokeResponse {
    pub contract_version: String,
    pub revoked: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub enum PairingError {
    Config(String),
    CredentialsMissing,
    Transport(String),
    HttpStatus(u16),
    MalformedResponse(String),
    /// The phone's agreement key or proof did not verify, so the desktop
    /// revoked the pair.
    Agreement,
}

impl fmt::Debug for PairingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(_) => f.write_str("Config(<redacted>)"),
            Self::CredentialsMissing => f.write_str("CredentialsMissing"),
            Self::Transport(_) => f.write_str("Transport(<redacted>)"),
            Self::HttpStatus(status) => f.debug_tuple("HttpStatus").field(status).finish(),
            Self::MalformedResponse(_) => f.write_str("MalformedResponse(<redacted>)"),
            Self::Agreement => f.write_str("Agreement"),
        }
    }
}

impl fmt::Display for PairingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(_) => f.write_str("pairing configuration error"),
            Self::CredentialsMissing => f.write_str("pairing requires a credential"),
            Self::Transport(_) => f.write_str("pairing unavailable"),
            Self::HttpStatus(409) => f.write_str("A phone is already paired."),
            Self::HttpStatus(429) => f.write_str("Wait ten seconds before replacing the code."),
            Self::HttpStatus(_) => f.write_str("The pairing request failed."),
            Self::MalformedResponse(_) => f.write_str("malformed pairing response"),
            Self::Agreement => f.write_str("The phone could not be verified. Pair again."),
        }
    }
}

impl std::error::Error for PairingError {}

pub trait PairingTransport: Send + Sync {
    fn challenge(
        &self,
        url: &str,
        request: &PairingHttpRequest,
        body: &serde_json::Value,
    ) -> Result<PairingChallengeResponse, PairingError>;

    fn status(
        &self,
        url: &str,
        request: &PairingHttpRequest,
    ) -> Result<PairingStatusResponse, PairingError>;

    fn revoke(
        &self,
        url: &str,
        request: &PairingHttpRequest,
        body: &serde_json::Value,
    ) -> Result<PairingRevokeResponse, PairingError>;
}

pub struct UreqPairingTransport {
    timeout: Duration,
}

impl UreqPairingTransport {
    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }
}

impl PairingTransport for UreqPairingTransport {
    fn challenge(
        &self,
        url: &str,
        request: &PairingHttpRequest,
        body: &serde_json::Value,
    ) -> Result<PairingChallengeResponse, PairingError> {
        let response = super::native_http::request("POST", CHALLENGE_PATH, || {
            muniment_core::http::agent()
                .post(url)
                .timeout(self.timeout)
                .set("Content-Type", "application/json")
                .set("Authorization", &request.authorization)
                .send_json(body)
                .map_err(Box::new)
        })
        .map_err(|error| map_http(*error))?;
        if response.status() != 201 {
            return Err(PairingError::HttpStatus(response.status()));
        }
        parse_json(response)
    }

    fn status(
        &self,
        url: &str,
        request: &PairingHttpRequest,
    ) -> Result<PairingStatusResponse, PairingError> {
        let response = super::native_http::request("GET", PAIRING_PATH, || {
            muniment_core::http::agent()
                .get(url)
                .timeout(self.timeout)
                .set("Authorization", &request.authorization)
                .call()
                .map_err(Box::new)
        })
        .map_err(|error| map_http(*error))?;
        if response.status() != 200 {
            return Err(PairingError::HttpStatus(response.status()));
        }
        parse_json(response)
    }

    fn revoke(
        &self,
        url: &str,
        request: &PairingHttpRequest,
        body: &serde_json::Value,
    ) -> Result<PairingRevokeResponse, PairingError> {
        let response = super::native_http::request("DELETE", PAIRING_PATH, || {
            muniment_core::http::agent()
                .request("DELETE", url)
                .timeout(self.timeout)
                .set("Content-Type", "application/json")
                .set("Authorization", &request.authorization)
                .send_json(body)
                .map_err(Box::new)
        })
        .map_err(|error| map_http(*error))?;
        if response.status() != 200 {
            return Err(PairingError::HttpStatus(response.status()));
        }
        parse_json(response)
    }
}

fn map_http(error: super::native_http::Error) -> PairingError {
    match error {
        super::native_http::Error::Status(status, _) => PairingError::HttpStatus(status),
        super::native_http::Error::Transport(_) => PairingError::Transport("request failed".into()),
    }
}

fn parse_json<T: serde::de::DeserializeOwned>(response: ureq::Response) -> Result<T, PairingError> {
    let mut body = Vec::new();
    response
        .into_reader()
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut body)
        .map_err(|_| PairingError::Transport("response read failed".into()))?;
    if body.len() as u64 > MAX_RESPONSE_BYTES {
        return Err(PairingError::MalformedResponse(
            "response exceeded the size limit".into(),
        ));
    }
    let mut deserializer = serde_json::Deserializer::from_slice(&body);
    let result = T::deserialize(&mut deserializer)
        .map_err(|_| PairingError::MalformedResponse("response was not contract JSON".into()))?;
    deserializer
        .end()
        .map_err(|_| PairingError::MalformedResponse("response contained trailing data".into()))?;
    Ok(result)
}

/// The keychain entry that holds the relay keys of one pair.
pub fn relay_key_entry(pair_id: PairId) -> String {
    format!("remote-control-relay-keys:{pair_id}")
}

/// Requests a challenge for the agreement key of `agreement` and returns the
/// QR view with the material to keep until the pair completes. The cloud must
/// echo this desktop's identity and agreement key, or no code is shown.
pub fn create_pairing_challenge(
    transport: &dyn PairingTransport,
    base_url: &str,
    access_token: &str,
    identity: &DesktopIdentity,
    mut agreement: DesktopAgreement,
) -> Result<(PairingChallengeView, PendingAgreement), PairingError> {
    let request = authorized_request(base_url, access_token)?;
    let body = serde_json::json!({
        "contract_version": CONTRACT_VERSION,
        "desktop_agreement_key": agreement.agreement_key(),
    });
    let response = transport.challenge(
        &format!("{}{CHALLENGE_PATH}", base_url.trim_end_matches('/')),
        &request,
        &body,
    )?;
    if response.contract_version != CONTRACT_VERSION {
        return Err(PairingError::MalformedResponse(
            "unsupported pairing contract".into(),
        ));
    }
    validate_qr(&response.qr)?;
    if response.qr.desktop_device_id != identity.device_id
        || response.qr.desktop_public_key != identity.public_key
        || response.qr.desktop_agreement_key != agreement.agreement_key()
    {
        return Err(PairingError::MalformedResponse(
            "the challenge does not name this desktop".into(),
        ));
    }
    agreement.set_challenge(&response.qr.challenge);
    let secret = agreement.pairing_secret();
    let payload = displayed_qr_json(&response.qr, &secret)?;
    let view = PairingChallengeView {
        expires_at: response.expires_at,
        qr_svg: qr_svg(&payload)?,
    };
    Ok((
        view,
        PendingAgreement {
            agreement,
            desktop_device_id: identity.device_id,
            expires_at: response.expires_at,
        },
    ))
}

/// Reads the current pair, keeps its peer identity under `store_path`, and
/// completes a pending agreement into relay keys in `keys`.
///
/// A revision 2 pair that answers the pending challenge must carry this
/// desktop's agreement key and a proof that verifies under the pairing
/// secret. Otherwise the desktop revokes the pair and reports the failure. A
/// pair this desktop holds no keys for reads as `pair_again`. When no pair
/// remains, the stored keys and relay counters go too. A pending agreement
/// past its code's expiry is erased.
pub fn read_pairing(
    transport: &dyn PairingTransport,
    base_url: &str,
    access_token: &str,
    store_path: &Path,
    pending: &Mutex<Option<PendingAgreement>>,
    keys: &dyn NativeCredentialBackend,
    now: DateTime<Utc>,
) -> Result<PairingStatusView, PairingError> {
    let request = authorized_request(base_url, access_token)?;
    let response = transport.status(
        &format!("{}{PAIRING_PATH}", base_url.trim_end_matches('/')),
        &request,
    )?;
    if response.contract_version != CONTRACT_VERSION
        && response.contract_version != LEGACY_CONTRACT_VERSION
    {
        return Err(PairingError::MalformedResponse(
            "unsupported pairing contract".into(),
        ));
    }
    let mut pending = pending
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(pair) = response.pair else {
        if pending
            .as_ref()
            .is_some_and(|pending| challenge_expired(pending.expires_at, now))
        {
            *pending = None;
        }
        forget_pair(store_path, keys)?;
        return Ok(PairingStatusView { pair: None });
    };
    validate_pair(&pair)?;
    if load_stored_pair(store_path)?.is_some_and(|stored| stored.pair_id != pair.pair_id) {
        forget_pair(store_path, keys)?;
    }
    let entry = relay_key_entry(pair.pair_id);
    let mut stored_keys = keys
        .get(&entry)
        .map_err(|_| PairingError::Transport("the relay keys are unavailable".into()))?
        .and_then(|secret| RelayKeys::from_secret(&secret).ok())
        .filter(|stored| stored.pair_id == pair.pair_id.to_string());
    let answers_pending = pending
        .as_ref()
        .is_some_and(|pending| pending.desktop_device_id == pair.desktop_device_id);
    if stored_keys.is_none() && answers_pending {
        let agreement = pending
            .take()
            .expect("a pending agreement was found")
            .agreement;
        match complete_agreement(agreement, &pair) {
            Ok(derived) => {
                keys.set(&entry, &derived.to_secret()).map_err(|_| {
                    PairingError::Transport("the relay keys could not be stored".into())
                })?;
                if let Some(profile) = store_path.parent() {
                    muniment_core::relay_publisher::forget_relay_state(profile).map_err(|_| {
                        PairingError::Transport("the relay state is unavailable".into())
                    })?;
                }
                stored_keys = Some(derived);
            }
            Err(_) => {
                drop(pending);
                let body = serde_json::json!({
                    "contract_version": CONTRACT_VERSION,
                    "pair_id": pair.pair_id,
                });
                transport.revoke(
                    &format!("{}{PAIRING_PATH}", base_url.trim_end_matches('/')),
                    &request,
                    &body,
                )?;
                forget_pair(store_path, keys)?;
                return Err(PairingError::Agreement);
            }
        }
    }
    save_stored_pair(store_path, &pair)?;
    let key_mismatch = store_path.parent().is_some_and(|profile| {
        muniment_core::relay_publisher::relay_key_mismatch(profile, &pair.pair_id.to_string())
    });
    let mut phone = authorized_phone(&pair);
    if stored_keys.is_none() || key_mismatch {
        phone.remote_control = RemoteControlState::PairAgain;
    }
    Ok(PairingStatusView { pair: Some(phone) })
}

fn complete_agreement(
    agreement: DesktopAgreement,
    pair: &RelayPair,
) -> Result<RelayKeys, RelayCryptoError> {
    let (Some(desktop_agreement_key), Some(mobile_agreement_key), Some(mobile_agreement_proof)) = (
        pair.desktop_agreement_key.as_deref(),
        pair.mobile_agreement_key.as_deref(),
        pair.mobile_agreement_proof.as_deref(),
    ) else {
        return Err(RelayCryptoError::KeyMismatch);
    };
    agreement.complete(&AuthorizedAgreement {
        pair_id: &pair.pair_id.to_string(),
        desktop_device_id: &pair.desktop_device_id.to_string(),
        mobile_device_id: &pair.mobile_device_id.to_string(),
        desktop_agreement_key,
        mobile_agreement_key,
        mobile_agreement_proof,
    })
}

/// The relay keys of the pair stored under `store_path`, or none.
pub fn stored_relay_keys(
    store_path: &Path,
    keys: &dyn NativeCredentialBackend,
) -> Option<RelayKeys> {
    let pair = load_stored_pair(store_path).ok()??;
    let secret = keys.get(&relay_key_entry(pair.pair_id)).ok()??;
    RelayKeys::from_secret(&secret)
        .ok()
        .filter(|stored| stored.pair_id == pair.pair_id.to_string())
}

/// Erases the stored pair, its relay keys and its relay counters.
fn forget_pair(store_path: &Path, keys: &dyn NativeCredentialBackend) -> Result<(), PairingError> {
    if let Some(stored) = load_stored_pair(store_path).ok().flatten() {
        keys.delete(&relay_key_entry(stored.pair_id))
            .map_err(|_| PairingError::Transport("the relay keys are unavailable".into()))?;
    }
    if let Some(profile) = store_path.parent() {
        muniment_core::relay_publisher::forget_relay_state(profile)
            .map_err(|_| PairingError::Transport("the relay state is unavailable".into()))?;
    }
    clear_stored_pair(store_path)
}

/// Revokes the named pair. Its relay keys and counters go with it, and the
/// stored peer identity goes when it names the same pair.
pub fn revoke_pairing(
    transport: &dyn PairingTransport,
    base_url: &str,
    access_token: &str,
    pair_id: PairId,
    store_path: &Path,
    keys: &dyn NativeCredentialBackend,
) -> Result<PairingRevokeView, PairingError> {
    let request = authorized_request(base_url, access_token)?;
    let body = serde_json::json!({
        "contract_version": CONTRACT_VERSION,
        "pair_id": pair_id,
    });
    let response = transport.revoke(
        &format!("{}{PAIRING_PATH}", base_url.trim_end_matches('/')),
        &request,
        &body,
    )?;
    if response.contract_version != CONTRACT_VERSION || !response.revoked {
        return Err(PairingError::MalformedResponse(
            "revocation was not confirmed".into(),
        ));
    }
    keys.delete(&relay_key_entry(pair_id))
        .map_err(|_| PairingError::Transport("the relay keys are unavailable".into()))?;
    if load_stored_pair(store_path)?.is_some_and(|stored| stored.pair_id == pair_id) {
        forget_pair(store_path, keys)?;
    }
    Ok(PairingRevokeView { revoked: true })
}

/// The server `qr` object as UTF-8 JSON, checked against the contract.
pub fn qr_json(qr: &PairingQr) -> Result<String, PairingError> {
    validate_qr(qr)?;
    let payload = serde_json::to_string(qr)
        .map_err(|_| PairingError::MalformedResponse("qr object could not be encoded".into()))?;
    if payload_has_forbidden_material(&payload) {
        return Err(PairingError::MalformedResponse(
            "qr object contained forbidden material".into(),
        ));
    }
    Ok(payload)
}

/// The QR code's JSON: the server `qr` object and the pairing secret.
pub fn displayed_qr_json(qr: &PairingQr, pairing_secret: &str) -> Result<String, PairingError> {
    qr_json(qr)?;
    validate_key(pairing_secret)?;
    serde_json::to_string(&DisplayedQr {
        contract_version: &qr.contract_version,
        desktop_device_id: qr.desktop_device_id,
        desktop_public_key: &qr.desktop_public_key,
        desktop_agreement_key: &qr.desktop_agreement_key,
        challenge: &qr.challenge,
        pairing_secret,
    })
    .map_err(|_| PairingError::MalformedResponse("qr object could not be encoded".into()))
}

pub fn qr_svg(payload: &str) -> Result<String, PairingError> {
    let code = QrCode::new(payload.as_bytes())
        .map_err(|_| PairingError::MalformedResponse("qr object could not be encoded".into()))?;
    let width = code.width();
    let mut path = String::new();
    for y in 0..width {
        for x in 0..width {
            if code[(x, y)] == qrcode::Color::Dark {
                path.push_str(&format!("M{},{}h1v1h-1z", x + 4, y + 4));
            }
        }
    }
    let width = width + 8;
    Ok(format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {width}" shape-rendering="crispEdges" role="img" aria-label="Pairing code"><rect width="{width}" height="{width}" fill="white"/><path fill="black" d="{path}"/></svg>"#
    ))
}

pub fn challenge_expired(expires_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    now >= expires_at
}

pub fn load_stored_pair(path: &Path) -> Result<Option<RelayPair>, PairingError> {
    match fs::read(path) {
        Ok(bytes) => {
            let pair: RelayPair = serde_json::from_slice(&bytes)
                .map_err(|_| PairingError::MalformedResponse("stored pair is unreadable".into()))?;
            validate_pair(&pair)?;
            Ok(Some(pair))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(PairingError::Transport("stored pair is unavailable".into())),
    }
}

fn authorized_phone(pair: &RelayPair) -> AuthorizedPhone {
    AuthorizedPhone {
        pair_id: pair.pair_id,
        mobile_device_id: pair.mobile_device_id,
        created_at: pair.created_at,
        remote_control: RemoteControlState::Ready,
    }
}

fn authorized_request(
    base_url: &str,
    access_token: &str,
) -> Result<PairingHttpRequest, PairingError> {
    validate_base_url(base_url)?;
    if access_token.is_empty() {
        return Err(PairingError::CredentialsMissing);
    }
    Ok(PairingHttpRequest {
        authorization: format!("Bearer {access_token}"),
    })
}

fn validate_qr(qr: &PairingQr) -> Result<(), PairingError> {
    if qr.contract_version != CONTRACT_VERSION {
        return Err(PairingError::MalformedResponse(
            "unsupported pairing contract".into(),
        ));
    }
    validate_key(&qr.desktop_public_key)?;
    validate_key(&qr.desktop_agreement_key)?;
    validate_key(&qr.challenge)?;
    Ok(())
}

fn validate_pair(pair: &RelayPair) -> Result<(), PairingError> {
    validate_key(&pair.desktop_public_key)?;
    validate_key(&pair.mobile_public_key)?;
    let agreement = [
        &pair.desktop_agreement_key,
        &pair.mobile_agreement_key,
        &pair.mobile_agreement_proof,
    ];
    if agreement.iter().any(|value| value.is_some()) {
        for value in agreement {
            validate_key(value.as_deref().unwrap_or_default())?;
        }
    }
    Ok(())
}

fn validate_key(value: &str) -> Result<(), PairingError> {
    let valid = value.len() == KEY_LENGTH
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    if valid {
        Ok(())
    } else {
        Err(PairingError::MalformedResponse(
            "pairing key was not contract material".into(),
        ))
    }
}

fn payload_has_forbidden_material(payload: &str) -> bool {
    let lower = payload.to_ascii_lowercase();
    lower.contains("bearer ")
        || lower.contains("http://")
        || lower.contains("https://")
        || lower.contains("access_token")
        || lower.contains("refresh_token")
        || lower.contains("private_key")
        || lower.contains("api_key")
}

fn save_stored_pair(path: &Path, pair: &RelayPair) -> Result<(), PairingError> {
    validate_pair(pair)?;
    let parent = path
        .parent()
        .ok_or_else(|| PairingError::Config("pair store path is invalid".into()))?;
    fs::create_dir_all(parent)
        .map_err(|_| PairingError::Transport("pair store is unavailable".into()))?;
    let bytes = serde_json::to_vec(pair)
        .map_err(|_| PairingError::MalformedResponse("pair could not be stored".into()))?;
    let temporary = path.with_extension("json.tmp");
    write_secret_file(&temporary, &bytes).map_err(|_| {
        let _ = fs::remove_file(&temporary);
        PairingError::Transport("pair store is unavailable".into())
    })?;
    atomic_file::replace(&temporary, path).map_err(|_| {
        let _ = fs::remove_file(&temporary);
        PairingError::Transport("pair store is unavailable".into())
    })
}

fn clear_stored_pair(path: &Path) -> Result<(), PairingError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(PairingError::Transport("pair store is unavailable".into())),
    }
}

fn write_secret_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn validate_base_url(value: &str) -> Result<(), PairingError> {
    let parsed =
        url::Url::parse(value).map_err(|_| PairingError::Config("base URL is invalid".into()))?;
    let loopback = parsed.scheme() == "http"
        && matches!(parsed.host_str(), Some("127.0.0.1" | "localhost" | "::1"));
    let bare = parsed.path() == "/"
        && parsed.query().is_none()
        && parsed.fragment().is_none()
        && parsed.username().is_empty()
        && parsed.password().is_none();
    if (parsed.scheme() == "https" || loopback) && bare {
        Ok(())
    } else {
        Err(PairingError::Config(
            "base URL must use HTTPS (HTTP is allowed only on loopback)".into(),
        ))
    }
}

#[cfg(test)]
mod tests;
