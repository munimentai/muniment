use std::collections::{BTreeSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use muniment_core::attach::{RuntimeActivityGuard, RuntimeActivityRegistry};
use muniment_core::chat_grant::{fetch_receipt, ChatGrant};
use muniment_core::chat_profile::ChatProfile;
use muniment_core::journal::pi_translation::{model_stream_delta_payload, tool_journal_entry};
use muniment_core::journal::reducer::ChatProjector;
use muniment_core::journal::split_model_stream_delta;
use muniment_core::memory_failure::{MemoryFailure, MemoryFailureAnswer};
use muniment_core::permission_gate::{
    coordinate_extension_ui_request, coordinate_permission_answer, PendingPermissionAnswer,
};
use muniment_core::run_events::{
    append_emit, append_terminal as core_append_terminal, fail, fail_start,
    fail_with_open_effects as core_fail_with_open_effects, ChatEventSink, SharedStorage,
};
use muniment_core::sidecar::pi_chat::{
    cancel_command, ExtensionUiAnswer, ExtensionUiDialog, ExtensionUiRequest, PiChatEvent,
    PiRunAdapter,
};
use muniment_core::sidecar::pi_install::resolve_current;
use muniment_core::sidecar::{
    pi_sidecar_config, PiRpcTransport, PiRpcWiring, SidecarStatus, SidecarSupervisor,
};
use serde_json::{json, Value};
use tauri::Manager;

use crate::chat::{
    coordinate_prepared_prompt, prepared_pi_prompt, ChatState, PiRuntime, PreparedPromptError,
    ResumeAttempt, ResumeContext, TauriChatEventSink, RPC_TIMEOUT,
};

/// Pairs one coordinate-loop state value with the runtime activity mark that
/// follows it. Every mutation runs through `with`, so the mark cannot drift
/// from the value. The mark is a drop guard, so a break out of the loop, an
/// early return, or a panic clears it.
struct MarkedState<T> {
    value: T,
    registry: Option<RuntimeActivityRegistry>,
    mark: Option<RuntimeActivityGuard>,
    marked: fn(&T) -> bool,
    start_mark: fn(&RuntimeActivityRegistry) -> RuntimeActivityGuard,
}

type MarkedEffects = MarkedState<BTreeSet<String>>;
type MarkedGate = MarkedState<Option<ExtensionUiRequest>>;

impl<T> MarkedState<T> {
    fn new(
        value: T,
        registry: Option<RuntimeActivityRegistry>,
        marked: fn(&T) -> bool,
        start_mark: fn(&RuntimeActivityRegistry) -> RuntimeActivityGuard,
    ) -> Self {
        let mut state = Self {
            value,
            registry,
            mark: None,
            marked,
            start_mark,
        };
        state.follow_value();
        state
    }

    fn with<O>(&mut self, step: impl FnOnce(&mut T) -> O) -> O {
        let outcome = step(&mut self.value);
        self.follow_value();
        outcome
    }

    fn follow_value(&mut self) {
        if !(self.marked)(&self.value) {
            self.mark = None;
        } else if self.mark.is_none() {
            self.mark = self.registry.as_ref().map(self.start_mark);
        }
    }
}

impl MarkedEffects {
    fn open_effects(registry: Option<RuntimeActivityRegistry>) -> Self {
        Self::new(
            BTreeSet::new(),
            registry,
            |open_effects| !open_effects.is_empty(),
            RuntimeActivityRegistry::mark_in_flight_external_effect,
        )
    }
}

impl MarkedGate {
    fn pending_permission(registry: Option<RuntimeActivityRegistry>) -> Self {
        Self::new(
            None,
            registry,
            |pending| pending.is_some(),
            RuntimeActivityRegistry::mark_pending_permission_gate,
        )
    }
}

/// Reads the shared registry the desktop already manages. The coordinate loop
/// never creates a second one.
fn runtime_activity<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Option<RuntimeActivityRegistry> {
    app.try_state::<ChatState>()
        .map(|state| state.runtime_activity.clone())
}

pub(super) fn coordinate<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    journal: SharedStorage,
    runtime: Arc<Mutex<Option<PiRuntime>>>,
    run_id: String,
    prompt: String,
    access_token: String,
    subject: Option<String>,
    grant: ChatGrant,
    cancelled: Arc<AtomicBool>,
    active_transport: Arc<Mutex<Option<Arc<PiRpcTransport>>>>,
    active_adapter: Arc<Mutex<Option<Arc<PiRunAdapter>>>>,
    permission_answers: Arc<Mutex<VecDeque<PendingPermissionAnswer>>>,
    resume: Option<ResumeContext>,
    resume_result: Option<std::sync::mpsc::Sender<Result<(), String>>>,
    prepared: Option<(u64, ChatProjector)>,
) {
    let app = TauriChatEventSink(app);
    let mut resume_attempt = ResumeAttempt::new(resume_result);
    let (mut seq, mut projector) = prepared.unwrap_or_else(|| {
        (
            resume.as_ref().map_or(0, |resume| {
                resume.events.last().map_or(0, |event| event.run_seq)
            }),
            ChatProjector::new(),
        )
    });
    if let Some(resume) = &resume {
        for event in &resume.events {
            if projector.apply(event).is_err() {
                return;
            }
        }
    } else if seq == 0
        && append_emit(
            &app,
            &journal,
            &mut projector,
            &run_id,
            &mut seq,
            "run.started",
            json!({}),
            subject.as_deref(),
        )
        .is_err()
    {
        return;
    }
    if cancelled.load(Ordering::SeqCst) {
        if resume.is_none() {
            let _ = append_emit(
                &app,
                &journal,
                &mut projector,
                &run_id,
                &mut seq,
                "run.cancelled",
                json!({}),
                subject.as_deref(),
            );
        }
        return;
    }
    let prepared_prompt = if resume.is_some() {
        None
    } else {
        let prepared_prompt = match prepared_pi_prompt(&journal, &run_id, &prompt) {
            Ok(prepared_prompt) => prepared_prompt,
            Err(message) => {
                fail_start(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    &message,
                    subject.as_deref(),
                    false,
                );
                return;
            }
        };
        Some(prepared_prompt)
    };
    let mut runtime = runtime
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // A local run owns one Pi conversation. Do not carry a previous run's
    // active session into this prompt.
    *runtime = None;
    {
        let root = match std::env::var("MUNIMENT_PI_ROOT") {
            Ok(root) => root,
            Err(_) => {
                fail_start(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    "The agent runtime is not installed.",
                    subject.as_deref(),
                    resume.is_some(),
                );
                return;
            }
        };
        #[cfg(test)]
        let executable = std::env::var_os("MUNIMENT_PI_TEST_EXECUTABLE")
            .map(std::path::PathBuf::from)
            .map(Ok)
            .unwrap_or_else(|| resolve_current(std::path::Path::new(&root)));
        #[cfg(not(test))]
        let executable = resolve_current(std::path::Path::new(&root));
        let executable = match executable {
            Ok(path) => path,
            Err(_) => {
                fail_start(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    "The agent runtime is unavailable.",
                    subject.as_deref(),
                    resume.is_some(),
                );
                return;
            }
        };
        let session_root = match app.path().app_data_dir() {
            Ok(path) => ChatProfile::new(path).pi_session_root(),
            Err(_) => {
                fail_start(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    "The agent runtime is unavailable.",
                    subject.as_deref(),
                    resume.is_some(),
                );
                return;
            }
        };
        let mut config = match pi_sidecar_config(
            executable.to_string_lossy(),
            &session_root,
            resume.as_ref().map(|resume| &resume.locator),
        ) {
            Ok(config) => config,
            Err(_) => {
                fail_start(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    "The agent runtime is unavailable.",
                    subject.as_deref(),
                    resume.is_some(),
                );
                return;
            }
        };
        // This is a scoped LiteLLM virtual key, never a provider credential. It is
        // inherited by the supervised child only and never serialized or logged.
        config
            .env
            .insert("OPENAI_API_KEY".into(), grant.virtual_key.clone());
        config
            .env
            .insert("OPENAI_BASE_URL".into(), grant.gateway_url.clone());
        if let Some(model) = &grant.model {
            config.env.insert("PI_DEFAULT_MODEL".into(), model.clone());
        }
        if let Some(memory) = app.try_state::<crate::memory::ApplicationMemoryRuntime>() {
            let memory_extension = memory.agent_extension_path();
            if memory_extension.is_file() {
                config.args.extend([
                    "--extension".into(),
                    memory_extension.to_string_lossy().into_owned(),
                ]);
            }
        }
        let wiring = PiRpcWiring::new();
        let supervisor =
            match SidecarSupervisor::spawn(config, wiring.readiness_probe(Duration::from_secs(10)))
            {
                Ok(value) => value,
                Err(_) => {
                    fail_start(
                        &app,
                        &journal,
                        &mut projector,
                        &run_id,
                        &mut seq,
                        "The agent runtime could not start.",
                        subject.as_deref(),
                        resume.is_some(),
                    );
                    return;
                }
            };
        *runtime = Some(PiRuntime { supervisor, wiring });
    }
    let runtime = runtime.as_mut().expect("runtime was initialized");
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while runtime.supervisor.status() == SidecarStatus::Starting
        && std::time::Instant::now() < deadline
    {
        if cancelled.load(Ordering::SeqCst) {
            if resume.is_none() {
                let _ = append_emit(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    "run.cancelled",
                    json!({}),
                    subject.as_deref(),
                );
            }
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let Some(transport) = runtime.wiring.transport() else {
        fail_start(
            &app,
            &journal,
            &mut projector,
            &run_id,
            &mut seq,
            "The agent runtime did not become ready.",
            subject.as_deref(),
            resume.is_some(),
        );
        return;
    };
    *active_transport
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::clone(&transport));
    if cancelled.load(Ordering::SeqCst) {
        if resume.is_none() {
            let _ = append_emit(
                &app,
                &journal,
                &mut projector,
                &run_id,
                &mut seq,
                "run.cancelled",
                json!({}),
                subject.as_deref(),
            );
        }
        return;
    }
    let (adapter, buffered_events) = if resume.is_some() {
        let (adapter, _) =
            match PiRunAdapter::start(run_id.clone(), &transport, &prompt, RPC_TIMEOUT) {
                Ok(value) => value,
                Err(_) => {
                    fail_start(
                        &app,
                        &journal,
                        &mut projector,
                        &run_id,
                        &mut seq,
                        "The reply could not be started.",
                        subject.as_deref(),
                        true,
                    );
                    return;
                }
            };
        let adapter = Arc::new(adapter);
        *active_adapter
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::clone(&adapter));
        if append_emit(
            &app,
            &journal,
            &mut projector,
            &run_id,
            &mut seq,
            "run.resumed",
            json!({}),
            subject.as_deref(),
        )
        .is_err()
        {
            if adapter
                .cancel_and_drain(&transport, Duration::from_secs(2))
                .is_err()
            {
                let _ = runtime.supervisor.shutdown();
            }
            return;
        }
        // Prompt acknowledgement only proves that Pi accepted work. Report a
        // successful resume after the transition is durable so callers never
        // observe an active continuation that the journal still calls
        // interrupted.
        resume_attempt.accepted();
        if append_emit(
            &app,
            &journal,
            &mut projector,
            &run_id,
            &mut seq,
            "model.prompt.accepted",
            json!({}),
            subject.as_deref(),
        )
        .is_err()
        {
            if adapter
                .cancel_and_drain(&transport, Duration::from_secs(2))
                .is_err()
            {
                let _ = runtime.supervisor.shutdown();
            }
            return;
        }
        (adapter, Vec::new())
    } else {
        let result = coordinate_prepared_prompt(
            &app,
            &journal,
            &mut projector,
            &run_id,
            &mut seq,
            subject.as_deref(),
            || {
                let prepared_prompt = prepared_prompt.expect("new runs prepare a Pi prompt");
                let (adapter, _) = PiRunAdapter::start_with_images(
                    run_id.clone(),
                    &transport,
                    prepared_prompt.message,
                    prepared_prompt.images,
                    RPC_TIMEOUT,
                )
                .map_err(|_| PreparedPromptError::Start)?;
                let adapter = Arc::new(adapter);
                *active_adapter
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) =
                    Some(Arc::clone(&adapter));
                let session_root = app
                    .path()
                    .app_data_dir()
                    .map(|path| ChatProfile::new(path).pi_session_root())
                    .map_err(|_| PreparedPromptError::SessionRoot)?;
                let (locator, events) = adapter
                    .await_session_binding(&transport, &session_root, RPC_TIMEOUT)
                    .map_err(|_| PreparedPromptError::Binding)?;
                Ok((adapter, locator, events))
            },
        );
        match result {
            Ok(value) => value,
            Err(error @ (PreparedPromptError::Journal | PreparedPromptError::SessionRoot)) => {
                let adapter = active_adapter
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone();
                if adapter.is_some_and(|adapter| {
                    adapter
                        .cancel_and_drain(&transport, Duration::from_secs(2))
                        .is_err()
                }) {
                    let _ = runtime.supervisor.shutdown();
                }
                if matches!(error, PreparedPromptError::SessionRoot) {
                    fail(
                        &app,
                        &journal,
                        &mut projector,
                        &run_id,
                        &mut seq,
                        "The reply could not be started.",
                        subject.as_deref(),
                    );
                }
                return;
            }
            Err(PreparedPromptError::Binding) => {
                // `await_session_binding` aborts and drains first. Reaping the
                // supervised child is the final containment boundary if Pi did
                // not acknowledge cancellation.
                let _ = runtime.supervisor.shutdown();
                fail(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    "The reply could not be started.",
                    subject.as_deref(),
                );
                return;
            }
            Err(PreparedPromptError::Start) => {
                fail(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    "The reply could not be started.",
                    subject.as_deref(),
                );
                return;
            }
        }
    };
    let mut buffered_events = buffered_events.into_iter();
    let mut aborting = false;
    let registry = runtime_activity(&app);
    let mut open_effects = MarkedEffects::open_effects(registry.clone());
    let mut pending_permission = MarkedGate::pending_permission(registry);
    'coordinate: loop {
        if cancelled.swap(false, Ordering::SeqCst) {
            aborting = true;
            let _ = transport.call(cancel_command(), Duration::from_secs(2));
        }
        let answers: Vec<_> = permission_answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .drain(..)
            .collect();
        for answer in answers {
            if pending_permission
                .with(|pending| {
                    coordinate_permission_answer(
                        pending,
                        answer,
                        |request, answer| adapter.answer_extension_ui(&transport, request, answer),
                        |kind, payload| {
                            append_emit(
                                &app,
                                &journal,
                                &mut projector,
                                &run_id,
                                &mut seq,
                                kind,
                                payload,
                                subject.as_deref(),
                            )?;
                            Ok(seq)
                        },
                    )
                })
                .is_err()
            {
                break 'coordinate;
            }
        }
        let event = buffered_events
            .next()
            .map(Ok)
            .unwrap_or_else(|| adapter.next(Duration::from_millis(100)));
        match event {
            Ok(PiChatEvent::TextDelta(text)) => {
                for slice in split_model_stream_delta(&text) {
                    if append_emit(
                        &app,
                        &journal,
                        &mut projector,
                        &run_id,
                        &mut seq,
                        "model.stream.delta",
                        model_stream_delta_payload(slice),
                        subject.as_deref(),
                    )
                    .is_err()
                    {
                        break 'coordinate;
                    }
                }
            }
            Ok(PiChatEvent::Completed) if aborting => {
                let _ = append_terminal(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    &mut open_effects,
                    "run.cancelled",
                    json!({}),
                    subject.as_deref(),
                );
                break;
            }
            Ok(PiChatEvent::Completed) => {
                match fetch_receipt(&grant.receipt_url, &access_token, &run_id) {
                    Ok(receipt) => {
                        let _ = append_terminal(
                            &app,
                            &journal,
                            &mut projector,
                            &run_id,
                            &mut seq,
                            &mut open_effects,
                            "run.completed",
                            json!({"receipt": receipt}),
                            subject.as_deref(),
                        );
                        break;
                    }
                    Err(_) => {
                        fail_with_open_effects(
                            &app,
                            &journal,
                            &mut projector,
                            &run_id,
                            &mut seq,
                            &mut open_effects,
                            "The reply finished, but its receipt was unavailable.",
                            subject.as_deref(),
                        );
                        break;
                    }
                }
            }
            Ok(PiChatEvent::Cancelled) => {
                let _ = append_terminal(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    &mut open_effects,
                    "run.cancelled",
                    json!({}),
                    subject.as_deref(),
                );
                break;
            }
            Ok(PiChatEvent::Failed) => {
                fail_with_open_effects(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    &mut open_effects,
                    "The model could not complete this reply.",
                    subject.as_deref(),
                );
                break;
            }
            Ok(event @ (PiChatEvent::ToolStarted { .. } | PiChatEvent::ToolFinished { .. })) => {
                if let Some((kind, payload)) =
                    open_effects.with(|open_effects| tool_journal_entry(&event, open_effects))
                {
                    if append_emit(
                        &app,
                        &journal,
                        &mut projector,
                        &run_id,
                        &mut seq,
                        kind,
                        payload,
                        subject.as_deref(),
                    )
                    .is_err()
                    {
                        break;
                    }
                }
            }
            Ok(event @ PiChatEvent::ExtensionUiRequest(_)) => {
                if coordinate_memory_search(
                    &app,
                    &journal,
                    &mut projector,
                    &adapter,
                    &transport,
                    &run_id,
                    &mut seq,
                    subject.as_deref(),
                    &event,
                ) {
                    continue;
                }
                if pending_permission
                    .with(|pending| {
                        coordinate_extension_ui_request(event, pending, |kind, payload| {
                            append_emit(
                                &app,
                                &journal,
                                &mut projector,
                                &run_id,
                                &mut seq,
                                kind,
                                payload,
                                subject.as_deref(),
                            )
                        })
                    })
                    .is_err()
                {
                    break;
                }
            }
            Ok(PiChatEvent::Interleaved | PiChatEvent::PromptAccepted) => {}
            Err(error) if error == "timed out waiting for Pi stream" => {
                if matches!(
                    runtime.supervisor.status(),
                    SidecarStatus::Failed | SidecarStatus::Stopped
                ) {
                    fail_with_open_effects(
                        &app,
                        &journal,
                        &mut projector,
                        &run_id,
                        &mut seq,
                        &mut open_effects,
                        "The agent runtime stopped unexpectedly.",
                        subject.as_deref(),
                    );
                    break;
                }
            }
            Err(_) => {
                fail_with_open_effects(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    &mut open_effects,
                    "The agent runtime stopped unexpectedly.",
                    subject.as_deref(),
                );
                break;
            }
        }
    }
}

