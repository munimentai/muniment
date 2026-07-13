use serde::{Deserialize, Serialize};

use super::{ErrorAction, ErrorCode, ErrorDetails, Protocol, ProtocolError, VersionRange};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Client {
    pub kind: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: Protocol,
    pub client: Client,
    pub supported: VersionRange,
    pub client_nonce: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Authorization {
    PairingRequired,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Welcome {
    pub selected: u32,
    pub desktop_version: String,
    pub server_nonce: String,
    pub authorization: Authorization,
    pub approval_challenge: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FirstMessage {
    Hello(Hello),
    Other(serde_json::Value),
}

#[derive(Debug, Clone, PartialEq)]
pub enum NegotiationError {
    HelloRequired,
    InvalidRange,
    Incompatible(ProtocolError),
}

pub fn negotiate_version(
    client: VersionRange,
    desktop: VersionRange,
) -> Result<u32, ProtocolError> {
    let min = client.min.max(desktop.min);
    let max = client.max.min(desktop.max);
    if client.min <= client.max && desktop.min <= desktop.max && min <= max {
        return Ok(max);
    }
    let action = if client.max < desktop.min {
        ErrorAction::UpgradeCompanion
    } else {
        ErrorAction::UpgradeDesktop
    };
    Err(ProtocolError {
        code: ErrorCode::ProtocolIncompatible,
        message: "The companion and desktop protocol versions are incompatible.".into(),
        retryable: false,
        action: Some(action),
        details: Some(ErrorDetails::SupportedVersions { supported: desktop }),
    })
}

/// Validates that the first decoded message is a hello and selects its version.
pub fn negotiate_first(
    message: FirstMessage,
    desktop: VersionRange,
) -> Result<u32, NegotiationError> {
    let FirstMessage::Hello(hello) = message else {
        return Err(NegotiationError::HelloRequired);
    };
    if hello.supported.min > hello.supported.max || desktop.min > desktop.max {
        return Err(NegotiationError::InvalidRange);
    }
    negotiate_version(hello.supported, desktop).map_err(NegotiationError::Incompatible)
}

pub fn welcome(
    selected: u32,
    desktop_version: impl Into<String>,
    server_nonce: impl Into<String>,
    approval_challenge: impl Into<String>,
) -> Welcome {
    Welcome {
        selected,
        desktop_version: desktop_version.into(),
        server_nonce: server_nonce.into(),
        authorization: Authorization::PairingRequired,
        approval_challenge: approval_challenge.into(),
    }
}
