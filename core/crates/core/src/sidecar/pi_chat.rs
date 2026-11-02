//! Typed projection of the pinned Pi chat event stream. `message_update`
//! frames carry only the `assistantMessageEvent` delta, never the cumulative message.
//!
//! Pi has one active agent stream. Muniment nevertheless tags every projected
//! event with the locally-owned run id; callers must create a fresh adapter for
//! each accepted prompt and discard unrelated frames.

use crate::runtime_eprintln as eprintln;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::Path;
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::PiRpcTransport;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptCommand<'a> {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub message: &'a str,
    pub streaming_behavior: StreamingBehavior,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<PiImageContent>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PiImageContent {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub data: String,
    pub mime_type: String,
}

impl PiImageContent {
    pub fn new(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self {
            kind: "image",
            data: data.into(),
            mime_type: mime_type.into(),
        }
    }
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
            images: Vec::new(),
        }
    }

    pub fn with_images(message: &'a str, images: Vec<PiImageContent>) -> Self {
        Self {
            images,
            ..Self::new(message)
        }
    }

    pub fn into_value(self) -> Value {
        serde_json::to_value(self).expect("PromptCommand is JSON serializable")
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Receipt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The name the provider of `model` shows on the Models page, such as
    /// `Anthropic` for the Claude subscription bridge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<String>,
    /// The catalog's estimate in US dollars that `cost` shows rounded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<CapabilityReceipt>,
    /// The tokens the reply's turns used, summed from Pi's usage on each assistant message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<TokenUsage>,
    /// How many assistant messages the run took.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turns: Option<u32>,
    /// One tally per tool name: calls made and calls that failed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolReceipt>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub classifiers: Vec<crate::model_router::wire::ClassifierUsage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routing: Vec<crate::model_router::wire::RoutingEvidence>,
    /// The permission mode the run ended in, by its id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityReceipt {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub reasoning: u64,
    pub total: u64,
}

impl TokenUsage {
    pub fn add(&mut self, other: &TokenUsage) {
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
        self.reasoning += other.reasoning;
        self.total += other.total;
    }

