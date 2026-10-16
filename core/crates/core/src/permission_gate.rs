//! Permission gate coordination for a chat run.
//!
//! A Pi extension-UI request opens a gate. The gate stays open until an answer
//! arrives for its id and matches its dialog shape. This state is
//! runtime-service state, so the rules live here rather than in the desktop
//! shell.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;

use crate::journal::pi_translation::permission_journal_payload;
use crate::journal::{
    ThreadPermissionDecision, ThreadPermissionPolicyDecided, ThreadPermissionRequestKind,
    ThreadPermissionResource,
};
use crate::sidecar::pi_chat::{
    ExtensionUiAnswer, ExtensionUiRequest, ExtensionUiResponse, PiChatEvent,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ChatPermissionAnswer {
    Select(String),
    Confirm(bool),
    Input(String),
    Editor(String),
    Cancelled,
    CodeDiff {
        gate_id: String,
        effect_id: String,
        code_diff_id: String,
        diff_sha256: String,
        write_plan_sha256: String,
    },
}

impl ChatPermissionAnswer {
    pub fn pi_answer(&self) -> Option<ExtensionUiAnswer> {
        Some(match self {
            Self::Select(value) => ExtensionUiAnswer::Selection(value.clone()),
            Self::Confirm(value) => ExtensionUiAnswer::Confirmation(*value),
            Self::Input(value) => ExtensionUiAnswer::Input(value.clone()),
            Self::Editor(value) => ExtensionUiAnswer::Editor(value.clone()),
            Self::Cancelled => ExtensionUiAnswer::Cancelled,
            Self::CodeDiff { .. } => return None,
        })
    }

    pub fn decision(&self) -> Value {
        serde_json::to_value(self).expect("permission answers serialize")
    }
}

#[derive(Clone, Debug)]
pub struct PendingPermissionAnswer {
    pub gate_id: String,
    pub answer: ChatPermissionAnswer,
    pub resolved: Option<std::sync::mpsc::SyncSender<Option<u64>>>,
}

/// The caller could not journal a gate event. The run stops, and the gate keeps
/// the state the failed append found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JournalAppendFailed;

/// Opens a gate for a Pi extension-UI request. The caller journals the request
/// through `append` before the gate becomes the pending one. The journal holds
/// one open gate per run, so a request that arrives while another waits, as
/// two parallel tool calls each asking a question do, joins `waiting` and
/// opens when the pending one resolves.
pub fn coordinate_extension_ui_request(
    event: PiChatEvent,
    pending: &mut Option<ExtensionUiRequest>,
    waiting: &mut VecDeque<ExtensionUiRequest>,
    append: impl FnOnce(&str, Value) -> Result<(), ()>,
) -> Result<(), JournalAppendFailed> {
    let PiChatEvent::ExtensionUiRequest(request) = event else {
        return Ok(());
    };
    if pending.is_some() {
        waiting.push_back(request);
        return Ok(());
    }
    append("permission.requested", permission_journal_payload(&request))
        .map_err(|()| JournalAppendFailed)?;
    *pending = Some(request);
    Ok(())
}

/// Opens the next waiting request once no gate is pending.
pub fn open_waiting_request(
    pending: &mut Option<ExtensionUiRequest>,
    waiting: &mut VecDeque<ExtensionUiRequest>,
    append: impl FnOnce(&str, Value) -> Result<(), ()>,
) -> Result<(), JournalAppendFailed> {
    if pending.is_some() {
        return Ok(());
    }
    let Some(next) = waiting.pop_front() else {
        return Ok(());
    };
    coordinate_extension_ui_request(
        PiChatEvent::ExtensionUiRequest(next),
        pending,
        waiting,
        append,
    )
}

/// Closes the open gate when the queued answer names it and fits its dialog.
/// A stale gate id, a rejected answer shape, or a failed send leaves the gate
/// open. Each of those three cases resolves the waiting caller with no
/// committed sequence number.
pub fn coordinate_permission_answer(
    pending: &mut Option<ExtensionUiRequest>,
    queued: PendingPermissionAnswer,
    send: impl FnOnce(&ExtensionUiRequest, ExtensionUiAnswer) -> Result<(), String>,
    append: impl FnOnce(&str, Value) -> Result<u64, ()>,
) -> Result<(), JournalAppendFailed> {
    let resolved = queued.resolved;
    let Some(request) = pending
        .as_ref()
        .filter(|request| request.id == queued.gate_id)
    else {
        if let Some(resolved) = resolved {
            let _ = resolved.send(None);
        }
        return Ok(());
    };
    let Some(answer) = queued.answer.pi_answer() else {
        if let Some(resolved) = resolved {
            let _ = resolved.send(None);
        }
        return Ok(());
    };
    if ExtensionUiResponse::new(request, answer.clone()).is_err() {
        if let Some(resolved) = resolved {
            let _ = resolved.send(None);
        }
        return Ok(());
    }
    if send(request, answer).is_err() {
        if let Some(resolved) = resolved {
            let _ = resolved.send(None);
        }
        return Ok(());
    }
    let committed_seq = append(
        "permission.resolved",
        json!({"gate_id": queued.gate_id, "decision": queued.answer.decision()}),
    )
    .map_err(|()| JournalAppendFailed)?;
    if let Some(resolved) = resolved {
        let _ = resolved.send(Some(committed_seq));
    }
    *pending = None;
    Ok(())
}

/// How much a run asks before a tool call that no thread policy decides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateMode {
    /// Reads run. Every other call asks.
    Ask,
    /// Every call runs.
    #[default]
    Auto,
    /// Every call asks, reads included.
    Strict,
}

