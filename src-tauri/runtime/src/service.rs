//! Dormant runtime service composition.

use muniment_core::attach::RuntimeActivityRegistry;
use muniment_core::auth::TokenSet;
use muniment_core::chat_coordinate::coordinate;
use muniment_core::chat_grant::ChatGrant;
use muniment_core::chat_profile::{ChatProfile, ChatProfileError};
use muniment_core::chat_resume::{
    install_resume_run, resumable_context, run_resume as drive_resume, ResumeLaunch,
};
use muniment_core::journal::reconciliation::reconcile_interrupted_runs;
use muniment_core::journal::Provenance;
use muniment_core::memory_index::ModelMemoryCapability;
use muniment_core::memory_runtime::ApplicationMemoryRuntime;
use muniment_core::pi_execution::PiRuntime;
use muniment_core::run_events::{ChatEvent, ChatStorage, SharedStorage};
use muniment_core::run_preparation::{prepare_new_run_with_session_thread, SessionThreadStart};
use muniment_core::run_start::ActiveRun;
use muniment_core::session_thread::SessionThread;
use muniment_core::sidecar::pi_install::{PiArtifactDescriptor, PI_ARTIFACT};
use std::collections::{BTreeMap, VecDeque};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use crate::RuntimeChatEventSink;

/// Opens profile storage after the ADR 0009 instance-lock cutover gate transfers ownership.
pub fn open_profile_storage(
    profile_directory: impl AsRef<Path>,
) -> Result<SharedStorage, ChatProfileError> {
    let profile = ChatProfile::new(profile_directory.as_ref());
    let (mut journal, cas) = profile.open_storage()?;
    reconcile_interrupted_runs(&mut journal, &runtime_provenance());
    Ok(Arc::new(Mutex::new(ChatStorage { journal, cas })))
}

/// Runs one prompt to completion through the dormant runtime service boundaries.
#[allow(clippy::too_many_arguments)]
pub fn run_prompt(
    profile_directory: impl AsRef<Path>,
    run_id: String,
    prompt: String,
    access_token: String,
    subject: Option<String>,
    grant: ChatGrant,
    subscriber: Option<Sender<ChatEvent>>,
    pi_artifact: Option<PiArtifactDescriptor>,
) -> Result<(), String> {
    let profile_directory = profile_directory.as_ref();
    let storage = open_profile_storage(profile_directory).map_err(|error| error.to_string())?;
    let session_thread = SessionThread::default();
    let prepared = prepare_new_run_with_session_thread(
        &storage,
        SessionThreadStart {
            tracker: &session_thread,
            continue_existing: false,
        },
        &run_id,
        &grant.workspace,
        subject.as_deref(),
        Vec::new(),
        Some(runtime_provenance()),
        "muniment-runtime",
        env!("CARGO_PKG_VERSION"),
        || Ok(()),
    )?;
    let memory_runtime = Arc::new(ApplicationMemoryRuntime::new(
        profile_directory.to_path_buf(),
        profile_directory.join("memory"),
    ));
    let thread_id = storage
        .lock()
        .map_err(|_| "Conversation history is unavailable.".to_string())?
        .journal
        .run_thread_id(&run_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Conversation history is unavailable.".to_string())?;
    memory_runtime
        .open_session(
            &run_id,
            &thread_id,
            ModelMemoryCapability {
                minimum_cacheable_prefix_characters: grant.minimum_cacheable_prefix_characters,
            },
        )
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    coordinate(
        RuntimeChatEventSink::new(profile_directory, subscriber, memory_runtime.clone())
            .with_pi_artifact(pi_artifact.unwrap_or(PI_ARTIFACT)),
        storage,
        Arc::new(Mutex::new(None)),
        RuntimeActivityRegistry::new(),
        memory_runtime.clone(),
        run_id.clone(),
        prompt,
        access_token,
        subject,
        grant,
        Arc::new(AtomicBool::new(false)),
        Arc::new(Mutex::new(None)),
        Arc::new(Mutex::new(None)),
        Arc::new(Mutex::new(VecDeque::new())),
        None,
        None,
        Some(prepared),
    );
    memory_runtime.close_session(&run_id);
    Ok(())
}

/// Resumes one interrupted run through the dormant runtime service boundaries.
#[allow(clippy::too_many_arguments)]
pub fn resume_run(
    profile_directory: impl AsRef<Path>,
    run_id: String,
    access_token: String,
    subject: Option<String>,
    grant: ChatGrant,
    subscriber: Option<Sender<ChatEvent>>,
    pi_artifact: Option<PiArtifactDescriptor>,
) -> Result<(), String> {
    let profile_directory = profile_directory.as_ref();
    let profile = ChatProfile::new(profile_directory);
    let storage = open_profile_storage(profile_directory).map_err(|error| error.to_string())?;
    let (resume, thread_id) = {
        let mut storage = storage
            .lock()
            .map_err(|_| "This reply cannot be resumed.".to_string())?;
        let events = storage
            .journal
            .events(&run_id)
            .map_err(|_| "This reply cannot be resumed.".to_string())?;
        let thread_id = storage
            .journal
            .run_thread_id(&run_id)
            .map_err(|_| "This reply cannot be resumed.".to_string())?
            .ok_or_else(|| "This reply cannot be resumed.".to_string())?;
        let resume = resumable_context(&events, subject.as_deref(), &profile.pi_session_root())
            .map_err(|_| "This reply cannot be resumed.".to_string())?;
        (resume, thread_id)
    };
    let memory_runtime = Arc::new(ApplicationMemoryRuntime::new(
        profile_directory.to_path_buf(),
        profile_directory.join("memory"),
    ));
    let runtime_activity = RuntimeActivityRegistry::new();
    let active = Arc::new(Mutex::new(None));
    let cancelled = Arc::new(AtomicBool::new(false));
    let transport = Arc::new(Mutex::new(None));
    let adapter = Arc::new(Mutex::new(None));
    let permission_answers = Arc::new(Mutex::new(VecDeque::new()));
    install_resume_run(
        &memory_runtime,
        &active,
        ActiveRun {
            id: run_id.clone(),
            workspace: grant.workspace.clone(),
            cancelled: Arc::clone(&cancelled),
            transport: Arc::clone(&transport),
            adapter: Arc::clone(&adapter),
            permission_answers: Arc::clone(&permission_answers),
            _activity: runtime_activity.mark_active_run(),
        },
        &thread_id,
        grant.minimum_cacheable_prefix_characters,
    )?;
    let (attempt, result) = std::sync::mpsc::channel();
    drive_resume(ResumeLaunch {
        sink: RuntimeChatEventSink::new(profile_directory, subscriber, memory_runtime.clone())
            .with_pi_artifact(pi_artifact.unwrap_or(PI_ARTIFACT)),
        storage,
        runtime: Arc::new(Mutex::new(None::<PiRuntime>)),
        runtime_activity,
        memory_runtime,
        active,
        run_id,
        tokens: TokenSet {
            access_token,
            refresh_token: None,
            expires_at: None,
            subject,
        },
        grant,
        cancelled,
        transport,
        adapter,
        permission_answers,
        resume,
        attempt,
    });
    result
        .recv()
        .map_err(|_| "This reply could not be resumed. Try again.".to_string())?
}

fn runtime_provenance() -> Provenance {
    Provenance {
        source: "muniment-runtime".into(),
        source_version: env!("CARGO_PKG_VERSION").into(),
        actor_id: None,
        device_id: None,
        rpc_request_id: None,
        capability_versions: None,
        extra: BTreeMap::new(),
    }
}