    fn from_frame(usage: &Value) -> Option<Self> {
        let count = |field: &str| {
            usage
                .get(field)
                .and_then(Value::as_f64)
                .map(|n| n.max(0.0) as u64)
        };
        Some(Self {
            input: count("input")?,
            output: count("output")?,
            cache_read: count("cacheRead").unwrap_or(0),
            cache_write: count("cacheWrite").unwrap_or(0),
            reasoning: count("reasoning").unwrap_or(0),
            total: count("totalTokens").unwrap_or(0),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolReceipt {
    pub name: String,
    pub calls: u32,
    pub failed: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PiChatEvent {
    PromptAccepted,
    /// Pi's agent loop started on the accepted prompt: the model is thinking.
    TurnStarted,
    RoutingStage(String),
    /// An assistant message ended: one turn, with the provider and model that
    /// wrote it, its token usage, and the cost Pi's catalog puts on that usage.
    ModelReported {
        routing: Option<Box<crate::model_router::wire::RoutingEvidence>>,
        classifier: Option<crate::model_router::wire::ClassifierUsage>,
        provider: String,
        model: String,
        usage: Option<TokenUsage>,
        cost: Option<f64>,
    },
    TextDelta(String),
    ToolStarted {
        tool_call_id: String,
        tool_name: String,
        input: Option<String>,
        /// The call that made this one, such as the script a codemode call
        /// runs.
        parent_tool_call_id: Option<String>,
    },
    ToolFinished {
        tool_call_id: String,
        failed: bool,
        output: Option<String>,
    },
    ExtensionUiRequest(ExtensionUiRequest),
    /// Pi answered a manual compaction: its token counts, or why it refused.
    Compacted(Result<crate::engine::CompactionOutcome, String>),
    Completed,
    Cancelled,
    /// The turn failed, with the provider's own sentence when it gave one.
    Failed(Option<String>),
    UsageLimit,
    /// A valid Pi event for another part of the agent lifecycle.
    Interleaved,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionUiRequest {
    pub id: String,
    pub dialog: ExtensionUiDialog,
    pub timeout: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionUiDialog {
    Select {
        title: String,
        options: Vec<String>,
    },
    Confirm {
        title: String,
        message: String,
    },
    Input {
        title: String,
        placeholder: Option<String>,
    },
    Editor {
        title: String,
        prefill: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionUiAnswer {
    Selection(String),
    Confirmation(bool),
    Input(String),
    Editor(String),
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionUiResponse {
    id: String,
    answer: ExtensionUiAnswer,
}

impl ExtensionUiResponse {
    pub fn new(
        request: &ExtensionUiRequest,
        answer: ExtensionUiAnswer,
    ) -> Result<Self, &'static str> {
        let matches = match (&request.dialog, &answer) {
            (_, ExtensionUiAnswer::Cancelled) => true,
            (ExtensionUiDialog::Select { options, .. }, ExtensionUiAnswer::Selection(value)) => {
                options.contains(value)
            }
            (ExtensionUiDialog::Confirm { .. }, ExtensionUiAnswer::Confirmation(_)) => true,
            (ExtensionUiDialog::Input { .. }, ExtensionUiAnswer::Input(_)) => true,
            (ExtensionUiDialog::Editor { .. }, ExtensionUiAnswer::Editor(_)) => true,
            _ => false,
        };
        if !matches {
            return Err("answer kind does not match extension UI request");
        }
        Ok(Self {
            id: request.id.clone(),
            answer,
        })
    }

    pub fn into_value(self) -> Value {
        match self.answer {
            ExtensionUiAnswer::Selection(value)
            | ExtensionUiAnswer::Input(value)
            | ExtensionUiAnswer::Editor(value) => {
                json!({"type": "extension_ui_response", "id": self.id, "value": value})
            }
            ExtensionUiAnswer::Confirmation(confirmed) => {
                json!({"type": "extension_ui_response", "id": self.id, "confirmed": confirmed})
            }
            ExtensionUiAnswer::Cancelled => {
                json!({"type": "extension_ui_response", "id": self.id, "cancelled": true})
            }
        }
    }
}

/// Parse only the documented fields needed by the UI. Raw assistant messages
/// do not cross this boundary, and a provider's error crosses only as the
/// cleaned sentence [`provider_error_detail`] keeps.
pub fn parse_frame(frame: &Value) -> Result<PiChatEvent, &'static str> {
    match frame.get("type").and_then(Value::as_str) {
        Some("response") if frame.get("command").and_then(Value::as_str) == Some("prompt") => {
            if frame.get("success").and_then(Value::as_bool) == Some(true) {
                Ok(PiChatEvent::PromptAccepted)
            } else {
                Ok(PiChatEvent::Failed(None))
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
        Some("compaction_start" | "auto_compaction_start") => Ok(PiChatEvent::ToolStarted {
            tool_call_id: "muniment:context-compaction".into(),
            tool_name: "compact_context".into(),
            input: Some(serde_json::json!({"reason": frame.get("reason").and_then(Value::as_str).unwrap_or("threshold")}).to_string()),
            parent_tool_call_id: None,
        }),
        Some("compaction_end" | "auto_compaction_end") => {
            let failed = frame.get("aborted").and_then(Value::as_bool) == Some(true)
                || frame.get("errorMessage").and_then(Value::as_str).is_some()
                || !frame.get("result").is_some_and(Value::is_object);
            let detail = if frame.get("aborted").and_then(Value::as_bool) == Some(true) {
                "Context compaction was cancelled. The conversation remains available.".to_owned()
            } else if failed {
                frame.get("errorMessage").and_then(Value::as_str).unwrap_or("Context compaction did not finish.").to_owned()
            } else {
                let tokens = frame.pointer("/result/tokensBefore").and_then(Value::as_u64);
                let after = frame.pointer("/result/estimatedTokensAfter").and_then(Value::as_u64);
                let summary = frame.pointer("/result/summary").and_then(Value::as_str).unwrap_or("");
                format!("Context compacted. Earlier messages remain in the conversation.\n{}\n{}{summary}", tokens.map(|n| format!("Context before compaction: {n} tokens.")).unwrap_or_default(), after.map(|n| format!("Context after compaction: about {n} tokens.\n")).unwrap_or_default())
            };
            Ok(PiChatEvent::ToolFinished {
                tool_call_id: "muniment:context-compaction".into(), failed,
                output: activity_detail(Some(&Value::String(detail))),
            })
        }
        Some("response") if frame.get("command").and_then(Value::as_str) == Some("compact") => {
            if frame.get("success").and_then(Value::as_bool) == Some(true) {
                frame
                    .get("data")
                    .and_then(compaction_outcome)
                    .map(|outcome| PiChatEvent::Compacted(Ok(outcome)))
                    .ok_or("compact response is missing tokensBefore")
            } else {
                let error = frame
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("Context compaction did not finish.");
                Ok(PiChatEvent::Compacted(Err(activity_detail(Some(
                    &Value::String(error.chars().take(500).collect()),
                ))
                .unwrap_or_default())))
            }
        }
        Some("agent_start") => Ok(PiChatEvent::TurnStarted),
        Some("message_end")
            if frame.pointer("/message/role").and_then(Value::as_str) == Some("assistant") =>
        {
            match (
                frame.pointer("/message/provider").and_then(Value::as_str),
                frame.pointer("/message/model").and_then(Value::as_str),
            ) {
                (Some(provider), Some(model)) if !provider.is_empty() && !model.is_empty() => {
                    let usage = frame.pointer("/message/usage");
                    let routed = (provider == crate::model_router::config::ROUTER_PROVIDER)
                        .then(|| {
                            frame
                                .pointer("/message/responseId")
                                .and_then(Value::as_str)
                                .and_then(crate::model_router::wire::response_model)
                        })
                        .flatten();
                    let tokens = usage.and_then(TokenUsage::from_frame);
                    // Cached input uses the standard input rate.
                    let estimate = |family: &str, model: &str| {
                        let price = crate::model_router::model_catalog::entry(family, model)?;
                        let tokens = tokens.as_ref()?;
                        Some(((tokens.input as f64 + tokens.cache_read as f64
                            + tokens.cache_write as f64) * price.price
                            + tokens.output as f64 * price.output) / 1_000_000.0)
                    };
                    // The router alias has no Pi catalog price. Estimate each turn
                    // against its selected model, never the alias or the final model
                    // of a multi-model run.
                    let cost = if provider == crate::model_router::config::ROUTER_PROVIDER {
                        routed.as_ref().and_then(|(family, model)| estimate(family, model))
                    } else if provider == crate::models::registry::CLAUDE_BRIDGE_PROVIDER {
                        // The bridge prices nothing, so its zero is no estimate. A
                        // subscription run shows its family's API price, or none.
                        estimate(crate::models::registry::catalog_id(provider), model)
                    } else {
                        usage.and_then(|usage| usage.pointer("/cost/total"))
                            .and_then(Value::as_f64)
                    };
                    let (provider, model) =
                        routed.unwrap_or_else(|| (provider.to_owned(), model.to_owned()));
                    let classifier = (frame.pointer("/message/provider").and_then(Value::as_str)
                        == Some(crate::model_router::config::ROUTER_PROVIDER))
                        .then(|| frame.pointer("/message/responseId").and_then(Value::as_str)
                            .and_then(crate::model_router::wire::response_classifier)).flatten();
                    let routing = (frame.pointer("/message/provider").and_then(Value::as_str) == Some(crate::model_router::config::ROUTER_PROVIDER))
                        .then(|| frame.pointer("/message/responseId").and_then(Value::as_str).and_then(crate::model_router::wire::response_routing)).flatten().map(Box::new);
                    Ok(PiChatEvent::ModelReported {
                        routing,
                        classifier,
                        provider,
                        model,
                        usage: tokens,
                        cost,
                    })
                }
                _ => Ok(PiChatEvent::Interleaved),
            }
        }
        Some("tool_execution_start") => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct ToolStart {
                tool_call_id: String,
                tool_name: String,
                #[serde(default)]
                parent_tool_call_id: Option<String>,
            }

            let Ok(event) = serde_json::from_value::<ToolStart>(frame.clone()) else {
                return Ok(PiChatEvent::Interleaved);
            };
            if event.tool_call_id.is_empty() || event.tool_name.is_empty() {
                return Ok(PiChatEvent::Interleaved);
            }
            Ok(PiChatEvent::ToolStarted {
                tool_call_id: event.tool_call_id,
                tool_name: event.tool_name,
                input: activity_detail(frame.get("args")),
                parent_tool_call_id: event.parent_tool_call_id.filter(|id| !id.is_empty()),
            })
        }
        Some("tool_execution_end") => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct ToolEnd {
                tool_call_id: String,
                is_error: bool,
            }

            let Ok(event) = serde_json::from_value::<ToolEnd>(frame.clone()) else {
                return Ok(PiChatEvent::Interleaved);
            };
            if event.tool_call_id.is_empty() {
                return Ok(PiChatEvent::Interleaved);
            }
            Ok(PiChatEvent::ToolFinished {
                tool_call_id: event.tool_call_id,
                failed: event.is_error,
                output: activity_detail(frame.get("result")),
            })
        }
        Some("extension_ui_request") => parse_extension_ui_request(frame),
        // Pi owns generation, not billing/routing provenance. Any similarly
        // named member is deliberately ignored; the control plane supplies it.
        Some("agent_end") => {
            let last_message = frame
                .get("messages")
                .and_then(Value::as_array)
                .and_then(|messages| {
                    messages.iter().rev().find(|message| {
                        message.get("role").and_then(Value::as_str) == Some("assistant")
                    })
                });
            let stop_reason = last_message.and_then(|message| message.get("stopReason"))
                .and_then(Value::as_str);
            let error_message = last_message
                .and_then(|message| message.get("errorMessage"))
                .and_then(Value::as_str);
            let provider = last_message
                .and_then(|message| message.get("provider"))
                .and_then(Value::as_str);
            match stop_reason {
                Some("error") if error_message
                    .is_some_and(|message| message.contains("This provider has no remaining allowance.")) => Ok(PiChatEvent::UsageLimit),
                Some("error") if error_message.is_some_and(refuses_tool_calls) => Ok(PiChatEvent::Failed(Some(
                    tool_calls_refused_detail(crate::state_root::state_directory().as_deref(), provider),
                ))),
                Some("error") => Ok(PiChatEvent::Failed(error_message.and_then(provider_error_detail))),
                Some("length") => Ok(PiChatEvent::Failed(None)),
                Some("aborted") => Ok(PiChatEvent::Cancelled),
                _ => Ok(PiChatEvent::Completed),
            }
        }
        Some("cancelled") => Ok(PiChatEvent::Cancelled),
        Some("error") => Ok(PiChatEvent::Failed(None)),
        Some(_) => Ok(PiChatEvent::Interleaved),
        None => Err("Pi frame is missing type"),
    }
}

/// The longest provider sentence a failed run carries.
const PROVIDER_DETAIL_CHARS: usize = 300;

/// The sentence a provider's refusal carries, from Pi's `errorMessage`: the
/// message of a JSON error body after an HTTP status, such as
/// `400: {"message": ...}`, or the text itself. It comes back on one line,
/// with credentials removed, and at most 300 characters long.
pub fn provider_error_detail(text: &str) -> Option<String> {
    if let Some(provider) = refused_key_provider(text) {
        return Some(key_failure_detail(
            crate::state_root::state_directory().as_deref(),
            provider,
        ));
    }
    let body = text
        .split_once(':')
        .filter(|(status, _)| !status.is_empty() && status.bytes().all(|b| b.is_ascii_digit()))
        .map_or(text, |(_, body)| body.trim());
    let message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            ["/message", "/error/message", "/detail", "/error"]
                .iter()
                .find_map(|pointer| value.pointer(pointer).and_then(Value::as_str))
                .map(str::to_owned)
        })
        .unwrap_or_else(|| text.to_owned());
    let line = message.split_whitespace().collect::<Vec<_>>().join(" ");
    let (clean, _) =
        crate::agent_replays::without_secrets(&crate::pi_launch::diagnostic_text(&line));
    let clean = clean.trim();
    if clean.is_empty() {
        return None;
    }
    Some(if clean.chars().count() > PROVIDER_DETAIL_CHARS {
        // The cut falls between words, so no word shows half.
        let cut: String = clean.chars().take(PROVIDER_DETAIL_CHARS - 1).collect();
        let whole = clean
            .chars()
            .nth(PROVIDER_DETAIL_CHARS - 1)
            .is_some_and(char::is_whitespace);
        let cut = match cut.rfind(char::is_whitespace) {
            Some(at) if !whole => &cut[..at],
            _ => cut.as_str(),
        };
        format!("{}…", cut.trim_end())
    } else {
        clean.to_owned()
    })
}

/// Whether a provider's refusal says the server takes no tool calls: vLLM
/// started without `--enable-auto-tool-choice` and `--tool-call-parser`
/// answers 400 to any request with tools, and Ollama names a model that
/// does not support tools.
pub fn refuses_tool_calls(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    [
        "--enable-auto-tool-choice",
        "--tool-call-parser",
        "does not support tools",
        "does not support tool calling",
        "tool calling is not supported",
        "tools are not supported",
    ]
    .iter()
    .any(|sign| text.contains(sign))
}

/// The sentence a run carries when the endpoint `provider` refused tool
/// calls, under its name when the state root at `state` names it.
fn tool_calls_refused_detail(state: Option<&Path>, provider: Option<&str>) -> String {
    let name = provider.map_or_else(
        || "The server".to_owned(),
        |provider| endpoint_name(state, provider),
    );
    format!(
        "{name} does not accept tool calls. Turn on tool calling on the server (for vLLM: --enable-auto-tool-choice --tool-call-parser <parser>) or turn off tools for this endpoint in Settings → Models."
    )
}

/// The name the record at `state` gives the endpoint `provider`, else its id.
fn endpoint_name(state: Option<&Path>, provider: &str) -> String {
    state
        .and_then(|state| {
            crate::models::pi_models::read_record(&crate::state_root::agent_directory(state)).ok()
        })
        .and_then(|record| {
            record
                .get("names")
                .and_then(|names| names.get(provider))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| provider.to_owned())
}

/// The sentence a run carries when the runtime's `secret get` answered no
/// key for the endpoint `provider`, under its name when the state root at
/// `state` names it. Pi drops the command's exit status, so the stale-key
/// mark the runtime keeps under `state` tells a stale key from a refusal: a
/// stale key never comes back, and the person enters it again.
fn key_failure_detail(state: Option<&Path>, provider: &str) -> String {
    let name = endpoint_name(state, provider);
    if state.is_some_and(|state| crate::state_root::endpoint_key_stale(state, provider)) {
        format!(
            "The saved key for {name} is out of date. Enter the key again in Settings → Models, then send the message again."
        )
    } else {
        format!(
            "The keychain refused the key for {name}. Answer its prompt if one is open, or choose Try again in Settings → Machines, then send the message again."
        )
    }
}

/// The provider whose key Pi failed to read through the runtime's
/// `secret get` command, from Pi's error, which names the command. Pi wraps
/// the sentence, as in `API key auth failed for provider <id>: Failed to
/// resolve API key for provider "<id>" from shell command: "<runtime>"
/// secret get <id>`, so the sentence counts wherever it stands. The runtime
/// answers that command from the keychain, so the failure is the keychain's
/// refusal or a prompt still open, and the command text stays out of the
/// reply. A command the user wrote keeps Pi's own sentence.
fn refused_key_provider(text: &str) -> Option<&str> {
    const RESOLVE: &str = "Failed to resolve API key for provider \"";
    let (_, rest) = text.split_once(RESOLVE)?;
    let (provider, command) = rest.split_once('"')?;
    if provider.is_empty()
        || !provider
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return None;
    }
    let command = command.strip_prefix(" from shell command: ")?;
    let (program, arguments) = match command.strip_prefix('"') {
        Some(quoted) => quoted.split_once('"')?,
        None => command.split_once(' ')?,
    };
    let name = program.rsplit(['/', '\\']).next()?;
    if !matches!(name, "muniment-runtime" | "muniment-runtime.exe") {
        return None;
    }
    let after = arguments
        .trim_start()
        .strip_prefix("secret get ")?
        .strip_prefix(provider)?;
    // A timeout or an exit status may follow the command, never more of the id.
    after
        .chars()
        .next()
        .is_none_or(|next| !(next.is_ascii_alphanumeric() || matches!(next, '-' | '_' | '.')))
        .then_some(provider)
}

/// Reads the token counts of a compaction result: `tokensBefore` and, when Pi
/// reports it, `estimatedTokensAfter`.
fn compaction_outcome(result: &Value) -> Option<crate::engine::CompactionOutcome> {
    Some(crate::engine::CompactionOutcome {
        tokens_before: result.get("tokensBefore").and_then(Value::as_u64)?,
        estimated_tokens_after: result.get("estimatedTokensAfter").and_then(Value::as_u64),
    })
}

fn parse_extension_ui_request(frame: &Value) -> Result<PiChatEvent, &'static str> {
    let Some(method) = frame.get("method").and_then(Value::as_str) else {
        return Ok(PiChatEvent::Interleaved);
    };
    if method == "notify" {
        if let Some(stage) = frame
            .get("message")
            .and_then(Value::as_str)
            .and_then(|text| text.strip_prefix("muniment:routing:"))
        {
            if matches!(
                stage,
                "choosing-model" | "waiting-for-account" | "fallback" | "thinking"
            ) {
                return Ok(PiChatEvent::RoutingStage(stage.into()));
            }
        }
    }
    if !matches!(method, "select" | "confirm" | "input" | "editor") {
        return Ok(PiChatEvent::Interleaved);
    }
    let id = frame
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .ok_or("blocking extension UI request is missing id")?;
    let title = frame
        .get("title")
        .and_then(Value::as_str)
        .filter(|title| !title.trim().is_empty())
        .ok_or("blocking extension UI request is missing title")?;
    let timeout = match frame.get("timeout") {
        Some(value) => Some(
            value
                .as_u64()
                .ok_or("blocking extension UI request has invalid timeout")?,
        ),
        None => None,
    };

    let dialog = match method {
        "select" => {
            let options = frame
                .get("options")
                .and_then(Value::as_array)
                .filter(|options| !options.is_empty())
                .ok_or("select extension UI request has invalid options")?
                .iter()
                .map(|option| {
                    option
                        .as_str()
                        .filter(|option| !option.is_empty())
                        .map(str::to_owned)
                        .ok_or("select extension UI request has invalid options")
                })
                .collect::<Result<Vec<_>, _>>()?;
            ExtensionUiDialog::Select {
                title: title.into(),
                options,
            }
        }
        "confirm" => ExtensionUiDialog::Confirm {
            title: title.into(),
            message: frame
                .get("message")
                .and_then(Value::as_str)
                .ok_or("confirm extension UI request is missing message")?
                .into(),
        },
        "input" => ExtensionUiDialog::Input {
            title: title.into(),
            placeholder: optional_string(frame, "placeholder")?,
        },
        "editor" => ExtensionUiDialog::Editor {
            title: title.into(),
            prefill: optional_string(frame, "prefill")?,
        },
        _ => unreachable!(),
    };
    Ok(PiChatEvent::ExtensionUiRequest(ExtensionUiRequest {
        id: id.into(),
        dialog,
        timeout,
    }))
}

fn optional_string(frame: &Value, field: &str) -> Result<Option<String>, &'static str> {
    match frame.get(field) {
        Some(value) => value
            .as_str()
            .map(|value| Some(value.to_owned()))
            .ok_or("blocking extension UI request has invalid optional field"),
        None => Ok(None),
    }
}

pub fn cancel_command() -> Value {
    json!({"type": "abort"})
}

const EMPTY_QUEUE_MESSAGE: &str = "Pi queued message must not be empty";
const QUEUE_COMMAND_FAILED: &str = "Pi queue command failed";

macro_rules! queue_command {
    ($name:ident, $kind:literal) => {
        #[derive(Clone, Debug, PartialEq, Eq, Serialize)]
        pub struct $name<'a> {
            #[serde(rename = "type")]
            kind: &'static str,
            message: &'a str,
        }

        impl<'a> $name<'a> {
            pub fn new(message: &'a str) -> Result<Self, &'static str> {
                if message.trim().is_empty() {
                    return Err(EMPTY_QUEUE_MESSAGE);
                }
                Ok(Self {
                    kind: $kind,
                    message,
                })
            }

            pub fn into_value(self) -> Value {
                serde_json::to_value(self).expect("Pi queue command is JSON serializable")
            }
        }
    };
}

queue_command!(SteerCommand, "steer");
queue_command!(FollowUpCommand, "follow_up");

fn require_queue_ack(response: &Value, command: &str) -> Result<(), String> {
    if response.get("type").and_then(Value::as_str) == Some("response")
        && response.get("command").and_then(Value::as_str) == Some(command)
        && response.get("success").and_then(Value::as_bool) == Some(true)
    {
        Ok(())
    } else {
        Err(QUEUE_COMMAND_FAILED.into())
    }
}

/// How long Pi has to produce a reply event after prompt submission:
/// `MUNIMENT_PI_FIRST_EVENT_TIMEOUT_S`, else 30 seconds. The runtime reads it
/// once. Prompt acknowledgments and unrelated lifecycle frames do not count.
pub fn first_event_timeout() -> Duration {
    static TIMEOUT: OnceLock<Duration> = OnceLock::new();
    *TIMEOUT.get_or_init(|| super::pi::pi_timeout_from_env(super::PI_FIRST_EVENT_TIMEOUT_ENV))
}

/// The failure reason of a run whose first reply event did not arrive in time.
pub fn first_event_timeout_reason() -> &'static str {
    static REASON: OnceLock<String> = OnceLock::new();
    REASON.get_or_init(|| {
        format!(
            "No reply arrived within {} seconds. Try again.",
            first_event_timeout().as_secs()
        )
    })
}

/// The titles of the dialogs among `frames` that nobody answered. Pi waits
/// on such a dialog when an extension asks it before the prompt is accepted.
fn unanswered_dialogs(frames: &VecDeque<Value>) -> Vec<String> {
    frames
        .iter()
        .filter_map(|frame| match parse_frame(frame) {
            Ok(PiChatEvent::ExtensionUiRequest(request)) => Some(match request.dialog {
                ExtensionUiDialog::Select { title, .. }
                | ExtensionUiDialog::Confirm { title, .. }
                | ExtensionUiDialog::Input { title, .. }
                | ExtensionUiDialog::Editor { title, .. } => title,
            }),
            _ => None,
        })
        .collect()
}

/// Binds Pi's single active stream to a locally-owned run. Construct this
/// before sending the prompt so no post-ack frame can be lost.
pub struct PiRunAdapter {
    run_id: String,
    frames: Mutex<mpsc::Receiver<Value>>,
    buffered: Mutex<VecDeque<Value>>,
    first_event_deadline: Mutex<Option<Instant>>,
    compaction_id: Mutex<Option<String>>,
    /// The session file this turn writes, once it is known.
    session: Mutex<Option<String>>,
    /// A compaction outcome that waits for the session file to be known.
    compaction: Mutex<Option<crate::engine::CompactionOutcome>>,
    pending_terminal: Mutex<Option<PiChatEvent>>,
    /// The messages the user sent during this turn that Pi took, in order,
    /// until the run's coordinator journals them.
    steered: Mutex<Vec<String>>,
}

impl PiRunAdapter {
    pub fn start(
        run_id: impl Into<String>,
        transport: &PiRpcTransport,
        prompt: &str,
        timeout: Duration,
    ) -> Result<(Self, PiChatEvent), String> {
        Self::start_with_images(run_id, transport, prompt, Vec::new(), timeout)
    }

    pub fn start_with_images(
        run_id: impl Into<String>,
        transport: &PiRpcTransport,
        prompt: &str,
        images: Vec<PiImageContent>,
        timeout: Duration,
    ) -> Result<(Self, PiChatEvent), String> {
        Self::start_with_images_and_handler(run_id, transport, prompt, images, timeout, |_, _| {
            false
        })
    }

    /// Services extension requests while Pi prepares a prompt, before its acknowledgement.
    pub fn start_with_images_and_handler(
        run_id: impl Into<String>,
        transport: &PiRpcTransport,
        prompt: &str,
        images: Vec<PiImageContent>,
        timeout: Duration,
        consume: impl FnMut(&Self, &PiChatEvent) -> bool,
    ) -> Result<(Self, PiChatEvent), String> {
        Self::start_prepared(
            run_id,
            transport,
            prompt,
            images,
            timeout,
            |_, _| {},
            consume,
        )
    }

    /// Starts a prompt after `prepare` sets the conversation up for it.
    /// `prepare` runs inside the first event bound and receives its deadline,
    /// so a slow setup leaves the prompt less time instead of more.
    pub fn start_prepared(
        run_id: impl Into<String>,
        transport: &PiRpcTransport,
        prompt: &str,
        images: Vec<PiImageContent>,
        timeout: Duration,
        prepare: impl FnOnce(&PiRpcTransport, Instant) + Send,
        mut consume: impl FnMut(&Self, &PiChatEvent) -> bool,
    ) -> Result<(Self, PiChatEvent), String> {
        let run_id = run_id.into();
        let deadline = Instant::now() + first_event_timeout();
        let adapter = Self {
            run_id: run_id.clone(),
            frames: Mutex::new(transport.subscribe()),
            buffered: Mutex::new(VecDeque::new()),
            compaction_id: Mutex::new(None),
            session: Mutex::new(None),
            compaction: Mutex::new(None),
            pending_terminal: Mutex::new(None),
            steered: Mutex::new(Vec::new()),
            first_event_deadline: Mutex::new(Some(deadline)),
        };
        let response = std::thread::scope(|scope| {
            let (sender, response) = mpsc::channel();
            scope.spawn(move || {
                prepare(transport, deadline);
                let result = transport.call(
                    PromptCommand::with_images(prompt, images).into_value(),
                    timeout.min(deadline.saturating_duration_since(Instant::now())),
                );
                let _ = sender.send(result);
            });
            loop {
                match response.try_recv() {
                    Ok(result) => return result,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        return Err("The prompt dispatcher stopped.".into())
                    }
                    Err(mpsc::TryRecvError::Empty) => {}
                }
                let frame = adapter
                    .frames
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .recv_timeout(Duration::from_millis(5));
                match frame {
                    Ok(frame) => {
                        let handled =
                            parse_frame(&frame).is_ok_and(|event| consume(&adapter, &event));
                        if !handled {
                            adapter
                                .buffered
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .push_back(frame);
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        return response
                            .recv()
                            .unwrap_or_else(|_| Err("The prompt dispatcher stopped.".into()))
                    }
                }
            }
        })
        .map_err(|error| {
            if Instant::now() >= deadline {
                let buffered = adapter
                    .buffered
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                for title in unanswered_dialogs(&buffered) {
                    eprintln!("muniment-runtime: run_id={run_id} prompt_blocked_on={title:?}");
                }
                format!(
                    "Pi did not acknowledge the prompt within {} seconds. {error}",
                    first_event_timeout().as_secs()
                )
            } else {
                error
            }
        })?;
        let accepted = parse_frame(&response).map_err(str::to_owned)?;
        if accepted != PiChatEvent::PromptAccepted {
            let detail: String = response
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("Pi rejected the prompt")
                .chars()
                .take(4096)
                .collect();
            eprintln!("muniment-runtime: run_id={run_id} provider_request outcome=not_started prompt_error={detail:?}");
            return Err("Pi rejected the prompt".into());
        }
        Ok((adapter, accepted))
    }

    /// Starts a manual compaction of the open session. Pi reports it with the
    /// same `compaction_start` and `compaction_end` events as an automatic
    /// one, with the reason `manual`, then answers the command.
    pub fn compact(
        run_id: impl Into<String>,
        transport: &PiRpcTransport,
        instructions: Option<&str>,
    ) -> Result<Self, String> {
        let adapter = Self {
            run_id: run_id.into(),
            frames: Mutex::new(transport.subscribe()),
            buffered: Mutex::new(VecDeque::new()),
            compaction_id: Mutex::new(None),
            session: Mutex::new(None),
            compaction: Mutex::new(None),
            pending_terminal: Mutex::new(None),
            steered: Mutex::new(Vec::new()),
            first_event_deadline: Mutex::new(None),
        };
        // A pre-correlated frame reaches the subscribers, so the answer
        // arrives in order behind the compaction events.
        let mut command = json!({
            "id": format!("muniment-compact-{}", uuid::Uuid::now_v7()),
            "type": "compact",
        });
        if let Some(instructions) = instructions.map(str::trim).filter(|text| !text.is_empty()) {
            command["customInstructions"] = Value::String(instructions.to_owned());
        }
        transport.send(command)?;
        Ok(adapter)
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// Names the session file this turn writes, so a compaction it reports
    /// is kept for the session's context usage.
    pub fn bind_session(&self, locator: &super::PiSessionLocator) {
        *self
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(locator.as_str().to_owned());
        if let Some(outcome) = self
            .compaction
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            crate::engine::record_compaction(locator.as_str(), outcome);
        }
    }

    fn observe_compaction(&self, outcome: crate::engine::CompactionOutcome) {
        match self
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_deref()
        {
            Some(session) => crate::engine::record_compaction(session, outcome),
            None => {
                *self
                    .compaction
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(outcome)
            }
        }
    }

    /// Answers a pending extension UI request without waiting for an RPC response.
    pub fn answer_extension_ui(
        &self,
        transport: &PiRpcTransport,
        request: &ExtensionUiRequest,
        answer: ExtensionUiAnswer,
    ) -> Result<(), String> {
        let response = ExtensionUiResponse::new(request, answer).map_err(str::to_owned)?;
        transport.send(response.into_value())
    }

    /// Waits for Pi to materialize the persistent session after accepting a
    /// prompt. Stream frames arriving while the file is being created are
    /// returned in order. If the binding cannot be validated, the accepted
    /// turn is cancelled and drained to a terminal event before returning.
    pub fn await_session_binding(
        &self,
        transport: &PiRpcTransport,
        session_root: &Path,
        timeout: Duration,
    ) -> Result<(super::PiSessionLocator, Vec<PiChatEvent>), String> {
        self.await_session_binding_with_handler(transport, session_root, timeout, |_| Ok(false))
    }

    pub fn await_session_binding_with_handler(
        &self,
        transport: &PiRpcTransport,
        session_root: &Path,
        timeout: Duration,
        mut consume: impl FnMut(&PiChatEvent) -> Result<bool, String>,
    ) -> Result<(super::PiSessionLocator, Vec<PiChatEvent>), String> {
        let mut deadline = std::time::Instant::now() + timeout;
        let mut buffered = Vec::new();
        while std::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let call_timeout = remaining.min(Duration::from_millis(100));
            if let Ok(locator) = transport.session_locator(session_root, call_timeout) {
                self.bind_session(&locator);
                return Ok((locator, buffered));
            }
            match self.next(Duration::from_millis(10).min(remaining)) {
                Ok(event) => {
                    // Pi can defer the session file until its first reply ends. Active
                    // streaming must not exhaust the session binding's idle timeout.
                    deadline = std::time::Instant::now() + timeout;
                    let started = std::time::Instant::now();
                    if consume(&event)? {
                        // Native recovery has its own timeout. Keep Pi's binding wait separate.
                        deadline += started.elapsed();
                    } else {
                        buffered.push(event);
                    }
                }
                Err(error) if error == "timed out waiting for Pi stream" => {}
                Err(error) if error == first_event_timeout_reason() => return Err(error),
                Err(_) => break,
            }
        }
        if self
            .first_event_deadline
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(first_event_timeout_reason().into());
        }
        let _ = self.cancel_and_drain(transport, Duration::from_secs(2));
        Err("Pi session binding failed".into())
    }

    /// Cancels an already-submitted prompt and waits until Pi reports that no
    /// agent work remains active.
    pub fn cancel_and_drain(
        &self,
        transport: &PiRpcTransport,
        timeout: Duration,
    ) -> Result<(), String> {
        transport.call(cancel_command(), timeout.min(Duration::from_millis(500)))?;
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            match self.next(remaining) {
                Ok(
                    PiChatEvent::Completed
                    | PiChatEvent::Cancelled
                    | PiChatEvent::Failed(_)
                    | PiChatEvent::UsageLimit
                    | PiChatEvent::Compacted(_),
                ) => return Ok(()),
                Ok(_) => {}
                Err(error) => return Err(error),
            }
        }
        Err("Pi cancellation did not finish".into())
    }

