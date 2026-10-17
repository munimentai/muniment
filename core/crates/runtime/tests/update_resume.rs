#![cfg(unix)]
//! A run an update interrupted resumes from its Pi session after the
//! restart, with a notice in its thread, while a run at a permission gate
//! stays as it was.

use std::fs;
use std::time::{Duration, Instant};

use muniment_core::pi_execution::coordinate_prepared_prompt;
use muniment_core::run_events::{ChatEvent, ChatEventSink};
use muniment_core::run_preparation::{prepare_new_run_with_session_thread, SessionThreadStart};
use muniment_core::session_thread::SessionThread;
use muniment_core::sidecar::validate_pi_session;
use muniment_runtime_core::update::{
    read_resume_ledger, write_resume_ledger, InterruptedRun, ResumeLedger, UPDATE_RESUME_NOTICE,
};
use muniment_runtime_core::{open_profile_storage, RuntimeAttachState, RuntimeHost};

mod common;
use common::{stage_pi_stub, TemporaryProfile};

struct FixtureSink;

impl ChatEventSink for FixtureSink {
    fn provenance(&self) -> (&str, &str) {
        ("test", "1")
    }

    fn deliver(&self, _event: ChatEvent) -> Result<(), ()> {
        Ok(())
    }
}

/// Starts a run in the local workspace whose Pi session is bound, as a run
/// the runtime stopped mid-reply leaves it, and answers its thread id.
fn interrupted_run(profile: &std::path::Path, run_id: &str) -> String {
    let session_root = muniment_core::chat_profile::ChatProfile::new(profile).pi_session_root();
    fs::create_dir_all(&session_root).unwrap();
    let session = format!("{run_id}.jsonl");
    fs::write(session_root.join(&session), b"{}\n").unwrap();
    let storage = open_profile_storage(profile).unwrap();
    let (mut sequence, mut projector) = prepare_new_run_with_session_thread(
        &storage,
        SessionThreadStart {
            tracker: &SessionThread::default(),
            continue_existing: false,
        },
        run_id,
        "local",
        None,
        Vec::new(),
        None,
        "test",
        "1",
        || Ok(()),
    )
    .unwrap();
    coordinate_prepared_prompt(
        &FixtureSink,
        &storage,
        &mut projector,
        run_id,
        &mut sequence,
        None,
        |_| {
            let (locator, _) = validate_pi_session(&session_root, &session)
                .map_err(|_| muniment_core::pi_execution::PreparedPromptError::SessionRoot)?;
            Ok(((), locator, Vec::new()))
        },
    )
    .unwrap();
    let thread_id = storage
        .lock()
        .unwrap()
        .journal
        .run_thread_id(run_id)
        .unwrap()
        .unwrap();
    thread_id
}