fn coordinate_memory_search<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    journal: &SharedStorage,
    projector: &mut ChatProjector,
    adapter: &PiRunAdapter,
    transport: &PiRpcTransport,
    run_id: &str,
    seq: &mut u64,
    subject: Option<&str>,
    event: &PiChatEvent,
) -> bool {
    coordinate_memory_search_with(
        app,
        journal,
        projector,
        run_id,
        seq,
        subject,
        event,
        |arguments| dispatch_memory_search(app, run_id, arguments),
        |request, answer| {
            let _ = adapter.answer_extension_ui(transport, request, answer);
        },
    )
}

fn dispatch_memory_search<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    run_id: &str,
    arguments: &str,
) -> Result<muniment_core::memory_index::MemorySearchResult, MemoryFailure> {
    app.try_state::<crate::memory::ApplicationMemoryRuntime>()
        .ok_or_else(MemoryFailure::runtime_unavailable)?
        .dispatch_tool_call(run_id, "memory-search", arguments.as_bytes())
        .map_err(MemoryFailure::from_index_error)
}

fn coordinate_memory_search_with<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    journal: &SharedStorage,
    projector: &mut ChatProjector,
    run_id: &str,
    seq: &mut u64,
    subject: Option<&str>,
    event: &PiChatEvent,
    dispatch: impl FnOnce(
        &str,
    ) -> Result<muniment_core::memory_index::MemorySearchResult, MemoryFailure>,
    respond: impl FnOnce(&ExtensionUiRequest, ExtensionUiAnswer),
) -> bool {
    let sink = TauriChatEventSink(app.clone());
    let PiChatEvent::ExtensionUiRequest(request) = event else {
        return false;
    };
    let ExtensionUiDialog::Editor { title, prefill } = &request.dialog else {
        return false;
    };
    if title != "muniment:memory-search" {
        return false;
    }
    let result = prefill
        .as_deref()
        .ok_or_else(MemoryFailure::missing_prefill)
        .and_then(dispatch);
    let answer = match result {
        Ok(result) => {
            if append_emit(
                &sink,
                journal,
                projector,
                run_id,
                seq,
                "memory.recalled",
                serde_json::to_value(&result.recall).unwrap_or_else(|_| json!({})),
                subject,
            )
            .is_err()
            {
                ExtensionUiAnswer::Cancelled
            } else {
                ExtensionUiAnswer::Editor(
                    serde_json::to_string(&result).expect("memory search result serializes"),
                )
            }
        }
        Err(error) => ExtensionUiAnswer::Editor(
            serde_json::to_string(&MemoryFailureAnswer::from(error))
                .expect("memory search failure serializes"),
        ),
    };
    respond(request, answer);
    true
}

