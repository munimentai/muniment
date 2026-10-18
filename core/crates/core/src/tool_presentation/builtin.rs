//! Describers for the tools the runtime ships: the harness's file, shell
//! and script tools, the web, background task and child agent packages,
//! and Muniment's own memory, agent, artifact, record and approval tools.

use serde_json::Value;

use super::result::took;
use super::text::{
    clip, code, count, file_name, first_line, host, listing, phrase, quoted, short_url, title,
};
use super::{ToolDetail, ToolPresentation};

fn text<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

fn first_text<'a>(input: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| text(input, key))
}

fn strings(input: &Value, key: &str) -> Vec<String> {
    input
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
        .collect()
}

fn number(input: &Value, key: &str) -> Option<f64> {
    input.get(key).and_then(Value::as_f64)
}

struct Call {
    category: &'static str,
    kind: &'static str,
    icon: &'static str,
    headline: String,
    progress: String,
    details: Vec<ToolDetail>,
    path: Option<String>,
}

impl Call {
    fn new(
        category: &'static str,
        kind: &'static str,
        icon: &'static str,
        headline: String,
        progress: String,
    ) -> Self {
        Self {
            category,
            kind,
            icon,
            headline,
            progress,
            details: Vec::new(),
            path: None,
        }
    }

    fn detail(mut self, detail: Option<ToolDetail>) -> Self {
        self.details.extend(detail);
        self
    }

    fn path(mut self, path: Option<&str>) -> Self {
        self.path = path.map(str::to_owned);
        self
    }
}

/// The folder a call names, by its last part, or `the working folder`.
fn folder(path: Option<&str>) -> String {
    match path.map(str::trim) {
        None | Some("" | "." | "./") => "the working folder".into(),
        Some(path) => file_name(path),
    }
}

fn path_detail(path: Option<&str>) -> Option<ToolDetail> {
    path.map(|path| ToolDetail::mono("Path", path))
}

