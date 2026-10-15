use super::*;
use serde_json::json;
#[cfg(any(unix, target_os = "windows"))]
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[cfg(any(unix, target_os = "windows"))]
#[derive(Clone, Debug, Eq, PartialEq)]
enum RunClientCall {
    Submit(String, Vec<String>, Option<String>, Option<String>),
    Resume(String),
    Steer(String, String),
    FollowUp(String, String),
    Cancel(String),
    PermissionAnswer(String, String, AttachChatPermissionAnswer),
}

#[cfg(any(unix, target_os = "windows"))]
#[derive(Clone, Default)]
struct FakeRunClient(Arc<Mutex<Vec<RunClientCall>>>, bool);

#[cfg(any(unix, target_os = "windows"))]
impl FakeRunClient {
    fn calls(&self) -> Vec<RunClientCall> {
        self.0.lock().unwrap().clone()
    }

    fn with_runtime_upgrade_pending() -> Self {
        Self(Arc::default(), true)
    }
}

#[cfg(any(unix, target_os = "windows"))]
impl RunCommandClient for FakeRunClient {
    fn run_submit(
        &self,
        text: &str,
        files: &[String],
        thread_id: Option<&str>,
        thinking_level: Option<&str>,
    ) -> Result<RunSubmitAccepted, String> {
        if self.1 {
            return Err(auth::desktop_client_error(
                ClientError::RuntimeUpgradePending,
            ));
        }
        self.0.lock().unwrap().push(RunClientCall::Submit(
            text.to_owned(),
            files.to_vec(),
            thread_id.map(str::to_owned),
            thinking_level.map(str::to_owned),
        ));
        Ok(RunSubmitAccepted {
            run_id: "remote-run".to_string(),
            thread_id: thread_id.unwrap_or("accepted-thread").to_string(),
            attachments: Vec::new(),
            committed_seq: 7,
            accepted_at: "now".to_string(),
        })
    }

    fn run_resume(&self, run_id: &str) -> Result<RunResumeAccepted, ClientError> {
        if self.1 {
            return Err(ClientError::RuntimeUpgradePending);
        }
        self.0
            .lock()
            .unwrap()
            .push(RunClientCall::Resume(run_id.to_string()));
        Ok(RunResumeAccepted {
            run_id: run_id.to_string(),
            thread_id: "thread-1".to_string(),
            committed_seq: 8,
            accepted_at: "now".to_string(),
        })
    }

    fn run_steer(&self, run_id: &str, text: &str) -> Result<RunMessageAccepted, ClientError> {
        if self.1 {
            return Err(ClientError::RuntimeUpgradePending);
        }
        self.0
            .lock()
            .unwrap()
            .push(RunClientCall::Steer(run_id.to_string(), text.to_string()));
        Ok(RunMessageAccepted {
            run_id: run_id.to_string(),
            accepted_at: "now".to_string(),
        })
    }

    fn run_follow_up(&self, run_id: &str, text: &str) -> Result<RunMessageAccepted, ClientError> {
        if self.1 {
            return Err(ClientError::RuntimeUpgradePending);
        }
        self.0.lock().unwrap().push(RunClientCall::FollowUp(
            run_id.to_string(),
            text.to_string(),
        ));
        Ok(RunMessageAccepted {
            run_id: run_id.to_string(),
            accepted_at: "now".to_string(),
        })
    }

    fn run_cancel(&self, run_id: &str) -> Result<RunCancelAccepted, ClientError> {
        self.0
            .lock()
            .unwrap()
            .push(RunClientCall::Cancel(run_id.to_string()));
        Ok(RunCancelAccepted {
            run_id: run_id.to_string(),
            accepted_at: "now".to_string(),
        })
    }

    fn run_permission_answer(
        &self,
        run_id: &str,
        gate_id: &str,
        answer: AttachChatPermissionAnswer,
    ) -> Result<RunPermissionAnswerAccepted, ClientError> {
        self.0.lock().unwrap().push(RunClientCall::PermissionAnswer(
            run_id.to_string(),
            gate_id.to_string(),
            answer.clone(),
        ));
        Ok(RunPermissionAnswerAccepted {
            run_id: run_id.to_string(),
            gate_id: gate_id.to_string(),
            answer,
            committed_seq: 9,
            accepted_at: "now".to_string(),
        })
    }
}

#[cfg(any(unix, target_os = "windows"))]
fn assert_disconnected<T>(result: Result<T, String>) {
    assert_eq!(
        result.err().unwrap(),
        "Muniment cannot reach its background service."
    );
}

