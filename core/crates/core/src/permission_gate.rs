//! Permission gate coordination for a chat run, and the permission modes
//! and rules that decide each tool call.
//!
//! A Pi extension-UI request opens a gate. The gate stays open until an answer
//! arrives for its id and matches its dialog shape. This state is
//! runtime-service state, so the rules live here rather than in the desktop
//! shell.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

pub mod classifier;
pub mod settings;
pub mod shell;
pub mod threads;

pub use classifier::AutoReview;

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

/// A global permission mode. Threads and agents both run in one, and the
/// same rules decide each tool call in every mode: deny rules, then
/// ask-first rules, then protected paths and critical commands, then
/// always-allow rules, then the mode's baseline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    /// Reads and read-only commands run. Everything else asks. An agent
    /// profile's old `ask` reads as this mode.
    #[default]
    #[serde(alias = "ask")]
    Manual,
    /// Also file edits and file commands inside the working folders run.
    EditAuto,
    /// Reads only; the agent writes a plan, and approving it picks the mode
    /// the thread continues in.
    Plan,
    /// A cheap model reviews each action that is not a read.
    Auto,
    /// Only reads and always-allowed actions run; the rest is denied
    /// without asking. An agent profile's old `strict` reads as this mode.
    #[serde(alias = "strict")]
    DontAsk,
    /// Everything runs except what deny and ask-first rules, protected
    /// paths and critical commands stop.
    Bypass,
}

impl PermissionMode {
    /// Every mode, in menu order.
    pub const ALL: [Self; 6] = [
        Self::Manual,
        Self::EditAuto,
        Self::Plan,
        Self::Auto,
        Self::DontAsk,
        Self::Bypass,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::EditAuto => "edit_auto",
            Self::Plan => "plan",
            Self::Auto => "auto",
            Self::DontAsk => "dont_ask",
            Self::Bypass => "bypass",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Manual => "Ask before changes",
            Self::EditAuto => "Edit automatically",
            Self::Plan => "Plan first",
            Self::Auto => "Auto review",
            Self::DontAsk => "Don't ask",
            Self::Bypass => "Bypass permissions",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Manual => "Reads and read-only commands run. Edits, other commands, web and MCP calls ask first.",
            Self::EditAuto => "Also edits files and runs mkdir, touch, mv, cp and rm inside the working folders without asking. Everything else asks.",
            Self::Plan => "Reads and read-only commands only. The agent writes a plan, and approving it switches to the mode you pick.",
            Self::Auto => "A fast model reviews each action and runs it or pauses it for you. After 3 blocks in a row or 20 in a run, it asks instead.",
            Self::DontAsk => "Only reads and always-allowed actions run. Everything else is denied without asking.",
            Self::Bypass => "Everything runs except deny and ask-first rules, protected paths and critical commands. Nothing else asks first.",
        }
    }
}

/// One pre-decided thread policy, without the identity a journaled
/// decision carries: the exact request a policy matches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyRequest {
    pub request_kind: ThreadPermissionRequestKind,
    pub resource: ThreadPermissionResource,
}

/// Checks one rule list: at most `max` exact requests.
pub fn validate_rules(requests: &[PolicyRequest], max: usize) -> Result<(), String> {
    if requests.len() > max {
        return Err(format!("Keep each permission list under {max} entries."));
    }
    if !requests.iter().all(|request| {
        crate::journal::thread_permission_policy::valid_policy_resource(
            &request.request_kind,
            &request.resource,
        )
    }) {
        return Err("A permission entry names an exact absolute path, command or tool.".into());
    }
    Ok(())
}

/// The pre-decided policies of one rule set, as `actor` decided them.
pub fn rule_policies(
    actor: &str,
    deny: &[PolicyRequest],
    ask_first: &[PolicyRequest],
    always_allow: &[PolicyRequest],
) -> Vec<ThreadPermissionPolicyDecided> {
    let with = |requests: &[PolicyRequest], decision: ThreadPermissionDecision| {
        requests
            .iter()
            .map(|request| settings::decided(actor, request, decision.clone()))
            .collect::<Vec<_>>()
    };
    let mut policies = with(always_allow, ThreadPermissionDecision::Allow);
    policies.extend(with(ask_first, ThreadPermissionDecision::Ask));
    policies.extend(with(deny, ThreadPermissionDecision::Deny));
    policies
}

