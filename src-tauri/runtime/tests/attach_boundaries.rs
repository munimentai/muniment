#![cfg(target_os = "linux")]

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use muniment_core::attach::linux::{ThreadListRequest, ThreadOpenRequest};
use muniment_core::attach::{ProtocolError, RuntimeActivityRegistry};
use muniment_core::journal::Provenance;
use muniment_core::memory_runtime::ApplicationMemoryRuntime;
use muniment_core::permission_gate::ChatPermissionAnswer;
use muniment_core::pi_execution::PiRuntime;
use muniment_core::run_preparation::{prepare_new_run_with_session_thread, SessionThreadStart};
use muniment_core::run_start::{ActiveRun, RunAttachBoundaries};
use muniment_core::session_thread::SessionThread;
use muniment_runtime::{open_profile_storage, RuntimeAttachBoundaries};

mod common;
use common::TemporaryProfile;

fn provenance() -> Provenance {
    let mut provenance = Provenance {
        source: "muniment-attach".into(),
        source_version: "test".into(),
        actor_id: None,
        device_id: None,
        rpc_request_id: None,
        capability_versions: None,
        extra: BTreeMap::new(),
    };
    provenance
        .extra
        .insert("attach_profile".into(), "profile-a".into());
    provenance
}

#[test]
fn runtime_boundaries_answer_all_attach_reads() {
    let temporary_profile = TemporaryProfile::new("attach-boundaries", false);
    let profile = temporary_profile.profile.clone();
    let storage = open_profile_storage(&profile).unwrap();
    let runtime_activity = RuntimeActivityRegistry::new();
    let permission_answers = Arc::new(Mutex::new(VecDeque::new()));
    let active = Arc::new(Mutex::new(Some(ActiveRun {
        id: "run-1".into(),
        workspace: "workspace-a".into(),
        cancelled: Arc::new(AtomicBool::new(false)),
        transport: Arc::new(Mutex::new(None)),
        adapter: Arc::new(Mutex::new(None)),
        permission_answers: Arc::clone(&permission_answers),
        _activity: runtime_activity.mark_active_run(),
    })));
    let config = temporary_profile.config.clone();
    let boundaries = RuntimeAttachBoundaries::new(
        Arc::clone(&storage),
        active,
        profile.clone(),
        config.clone(),
        Arc::new(Mutex::new(None::<PiRuntime>)),
        Arc::new(ApplicationMemoryRuntime::new(
            config,
            profile.join("memory"),
        )),
        runtime_activity,
        SessionThread::default(),
    );

    let created_thread = boundaries
        .create_thread("workspace-a", provenance())
        .unwrap();
    let created_events = storage
        .lock()
        .unwrap()
        .journal
        .thread_events(&created_thread)
        .unwrap();
    assert_eq!(created_events[0].provenance.source, "muniment-runtime");
    assert_eq!(
        created_events[0].provenance.source_version,
        env!("CARGO_PKG_VERSION")
    );

    let run_id = "01900000-0000-7000-8000-000000000020";
    let (current_run_seq, _) = prepare_new_run_with_session_thread(
        &storage,
        SessionThreadStart {
            tracker: &SessionThread::default(),
            continue_existing: false,
        },
        run_id,
        "workspace-a",
        None,
        Vec::new(),
        None,
        "test",
        "1",
        || Ok(()),
    )
    .unwrap();
    let run_thread = storage
        .lock()
        .unwrap()
        .journal
        .run_thread_id(run_id)
        .unwrap()
        .unwrap();

    let threads = boundaries
        .list_threads(
            "workspace-a",
            ThreadListRequest {
                limit: 10,
                cursor: None,
            },
        )
        .unwrap();
    assert!(threads
        .threads
        .iter()
        .any(|thread| thread.thread_id == run_thread));

    let opened = boundaries
        .open_thread(
            "workspace-a",
            ThreadOpenRequest {
                thread_id: run_thread.clone(),
                limit: 10,
                cursor: None,
            },
        )
        .unwrap();
    assert_eq!(opened.thread_id, run_thread);

    let stream = boundaries.stream_run("workspace-a", run_id, 0).unwrap();
    assert_eq!(stream.current_run_seq, current_run_seq);

    let subscription = boundaries.subscribe_run_commits(run_id).unwrap();
    assert_eq!(subscription.committed_high_water, current_run_seq);
    assert_eq!(
        boundaries.subscribe_run_commits("unknown-run").unwrap_err(),
        ProtocolError::thread_not_found()
    );

    let _commit = boundaries
        .queue_attach_permission_answer(
            "workspace-a",
            "run-1",
            "gate-1",
            ChatPermissionAnswer::Confirm(true),
        )
        .unwrap();
    let queued = permission_answers.lock().unwrap().pop_front().unwrap();
    assert_eq!(queued.gate_id, "gate-1");

    drop(boundaries);
    drop(storage);
}
