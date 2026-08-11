//! Dormant runtime service composition.

use muniment_core::attach::RuntimeActivityRegistry;
use muniment_core::auth::TokenSet;
use muniment_core::chat_coordinate::coordinate;
use muniment_core::chat_grant::ChatGrant;
use muniment_core::chat_profile::{ChatProfile, ChatProfileError};
use muniment_core::chat_resume::{
    clear_active_run, install_active_run, install_resume_run, resumable_context,
    run_resume as drive_resume, ResumeLaunch,
};
use muniment_core::journal::reconciliation::reconcile_interrupted_runs;
use muniment_core::journal::thread_summaries::ThreadSummaryPage;
use muniment_core::journal::Provenance;
use muniment_core::memory_index::ModelMemoryCapability;
use muniment_core::memory_runtime::ApplicationMemoryRuntime;
use muniment_core::owned_threads::chat_thread_summaries_page;
use muniment_core::pi_execution::PiRuntime;
use muniment_core::run_events::{ChatEvent, ChatStorage, SharedStorage};
use muniment_core::run_preparation::{
    prepare_new_run_in_thread_after_validation, prepare_new_run_with_session_thread,
    SessionThreadStart,
};
use muniment_core::run_start::ActiveRun;
use muniment_core::session_thread::SessionThread;
use muniment_core::sidecar::pi_install::{PiArtifactDescriptor, PI_ARTIFACT};
use muniment_core::thread_history::{chat_thread_open_page, ChatThreadOpenPage};
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

/// Lists the threads owned by one subject.
pub fn thread_summaries(
    profile_directory: impl AsRef<Path>,
    subject: Option<String>,
    limit: usize,
    cursor: Option<String>,
) -> Result<ThreadSummaryPage, String> {
    let storage = open_profile_storage(profile_directory)
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    let mut storage = storage
        .lock()
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    chat_thread_summaries_page(
        &mut storage.journal,
        subject.as_deref(),
        limit,
        cursor.as_deref(),
    )
    .map_err(|_| "Conversation history is unavailable.".to_string())
}

/// Opens one thread owned by one subject.
pub fn thread_page(
    profile_directory: impl AsRef<Path>,
    subject: Option<String>,
    thread_id: String,
    limit: usize,
    cursor: Option<String>,
) -> Result<ChatThreadOpenPage, String> {
    let profile_directory = profile_directory.as_ref();
    let profile = ChatProfile::new(profile_directory);
    let storage = open_profile_storage(profile_directory)
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    let mut storage = storage
        .lock()
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    let ChatStorage { journal, cas } = &mut *storage;
    chat_thread_open_page(
        journal,
        Some(cas),
        subject.as_deref(),
        &profile.pi_session_root(),
        &thread_id,
        limit,
        cursor.as_deref(),
    )
    .map_err(|_| "Conversation history is unavailable.".to_string())
}

/// Runs one prompt to completion through the dormant runtime service boundaries.
#[allow(clippy::too_many_arguments)]
pub fn run_prompt(
    profile_directory: impl AsRef<Path>,
    config_directory: impl AsRef<Path>,
    run_id: String,
    prompt: String,
    thread_id: Option<String>,
    access_token: String,
    subject: Option<String>,
    grant: ChatGrant,
    active: Arc<Mutex<Option<ActiveRun>>>,
    subscriber: Option<Sender<ChatEvent>>,
    pi_artifact: Option<PiArtifactDescriptor>,
) -> Result<(), String> {
    let profile_directory = profile_directory.as_ref();
    let storage = open_profile_storage(profile_directory).map_err(|error| error.to_string())?;
    let memory_runtime = Arc::new(ApplicationMemoryRuntime::new(
        config_directory.as_ref().to_path_buf(),
        profile_directory.join("memory"),
    ));
    let runtime_activity = RuntimeActivityRegistry::new();
    let cancelled = Arc::new(AtomicBool::new(false));
    let transport = Arc::new(Mutex::new(None));
    let adapter = Arc::new(Mutex::new(None));
    let permission_answers = Arc::new(Mutex::new(VecDeque::new()));
    install_active_run(
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
    )?;
    let setup = (|| {
        let prepared = match thread_id.as_deref() {
            Some(thread_id) => prepare_new_run_in_thread_after_validation(
                &storage,
                &run_id,
                &grant.workspace,
                subject.as_deref(),
                Vec::new(),
                Some(runtime_provenance()),
                thread_id,
                "muniment-runtime",
                env!("CARGO_PKG_VERSION"),
                || Ok(()),
            ),
            None => {
                let session_thread = SessionThread::default();
                prepare_new_run_with_session_thread(
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
                )
            }
        }?;
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
        Ok::<_, String>(prepared)
    })();
    let prepared = match setup {
        Ok(prepared) => prepared,
        Err(error) => {
            clear_active_run(&active, &run_id);
            memory_runtime.close_session(&run_id);
            return Err(error);
        }
    };
    coordinate(
        RuntimeChatEventSink::new(profile_directory, subscriber, memory_runtime.clone())
            .with_pi_artifact(pi_artifact.unwrap_or(PI_ARTIFACT)),
        storage,
        Arc::new(Mutex::new(None)),
        runtime_activity,
        memory_runtime.clone(),
        run_id.clone(),
        prompt,
        access_token,
        subject,
        grant,
        cancelled,
        transport,
        adapter,
        permission_answers,
        None,
        None,
        Some(prepared),
    );
    memory_runtime.close_session(&run_id);
    clear_active_run(&active, &run_id);
    Ok(())
}

/// Resumes one interrupted run through the dormant runtime service boundaries.
#[allow(clippy::too_many_arguments)]
pub fn resume_run(
    profile_directory: impl AsRef<Path>,
    config_directory: impl AsRef<Path>,
    run_id: String,
    access_token: String,
    subject: Option<String>,
    grant: ChatGrant,
    active: Arc<Mutex<Option<ActiveRun>>>,
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
        config_directory.as_ref().to_path_buf(),
        profile_directory.join("memory"),
    ));
    let runtime_activity = RuntimeActivityRegistry::new();
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
