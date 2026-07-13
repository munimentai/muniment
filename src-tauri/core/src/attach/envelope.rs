use std::fmt;

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use uuid::Uuid;

pub const PROTOCOL: &str = "muniment.attach/1";
pub const MAX_ID_LENGTH: usize = 64;
pub const MAX_TEXT_LENGTH: usize = 64 * 1024;

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
        (value == PROTOCOL)
            .then_some(Self)
            .ok_or_else(|| de::Error::custom("unsupported attach protocol"))
    }
}

/// An opaque, UUID-shaped protocol identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Id(String);

impl Id {
    pub fn new(value: impl Into<String>) -> Result<Self, IdError> {
        let value = value.into();
        if value.len() > MAX_ID_LENGTH || Uuid::parse_str(&value).is_err() {
            return Err(IdError);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdError;

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid protocol id")
    }
}
impl std::error::Error for IdError {}

impl Serialize for Id {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}
impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub protocol: Protocol,
    pub request_id: Id,
    pub operation: Operation,
    pub capability: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<Id>,
    pub body: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub protocol: Protocol,
    pub request_id: Id,
    pub ok: Success,
    pub body: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorEnvelope {
    pub protocol: Protocol,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<Id>,
    pub ok: Failure,
    pub error: ProtocolError,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub protocol: Protocol,
    pub subscription_id: Id,
    pub event: EventName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_seq: Option<u64>,
    pub body: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Envelope {
    Response(Response),
    Error(ErrorEnvelope),
    Request(Request),
    Event(Event),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    ProtocolIncompatible,
    PayloadTooLarge,
    MalformedFrame,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventName {
    #[serde(rename = "run.event")]
    RunEvent,
    #[serde(rename = "subscription.caught_up")]
    SubscriptionCaughtUp,
    #[serde(rename = "permission.pending")]
    PermissionPending,
    #[serde(rename = "artifact.chunk")]
    ArtifactChunk,
    #[serde(rename = "artifact.complete")]
    ArtifactComplete,
    #[serde(rename = "request.cancelled")]
    RequestCancelled,
    #[serde(rename = "capability.revoked")]
    CapabilityRevoked,
    #[serde(rename = "stream.closed")]
    StreamClosed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorAction {
    UpgradeCompanion,
    UpgradeDesktop,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Success;

impl Serialize for Success {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bool(true)
    }
}
impl<'de> Deserialize<'de> for Success {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        bool::deserialize(deserializer)?
            .then_some(Self)
            .ok_or_else(|| de::Error::custom("ok must be true"))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Failure;

impl Serialize for Failure {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bool(false)
    }
}
impl<'de> Deserialize<'de> for Failure {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        (!bool::deserialize(deserializer)?)
            .then_some(Self)
            .ok_or_else(|| de::Error::custom("ok must be false"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorMessage {
    #[serde(rename = "The companion and desktop protocol versions are incompatible.")]
    ProtocolIncompatible,
    #[serde(rename = "The payload exceeds the allowed size.")]
    PayloadTooLarge,
    #[serde(rename = "The frame is malformed.")]
    MalformedFrame,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProtocolError {
    code: ErrorCode,
    message: ErrorMessage,
    retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    action: Option<ErrorAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    details: Option<ErrorDetails>,
}

impl ProtocolError {
    pub fn protocol_incompatible(supported: VersionRange, action: ErrorAction) -> Self {
        Self {
            code: ErrorCode::ProtocolIncompatible,
            message: ErrorMessage::ProtocolIncompatible,
            retryable: false,
            action: Some(action),
            details: Some(ErrorDetails::SupportedVersions { supported }),
        }
    }

    pub fn payload_too_large() -> Self {
        Self::simple(ErrorCode::PayloadTooLarge, ErrorMessage::PayloadTooLarge)
    }

    pub fn malformed_frame() -> Self {
        Self::simple(ErrorCode::MalformedFrame, ErrorMessage::MalformedFrame)
    }

    fn simple(code: ErrorCode, message: ErrorMessage) -> Self {
        Self {
            code,
            message,
            retryable: false,
            action: None,
            details: None,
        }
    }

    pub fn code(&self) -> ErrorCode {
        self.code
    }
    pub fn message(&self) -> ErrorMessage {
        self.message
    }
    pub fn retryable(&self) -> bool {
        self.retryable
    }
    pub fn action(&self) -> Option<ErrorAction> {
        self.action
    }
    pub fn details(&self) -> Option<&ErrorDetails> {
        self.details.as_ref()
    }
}

impl<'de> Deserialize<'de> for ProtocolError {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            code: ErrorCode,
            message: ErrorMessage,
            retryable: bool,
            #[serde(default)]
            action: Option<ErrorAction>,
            #[serde(default)]
            details: Option<ErrorDetails>,
        }
        let wire = Wire::deserialize(deserializer)?;
        let expected = match (wire.code, wire.action, wire.details) {
            (
                ErrorCode::ProtocolIncompatible,
                Some(action),
                Some(ErrorDetails::SupportedVersions { supported }),
            ) => Self::protocol_incompatible(supported, action),
            (ErrorCode::PayloadTooLarge, None, None) => Self::payload_too_large(),
            (ErrorCode::MalformedFrame, None, None) => Self::malformed_frame(),
            _ => return Err(de::Error::custom("invalid error schema")),
        };
        if wire.message != expected.message || wire.retryable != expected.retryable {
            return Err(de::Error::custom("invalid error schema"));
        }
        Ok(expected)
    }
}

/// Closed, deliberately non-secret error detail vocabulary for this slice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ErrorDetails {
    SupportedVersions { supported: VersionRange },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionRange {
    pub min: u32,
    pub max: u32,
}
