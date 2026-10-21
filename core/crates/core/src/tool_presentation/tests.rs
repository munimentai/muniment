use serde_json::{json, Value};

use super::*;

fn call(tool: &str, input: Value) -> ToolPresentation {
    describe_with(tool, &input, None, CallOutcome::PENDING)
}

fn ended(
    tool: &str,
    input: Value,
    status: CallStatus,
    output: &str,
    seconds: Option<f64>,
) -> ToolPresentation {
    describe_with(
        tool,
        &input,
        None,
        CallOutcome {
            status,
            output: Some(output),
            seconds,
        },
    )
}

fn detail(presentation: &ToolPresentation, label: &str) -> Option<String> {
    presentation
        .details
        .iter()
        .find(|detail| detail.label == label)
        .map(|detail| detail.value.clone())
}

fn assert_no_json(presentation: &ToolPresentation) {
    for text in [&presentation.headline, &presentation.progress] {
        assert!(
            !text.contains('{') && !text.contains("\":"),
            "raw JSON in {text:?}"
        );
    }
}

#[test]
fn web_search_reads_its_query_and_site_and_hides_defaults() {
    let input = json!({
        "category": "", "domainFilter": ["news.ycombinator.com"], "includeContent": true,
        "numResults": 8, "provider": "auto", "proxy": "",
        "queries": ["site:news.ycombinator.com \"AI\" \"desktop\"", "site:news.ycombinator.com \"Show HN\""],
    });
    let shown = call("web_search", input);
    assert_eq!(
        shown.headline,
        "Search the web for \u{201c}AI desktop\u{201d} and 1 more on news.ycombinator.com"
    );
    assert_eq!(
        (
            shown.category.as_str(),
            shown.kind.as_str(),
            shown.icon.as_str()
        ),
        ("web", "web", "globe")
    );
    assert_eq!(shown.progress, "Searching the web");
    assert_eq!(detail(&shown, "Results per query").as_deref(), Some("8"));
    assert_eq!(detail(&shown, "Page content").as_deref(), Some("Included"));
    assert!(detail(&shown, "Queries").unwrap().contains("Show HN"));
    assert_eq!(detail(&shown, "Provider"), None);
    assert_eq!(detail(&shown, "Category"), None);
    assert_no_json(&shown);

    let one = call("web_search", json!({"query": "rust serde"}));
    assert_eq!(
        one.headline,
        "Search the web for \u{201c}rust serde\u{201d}"
    );
    let found = ended(
        "web_search",
        json!({"query": "x"}),
        CallStatus::Completed,
        "1. A\n   https://a.test/1\n\n2. B\n   https://b.test/2",
        None,
    );
    assert_eq!(found.result.as_deref(), Some("Found 2 results"));
    let none = ended(
        "web_search",
        json!({"query": "x"}),
        CallStatus::Completed,
        "No results found.",
        None,
    );
    assert_eq!(none.result.as_deref(), Some("No results"));
}

