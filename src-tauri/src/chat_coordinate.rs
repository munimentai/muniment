use std::collections::{BTreeSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use muniment_core::chat_grant::{fetch_receipt, ChatGrant};
use muniment_core::chat_profile::ChatProfile;
use muniment_core::journal::pi_translation::{
    close_open_effects, model_stream_delta_payload, permission_journal_payload, tool_journal_entry,
};
use muniment_core::journal::reducer::{ChatProjection, ChatProjector};
use muniment_core::journal::run_append::append_run_event;
use muniment_core::journal::split_model_stream_delta;
use muniment_core::sidecar::pi_chat::{
    cancel_command, ExtensionUiAnswer, ExtensionUiDialog, ExtensionUiRequest, ExtensionUiResponse,
    PiChatEvent, PiRunAdapter,
};
use muniment_core::sidecar::pi_install::resolve_current;
use muniment_core::sidecar::{
    pi_sidecar_config, PiRpcTransport, PiRpcWiring, SidecarStatus, SidecarSupervisor,
};
use serde_json::{json, Value};
use tauri::{Emitter, Manager};

use crate::chat::{
    chat_attachments, chat_pending_permission, chat_tool_activity, coordinate_prepared_prompt,
    event_envelope, prepared_pi_prompt, ChatEvent, PendingPermissionAnswer, PiRuntime,
    PreparedPromptError, ResumeAttempt, ResumeContext, SharedStorage, RPC_TIMEOUT,
};
use crate::chat_threads::projection_phase;

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
    let mut open_effects = BTreeSet::new();
    let mut pending_permission = None;
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
            if coordinate_permission_answer(
                &mut pending_permission,
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
                if let Some((kind, payload)) = tool_journal_entry(&event, &mut open_effects) {
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
                if coordinate_extension_ui_request(
                    event,
                    &mut pending_permission,
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
                        )
                    },
                )
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
        .ok_or(())
        .and_then(|arguments| {
            app.try_state::<crate::memory::ApplicationMemoryRuntime>()
                .ok_or(())?
                .dispatch_tool_call(run_id, "memory-search", arguments.as_bytes())
                .map_err(|_| ())
        });
    let answer = match result {
        Ok(result) => {
            if append_emit(
                app,
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
                match serde_json::to_string(&result) {
                    Ok(result) => ExtensionUiAnswer::Editor(result),
                    Err(_) => ExtensionUiAnswer::Cancelled,
                }
            }
        }
        Err(()) => ExtensionUiAnswer::Cancelled,
    };
    let _ = adapter.answer_extension_ui(transport, request, answer);
    true
}

fn coordinate_extension_ui_request(
    event: PiChatEvent,
    pending: &mut Option<ExtensionUiRequest>,
    append: impl FnOnce(&str, Value) -> Result<(), ()>,
) -> Result<(), ()> {
    let PiChatEvent::ExtensionUiRequest(request) = event else {
        return Ok(());
    };
    append("permission.requested", permission_journal_payload(&request))?;
    *pending = Some(request);
    Ok(())
}

fn coordinate_permission_answer(
    pending: &mut Option<ExtensionUiRequest>,
    queued: PendingPermissionAnswer,
    send: impl FnOnce(&ExtensionUiRequest, ExtensionUiAnswer) -> Result<(), String>,
    append: impl FnOnce(&str, Value) -> Result<u64, ()>,
) -> Result<(), ()> {
    let resolved = queued.resolved;
    let Some(request) = pending
        .as_ref()
        .filter(|request| request.id == queued.gate_id)
    else {
        if let Some(resolved) = resolved {
            let _ = resolved.send(None);
        }
        return Ok(());
    };
    let answer = queued.answer.pi_answer();
    if ExtensionUiResponse::new(request, answer.clone()).is_err() {
        if let Some(resolved) = resolved {
            let _ = resolved.send(None);
        }
        return Ok(());
    }
    if send(request, answer).is_err() {
        if let Some(resolved) = resolved {
            let _ = resolved.send(None);
        }
        return Ok(());
    }
    let committed_seq = append(
        "permission.resolved",
        json!({"gate_id": queued.gate_id, "decision": queued.answer.decision()}),
    )?;
    if let Some(resolved) = resolved {
        let _ = resolved.send(Some(committed_seq));
    }
    *pending = None;
    Ok(())
}

fn append_terminal<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    journal: &SharedStorage,
    projector: &mut ChatProjector,
    run_id: &str,
    seq: &mut u64,
    open_effects: &mut BTreeSet<String>,
    kind: &str,
    payload: Value,
    subject: Option<&str>,
) -> Result<(), ()> {
    close_open_effects(open_effects, |kind, payload| {
        append_emit(app, journal, projector, run_id, seq, kind, payload, subject)
    })?;
    append_emit(app, journal, projector, run_id, seq, kind, payload, subject)
}

