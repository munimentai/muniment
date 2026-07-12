use std::{fmt, str::FromStr};

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use uuid::Uuid;

pub const PROTOCOL: &str = "muniment.attach/1";
pub const VERSION: u16 = 1;
pub const MAX_ID_TEXT_BYTES: usize = 64;
pub const MAX_IDEMPOTENCY_KEY_BYTES: usize = 256;
pub const MAX_CLIENT_TEXT_BYTES: usize = 256;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Protocol;

impl Serialize for Protocol {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(PROTOCOL)
    }
}
impl<'de> Deserialize<'de> for Protocol {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value == PROTOCOL {
            Ok(Self)
        } else {
            Err(de::Error::custom("unsupported attach protocol"))
        }
    }
}

/// Opaque wire identifier. V1 IDs are canonical UUID strings and are bounded
/// before parsing so malformed input cannot create unbounded diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Id(pub Uuid);

impl FromStr for Id {
    type Err = &'static str;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() || value.len() > MAX_ID_TEXT_BYTES {
            return Err("invalid identifier");
        }
        Uuid::parse_str(value)
            .map(Self)
            .map_err(|_| "invalid identifier")
    }
}
impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl Serialize for Id {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}
impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionRange {
    pub min: u16,
    pub max: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientInfo {
    pub kind: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: Protocol,
    pub client: ClientInfo,
    pub supported: VersionRange,
    pub client_nonce: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Welcome {
    pub selected: u16,
    pub desktop_version: String,
    pub server_nonce: String,
    pub authorization: AuthorizationStatus,
    pub approval_challenge: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorizationStatus {
    PairingRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Authorized {
    pub capability: String,
    pub expires_at: String,
    pub idle_timeout_seconds: u32,
    pub workspace_scopes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Operation {
    #[serde(rename = "thread.list")]
    ThreadList,
    #[serde(rename = "thread.open")]
    ThreadOpen,
    #[serde(rename = "run.open")]
    RunOpen,
    #[serde(rename = "run.start")]
    RunStart,
    #[serde(rename = "run.stream")]
    RunStream,
    #[serde(rename = "run.cursor_ack")]
    RunCursorAck,
    #[serde(rename = "run.steer")]
    RunSteer,
    #[serde(rename = "run.follow_up")]
    RunFollowUp,
    #[serde(rename = "run.cancel")]
    RunCancel,
    #[serde(rename = "permission.answer")]
    PermissionAnswer,
    #[serde(rename = "artifact.fetch")]
    ArtifactFetch,
    #[serde(rename = "artifact.window")]
    ArtifactWindow,
    #[serde(rename = "request.cancel")]
    RequestCancel,
}
impl Operation {
    pub fn requires_idempotency(self) -> bool {
        matches!(
            self,
            Self::RunStart
                | Self::RunSteer
                | Self::RunFollowUp
                | Self::RunCancel
                | Self::PermissionAnswer
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind {
    RunEvent,
    SubscriptionCaughtUp,
    PermissionPending,
    ArtifactChunk,
    ArtifactComplete,
    RequestCancelled,
    CapabilityRevoked,
    StreamClosed,
    /// V1 explicitly permits clients to ignore newly-added server events.
    Unknown(String),
}
impl EventKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::RunEvent => "run.event",
            Self::SubscriptionCaughtUp => "subscription.caught_up",
            Self::PermissionPending => "permission.pending",
            Self::ArtifactChunk => "artifact.chunk",
            Self::ArtifactComplete => "artifact.complete",
            Self::RequestCancelled => "request.cancelled",
            Self::CapabilityRevoked => "capability.revoked",
            Self::StreamClosed => "stream.closed",
            Self::Unknown(v) => v,
        }
    }
}
impl Serialize for EventKind {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}
impl<'de> Deserialize<'de> for EventKind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = String::deserialize(d)?;
        if v.is_empty() || v.len() > MAX_CLIENT_TEXT_BYTES {
            return Err(de::Error::custom("invalid event"));
        }
        Ok(match v.as_str() {
            "run.event" => Self::RunEvent,
            "subscription.caught_up" => Self::SubscriptionCaughtUp,
            "permission.pending" => Self::PermissionPending,
            "artifact.chunk" => Self::ArtifactChunk,
            "artifact.complete" => Self::ArtifactComplete,
            "request.cancelled" => Self::RequestCancelled,
            "capability.revoked" => Self::CapabilityRevoked,
            "stream.closed" => Self::StreamClosed,
            _ => Self::Unknown(v),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub protocol: Protocol,
    pub request_id: Id,
    pub operation: Operation,
    pub capability: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    pub body: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub protocol: Protocol,
    pub request_id: Id,
    pub ok: True,
    pub body: Value,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct True;
impl Serialize for True {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bool(true)
    }
}
impl<'de> Deserialize<'de> for True {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        if bool::deserialize(d)? {
            Ok(Self)
        } else {
            Err(de::Error::custom("ok must be true"))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorEnvelope {
    pub protocol: Protocol,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<Id>,
    pub ok: False,
    pub error: ProtocolError,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct False;
impl Serialize for False {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bool(false)
    }
}
impl<'de> Deserialize<'de> for False {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        if !bool::deserialize(d)? {
            Ok(Self)
        } else {
            Err(de::Error::custom("ok must be false"))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub protocol: Protocol,
    pub subscription_id: Id,
    pub event: EventKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_seq: Option<u64>,
    pub body: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<ErrorAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<ErrorDetails>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    IdempotencyKeyRequired,
    ProtocolIncompatible,
    PayloadTooLarge,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorAction {
    UpgradeCompanion,
    UpgradeDesktop,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ErrorDetails {
    Supported { supported: VersionRange },
}

impl ProtocolError {
    fn invalid(message: &'static str, code: ErrorCode) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: false,
            action: None,
            details: None,
        }
    }
}

pub fn validate_hello(hello: &Hello) -> Result<(), ProtocolError> {
    for value in [
        &hello.client.kind,
        &hello.client.version,
        &hello.client_nonce,
    ] {
        if value.is_empty() || value.len() > MAX_CLIENT_TEXT_BYTES {
            return Err(ProtocolError::invalid(
                "invalid handshake field",
                ErrorCode::InvalidRequest,
            ));
        }
    }
    if hello.supported.min > hello.supported.max {
        return Err(ProtocolError::invalid(
            "invalid version range",
            ErrorCode::InvalidRequest,
        ));
    }
    Ok(())
}

pub fn validate_request(request: &Request) -> Result<(), ProtocolError> {
    if request.capability.is_empty() || request.capability.len() > MAX_CLIENT_TEXT_BYTES {
        return Err(ProtocolError::invalid(
            "invalid capability",
            ErrorCode::InvalidRequest,
        ));
    }
    match &request.idempotency_key {
        Some(key) if key.is_empty() || key.len() > MAX_IDEMPOTENCY_KEY_BYTES => {
            return Err(ProtocolError::invalid(
                "invalid idempotency key",
                ErrorCode::InvalidRequest,
            ))
        }
        None if request.operation.requires_idempotency() => {
            return Err(ProtocolError::invalid(
                "idempotency key required",
                ErrorCode::IdempotencyKeyRequired,
            ))
        }
        Some(_) if !request.operation.requires_idempotency() => {
            return Err(ProtocolError::invalid(
                "idempotency key is not valid for this operation",
                ErrorCode::InvalidRequest,
            ))
        }
        _ => {}
    }
    if request.operation == Operation::RequestCancel {
        let object = request.body.as_object().ok_or_else(|| {
            ProtocolError::invalid("invalid cancellation target", ErrorCode::InvalidRequest)
        })?;
        let kind = object.get("kind").and_then(Value::as_str);
        let id = match kind {
            Some("request") => object.get("request_id"),
            Some("subscription") => object.get("subscription_id"),
            _ => None,
        }
        .and_then(Value::as_str);
        let exactly = object.get("request_id").is_some() ^ object.get("subscription_id").is_some();
        if !exactly || id.and_then(|v| v.parse::<Id>().ok()).is_none() {
            return Err(ProtocolError::invalid(
                "invalid cancellation target",
                ErrorCode::InvalidRequest,
            ));
        }
    }
    Ok(())
}

/// Selects v1 without consulting or disclosing any desktop runtime state.
pub fn negotiate_version(peer: VersionRange) -> Result<u16, ProtocolError> {
    let ours = VersionRange {
        min: VERSION,
        max: VERSION,
    };
    if peer.min <= VERSION && peer.max >= VERSION {
        return Ok(VERSION);
    }
    let action = if peer.max < VERSION {
        ErrorAction::UpgradeCompanion
    } else {
        ErrorAction::UpgradeDesktop
    };
    Err(ProtocolError {
        code: ErrorCode::ProtocolIncompatible,
        message: "attach protocol versions do not overlap".into(),
        retryable: false,
        action: Some(action),
        details: Some(ErrorDetails::Supported { supported: ours }),
    })
}