#[test]
fn file_tools_name_the_file_and_count_what_they_did() {
    let read = ended(
        "read",
        json!({"path": "/work/muniment/README.md"}),
        CallStatus::Completed,
        &"line\n".repeat(120),
        None,
    );
    assert_eq!(read.headline, "Read README.md");
    assert_eq!(read.progress, "Reading README.md");
    assert_eq!(read.path.as_deref(), Some("/work/muniment/README.md"));
    assert_eq!(read.result.as_deref(), Some("Read 120 lines"));
    assert_eq!(
        detail(&read, "Path").as_deref(),
        Some("/work/muniment/README.md")
    );
    assert!(read.details[0].mono);
    assert_eq!(
        call("bash", json!({"command": "npm test"})).progress,
        "Running npm test"
    );
    let compacted = ended(
        "compact_context",
        json!({"reason": "threshold"}),
        CallStatus::Completed,
        "Context compacted.",
        None,
    );
    assert_eq!(
        (
            compacted.headline.as_str(),
            compacted.kind.as_str(),
            compacted.result.as_deref()
        ),
        ("Context automatically compacted", "compact", None)
    );

    let ranged = call("read", json!({"path": "a.rs", "offset": 10, "limit": 40}));
    assert_eq!(
        detail(&ranged, "Lines").as_deref(),
        Some("40 lines from line 10")
    );

    let write = ended(
        "write",
        json!({"path": "notes/plan.md", "content": "a\nb\nc"}),
        CallStatus::Completed,
        "ok",
        None,
    );
    assert_eq!(
        (write.headline.as_str(), write.kind.as_str()),
        ("Write plan.md", "edit")
    );
    assert_eq!(write.result.as_deref(), Some("Wrote 3 lines"));

    let edit = ended(
        "edit",
        json!({"path": "src/main.rs", "edits": [{"oldText": "a", "newText": "b"}, {"oldText": "c", "newText": "d"}]}),
        CallStatus::Completed,
        "Successfully replaced",
        None,
    );
    assert_eq!(edit.headline, "Edit main.rs");
    assert_eq!(edit.result.as_deref(), Some("Made 2 changes"));

    let patch = call(
        "apply_patch",
        json!({"patch": "*** Begin Patch\n*** Update File: a.rs\n*** Add File: b.rs\n*** Delete File: c.rs\n*** End Patch"}),
    );
    assert_eq!(patch.headline, "Edit 3 files");

    let ls = ended("ls", json!({}), CallStatus::Completed, "a.rs\nb/\n", None);
    assert_eq!(
        (ls.headline.as_str(), ls.result.as_deref()),
        ("List the working folder", Some("2 entries"))
    );

    let grep = ended(
        "grep",
        json!({"pattern": "TODO", "path": "src", "glob": "*.rs"}),
        CallStatus::Completed,
        "src/a.rs:3: TODO\nsrc/b.rs:9: TODO",
        None,
    );
    assert_eq!(grep.headline, "Search for \u{201c}TODO\u{201d} in src");
    assert_eq!(
        (grep.kind.as_str(), grep.result.as_deref()),
        ("search", Some("Found 2 matches"))
    );
    assert_eq!(detail(&grep, "Files").as_deref(), Some("*.rs"));

    let find = ended(
        "find",
        json!({"pattern": "*.toml"}),
        CallStatus::Completed,
        "No files found matching pattern",
        None,
    );
    assert_eq!(find.headline, "Find `*.toml` in the working folder");
    assert_eq!(find.result.as_deref(), Some("No files found"));
}

#[test]
fn shell_commands_name_the_command_folder_and_exit() {
    let request = PolicyRequest {
        request_kind: crate::journal::ThreadPermissionRequestKind::Command,
        resource: ThreadPermissionResource::Command {
            arguments: vec!["bash".into(), "npm test".into()],
            working_directory: "/Users/me/gk/muniment-desktop".into(),
        },
    };
    let gate = describe_request(&request);
    assert_eq!(gate.headline, "Run `npm test` in muniment-desktop");
    assert_eq!(
        detail(&gate, "Folder").as_deref(),
        Some("/Users/me/gk/muniment-desktop")
    );
    assert_eq!(
        (gate.category.as_str(), gate.icon.as_str()),
        ("shell", "square-terminal")
    );

    let done = ended(
        "bash",
        json!({"command": "npm test"}),
        CallStatus::Completed,
        "ok",
        Some(4.24),
    );
    assert_eq!(done.result.as_deref(), Some("Exit 0 in 4.2 s"));
    let failed = ended(
        "bash",
        json!({"command": "false"}),
        CallStatus::Failed,
        "boom\n\nCommand exited with code 2",
        Some(61.0),
    );
    assert_eq!(failed.result.as_deref(), Some("Exit 2 in 1 min 1 s"));
    let denied = ended(
        "bash",
        json!({"command": "cat /root/x"}),
        CallStatus::Failed,
        "Error: Permission denied.",
        None,
    );
    assert_eq!(denied.result.as_deref(), Some("Failed: permission denied"));
    let long = call("bash", json!({"command": "x".repeat(200)}));
    assert!(long.headline.chars().count() < 80);
    assert_eq!(detail(&long, "Command").map(|value| value.len()), Some(200));
}

