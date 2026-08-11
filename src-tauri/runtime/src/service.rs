//! Dormant runtime service composition.

use muniment_core::attach::RuntimeActivityRegistry;
use muniment_core::chat_coordinate::coordinate;
use muniment_core::chat_grant::ChatGrant;
use muniment_core::chat_profile::{ChatProfile, ChatProfileError};
use muniment_core::journal::reconciliation::reconcile_interrupted_runs;
use muniment_core::journal::Provenance;
use muniment_core::memory_runtime::ApplicationMemoryRuntime;
use muniment_core::run_events::{ChatEvent, ChatStorage, SharedStorage};
use muniment_core::run_preparation::{prepare_new_run_with_session_thread, SessionThreadStart};
use muniment_core::session_thread::SessionThread;
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
pub fn run_prompt(
    profile_directory: impl AsRef<Path>,
    run_id: String,
    prompt: String,
    access_token: String,
    subject: Option<String>,
    grant: ChatGrant,
    subscriber: Option<Sender<ChatEvent>>,
) -> Result<(), String> {
    run_prompt_with_sink(
        profile_directory,
        run_id,
        prompt,
        access_token,
        subject,
        grant,
        subscriber,
        None,
    )
}

#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn run_prompt_with_pi_executable(
    profile_directory: impl AsRef<Path>,
    run_id: String,
    prompt: String,
    access_token: String,
    subject: Option<String>,
    grant: ChatGrant,
    subscriber: Option<Sender<ChatEvent>>,
    executable: std::path::PathBuf,
) -> Result<(), String> {
    run_prompt_with_sink(
        profile_directory,
        run_id,
        prompt,
        access_token,
        subject,
        grant,
        subscriber,
        Some(executable),
    )
}

#[allow(clippy::too_many_arguments)]
fn run_prompt_with_sink(
    profile_directory: impl AsRef<Path>,
    run_id: String,
    prompt: String,
    access_token: String,
    subject: Option<String>,
    grant: ChatGrant,
    subscriber: Option<Sender<ChatEvent>>,
    executable: Option<std::path::PathBuf>,
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
    let mut sink = RuntimeChatEventSink::new(profile_directory, subscriber);
    if let Some(executable) = executable {
        sink = sink.with_pi_executable(executable);
    }
    coordinate(
        sink,
        storage,
        Arc::new(Mutex::new(None)),
        RuntimeActivityRegistry::new(),
        memory_runtime,
        run_id,
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
    Ok(())
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