/// One pre-decided thread policy, without the identity a journaled
/// decision carries: the exact request a policy matches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyRequest {
    pub request_kind: ThreadPermissionRequestKind,
    pub resource: ThreadPermissionResource,
}

/// The gate one run applies: its mode, whether an event started it, and
/// the pre-decided policies that hold in every thread of its agent. The
/// thread's own journaled policies join these at each call.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunPermissions {
    pub mode: GateMode,
    pub event_triggered: bool,
    pub policies: Vec<ThreadPermissionPolicyDecided>,
}

impl RunPermissions {
    /// Whether any call can be asked about or denied, so the run loads the gate.
    pub fn gates(&self) -> bool {
        self.event_triggered
            || self.mode != GateMode::Auto
            || self
                .policies
                .iter()
                .any(|policy| policy.decision != ThreadPermissionDecision::Allow)
    }
}

/// The gate's answer for one tool call.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum GateDecision {
    Allow,
    /// A deny policy refuses the call without asking.
    Deny {
        reason: String,
    },
    /// The user answers at the gate.
    Ask {
        reason: String,
    },
}

/// Tools that read and change nothing.
const READ_ONLY_TOOLS: &[&str] = &[
    "read",
    "grep",
    "find",
    "ls",
    "web_search",
    "fetch_content",
    "get_search_content",
    "memory-search",
    "memory-profile-read",
    "agent-list",
    "agent-read",
    "routine-list",
    "bg_status",
    "bg_logs",
    "ask_user_question",
    "mcp__record__sql",
];

/// The exact policy request one tool call makes: a shell command as its
/// argument vector in the working directory, a file tool's absolute path
/// and operation, or any other tool's name and exact input.
pub fn tool_request(
    tool: &str,
    input: &Value,
    working_directory: &std::path::Path,
) -> PolicyRequest {
    let field = |names: &[&str]| {
        names
            .iter()
            .find_map(|name| input.get(*name).and_then(Value::as_str))
    };
    let absolute = |path: &str| {
        let mut resolved = std::path::PathBuf::new();
        for component in working_directory.join(path).components() {
            match component {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    resolved.pop();
                }
                component => resolved.push(component),
            }
        }
        resolved.to_string_lossy().into_owned()
    };
    let request = match tool {
        "bash" | "powershell" | "bg_run" => field(&["command"]).map(|command| PolicyRequest {
            request_kind: ThreadPermissionRequestKind::Command,
            resource: ThreadPermissionResource::Command {
                arguments: vec![tool.to_owned(), command.to_owned()],
                working_directory: absolute("."),
            },
        }),
        "read" | "write" | "edit" | "ls" | "find" | "grep" => {
            field(&["path", "file_path"]).map(|path| PolicyRequest {
                request_kind: ThreadPermissionRequestKind::Path,
                resource: ThreadPermissionResource::Path {
                    path: absolute(path),
                    operation: tool.to_owned(),
                },
            })
        }
        _ => None,
    };
    request.unwrap_or_else(|| PolicyRequest {
        request_kind: ThreadPermissionRequestKind::Tool,
        resource: ThreadPermissionResource::Tool {
            name: tool.to_owned(),
            input: input.clone(),
        },
    })
}