#[test]
fn background_scripts_and_child_agents_read_as_actions() {
    assert_eq!(
        call(
            "bg_run",
            json!({"name": "dev server", "command": "npm run dev"})
        )
        .headline,
        "Start \u{201c}dev server\u{201d} in the background"
    );
    assert_eq!(
        call("bg_status", json!({})).headline,
        "Check background tasks"
    );
    assert_eq!(
        call("bg_logs", json!({"taskId": "t1"})).headline,
        "Read the log of background task t1"
    );
    let script = call(
        "codemode",
        json!({"code": "const a = await tools.mcp__linear__create_issue({})\nreturn a"}),
    );
    assert_eq!(
        script.headline,
        "Run a script that uses Linear create issue"
    );
    assert_eq!(
        (script.category.as_str(), script.icon.as_str()),
        ("code", "file-code")
    );
    assert_eq!(
        call(
            "subagent",
            json!({"agent": "researcher", "task": "Find the lease terms"})
        )
        .headline,
        "Hand \u{201c}Find the lease terms\u{201d} to Researcher"
    );
    assert_eq!(
        call("subagent", json!({"action": "list"})).headline,
        "List child agents"
    );
    assert_eq!(
        call(
            "bg_delegate",
            json!({"name": "audit", "prompt": "Check it"})
        )
        .category,
        "agent"
    );
}

#[test]
fn muniment_tools_read_as_actions() {
    assert_eq!(
        call("memory-search", json!({"query": "lease"})).headline,
        "Search memory for \u{201c}lease\u{201d}"
    );
    assert_eq!(
        call(
            "memory-save",
            json!({"title": "Prefers mornings", "content": "x"})
        )
        .headline,
        "Remember \u{201c}Prefers mornings\u{201d}"
    );
    let message = call(
        "agent-message",
        json!({"to": "Scout", "intent": "question", "text": "Any news?"}),
    );
    assert_eq!(message.headline, "Message Scout: \u{201c}Any news?\u{201d}");
    assert_eq!(detail(&message, "Intent").as_deref(), Some("Question"));
    assert_eq!(
        call("agent-handoff", json!({"to": "Scout", "task": "Draft it"})).headline,
        "Hand \u{201c}Draft it\u{201d} to Scout"
    );
    assert_eq!(
        call("artifact-publish", json!({"path": "/w/page.html"})).headline,
        "Publish page.html"
    );
    assert_eq!(
        call(
            "creation-plan",
            json!({"kind": "agent", "goal": "Watch prices"})
        )
        .headline,
        "Plan an agent: \u{201c}Watch prices\u{201d}"
    );
    assert_eq!(
        call("plan_submit", json!({"plan": "# Plan"})).headline,
        "Submit a plan for approval"
    );
    assert_eq!(
        call(
            "ask_user_question",
            json!({"questions": [{"id": "a", "question": "Which month?"}]})
        )
        .headline,
        "Ask you: \u{201c}Which month?\u{201d}"
    );
    let sql = ended(
        "mcp__record__sql",
        json!({"sql": "select 1"}),
        CallStatus::Completed,
        "a,b\n1,2\n3,4",
        None,
    );
    assert_eq!(
        (sql.headline.as_str(), sql.result.as_deref()),
        ("Query the company record", Some("2 rows"))
    );
    assert_eq!(detail(&sql, "Query").as_deref(), Some("select 1"));
    let memory = ended(
        "memory-search",
        json!({"query": "x"}),
        CallStatus::Completed,
        r#"{"items":[1,2,3],"recall":{}}"#,
        None,
    );
    assert_eq!(memory.result.as_deref(), Some("Found 3 memories"));
    let answer = ended(
        "ask_user_question",
        json!({}),
        CallStatus::Completed,
        r#"{"answers":[{"id":"a","selected":["June"],"text":""}]}"#,
        None,
    );
    assert_eq!(
        answer.result.as_deref(),
        Some("Answered \u{201c}June\u{201d}")
    );
    assert_eq!(
        call(
            "fetch_content",
            json!({"url": "https://www.example.com/docs/"})
        )
        .headline,
        "Fetch example.com/docs"
    );
    assert_eq!(
        call("browser", json!({"goal": "Open the pricing page"})).progress,
        "Using the browser"
    );
}

