use std::{fmt, str::FromStr};

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use uuid::Uuid;

pub const PROTOCOL: &str = "muniment.attach/1";
pub const VERSION: u16 = 1;
pub const MAX_ID_TEXT_BYTES: usize = 64;
pub const MAX_IDEMPOTENCY_KEY_BYTES: usize = 256;
pub const MAX_CLIENT_TEXT_BYTES: usize = 256;
pub const MAX_TEXT_BYTES: usize = 64 * 1024;
pub const MAX_PAGE_SIZE: u16 = 100;
pub const MAX_WORKSPACE_SCOPES: usize = 100;
pub const MAX_CONTEXT_ITEMS: usize = 100;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Protocol;
impl Serialize for Protocol {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(PROTOCOL)
    }
}
impl<'de> Deserialize<'de> for Protocol {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        if String::deserialize(d)? == PROTOCOL {
            Ok(Self)
        } else {
            Err(de::Error::custom("unsupported attach protocol"))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Id(pub Uuid);
impl FromStr for Id {
    type Err = &'static str;
    fn from_str(v: &str) -> Result<Self, Self::Err> {
        if v.is_empty() || v.len() > MAX_ID_TEXT_BYTES {
            return Err("invalid identifier");
        }
        Uuid::parse_str(v)
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
        String::deserialize(d)?.parse().map_err(de::Error::custom)
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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Request {
    pub protocol: Protocol,
    pub request_id: Id,
    pub operation: Operation,
    pub capability: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    pub body: RequestBody,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContextValue {
    Text(String),
    Number(i64),
    Flag(bool),
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageBody {
    pub workspace_id: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u16>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThreadOpenBody {
    pub thread_id: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u16>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunIdBody {
    pub run_id: Id,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunStartBody {
    pub workspace_id: Id,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<ContextValue>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunStreamBody {
    pub run_id: Id,
    pub last_run_seq: u64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CursorAckBody {
    pub subscription_id: Id,
    pub through_run_seq: u64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunTextBody {
    pub run_id: Id,
    pub text: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    Allow,
    Deny,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionAnswerBody {
    pub gate_id: Id,
    pub decision: PermissionDecision,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactFetchBody {
    pub artifact_id: Id,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactWindowBody {
    pub transfer_id: Id,
    pub ack_through_chunk: i64,
    pub max_chunks: u16,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CancelTarget {
    Request { request_id: Id },
    Subscription { subscription_id: Id },
}
#[derive(Debug, Clone, PartialEq)]
pub enum RequestBody {
    ThreadList(PageBody),
    ThreadOpen(ThreadOpenBody),
    RunOpen(RunIdBody),
    RunStart(RunStartBody),
    RunStream(RunStreamBody),
    RunCursorAck(CursorAckBody),
    RunSteer(RunTextBody),
    RunFollowUp(RunTextBody),
    RunCancel(RunIdBody),
    PermissionAnswer(PermissionAnswerBody),
    ArtifactFetch(ArtifactFetchBody),
    ArtifactWindow(ArtifactWindowBody),
    RequestCancel(CancelTarget),
}
impl RequestBody {
    fn value(&self) -> Value {
        match self {
            Self::ThreadList(v) => serde_json::to_value(v),
            Self::ThreadOpen(v) => serde_json::to_value(v),
            Self::RunOpen(v) | Self::RunCancel(v) => serde_json::to_value(v),
            Self::RunStart(v) => serde_json::to_value(v),
            Self::RunStream(v) => serde_json::to_value(v),
            Self::RunCursorAck(v) => serde_json::to_value(v),
            Self::RunSteer(v) | Self::RunFollowUp(v) => serde_json::to_value(v),
            Self::PermissionAnswer(v) => serde_json::to_value(v),
            Self::ArtifactFetch(v) => serde_json::to_value(v),
            Self::ArtifactWindow(v) => serde_json::to_value(v),
            Self::RequestCancel(v) => serde_json::to_value(v),
        }
        .expect("wire bodies serialize")
    }
}
impl Serialize for RequestBody {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.value().serialize(s)
    }
}
#[derive(Deserialize)]
struct RawRequest {
    protocol: Protocol,
    request_id: Id,
    operation: Operation,
    capability: String,
    #[serde(default)]
    idempotency_key: Option<String>,
    body: Value,
}
impl<'de> Deserialize<'de> for Request {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let r = RawRequest::deserialize(d)?;
        let body = match r.operation {
            Operation::ThreadList => RequestBody::ThreadList(from(r.body)?),
            Operation::ThreadOpen => RequestBody::ThreadOpen(from(r.body)?),
            Operation::RunOpen => RequestBody::RunOpen(from(r.body)?),
            Operation::RunStart => RequestBody::RunStart(from(r.body)?),
            Operation::RunStream => RequestBody::RunStream(from(r.body)?),
            Operation::RunCursorAck => RequestBody::RunCursorAck(from(r.body)?),
            Operation::RunSteer => RequestBody::RunSteer(from(r.body)?),
            Operation::RunFollowUp => RequestBody::RunFollowUp(from(r.body)?),
            Operation::RunCancel => RequestBody::RunCancel(from(r.body)?),
            Operation::PermissionAnswer => RequestBody::PermissionAnswer(from(r.body)?),
            Operation::ArtifactFetch => RequestBody::ArtifactFetch(from(r.body)?),
            Operation::ArtifactWindow => RequestBody::ArtifactWindow(from(r.body)?),
            Operation::RequestCancel => RequestBody::RequestCancel(from(r.body)?),
        };
        Ok(Self {
            protocol: r.protocol,
            request_id: r.request_id,
            operation: r.operation,
            capability: r.capability,
            idempotency_key: r.idempotency_key,
            body,
        })
    }
}
fn from<T: for<'a> Deserialize<'a>, E: de::Error>(v: Value) -> Result<T, E> {
    serde_json::from_value(v).map_err(|_| E::custom("invalid operation body"))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response<B> {
    pub protocol: Protocol,
    pub request_id: Id,
    pub ok: True,
    pub body: B,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
pub struct RunEventBody {
    pub journal_event: String,
    pub payload: Value,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaughtUpBody {
    pub through_run_seq: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionPendingBody {
    pub gate_id: Id,
    pub description: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactChunkBody {
    pub artifact_id: Id,
    pub chunk_index: u64,
    pub offset: u64,
    pub byte_length: u32,
    pub chunk_sha256: String,
    pub data: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactCompleteBody {
    pub transfer_id: Id,
    pub total_bytes: u64,
    pub sha256: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestCancelledBody {
    pub kind: String,
    pub id: Id,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevokedBody {
    pub reason: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamClosedBody {
    pub code: String,
    pub resumable: bool,
}
#[derive(Debug, Clone, PartialEq)]
pub enum EventBody {
    RunEvent(RunEventBody),
    SubscriptionCaughtUp(CaughtUpBody),
    PermissionPending(PermissionPendingBody),
    ArtifactChunk(ArtifactChunkBody),
    ArtifactComplete(ArtifactCompleteBody),
    RequestCancelled(RequestCancelledBody),
    CapabilityRevoked(RevokedBody),
    StreamClosed(StreamClosedBody),
    Unknown(Value),
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Event {
    pub protocol: Protocol,
    pub subscription_id: Id,
    pub event: EventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_seq: Option<u64>,
    pub body: EventBody,
}
impl Serialize for EventBody {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::RunEvent(v) => v.serialize(s),
            Self::SubscriptionCaughtUp(v) => v.serialize(s),
            Self::PermissionPending(v) => v.serialize(s),
            Self::ArtifactChunk(v) => v.serialize(s),
            Self::ArtifactComplete(v) => v.serialize(s),
            Self::RequestCancelled(v) => v.serialize(s),
            Self::CapabilityRevoked(v) => v.serialize(s),
            Self::StreamClosed(v) => v.serialize(s),
            Self::Unknown(v) => v.serialize(s),
        }
    }
}
#[derive(Deserialize)]
struct RawEvent {
    protocol: Protocol,
    subscription_id: Id,
    event: EventKind,
    #[serde(default)]
    run_id: Option<Id>,
    #[serde(default)]
    run_seq: Option<u64>,
    body: Value,
}
impl<'de> Deserialize<'de> for Event {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let r = RawEvent::deserialize(d)?;
        let body = match &r.event {
            EventKind::RunEvent => EventBody::RunEvent(from(r.body)?),
            EventKind::SubscriptionCaughtUp => EventBody::SubscriptionCaughtUp(from(r.body)?),
            EventKind::PermissionPending => EventBody::PermissionPending(from(r.body)?),
            EventKind::ArtifactChunk => EventBody::ArtifactChunk(from(r.body)?),
            EventKind::ArtifactComplete => EventBody::ArtifactComplete(from(r.body)?),
            EventKind::RequestCancelled => EventBody::RequestCancelled(from(r.body)?),
            EventKind::CapabilityRevoked => EventBody::CapabilityRevoked(from(r.body)?),
            EventKind::StreamClosed => EventBody::StreamClosed(from(r.body)?),
            EventKind::Unknown(_) => EventBody::Unknown(r.body),
        };
        Ok(Self {
            protocol: r.protocol,
            subscription_id: r.subscription_id,
            event: r.event,
            run_id: r.run_id,
            run_seq: r.run_seq,
            body,
        })
    }
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
fn bounded(v: &str, max: usize) -> bool {
    !v.is_empty() && v.len() <= max
}
pub fn validate_hello(v: &Hello) -> Result<(), ProtocolError> {
    if !bounded(&v.client.kind, MAX_CLIENT_TEXT_BYTES)
        || !bounded(&v.client.version, MAX_CLIENT_TEXT_BYTES)
        || !bounded(&v.client_nonce, MAX_CLIENT_TEXT_BYTES)
        || v.supported.min > v.supported.max
    {
        return Err(ProtocolError::invalid(
            "invalid handshake",
            ErrorCode::InvalidRequest,
        ));
    }
    Ok(())
}
pub fn validate_welcome(v: &Welcome) -> Result<(), ProtocolError> {
    if v.selected != VERSION
        || !bounded(&v.desktop_version, MAX_CLIENT_TEXT_BYTES)
        || !bounded(&v.server_nonce, MAX_CLIENT_TEXT_BYTES)
        || !bounded(&v.approval_challenge, MAX_CLIENT_TEXT_BYTES)
    {
        return Err(ProtocolError::invalid(
            "invalid welcome",
            ErrorCode::InvalidRequest,
        ));
    }
    Ok(())
}
pub fn validate_authorized(v: &Authorized) -> Result<(), ProtocolError> {
    if !bounded(&v.capability, MAX_CLIENT_TEXT_BYTES)
        || !bounded(&v.expires_at, MAX_CLIENT_TEXT_BYTES)
        || v.idle_timeout_seconds == 0
        || v.workspace_scopes.is_empty()
        || v.workspace_scopes.len() > MAX_WORKSPACE_SCOPES
        || v.workspace_scopes
            .iter()
            .any(|s| !bounded(s, MAX_CLIENT_TEXT_BYTES))
    {
        return Err(ProtocolError::invalid(
            "invalid authorization",
            ErrorCode::InvalidRequest,
        ));
    }
    Ok(())
}
pub fn validate_request(v: &Request) -> Result<(), ProtocolError> {
    if !bounded(&v.capability, MAX_CLIENT_TEXT_BYTES) {
        return Err(ProtocolError::invalid(
            "invalid capability",
            ErrorCode::InvalidRequest,
        ));
    }
    match &v.idempotency_key {
        Some(k) if !bounded(k, MAX_IDEMPOTENCY_KEY_BYTES) => {
            return Err(ProtocolError::invalid(
                "invalid idempotency key",
                ErrorCode::InvalidRequest,
            ))
        }
        None if v.operation.requires_idempotency() => {
            return Err(ProtocolError::invalid(
                "idempotency key required",
                ErrorCode::IdempotencyKeyRequired,
            ))
        }
        Some(_) if !v.operation.requires_idempotency() => {
            return Err(ProtocolError::invalid(
                "idempotency key is not valid for this operation",
                ErrorCode::InvalidRequest,
            ))
        }
        _ => {}
    }
    let bad = match &v.body {
        RequestBody::ThreadList(x) => {
            x.limit.is_some_and(|n| n == 0 || n > MAX_PAGE_SIZE)
                || x.cursor
                    .as_ref()
                    .is_some_and(|s| !bounded(s, MAX_CLIENT_TEXT_BYTES))
        }
        RequestBody::ThreadOpen(x) => {
            x.limit.is_some_and(|n| n == 0 || n > MAX_PAGE_SIZE)
                || x.cursor
                    .as_ref()
                    .is_some_and(|s| !bounded(s, MAX_CLIENT_TEXT_BYTES))
        }
        RequestBody::RunStart(x) => {
            !bounded(&x.text, MAX_TEXT_BYTES) || x.context.len() > MAX_CONTEXT_ITEMS
        }
        RequestBody::RunSteer(x) | RequestBody::RunFollowUp(x) => !bounded(&x.text, MAX_TEXT_BYTES),
        RequestBody::ArtifactWindow(x) => x.ack_through_chunk < -1 || x.max_chunks == 0,
        _ => false,
    };
    if bad {
        return Err(ProtocolError::invalid(
            "invalid operation body",
            ErrorCode::InvalidRequest,
        ));
    }
    Ok(())
}
pub fn validate_event(v: &Event) -> Result<(), ProtocolError> {
    let bad = match &v.body {
        EventBody::RunEvent(x) => !bounded(&x.journal_event, MAX_CLIENT_TEXT_BYTES),
        EventBody::PermissionPending(x) => !bounded(&x.description, MAX_TEXT_BYTES),
        EventBody::ArtifactChunk(x) => {
            x.chunk_sha256.len() != 64 || !x.chunk_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        }
        EventBody::ArtifactComplete(x) => {
            x.sha256.len() != 64 || !x.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        }
        EventBody::RequestCancelled(x) => !bounded(&x.kind, MAX_CLIENT_TEXT_BYTES),
        EventBody::CapabilityRevoked(x) => !bounded(&x.reason, MAX_CLIENT_TEXT_BYTES),
        EventBody::StreamClosed(x) => !bounded(&x.code, MAX_CLIENT_TEXT_BYTES),
        _ => false,
    };
    if bad {
        return Err(ProtocolError::invalid(
            "invalid event",
            ErrorCode::InvalidRequest,
        ));
    }
    if matches!(v.event, EventKind::RunEvent) && (v.run_id.is_none() || v.run_seq.is_none()) {
        return Err(ProtocolError::invalid(
            "invalid run event correlation",
            ErrorCode::InvalidRequest,
        ));
    }
    Ok(())
}
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
