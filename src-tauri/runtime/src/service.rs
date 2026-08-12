//! Dormant runtime service composition.

use muniment_core::attach::RuntimeActivityRegistry;
use muniment_core::auth::TokenSet;
use muniment_core::auth::{
    api_base_url, ensure_native_session as ensure_core_native_session, FreshNativeSession,
    FreshNativeSessionError, KeyringNativeCredentialStore,
};
use muniment_core::chat_coordinate::coordinate;
use muniment_core::chat_grant::{fetch_grant, validate_grant, ChatGrant, FetchGrantError};
use muniment_core::chat_profile::{ChatProfile, ChatProfileError};
use muniment_core::chat_resume::{
    clear_active_run, install_active_run, install_resume_run, resumable_context,
    run_resume as drive_resume, ResumeLaunch,
};
use muniment_core::journal::reconciliation::reconcile_interrupted_runs;
use muniment_core::journal::thread_mutation::{append_thread_delete_now, append_thread_rename_now};
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
use std::time::{SystemTime, UNIX_EPOCH};

use crate::RuntimeChatEventSink;

/// Returns a fresh native session from the platform credential store.
pub fn ensure_native_session() -> Result<FreshNativeSession, FreshNativeSessionError> {
    let now_unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    ensure_core_native_session(
        &KeyringNativeCredentialStore::new(),
        &api_base_url(),
        now_unix_seconds,
    )
}

/// Fetches and validates a cloud chat grant.
pub fn fetch_chat_grant(access_token: &str) -> Result<ChatGrant, FetchGrantError> {
    let grant = fetch_grant(&api_base_url(), access_token)?;
    validate_grant(&grant)?;
    Ok(grant)
}

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
    storage: SharedStorage,
    subject: Option<String>,
    limit: usize,
    cursor: Option<String>,
) -> Result<ThreadSummaryPage, String> {
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

/// Renames one thread owned by one subject.
pub fn rename_thread(
    storage: SharedStorage,
    subject: Option<String>,
    thread_id: String,
    title: String,
) -> Result<(), String> {
    let mut storage = storage
        .lock()
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    append_thread_rename_now(
        &mut storage.journal,
        subject.as_deref(),
        &thread_id,
        &title,
        &runtime_provenance(),
    )
    .map_err(|_| "Conversation history is unavailable.".to_string())
}

/// Deletes one thread owned by one subject.
pub fn delete_thread(
    storage: SharedStorage,
    subject: Option<String>,
    thread_id: String,
) -> Result<(), String> {
    let mut storage = storage
        .lock()
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    append_thread_delete_now(
        &mut storage.journal,
        subject.as_deref(),
        &thread_id,
        &runtime_provenance(),
    )
    .map_err(|_| "Conversation history is unavailable.".to_string())
}

/// Opens one thread owned by one subject.
pub fn thread_page(
    profile_directory: impl AsRef<Path>,
    storage: SharedStorage,
    subject: Option<String>,
    thread_id: String,
    limit: usize,
    cursor: Option<String>,
) -> Result<ChatThreadOpenPage, String> {
    let profile_directory = profile_directory.as_ref();
    let profile = ChatProfile::new(profile_directory);
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
    storage: SharedStorage,
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
    storage: SharedStorage,
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
