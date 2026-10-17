use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadPermissionDecision {
    Allow,
    Deny,
    /// Always asks. It outranks every matching allow policy, as an owner
    /// always-ask rule does, and never answers a gate by itself.
    Ask,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadPermissionRequestKind {
    Path,
    Command,
    /// A call of a tool that neither reads or writes one path nor runs one
    /// command, such as an MCP tool.
    Tool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ThreadPermissionResource {
    Path {
        path: String,
        operation: String,
    },
    Command {
        arguments: Vec<String>,
        working_directory: String,
    },
    /// The exact tool name and the exact JSON input of its call.
    Tool {
        name: String,
        input: serde_json::Value,
    },
}

/// Whether a request kind and its resource form a valid, exact policy
/// resource: an absolute path without `.` or `..` and an operation, an
/// argument vector and an absolute working directory, or a tool name.
pub fn valid_policy_resource(
    request_kind: &ThreadPermissionRequestKind,
    resource: &ThreadPermissionResource,
) -> bool {
    fn canonical_absolute_path(value: &str) -> bool {
        let path = std::path::Path::new(value);
        !value.is_empty()
            && path.is_absolute()
            && !path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::CurDir | std::path::Component::ParentDir
                )
            })
    }
    match (request_kind, resource) {
        (ThreadPermissionRequestKind::Path, ThreadPermissionResource::Path { path, operation }) => {
            canonical_absolute_path(path) && !operation.trim().is_empty()
        }
        (
            ThreadPermissionRequestKind::Command,
            ThreadPermissionResource::Command {
                arguments,
                working_directory,
            },
        ) => {
            arguments
                .first()
                .is_some_and(|argument| !argument.is_empty())
                && arguments.iter().all(|argument| !argument.contains('\0'))
                && canonical_absolute_path(working_directory)
        }
        (ThreadPermissionRequestKind::Tool, ThreadPermissionResource::Tool { name, .. }) => {
            !name.trim().is_empty() && name.len() <= 256 && !name.chars().any(char::is_control)
        }
        _ => false,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadPermissionAnswerSource {
    Native,
    Acp,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThreadPermissionPolicyDecided {
    pub policy_id: String,
    pub decision: ThreadPermissionDecision,
    pub request_kind: ThreadPermissionRequestKind,
    pub resource: ThreadPermissionResource,
    pub actor: String,
    pub answer_source: ThreadPermissionAnswerSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThreadPermissionPolicyRevoked {
    pub policy_id: String,
    pub actor: String,
    pub reason: String,
}

/// The event that sets a thread's permission mode.
pub const THREAD_PERMISSION_MODE_SET: &str = "thread.permission.mode.set";

/// A thread's permission mode, set by a person or by approving a plan. The
/// latest one holds for every later call in the thread.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThreadPermissionModeSet {
    pub mode: crate::permission_gate::PermissionMode,
    pub actor: String,
    /// `user` when a person chose it, `plan` when approving a plan did.
    pub source: String,
}