fn fail_with_open_effects<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    journal: &SharedStorage,
    projector: &mut ChatProjector,
    run_id: &str,
    seq: &mut u64,
    open_effects: &mut BTreeSet<String>,
    reason: &str,
    subject: Option<&str>,
) {
    let _ = append_terminal(
        app,
        journal,
        projector,
        run_id,
        seq,
        open_effects,
        "run.failed",
        json!({"reason": reason}),
        subject,
    );
}

pub(super) fn append_emit<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    journal: &SharedStorage,
    projector: &mut ChatProjector,
    run_id: &str,
    seq: &mut u64,
    kind: &str,
    payload: Value,
    subject: Option<&str>,
) -> Result<(), ()> {
    *seq += 1;
    let envelope = event_envelope(run_id, *seq, kind, payload, subject);
    let projection = append_run_event(
        &mut journal.lock().map_err(|_| ())?.journal,
        projector,
        &envelope,
    )
    .map_err(|_| ())?;
    app.emit("chat-event", chat_event(run_id, projection))
        .map_err(|_| ())
}

fn chat_event(run_id: &str, projection: ChatProjection) -> ChatEvent {
    let attachments = chat_attachments(&projection.attachments);
    ChatEvent {
        run_id: run_id.into(),
        phase: projection_phase(&projection.status).into(),
        text: projection.text,
        receipt: projection.receipt,
        tool_activity: chat_tool_activity(&projection.tool_activity),
        attachments,
        pending_permission: chat_pending_permission(projection.pending_permission),
    }
}

fn fail<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    journal: &SharedStorage,
    projector: &mut ChatProjector,
    run_id: &str,
    seq: &mut u64,
    reason: &str,
    subject: Option<&str>,
) {
    let _ = append_emit(
        app,
        journal,
        projector,
        run_id,
        seq,
        "run.failed",
        json!({"reason": reason}),
        subject,
    );
}