    /// Queues a message for delivery during the active turn. Success only
    /// confirms that Pi queued it; stream completion continues through `next`.
    pub fn steer(
        &self,
        transport: &PiRpcTransport,
        message: &str,
        timeout: Duration,
    ) -> Result<(), String> {
        let command = SteerCommand::new(message).map_err(str::to_owned)?;
        let response = transport
            .call(command.into_value(), timeout)
            .map_err(|_| QUEUE_COMMAND_FAILED.to_string())?;
        require_queue_ack(&response, "steer")
    }

    /// Keeps a message Pi took during this turn for the run's journal.
    pub fn record_steer(&self, message: &str) {
        self.steered
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(message.to_owned());
    }

    /// The messages Pi took since the last call, oldest first.
    pub fn take_steered(&self) -> Vec<String> {
        std::mem::take(
            &mut *self
                .steered
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    /// Queues a message for delivery after the active turn finishes.
    pub fn follow_up(
        &self,
        transport: &PiRpcTransport,
        message: &str,
        timeout: Duration,
    ) -> Result<(), String> {
        let command = FollowUpCommand::new(message).map_err(str::to_owned)?;
        let response = transport
            .call(command.into_value(), timeout)
            .map_err(|_| QUEUE_COMMAND_FAILED.to_string())?;
        require_queue_ack(&response, "follow_up")
    }

    /// Pi emits agent_end before automatic compaction. A state RPC is a
    /// protocol barrier: drain the following lifecycle events before settling.
    pub fn next_settled(
        &self,
        transport: &PiRpcTransport,
        timeout: Duration,
    ) -> Result<PiChatEvent, String> {
        self.next_with_settlement(timeout, || {
            transport.call(json!({"type": "get_state"}), Duration::from_secs(5))
        })
    }

    fn next_with_settlement(
        &self,
        timeout: Duration,
        state: impl FnOnce() -> Result<Value, String>,
    ) -> Result<PiChatEvent, String> {
        let result = self.next(timeout);
        let mut pending = self
            .pending_terminal
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match result {
            Ok(
                event @ (PiChatEvent::Completed
                | PiChatEvent::Failed(_)
                | PiChatEvent::UsageLimit
                | PiChatEvent::Cancelled
                | PiChatEvent::Compacted(_)),
            ) => {
                *pending = Some(event);
                Ok(PiChatEvent::Interleaved)
            }
            Err(ref error) if error == "timed out waiting for Pi stream" && pending.is_some() => {
                let snapshot = state()?;
                if snapshot
                    .pointer("/data/isCompacting")
                    .and_then(Value::as_bool)
                    == Some(true)
                    || snapshot
                        .pointer("/data/isStreaming")
                        .and_then(Value::as_bool)
                        == Some(true)
                {
                    return Ok(PiChatEvent::Interleaved);
                }
                // The barrier may have delivered compaction events to the subscriber.
                if let Ok(frame) = self
                    .frames
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .try_recv()
                {
                    self.buffered
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push_back(frame);
                    return Ok(PiChatEvent::Interleaved);
                }
                if self
                    .compaction_id
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .is_some()
                {
                    return Ok(PiChatEvent::Interleaved);
                }
                Ok(pending.take().unwrap())
            }
            other => other,
        }
    }

    pub fn next(&self, timeout: Duration) -> Result<PiChatEvent, String> {
        let mut deadline = self
            .first_event_deadline
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let timeout = if let Some(deadline) = *deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(first_event_timeout_reason().into());
            }
            timeout.min(remaining)
        } else {
            timeout
        };
        let buffered = self
            .buffered
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front();
        let frame = match buffered {
            Some(frame) => Ok(frame),
            None => self
                .frames
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .recv_timeout(timeout),
        }
        .map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout
                if deadline.is_some_and(|deadline| Instant::now() >= deadline) =>
            {
                first_event_timeout_reason().to_string()
            }
            mpsc::RecvTimeoutError::Timeout => "timed out waiting for Pi stream".to_string(),
            mpsc::RecvTimeoutError::Disconnected => "Pi process stream ended".to_string(),
        })?;
        let mut event = parse_frame(&frame).map_err(str::to_owned)?;
        let outcome = match (&event, frame.get("type").and_then(Value::as_str)) {
            (PiChatEvent::Compacted(Ok(outcome)), _) => Some(*outcome),
            (_, Some("compaction_end" | "auto_compaction_end")) => {
                frame.get("result").and_then(compaction_outcome)
            }
            _ => None,
        };
        if let Some(outcome) = outcome {
            self.observe_compaction(outcome);
        }
        if frame.get("type").and_then(Value::as_str) == Some("agent_start")
            || (matches!(
                frame.get("type").and_then(Value::as_str),
                Some("compaction_end" | "auto_compaction_end")
            ) && frame.get("willRetry").and_then(Value::as_bool) == Some(true))
        {
            self.pending_terminal
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
        }
        match &mut event {
            PiChatEvent::ToolStarted { tool_call_id, .. }
                if tool_call_id == "muniment:context-compaction" =>
            {
                *tool_call_id = format!("muniment:context-compaction:{}", uuid::Uuid::now_v7());
                *self
                    .compaction_id
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) =
                    Some(tool_call_id.clone());
            }
            PiChatEvent::ToolFinished { tool_call_id, .. }
                if tool_call_id == "muniment:context-compaction" =>
            {
                if let Some(id) = self
                    .compaction_id
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take()
                {
                    *tool_call_id = id;
                }
            }
            _ => {}
        }
        if frame.get("type").and_then(Value::as_str) == Some("message_end")
            && frame.pointer("/message/role").and_then(Value::as_str) == Some("assistant")
        {
            let outcome = match frame.pointer("/message/stopReason").and_then(Value::as_str) {
                Some("stop" | "length" | "toolUse") => "completed",
                Some("error") => "failed",
                Some("aborted") => "cancelled",
                _ => "unknown",
            };
            let detail = (outcome == "failed")
                .then(|| {
                    frame
                        .pointer("/message/errorMessage")
                        .and_then(Value::as_str)
                })
                .flatten()
                .and_then(provider_error_detail)
                .map(|detail| format!(" reason={detail:?}"))
                .unwrap_or_default();
            eprintln!("muniment-runtime: run_id={} provider_request outcome={outcome} source=pi_message_end{detail}", self.run_id);
        }
        let kind = match &event {
            PiChatEvent::TextDelta(_) => Some("text_delta"),
            PiChatEvent::ToolStarted { .. } => Some("tool_start"),
            PiChatEvent::ToolFinished { .. } => Some("tool_end"),
            PiChatEvent::ExtensionUiRequest(_) => Some("extension_ui_request"),
            PiChatEvent::Completed => Some("completed"),
            PiChatEvent::Cancelled => Some("cancelled"),
            PiChatEvent::Failed(_) | PiChatEvent::UsageLimit => Some("failed"),
            _ => match frame.get("type").and_then(Value::as_str) {
                Some(
                    kind @ ("agent_start" | "turn_start" | "turn_end" | "message_start"
                    | "message_update" | "message_end"),
                ) => Some(kind),
                _ => None,
            },
        };
        if let Some(kind) = kind.filter(|_| deadline.take().is_some()) {
            eprintln!(
                "muniment-runtime: run_id={} first_event {kind}",
                self.run_id
            );
        }
        Ok(event)
    }
}