#[test]
fn an_mcp_tool_without_metadata_reads_from_its_name_and_arguments() {
    let shown = call(
        "mcp__linear__create_issue",
        json!({"title": "Fix sync bug", "teamId": "ENG", "priority": 2, "labels": [], "draft": false}),
    );
    assert_eq!(
        shown.headline,
        "Ask Linear: create issue \u{201c}Fix sync bug\u{201d}"
    );
    assert_eq!(
        (
            shown.category.as_str(),
            shown.kind.as_str(),
            shown.icon.as_str()
        ),
        ("mcp:linear", "mcp", "server")
    );
    assert_eq!(shown.progress, "Asking Linear");
    assert_eq!(detail(&shown, "Team ID").as_deref(), Some("ENG"));
    assert_eq!(detail(&shown, "Priority").as_deref(), Some("2"));
    assert_eq!(detail(&shown, "Labels"), None);
    assert_eq!(detail(&shown, "Draft"), None);
    assert_no_json(&shown);

    // A camel-case tool, a server prefix on the tool and an item id prefix on the server.
    let camel = call(
        "mcp__extend-0b8f5e6a-1c2d-4e5f-8a9b-0c1d2e3f4a5b-github__github_searchRepositories",
        json!({"q": "tauri"}),
    );
    assert_eq!(
        camel.headline,
        "Ask Github: search repositories \u{201c}tauri\u{201d}"
    );
    let bare = call("mcp__Context7__resolve_library_id", json!({}));
    assert_eq!(bare.headline, "Ask Context7: resolve library ID");
}

#[test]
fn an_mcp_tool_with_a_schema_uses_its_titles_defaults_and_enums() {
    let meta = ToolMeta::from_value(&json!({
        "name": "mcp__crm__find_leads",
        "title": "Find leads",
        "inputSchema": {
            "type": "object",
            "required": ["region"],
            "properties": {
                "region": {"type": "string", "title": "Sales region"},
                "limit": {"type": "integer", "default": 20},
                "stage": {"type": "string", "oneOf": [{"const": "won", "title": "Closed won"}, {"const": "open", "title": "Open"}]},
                "fresh": {"type": "boolean", "default": true},
                "apiToken": {"type": "string"},
            },
        },
    }));
    let input =
        json!({"region": "EMEA", "limit": 20, "stage": "won", "fresh": false, "apiToken": "abc"});
    let shown = describe_with(
        "mcp__crm__find_leads",
        &input,
        Some(&meta),
        CallOutcome::PENDING,
    );
    // The one required text argument names the subject.
    assert_eq!(shown.headline, "Ask CRM: find leads \u{201c}EMEA\u{201d}");
    assert_eq!(detail(&shown, "Limit"), None);
    assert_eq!(detail(&shown, "Stage").as_deref(), Some("Closed won"));
    assert_eq!(detail(&shown, "Fresh").as_deref(), Some("No"));
    assert_eq!(detail(&shown, "API token").as_deref(), Some("Hidden"));

    let server = ToolMeta {
        server: Some("HubSpot CRM".into()),
        ..meta
    };
    assert!(describe_with(
        "mcp__crm__find_leads",
        &input,
        Some(&server),
        CallOutcome::PENDING
    )
    .headline
    .starts_with("Ask HubSpot CRM: find leads"));
}