/// The gate one run applies: the mode it launched in, whether an outside
/// event started it, the global and agent rules, and the folders it works
/// in. The thread's own journaled policies and mode join these at each call.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunPermissions {
    pub mode: PermissionMode,
    pub event_triggered: bool,
    /// Global rules first, then the agent's.
    pub policies: Vec<ThreadPermissionPolicyDecided>,
    /// The folders `edit_auto` edits without asking.
    pub working_directories: Vec<PathBuf>,
    /// Folders no action reads or writes without a person's answer, such as
    /// the runtime's state folder, apart from the working folders inside
    /// them.
    pub protected_roots: Vec<PathBuf>,
    /// The user's home, for `~` in commands and the critical list.
    pub home: Option<PathBuf>,
    pub bypass_enabled: bool,
}

impl RunPermissions {
    /// The mode a call runs in: the thread's journaled mode when it has one,
    /// else the launch mode. `bypass` while it is off runs as `manual`.
    pub fn mode_for(&self, thread_mode: Option<PermissionMode>) -> PermissionMode {
        match thread_mode.unwrap_or(self.mode) {
            PermissionMode::Bypass if !self.bypass_enabled => PermissionMode::Manual,
            mode => mode,
        }
    }
}

/// The gate's answer for one tool call.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum GateDecision {
    Allow,
    /// The call is refused without asking.
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

/// Tools that reach the web. They read, but every mode short of `auto` and
/// `bypass` asks before them.
const WEB_TOOLS: &[&str] = &["web_search", "fetch_content", "get_search_content"];

/// Muniment's own tools that keep the user's memory and the run's plan.
/// They run in every mode.
const KEEPING_TOOLS: &[&str] = &[
    "memory-save",
    "memory-delete",
    "memory-profile-save",
    "creation-plan",
    PLAN_TOOL,
];

/// The tool a `plan` run submits its plan with.
pub const PLAN_TOOL: &str = "plan_submit";

/// Whether the tool named `tool` reads and changes nothing.
pub fn is_read_only_tool(tool: &str) -> bool {
    READ_ONLY_TOOLS.contains(&tool)
}

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

/// What one call does, for the mode baselines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionClass {
    /// Reads, read-only commands and Muniment's own keeping tools.
    Read,
    /// File edits and writes, and commands that only create, move, copy or
    /// remove files. `inside` when every written path is inside a working
    /// folder, in both its given and its symlink-resolved form.
    Edit {
        inside: bool,
    },
    /// Any other command or tool.
    Command,
    Web,
    Mcp {
        read_only: bool,
    },
}

/// One call's class, and whether it touches a protected path or is a
/// critical command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Assessment {
    pub class: ActionClass,
    pub protected: bool,
    pub critical: bool,
}

/// File names that configure MCP servers.
const MCP_CONFIG_FILES: [&str; 4] = [
    ".mcp.json",
    "mcp.json",
    "mcp_config.json",
    "claude_desktop_config.json",
];
/// Shell startup files: a write there runs on every new shell.
const SHELL_RC_FILES: [&str; 16] = [
    ".bashrc",
    ".bash_profile",
    ".bash_login",
    ".bash_logout",
    ".profile",
    ".zshrc",
    ".zprofile",
    ".zshenv",
    ".zlogin",
    ".zlogout",
    ".cshrc",
    ".tcshrc",
    ".kshrc",
    "config.fish",
    "Microsoft.PowerShell_profile.ps1",
    "profile.ps1",
];
/// Folders inside a protected root that runs read without asking: the
/// installed extensions and the harness's skills.
const READABLE_IN_PROTECTED_ROOTS: [&str; 2] = ["extensions", "agent/skills"];
/// Folders whose every file is protected.
const PROTECTED_FOLDERS: [&str; 2] = [".git", ".muniment"];

/// A path as given and as the file system resolves it, symlinks followed.
/// A path that does not exist yet resolves through its nearest existing
/// parent.
fn path_forms(path: &Path) -> [PathBuf; 2] {
    let mut existing = path.to_path_buf();
    let mut rest = Vec::new();
    let resolved = loop {
        if let Ok(resolved) = std::fs::canonicalize(&existing) {
            break rest
                .iter()
                .rev()
                .fold(resolved, |path: PathBuf, part| path.join(part));
        }
        match (existing.file_name(), existing.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name.to_owned());
                existing = parent.to_path_buf();
            }
            _ => break path.to_path_buf(),
        }
    };
    [path.to_path_buf(), resolved]
}

impl RunPermissions {
    /// Whether `path` is strictly inside a working folder, in both forms.
    fn inside_workspace(&self, path: &Path) -> bool {
        let [given, resolved] = path_forms(path);
        self.working_directories.iter().any(|folder| {
            let [folder_given, folder_resolved] = path_forms(folder);
            given.starts_with(&folder_given)
                && given != folder_given
                && resolved.starts_with(&folder_resolved)
                && resolved != folder_resolved
        })
    }