#[allow(clippy::result_unit_err, clippy::too_many_arguments)]
fn append_terminal(
    sink: &impl ChatEventSink,
    journal: &SharedStorage,
    projector: &mut ChatProjector,
    run_id: &str,
    seq: &mut u64,
    open_effects: &mut MarkedEffects,
    kind: &str,
    payload: Value,
    subject: Option<&str>,
) -> Result<(), ()> {
    open_effects.with(|open_effects| {
        core_append_terminal(
            sink,
            journal,
            projector,
            run_id,
            seq,
            open_effects,
            kind,
            payload,
            subject,
        )
    })
}

#[allow(clippy::too_many_arguments)]
fn fail_with_open_effects(
    sink: &impl ChatEventSink,
    journal: &SharedStorage,
    projector: &mut ChatProjector,
    run_id: &str,
    seq: &mut u64,
    open_effects: &mut MarkedEffects,
    reason: &str,
    subject: Option<&str>,
) {
    open_effects.with(|open_effects| {
        core_fail_with_open_effects(
            sink,
            journal,
            projector,
            run_id,
            seq,
            open_effects,
            reason,
            subject,
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::append_test_event;
    use muniment_core::journal::pi_translation::close_open_effects;
    use muniment_core::journal::reducer::ChatProjection;
    use muniment_core::journal::reducer::ProjectedRecall;
    use muniment_core::journal::RunJournal;
    use muniment_core::memory_index::MemoryIndexError;
    use muniment_core::permission_gate::ChatPermissionAnswer;
    use std::cell::RefCell;
    use std::io;
    use uuid::Uuid;

    fn failed_memory_search_answer(
        prefill: Option<&str>,
        failure: Option<MemoryFailure>,
    ) -> ExtensionUiAnswer {
        let root = std::env::temp_dir().join(format!("muniment-memory-failure-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&root).unwrap();
        let app = tauri::test::mock_app();
        let shared = Arc::new(Mutex::new(muniment_core::run_events::ChatStorage {
            journal: RunJournal::open(root.join("runs.sqlite3")).unwrap(),
            cas: muniment_core::cas::LocalCas::open(&root.join("cas")).unwrap(),
        }));
        let event = PiChatEvent::ExtensionUiRequest(ExtensionUiRequest {
            id: "memory-failure".into(),
            dialog: ExtensionUiDialog::Editor {
                title: "muniment:memory-search".into(),
                prefill: prefill.map(str::to_owned),
            },
            timeout: None,
        });
        let captured = RefCell::new(None);

        assert!(coordinate_memory_search_with(
            app.handle(),
            &shared,
            &mut ChatProjector::new(),
            "run-1",
            &mut 0,
            None,
            &event,
            |arguments| match failure {
                Some(failure) => Err(failure),
                None => dispatch_memory_search(app.handle(), "run-1", arguments),
            },
            |_, answer| *captured.borrow_mut() = Some(answer),
        ));

        captured.into_inner().unwrap()
    }

    #[test]
    fn failed_memory_search_calls_answer_with_each_typed_error() {
        let cases = [
            (
                MemoryIndexError::InvalidToolArguments,
                "invalid_tool_arguments",
                "The memory search arguments are invalid.",
            ),
            (
                MemoryIndexError::LimitRaised,
                "limit_raised",
                "The memory search requested a limit above the configured limit.",
            ),
            (
                MemoryIndexError::QueryTooLong,
                "query_too_long",
                "The memory search query is too long.",
            ),
            (
                MemoryIndexError::TimedOut,
                "timed_out",
                "The memory search timed out.",
            ),
            (
                MemoryIndexError::SecretRejected,
                "secret_rejected",
                "The memory search rejected content that contains a secret.",
            ),
            (
                MemoryIndexError::InvalidPath,
                "invalid_path",
                "The memory search found an invalid path.",
            ),
            (
                MemoryIndexError::Sqlite(rusqlite::Error::InvalidQuery),
                "sqlite",
                "The memory search database failed.",
            ),
            (
                MemoryIndexError::Io(io::Error::other("test")),
                "io",
                "The memory search input or output operation failed.",
            ),
        ];

        for (error, kind, message) in cases {
            assert_eq!(
                failed_memory_search_answer(
                    Some(r#"{"query":"records"}"#),
                    Some(MemoryFailure::from_index_error(error)),
                ),
                ExtensionUiAnswer::Editor(
                    json!({"error": {"kind": kind, "message": message}}).to_string()
                ),
            );
        }
    }

    #[test]
    fn failed_memory_search_calls_answer_for_missing_inputs_and_runtime() {
        assert_eq!(
            failed_memory_search_answer(None, None),
            ExtensionUiAnswer::Editor(
                json!({
                    "error": {
                        "kind": "missing_prefill",
                        "message": "The memory search arguments are missing."
                    }
                })
                .to_string()
            ),
        );
        assert_eq!(
            failed_memory_search_answer(Some(r#"{"query":"records"}"#), None),
            ExtensionUiAnswer::Editor(
                json!({
                    "error": {
                        "kind": "runtime_unavailable",
                        "message": "The memory search runtime is unavailable."
                    }
                })
                .to_string()
            ),
        );
    }

    #[test]
    fn live_chat_event_carries_projected_recalls() {
        let event = muniment_core::run_events::chat_event(
            "run-1",
            ChatProjection {
                recalls: vec![ProjectedRecall {
                    query: "lease terms".into(),
                    files: vec!["memory/lease.md".into()],
                }],
                ..ChatProjection::default()
            },
        );

        assert_eq!(
            serde_json::to_value(event).unwrap()["recalls"],
            json!([{"query": "lease terms", "files": ["memory/lease.md"]}])
        );
    }

    fn confirm_request(gate_id: &str) -> ExtensionUiRequest {
        ExtensionUiRequest {
            id: gate_id.into(),
            dialog: ExtensionUiDialog::Confirm {
                title: "Run the command?".into(),
                message: "The agent wants to run a command.".into(),
            },
            timeout: None,
        }
    }

    fn open_gate(gate: &mut MarkedGate, gate_id: &str) {
        gate.with(|pending| {
            coordinate_extension_ui_request(
                PiChatEvent::ExtensionUiRequest(confirm_request(gate_id)),
                pending,
                |_, _| Ok(()),
            )
        })
        .unwrap();
    }

    fn answer_gate(gate: &mut MarkedGate, gate_id: &str) {
        gate.with(|pending| {
            coordinate_permission_answer(
                pending,
                PendingPermissionAnswer {
                    gate_id: gate_id.into(),
                    answer: ChatPermissionAnswer::Confirm(true),
                    resolved: None,
                },
                |_, _| Ok(()),
                |_, _| Ok(7),
            )
        })
        .unwrap();
    }

    fn start_effect(effects: &mut MarkedEffects, effect_id: &str) {
        let started = PiChatEvent::ToolStarted {
            tool_call_id: effect_id.into(),
            tool_name: "Read file".into(),
        };
        assert!(effects
            .with(|open_effects| tool_journal_entry(&started, open_effects))
            .is_some());
    }

    #[test]
    fn an_open_permission_gate_marks_runtime_activity_until_the_answer_lands() {
        let registry = RuntimeActivityRegistry::new();
        let mut gate = MarkedGate::pending_permission(Some(registry.clone()));
        assert!(!registry.snapshot().pending_permission_gate);

        open_gate(&mut gate, "gate-1");
        assert!(registry.snapshot().pending_permission_gate);

        answer_gate(&mut gate, "gate-1");
        assert!(!registry.snapshot().pending_permission_gate);
    }

    #[test]
    fn an_answer_for_another_gate_leaves_the_permission_mark_set() {
        let registry = RuntimeActivityRegistry::new();
        let mut gate = MarkedGate::pending_permission(Some(registry.clone()));
        open_gate(&mut gate, "gate-1");
        assert!(registry.snapshot().pending_permission_gate);

        answer_gate(&mut gate, "gate-2");
        assert!(registry.snapshot().pending_permission_gate);
    }

    #[test]
    fn an_open_external_effect_marks_runtime_activity_until_the_last_one_closes() {
        let registry = RuntimeActivityRegistry::new();
        let mut effects = MarkedEffects::open_effects(Some(registry.clone()));
        assert!(!registry.snapshot().in_flight_external_effect);

        start_effect(&mut effects, "effect-1");
        assert!(registry.snapshot().in_flight_external_effect);
        start_effect(&mut effects, "effect-2");
        assert!(registry.snapshot().in_flight_external_effect);

        let finished = PiChatEvent::ToolFinished {
            tool_call_id: "effect-1".into(),
            failed: false,
        };
        assert!(effects
            .with(|open_effects| tool_journal_entry(&finished, open_effects))
            .is_some());
        assert!(registry.snapshot().in_flight_external_effect);

        let finished = PiChatEvent::ToolFinished {
            tool_call_id: "effect-2".into(),
            failed: false,
        };
        assert!(effects
            .with(|open_effects| tool_journal_entry(&finished, open_effects))
            .is_some());
        assert!(!registry.snapshot().in_flight_external_effect);
    }

    #[test]
    fn closing_the_open_effects_at_a_terminal_event_clears_the_effect_mark() {
        let registry = RuntimeActivityRegistry::new();
        let mut effects = MarkedEffects::open_effects(Some(registry.clone()));
        start_effect(&mut effects, "effect-1");
        start_effect(&mut effects, "effect-2");
        assert!(registry.snapshot().in_flight_external_effect);

        let mut closed = Vec::new();
        effects
            .with(|open_effects| {
                close_open_effects(open_effects, |kind, payload| {
                    closed.push((kind.to_owned(), payload));
                    Ok::<(), ()>(())
                })
            })
            .unwrap();
        assert_eq!(closed.len(), 2);
        assert!(!registry.snapshot().in_flight_external_effect);
    }

    #[test]
    fn a_run_that_ends_with_an_open_gate_and_an_open_effect_clears_both_marks() {
        let registry = RuntimeActivityRegistry::new();
        {
            let mut gate = MarkedGate::pending_permission(Some(registry.clone()));
            let mut effects = MarkedEffects::open_effects(Some(registry.clone()));
            open_gate(&mut gate, "gate-1");
            start_effect(&mut effects, "effect-1");
            assert!(registry.snapshot().pending_permission_gate);
            assert!(registry.snapshot().in_flight_external_effect);
        }
        // The coordinate loop drops both values when it returns, whether the run
        // failed or ended.
        assert!(!registry.snapshot().pending_permission_gate);
        assert!(!registry.snapshot().in_flight_external_effect);
    }

    #[test]
    fn application_chat_session_dispatches_two_memory_turns_and_persists_recalls() {
        let root = std::env::temp_dir().join(format!("muniment-memory-chat-{}", Uuid::now_v7()));
        let home = root.join("home");
        std::fs::create_dir_all(home.join("memory")).unwrap();
        std::fs::write(home.join("memory/fact.md"), "saffron belongs in the pantry").unwrap();
        let runtime =
            crate::memory::ApplicationMemoryRuntime::new(root.join("config"), root.join("cache"));
        let run_id = Uuid::now_v7().to_string();
        runtime.open_session_for_home(
            &run_id,
            "thread-1",
            muniment_core::memory_index::ModelMemoryCapability {
                minimum_cacheable_prefix_characters: 100,
            },
            &home,
            root.join("cache/index.sqlite3"),
        );
        let first_definition = runtime.tool_definition_for_turn(&run_id).unwrap();
        let second_definition = runtime.tool_definition_for_turn(&run_id).unwrap();
        assert_eq!(first_definition, second_definition);

        let app = tauri::test::mock_app();
        app.manage(runtime);
        let mut journal = RunJournal::open(root.join("runs.sqlite3")).unwrap();
        append_test_event(&mut journal, &run_id, 1, "run.started", json!({}), None);
        let mut projector = ChatProjector::new();
        projector
            .apply(&journal.events(&run_id).unwrap()[0])
            .unwrap();
        let shared = Arc::new(Mutex::new(muniment_core::run_events::ChatStorage {
            journal,
            cas: muniment_core::cas::LocalCas::open(&root.join("cas")).unwrap(),
        }));

        let executable_name = if cfg!(windows) {
            "sidecar-test-stub.exe"
        } else {
            "sidecar-test-stub"
        };
        let test_executable = std::env::current_exe().unwrap();
        let stub = test_executable
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("examples")
            .join(executable_name);
        assert!(stub.is_file(), "sidecar test stub was not built");
        let capture = root.join("answers.jsonl");
        let mut config = muniment_core::sidecar::SidecarConfig::new(stub.to_string_lossy());
        config.args = vec![
            "pi-chat-extension-ui".into(),
            capture.to_string_lossy().into_owned(),
        ];
        config.health_interval = Duration::from_secs(60);
        let wiring = PiRpcWiring::new();
        let mut supervisor =
            SidecarSupervisor::spawn(config, wiring.readiness_probe(Duration::from_millis(100)))
                .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while supervisor.status() != SidecarStatus::Healthy && std::time::Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        let transport = wiring.transport().unwrap();
        let (adapter, _) =
            PiRunAdapter::start(&run_id, &transport, "start", Duration::from_secs(1)).unwrap();
        let mut seq = 1;
        for turn in 1..=2 {
            let event = PiChatEvent::ExtensionUiRequest(ExtensionUiRequest {
                id: format!("memory-{turn}"),
                dialog: ExtensionUiDialog::Editor {
                    title: "muniment:memory-search".into(),
                    prefill: Some(r#"{"query":"saffron"}"#.into()),
                },
                timeout: None,
            });
            assert!(coordinate_memory_search(
                app.handle(),
                &shared,
                &mut projector,
                &adapter,
                &transport,
                &run_id,
                &mut seq,
                None,
                &event,
            ));
        }

        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        let answers = loop {
            let answers: Vec<serde_json::Value> = std::fs::read_to_string(&capture)
                .unwrap_or_default()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            if answers.len() == 2 || std::time::Instant::now() >= deadline {
                break answers;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(answers.len(), 2);
        for answer in answers {
            let result: serde_json::Value =
                serde_json::from_str(answer["value"].as_str().unwrap()).unwrap();
            assert_eq!(result["items"][0]["path"], "memory/fact.md");
        }

        let events = shared.lock().unwrap().journal.events(&run_id).unwrap();
        let recalls: Vec<_> = events
            .iter()
            .filter(|event| event.event_type == "memory.recalled")
            .collect();
        assert_eq!(recalls.len(), 2);
        for recall in recalls {
            let muniment_core::journal::EventPayload::Inline { payload_json } = &recall.payload
            else {
                panic!("memory recall must use an inline payload");
            };
            assert_eq!(payload_json["files"], json!(["memory/fact.md"]));
        }
        app.state::<crate::memory::ApplicationMemoryRuntime>()
            .close_session(&run_id);
        assert_eq!(
            shared
                .lock()
                .unwrap()
                .journal
                .events(&run_id)
                .unwrap()
                .iter()
                .filter(|event| event.event_type == "memory.recalled")
                .count(),
            2
        );
        supervisor.shutdown().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
