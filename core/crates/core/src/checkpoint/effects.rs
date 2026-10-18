//! The tool calls a rewind cannot take back.

use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

/// The longest label one effect carries.
const MAX_LABEL_CHARS: usize = 160;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IrreversibleKind {
    /// A shell command. Its changes inside the folder are restored; anything
    /// else it did, such as a push, an install or a process, stays.
    Command,
    /// A web search, a page fetch or a browser step.
    Network,
    /// A message to another agent or thread.
    Message,
    /// A tool of an MCP server or another extension.
    External,
    /// A file a file tool changed outside the folder.
    OutsideFolder,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IrreversibleEffect {
    pub kind: IrreversibleKind,
    /// The command, the query, the tool or the path.
    pub label: String,
}

/// Muniment's own tools that change nothing a rewind should name: reads,
/// memory, plans and questions.
const SILENT_TOOLS: &[&str] = &[
    "read",
    "grep",
    "find",
    "ls",
    "memory-search",
    "memory-profile-read",
    "memory-save",
    "memory-delete",
    "memory-profile-save",
    "agent-list",
    "agent-read",
    "agent-memory",
    "routine-list",
    "bg_status",
    "bg_logs",
    "ask_user_question",
    "creation-plan",
    "plan_submit",
    "compact_context",
];

const WEB_TOOLS: &[&str] = &[
    "web_search",
    "fetch_content",
    "get_search_content",
    "muniment:browser",
    "browser",
];

const COMMAND_TOOLS: &[&str] = &["bash", "powershell", "bg_run", "shell", "exec"];

const MESSAGE_TOOLS: &[&str] = &["agent-message", "agent-handoff"];

const FILE_TOOLS: &[&str] = &["write", "edit", "apply_patch"];

/// What one tool call did that a rewind of `folder` leaves in place, if
/// anything.
pub fn irreversible_effect(
    tool: &str,
    input: Option<&str>,
    folder: &Path,
) -> Option<IrreversibleEffect> {
    let tool = tool.trim();
    let input: Value = input
        .and_then(|input| serde_json::from_str(input).ok())
        .unwrap_or(Value::Null);
    let field = |names: &[&str]| {
        names
            .iter()
            .find_map(|name| input.get(*name).and_then(Value::as_str))
            .map(str::to_owned)
    };
    let lowered = tool.to_ascii_lowercase();
    let effect = |kind, label: Option<String>| {
        Some(IrreversibleEffect {
            kind,
            label: bounded(&label.unwrap_or_else(|| tool.to_owned())),
        })
    };
    if SILENT_TOOLS.contains(&lowered.as_str()) {
        return None;
    }
    if COMMAND_TOOLS.contains(&lowered.as_str()) {
        return effect(IrreversibleKind::Command, field(&["command"]));
    }
    if WEB_TOOLS.contains(&lowered.as_str()) {
        let queries = input
            .get("queries")
            .and_then(Value::as_array)
            .map(|queries| {
                queries
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|queries| !queries.is_empty());
        return effect(
            IrreversibleKind::Network,
            field(&["url", "query"]).or(queries),
        );
    }
    if MESSAGE_TOOLS.contains(&lowered.as_str()) {
        return effect(
            IrreversibleKind::Message,
            field(&["to", "agent", "agent_id"]),
        );
    }
    if FILE_TOOLS.contains(&lowered.as_str()) {
        let path = field(&["path", "file_path", "filePath"])?;
        return (!inside(folder, &path)).then(|| IrreversibleEffect {
            kind: IrreversibleKind::OutsideFolder,
            label: bounded(&path),
        });
    }
    if let Some(rest) = tool.strip_prefix("mcp__") {
        let label = rest.replacen("__", " · ", 1).replace(['_', '-'], " ");
        return effect(IrreversibleKind::External, Some(format!("MCP {label}")));
    }
    effect(IrreversibleKind::External, None)
}

/// Whether a file tool's path, relative to the folder or absolute, lies
/// inside the folder.
fn inside(folder: &Path, path: &str) -> bool {
    let joined = folder.join(path);
    let mut resolved = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            component => resolved.push(component),
        }
    }
    canonical(&resolved).starts_with(canonical(folder))
}

/// A path with its deepest existing folder resolved, so a link such as
/// macOS's `/var` names one place.
fn canonical(path: &Path) -> PathBuf {
    let mut existing = path;
    let mut rest = Vec::new();
    loop {
        if let Ok(resolved) = std::fs::canonicalize(existing) {
            return rest
                .iter()
                .rev()
                .fold(resolved, |path, name| path.join(name));
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_owned());
                existing = parent;
            }
            _ => return path.to_owned(),
        }
    }
}

fn bounded(label: &str) -> String {
    let single: String = label.split_whitespace().collect::<Vec<_>>().join(" ");
    if single.chars().count() <= MAX_LABEL_CHARS {
        return single;
    }
    let mut cut: String = single.chars().take(MAX_LABEL_CHARS - 1).collect();
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_memory_leave_nothing_to_name() {
        let folder = Path::new("/work");
        assert_eq!(
            irreversible_effect("read", Some(r#"{"path":"a"}"#), folder),
            None
        );
        assert_eq!(irreversible_effect("memory-save", None, folder), None);
    }

    #[test]
    fn commands_web_messages_and_mcp_tools_are_named() {
        let folder = Path::new("/work");
        let command =
            irreversible_effect("bash", Some(r#"{"command":"git push\n origin"}"#), folder)
                .unwrap();
        assert_eq!(command.kind, IrreversibleKind::Command);
        assert_eq!(command.label, "git push origin");
        let web =
            irreversible_effect("web_search", Some(r#"{"queries":["a","b"]}"#), folder).unwrap();
        assert_eq!(
            (web.kind, web.label.as_str()),
            (IrreversibleKind::Network, "a, b")
        );
        let message =
            irreversible_effect("agent-message", Some(r#"{"to":"Scout"}"#), folder).unwrap();
        assert_eq!(message.kind, IrreversibleKind::Message);
        let mcp = irreversible_effect("mcp__github__create_issue", None, folder).unwrap();
        assert_eq!(
            (mcp.kind, mcp.label.as_str()),
            (IrreversibleKind::External, "MCP github · create issue")
        );
    }

    #[test]
    fn a_file_tool_names_only_a_path_outside_the_folder() {
        let folder = std::env::temp_dir().join(format!("effects-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&folder).unwrap();
        assert_eq!(
            irreversible_effect("write", Some(r#"{"path":"notes/a.md"}"#), &folder),
            None
        );
        let outside =
            irreversible_effect("edit", Some(r#"{"path":"../elsewhere/b.md"}"#), &folder).unwrap();
        std::fs::remove_dir_all(&folder).unwrap();
        assert_eq!(outside.kind, IrreversibleKind::OutsideFolder);
        assert_eq!(outside.label, "../elsewhere/b.md");
    }
}