#[cfg(any(unix, target_os = "windows"))]
fn assert_runtime_update_pending<T>(result: Result<T, String>) {
    assert_eq!(
        result.err().unwrap(),
        "A runtime update is pending. Muniment will start new runs after the update."
    );
}

#[cfg(any(unix, target_os = "windows"))]
#[test]
fn chat_submit_routes_all_desktop_client_states() {
    let state = ChatState::default();
    assert_disconnected(tauri::async_runtime::block_on(handle_run_submit::<
        FakeRunClient,
    >(
        RunCommandSession::NoSupervisor,
        &state,
        None,
        "prompt",
        vec![],
        None,
    )));

    state
        .session_thread
        .select("selected-thread".into(), Some("subject-1"));
    let client = FakeRunClient::default();
    let connected = tauri::async_runtime::block_on(handle_run_submit(
        RunCommandSession::Connected(client.clone()),
        &state,
        Some("subject-1"),
        "prompt",
        vec![SelectedFile {
            path: PathBuf::from("/tmp/a.txt"),
        }],
        Some("high"),
    ));
    assert_eq!(connected.unwrap().run_id, "remote-run");
    assert_eq!(
        state.session_thread.current(Some("subject-1")),
        Some("selected-thread".to_string())
    );
    assert_eq!(
        client.calls(),
        [RunClientCall::Submit(
            "prompt".to_string(),
            vec!["/tmp/a.txt".to_string()],
            Some("selected-thread".to_string()),
            Some("high".to_string())
        )]
    );

    let new_client = FakeRunClient::default();
    let created = tauri::async_runtime::block_on(handle_run_submit(
        RunCommandSession::Connected(new_client),
        &state,
        Some("subject-2"),
        "new prompt",
        vec![],
        None,
    ));
    assert!(created.is_ok());
    assert_eq!(
        state.session_thread.current(Some("subject-2")),
        Some("accepted-thread".to_string())
    );
    assert_runtime_update_pending(tauri::async_runtime::block_on(handle_run_submit(
        RunCommandSession::Connected(FakeRunClient::with_runtime_upgrade_pending()),
        &state,
        None,
        "prompt",
        vec![],
        None,
    )));
    assert_disconnected(tauri::async_runtime::block_on(handle_run_submit::<
        FakeRunClient,
    >(
        RunCommandSession::Disconnected,
        &state,
        None,
        "prompt",
        vec![],
        None,
    )));
}

#[cfg(any(unix, target_os = "windows"))]
#[test]
fn chat_resume_routes_all_desktop_client_states() {
    assert_disconnected(tauri::async_runtime::block_on(handle_run_resume::<
        FakeRunClient,
    >(
        RunCommandSession::NoSupervisor, "run-1"
    )));
    let client = FakeRunClient::default();
    let connected = tauri::async_runtime::block_on(handle_run_resume(
        RunCommandSession::Connected(client.clone()),
        "run-1",
    ));
    assert!(connected.is_ok());
    assert_eq!(client.calls(), [RunClientCall::Resume("run-1".to_string())]);
    assert_runtime_update_pending(tauri::async_runtime::block_on(handle_run_resume(
        RunCommandSession::Connected(FakeRunClient::with_runtime_upgrade_pending()),
        "run-1",
    )));
    assert_disconnected(tauri::async_runtime::block_on(handle_run_resume::<
        FakeRunClient,
    >(
        RunCommandSession::Disconnected, "run-1"
    )));
}

#[cfg(any(unix, target_os = "windows"))]
#[test]
fn chat_queue_routes_all_desktop_client_states_and_deliveries() {
    assert_disconnected(handle_run_queue::<FakeRunClient>(
        RunCommandSession::NoSupervisor,
        "run-1",
        ChatDelivery::Steer,
        "message",
    ));
    let client = FakeRunClient::default();
    assert!(handle_run_queue(
        RunCommandSession::Connected(client.clone()),
        "run-1",
        ChatDelivery::Steer,
        "steer",
    )
    .is_ok());
    assert!(handle_run_queue(
        RunCommandSession::Connected(client.clone()),
        "run-2",
        ChatDelivery::FollowUp,
        "follow-up",
    )
    .is_ok());
    assert_eq!(
        client.calls(),
        [
            RunClientCall::Steer("run-1".to_string(), "steer".to_string()),
            RunClientCall::FollowUp("run-2".to_string(), "follow-up".to_string())
        ]
    );
    assert_runtime_update_pending(handle_run_queue(
        RunCommandSession::Connected(FakeRunClient::with_runtime_upgrade_pending()),
        "run-1",
        ChatDelivery::FollowUp,
        "message",
    ));
    assert_disconnected(handle_run_queue::<FakeRunClient>(
        RunCommandSession::Disconnected,
        "run-1",
        ChatDelivery::FollowUp,
        "message",
    ));
}

