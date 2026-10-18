//! The describer for any tool without one of its own: MCP tools, plugin
//! tools and tools added later. It reads the tool's metadata when the
//! runtime holds it, its title, input schema and presentation hints, and
//! otherwise works from the tool's name and the call's arguments. A
//! headline never holds raw JSON.

use serde_json::{Map, Value};

use super::hints::{fill, PresentationHints, ToolMeta};
use super::text::{
    clip, code, file_name, listing, lower_first, phrase, plural, quoted, short_url, singular,
    title, words,
};
use super::{is_empty, mcp_parts, ToolDetail, ToolPresentation, MAX_DETAILS};

/// The arguments a headline names first, in order.
const SALIENT: [&str; 22] = [
    "query",
    "q",
    "queries",
    "search",
    "prompt",
    "question",
    "title",
    "name",
    "subject",
    "path",
    "file_path",
    "filePath",
    "file",
    "url",
    "uri",
    "urls",
    "command",
    "cmd",
    "code",
    "message",
    "text",
    "id",
];

/// Key endings that also name a salient argument, such as `issueTitle` or
/// `search_query`, in the order of [`SALIENT`].
const SALIENT_ENDINGS: [&str; 7] = ["query", "title", "name", "path", "url", "id", "key"];

/// Values that name a default rather than a choice.
const DEFAULT_WORDS: [&str; 3] = ["auto", "default", "none"];

/// Keys whose values never show.
const SECRET_WORDS: [&str; 7] = [
    "password",
    "secret",
    "token",
    "apikey",
    "authorization",
    "credential",
    "cookie",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Path,
    Url,
    Code,
    Id,
    Text,
}

fn shape(key: &str) -> Shape {
    let lower = key.to_ascii_lowercase();
    let last = words(key).pop().unwrap_or_default();
    if lower == "path"
        || lower == "file"
        || last == "path"
        || last == "file"
        || last == "dir"
        || last == "folder"
    {
        Shape::Path
    } else if ["url", "uri", "urls", "link", "href"].contains(&last.as_str()) {
        Shape::Url
    } else if ["command", "cmd", "code", "script", "sql", "expression"].contains(&last.as_str()) {
        Shape::Code
    } else if ["id", "ids", "key", "number", "ref"].contains(&last.as_str()) {
        Shape::Id
    } else {
        Shape::Text
    }
}

fn secret(key: &str) -> bool {
    let joined: String = words(key).concat();
    SECRET_WORDS.iter().any(|word| joined.contains(word))
}

/// A value as the headline names it.
pub(super) fn salient_text(key: &str, value: &Value) -> Option<String> {
    let one = |text: &str| match shape(key) {
        Shape::Path => file_name(text),
        Shape::Url => short_url(text, 60),
        Shape::Code => code(text, 60),
        Shape::Id => clip(text, 40),
        Shape::Text => quoted(text, 60),
    };
    match value {
        Value::String(text) if !text.trim().is_empty() => Some(one(text)),
        Value::Number(number) => Some(number.to_string()),
        Value::Array(items) => {
            let texts: Vec<&str> = items
                .iter()
                .filter_map(Value::as_str)
                .filter(|text| !text.trim().is_empty())
                .collect();
            let first = one(texts.first()?);
            Some(match texts.len() {
                1 => first,
                n => format!("{first} and {} more", n - 1),
            })
        }
        _ => None,
    }
}

fn properties(meta: Option<&ToolMeta>) -> Option<&Map<String, Value>> {
    meta.and_then(|meta| meta.schema.as_ref())
        .and_then(|schema| schema.get("properties"))
        .and_then(Value::as_object)
}

