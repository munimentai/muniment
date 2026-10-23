//! What a tool says about itself, for the describers: its label, title,
//! description and input schema, the MCP server that serves it, and the
//! presentation hints a tool author may add.
//!
//! The runtime learns this metadata as runs report their tool catalogs and
//! as MCP connection tests list a server's tools. It stays in memory, keyed
//! by the tool name the model calls, and a call with no metadata falls back
//! to its name and arguments alone.

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The most tools the registry keeps.
const MAX_TOOLS: usize = 4096;
/// The longest description or schema the registry keeps, in bytes.
const MAX_META_BYTES: usize = 32 * 1024;

/// The key a tool's presentation hints sit under in an MCP tool's `_meta`
/// or `annotations`.
pub const HINTS_KEY: &str = "muniment/presentation";
/// The key they sit under in a tool's input schema, which reaches the
/// runtime from every kind of tool.
pub const SCHEMA_HINTS_KEY: &str = "x-muniment-presentation";

/// How a tool asks to be shown. Each field is optional, and a template
/// names input properties in braces, such as `Create issue {title}`. A
/// template whose properties are missing from the call is skipped.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PresentationHints {
    /// `web`, `files`, `shell`, `code`, `agent`, `memory` or `other`.
    pub category: Option<String>,
    /// A Lucide icon name.
    pub icon: Option<String>,
    /// The headline, in the imperative: `Create issue {title}`.
    pub headline: Option<String>,
    /// The words while the call runs: `Creating an issue`.
    pub progress: Option<String>,
    /// The input property the headline names when no template fits.
    pub subject: Option<String>,
    /// The result line, naming fields of a JSON result: `Created {id}`.
    pub result: Option<String>,
    /// Input properties the details leave out.
    pub hidden: Vec<String>,
}

/// One tool's metadata.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolMeta {
    /// The label a tool registration gives, such as `Read files`.
    pub label: Option<String>,
    /// An MCP tool's `title`, or its `annotations.title`.
    pub title: Option<String>,
    pub description: Option<String>,
    /// The input JSON Schema.
    pub schema: Option<Value>,
    /// The display name of the MCP server that serves the tool.
    pub server: Option<String>,
    pub hints: Option<PresentationHints>,
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|text| text.chars().take(2048).collect())
}

fn parse_hints(value: Option<&Value>) -> Option<PresentationHints> {
    value
        .filter(|value| value.is_object())
        .and_then(|value| serde_json::from_value(value.clone()).ok())
}

impl ToolMeta {
    /// Reads a tool description in any of the shapes the runtime meets: a
    /// tool registration (`label`, `description`, `parameters`), an MCP
    /// tool (`title`, `description`, `inputSchema`, `annotations`,
    /// `_meta`), or this type's own serialized form.
    pub fn from_value(value: &Value) -> Self {
        let annotations = value.get("annotations");
        let schema = ["schema", "inputSchema", "parameters", "input_schema"]
            .iter()
            .find_map(|key| value.get(*key))
            .filter(|schema| schema.is_object())
            .filter(|schema| schema.to_string().len() <= MAX_META_BYTES)
            .cloned();
        let hints = parse_hints(value.get("hints"))
            .or_else(|| parse_hints(value.get("presentation")))
            .or_else(|| parse_hints(value.pointer("/_meta").and_then(|meta| meta.get(HINTS_KEY))))
            .or_else(|| parse_hints(annotations.and_then(|meta| meta.get(HINTS_KEY))))
            .or_else(|| {
                parse_hints(
                    schema
                        .as_ref()
                        .and_then(|schema| schema.get(SCHEMA_HINTS_KEY)),
                )
            });
        Self {
            label: text(value, "label"),
            title: text(value, "title").or_else(|| annotations.and_then(|a| text(a, "title"))),
            description: text(value, "description"),
            schema,
            server: text(value, "server"),
            hints,
        }
    }
}

fn registry() -> &'static RwLock<HashMap<String, ToolMeta>> {
    static REGISTRY: OnceLock<RwLock<HashMap<String, ToolMeta>>> = OnceLock::new();
    REGISTRY.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Keeps `meta` for the tool the model calls `name`. A later report for the
/// same name replaces it, and a full registry keeps the tools it holds.
pub fn remember(name: &str, meta: ToolMeta) {
    let name = name.trim();
    if name.is_empty() || name.len() > 256 {
        return;
    }
    let Ok(mut tools) = registry().write() else {
        return;
    };
    if tools.len() >= MAX_TOOLS && !tools.contains_key(name) {
        return;
    }
    tools.insert(name.to_owned(), meta);
}

/// Keeps each tool of a reported catalog: an array of tool descriptions
/// that each carry a `name`.
pub fn remember_catalog(tools: &Value) {
    for tool in tools.as_array().into_iter().flatten() {
        if let Some(name) = tool.get("name").and_then(Value::as_str) {
            remember(name, ToolMeta::from_value(tool));
        }
    }
}

/// The metadata the runtime holds for `name`.
pub fn lookup(name: &str) -> Option<ToolMeta> {
    registry().read().ok()?.get(name).cloned()
}

fn agent_names() -> &'static RwLock<HashMap<String, String>> {
    static NAMES: OnceLock<RwLock<HashMap<String, String>>> = OnceLock::new();
    NAMES.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Keeps the name of the agent `id`, so a call that names the agent by its
/// id reads with its name. A full registry keeps the agents it holds.
pub fn remember_agent(id: &str, name: &str) {
    let (id, name) = (id.trim(), name.trim());
    if id.is_empty() || name.is_empty() || id.len() > 64 || name.len() > 256 {
        return;
    }
    let Ok(mut names) = agent_names().write() else {
        return;
    };
    if names.len() >= MAX_TOOLS && !names.contains_key(id) {
        return;
    }
    names.insert(id.to_owned(), name.to_owned());
}

/// The name the runtime holds for the agent `id`.
pub fn agent_name(id: &str) -> Option<String> {
    agent_names().read().ok()?.get(id.trim()).cloned()
}

/// Fills a template's `{property}` slots from `values`. `None` when a slot
/// names a property that is missing or empty.
pub fn fill(
    template: &str,
    values: &Value,
    render: impl Fn(&str, &Value) -> String,
) -> Option<String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let end = rest[start..].find('}')? + start;
        let key = rest[start + 1..end].trim();
        let value = values.get(key).filter(|value| !super::is_empty(value))?;
        out.push_str(&render(key, value));
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    let out = out.trim().to_owned();
    (!out.is_empty()).then_some(out)
}