impl Drop for PiRunAdapter {
    fn drop(&mut self) {
        if self
            .first_event_deadline
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
        {
            eprintln!(
                "muniment-runtime: run_id={} first_event absent",
                self.run_id
            );
        }
    }
}

// Activity details are bounded plain text. The same secret gate as memory
// prevents credentials from entering the journal or the webview.
fn activity_detail(value: Option<&Value>) -> Option<String> {
    let value = value?;
    let text = if let Some(text) = value.as_str() {
        text.to_owned()
    } else if let Some(content) = value.get("content").and_then(Value::as_array) {
        content
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        serde_json::to_string_pretty(value).ok()?
    };
    if crate::memory_secret::reject_memory_secret(&text).is_err() {
        return Some("Details withheld because they may contain a secret.".into());
    }
    let mut clipped: String = text.chars().take(8000).collect();
    if text.chars().count() > 8000 {
        clipped.push_str("\n[Output shortened]");
    }
    Some(clipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unanswered_dialog_names_what_the_prompt_waits_on() {
        let frames = VecDeque::from([
            json!({"type":"extension_ui_request", "id":"w-1", "method":"setWidget", "widgetKey":"tasks"}),
            json!({"type":"extension_ui_request", "id":"m-1", "method":"editor",
                "title":"muniment:permission-mode", "prefill":"{}"}),
            json!({"type":"agent_start"}),
        ]);
        assert_eq!(
            unanswered_dialogs(&frames),
            vec!["muniment:permission-mode".to_owned()]
        );
    }

    #[test]
    fn disconnected_stream_reports_process_death_without_upstream_details() {
        let (sender, frames) = mpsc::channel();
        let adapter = PiRunAdapter {
            run_id: "0190a100-0000-7000-8000-000000000001".into(),
            frames: Mutex::new(frames),
            buffered: Mutex::new(VecDeque::new()),
            compaction_id: Mutex::new(None),
            session: Mutex::new(None),
            compaction: Mutex::new(None),
            pending_terminal: Mutex::new(None),
            steered: Mutex::new(Vec::new()),
            first_event_deadline: Mutex::new(Some(Instant::now() + Duration::from_secs(1))),
        };
        drop(sender);

        assert_eq!(
            adapter.next(Duration::from_millis(1)).unwrap_err(),
            "Pi process stream ended"
        );
    }

    fn bounded_adapter(timeout: Duration) -> (mpsc::Sender<Value>, PiRunAdapter) {
        let (sender, frames) = mpsc::channel();
        (
            sender,
            PiRunAdapter {
                run_id: "run-bound".into(),
                frames: Mutex::new(frames),
                buffered: Mutex::new(VecDeque::new()),
                compaction_id: Mutex::new(None),
                session: Mutex::new(None),
                compaction: Mutex::new(None),
                pending_terminal: Mutex::new(None),
                steered: Mutex::new(Vec::new()),
                first_event_deadline: Mutex::new(Some(Instant::now() + timeout)),
            },
        )
    }

    #[test]
    fn a_codemode_script_reports_each_tool_it_calls_as_its_own_activity() {
        // Pi runs a script's tool calls through the same pipeline as the
        // model's own calls, under an id that names the script's call.
        let started = parse_frame(
            &json!({"type":"tool_execution_start","toolCallId":"call_1/1",
            "parentToolCallId":"call_1","toolName":"mcp__record__sql","args":{"query":"select 1"}}),
        )
        .unwrap();
        assert!(
            matches!(started, PiChatEvent::ToolStarted { ref tool_call_id, ref tool_name, ref parent_tool_call_id, .. }
            if tool_call_id == "call_1/1" && tool_name == "mcp__record__sql"
                && parent_tool_call_id.as_deref() == Some("call_1"))
        );
        let finished = parse_frame(&json!({"type":"tool_execution_end","toolCallId":"call_1/1",
            "parentToolCallId":"call_1","toolName":"mcp__record__sql","isError":false}))
        .unwrap();
        assert!(
            matches!(finished, PiChatEvent::ToolFinished { ref tool_call_id, failed: false, .. }
            if tool_call_id == "call_1/1")
        );
    }

    #[test]
    fn completion_waits_for_post_reply_compaction_and_overflow_retry() {
        let (sender, adapter) = bounded_adapter(Duration::from_secs(1));
        let next = || {
            adapter
                .next_with_settlement(Duration::from_millis(1), || {
                    Ok(json!({"success":true,"data":{"isStreaming":false,"isCompacting":false}}))
                })
                .unwrap()
        };
        sender.send(json!({"type":"agent_end"})).unwrap();
        assert_eq!(next(), PiChatEvent::Interleaved);
        sender
            .send(json!({"type":"compaction_start","reason":"threshold"}))
            .unwrap();
        assert!(matches!(next(), PiChatEvent::ToolStarted { .. }));
        assert_eq!(next(), PiChatEvent::Interleaved);
        sender.send(json!({"type":"compaction_end","result":{"summary":"Keep the goal"},"willRetry":false})).unwrap();
        assert!(matches!(
            next(),
            PiChatEvent::ToolFinished { failed: false, .. }
        ));
        assert_eq!(next(), PiChatEvent::Completed);

        sender
            .send(
                json!({"type":"agent_end","messages":[{"role":"assistant","stopReason":"error"}]}),
            )
            .unwrap();
        assert_eq!(next(), PiChatEvent::Interleaved);
        sender
            .send(json!({"type":"compaction_start","reason":"overflow"}))
            .unwrap();
        assert!(matches!(next(), PiChatEvent::ToolStarted { .. }));
        sender.send(json!({"type":"compaction_end","result":{"summary":"Keep the goal"},"willRetry":true})).unwrap();
        assert!(matches!(next(), PiChatEvent::ToolFinished { .. }));
        assert!(adapter.pending_terminal.lock().unwrap().is_none());
        sender.send(json!({"type":"agent_end"})).unwrap();
        assert_eq!(next(), PiChatEvent::Interleaved);
        assert_eq!(next(), PiChatEvent::Completed);
    }

    #[test]
    fn compaction_cycles_have_distinct_ids_and_failed_endings_are_visible() {
        let (sender, adapter) = bounded_adapter(Duration::from_secs(1));
        let mut ids = Vec::new();
        for prefix in ["compaction", "auto_compaction"] {
            sender
                .send(json!({"type": format!("{prefix}_start"), "reason": "threshold"}))
                .unwrap();
            let PiChatEvent::ToolStarted {
                tool_call_id,
                tool_name,
                ..
            } = adapter.next(Duration::from_secs(1)).unwrap()
            else {
                panic!("missing start")
            };
            assert_eq!(tool_name, "compact_context");
            sender.send(json!({"type": format!("{prefix}_end"), "result": {"summary":"Keep the user goal", "tokensBefore":50000}})).unwrap();
            let PiChatEvent::ToolFinished {
                tool_call_id: ended,
                failed,
                output,
            } = adapter.next(Duration::from_secs(1)).unwrap()
            else {
                panic!("missing end")
            };
            assert_eq!(tool_call_id, ended);
            assert!(!failed);
            assert!(output.unwrap().contains("50000"));
            ids.push(tool_call_id);
        }
        assert_ne!(ids[0], ids[1]);
        assert!(matches!(
            parse_frame(&json!({"type":"compaction_end","aborted":true,"result":null})).unwrap(),
            PiChatEvent::ToolFinished { failed: true, .. }
        ));
    }

    #[test]
    fn a_compaction_detail_names_the_tokens_before_and_the_estimate_after() {
        let PiChatEvent::ToolFinished { output, failed, .. } = parse_frame(&json!({
            "type": "compaction_end",
            "reason": "manual",
            "result": {"summary": "Keep the goal", "tokensBefore": 152400, "estimatedTokensAfter": 31900}
        }))
        .unwrap() else {
            panic!("compaction end")
        };
        assert!(!failed);
        assert_eq!(
            output.unwrap(),
            "Context compacted. Earlier messages remain in the conversation.\nContext before compaction: 152400 tokens.\nContext after compaction: about 31900 tokens.\nKeep the goal"
        );
        let PiChatEvent::ToolFinished { output, .. } = parse_frame(&json!({
            "type": "compaction_end",
            "result": {"summary": "Keep the goal", "tokensBefore": 152400}
        }))
        .unwrap() else {
            panic!("compaction end")
        };
        assert_eq!(
            output.unwrap(),
            "Context compacted. Earlier messages remain in the conversation.\nContext before compaction: 152400 tokens.\nKeep the goal"
        );
    }

    #[test]
    fn a_compact_answer_reports_its_counts_or_its_refusal() {
        assert_eq!(
            parse_frame(&json!({
                "type": "response", "command": "compact", "success": true,
                "data": {"summary": "s", "tokensBefore": 150000, "estimatedTokensAfter": 32000}
            }))
            .unwrap(),
            PiChatEvent::Compacted(Ok(crate::engine::CompactionOutcome {
                tokens_before: 150000,
                estimated_tokens_after: Some(32000),
            }))
        );
        assert_eq!(
            parse_frame(&json!({
                "type": "response", "command": "compact", "success": false,
                "error": "Nothing to compact"
            }))
            .unwrap(),
            PiChatEvent::Compacted(Err("Nothing to compact".into()))
        );
    }

    #[test]
    fn a_manual_compaction_ends_after_its_activity_and_keeps_its_estimate() {
        let (sender, adapter) = bounded_adapter(Duration::from_secs(1));
        *adapter.first_event_deadline.lock().unwrap() = None;
        let root =
            std::env::temp_dir().join(format!("muniment-compaction-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&root).unwrap();
        let locator = format!("{}.jsonl", uuid::Uuid::now_v7());
        std::fs::write(root.join(&locator), "{}\n").unwrap();
        let (bound, _) = crate::sidecar::validate_pi_session(&root, &locator).unwrap();
        adapter.bind_session(&bound);
        std::fs::remove_dir_all(root).unwrap();
        let next = || {
            adapter
                .next_with_settlement(Duration::from_millis(1), || {
                    Ok(json!({"success":true,"data":{"isStreaming":false,"isCompacting":false}}))
                })
                .unwrap()
        };
        sender
            .send(json!({"type":"compaction_start","reason":"manual"}))
            .unwrap();
        let PiChatEvent::ToolStarted {
            input, tool_name, ..
        } = next()
        else {
            panic!("compaction start")
        };
        assert_eq!(tool_name, "compact_context");
        assert_eq!(input.unwrap(), r#"{"reason":"manual"}"#);
        sender
            .send(json!({"type":"response","command":"compact","success":true,"data":{"tokensBefore":152400,"estimatedTokensAfter":31900}}))
            .unwrap();
        assert_eq!(next(), PiChatEvent::Interleaved);
        // The open compaction holds the answer until its end arrives.
        assert_eq!(next(), PiChatEvent::Interleaved);
        sender
            .send(json!({"type":"compaction_end","reason":"manual","result":{"summary":"s","tokensBefore":152400,"estimatedTokensAfter":31900}}))
            .unwrap();
        assert!(matches!(
            next(),
            PiChatEvent::ToolFinished { failed: false, .. }
        ));
        assert_eq!(
            next(),
            PiChatEvent::Compacted(Ok(crate::engine::CompactionOutcome {
                tokens_before: 152400,
                estimated_tokens_after: Some(31900),
            }))
        );
        assert_eq!(
            crate::engine::last_compaction(&locator),
            Some(crate::engine::CompactionOutcome {
                tokens_before: 152400,
                estimated_tokens_after: Some(31900),
            })
        );
    }

    #[test]
    fn silence_ends_at_the_first_event_bound() {
        let (_sender, adapter) = bounded_adapter(Duration::from_millis(20));
        let started = Instant::now();
        assert_eq!(
            adapter.next(Duration::from_secs(1)).unwrap_err(),
            "No reply arrived within 30 seconds. Try again."
        );
        assert!(started.elapsed() < Duration::from_millis(500));
        assert_eq!(
            adapter.next(Duration::ZERO).unwrap_err(),
            "No reply arrived within 30 seconds. Try again."
        );
    }

    #[test]
    fn the_first_event_bound_defaults_to_30_seconds() {
        assert!(std::env::var_os("MUNIMENT_PI_FIRST_EVENT_TIMEOUT_S").is_none());
        assert_eq!(first_event_timeout(), Duration::from_secs(30));
        assert_eq!(
            first_event_timeout_reason(),
            "No reply arrived within 30 seconds. Try again."
        );
    }

    #[test]
    fn acknowledgments_and_unrelated_frames_do_not_extend_the_bound() {
        let (sender, adapter) = bounded_adapter(Duration::from_millis(20));
        for frame in [
            json!({"type":"queue_update"}),
            json!({"type":"response", "command":"prompt", "success":true}),
        ] {
            sender.send(frame).unwrap();
            adapter.next(Duration::ZERO).unwrap();
        }
        assert_eq!(
            adapter.next(Duration::from_secs(1)).unwrap_err(),
            "No reply arrived within 30 seconds. Try again."
        );
    }

    #[test]
    fn a_reply_event_disarms_the_bound_but_a_late_event_does_not() {
        let (sender, adapter) = bounded_adapter(Duration::from_secs(1));
        sender
            .send(json!({"type":"message_update", "assistantMessageEvent":{
                "type":"text_delta", "delta":"hello"
            }}))
            .unwrap();
        assert_eq!(
            adapter.next(Duration::ZERO).unwrap(),
            PiChatEvent::TextDelta("hello".into())
        );
        assert!(adapter.first_event_deadline.lock().unwrap().is_none());
        assert_eq!(
            adapter.next(Duration::ZERO).unwrap_err(),
            "timed out waiting for Pi stream"
        );

        let (sender, adapter) = bounded_adapter(Duration::ZERO);
        sender.send(json!({"type":"agent_end"})).unwrap();
        assert_eq!(
            adapter.next(Duration::ZERO).unwrap_err(),
            "No reply arrived within 30 seconds. Try again."
        );
    }

    #[test]
    fn run_lifecycle_events_disarm_the_bound_before_text_arrives() {
        for (kind, event) in [
            ("agent_start", PiChatEvent::TurnStarted),
            ("turn_start", PiChatEvent::Interleaved),
            ("message_start", PiChatEvent::Interleaved),
            ("message_update", PiChatEvent::Interleaved),
        ] {
            let (sender, adapter) = bounded_adapter(Duration::from_secs(1));
            sender.send(json!({"type":kind})).unwrap();
            assert_eq!(adapter.next(Duration::ZERO).unwrap(), event);
            assert!(adapter.first_event_deadline.lock().unwrap().is_none());
        }
    }

    #[test]
    fn routed_messages_report_their_own_selected_model() {
        let fast = crate::model_router::wire::routed_response_id("openai", "gpt-5.6-luna");
        let deep = crate::model_router::wire::routed_response_id("anthropic", "claude-opus-5");
        for (id, provider, model) in [
            (deep, "anthropic", "claude-opus-5"),
            (fast, "openai", "gpt-5.6-luna"),
        ] {
            let frame = json!({"type":"message_end","message":{"role":"assistant","provider":"muniment-router","model":"auto","responseId":id}});
            assert_eq!(
                parse_frame(&frame).unwrap(),
                PiChatEvent::ModelReported {
                    routing: None,
                    classifier: None,
                    provider: provider.into(),
                    model: model.into(),
                    usage: None,
                    cost: None
                }
            );
            // Another provider cannot accidentally consume router metadata.
            let mut direct = frame;
            direct["message"]["provider"] = json!("ollama");
            assert_eq!(
                parse_frame(&direct).unwrap(),
                PiChatEvent::ModelReported {
                    routing: None,
                    classifier: None,
                    provider: "ollama".into(),
                    model: "auto".into(),
                    usage: None,
                    cost: None
                }
            );
        }
        let frame = json!({"type":"message_end","message":{"role":"assistant","provider":"muniment-router","model":"auto","responseId":"muniment-route-v1.invalid"}});
        assert_eq!(
            parse_frame(&frame).unwrap(),
            PiChatEvent::ModelReported {
                routing: None,
                classifier: None,
                provider: "muniment-router".into(),
                model: "auto".into(),
                usage: None,
                cost: None
            }
        );
    }

    #[test]
    fn classifier_usage_follows_its_own_routed_response() {
        let classifier = crate::model_router::wire::ClassifierUsage {
            model: "typesafe/jev-latest".into(),
            tokens: Some(crate::model_router::wire::Tokens {
                input: 1000,
                output: 0,
                ..Default::default()
            }),
            cost: Some(0.000042),
        };
        let id = crate::model_router::wire::classified_response_id(
            "anthropic",
            "claude-sonnet-5",
            Some(&classifier),
        );
        let frame = json!({"type":"message_end","message":{
            "role":"assistant","provider":"muniment-router","model":"auto","responseId":id,
            "usage":{"input":1000,"output":1000,"cost":{"total":0}}
        }});
        let PiChatEvent::ModelReported {
            routing: None,
            classifier: recorded,
            model,
            cost,
            ..
        } = parse_frame(&frame).unwrap()
        else {
            panic!("model usage expected");
        };
        assert_eq!(recorded, Some(classifier));
        assert_eq!(model, "claude-sonnet-5");
        assert_eq!(cost, Some(0.018));
    }

    #[test]
    fn routed_cost_uses_the_selected_model_and_unknown_prices_are_unavailable() {
        for (model, expected) in [("claude-sonnet-5", Some(0.018)), ("private-model", None)] {
            let id = crate::model_router::wire::routed_response_id("anthropic", model);
            let frame = json!({"type":"message_end","message":{
                "role":"assistant","provider":"muniment-router","model":"auto","responseId":id,
                "usage":{"input":1000,"output":1000,"cost":{"total":0}}
            }});
            let PiChatEvent::ModelReported { cost, .. } = parse_frame(&frame).unwrap() else {
                panic!("expected model usage");
            };
            assert_eq!(cost, expected);
        }
    }

    #[test]
    fn a_bridge_run_shows_the_api_price_estimate_not_its_zero() {
        for (model, expected) in [("claude-sonnet-5", Some(0.018)), ("private-model", None)] {
            let frame = json!({"type":"message_end","message":{
                "role":"assistant","provider":"claude-bridge","model":model,
                "usage":{"input":1000,"output":1000,"cost":{"total":0}}
            }});
            let PiChatEvent::ModelReported { provider, cost, .. } = parse_frame(&frame).unwrap()
            else {
                panic!("expected model usage");
            };
            assert_eq!(provider, "claude-bridge");
            assert_eq!(cost, expected, "{model}");
        }
    }

    #[test]
    fn an_assistant_message_end_reports_its_provider_and_model() {
        assert_eq!(
            parse_frame(&json!({"type":"message_end", "message":{
                "role":"assistant", "provider":"ollama", "model":"llama3.2:3b", "stopReason":"stop"
            }}))
            .unwrap(),
            PiChatEvent::ModelReported {
                routing: None,
                classifier: None,
                provider: "ollama".into(),
                model: "llama3.2:3b".into(),
                usage: None,
                cost: None,
            }
        );
        assert_eq!(
            parse_frame(&json!({"type":"message_end", "message":{
                "role":"assistant", "provider":"openai-codex", "model":"gpt-5.5",
                "usage":{"input":11414,"output":64,"cacheRead":0,"cacheWrite":0,"reasoning":0,"totalTokens":11478,
                         "cost":{"input":0.05707,"output":0.00192,"cacheRead":0,"cacheWrite":0,"total":0.05899}}
            }}))
            .unwrap(),
            PiChatEvent::ModelReported {
                    routing: None,
                    classifier: None,
                provider: "openai-codex".into(),
                model: "gpt-5.5".into(),
                usage: Some(TokenUsage {
                    input: 11414,
                    output: 64,
                    cache_read: 0,
                    cache_write: 0,
                    reasoning: 0,
                    total: 11478
                }),
                cost: Some(0.05899),
            }
        );
        assert_eq!(
            parse_frame(&json!({"type":"message_end", "message":{
                "role":"user", "provider":"ollama", "model":"llama3.2:3b"
            }}))
            .unwrap(),
            PiChatEvent::Interleaved
        );
    }

    #[test]
    fn agent_start_is_the_turn_start_and_the_other_lifecycle_frames_interleave() {
        assert_eq!(
            parse_frame(&json!({"type":"agent_start"})).unwrap(),
            PiChatEvent::TurnStarted
        );
        for kind in ["turn_start", "turn_end", "message_start", "message_end"] {
            assert_eq!(
                parse_frame(&json!({"type": kind})).unwrap(),
                PiChatEvent::Interleaved
            );
        }
    }

    #[test]
    fn usage_limit_has_a_safe_terminal_event_without_raw_provider_details() {
        assert_eq!(parse_frame(&json!({"type":"agent_end", "messages":[{
            "role":"assistant", "stopReason":"error",
            "errorMessage":"503: This provider has no remaining allowance. private account details"
        }]})).unwrap(), PiChatEvent::UsageLimit);
    }

    #[test]
    fn a_refused_endpoint_key_reads_as_the_keychain_refusal_without_the_command() {
        // Pi's errorMessage for a declined key and for the first send while
        // the prompt is open: the command exits with the refused or the
        // pending status, and Pi names the command either way.
        let live = r#"API key auth failed for provider custom-live28: Failed to resolve API key for provider "custom-live28" from shell command: "/Applications/muniment.app/Contents/Library/LaunchServices/muniment-runtime" secret get custom-live28"#;
        let detail = provider_error_detail(live).unwrap();
        assert_eq!(
            detail,
            "The keychain refused the key for custom-live28. Answer its prompt if one is open, or choose Try again in Settings → Machines, then send the message again."
        );
        assert!(!detail.contains("muniment-runtime") && !detail.contains("secret get"));
        let event = parse_frame(&json!({"type": "agent_end", "messages": [
            {"role": "assistant", "stopReason": "error", "errorMessage": live}
        ]}))
        .unwrap();
        assert_eq!(event, PiChatEvent::Failed(Some(detail)));
        for error in [
            // Pi's inner sentence alone, and with a timeout after the command.
            r#"Failed to resolve API key for provider "custom-live27" from shell command: "/Applications/muniment.app/Contents/MacOS/muniment-runtime" secret get custom-live27"#,
            r#"API key auth failed for provider custom-live27: Failed to resolve API key for provider "custom-live27" from shell command: "/Applications/muniment.app/Contents/Library/LaunchServices/muniment-runtime" secret get custom-live27: Command timed out"#,
            r#"API key auth failed for provider custom-live27: Failed to resolve API key for provider "custom-live27" from shell command: "C:\Program Files\Muniment\muniment-runtime.exe" secret get custom-live27"#,
        ] {
            let detail = provider_error_detail(error).unwrap();
            assert!(
                detail.starts_with("The keychain refused the key for custom-live27."),
                "{error}"
            );
        }
        // A command the user wrote, another program's `secret get`, or a key
        // for another provider keeps Pi's own sentence.
        for own in [
            r#"API key auth failed for provider custom-own: Failed to resolve API key for provider "custom-own" from shell command: op read key"#,
            r#"API key auth failed for provider custom-own: Failed to resolve API key for provider "custom-own" from shell command: "/usr/local/bin/vault" secret get custom-own"#,
            r#"API key auth failed for provider custom-own: Failed to resolve API key for provider "custom-own" from shell command: "/opt/muniment-runtime" secret get custom-other"#,
            r#"API key auth failed for provider custom-own: Failed to resolve API key for provider "custom-own" from shell command: "/opt/muniment-runtime" secret get custom-own-two"#,
        ] {
            assert!(
                provider_error_detail(own)
                    .unwrap()
                    .contains("Failed to resolve API key"),
                "{own}"
            );
        }
    }

    #[test]
    fn a_stale_endpoint_key_reads_as_out_of_date_under_the_endpoint_name() {
        let state = std::env::temp_dir().join(format!(
            "muniment-stale-key-detail-{}",
            uuid::Uuid::new_v4()
        ));
        let agent = crate::state_root::agent_directory(&state);
        std::fs::create_dir_all(&agent).unwrap();
        std::fs::write(
            agent.join(crate::models::pi_models::RECORD_FILE),
            r#"{"names":{"custom-live30":"Live30"}}"#,
        )
        .unwrap();
        // A refusal names the endpoint by its name.
        assert_eq!(
            key_failure_detail(Some(&state), "custom-live30"),
            "The keychain refused the key for Live30. Answer its prompt if one is open, or choose Try again in Settings → Machines, then send the message again."
        );
        // A stale key asks for the key again in Models, and never for a retry.
        let marks = crate::state_root::stale_endpoint_keys(&state);
        std::fs::create_dir_all(&marks).unwrap();
        std::fs::write(marks.join("custom-live30"), b"").unwrap();
        let stale = key_failure_detail(Some(&state), "custom-live30");
        assert_eq!(
            stale,
            "The saved key for Live30 is out of date. Enter the key again in Settings → Models, then send the message again."
        );
        assert!(!stale.contains("Machines") && !stale.contains("custom-live30"));
        // An endpoint with no name reads by its id.
        std::fs::write(marks.join("custom-unnamed"), b"").unwrap();
        assert!(key_failure_detail(Some(&state), "custom-unnamed")
            .starts_with("The saved key for custom-unnamed is out of date."));
        assert!(key_failure_detail(None, "custom-live30")
            .starts_with("The keychain refused the key for custom-live30."));
        let _ = std::fs::remove_dir_all(state);
    }

    #[test]
    fn a_server_without_tool_calling_reads_as_a_plain_sentence_under_its_name() {
        let state = std::env::temp_dir().join(format!(
            "muniment-tool-calls-detail-{}",
            uuid::Uuid::new_v4()
        ));
        let agent = crate::state_root::agent_directory(&state);
        std::fs::create_dir_all(&agent).unwrap();
        std::fs::write(
            agent.join(crate::models::pi_models::RECORD_FILE),
            r#"{"names":{"custom-do-test":"DO Test"}}"#,
        )
        .unwrap();
        // vLLM's two refusals, Pi's wording of them, and Ollama's.
        for error in [
            r#"400 "auto" tool choice requires --enable-auto-tool-choice and --tool-call-parser to be set"#,
            r#"400: {"object":"error","message":"\"auto\" tool choice requires --enable-auto-tool-choice and --tool-call-parser to be set","type":"BadRequestError","param":null,"code":400}"#,
            r#"400 tool_choice=function "x" requires --tool-call-parser to be set"#,
            "registry.ollama.ai/library/gemma:2b does not support tools",
        ] {
            assert!(refuses_tool_calls(error), "{error}");
        }
        assert!(!refuses_tool_calls("400 model gone"));
        assert!(!refuses_tool_calls(
            "This model's maximum context length is 32768 tokens"
        ));
        assert_eq!(
            tool_calls_refused_detail(Some(&state), Some("custom-do-test")),
            "DO Test does not accept tool calls. Turn on tool calling on the server (for vLLM: --enable-auto-tool-choice --tool-call-parser <parser>) or turn off tools for this endpoint in Settings → Models."
        );
        assert!(tool_calls_refused_detail(None, None)
            .starts_with("The server does not accept tool calls."));
        // The run fails with the sentence, never with vLLM's own text.
        let event = parse_frame(&json!({"type": "agent_end", "messages": [{
            "role": "assistant", "stopReason": "error", "provider": "custom-do-test",
            "errorMessage": r#"400 "auto" tool choice requires --enable-auto-tool-choice and --tool-call-parser to be set"#
        }]}))
        .unwrap();
        let PiChatEvent::Failed(Some(detail)) = event else {
            panic!("{event:?}");
        };
        assert!(detail.contains("does not accept tool calls") && !detail.contains("\"auto\""));
        let _ = std::fs::remove_dir_all(state);
    }

    #[test]
    fn a_provider_error_crosses_as_its_cleaned_sentence() {
        assert_eq!(
            parse_frame(&json!({"type":"agent_end", "messages":[{
                "role":"assistant", "stopReason":"error",
                "errorMessage":"400: {\"message\":\"The plan of Work does not offer gpt-5.5. Choose another model.\",\"type\":\"router_error\"}"
            }]}))
            .unwrap(),
            PiChatEvent::Failed(Some(
                "The plan of Work does not offer gpt-5.5. Choose another model.".into()
            ))
        );
        assert_eq!(
            provider_error_detail(r#"404: {"error":{"message":"model gone","type":"x"}}"#),
            Some("model gone".into())
        );
        assert_eq!(
            provider_error_detail("connection\nreset   by peer"),
            Some("connection reset by peer".into())
        );
        let leaked = provider_error_detail(
            "401: invalid key sk-proj-abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGH",
        )
        .unwrap();
        assert!(!leaked.contains("abcdefghijklmnopqrstuvwxyz"), "{leaked}");
        let long = provider_error_detail(&"word ".repeat(200)).unwrap();
        assert_eq!(long.chars().count(), PROVIDER_DETAIL_CHARS);
        assert!(long.ends_with('…'));
        let words = provider_error_detail(&"abcdefg ".repeat(60)).unwrap();
        assert!(words.ends_with("abcdefg…"), "{words}");
        assert_eq!(provider_error_detail("   "), None);
    }

    #[test]
    fn provider_error_token_limit_and_abort_do_not_become_success_at_agent_end() {
        for (reason, expected) in [
            ("error", PiChatEvent::Failed(Some("private detail".into()))),
            ("length", PiChatEvent::Failed(None)),
            ("aborted", PiChatEvent::Cancelled),
        ] {
            assert_eq!(
                parse_frame(&json!({"type":"agent_end", "messages":[{
                    "role":"assistant", "stopReason":reason, "errorMessage":"private detail"
                }]}))
                .unwrap(),
                expected
            );
        }
        assert_eq!(
            parse_frame(&json!({"type":"message_end", "message":{
                "role":"assistant", "stopReason":"error"
            }}))
            .unwrap(),
            PiChatEvent::Interleaved
        );
        assert_eq!(
            parse_frame(&json!({"type":"agent_end", "messages":[
                {"role":"assistant", "stopReason":"error"},
                {"role":"assistant", "stopReason":"stop"}
            ]}))
            .unwrap(),
            PiChatEvent::Completed
        );
        for role in ["user", "toolResult"] {
            assert_eq!(
                parse_frame(&json!({"type":"message_end", "message":{
                    "role":role, "stopReason":"error"
                }}))
                .unwrap(),
                PiChatEvent::Interleaved
            );
        }
    }

    #[test]
    fn queue_acknowledgements_are_exact_and_non_sensitive() {
        assert!(require_queue_ack(
            &json!({"type":"response", "command":"steer", "success":true}),
            "steer"
        )
        .is_ok());
        for response in [
            json!({"type":"response", "command":"follow_up", "success":true}),
            json!({"type":"response", "command":"steer", "success":false, "message":"secret"}),
            json!({"type":"agent_end"}),
            json!({"command":"steer", "success":true}),
        ] {
            assert_eq!(
                require_queue_ack(&response, "steer").unwrap_err(),
                QUEUE_COMMAND_FAILED
            );
        }
    }
}