fn fail_start<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    journal: &SharedStorage,
    projector: &mut ChatProjector,
    run_id: &str,
    seq: &mut u64,
    reason: &str,
    subject: Option<&str>,
    resuming: bool,
) {
    if !resuming {
        fail(app, journal, projector, run_id, seq, reason, subject);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::ChatPermissionAnswer;
    use crate::test_support::append_test_event;
    use muniment_core::journal::RunJournal;
    use muniment_core::sidecar::pi_chat::ExtensionUiDialog;
    use uuid::Uuid;

    #[test]
    fn application_chat_session_dispatches_two_memory_turns_and_persists_recalls() {
        let root = std::env::temp_dir().join(format!("muniment-memory-chat-{}", Uuid::now_v7()));
        let home = root.join("home");
        std::fs::create_dir_all(home.join("memory")).unwrap();
        std::fs::write(
            home.join("memory/fact.md"),
            "saffron belongs in the pantry",
        )
        .unwrap();
        let runtime = crate::memory::ApplicationMemoryRuntime::new(
            root.join("config"),
            root.join("cache"),
        );
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
        let shared = Arc::new(Mutex::new(crate::chat::ChatStorage {
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
        let mut supervisor = SidecarSupervisor::spawn(
            config,
            wiring.readiness_probe(Duration::from_millis(100)),
        )
        .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while supervisor.status() != SidecarStatus::Healthy
            && std::time::Instant::now() < deadline
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

    #[test]
    fn coordinator_journals_extension_ui_before_projecting_and_stops_on_failure() {
        let directory = std::env::temp_dir().join(format!("muniment-chat-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut journal = RunJournal::open(directory.join("runs.sqlite3")).unwrap();
        let run_id = Uuid::now_v7().to_string();
        append_test_event(&mut journal, &run_id, 1, "run.started", json!({}), None);
        let mut projector = ChatProjector::new();
        projector
            .apply(&journal.events(&run_id).unwrap()[0])
            .unwrap();
        let mut emitted = Vec::new();
        let mut pending = None;

        coordinate_extension_ui_request(
            PiChatEvent::ExtensionUiRequest(ExtensionUiRequest {
                id: "pi-request-1".into(),
                dialog: ExtensionUiDialog::Confirm {
                    title: "Allow?".into(),
                    message: "Proceed?".into(),
                },
                timeout: Some(5_000),
            }),
            &mut pending,
            |kind, payload| {
                let envelope = event_envelope(&run_id, 2, kind, payload, None);
                journal.append(1, &envelope).map_err(|_| ())?;
                assert_eq!(journal.events(&run_id).unwrap().len(), 2);
                projector.apply(&envelope).map_err(|_| ())?;
                emitted.push(chat_event(&run_id, projector.projection().map_err(|_| ())?));
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(
            journal.events(&run_id).unwrap()[1].event_type,
            "permission.requested"
        );
        let projection = emitted.pop().unwrap();
        assert_eq!(projection.phase, "pending-permission");
        assert_eq!(
            projection.pending_permission.unwrap().gate_id,
            "pi-request-1"
        );

        let mut append_attempts = 0;
        assert!(coordinate_extension_ui_request(
            PiChatEvent::ExtensionUiRequest(ExtensionUiRequest {
                id: "pi-request-2".into(),
                dialog: ExtensionUiDialog::Input {
                    title: "Secret".into(),
                    placeholder: None,
                },
                timeout: None,
            }),
            &mut pending,
            |_kind, _payload| {
                append_attempts += 1;
                Err(())
            },
        )
        .is_err());
        assert_eq!(append_attempts, 1);

        let mut emitted_after_projection_failure = false;
        assert!(coordinate_extension_ui_request(
            PiChatEvent::ExtensionUiRequest(ExtensionUiRequest {
                id: "pi-request-3".into(),
                dialog: ExtensionUiDialog::Select {
                    title: "Choose".into(),
                    options: vec!["A".into(), "B".into()],
                },
                timeout: None,
            }),
            &mut pending,
            |kind, payload| {
                let envelope = event_envelope(&run_id, 3, kind, payload, None);
                let mut next_projector = projector.clone();
                next_projector.apply(&envelope).map_err(|_| ())?;
                next_projector.projection().map_err(|_| ())?;
                journal.append(2, &envelope).map_err(|_| ())?;
                emitted_after_projection_failure = true;
                Ok(())
            },
        )
        .is_err());
        assert!(!emitted_after_projection_failure);
        // The coordinator handler has no response transport and therefore cannot
        // synthesize an allow/deny (or any other Pi extension-UI response).
        let events = journal.events(&run_id).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "run.started");
        assert_eq!(events[1].event_type, "permission.requested");
        let replayed = muniment_core::journal::reducer::project_chat(&events).unwrap();
        assert_eq!(replayed.pending_permission.unwrap().gate_id, "pi-request-1");

        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn coordinator_resolves_only_matching_valid_permission_answers() {
        let request = ExtensionUiRequest {
            id: "gate-1".into(),
            dialog: ExtensionUiDialog::Select {
                title: "Choose".into(),
                options: vec!["A".into(), "B".into()],
            },
            timeout: None,
        };
        let mut pending = Some(request.clone());
        let mut sent = Vec::new();
        let mut appended = Vec::new();
        coordinate_permission_answer(
            &mut pending,
            PendingPermissionAnswer {
                gate_id: "other-gate".into(),
                answer: ChatPermissionAnswer::Select("A".into()),
                resolved: None,
            },
            |_, answer| {
                sent.push(answer);
                Ok(())
            },
            |kind, payload| {
                appended.push((kind.to_string(), payload));
                Ok(1)
            },
        )
        .unwrap();
        assert!(sent.is_empty());
        assert!(appended.is_empty());
        assert_eq!(pending, Some(request.clone()));

        coordinate_permission_answer(
            &mut pending,
            PendingPermissionAnswer {
                gate_id: "gate-1".into(),
                answer: ChatPermissionAnswer::Select("C".into()),
                resolved: None,
            },
            |_, answer| {
                sent.push(answer);
                Ok(())
            },
            |kind, payload| {
                appended.push((kind.to_string(), payload));
                Ok(1)
            },
        )
        .unwrap();
        assert!(sent.is_empty());
        assert!(appended.is_empty());
        assert_eq!(pending, Some(request));

        coordinate_permission_answer(
            &mut pending,
            PendingPermissionAnswer {
                gate_id: "gate-1".into(),
                answer: ChatPermissionAnswer::Select("B".into()),
                resolved: None,
            },
            |_, answer| {
                sent.push(answer);
                Ok(())
            },
            |kind, payload| {
                appended.push((kind.to_string(), payload));
                Ok(1)
            },
        )
        .unwrap();
        assert_eq!(sent, [ExtensionUiAnswer::Selection("B".into())]);
        assert_eq!(
            appended,
            [(
                "permission.resolved".into(),
                json!({
                    "gate_id": "gate-1",
                    "decision": {"type": "select", "value": "B"}
                }),
            )]
        );
        assert!(pending.is_none());
    }
}
