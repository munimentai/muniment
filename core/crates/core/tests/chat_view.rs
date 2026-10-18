use std::path::PathBuf;

use muniment_core::chat_view::{
    chat_attachments, chat_pending_permission, chat_tool_activity, projection_phase, SelectedFile,
};
use muniment_core::journal::reducer::{
    AttentionReason, PermissionGate, PermissionRequest, ProjectedAttachment, RunStatus,
    ToolActivity, ToolActivityStatus,
};
use serde_json::json;

#[test]
fn selected_file_uses_the_webview_contract() {
    let file: SelectedFile = serde_json::from_value(json!({"path": "/work/note.txt"})).unwrap();
    assert_eq!(file.path, PathBuf::from("/work/note.txt"));
    assert!(serde_json::from_value::<SelectedFile>(
        json!({"path": "/work/note.txt", "unexpected": true})
    )
    .is_err());
}

#[test]
fn attachments_use_the_webview_contract() {
    let attachments = chat_attachments(&[
        ProjectedAttachment {
            display_name: "note.txt".into(),
            byte_length: 42,
            media_type: Some("text/plain".into()),
            sha256: Some("9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08".into()),
        },
        ProjectedAttachment {
            display_name: "unknown".into(),
            byte_length: 0,
            media_type: None,
            sha256: None,
        },
    ]);

    assert_eq!(
        serde_json::to_value(attachments).unwrap(),
        json!([
            {"displayName": "note.txt", "byteLength": 42, "mediaType": "text/plain",
             "sha256": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"},
            {"displayName": "unknown", "byteLength": 0}
        ])
    );
    assert!(chat_attachments(&[]).is_empty());
}

#[test]
fn pending_permission_uses_the_webview_contract() {
    let permission = chat_pending_permission(
        Some(PermissionGate {
            gate_id: "gate-1".into(),
            request: PermissionRequest::Confirm {
                title: "Allow?".into(),
                message: "Run the tool?".into(),
                timeout: None,
            },
            policy: None,
        }),
        None,
    );

    assert_eq!(
        serde_json::to_value(permission).unwrap(),
        json!({
            "gateId": "gate-1",
            "kind": "confirm",
            "title": "Allow?",
            "message": "Run the tool?"
        })
    );
    assert!(chat_pending_permission(None, None).is_none());

    let code_diff = chat_pending_permission(
        Some(PermissionGate {
            gate_id: "code-gate".into(),
            request: PermissionRequest::CodeDiff {
                effect_id: "effect-1".into(),
                code_diff_id: "diff-1".into(),
                diff_sha256: "aa".into(),
                write_plan_sha256: "bb".into(),
            },
            policy: None,
        }),
        None,
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(code_diff).unwrap(),
        json!({
            "gateId": "code-gate",
            "kind": "code_diff",
            "effect_id": "effect-1",
            "code_diff_id": "diff-1",
            "diff_sha256": "aa",
            "write_plan_sha256": "bb"
        })
    );
}

#[test]
fn tool_activity_uses_the_webview_contract() {
    let activity = chat_tool_activity(&[
        ToolActivity {
            text_offset: 0,
            effect_id: "effect-1".into(),
            parent_effect_id: None,
            display_name: Some("Search".into()),
            status: ToolActivityStatus::Running,
            input: None,
            output: None,
            started_at: None,
            finished_at: None,
        },
        ToolActivity {
            text_offset: 0,
            effect_id: "effect-2".into(),
            parent_effect_id: None,
            display_name: None,
            status: ToolActivityStatus::Completed,
            input: None,
            output: None,
            started_at: None,
            finished_at: None,
        },
        ToolActivity {
            text_offset: 0,
            effect_id: "effect-3".into(),
            parent_effect_id: None,
            display_name: None,
            status: ToolActivityStatus::Failed,
            input: None,
            output: None,
            started_at: None,
            finished_at: None,
        },
    ]);

    assert_eq!(
        serde_json::to_value(activity).unwrap(),
        json!([
            {"textOffset":0,"effectId": "effect-1", "displayName": "Search", "status": "running",
             "presentation": {"category": "other", "kind": "other", "icon": "wrench", "headline": "Search", "progress": "Using search"}},
            {"textOffset":0,"effectId": "effect-2", "displayName": null, "status": "completed",
             "presentation": {"category": "other", "kind": "other", "icon": "wrench", "headline": "Use a tool", "progress": "Using a tool", "result": "Done"}},
            {"textOffset":0,"effectId": "effect-3", "displayName": null, "status": "failed",
             "presentation": {"category": "other", "kind": "other", "icon": "wrench", "headline": "Use a tool", "progress": "Using a tool", "result": "Failed"}}
        ])
    );
    assert!(chat_tool_activity(&[]).is_empty());
}

#[test]
fn projection_phase_maps_every_status() {
    let pending = PermissionGate {
        gate_id: "gate-1".into(),
        request: PermissionRequest::Confirm {
            title: "Allow?".into(),
            message: "Run the tool?".into(),
            timeout: None,
        },
        policy: None,
    };
    let cases = [
        (None, "thinking"),
        (Some(RunStatus::Active), "thinking"),
        (Some(RunStatus::Streaming), "streaming"),
        (Some(RunStatus::Completed), "complete"),
        (Some(RunStatus::Cancelled), "cancelled"),
        (Some(RunStatus::Failed { reason: None }), "failed"),
        (
            Some(RunStatus::NeedsAttention(AttentionReason::Recorded {
                reason: "unknown".into(),
            })),
            "interrupted",
        ),
        (
            Some(RunStatus::PendingPermission(pending)),
            "pending-permission",
        ),
    ];

    for (status, expected) in cases {
        assert_eq!(projection_phase(&status), expected);
    }
}

#[test]
fn a_script_tool_call_names_its_script_and_notes_how_it_ran() {
    let call = |effect_id: &str, parent: Option<&str>, name: &str, input: &str| ToolActivity {
        text_offset: 0,
        effect_id: effect_id.into(),
        parent_effect_id: parent.map(Into::into),
        display_name: Some(name.into()),
        status: ToolActivityStatus::Completed,
        input: Some(input.into()),
        output: None,
        started_at: None,
        finished_at: None,
    };
    let activity = chat_tool_activity(&[
        call(
            "call_1",
            None,
            "codemode",
            r#"{"code":"await tools.web_search({})"}"#,
        ),
        call(
            "call_1/1",
            Some("call_1"),
            "web_search",
            r#"{"query":"desktop apps"}"#,
        ),
        call(
            "orphan/1",
            Some("orphan"),
            "read",
            r#"{"path":"/work/a.md"}"#,
        ),
    ]);
    let value = serde_json::to_value(&activity).unwrap();
    assert!(value[0].get("parentEffectId").is_none());
    assert!(value[0]["presentation"].get("via").is_none());
    assert_eq!(value[1]["parentEffectId"], "call_1");
    assert_eq!(value[1]["presentation"]["via"], "via script");
    // A parent the run does not hold names none.
    assert!(value[2].get("parentEffectId").is_none());
    assert!(value[2]["presentation"].get("via").is_none());
}

/// The drive's run: two codemode scripts, each making one web search, as the
/// journal recorded them.
#[test]
fn a_script_with_one_search_is_one_parent_with_one_child() {
    let call = |effect_id: &str, parent: Option<&str>, name: &str, input: serde_json::Value| {
        ToolActivity {
            text_offset: 0,
            effect_id: effect_id.into(),
            parent_effect_id: parent.map(Into::into),
            display_name: Some(name.into()),
            status: ToolActivityStatus::Completed,
            input: Some(input.to_string()),
            output: None,
            started_at: None,
            finished_at: None,
        }
    };
    let search = json!({"numResults": 3, "query": "Tauri 2 release notes"});
    let activity = chat_tool_activity(&[
        call(
            "call_a",
            None,
            "codemode",
            json!({"code": "const r = await tools.web_search({query: \"Tauri 2 release notes\", numResults: 3}); text(r);"}),
        ),
        call("call_a/1", Some("call_a"), "web_search", search.clone()),
        call(
            "call_b",
            None,
            "codemode",
            json!({"code": "const r = await tools.web_search({query: \"Tauri 2 release notes\", numResults: 3});\nfor (const result of r.queries[0].results.slice(0, 3)) text(result.title);"}),
        ),
        call("call_b/1", Some("call_b"), "web_search", search),
    ]);
    let value = serde_json::to_value(&activity).unwrap();
    let rows: Vec<_> = value
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["parentEffectId"].as_str(),
                row["uncounted"].as_bool().unwrap_or(false),
                row["presentation"]["kind"].as_str().unwrap(),
                row["presentation"]["via"].as_str(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            (None, true, "web", None),
            (Some("call_a"), false, "web", Some("via script")),
            (None, true, "web", None),
            (Some("call_b"), false, "web", Some("via script")),
        ]
    );
    // Each script names its own lines: the first is one line, the second two.
    let lines = |row: &serde_json::Value| {
        row["presentation"]["details"]
            .as_array()
            .unwrap()
            .iter()
            .find(|detail| detail["label"] == "Script")
            .map(|detail| detail["value"].clone())
    };
    assert_eq!(lines(&value[0]), Some(json!("1 line")));
    assert_eq!(lines(&value[2]), Some(json!("2 lines")));
}

