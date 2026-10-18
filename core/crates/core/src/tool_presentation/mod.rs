//! What a tool call says to a person, for every tool, including MCP and
//! plugin tools the runtime has never seen.
//!
//! A presentation names the call's category and icon, a short headline in
//! the imperative ("Read README.md"), the words while it runs, a few
//! labeled details and, once it ends, a one-line result ("Exit 0 in 4.2 s").
//! The raw input and output stay with the call for its details. Hosts
//! render the presentation as it comes and compute none of it.
//!
//! Muniment's own tools and the harness tools have describers. Any other
//! tool takes the generic describer, which reads the tool's metadata when
//! the runtime holds it (see [`hints`]) and its name and arguments always.

use serde::{Deserialize, Serialize};
use serde_json::Value;

mod builtin;
mod generic;
pub mod hints;
mod result;
#[cfg(test)]
mod tests;
pub mod text;

pub use hints::{lookup, remember, remember_catalog, PresentationHints, ToolMeta};

use crate::journal::reducer::{ToolActivity, ToolActivityStatus};
use crate::journal::ThreadPermissionResource;
use crate::permission_gate::PolicyRequest;

/// The most labeled details one call shows.
pub const MAX_DETAILS: usize = 6;

/// One call as a person reads it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolPresentation {
    /// `web`, `files`, `shell`, `code`, `agent`, `memory`, `mcp:<server>`
    /// or `other`.
    pub category: String,
    /// The group the transcript folds the call into: `read`, `search`,
    /// `list`, `edit`, `command`, `web`, `compact`, `agent`, `memory`,
    /// `mcp` or `other`.
    pub kind: String,
    /// A Lucide icon name.
    pub icon: String,
    /// What the call does, in the imperative: `Search the web for "x"`.
    pub headline: String,
    /// What the call does while it runs: `Searching the web`.
    pub progress: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<ToolDetail>,
    /// How the call ended, once it has: `Found 8 results`, `Failed: ...`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// The file a host may open for the call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// How the call ran when another call made it, such as a tool call a
    /// script makes: `via script`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
}

/// One labeled value of a call. `mono` marks a record, such as a path or a
/// command, that renders in the record face.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolDetail {
    pub label: String,
    pub value: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub mono: bool,
}

impl ToolDetail {
    pub fn new(label: &str, value: impl Into<String>) -> Self {
        Self {
            label: label.to_owned(),
            value: value.into(),
            mono: false,
        }
    }

    pub fn mono(label: &str, value: impl Into<String>) -> Self {
        Self {
            mono: true,
            ..Self::new(label, value)
        }
    }
}

/// Where a call stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallStatus {
    /// It waits for an answer or has not started.
    Pending,
    Running,
    Completed,
    Failed,
}

/// How one call went: its state, its output text and how long it ran.
#[derive(Clone, Copy, Debug)]
pub struct CallOutcome<'a> {
    pub status: CallStatus,
    pub output: Option<&'a str>,
    pub seconds: Option<f64>,
}

impl CallOutcome<'_> {
    pub const PENDING: CallOutcome<'static> = CallOutcome {
        status: CallStatus::Pending,
        output: None,
        seconds: None,
    };
}

/// Whether a value carries nothing worth showing: null, an empty or blank
/// string, false, or an empty array or object.
pub fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => true,
        Value::String(text) => text.trim().is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(fields) => fields.is_empty(),
        _ => false,
    }
}

/// Describes a call that has not run, with the metadata the runtime holds
/// for the tool.
pub fn describe(tool: &str, input: &Value) -> ToolPresentation {
    describe_with(tool, input, lookup(tool).as_ref(), CallOutcome::PENDING)
}

