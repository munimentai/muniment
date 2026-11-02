use std::fs;
use std::sync::{mpsc, Arc, Mutex};

use muniment_core::active_run::ChatDelivery;
use muniment_core::attach::{RuntimeActivityRegistry, SignedWorkspaceApproval};
use muniment_core::memory_runtime::ApplicationMemoryRuntime;
use muniment_core::run_start::RunAttachBoundaries;
use muniment_core::serde_json;
use muniment_core::session_thread::SessionThread;
use muniment_runtime_core::{
    open_companion_registry, open_profile_storage, run_prompt, RuntimeAttachBoundaries,
    RuntimeChatEventTarget, RuntimeHost,
};

mod common;
use common::CI_WAIT;
use common::{fixture_grant, stage_pi_stub, TemporaryProfile};

static ENVIRONMENT: Mutex<()> = Mutex::new(());

fn queued_message_reaches_a_live_runtime_run(
    test_name: &str,
    run_id: &str,
    delivery: ChatDelivery,
    message: &str,
    command: &str,
) {
    queued_message_answers_after(test_name, run_id, delivery, message, command, None);
}

/// Sends `message` into a live reply that wrote `held_text`, when given,
/// before the message reached it, and answers the reply's text.
fn queued_message_answers_after(
    test_name: &str,
    run_id: &str,
    delivery: ChatDelivery,
    message: &str,
    command: &str,
    held_text: Option<&str>,
) -> String {
    let _environment = ENVIRONMENT.lock().unwrap();
    common::use_test_prompt_store();
    let temporary_profile = TemporaryProfile::new(test_name, true);
    let temporary_root = temporary_profile.root.clone();
    let profile = temporary_profile.profile.clone();
    let config = temporary_profile.config.clone();
    let descriptor = stage_pi_stub(&temporary_root);
    let steer_capture = temporary_root.join("steer.json");
    std::env::set_var("PI_RESUME_STUB_STEER_CAPTURE", &steer_capture);
    match held_text {
        Some(text) => std::env::set_var("PI_RESUME_STUB_HELD_TEXT", text),
        None => std::env::remove_var("PI_RESUME_STUB_HELD_TEXT"),
    }

    let storage = open_profile_storage(&profile).unwrap();
    let runtime = muniment_core::pi_execution::PiProcesses::default();
    let runtime_activity = RuntimeActivityRegistry::new();
    let active = muniment_core::active_run::RunRegistry::default();
    let boundaries = RuntimeAttachBoundaries::new(
        Arc::clone(&storage),
        active.clone(),
        profile.clone(),
        config.clone(),
        runtime.clone(),
        Arc::new(ApplicationMemoryRuntime::new(
            config.clone(),
            profile.join("memory"),
        )),
        runtime_activity.clone(),
        RuntimeHost::default(),
        SignedWorkspaceApproval::default(),
        Arc::new(SessionThread::default()),
        open_companion_registry(&profile).unwrap(),
    );
    let (subscriber, events) = mpsc::channel();
    let mut live_text = String::new();
    std::thread::scope(|scope| {
        let run = scope.spawn(|| {
            run_prompt(
                &RuntimeHost::default(),
                &profile,
                Arc::clone(&storage),
                runtime.clone(),
                &runtime_activity,
                &config,
                run_id.into(),
                "initial prompt".into(),
                None,
                &muniment_core::session_thread::SessionThread::default(),
                false,
                "token".into(),
                Some("owner".into()),
                Vec::new(),
                fixture_grant(),
                active.clone(),
                RuntimeChatEventTarget::Subscriber(Some(subscriber)),
                Some(descriptor),
            )
        });
        assert_eq!(events.recv_timeout(CI_WAIT).unwrap().phase, "thinking");
        boundaries
            .queue_attach_message("workspace-a", run_id, delivery, message)
            .unwrap();
        let mut delivered = false;
        for event in events.iter() {
            delivered |= event.text.contains(command);
            if !event.text.is_empty() {
                live_text.clone_from(&event.text);
            }
            if matches!(event.phase.as_str(), "complete" | "failed" | "cancelled") {
                break;
            }
        }
        assert!(delivered);
        run.join().unwrap().unwrap();
    });

    let captured = fs::read_to_string(steer_capture).unwrap();
    assert!(captured.contains(&format!(r#""type":"{command}""#)));
    // The queued text arrives behind the line that says when it was sent.
    assert!(captured.contains(r#""message":"[sent 20"#), "{captured}");
    assert!(captured.contains(&format!(r#"\n{message}""#)), "{captured}");
    for id in [run_id, "018f0000-0000-7000-8000-000000000999"] {
        assert!(boundaries
            .queue_attach_message("workspace-a", id, ChatDelivery::Steer, "too late")
            .is_err());
    }

    drop(storage);
    let storage = open_profile_storage(&profile).unwrap();
    let journal_events = storage.lock().unwrap().journal.events(run_id).unwrap();
    assert!(matches!(
        journal_events.last().map(|event| event.event_type.as_str()),
        Some("run.completed" | "run.failed" | "run.cancelled")
    ));
    // The journal keeps the message as the user wrote it, and the history
    // read of the reopened profile places it inside the reply.
    let steered: Vec<_> = journal_events
        .iter()
        .filter(|event| event.event_type == "run.steered")
        .collect();
    assert_eq!(steered.len(), 1, "{journal_events:?}");
    let thread_id = {
        let storage = storage.lock().unwrap();
        storage.journal.run_thread_id(run_id).unwrap().unwrap()
    };
    let page = {
        let mut storage = storage.lock().unwrap();
        muniment_core::thread_history::chat_thread_open_page(
            &mut storage.journal,
            None,
            Some("owner"),
            &temporary_root,
            &thread_id,
            10,
            None,
        )
        .unwrap()
    };
    let entry = page
        .entries
        .iter()
        .find(|entry| entry.run_id == run_id)
        .unwrap();
    assert_eq!(entry.steers.len(), 1);
    let steer = &entry.steers[0];
    assert_eq!(steer.text, message);
    assert_eq!(steer.sent_at, steered[0].recorded_at);
    assert!(steer.text_offset <= entry.text.encode_utf16().count());
    let wire = serde_json::to_value(entry).unwrap();
    assert_eq!(wire["steers"][0]["text"], message);
    assert_eq!(wire["steers"][0]["textOffset"], steer.text_offset);
    assert_eq!(wire["steers"][0]["sentAt"], steer.sent_at.as_str());

    // The reply streams the text its history keeps.
    assert_eq!(live_text, entry.text);
    let text = entry.text.clone();
    drop(journal_events);
    drop(storage);
    std::env::remove_var("PI_RESUME_STUB_STEER_CAPTURE");
    std::env::remove_var("PI_RESUME_STUB_HELD_TEXT");
    std::env::remove_var("MUNIMENT_PI_ROOT");
    text
}

#[test]
fn a_queued_steer_reaches_a_live_runtime_run() {
    queued_message_reaches_a_live_runtime_run(
        "steer",
        "018f0000-0000-7000-8000-000000000112",
        ChatDelivery::Steer,
        "redirect here",
        "steer",
    );
}

#[test]
fn a_queued_follow_up_reaches_a_live_runtime_run() {
    queued_message_reaches_a_live_runtime_run(
        "follow-up",
        "018f0000-0000-7000-8000-000000000114",
        ChatDelivery::FollowUp,
        "then do this",
        "follow_up",
    );
}

#[test]
fn the_answer_to_a_steer_starts_a_paragraph_after_the_text_before_it() {
    let text = queued_message_answers_after(
        "steer-paragraph",
        "018f0000-0000-7000-8000-000000000116",
        ChatDelivery::Steer,
        "count on",
        "steer",
        Some("one hundred"),
    );
    assert_eq!(text, "one hundred\n\nsteer");
}
