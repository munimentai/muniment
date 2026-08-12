use std::fs;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use muniment_core::active_run::{ChatDelivery, ChatQueueRequest};
use muniment_core::chat_grant::ChatGrant;
use muniment_core::home::confirm_home;
use muniment_runtime::{open_profile_storage, queue_run_message, run_prompt};

mod common;
use common::stage_pi_stub;

static ENVIRONMENT: Mutex<()> = Mutex::new(());

fn fixture_grant() -> ChatGrant {
    ChatGrant {
        workspace: "workspace-a".into(),
        gateway_url: "https://gateway.example.com".into(),
        virtual_key: "virtual-key".into(),
        model: None,
        minimum_cacheable_prefix_characters: 8_192,
        receipt_url: "https://receipts.example.com".into(),
    }
}

#[test]
fn a_queued_steer_reaches_a_live_runtime_run() {
    let _environment = ENVIRONMENT.lock().unwrap();
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    let temporary_root =
        std::env::temp_dir().join(format!("muniment-runtime-steer-{}", std::process::id()));
    let _ = fs::remove_dir_all(&temporary_root);
    let profile = temporary_root.join("profile");
    let config = temporary_root.join("config");
    fs::create_dir_all(&profile).unwrap();
    confirm_home(&config, &temporary_root.join("home")).unwrap();
    let descriptor = stage_pi_stub(&temporary_root);
    let steer_capture = temporary_root.join("steer.json");
    std::env::set_var("PI_RESUME_STUB_STEER_CAPTURE", &steer_capture);

    let run_id = "018f0000-0000-7000-8000-000000000112";
    let storage = open_profile_storage(&profile).unwrap();
    let runtime = Arc::new(Mutex::new(None));
    let active = Arc::new(Mutex::new(None));
    let (subscriber, events) = mpsc::channel();
    std::thread::scope(|scope| {
        let run = scope.spawn(|| {
            run_prompt(
                &profile,
                Arc::clone(&storage),
                Arc::clone(&runtime),
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
                Arc::clone(&active),
                Some(subscriber),
                Some(descriptor),
            )
        });
        assert_eq!(
            events.recv_timeout(Duration::from_secs(5)).unwrap().phase,
            "thinking"
        );
        queue_run_message(
            Arc::clone(&active),
            ChatQueueRequest {
                run_id: run_id.into(),
                delivery: ChatDelivery::Steer,
                message: "redirect here".into(),
            },
        )
        .unwrap();
        let delivered = events
            .iter()
            .take_while(|event| {
                !matches!(event.phase.as_str(), "complete" | "failed" | "cancelled")
            })
            .any(|event| event.text.contains("steered"));
        assert!(delivered);
        run.join().unwrap().unwrap();
    });

    let captured = fs::read_to_string(steer_capture).unwrap();
    assert!(captured.contains(r#""type":"steer""#));
    assert!(captured.contains(r#""message":"redirect here""#));
    let inactive = |id: &str| ChatQueueRequest {
        run_id: id.into(),
        delivery: ChatDelivery::Steer,
        message: "too late".into(),
    };
    for id in [run_id, "018f0000-0000-7000-8000-000000000999"] {
        assert_eq!(
            queue_run_message(Arc::clone(&active), inactive(id)),
            Err("That reply is no longer active.".into())
        );
    }

    drop(storage);
    let storage = open_profile_storage(&profile).unwrap();
    let journal_events = storage.lock().unwrap().journal.events(run_id).unwrap();
    assert!(matches!(
        journal_events.last().map(|event| event.event_type.as_str()),
        Some("run.completed" | "run.failed" | "run.cancelled")
    ));

    drop(journal_events);
    drop(storage);
    std::env::remove_var("PI_RESUME_STUB_STEER_CAPTURE");
    std::env::remove_var("MUNIMENT_PI_ROOT");
    fs::remove_dir_all(temporary_root).unwrap();
}