    /// Whether `path`, in either form, sits in a protected root outside
    /// every working folder.
    fn in_protected_root(&self, path: &Path) -> bool {
        let forms = path_forms(path);
        let in_workspace = |form: &PathBuf| {
            self.working_directories
                .iter()
                .flat_map(|folder| path_forms(folder))
                .any(|folder| form.starts_with(folder))
        };
        self.protected_roots.iter().any(|root| {
            path_forms(root).iter().any(|root| {
                forms
                    .iter()
                    .any(|form| form.starts_with(root) && !in_workspace(form))
            })
        })
    }

    /// Whether reading `path` needs a person's answer: a file in a
    /// protected root, apart from the installed skills and plugins the
    /// harness tells the model to read.
    pub fn protected_read(&self, path: &Path) -> bool {
        let forms = path_forms(path);
        self.in_protected_root(path)
            && !self.protected_roots.iter().any(|root| {
                path_forms(root).iter().any(|root| {
                    READABLE_IN_PROTECTED_ROOTS
                        .iter()
                        .any(|folder| forms.iter().any(|form| form.starts_with(root.join(folder))))
                })
            })
    }

    /// Whether writing `path` needs a person's answer: a file in `.git` or
    /// `.muniment`, an MCP configuration, a shell startup file, or a
    /// protected root.
    pub fn protected_write(&self, path: &Path) -> bool {
        path_forms(path).iter().any(|form| {
            form.components().any(|component| {
                PROTECTED_FOLDERS
                    .iter()
                    .any(|folder| component.as_os_str() == *folder)
            }) || form.file_name().is_some_and(|name| {
                MCP_CONFIG_FILES
                    .iter()
                    .chain(SHELL_RC_FILES.iter())
                    .any(|file| name == *file)
            })
        }) || self.in_protected_root(path)
    }

    /// Classes one call.
    pub fn assess(&self, request: &PolicyRequest, tool: &str, read_only_hint: bool) -> Assessment {
        let plain = |class| Assessment {
            class,
            protected: false,
            critical: false,
        };
        if WEB_TOOLS.contains(&tool) {
            return plain(ActionClass::Web);
        }
        match &request.resource {
            ThreadPermissionResource::Path { path, operation } => {
                let path = Path::new(path);
                match operation.as_str() {
                    "read" | "ls" | "find" | "grep" => Assessment {
                        class: ActionClass::Read,
                        protected: self.protected_read(path),
                        critical: false,
                    },
                    "write" | "edit" => Assessment {
                        class: ActionClass::Edit {
                            inside: self.inside_workspace(path),
                        },
                        protected: self.protected_write(path),
                        critical: false,
                    },
                    _ => plain(ActionClass::Command),
                }
            }
            ThreadPermissionResource::Command {
                arguments,
                working_directory,
            } => {
                let command = arguments.get(1).map(String::as_str).unwrap_or_default();
                let critical = shell::is_critical(command, self.home.as_deref());
                if arguments.first().map(String::as_str) == Some("powershell") {
                    return Assessment {
                        class: ActionClass::Command,
                        protected: false,
                        critical,
                    };
                }
                let reading = shell::read_command(
                    command,
                    Path::new(working_directory),
                    self.home.as_deref(),
                );
                let reads_protected = reading.reads.iter().any(|path| self.protected_read(path));
                let writes_protected = reading
                    .writes
                    .iter()
                    .chain(&reading.unknown)
                    .any(|path| self.protected_write(path));
                let class = match reading.kind {
                    shell::ShellKind::ReadOnly => ActionClass::Read,
                    shell::ShellKind::FileOps => ActionClass::Edit {
                        inside: reading
                            .writes
                            .iter()
                            .all(|path| self.inside_workspace(path)),
                    },
                    shell::ShellKind::Other(_) => ActionClass::Command,
                };
                Assessment {
                    class,
                    protected: reads_protected || writes_protected,
                    critical,
                }
            }
            ThreadPermissionResource::Tool { name, .. } => {
                let class = if name == "mcp__record__sql"
                    || KEEPING_TOOLS.contains(&name.as_str())
                    || (READ_ONLY_TOOLS.contains(&name.as_str()))
                {
                    ActionClass::Read
                } else if name.starts_with("mcp__") {
                    ActionClass::Mcp {
                        read_only: read_only_hint,
                    }
                } else if read_only_hint {
                    ActionClass::Read
                } else {
                    ActionClass::Command
                };
                plain(class)
            }
        }
    }
}

/// The agent gate's answers, as the gate offers them and as
/// `agent.approval_answer` names them.
pub const GATE_ANSWERS: [(&str, &str); 5] = [
    ("allow_once", "Allow once"),
    ("allow_thread", "Allow for this thread"),
    ("always_allow_agent", "Always allow for this agent"),
    ("always_allow", "Always allow"),
    ("deny", "Deny"),
];