#[cfg(any(unix, target_os = "windows"))]
#[test]
fn chat_cancel_routes_all_desktop_client_states() {
    assert_disconnected(handle_run_cancel::<FakeRunClient>(
        RunCommandSession::NoSupervisor,
        "run-1",
    ));
    let client = FakeRunClient::default();
    assert!(handle_run_cancel(RunCommandSession::Connected(client.clone()), "run-1").is_ok());
    assert_eq!(client.calls(), [RunClientCall::Cancel("run-1".to_string())]);
    assert_disconnected(handle_run_cancel::<FakeRunClient>(
        RunCommandSession::Disconnected,
        "run-1",
    ));
}

#[cfg(any(unix, target_os = "windows"))]
#[test]
fn chat_answer_permission_routes_all_desktop_client_states() {
    let answer = AttachChatPermissionAnswer::Confirm(true);
    assert_disconnected(handle_run_permission_answer::<FakeRunClient>(
        RunCommandSession::NoSupervisor,
        "run-1",
        "gate-1",
        answer.clone(),
    ));
    let client = FakeRunClient::default();
    assert!(handle_run_permission_answer(
        RunCommandSession::Connected(client.clone()),
        "run-1",
        "gate-1",
        answer.clone(),
    )
    .is_ok());
    assert_eq!(
        client.calls(),
        [RunClientCall::PermissionAnswer(
            "run-1".to_string(),
            "gate-1".to_string(),
            answer.clone()
        )]
    );
    assert_disconnected(handle_run_permission_answer::<FakeRunClient>(
        RunCommandSession::Disconnected,
        "run-1",
        "gate-1",
        answer,
    ));
}

#[test]
fn permission_answers_are_closed_and_cover_each_dialog() {
    for value in [
        json!({"type": "select", "value": "A"}),
        json!({"type": "confirm", "value": true}),
        json!({"type": "input", "value": "text"}),
        json!({"type": "editor", "value": "draft"}),
        json!({"type": "cancelled"}),
    ] {
        assert!(serde_json::from_value::<ChatPermissionAnswer>(value).is_ok());
    }
    for value in [
        json!({"type": "select", "value": 1}),
        json!({"type": "unknown", "value": "A"}),
        json!({"type": "cancelled", "extra": true}),
    ] {
        assert!(serde_json::from_value::<ChatPermissionAnswer>(value).is_err());
    }
}

#[test]
fn file_panel_reads_text_and_rejects_binary_large_missing_and_directory() {
    let temp = std::env::temp_dir().join(format!("muniment-file-panel-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&temp).unwrap();
    let path = temp.as_path().join("preview.rs");
    std::fs::write(&path, "fn main() {}\n").unwrap();
    assert_eq!(tauri::async_runtime::block_on(chat_file_content(path.clone(), None)).unwrap(), "fn main() {}\n");
    std::fs::write(&path, [0, 1, 2]).unwrap();
    assert!(tauri::async_runtime::block_on(chat_file_content(path.clone(), None)).unwrap_err().contains("binary"));
    std::fs::write(&path, vec![b'a'; 512 * 1024 + 1]).unwrap();
    assert!(tauri::async_runtime::block_on(chat_file_content(path, None)).unwrap_err().contains("too large"));
    assert!(tauri::async_runtime::block_on(chat_file_content(temp.as_path().to_owned(), None)).is_err());
    assert!(tauri::async_runtime::block_on(chat_file_content(temp.as_path().join("missing"), None)).is_err());
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
fn composer_file_search_stays_in_home_and_skips_generated_and_hidden_files() {
    let root = std::env::temp_dir().join(format!("muniment-mention-{}", Uuid::now_v7()));
    for directory in ["src", ".private", "node_modules"] { std::fs::create_dir_all(root.join(directory)).unwrap(); }
    for file in ["src/ISSUES.md", ".private/ISSUES.md", "node_modules/ISSUES.md"] { std::fs::write(root.join(file), "content").unwrap(); }
    let files = super::run_preparation::search_home_files(&root, "issues");
    assert_eq!(files.len(), 1);
    let value = serde_json::to_value(&files).unwrap();
    assert_eq!(value[0]["displayName"], "ISSUES.md");
    std::fs::remove_dir_all(root).unwrap();
}