#[test]
fn a_script_counts_its_real_lines_and_a_call_cannot_name_a_later_parent() {
    let code = "const r = await tools.web_search({\n  query: \"x\",\n  numResults: 3\n});\nfor (const result of r) {\n  text(result.title);\n}";
    let activity = chat_tool_activity(&[
        ToolActivity {
            text_offset: 0,
            effect_id: "loop".into(),
            parent_effect_id: Some("script".into()),
            display_name: Some("read".into()),
            status: ToolActivityStatus::Completed,
            input: Some(r#"{"path":"/work/a.md"}"#.into()),
            output: None,
            started_at: None,
            finished_at: None,
        },
        ToolActivity {
            text_offset: 0,
            effect_id: "script".into(),
            parent_effect_id: Some("loop".into()),
            display_name: Some("codemode".into()),
            status: ToolActivityStatus::Completed,
            input: Some(json!({ "code": code }).to_string()),
            output: None,
            started_at: None,
            finished_at: None,
        },
    ]);
    let value = serde_json::to_value(&activity).unwrap();
    assert!(value[0].get("parentEffectId").is_none());
    assert_eq!(value[1]["parentEffectId"], "loop");
    assert!(value[1]["presentation"]["details"]
        .as_array()
        .unwrap()
        .contains(&json!({"label": "Script", "value": "7 lines"})));
}