/// The plan gate's answers: each approval and the mode the thread then
/// runs in, and keep planning.
pub const PLAN_ANSWERS: [(&str, &str, Option<PermissionMode>); 4] = [
    (
        "plan_manual",
        "Approve, then ask before changes",
        Some(PermissionMode::Manual),
    ),
    (
        "plan_edit_auto",
        "Approve and edit automatically",
        Some(PermissionMode::EditAuto),
    ),
    (
        "plan_auto",
        "Approve with auto review",
        Some(PermissionMode::Auto),
    ),
    ("plan_keep", "Keep planning", None),
];

/// The title of the gate a submitted plan opens.
pub const PLAN_GATE_TITLE: &str = "Approve this plan?";

/// The plan gate's options.
pub fn plan_options() -> Vec<String> {
    PLAN_ANSWERS
        .iter()
        .map(|(_, option, _)| (*option).to_owned())
        .collect()
}

/// The mode a plan gate option approves, or `None` for keep planning and
/// any other text.
pub fn plan_choice(option: &str) -> Option<PermissionMode> {
    PLAN_ANSWERS
        .iter()
        .find(|(_, label, _)| *label == option)
        .and_then(|(_, _, mode)| *mode)
}

/// The gate option an answer id names, such as `Allow once` for `allow_once`.
pub fn gate_option(answer: &str) -> Option<&'static str> {
    GATE_ANSWERS
        .iter()
        .find(|(id, _)| *id == answer)
        .map(|(_, option)| *option)
        .or_else(|| {
            PLAN_ANSWERS
                .iter()
                .find(|(id, _, _)| *id == answer)
                .map(|(_, option, _)| *option)
        })
}

/// The answer id of a gate option, such as `allow_once` for `Allow once`.
pub fn gate_answer(option: &str) -> Option<&'static str> {
    GATE_ANSWERS
        .iter()
        .find(|(_, label)| *label == option)
        .map(|(id, _)| *id)
        .or_else(|| {
            PLAN_ANSWERS
                .iter()
                .find(|(_, label, _)| *label == option)
                .map(|(id, _, _)| *id)
        })
}

/// The options the gate offers for one call. "Allow for this thread" is
/// left out when no rule can allow the call, such as a protected path. A
/// run of an agent offers "Always allow for this agent" and any other run
/// "Always allow", which saves a global rule; both are left out also when an
/// ask policy covers the call, since ask-first outranks every allow.
pub fn gate_options(thread_rule: bool, always_rule: bool, agent: bool) -> Vec<String> {
    GATE_ANSWERS
        .iter()
        .filter(|(id, _)| match *id {
            "allow_thread" => thread_rule,
            "always_allow_agent" => thread_rule && always_rule && agent,
            "always_allow" => thread_rule && always_rule && !agent,
            _ => true,
        })
        .map(|(_, option)| (*option).to_owned())
        .collect()
}

/// Whether an ask policy, a global, agent or thread one, matches the
/// request exactly.
pub fn asks_first(
    permissions: &RunPermissions,
    thread_policies: &[ThreadPermissionPolicyDecided],
    request: &PolicyRequest,
) -> bool {
    permissions
        .policies
        .iter()
        .chain(thread_policies)
        .any(|policy| {
            policy.decision == ThreadPermissionDecision::Ask
                && policy.request_kind == request.request_kind
                && policy.resource == request.resource
        })
}

/// The most characters of a call summary.
const MAX_SUMMARY_CHARS: usize = 240;

/// The exact operation and inputs of one call on one line, for the auto
/// reviewer: the command and its folder, the file operation and its path,
/// or the tool and its exact input.
pub fn exact_summary(request: &PolicyRequest) -> String {
    let text = match &request.resource {
        ThreadPermissionResource::Command {
            arguments,
            working_directory,
        } => {
            let (program, rest) = arguments
                .split_first()
                .map_or(("", &[][..]), |(first, rest)| (first.as_str(), rest));
            format!("{program} `{}` in {working_directory}", rest.join(" "))
        }
        ThreadPermissionResource::Path { path, operation } => format!("{operation} {path}"),
        ThreadPermissionResource::Tool { name, input } => {
            let input = input.to_string();
            if input == "{}" || input == "null" {
                name.clone()
            } else {
                format!("{name} {input}")
            }
        }
    };
    if text.chars().count() <= MAX_SUMMARY_CHARS {
        return text;
    }
    let mut short: String = text.chars().take(MAX_SUMMARY_CHARS - 1).collect();
    short.push('…');
    short
}

/// What one call does, as a person reads it: the headline of its
/// presentation, such as "Run `git push` in work".
pub fn summary(request: &PolicyRequest) -> String {
    crate::tool_presentation::describe_request(request).headline
}

