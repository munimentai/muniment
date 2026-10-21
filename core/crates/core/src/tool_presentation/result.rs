//! The one-line result of a call that ended: what a known tool returned,
//! such as `Read 120 lines` or `Exit 0 in 4.2 s`, a failure's message, or a
//! summary of a common output shape for any other tool.

use serde_json::Value;

use super::hints::{fill, ToolMeta};
use super::text::{clip, count, duration, first_line, lower_first, plural, quoted, singular};
use super::{CallOutcome, CallStatus, ToolPresentation};

/// Fields of a JSON result that hold its list of records.
const LIST_FIELDS: [&str; 16] = [
    "results",
    "items",
    "data",
    "records",
    "rows",
    "issues",
    "files",
    "matches",
    "entries",
    "messages",
    "events",
    "pages",
    "documents",
    "nodes",
    "users",
    "tasks",
];

/// The result line of a call, or `None` while it has not ended.
pub(super) fn summary(
    tool: &str,
    input: &Value,
    meta: Option<&ToolMeta>,
    presentation: &ToolPresentation,
    outcome: CallOutcome<'_>,
) -> Option<String> {
    let output = outcome.output.unwrap_or_default();
    match outcome.status {
        CallStatus::Pending | CallStatus::Running => None,
        // A compaction's headline says how it ended, and hosts show its counts.
        _ if presentation.kind == "compact" => None,
        CallStatus::Failed => Some(
            super::builtin::failure(tool, output, outcome.seconds)
                .unwrap_or_else(|| failure(output)),
        ),
        CallStatus::Completed => super::builtin::result(tool, input, output, outcome.seconds)
            .or_else(|| hinted(meta, output))
            .or_else(|| Some(shape(output, presentation))),
    }
}

/// `Failed: <the error's message>`, or `Failed` when it carries none.
pub(super) fn failure(output: &str) -> String {
    failure_cased(output, lower_first)
}

/// `Failed: <the error's message>` with the message's first letter as
/// `case` sets it.
pub(super) fn failure_cased(output: &str, case: impl Fn(&str) -> String) -> String {
    let message = serde_json::from_str::<Value>(output)
        .ok()
        .and_then(|value| error_message(&value))
        .unwrap_or_else(|| first_line(output, 100));
    // The first sentence carries the cause.
    let message = message.split(". ").next().unwrap_or_default();
    let message = message
        .trim()
        .trim_start_matches("Error:")
        .trim_start_matches("error:")
        .trim()
        .trim_end_matches('.')
        .to_owned();
    if message.is_empty() {
        "Failed".into()
    } else {
        format!("Failed: {}", case(&clip(&message, 100)))
    }
}

fn error_message(value: &Value) -> Option<String> {
    let field = |value: &Value| {
        value
            .get("message")
            .or_else(|| value.get("error_description"))
            .or_else(|| value.get("detail"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    match value.get("error") {
        Some(Value::String(message)) => Some(message.clone()),
        Some(error) if error.is_object() => field(error),
        _ => field(value),
    }
}

fn hinted(meta: Option<&ToolMeta>, output: &str) -> Option<String> {
    let template = meta?.hints.as_ref()?.result.as_deref()?;
    let value = serde_json::from_str::<Value>(output).ok()?;
    fill(template, &value, |_, value| match value {
        Value::String(text) => clip(text, 60),
        other => clip(&other.to_string(), 60),
    })
}

/// A result from its shape: a JSON list counts its items, a JSON object
/// counts its list field or names its record, and text reads its first
/// line.
fn shape(output: &str, presentation: &ToolPresentation) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return "Done".into();
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        if let Some(line) = json_shape(&value) {
            return line;
        }
    }
    let line = first_line(trimmed, 80);
    if line.is_empty() || line == presentation.headline {
        "Done".into()
    } else {
        line
    }
}

fn json_shape(value: &Value) -> Option<String> {
    match value {
        Value::Array(items) => Some(count(items.len(), "item", "items")),
        Value::Object(fields) => {
            if let Some((name, items)) = LIST_FIELDS.iter().find_map(|name| {
                fields
                    .get(*name)
                    .and_then(Value::as_array)
                    .map(|items| (*name, items))
            }) {
                let noun = if name == "data" || name == "nodes" {
                    "items"
                } else {
                    name
                };
                let noun = if items.len() == 1 {
                    singular(noun)
                } else {
                    plural(noun)
                };
                return Some(format!("{} {noun}", items.len()));
            }
            for key in ["title", "name", "subject"] {
                if let Some(text) = fields
                    .get(key)
                    .and_then(Value::as_str)
                    .filter(|text| !text.trim().is_empty())
                {
                    return Some(format!("Returned {}", quoted(text, 60)));
                }
            }
            if let Some(id) = fields
                .get("id")
                .filter(|id| id.is_string() || id.is_number())
            {
                let id = id.as_str().map_or_else(|| id.to_string(), str::to_owned);
                return Some(format!("Returned {}", clip(&id, 40)));
            }
            Some(count(fields.len(), "field", "fields"))
        }
        Value::String(text) => Some(first_line(text, 80)).filter(|line| !line.is_empty()),
        Value::Bool(true) => Some("Done".into()),
        _ => None,
    }
}

/// `in 4.2 s`, or nothing when the call recorded no time.
pub(super) fn took(seconds: Option<f64>) -> String {
    seconds
        .map(duration)
        .filter(|text| !text.is_empty())
        .map(|text| format!(" in {text}"))
        .unwrap_or_default()
}