fn required(meta: Option<&ToolMeta>) -> Vec<String> {
    meta.and_then(|meta| meta.schema.as_ref())
        .and_then(|schema| schema.get("required"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

/// The argument a headline names: the hint's subject, then the first of
/// [`SALIENT`] the call sets, then a key with a salient ending, then the
/// one required text argument, then the only text argument.
fn salient_key(
    input: &Map<String, Value>,
    meta: Option<&ToolMeta>,
    hints: Option<&PresentationHints>,
) -> Option<String> {
    let usable = |key: &str| {
        input
            .get(key)
            .is_some_and(|value| salient_text(key, value).is_some())
    };
    if let Some(subject) = hints
        .and_then(|hints| hints.subject.as_deref())
        .filter(|key| usable(key))
    {
        return Some(subject.to_owned());
    }
    if let Some(key) = SALIENT.iter().find(|key| usable(key)) {
        return Some((*key).to_owned());
    }
    for ending in SALIENT_ENDINGS {
        if let Some(key) = input
            .keys()
            .find(|key| words(key).last().is_some_and(|last| last == ending) && usable(key))
        {
            return Some(key.clone());
        }
    }
    let texts = |keys: Vec<&String>| {
        let texts: Vec<&String> = keys
            .into_iter()
            .filter(|key| input.get(*key).is_some_and(Value::is_string) && usable(key))
            .collect();
        (texts.len() == 1).then(|| texts[0].clone())
    };
    let required = required(meta);
    texts(input.keys().filter(|key| required.contains(key)).collect())
        .or_else(|| texts(input.keys().collect()))
}

/// The label of one argument: its schema title, else its name humanized.
fn label(key: &str, schema: Option<&Value>) -> String {
    schema
        .and_then(|schema| schema.get("title"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty() && title.len() <= 40)
        .map(str::to_owned)
        .unwrap_or_else(|| phrase(key))
}

/// The title an enum value has in the schema, through `oneOf` or `anyOf`
/// entries with `const` and `title`, or `enumNames`.
fn enum_title(value: &Value, schema: Option<&Value>) -> Option<String> {
    let schema = schema?;
    for list in ["oneOf", "anyOf"] {
        for option in schema
            .get(list)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if option.get("const") == Some(value) {
                return option
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
        }
    }
    let index = schema
        .get("enum")?
        .as_array()?
        .iter()
        .position(|option| option == value)?;
    schema
        .get("enumNames")?
        .as_array()?
        .get(index)?
        .as_str()
        .map(str::to_owned)
}

/// One argument as a detail value, or `None` when it carries nothing worth
/// showing: empty, false, the schema's default, or a default word.
pub(super) fn detail_value(
    key: &str,
    value: &Value,
    schema: Option<&Value>,
) -> Option<(String, bool)> {
    if is_empty(value) {
        let default_true =
            schema.and_then(|schema| schema.get("default")) == Some(&Value::Bool(true));
        return (value == &Value::Bool(false) && default_true).then(|| ("No".to_owned(), false));
    }
    if schema.and_then(|schema| schema.get("default")) == Some(value) {
        return None;
    }
    if value
        .as_str()
        .is_some_and(|text| DEFAULT_WORDS.contains(&text.trim().to_ascii_lowercase().as_str()))
    {
        return None;
    }
    if secret(key) {
        return Some(("Hidden".into(), false));
    }
    if let Some(title) = enum_title(value, schema) {
        return Some((title, false));
    }
    let mono = matches!(
        shape(key),
        Shape::Path | Shape::Url | Shape::Code | Shape::Id
    );
    let scalar = |value: &Value| match value {
        Value::String(text) => Some(clip(text, 120)),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(true) => Some("Yes".into()),
        Value::Bool(false) => Some("No".into()),
        _ => None,
    };
    match value {
        Value::Array(items) => {
            let short: Vec<String> = items
                .iter()
                .filter_map(|item| scalar(item).filter(|text| text.chars().count() <= 40))
                .collect();
            if short.len() == items.len() && items.len() <= 3 {
                return Some((listing(&short), mono));
            }
            let noun = lower_first(&label(key, schema));
            let noun = if items.len() == 1 {
                singular(&noun)
            } else {
                plural(&noun)
            };
            Some((format!("{} {noun}", items.len()), false))
        }
        Value::Object(fields) => {
            let parts: Vec<String> = fields
                .iter()
                .filter(|(_, item)| !is_empty(item))
                .filter_map(|(name, item)| {
                    scalar(item)
                        .map(|text| format!("{}: {}", lower_first(&phrase(name)), clip(&text, 40)))
                })
                .collect();
            if !parts.is_empty() && parts.len() == fields.len() && parts.len() <= 3 {
                Some((parts.join(", "), false))
            } else {
                Some((super::text::count(fields.len(), "field", "fields"), false))
            }
        }
        other => scalar(other).map(|text| (text, mono)),
    }
}

/// The labeled details of the arguments a headline does not name, required
/// ones first, at most [`MAX_DETAILS`].
pub(super) fn details(
    input: &Map<String, Value>,
    meta: Option<&ToolMeta>,
    skip: &[&str],
) -> Vec<ToolDetail> {
    let properties = properties(meta);
    let hidden: Vec<String> = meta
        .and_then(|meta| meta.hints.as_ref())
        .map(|hints| hints.hidden.clone())
        .unwrap_or_default();
    let required = required(meta);
    let mut keys: Vec<&String> = input.keys().collect();
    keys.sort_by_key(|key| !required.contains(key));
    keys.into_iter()
        .filter(|key| !skip.contains(&key.as_str()) && !hidden.contains(key))
        .filter_map(|key| {
            let schema = properties.and_then(|properties| properties.get(key.as_str()));
            let (value, mono) = detail_value(key, &input[key.as_str()], schema)?;
            Some(ToolDetail {
                label: label(key, schema),
                value,
                mono,
            })
        })
        .take(MAX_DETAILS)
        .collect()
}

/// A tool's action as a phrase: its title, its label when that is not the
/// name itself, else its name humanized without the server's name.
fn action(local: &str, server: Option<&str>, meta: Option<&ToolMeta>) -> String {
    let named = |text: &Option<String>| {
        text.as_deref()
            .map(|text| text.trim().trim_end_matches('.').to_owned())
            .filter(|text| {
                !text.is_empty()
                    && !text.contains('/')
                    && text.chars().count() <= 48
                    && text != local
                    && words(text).concat() != words(local).concat()
            })
    };
    if let Some(title) = meta.and_then(|meta| named(&meta.title).or_else(|| named(&meta.label))) {
        return super::text::capitalize(&title);
    }
    let mut parts = words(local);
    if let Some(server) = server {
        let prefix = words(server);
        if parts.len() > prefix.len() && parts.starts_with(&prefix) {
            parts.drain(..prefix.len());
        }
    }
    let phrase = phrase(&parts.join("_"));
    if phrase.is_empty() {
        "Use a tool".into()
    } else {
        phrase
    }
}

pub(super) fn describe(tool: &str, input: &Value, meta: Option<&ToolMeta>) -> ToolPresentation {
    let empty = Map::new();
    let args = input.as_object().unwrap_or(&empty);
    let hints = meta.and_then(|meta| meta.hints.as_ref());
    let mcp = mcp_parts(tool);
    let server_id = mcp
        .as_ref()
        .map(|(server, _)| server.clone())
        .filter(|server| !server.is_empty());
    let server = meta
        .and_then(|meta| meta.server.clone())
        .or_else(|| server_id.as_deref().map(title));
    let local = mcp.as_ref().map_or(tool, |(_, name)| name.as_str());
    let action = action(local, server_id.as_deref(), meta);
    let key = salient_key(args, meta, hints);
    let subject = key.as_deref().and_then(|key| salient_text(key, &args[key]));
    let render = |key: &str, value: &Value| {
        salient_text(key, value).unwrap_or_else(|| clip(&value.to_string(), 60))
    };
    let templated = hints
        .and_then(|hints| hints.headline.as_deref())
        .and_then(|template| fill(template, input, render));
    let phrase = templated.clone().unwrap_or_else(|| match &subject {
        Some(subject) => format!("{action} {subject}"),
        None => action.clone(),
    });
    let headline = match &server {
        Some(server) => format!("Ask {server}: {}", lower_first(&phrase)),
        None => phrase,
    };
    let progress = hints
        .and_then(|hints| hints.progress.as_deref())
        .and_then(|template| fill(template, input, render))
        .unwrap_or_else(|| match &server {
            Some(server) => format!("Asking {server}"),
            None => match action.strip_prefix("Use ") {
                Some(rest) => format!("Using {rest}"),
                None => format!("Using {}", lower_first(&action)),
            },
        });
    let template = hints
        .and_then(|hints| hints.headline.as_deref())
        .filter(|_| templated.is_some());
    let skip: Vec<&str> = match (template, key.as_deref()) {
        (Some(template), _) => args
            .keys()
            .map(String::as_str)
            .filter(|key| template.contains(&format!("{{{key}}}")))
            .collect(),
        (None, Some(key)) => vec![key],
        (None, None) => Vec::new(),
    };
    let category = hints
        .and_then(|hints| hints.category.clone())
        .or_else(|| {
            server_id
                .as_ref()
                .map(|server| format!("mcp:{}", words(server).join("-")))
        })
        .unwrap_or_else(|| "other".into());
    let icon = hints
        .and_then(|hints| hints.icon.clone())
        .unwrap_or_else(|| if mcp.is_some() { "server" } else { "wrench" }.into());
    ToolPresentation {
        kind: if mcp.is_some() { "mcp" } else { "other" }.into(),
        category,
        icon,
        headline,
        progress,
        details: details(args, meta, &skip),
        result: None,
        path: None,
    }
}