/// What a `plan` run is told when it tries a change.
pub const PLAN_REFUSAL: &str =
    "Plan mode refuses changes while planning. Read what you need, then submit your plan with plan_submit.";

/// Decides one tool call in `mode`: deny rules (global, agent, thread),
/// then ask-first rules, then protected paths and critical commands, then
/// always-allow rules, then the mode's baseline. Rules match the request
/// exactly. `review` answers for `auto`.
pub fn decide_tool_call(
    permissions: &RunPermissions,
    thread_policies: &[ThreadPermissionPolicyDecided],
    mode: PermissionMode,
    request: &PolicyRequest,
    tool: &str,
    read_only_hint: bool,
    review: &mut AutoReview,
) -> GateDecision {
    let mode = permissions.mode_for(Some(mode));
    let matching = |decision: ThreadPermissionDecision| {
        permissions
            .policies
            .iter()
            .chain(thread_policies)
            .any(|policy| {
                policy.decision == decision
                    && policy.request_kind == request.request_kind
                    && policy.resource == request.resource
            })
    };
    let ask = |reason: &str| GateDecision::Ask {
        reason: reason.into(),
    };
    let deny = |reason: &str| GateDecision::Deny {
        reason: reason.into(),
    };
    if matching(ThreadPermissionDecision::Deny) {
        return deny("A deny rule refuses this action.");
    }
    if matching(ThreadPermissionDecision::Ask) {
        return if mode == PermissionMode::DontAsk {
            deny("An ask-first rule covers this action, and Don't ask mode denies instead of asking.")
        } else {
            ask("An ask-first rule covers this action.")
        };
    }
    let assessment = permissions.assess(request, tool, read_only_hint);
    let changes = matches!(
        assessment.class,
        ActionClass::Edit { .. } | ActionClass::Command | ActionClass::Mcp { read_only: false }
    );
    if assessment.critical {
        return match mode {
            PermissionMode::DontAsk => deny("Don't ask mode denies critical commands."),
            PermissionMode::Plan => deny(PLAN_REFUSAL),
            _ => ask("This is a critical command, so it always asks."),
        };
    }
    if assessment.protected {
        return match mode {
            PermissionMode::Bypass => GateDecision::Allow,
            PermissionMode::DontAsk => deny("Don't ask mode denies actions on protected paths."),
            PermissionMode::Plan if changes => deny(PLAN_REFUSAL),
            _ => ask("This action touches a protected path, so it always asks."),
        };
    }
    if mode == PermissionMode::Plan && changes {
        return deny(PLAN_REFUSAL);
    }
    if matching(ThreadPermissionDecision::Allow) {
        return GateDecision::Allow;
    }
    match (mode, assessment.class) {
        (_, ActionClass::Read) | (PermissionMode::Bypass, _) => GateDecision::Allow,
        (PermissionMode::EditAuto, ActionClass::Edit { inside: true }) => GateDecision::Allow,
        (PermissionMode::DontAsk, _) => deny("Don't ask mode denies actions that no rule allows."),
        (PermissionMode::Auto, _) => match review.review(tool, &exact_summary(request)) {
            classifier::Review::Allow => GateDecision::Allow,
            classifier::Review::Pause(reason) => GateDecision::Ask { reason },
        },
        (PermissionMode::EditAuto, ActionClass::Edit { inside: false }) => {
            ask("This edit is outside the working folders, so it asks.")
        }
        // One reason for every call the mode itself stops, named as the
        // mode menu names the mode.
        (
            _,
            ActionClass::Edit { .. }
            | ActionClass::Web
            | ActionClass::Mcp { .. }
            | ActionClass::Command,
        ) => ask(&format!("Needs your OK in {} mode.", mode.title())),
    }
}

/// Journals `mode` as the thread's permission mode. `source` is `user` or
/// `plan`.
pub fn set_thread_mode(
    journal: &mut crate::journal::RunJournal,
    thread_id: &str,
    mode: PermissionMode,
    source: &str,
    actor: &str,
    provenance: &crate::journal::Provenance,
) -> Result<(), String> {
    let last = journal
        .last_thread_seq(thread_id)
        .map_err(|error| error.to_string())?;
    journal
        .append_thread_permission_mode_set(
            last,
            thread_id,
            &crate::journal::ThreadPermissionModeSet {
                mode,
                actor: actor.to_owned(),
                source: source.to_owned(),
            },
            &crate::run_start::accepted_time_now(),
            provenance,
        )
        .map_err(|error| error.to_string())
}