/// Describes a call with the given metadata and outcome. Muniment's own
/// tools and the harness tools take their describers, then a tool's own
/// hints apply, then the generic describer.
pub fn describe_with(
    tool: &str,
    input: &Value,
    meta: Option<&ToolMeta>,
    outcome: CallOutcome<'_>,
) -> ToolPresentation {
    let empty = Value::Object(Default::default());
    let input = if input.is_object() { input } else { &empty };
    let mut presentation =
        builtin::describe(tool, input).unwrap_or_else(|| generic::describe(tool, input, meta));
    presentation.details.truncate(MAX_DETAILS);
    if presentation.kind == "compact" {
        presentation.headline = match outcome.status {
            CallStatus::Failed => "Context compaction did not finish",
            _ if input.get("reason").and_then(Value::as_str) == Some("manual") => {
                "Context compacted"
            }
            _ => "Context automatically compacted",
        }
        .into();
    }
    presentation.result = result::summary(tool, input, meta, &presentation, outcome);
    presentation
}

/// Describes one call of a run's transcript.
pub fn describe_activity(activity: &ToolActivity) -> ToolPresentation {
    let tool = activity.display_name.as_deref().unwrap_or_default();
    let input = activity
        .input
        .as_deref()
        .and_then(|input| serde_json::from_str::<Value>(input).ok())
        .unwrap_or(Value::Null);
    let seconds = match (&activity.started_at, &activity.finished_at) {
        (Some(start), Some(end)) => seconds_between(start, end),
        _ => None,
    };
    let status = match activity.status {
        ToolActivityStatus::Running => CallStatus::Running,
        ToolActivityStatus::Completed => CallStatus::Completed,
        ToolActivityStatus::Failed => CallStatus::Failed,
    };
    describe_with(
        tool,
        &input,
        lookup(tool).as_ref(),
        CallOutcome {
            status,
            output: activity.output.as_deref(),
            seconds,
        },
    )
}

/// The note a call another call made carries: `via script` under a
/// codemode script, else the kind of call that made it.
pub fn via(parent: &ToolPresentation) -> String {
    match parent.category.as_str() {
        "code" => "via script".into(),
        "agent" => "via agent".into(),
        _ => "via another tool".into(),
    }
}

/// Describes the exact operation a permission gate asks about: a shell
/// command in its folder, a file operation on its path, or a tool and its
/// input.
pub fn describe_request(request: &PolicyRequest) -> ToolPresentation {
    match &request.resource {
        ThreadPermissionResource::Command {
            arguments,
            working_directory,
        } => {
            let (shell, rest) = arguments
                .split_first()
                .map_or(("bash", &[][..]), |(first, rest)| (first.as_str(), rest));
            let input = serde_json::json!({
                "command": rest.join(" "),
                "cwd": working_directory,
            });
            describe(shell, &input)
        }
        ThreadPermissionResource::Path { path, operation } => {
            describe(operation, &serde_json::json!({ "path": path }))
        }
        ThreadPermissionResource::Tool { name, input } => describe(name, input),
    }
}

fn seconds_between(start: &str, end: &str) -> Option<f64> {
    let start = chrono::DateTime::parse_from_rfc3339(start).ok()?;
    let end = chrono::DateTime::parse_from_rfc3339(end).ok()?;
    let millis = (end - start).num_milliseconds();
    (millis >= 0).then(|| millis as f64 / 1000.0)
}

/// The server name and the tool's own name of an MCP tool, which the
/// harness names `mcp__<server>__<tool>`. A server takes its extension's
/// name, and older runs named it by its item id, which reads as no name.
pub fn mcp_parts(tool: &str) -> Option<(String, String)> {
    let rest = tool.strip_prefix("mcp__")?;
    let (server, name) = rest.split_once("__")?;
    let server = strip_item_id(server);
    Some((server.to_owned(), name.to_owned()))
}

/// A server name without the `extend-<uuid>` prefix of an item id.
fn strip_item_id(server: &str) -> &str {
    let Some(rest) = server.strip_prefix("extend-") else {
        return server;
    };
    let uuid = rest.get(..36).filter(|id| {
        id.chars().enumerate().all(|(index, c)| match index {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
    });
    match uuid {
        Some(_) => rest[36..].trim_start_matches('-'),
        None => server,
    }
}