/// A search query as the headline quotes it: `site:` operators and quotes
/// removed, so `site:x.com "AI" "desktop"` reads `AI desktop`.
fn plain_query(query: &str) -> String {
    query
        .split_whitespace()
        .filter(|word| !word.starts_with("site:") && !word.starts_with("-site:"))
        .map(|word| word.trim_matches('"'))
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn sites(input: &Value, queries: &[String]) -> Vec<String> {
    let mut sites = strings(input, "domainFilter");
    for query in queries {
        for word in query.split_whitespace() {
            if let Some(site) = word.strip_prefix("site:") {
                sites.push(site.trim_matches('"').to_owned());
            }
        }
    }
    let mut unique: Vec<String> = Vec::new();
    for site in sites
        .into_iter()
        .map(|site| host(site.trim_start_matches('-')))
    {
        if !site.is_empty() && !unique.contains(&site) {
            unique.push(site);
        }
    }
    unique
}

fn web_search(input: &Value) -> Call {
    let mut queries = strings(input, "queries");
    if queries.is_empty() {
        queries.extend(text(input, "query").map(str::to_owned));
    }
    let sites = sites(input, &queries);
    let subject = match queries.as_slice() {
        [] => String::new(),
        [one] => format!(" for {}", quoted(&plain_query(one), 60)),
        [one, rest @ ..] => format!(
            " for {} and {} more",
            quoted(&plain_query(one), 48),
            rest.len()
        ),
    };
    let place = match sites.as_slice() {
        [] => String::new(),
        [one] => format!(" on {one}"),
        many => format!(" on {} sites", many.len()),
    };
    let recency = text(input, "recencyFilter").map(|recency| match recency {
        "day" => "Past day".to_owned(),
        "week" => "Past week".to_owned(),
        "month" => "Past month".to_owned(),
        "year" => "Past year".to_owned(),
        other => phrase(other),
    });
    let mut call = Call::new(
        "web",
        "web",
        "globe",
        format!("Search the web{subject}{place}"),
        "Searching the web".into(),
    )
    .detail((queries.len() > 1).then(|| {
        ToolDetail::new(
            "Queries",
            queries
                .iter()
                .take(8)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }))
    .detail((sites.len() > 1).then(|| ToolDetail::mono("Sites", listing(&sites))))
    .detail(
        number(input, "numResults").map(|n| ToolDetail::new("Results per query", n.to_string())),
    )
    .detail(recency.map(|recency| ToolDetail::new("Recency", recency)))
    .detail(text(input, "category").map(|category| ToolDetail::new("Category", phrase(category))));
    if input.get("includeContent") == Some(&Value::Bool(true)) {
        call = call.detail(Some(ToolDetail::new("Page content", "Included")));
    }
    call.detail(
        text(input, "provider")
            .filter(|p| *p != "auto")
            .map(|provider| ToolDetail::new("Provider", title(provider))),
    )
}

fn fetch_content(input: &Value) -> Call {
    let mut urls = strings(input, "urls");
    if urls.is_empty() {
        urls.extend(text(input, "url").map(str::to_owned));
    }
    let headline = match urls.as_slice() {
        [] => "Fetch a page".to_owned(),
        [one] => format!("Fetch {}", short_url(one, 60)),
        many => format!("Fetch {} pages", many.len()),
    };
    Call::new("web", "web", "link", headline, "Fetching a page".into())
        .detail((urls.len() > 1).then(|| {
            ToolDetail::mono(
                "Pages",
                urls.iter().take(8).cloned().collect::<Vec<_>>().join("\n"),
            )
        }))
        .detail(text(input, "prompt").map(|prompt| ToolDetail::new("Question", clip(prompt, 160))))
}

fn shell(tool: &str, input: &Value) -> Call {
    let command = text(input, "command").unwrap_or_default();
    let cwd = first_text(input, &["cwd", "working_directory"]);
    let place = cwd
        .map(|cwd| format!(" in {}", file_name(cwd)))
        .unwrap_or_default();
    let headline = if command.is_empty() {
        format!("Run a command{place}")
    } else {
        format!("Run {}{place}", code(command, 60))
    };
    let progress = if command.is_empty() {
        "Running a command".into()
    } else {
        format!("Running {}", clip(command, 40))
    };
    Call::new("shell", "command", "square-terminal", headline, progress)
        .detail((command.chars().count() > 60).then(|| ToolDetail::mono("Command", command)))
        .detail(cwd.map(|cwd| ToolDetail::mono("Folder", cwd)))
        .detail((tool == "powershell").then(|| ToolDetail::new("Shell", "PowerShell")))
        .detail(
            number(input, "timeout")
                .map(|seconds| ToolDetail::new("Timeout", super::text::duration(seconds))),
        )
}

fn task_id(input: &Value) -> Option<&str> {
    first_text(input, &["taskId", "id"])
}

fn background(tool: &str, input: &Value) -> Option<Call> {
    let task = task_id(input)
        .map(|id| format!(" {}", clip(id, 24)))
        .unwrap_or_default();
    let named = first_text(input, &["description", "name"]);
    let call = |headline: String, progress: &str| {
        Call::new("shell", "command", "clock", headline, progress.into())
    };
    Some(match tool {
        "bg_run" => {
            let command = text(input, "command");
            let headline = match (named, command) {
                (Some(name), _) => format!("Start {} in the background", quoted(name, 48)),
                (None, Some(command)) => format!("Start {} in the background", code(command, 48)),
                (None, None) => "Start a background task".into(),
            };
            call(headline, "Starting a background task")
                .detail(command.map(|command| ToolDetail::mono("Command", command)))
        }
        "bg_status" if task.is_empty() => {
            call("Check background tasks".into(), "Checking background tasks")
        }
        "bg_status" => call(
            format!("Check background task{task}"),
            "Checking a background task",
        ),
        "bg_logs" => call(
            format!("Read the log of background task{task}"),
            "Reading a task log",
        ),
        "bg_kill" => call(
            format!("Stop background task{task}"),
            "Stopping a background task",
        ),
        "bg_wait" if task.is_empty() => call(
            "Wait for background tasks".into(),
            "Waiting for background tasks",
        ),
        "bg_wait" => call(
            format!("Wait for background task{task}"),
            "Waiting for a background task",
        ),
        "bg_result" => call(
            format!("Get the result of background task{task}"),
            "Getting a task result",
        ),
        "bg_delegate" => Call::new(
            "agent",
            "agent",
            "bot",
            match named {
                Some(name) => format!("Hand {} to a background agent", quoted(name, 48)),
                None => "Hand a task to a background agent".into(),
            },
            "Delegating".into(),
        )
        .detail(text(input, "prompt").map(|prompt| ToolDetail::new("Task", clip(prompt, 160)))),
        _ => return None,
    })
}

/// The tools a script calls, as `tools.<name>(`.
fn script_tools(script: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for part in script.split("tools.").skip(1) {
        let name: String = part
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        let readable = super::mcp_parts(&name).map_or_else(
            || phrase(&name),
            |(server, tool)| {
                format!(
                    "{} {}",
                    title(&server),
                    super::text::lower_first(&phrase(&tool))
                )
            },
        );
        if !name.is_empty() && !names.contains(&readable) {
            names.push(readable);
        }
    }
    names
}

fn subagent(input: &Value) -> Call {
    let agent = text(input, "agent").map(title);
    let id = text(input, "id").map(|id| clip(id, 24));
    let action = text(input, "action");
    let call = |headline: String| Call::new("agent", "agent", "bot", headline, "Delegating".into());
    match (action, text(input, "task"), text(input, "message")) {
        (Some("list"), _, _) => call("List child agents".into()),
        (Some("status"), _, _) => call(match id {
            Some(id) => format!("Check child agent {id}"),
            None => "Check child agents".into(),
        }),
        (_, _, Some(message)) => call(format!(
            "Message child agent{}: {}",
            id.map(|id| format!(" {id}")).unwrap_or_default(),
            quoted(message, 48)
        )),
        (_, Some(task), _) => call(match &agent {
            Some(agent) => format!("Hand {} to {agent}", quoted(task, 48)),
            None => format!("Hand {} to a child agent", quoted(task, 48)),
        })
        .detail(text(input, "model").map(|model| ToolDetail::mono("Model", model)))
        .detail(text(input, "cwd").map(|cwd| ToolDetail::mono("Folder", cwd))),
        _ => call(match &agent {
            Some(agent) => format!("Use child agent {agent}"),
            None => "Use a child agent".into(),
        }),
    }
}

fn record(tool: &str, input: &Value) -> Option<Call> {
    let call = |headline: &str, progress: &str| {
        Call::new(
            "mcp:record",
            "other",
            "database",
            headline.into(),
            progress.into(),
        )
    };
    Some(match tool {
        "mcp__record__sql" => call("Query the company record", "Querying the record")
            .detail(text(input, "sql").map(|sql| ToolDetail::mono("Query", sql))),
        "mcp__record__propose" => {
            let what = [text(input, "op"), text(input, "kind")]
                .into_iter()
                .flatten()
                .map(phrase)
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase();
            call(
                &if what.is_empty() {
                    "Propose a record change".to_owned()
                } else {
                    format!("Propose a record change: {what}")
                },
                "Proposing a record change",
            )
        }
        "mcp__record__commit" => call("Commit a record change", "Committing a record change"),
        _ => return None,
    })
}

fn agents(tool: &str, input: &Value) -> Option<Call> {
    let call = |icon: &'static str, headline: String, progress: &str| {
        Call::new("agent", "agent", icon, headline, progress.into())
    };
    let to = text(input, "to").unwrap_or("another agent");
    let id = text(input, "id").map(|id| clip(id, 32)).unwrap_or_default();
    Some(match tool {
        "agent-message" => call(
            "messages-square",
            match text(input, "text") {
                Some(message) => format!("Message {to}: {}", quoted(message, 48)),
                None => format!("Message {to}"),
            },
            &format!("Messaging {to}"),
        )
        .detail(text(input, "intent").map(|intent| ToolDetail::new("Intent", phrase(intent)))),
        "agent-handoff" => call(
            "messages-square",
            match text(input, "task") {
                Some(task) => format!("Hand {} to {to}", quoted(task, 48)),
                None => format!("Hand a task to {to}"),
            },
            &format!("Handing off to {to}"),
        )
        .detail(
            text(input, "context").map(|context| ToolDetail::new("Context", clip(context, 160))),
        ),
        "agent-list" => call("bot", "List agents".into(), "Listing agents"),
        "agent-read" => call(
            "bot",
            format!("Read agent {id}").trim().to_owned(),
            "Reading an agent",
        ),
        "agent-save" => call(
            "bot",
            match text(input, "name") {
                Some(name) => format!("Save agent {}", quoted(name, 48)),
                None => "Save an agent".into(),
            },
            "Saving an agent",
        ),
        "agent-run" => call(
            "play",
            format!("Run agent {id}").trim().to_owned(),
            "Starting an agent",
        ),
        "routine-list" => call("list-checks", "List routines".into(), "Listing routines"),
        "routine-save" => {
            let name = input
                .pointer("/routine/name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            call(
                "list-checks",
                if name.is_empty() {
                    "Save a routine".into()
                } else {
                    format!("Save routine {}", quoted(name, 48))
                },
                "Saving a routine",
            )
        }
        "routine-delete" => call(
            "list-checks",
            format!(
                "Delete routine {}",
                text(input, "routine_id").unwrap_or_default()
            )
            .trim()
            .to_owned(),
            "Deleting a routine",
        ),
        "creation-plan" => {
            let kind = text(input, "kind").unwrap_or("artifact");
            let goal = text(input, "goal")
                .map(|goal| format!(": {}", quoted(goal, 48)))
                .unwrap_or_default();
            let article = if kind.starts_with(['a', 'e', 'i', 'o', 'u']) {
                "an"
            } else {
                "a"
            };
            call(
                "list-checks",
                format!("Plan {article} {kind}{goal}"),
                "Planning",
            )
            .detail(
                text(input, "output").map(|output| ToolDetail::new("Output", clip(output, 160))),
            )
        }
        "artifact-publish" => {
            let name = text(input, "name")
                .map(|name| quoted(name, 48))
                .or_else(|| text(input, "path").map(file_name))
                .unwrap_or_default();
            call(
                "file-text",
                format!("Publish {name}").trim().to_owned(),
                "Publishing",
            )
            .detail(path_detail(text(input, "path")))
        }
        "ask_user_question" => {
            let questions: Vec<String> = input
                .get("questions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|question| question.get("question").and_then(Value::as_str))
                .map(str::to_owned)
                .collect();
            let headline = match questions.as_slice() {
                [] => "Ask you a question".to_owned(),
                [one] => format!("Ask you: {}", quoted(one, 60)),
                many => format!("Ask you {} questions", many.len()),
            };
            call("message-square", headline, "Waiting for your answer")
        }
        "plan_submit" => call(
            "list-checks",
            "Submit a plan for approval".into(),
            "Submitting a plan",
        ),
        "browser" => Call::new(
            "web",
            "web",
            "globe",
            match text(input, "goal") {
                Some(goal) => format!("Use the browser: {}", quoted(goal, 60)),
                None => "Use the browser".into(),
            },
            "Using the browser".into(),
        )
        .detail(text(input, "url").map(|url| ToolDetail::mono("Page", url))),
        _ => return None,
    })
}

fn memory(tool: &str, input: &Value) -> Option<Call> {
    let call = |headline: String, progress: &str| {
        Call::new("memory", "memory", "brain", headline, progress.into())
    };
    Some(match tool {
        "memory-search" => call(
            match text(input, "query") {
                Some(query) => format!("Search memory for {}", quoted(query, 60)),
                None => "Search memory".into(),
            },
            "Searching memory",
        ),
        "memory-save" => call(
            match text(input, "title") {
                Some(title) => format!("Remember {}", quoted(title, 60)),
                None => "Save a memory".into(),
            },
            "Saving a memory",
        ),
        "memory-delete" => call("Forget a memory".into(), "Forgetting a memory").detail(
            text(input, "reason").map(|reason| ToolDetail::new("Reason", clip(reason, 160))),
        ),
        "memory-profile-read" => call("Read your profile".into(), "Reading your profile"),
        "memory-profile-save" => call("Update your profile".into(), "Updating your profile"),
        _ => return None,
    })
}

/// Files an `apply_patch` input names.
fn patch_files(input: &Value) -> Vec<String> {
    let patch = first_text(input, &["patch", "input"]).unwrap_or_default();
    patch
        .lines()
        .filter_map(|line| {
            ["*** Update File: ", "*** Add File: ", "*** Delete File: "]
                .iter()
                .find_map(|prefix| line.strip_prefix(prefix))
        })
        .map(|path| path.trim().to_owned())
        .collect()
}

fn files(tool: &str, input: &Value) -> Option<Call> {
    let path = first_text(input, &["path", "file_path", "filePath"]);
    let name = path.map(file_name);
    let file = name.clone().unwrap_or_else(|| "a file".into());
    Some(match tool {
        "read" => {
            let range = match (number(input, "offset"), number(input, "limit")) {
                (Some(offset), Some(limit)) => Some(format!("{limit} lines from line {offset}")),
                (Some(offset), None) => Some(format!("From line {offset}")),
                (None, Some(limit)) => Some(format!("First {limit} lines")),
                (None, None) => None,
            };
            Call::new(
                "files",
                "read",
                "book-open",
                format!("Read {file}"),
                format!("Reading {file}"),
            )
            .detail(path_detail(path))
            .detail(range.map(|range| ToolDetail::new("Lines", range)))
            .path(path)
        }
        "write" => Call::new(
            "files",
            "edit",
            "file-plus",
            format!("Write {file}"),
            format!("Writing {file}"),
        )
        .detail(path_detail(path))
        .detail(text(input, "content").map(|content| {
            ToolDetail::new("Size", count(content.lines().count(), "line", "lines"))
        }))
        .path(path),
        "edit" => {
            let changes = input
                .get("edits")
                .and_then(Value::as_array)
                .map_or(1, Vec::len);
            Call::new(
                "files",
                "edit",
                "square-pen",
                format!("Edit {file}"),
                format!("Editing {file}"),
            )
            .detail(path_detail(path))
            .detail((changes > 1).then(|| ToolDetail::new("Changes", changes.to_string())))
            .path(path)
        }
        "apply_patch" => {
            let files = patch_files(input);
            let headline = match files.as_slice() {
                [one] => format!("Edit {}", file_name(one)),
                [] => "Edit files".to_owned(),
                many => format!("Edit {} files", many.len()),
            };
            Call::new(
                "files",
                "edit",
                "square-pen",
                headline,
                "Editing files".into(),
            )
            .detail((files.len() > 1).then(|| ToolDetail::mono("Files", files.join("\n"))))
            .path(
                files
                    .first()
                    .map(String::as_str)
                    .filter(|_| files.len() == 1),
            )
        }
        "ls" => Call::new(
            "files",
            "list",
            "folder",
            format!("List {}", folder(path)),
            format!("Listing {}", folder(path)),
        )
        .detail(path_detail(path)),
        "find" => {
            let headline = match text(input, "pattern") {
                Some(pattern) => format!("Find {} in {}", code(pattern, 40), folder(path)),
                None => format!("Find files in {}", folder(path)),
            };
            Call::new(
                "files",
                "search",
                "search",
                headline,
                "Finding files".into(),
            )
            .detail(path_detail(path))
        }
        "grep" => {
            let pattern = text(input, "pattern")
                .map(|pattern| format!(" for {}", quoted(pattern, 48)))
                .unwrap_or_default();
            Call::new(
                "files",
                "search",
                "search",
                format!("Search{pattern} in {}", folder(path)),
                "Searching files".into(),
            )
            .detail(path_detail(path))
            .detail(text(input, "glob").map(|glob| ToolDetail::mono("Files", glob)))
            .detail(
                (input.get("ignoreCase") == Some(&Value::Bool(true)))
                    .then(|| ToolDetail::new("Case", "Ignored")),
            )
        }
        _ => return None,
    })
}

fn other(tool: &str, input: &Value) -> Option<Call> {
    Some(match tool {
        "codemode" => {
            let script = text(input, "code").unwrap_or_default();
            let uses = script_tools(script);
            Call::new(
                "code",
                "command",
                "file-code",
                match uses.as_slice() {
                    [] => "Run a script".to_owned(),
                    [one] => format!("Run a script that uses {one}"),
                    many => format!("Run a script that uses {} tools", many.len()),
                },
                "Running a script".into(),
            )
            .detail((uses.len() > 1).then(|| {
                ToolDetail::new(
                    "Uses",
                    listing(&uses.into_iter().take(4).collect::<Vec<_>>()),
                )
            }))
            .detail(
                (!script.is_empty()).then(|| {
                    ToolDetail::new("Script", count(script.lines().count(), "line", "lines"))
                }),
            )
        }
        "get_search_content" => Call::new(
            "web",
            "web",
            "globe",
            match first_text(input, &["query", "url"]) {
                Some(subject) => format!("Read the saved result for {}", quoted(subject, 60)),
                None => "Read a saved search result".into(),
            },
            "Reading a search result".into(),
        ),
        "source_check" => Call::new(
            "web",
            "web",
            "globe",
            match text(input, "claim") {
                Some(claim) => format!("Check sources for {}", quoted(claim, 60)),
                None => "Check sources".into(),
            },
            "Checking sources".into(),
        ),
        "subagent" => subagent(input),
        "fusion_reason" | "fusion_investigate" | "fusion_research" | "fusion_validate" => {
            let subject = first_text(input, &["objective", "prompt"])
                .map(|text| format!(": {}", quoted(text, 60)))
                .unwrap_or_default();
            let verb = phrase(tool.trim_start_matches("fusion_"));
            Call::new(
                "agent",
                "agent",
                "bot",
                format!("{verb} with several models{subject}"),
                "Consulting several models".into(),
            )
        }
        "tool_search" => Call::new(
            "other",
            "search",
            "search",
            match text(input, "query") {
                Some(query) => format!("Find tools for {}", quoted(query, 48)),
                None => "Find tools".into(),
            },
            "Finding tools".into(),
        ),
        "list_mcp_resources" | "list_mcp_resource_templates" => Call::new(
            "other",
            "mcp",
            "server",
            match text(input, "server") {
                Some(server) => format!("List the resources of {}", title(server)),
                None => "List server resources".into(),
            },
            "Listing resources".into(),
        ),
        "read_mcp_resource" => {
            let uri = text(input, "uri")
                .map_or_else(|| "a resource".to_owned(), |uri| short_url(uri, 48));
            let server = text(input, "server")
                .map(|server| format!(" from {}", title(server)))
                .unwrap_or_default();
            Call::new(
                "other",
                "mcp",
                "server",
                format!("Read {uri}{server}"),
                "Reading a resource".into(),
            )
        }
        "compact_context" => Call::new(
            "agent",
            "compact",
            "combine",
            "Compact the context".into(),
            "Compacting context".into(),
        ),
        _ => return None,
    })
}

/// The presentation of a tool the runtime ships, or `None` for any other.
pub(super) fn describe(tool: &str, input: &Value) -> Option<ToolPresentation> {
    let call = match tool {
        "web_search" => web_search(input),
        "fetch_content" => fetch_content(input),
        "bash" | "powershell" => shell(tool, input),
        _ => files(tool, input)
            .or_else(|| background(tool, input))
            .or_else(|| memory(tool, input))
            .or_else(|| agents(tool, input))
            .or_else(|| record(tool, input))
            .or_else(|| other(tool, input))?,
    };
    Some(ToolPresentation {
        category: call.category.into(),
        kind: call.kind.into(),
        icon: call.icon.into(),
        headline: call.headline,
        progress: call.progress,
        details: call.details,
        result: None,
        path: call.path,
        via: None,
    })
}

fn urls(output: &str) -> usize {
    let mut seen: Vec<&str> = Vec::new();
    for word in
        output.split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == '<' || c == '>')
    {
        let word = word.trim_end_matches(['.', ',', ';']);
        if (word.starts_with("http://") || word.starts_with("https://")) && !seen.contains(&word) {
            seen.push(word);
        }
    }
    seen.len()
}

/// The exit code a failed command's message names, as `exited with code N`
/// or `exit code N`.
fn exit_code(output: &str) -> Option<i64> {
    let lower = output.to_ascii_lowercase();
    ["exited with code ", "exit code ", "exit status "]
        .iter()
        .find_map(|marker| {
            let start = lower.rfind(marker)? + marker.len();
            lower[start..]
                .split(|c: char| !(c.is_ascii_digit() || c == '-'))
                .next()?
                .parse()
                .ok()
        })
}

/// How a failed call of a shipped tool reads, when its output says more
/// than its message.
pub(super) fn failure(tool: &str, output: &str, seconds: Option<f64>) -> Option<String> {
    match tool {
        "bash" | "powershell" => {
            let lower = output.to_ascii_lowercase();
            if let Some(code) = exit_code(output) {
                Some(format!("Exit {code}{}", took(seconds)))
            } else if lower.contains("timed out") || lower.contains("timeout") {
                Some(format!("Timed out{}", took(seconds)))
            } else if lower.contains("aborted") || lower.contains("cancelled") {
                Some("Stopped".into())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// How a completed call of a shipped tool reads, or `None` to summarize its
/// output by shape.
pub(super) fn result(
    tool: &str,
    input: &Value,
    output: &str,
    seconds: Option<f64>,
) -> Option<String> {
    let lines = || {
        output
            .lines()
            .filter(|line| !line.trim().is_empty() && !line.starts_with('['))
            .count()
    };
    let json = || serde_json::from_str::<Value>(output).ok();
    Some(match tool {
        "read" if output.starts_with("Details withheld") => return None,
        "read" if !output.is_empty() => {
            format!("Read {}", count(output.lines().count(), "line", "lines"))
        }
        "write" => format!(
            "Wrote {}",
            count(
                text(input, "content").map_or(0, |content| content.lines().count()),
                "line",
                "lines"
            )
        ),
        "edit" => {
            let changes = input
                .get("edits")
                .and_then(Value::as_array)
                .map_or(1, Vec::len);
            format!("Made {}", count(changes, "change", "changes"))
        }
        "apply_patch" => format!(
            "Changed {}",
            count(patch_files(input).len().max(1), "file", "files")
        ),
        "ls" if output.contains("empty directory") => "Empty folder".into(),
        "ls" => count(lines(), "entry", "entries"),
        "find" if output.contains("No files found") => "No files found".into(),
        "find" => format!("Found {}", count(lines(), "file", "files")),
        "grep" if output.contains("No matches found") => "No matches".into(),
        "grep" => format!("Found {}", count(lines(), "match", "matches")),
        "bash" | "powershell" => format!("Exit 0{}", took(seconds)),
        "web_search" if output.contains("No results found") => "No results".into(),
        "web_search" | "source_check" => match urls(output) {
            0 => return None,
            n => format!("Found {}", count(n, "result", "results")),
        },
        "fetch_content" => match output
            .lines()
            .find_map(|line| line.trim().strip_prefix("# "))
        {
            Some(heading) => format!("Read {}", quoted(heading, 60)),
            None => "Fetched".into(),
        },
        "memory-search" => {
            let found = json()
                .and_then(|value| value.get("items").and_then(Value::as_array).map(Vec::len))?;
            if found == 0 {
                "No memories found".into()
            } else {
                format!("Found {}", count(found, "memory", "memories"))
            }
        }
        "memory-save" | "memory-profile-save" | "agent-save" | "routine-save" | "creation-plan" => {
            "Saved".into()
        }
        "memory-delete" | "routine-delete" => "Deleted".into(),
        "artifact-publish" => "Published".into(),
        "agent-message" | "agent-handoff" => "Sent".into(),
        "agent-run" => "Queued".into(),
        "agent-list" => count(
            json().and_then(|value| value.get("agents").and_then(Value::as_array).map(Vec::len))?,
            "agent",
            "agents",
        ),
        "mcp__record__sql" => count(
            output
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count()
                .saturating_sub(1),
            "row",
            "rows",
        ),
        "ask_user_question" => {
            let answers = json()?;
            let picked: Vec<String> = answers
                .get("answers")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .flat_map(|answer| {
                    let selected = strings(answer, "selected");
                    if selected.is_empty() {
                        answer
                            .get("text")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                            .into_iter()
                            .collect()
                    } else {
                        selected
                    }
                })
                .collect();
            if picked.is_empty() {
                "No answer".into()
            } else {
                format!("Answered {}", quoted(&picked.join(", "), 60))
            }
        }
        "compact_context" => return None,
        _ => return None,
    })
    .or_else(|| {
        (tool == "subagent")
            .then(|| first_line(output, 80))
            .filter(|line| !line.is_empty())
    })
}