/// Whether a thread or agent rule could allow the call: protected paths
/// and critical commands never take an allow rule.
pub fn rules_can_allow(permissions: &RunPermissions, request: &PolicyRequest, tool: &str) -> bool {
    let assessment = permissions.assess(request, tool, false);
    !assessment.protected && !assessment.critical
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
        settings::decided("owner", request, decision)
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

    /// A working folder with a `.git` folder, a link that leaves it, and a
    /// state folder outside it.
    struct Folders {
        root: PathBuf,
        work: PathBuf,
        state: PathBuf,
    }

    impl Folders {
        fn new() -> Self {
            let root = std::env::temp_dir().join(uuid::Uuid::now_v7().to_string());
            let work = root.join("work");
            let state = root.join("state");
            std::fs::create_dir_all(work.join(".git")).unwrap();
            std::fs::create_dir_all(work.join("src")).unwrap();
            std::fs::create_dir_all(&state).unwrap();
            std::fs::write(state.join("token"), "secret").unwrap();
            #[cfg(unix)]
            std::os::unix::fs::symlink(&state, work.join("escape")).unwrap();
            Self {
                root: std::fs::canonicalize(&root).unwrap(),
                work: std::fs::canonicalize(&work).unwrap(),
                state: std::fs::canonicalize(&state).unwrap(),
            }
        }

        fn permissions(&self, mode: PermissionMode) -> RunPermissions {
            RunPermissions {
                mode,
                working_directories: vec![self.work.clone()],
                protected_roots: vec![self.state.clone()],
                home: Some(self.root.clone()),
                bypass_enabled: true,
                ..RunPermissions::default()
            }
        }

        fn call(&self, tool: &str, input: Value) -> (PolicyRequest, String) {
            (tool_request(tool, &input, &self.work), tool.to_owned())
        }
    }

    impl Drop for Folders {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// The decision letter: `A`llow, `D`eny or `Q` (ask).
    fn letter(decision: &GateDecision) -> char {
        match decision {
            GateDecision::Allow => 'A',
            GateDecision::Deny { .. } => 'D',
            GateDecision::Ask { .. } => 'Q',
        }
    }

    fn allow_review() -> AutoReview {
        let model: crate::memory_extract::Model = std::sync::Arc::new(|_, _| {
            Ok(crate::memory_extract::Answer {
                text: r#"{"verdict":"allow","reason":"The request asks for it."}"#.into(),
                via: "router",
            })
        });
        AutoReview::new(Some(model), "Tidy the project.", false)
    }

    #[test]
    fn every_mode_decides_every_action_class() {
        let folders = Folders::new();
        let work = folders.work.to_string_lossy().into_owned();
        let calls = [
            ("read", folders.call("read", json!({"path": "src/main.rs"}))),
            ("edit", folders.call("edit", json!({"path": "src/main.rs"}))),
            (
                "edit outside",
                folders.call("write", json!({"path": "../elsewhere.md"})),
            ),
            (
                "shell read",
                folders.call("bash", json!({"command": "git status && rg todo src"})),
            ),
            (
                "shell file op",
                folders.call(
                    "bash",
                    json!({"command": "mkdir -p src/new && touch src/new/a.rs"}),
                ),
            ),
            (
                "shell mutating",
                folders.call("bash", json!({"command": "npm install left-pad"})),
            ),
            ("web", folders.call("web_search", json!({"query": "rust"}))),
            ("mcp", folders.call("mcp__crm__delete", json!({"id": 7}))),
            (
                "protected",
                folders.call("write", json!({"path": ".git/config"})),
            ),
            (
                "critical",
                folders.call("bash", json!({"command": "rm -rf ~"})),
            ),
            ("keeping", folders.call("memory-save", json!({"text": "x"}))),
        ];
        // Columns: manual, edit_auto, plan, auto, dont_ask, bypass.
        let expected = [
            ("read", "AAAAAA"),
            ("edit", "QADADA"),
            ("edit outside", "QQDADA"),
            ("shell read", "AAAAAA"),
            ("shell file op", "QADADA"),
            ("shell mutating", "QQDADA"),
            ("web", "QQQADA"),
            ("mcp", "QQDADA"),
            ("protected", "QQDQDA"),
            ("critical", "QQDQDQ"),
            ("keeping", "AAAAAA"),
        ];
        for ((name, (request, tool)), (expected_name, letters)) in calls.iter().zip(expected) {
            assert_eq!(*name, expected_name);
            let decided: String = PermissionMode::ALL
                .iter()
                .map(|mode| {
                    letter(&decide_tool_call(
                        &folders.permissions(*mode),
                        &[],
                        *mode,
                        request,
                        tool,
                        false,
                        &mut allow_review(),
                    ))
                })
                .collect();
            assert_eq!(decided, letters, "{name} in {work}");
        }
    }

    #[test]
    fn paths_are_checked_by_direction_and_through_symlinks() {
        let folders = Folders::new();
        let permissions = folders.permissions(PermissionMode::EditAuto);
        let decide = |tool: &str, input: Value| {
            let (request, tool) = folders.call(tool, input);
            decide_tool_call(
                &permissions,
                &[],
                PermissionMode::EditAuto,
                &request,
                &tool,
                false,
                &mut allow_review(),
            )
        };
        // Reading .git is fine; writing it asks.
        assert_eq!(
            decide("read", json!({"path": ".git/HEAD"})),
            GateDecision::Allow
        );
        assert!(matches!(
            decide("edit", json!({"path": ".git/HEAD"})),
            GateDecision::Ask { .. }
        ));
        // The state folder is protected both ways.
        let token = folders.state.join("token").to_string_lossy().into_owned();
        assert!(matches!(
            decide("read", json!({"path": token})),
            GateDecision::Ask { .. }
        ));
        assert!(matches!(
            decide("bash", json!({"command": format!("cat {token}")})),
            GateDecision::Ask { .. }
        ));
        #[cfg(unix)]
        {
            // A link inside the working folder that resolves into the state
            // folder is protected and outside the working folder.
            assert!(matches!(
                decide("read", json!({"path": "escape/token"})),
                GateDecision::Ask { .. }
            ));
            assert!(matches!(
                decide("write", json!({"path": "escape/new.txt"})),
                GateDecision::Ask { .. }
            ));
        }
        // Shell startup files and MCP configuration are protected writes.
        for path in ["~/.zshrc", ".mcp.json"] {
            assert!(matches!(
                decide("bash", json!({"command": format!("echo x >> {path}")})),
                GateDecision::Ask { .. }
            ));
        }
        assert_eq!(
            decide("bash", json!({"command": "echo x >> notes.md"})),
            GateDecision::Allow
        );
        // A protected path never takes an allow rule.
        let (request, tool) = folders.call("write", json!({"path": ".git/config"}));
        assert!(!rules_can_allow(&permissions, &request, &tool));
        let allowed = [policy(ThreadPermissionDecision::Allow, &request)];
        assert!(matches!(
            decide_tool_call(
                &permissions,
                &allowed,
                PermissionMode::Manual,
                &request,
                &tool,
                false,
                &mut allow_review()
            ),
            GateDecision::Ask { .. }
        ));
    }

    #[test]
    fn rules_apply_in_order_deny_then_ask_then_allow_then_the_mode() {
        let folders = Folders::new();
        let (push, bash) = folders.call("bash", json!({"command": "git push"}));
        let decide = |permissions: &RunPermissions, thread: &[ThreadPermissionPolicyDecided]| {
            decide_tool_call(
                permissions,
                thread,
                permissions.mode,
                &push,
                &bash,
                false,
                &mut AutoReview::new(None, "", false),
            )
        };
        let mut permissions = folders.permissions(PermissionMode::Manual);
        assert!(matches!(
            decide(&permissions, &[]),
            GateDecision::Ask { .. }
        ));
        // A thread allow runs it.
        let thread_allow = [policy(ThreadPermissionDecision::Allow, &push)];
        assert_eq!(decide(&permissions, &thread_allow), GateDecision::Allow);
        // An agent ask-first outranks the thread allow.
        permissions.policies = rule_policies("agent:a", &[], std::slice::from_ref(&push), &[]);
        assert!(matches!(
            decide(&permissions, &thread_allow),
            GateDecision::Ask { .. }
        ));
        // A global deny outranks everything, bypass included.
        let mut global = settings::PermissionSettings {
            deny: vec![push.clone()],
            ..Default::default()
        }
        .policies();
        global.extend(permissions.policies.clone());
        permissions.policies = global;
        permissions.mode = PermissionMode::Bypass;
        assert!(matches!(
            decide(&permissions, &thread_allow),
            GateDecision::Deny { .. }
        ));
        // Don't ask turns an ask-first rule into a denial.
        let ask_only = RunPermissions {
            mode: PermissionMode::DontAsk,
            policies: rule_policies("global", &[], std::slice::from_ref(&push), &[]),
            ..folders.permissions(PermissionMode::DontAsk)
        };
        assert!(matches!(decide(&ask_only, &[]), GateDecision::Deny { .. }));
        // Don't ask runs what an always-allow rule names.
        let allowed = RunPermissions {
            policies: rule_policies("agent:a", &[], &[], std::slice::from_ref(&push)),
            ..folders.permissions(PermissionMode::DontAsk)
        };
        assert_eq!(decide(&allowed, &[]), GateDecision::Allow);
        // Plan refuses a change even when a rule allows it.
        let planning = RunPermissions {
            policies: rule_policies("agent:a", &[], &[], std::slice::from_ref(&push)),
            ..folders.permissions(PermissionMode::Plan)
        };
        assert!(matches!(decide(&planning, &[]), GateDecision::Deny { .. }));
    }

    #[test]
    fn bypass_while_off_runs_as_manual_and_a_thread_mode_wins() {
        let folders = Folders::new();
        let (push, bash) = folders.call("bash", json!({"command": "git push"}));
        let permissions = RunPermissions {
            bypass_enabled: false,
            ..folders.permissions(PermissionMode::Bypass)
        };
        assert_eq!(permissions.mode_for(None), PermissionMode::Manual);
        assert!(matches!(
            decide_tool_call(
                &permissions,
                &[],
                PermissionMode::Bypass,
                &push,
                &bash,
                false,
                &mut AutoReview::new(None, "", false)
            ),
            GateDecision::Ask { .. }
        ));
        let manual = folders.permissions(PermissionMode::Manual);
        assert_eq!(
            manual.mode_for(Some(PermissionMode::EditAuto)),
            PermissionMode::EditAuto
        );
    }

    #[test]
    fn auto_mode_asks_when_the_reviewer_fails() {
        let folders = Folders::new();
        let (push, bash) = folders.call("bash", json!({"command": "git push"}));
        let decided = decide_tool_call(
            &folders.permissions(PermissionMode::Auto),
            &[],
            PermissionMode::Auto,
            &push,
            &bash,
            false,
            &mut AutoReview::new(None, "Ship it.", false),
        );
        assert!(matches!(decided, GateDecision::Ask { .. }));
    }

    #[test]
    fn modes_migrate_from_the_old_agent_profile_ids() {
        for (old, new) in [
            ("ask", PermissionMode::Manual),
            ("auto", PermissionMode::Auto),
            ("strict", PermissionMode::DontAsk),
        ] {
            assert_eq!(
                serde_json::from_value::<PermissionMode>(json!(old)).unwrap(),
                new
            );
        }
        for mode in PermissionMode::ALL {
            assert_eq!(serde_json::to_value(mode).unwrap(), json!(mode.id()));
        }
    }

    #[test]
    fn the_gate_names_each_call_and_offers_only_the_rules_that_can_apply() {
        let directory = std::path::Path::new("/work");
        let push = tool_request("bash", &json!({"command": "git push"}), directory);
        assert_eq!(summary(&push), "Run `git push` in work");
        assert_eq!(exact_summary(&push), "bash `git push` in /work");
        let write = tool_request("write", &json!({"path": "a.md"}), directory);
        assert_eq!(summary(&write), "Write a.md");
        assert_eq!(exact_summary(&write), "write /work/a.md");
        let mcp = tool_request("mcp__crm__delete", &json!({"id": 7}), directory);
        assert_eq!(summary(&mcp), "Ask CRM: delete 7");
        assert_eq!(exact_summary(&mcp), "mcp__crm__delete {\"id\":7}");
        let long = tool_request(
            "mcp__crm__note",
            &json!({"text": "x".repeat(400)}),
            directory,
        );
        assert!(summary(&long).chars().count() < 80);
        assert!(!summary(&long).contains('{'));
        assert_eq!(exact_summary(&long).chars().count(), 240);
        assert!(exact_summary(&long).ends_with('…'));
        assert_eq!(
            gate_options(true, true, true),
            [
                "Allow once",
                "Allow for this thread",
                "Always allow for this agent",
                "Deny"
            ]
        );
        assert_eq!(
            gate_options(true, true, false),
            [
                "Allow once",
                "Allow for this thread",
                "Always allow",
                "Deny"
            ]
        );
        assert_eq!(
            gate_options(true, false, true),
            ["Allow once", "Allow for this thread", "Deny"]
        );
        assert_eq!(gate_options(false, true, false), ["Allow once", "Deny"]);
        for (id, option) in GATE_ANSWERS {
            assert_eq!(gate_option(id), Some(option));
            assert_eq!(gate_answer(option), Some(id));
        }
        for (id, option, mode) in PLAN_ANSWERS {
            assert_eq!(gate_option(id), Some(option));
            assert_eq!(gate_answer(option), Some(id));
            assert_eq!(plan_choice(option), mode);
        }
        assert_eq!(gate_option("allow_forever"), None);
        let ask_first = RunPermissions {
            policies: vec![policy(ThreadPermissionDecision::Ask, &push)],
            ..RunPermissions::default()
        };
        assert!(asks_first(&ask_first, &[], &push));
        assert!(!asks_first(&ask_first, &[], &write));
        let thread_ask = [policy(ThreadPermissionDecision::Ask, &write)];
        assert!(asks_first(&RunPermissions::default(), &thread_ask, &write));
    }
}
