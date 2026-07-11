//! Typed projection of the Pi 0.73.1 chat event stream.
//!
//! Pi has one active agent stream. Muniment nevertheless tags every projected
//! event with the locally-owned run id; callers must create a fresh adapter for
//! each accepted prompt and discard unrelated frames.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptCommand<'a> {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub message: &'a str,
    pub streaming_behavior: StreamingBehavior,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum StreamingBehavior {
    #[serde(rename = "steer")]
    Steer,
    #[serde(rename = "followUp")]
    FollowUp,
}

impl<'a> PromptCommand<'a> {
    pub fn new(message: &'a str) -> Self {
        Self {
            kind: "prompt",
            message,
            streaming_behavior: StreamingBehavior::Steer,
        }
    }

    pub fn into_value(self) -> Value {
        serde_json::to_value(self).expect("PromptCommand is JSON serializable")
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Receipt {
    #[serde(default)]
    pub route: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub cost: Option<String>,
    #[serde(default)]
    pub time: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<CapabilityReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityReceipt {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PiChatEvent {
    PromptAccepted,
    TextDelta(String),
    Completed {
        receipt: Receipt,
    },
    Cancelled,
    Failed,
    /// A valid Pi event for another part of the agent lifecycle.
    Interleaved,
}

/// Parse only the documented fields needed by the UI. Error details and raw
/// assistant messages deliberately do not cross this boundary.
pub fn parse_frame(frame: &Value) -> Result<PiChatEvent, &'static str> {
    match frame.get("type").and_then(Value::as_str) {
        Some("response") if frame.get("command").and_then(Value::as_str) == Some("prompt") => {
            if frame.get("success").and_then(Value::as_bool) == Some(true) {
                Ok(PiChatEvent::PromptAccepted)
            } else {
                Ok(PiChatEvent::Failed)
            }
        }
        Some("message_update") => {
            let event = frame.get("assistantMessageEvent").unwrap_or(frame);
            match event.get("type").and_then(Value::as_str) {
                Some("text_delta") => event
                    .get("delta")
                    .and_then(Value::as_str)
                    .map(|text| PiChatEvent::TextDelta(text.to_owned()))
                    .ok_or("text delta is missing delta"),
                _ => Ok(PiChatEvent::Interleaved),
            }
        }
        Some("agent_end") => {
            let receipt = frame.get("receipt").cloned().unwrap_or_else(|| json!({}));
            serde_json::from_value(receipt)
                .map(|receipt| PiChatEvent::Completed { receipt })
                .map_err(|_| "invalid receipt")
        }
        Some("cancelled") => Ok(PiChatEvent::Cancelled),
        Some("error") => Ok(PiChatEvent::Failed),
        Some(_) => Ok(PiChatEvent::Interleaved),
        None => Err("Pi frame is missing type"),
    }
}

pub fn cancel_command() -> Value {
    json!({"type": "abort"})
}