#[test]
fn a_tool_can_opt_in_to_its_own_presentation() {
    let meta = ToolMeta::from_value(&json!({
        "name": "mcp__tracker__issue_create",
        "parameters": {
            "type": "object",
            "properties": {"summary": {"type": "string"}, "board": {"type": "string"}},
            "x-muniment-presentation": {
                "headline": "File {summary} on {board}",
                "progress": "Filing an issue",
                "icon": "list-checks",
                "category": "agent",
                "result": "Filed {key}",
            },
        },
    }));
    let input = json!({"summary": "Crash on start", "board": "APP"});
    let done = describe_with(
        "mcp__tracker__issue_create",
        &input,
        Some(&meta),
        CallOutcome {
            status: CallStatus::Completed,
            output: Some(r#"{"key":"APP-12"}"#),
            seconds: None,
        },
    );
    assert_eq!(
        done.headline,
        "Ask Tracker: file \u{201c}Crash on start\u{201d} on \u{201c}APP\u{201d}"
    );
    assert_eq!(
        (
            done.progress.as_str(),
            done.icon.as_str(),
            done.category.as_str()
        ),
        ("Filing an issue", "list-checks", "agent")
    );
    assert_eq!(done.result.as_deref(), Some("Filed APP-12"));
    assert!(done.details.is_empty());

    // A template whose property the call leaves out falls back to the heuristic.
    let partial = describe_with(
        "mcp__tracker__issue_create",
        &json!({"summary": "Crash"}),
        Some(&meta),
        CallOutcome::PENDING,
    );
    assert_eq!(
        partial.headline,
        "Ask Tracker: issue create \u{201c}Crash\u{201d}"
    );
}

#[test]
fn an_unknown_plugin_tool_reads_from_its_name() {
    let shown = call(
        "translateDocument",
        json!({"file_path": "/docs/brief.pdf", "targetLanguage": "de", "options": {"formal": true}}),
    );
    assert_eq!(shown.headline, "Translate document brief.pdf");
    assert_eq!(shown.progress, "Using translate document");
    assert_eq!(
        (
            shown.category.as_str(),
            shown.kind.as_str(),
            shown.icon.as_str()
        ),
        ("other", "other", "wrench")
    );
    assert_eq!(detail(&shown, "Target language").as_deref(), Some("de"));
    assert_eq!(detail(&shown, "Options").as_deref(), Some("formal: Yes"));

    let registered = ToolMeta::from_value(&json!({"name": "weather", "label": "Weather forecast"}));
    let named = describe_with(
        "weather",
        &json!({"city": "Lisbon", "days": 3}),
        Some(&registered),
        CallOutcome::PENDING,
    );
    assert_eq!(named.headline, "Weather forecast \u{201c}Lisbon\u{201d}");

    let many = call(
        "bulk_tag",
        json!({"ids": ["a", "b", "c", "d", "e"], "tag": "x".repeat(400)}),
    );
    assert_eq!(detail(&many, "IDs").as_deref(), Some("5 IDs"));
    // The only text argument names the subject, clipped.
    assert!(
        many.headline.starts_with("Bulk tag \u{201c}xxx") && many.headline.chars().count() < 80
    );
    assert_no_json(&many);

    let empty = call("ping", Value::Null);
    assert_eq!((empty.headline.as_str(), empty.details.len()), ("Ping", 0));
}

#[test]
fn results_summarize_common_shapes_and_failures() {
    let list = |output: &str| {
        ended(
            "mcp__x__list_things",
            json!({}),
            CallStatus::Completed,
            output,
            None,
        )
        .result
    };
    assert_eq!(list("[1,2,3]").as_deref(), Some("3 items"));
    assert_eq!(
        list(r#"{"issues":[{"id":1}],"next":null}"#).as_deref(),
        Some("1 issue")
    );
    assert_eq!(list(r#"{"results":[1,2]}"#).as_deref(), Some("2 results"));
    assert_eq!(
        list(r#"{"id":"ENG-42","title":"Fix sync bug"}"#).as_deref(),
        Some("Returned \u{201c}Fix sync bug\u{201d}")
    );
    assert_eq!(list(r#"{"id":42}"#).as_deref(), Some("Returned 42"));
    assert_eq!(
        list("Created the issue.\nMore text").as_deref(),
        Some("Created the issue.")
    );
    assert_eq!(list("").as_deref(), Some("Done"));
    let failed =
        |output: &str| ended("mcp__x__y", json!({}), CallStatus::Failed, output, None).result;
    assert_eq!(
        failed(r#"{"error":{"message":"Rate limit exceeded"}}"#).as_deref(),
        Some("Failed: rate limit exceeded")
    );
    assert_eq!(
        failed("Error: Not found").as_deref(),
        Some("Failed: not found")
    );
    assert_eq!(failed("").as_deref(), Some("Failed"));
    assert_eq!(
        ended("mcp__x__y", json!({}), CallStatus::Running, "partial", None).result,
        None
    );
}

#[test]
fn registered_metadata_reaches_calls_and_activities() {
    remember_catalog(&json!([
        {"name": "mcp__notes__append_note", "description": "Append", "parameters": {"type": "object", "required": ["note"], "properties": {"note": {"type": "string"}, "pinned": {"type": "boolean", "default": false}}}},
    ]));
    let shown = describe(
        "mcp__notes__append_note",
        &json!({"note": "Call Sam", "pinned": false}),
    );
    assert_eq!(
        shown.headline,
        "Ask Notes: append note \u{201c}Call Sam\u{201d}"
    );
    let activity = ToolActivity {
        text_offset: 0,
        effect_id: "e1".into(),
        parent_effect_id: None,
        display_name: Some("bash".into()),
        status: ToolActivityStatus::Completed,
        input: Some(r#"{"command":"ls"}"#.into()),
        output: Some("a".into()),
        started_at: Some("2026-10-18T10:00:00Z".into()),
        finished_at: Some("2026-10-18T10:00:02.500Z".into()),
    };
    let described = describe_activity(&activity);
    assert_eq!(
        (described.headline.as_str(), described.result.as_deref()),
        ("Run `ls`", Some("Exit 0 in 2.5 s"))
    );
    let withheld = ToolActivity {
        input: Some("Details withheld because they may contain a secret.".into()),
        display_name: Some("mcp__vault__read_secret".into()),
        ..activity
    };
    assert_eq!(
        describe_activity(&withheld).headline,
        "Ask Vault: read secret"
    );
}

#[test]
fn a_gate_on_a_tool_describes_the_tool_and_its_input() {
    let request = PolicyRequest {
        request_kind: crate::journal::ThreadPermissionRequestKind::Path,
        resource: ThreadPermissionResource::Path {
            path: "/work/a.md".into(),
            operation: "write".into(),
        },
    };
    assert_eq!(describe_request(&request).headline, "Write a.md");
    let tool = PolicyRequest {
        request_kind: crate::journal::ThreadPermissionRequestKind::Tool,
        resource: ThreadPermissionResource::Tool {
            name: "mcp__crm__delete".into(),
            input: json!({"id": 7}),
        },
    };
    assert_eq!(describe_request(&tool).headline, "Ask CRM: delete 7");
}

#[test]
fn text_helpers_humanize_identifiers() {
    assert_eq!(text::phrase("numResults"), "Number of results");
    assert_eq!(text::phrase("getHTTPResponse"), "Get HTTP response");
    assert_eq!(text::phrase("file_path"), "File path");
    assert_eq!(text::title("google-drive"), "Google Drive");
    assert_eq!(text::duration(0.04), "0.0 s");
    assert_eq!(text::duration(42.4), "42 s");
    assert_eq!(text::duration(3725.0), "1 h 2 min");
    assert_eq!(text::plural("query"), "queries");
    assert_eq!(text::singular("queries"), "query");
    assert_eq!(text::clip("a  b\nc", 10), "a b c");
    assert_eq!(text::clip("abcdefghij", 5), "abcd…");
}

/// The desktop's tests and probe gallery render these presentations.
/// `MUNIMENT_WRITE_FIXTURES=1` writes them again after a describer changes.
#[test]
fn the_gallery_fixture_matches_the_describers() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocol-fixtures/tool-presentation");
    let gallery: Value =
        serde_json::from_slice(&std::fs::read(folder.join("calls.json")).unwrap()).unwrap();
    let start = chrono::DateTime::parse_from_rfc3339("2026-10-18T12:00:00Z").unwrap();
    let stamp = |at: chrono::DateTime<chrono::FixedOffset>| {
        at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    };
    let calls: Vec<Value> = gallery["calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|call| {
            let tool = call["tool"].as_str().unwrap();
            let status = call["status"].as_str().unwrap();
            let seconds = call["seconds"].as_f64();
            let output = call["output"].as_str();
            let meta = call.get("meta").map(ToolMeta::from_value);
            let presentation = describe_with(
                tool,
                &call["input"],
                meta.as_ref(),
                CallOutcome {
                    status: match status {
                        "running" => CallStatus::Running,
                        "failed" => CallStatus::Failed,
                        _ => CallStatus::Completed,
                    },
                    output,
                    seconds,
                },
            );
            let mut activity = json!({
                "textOffset": 0,
                "effectId": call["id"],
                "displayName": tool,
                "status": status,
                "input": serde_json::to_string_pretty(&call["input"]).unwrap(),
                "startedAt": stamp(start),
                "presentation": presentation,
            });
            if let Some(output) = output {
                activity["output"] = json!(output);
            }
            if let Some(seconds) = seconds {
                let end = start + chrono::Duration::milliseconds((seconds * 1000.0) as i64);
                activity["finishedAt"] = json!(stamp(end));
            }
            activity
        })
        .collect();
    let gates: Vec<Value> = gallery["gates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|gate| {
            let rule: PolicyRequest = serde_json::from_value(gate["rule"].clone()).unwrap();
            let presentation = describe_request(&rule);
            json!({
                "id": gate["id"],
                "agent_name": gate["agent"],
                "reason": gate["reason"],
                "rule": rule,
                "summary": presentation.headline,
                "presentation": presentation,
            })
        })
        .collect();
    for activity in &calls {
        let presentation: ToolPresentation =
            serde_json::from_value(activity["presentation"].clone()).unwrap();
        assert_no_json(&presentation);
    }
    let presented =
        serde_json::to_string_pretty(&json!({"calls": calls, "gates": gates})).unwrap() + "\n";
    if std::env::var_os("MUNIMENT_WRITE_FIXTURES").is_some() {
        std::fs::write(folder.join("presented.json"), &presented).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(folder.join("presented.json")).unwrap(),
        presented
    );
}

#[test]
fn an_agent_named_by_its_id_reads_with_its_name_and_its_refusal_keeps_the_capital() {
    let id = "4f3c0183-7f2a-4c4e-9b1d-2a6f0c9e8d11";
    remember_agent(id, "Drive9 Helper");
    let message = json!({"to": id, "intent": "request", "text": "Find the release."});
    assert_eq!(
        call("agent-message", message.clone()).headline,
        "Message Drive9 Helper: \u{201c}Find the release.\u{201d}"
    );
    let unknown = json!({"to": "0b7e1f3a-1c2d-4e5f-8a9b-0c1d2e3f4a5b", "text": "Hi"});
    assert_eq!(
        call("agent-message", unknown).headline,
        "Message another agent: \u{201c}Hi\u{201d}"
    );
    let refused = ended(
        "agent-message",
        message,
        CallStatus::Failed,
        "Tauri Morning may not message Drive9 Helper. Choose agents under Messages to other agents in its profile.",
        None,
    );
    assert_eq!(
        refused.result.as_deref(),
        Some("Failed: Tauri Morning may not message Drive9 Helper")
    );
}