/// The tests change process-wide stub settings, so they run one at a time.
static ENVIRONMENT: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn an_interrupted_run_resumes_after_an_update_with_a_notice() {
    let _environment = ENVIRONMENT
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    common::use_test_prompt_store();
    let temporary = TemporaryProfile::new("update-resume", false);
    let profile = temporary.profile.clone();
    fs::write(
        profile.join(muniment_core::local_mode::LOCAL_MODE_MARKER),
        "1",
    )
    .unwrap();
    let artifact = stage_pi_stub(&profile);
    let resumed_run = "018f0000-0000-7000-8000-000000000201";
    let gated_run = "018f0000-0000-7000-8000-000000000202";
    let resumed_thread = interrupted_run(&profile, resumed_run);
    let gated_thread = interrupted_run(&profile, gated_run);
    write_resume_ledger(
        &profile,
        &ResumeLedger {
            interrupted: vec![
                InterruptedRun {
                    run_id: resumed_run.into(),
                    thread_id: resumed_thread.clone(),
                    workspace: "local".into(),
                    permission_gate: false,
                },
                InterruptedRun {
                    run_id: gated_run.into(),
                    thread_id: gated_thread.clone(),
                    workspace: "local".into(),
                    permission_gate: true,
                },
            ],
            ..ResumeLedger::default()
        },
    )
    .unwrap();

    // The restart reconciles both runs as interrupted, then resumes one.
    let state = RuntimeAttachState::open(RuntimeHost::default(), &profile, &profile).unwrap();
    state.resume_interrupted_runs_with(state.boundaries().with_pi_artifact(artifact));

    let ledger = read_resume_ledger(&profile);
    assert!(ledger.interrupted.is_empty());
    assert_eq!(ledger.resumed[&resumed_thread].count, 1);
    assert!(!ledger.resumed.contains_key(&gated_thread));

    let storage = open_profile_storage(&profile).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let events = loop {
        let events = storage.lock().unwrap().journal.events(resumed_run).unwrap();
        let finished = events.iter().any(|event| {
            matches!(
                event.event_type.as_str(),
                "run.completed" | "run.failed" | "run.cancelled"
            )
        });
        if finished || Instant::now() > deadline {
            break events;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let resumed = events
        .iter()
        .find(|event| event.event_type == "run.resumed")
        .expect("the run resumed");
    let muniment_core::journal::EventPayload::Inline { payload_json } = &resumed.payload else {
        panic!("run.resumed carries an inline payload");
    };
    assert_eq!(payload_json["notice"], UPDATE_RESUME_NOTICE);
    let projection = muniment_core::journal::reducer::project_chat(&events).unwrap();
    assert_eq!(
        projection.resume_notice.as_deref(),
        Some(UPDATE_RESUME_NOTICE)
    );

    // The gated run stays interrupted for its owner.
    let gated = storage.lock().unwrap().journal.events(gated_run).unwrap();
    assert_eq!(gated.last().unwrap().event_type, "run.needs_attention");
    drop(storage);
    drop(state);
    std::env::remove_var("MUNIMENT_PI_ROOT");
}

#[test]
fn a_drain_records_every_live_run_and_the_restart_resumes_each_one() {
    use muniment_core::run_start::RunAttachBoundaries;
    let _environment = ENVIRONMENT
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    common::use_test_prompt_store();
    let temporary = TemporaryProfile::new("update-resume-many", false);
    let profile = temporary.profile.clone();
    fs::write(
        profile.join(muniment_core::local_mode::LOCAL_MODE_MARKER),
        "1",
    )
    .unwrap();
    let artifact = stage_pi_stub(&profile);

    // Three replies in three threads are live when the update drains.
    std::env::set_var(
        "PI_RESUME_STUB_STEER_CAPTURE",
        temporary.root.join("steer.json"),
    );
    let state = std::sync::Arc::new(
        RuntimeAttachState::open(RuntimeHost::default(), &profile, &profile).unwrap(),
    );
    let boundaries = state.boundaries().with_pi_artifact(artifact);
    let live: Vec<String> = (0..3)
        .map(|index| {
            boundaries
                .submit_run(
                    "local",
                    format!("Hold reply {index}."),
                    Vec::new(),
                    None,
                    None,
                )
                .unwrap()
                .run_id
        })
        .collect();
    let registry = state.run_registry();
    assert_eq!(registry.len(), 3);
    // A fourth reply waits for a free place, and a thread replies once at a time.
    let refused = boundaries
        .submit_run("local", "One more.".into(), Vec::new(), None, None)
        .unwrap_err()
        .into_message();
    assert!(refused.contains("already running 3 replies"), "{refused}");
    let busy_thread = registry.identities()[0].thread_id.clone().unwrap();
    let refused = boundaries
        .submit_run(
            "local",
            "Again.".into(),
            Vec::new(),
            Some(busy_thread),
            None,
        )
        .unwrap_err()
        .into_message();
    assert_eq!(refused, muniment_core::active_run::THREAD_BUSY);
    let outcome = state.drain_for_update(Duration::from_millis(200));
    assert!(matches!(
        outcome,
        muniment_runtime_core::update::DrainOutcome::TimedOut { .. }
    ));
    let ledger = read_resume_ledger(&profile);
    let mut recorded: Vec<&str> = ledger
        .interrupted
        .iter()
        .map(|run| run.run_id.as_str())
        .collect();
    recorded.sort();
    let mut expected: Vec<&str> = live.iter().map(String::as_str).collect();
    expected.sort();
    assert_eq!(recorded, expected);
    let threads: std::collections::BTreeSet<_> = ledger
        .interrupted
        .iter()
        .map(|run| &run.thread_id)
        .collect();
    assert_eq!(threads.len(), 3);
    assert!(ledger.interrupted.iter().all(|run| !run.permission_gate));
    for run in &live {
        muniment_core::active_run::cancel_active_run(&registry, run, None).unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    while !registry.is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(registry.is_empty());
    drop(boundaries);
    drop(state);
    std::env::remove_var("PI_RESUME_STUB_STEER_CAPTURE");

    // After the restart, every interrupted run resumes, each in its thread.
    let runs = [
        "018f0000-0000-7000-8000-000000000301",
        "018f0000-0000-7000-8000-000000000302",
        "018f0000-0000-7000-8000-000000000303",
    ];
    let interrupted: Vec<InterruptedRun> = runs
        .iter()
        .map(|run_id| InterruptedRun {
            run_id: (*run_id).into(),
            thread_id: interrupted_run(&profile, run_id),
            workspace: "local".into(),
            permission_gate: false,
        })
        .collect();
    write_resume_ledger(
        &profile,
        &ResumeLedger {
            interrupted,
            ..ResumeLedger::default()
        },
    )
    .unwrap();
    let state = RuntimeAttachState::open(RuntimeHost::default(), &profile, &profile).unwrap();
    state.resume_interrupted_runs_with(state.boundaries().with_pi_artifact(artifact));
    let storage = open_profile_storage(&profile).unwrap();
    for run_id in runs {
        let events = storage.lock().unwrap().journal.events(run_id).unwrap();
        assert!(
            events.iter().any(|event| event.event_type == "run.resumed"),
            "{run_id} resumed"
        );
        assert_eq!(events.last().unwrap().event_type, "run.completed");
    }
    assert!(read_resume_ledger(&profile).interrupted.is_empty());
    drop(storage);
    drop(state);
    std::env::remove_var("MUNIMENT_PI_ROOT");
}
