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