/// Decides one tool call from the policies in force, oldest first, and the
/// run's mode. A matching ask policy always asks, even when an allow policy
/// also matches. Otherwise the latest matching policy allows or denies.
/// With none, the mode decides; a run an event started asks before any
/// call that is not a read.
pub fn decide_tool_call(
    permissions: &RunPermissions,
    thread_policies: &[ThreadPermissionPolicyDecided],
    request: &PolicyRequest,
    tool: &str,
    read_only_hint: bool,
) -> GateDecision {
    let matching: Vec<&ThreadPermissionPolicyDecided> = permissions
        .policies
        .iter()
        .chain(thread_policies)
        .filter(|policy| {
            policy.request_kind == request.request_kind && policy.resource == request.resource
        })
        .collect();
    if matching
        .iter()
        .any(|policy| policy.decision == ThreadPermissionDecision::Ask)
    {
        return GateDecision::Ask {
            reason: "A policy asks before this action.".into(),
        };
    }
    match matching.last().map(|policy| &policy.decision) {
        Some(ThreadPermissionDecision::Allow) => return GateDecision::Allow,
        Some(ThreadPermissionDecision::Deny) => {
            return GateDecision::Deny {
                reason: "A policy denies this action.".into(),
            }
        }
        _ => {}
    }
    let read_only = read_only_hint || READ_ONLY_TOOLS.contains(&tool);
    let mode = match permissions.mode {
        GateMode::Auto if permissions.event_triggered => GateMode::Ask,
        mode => mode,
    };
    match mode {
        GateMode::Auto => GateDecision::Allow,
        GateMode::Ask if read_only => GateDecision::Allow,
        GateMode::Ask if permissions.event_triggered => GateDecision::Ask {
            reason: "An event started this run, so it asks before it changes anything.".into(),
        },
        GateMode::Ask => GateDecision::Ask {
            reason: "This agent asks before it changes anything.".into(),
        },
        GateMode::Strict => GateDecision::Ask {
            reason: "This agent asks before every action.".into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sidecar::pi_chat::ExtensionUiDialog;

    fn select(id: &str) -> ExtensionUiRequest {
        ExtensionUiRequest {
            id: id.into(),
            dialog: ExtensionUiDialog::Select {
                title: "MCP Input Request".into(),
                options: vec!["Continue".into(), "Decline".into()],
            },
            timeout: None,
        }
    }

    #[test]
    fn a_second_request_waits_for_the_first_gate_and_opens_after_its_answer() {
        let mut pending = None;
        let mut waiting = VecDeque::new();
        let mut journaled = Vec::new();
        for id in ["gate-1", "gate-2"] {
            coordinate_extension_ui_request(
                PiChatEvent::ExtensionUiRequest(select(id)),
                &mut pending,
                &mut waiting,
                |kind, payload| {
                    journaled.push((kind.to_owned(), payload["gate_id"].clone()));
                    Ok(())
                },
            )
            .unwrap();
        }
        assert_eq!(
            pending.as_ref().map(|request| request.id.as_str()),
            Some("gate-1")
        );
        assert_eq!(waiting.len(), 1);
        assert_eq!(
            journaled,
            [("permission.requested".to_owned(), json!("gate-1"))]
        );

        // The card names gate-1, so its answer lands, and gate-2 opens next.
        let mut sent = Vec::new();
        coordinate_permission_answer(
            &mut pending,
            PendingPermissionAnswer {
                gate_id: "gate-1".into(),
                answer: ChatPermissionAnswer::Select("Continue".into()),
                resolved: None,
            },
            |request, _| {
                sent.push(request.id.clone());
                Ok(())
            },
            |_, _| Ok(9),
        )
        .unwrap();
        assert!(pending.is_none());
        assert_eq!(sent, ["gate-1"]);
        open_waiting_request(&mut pending, &mut waiting, |kind, payload| {
            journaled.push((kind.to_owned(), payload["gate_id"].clone()));
            Ok(())
        })
        .unwrap();
        assert_eq!(
            pending.as_ref().map(|request| request.id.as_str()),
            Some("gate-2")
        );
        assert!(waiting.is_empty());
        assert_eq!(
            journaled[1],
            ("permission.requested".to_owned(), json!("gate-2"))
        );

        // With a gate open, the next opener does nothing.
        open_waiting_request(&mut pending, &mut waiting, |_, _| panic!("no append")).unwrap();
        assert_eq!(
            pending.as_ref().map(|request| request.id.as_str()),
            Some("gate-2")
        );
    }

    fn policy(
        decision: ThreadPermissionDecision,
        request: &PolicyRequest,
    ) -> ThreadPermissionPolicyDecided {
        ThreadPermissionPolicyDecided {
            policy_id: uuid::Uuid::now_v7().to_string(),
            decision,
            request_kind: request.request_kind.clone(),
            resource: request.resource.clone(),
            actor: "owner".into(),
            answer_source: crate::journal::ThreadPermissionAnswerSource::Native,
            gate_id: None,
        }
    }

    #[test]
    fn tool_calls_map_to_exact_policy_requests() {
        let directory = std::path::Path::new("/work/project");
        assert_eq!(
            tool_request("bash", &json!({"command": "git push"}), directory),
            PolicyRequest {
                request_kind: ThreadPermissionRequestKind::Command,
                resource: ThreadPermissionResource::Command {
                    arguments: vec!["bash".into(), "git push".into()],
                    working_directory: "/work/project".into(),
                },
            }
        );
        let write = tool_request("write", &json!({"path": "../notes/a.md"}), directory);
        assert_eq!(
            write.resource,
            ThreadPermissionResource::Path {
                path: "/work/notes/a.md".into(),
                operation: "write".into()
            }
        );
        assert!(
            crate::journal::thread_permission_policy::valid_policy_resource(
                &write.request_kind,
                &write.resource
            )
        );
        let mcp = tool_request("mcp__crm__delete", &json!({"id": 7}), directory);
        assert_eq!(mcp.request_kind, ThreadPermissionRequestKind::Tool);
        assert!(
            crate::journal::thread_permission_policy::valid_policy_resource(
                &mcp.request_kind,
                &mcp.resource
            )
        );
    }

    #[test]
    fn modes_policies_and_events_decide_tool_calls() {
        let directory = std::path::Path::new("/work");
        let push = tool_request("bash", &json!({"command": "git push"}), directory);
        let status = tool_request("bash", &json!({"command": "git status"}), directory);
        let read = tool_request("read", &json!({"path": "a.md"}), directory);
        let asks = |decision: GateDecision| matches!(decision, GateDecision::Ask { .. });
        let auto = RunPermissions::default();
        assert!(!auto.gates());
        assert_eq!(
            decide_tool_call(&auto, &[], &push, "bash", false),
            GateDecision::Allow
        );
        let ask = RunPermissions {
            mode: GateMode::Ask,
            policies: vec![policy(ThreadPermissionDecision::Allow, &status)],
            ..RunPermissions::default()
        };
        assert!(asks(decide_tool_call(&ask, &[], &push, "bash", false)));
        assert_eq!(
            decide_tool_call(&ask, &[], &status, "bash", false),
            GateDecision::Allow
        );
        assert_eq!(
            decide_tool_call(&ask, &[], &read, "read", false),
            GateDecision::Allow
        );
        // A thread's own "allow for this thread" answer joins the profile.
        let thread = [policy(ThreadPermissionDecision::Allow, &push)];
        assert_eq!(
            decide_tool_call(&ask, &thread, &push, "bash", false),
            GateDecision::Allow
        );
        let strict = RunPermissions {
            mode: GateMode::Strict,
            ..RunPermissions::default()
        };
        assert!(asks(decide_tool_call(&strict, &[], &read, "read", false)));
        // An event's run asks before writes even in auto mode.
        let event = RunPermissions {
            event_triggered: true,
            ..RunPermissions::default()
        };
        assert!(event.gates());
        assert!(asks(decide_tool_call(&event, &[], &push, "bash", false)));
        assert_eq!(
            decide_tool_call(&event, &[], &read, "read", false),
            GateDecision::Allow
        );
        assert_eq!(
            decide_tool_call(&event, &thread, &push, "bash", false),
            GateDecision::Allow
        );
        let deny = RunPermissions {
            policies: vec![policy(ThreadPermissionDecision::Deny, &push)],
            ..RunPermissions::default()
        };
        assert!(deny.gates());
        assert!(matches!(
            decide_tool_call(&deny, &[], &push, "bash", false),
            GateDecision::Deny { .. }
        ));
    }

    #[test]
    fn an_ask_policy_beats_every_allow_policy() {
        let push = tool_request(
            "bash",
            &json!({"command": "git push"}),
            std::path::Path::new("/work"),
        );
        let permissions = RunPermissions {
            policies: vec![policy(ThreadPermissionDecision::Ask, &push)],
            ..RunPermissions::default()
        };
        assert!(permissions.gates());
        let later_allow = [policy(ThreadPermissionDecision::Allow, &push)];
        assert!(matches!(
            decide_tool_call(&permissions, &later_allow, &push, "bash", false),
            GateDecision::Ask { .. }
        ));
    }
}
