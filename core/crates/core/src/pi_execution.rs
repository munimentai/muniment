use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;

use crate::active_run::RunRegistry;
use crate::attachment::{prepare_pi_images, AttachmentDeliveryError};
use crate::chat_launch::ChatGrant;
use crate::engine::{AgentEngine, ContextUsage, OpenError, PiOpen, PiRpcEngine};
use crate::journal::reducer::ChatProjector;
use crate::journal::{pi_translation::model_stream_delta_payload, split_model_stream_delta};
use crate::pi_launch::{pi_launch_config, PiLaunchBoundaries, PiLaunchError};
use crate::run_events::{append_emit, ChatEventSink, SharedStorage};
use crate::sidecar::pi_chat::{PiChatEvent, PiImageContent, PromptCommand};
use crate::sidecar::{PiRpcTransport, PiRpcWiring, PiSessionLocator, SidecarSupervisor};

pub const RPC_TIMEOUT: Duration = Duration::from_secs(30);

pub fn previous_thread_session(
    storage: &SharedStorage,
    run_id: &str,
    subject: Option<&str>,
    session_root: &std::path::Path,
) -> Result<Option<PiSessionLocator>, String> {
    let binding = storage
        .lock()
        .map_err(|_| "Conversation history is busy.")?
        .journal
        .previous_thread_session_binding(run_id)
        .map_err(|_| "Conversation history could not load.")?;
    session_from_binding(binding, subject, session_root)
}

/// The saved conversation a thread's latest run bound, if it has one.
pub fn thread_session(
    storage: &SharedStorage,
    thread_id: &str,
    subject: Option<&str>,
    session_root: &std::path::Path,
) -> Result<Option<PiSessionLocator>, String> {
    let binding = storage
        .lock()
        .map_err(|_| "Conversation history is busy.")?
        .journal
        .thread_session_binding(thread_id)
        .map_err(|_| "Conversation history could not load.")?;
    session_from_binding(binding, subject, session_root)
}

fn session_from_binding(
    binding: Option<crate::journal::EventEnvelope>,
    subject: Option<&str>,
    session_root: &std::path::Path,
) -> Result<Option<PiSessionLocator>, String> {
    let Some(binding) = binding else {
        return Ok(None);
    };
    if binding.provenance.actor_id.as_deref() != subject {
        return Err("Conversation history belongs to another account.".into());
    }
    let crate::journal::EventPayload::Inline { payload_json } = binding.payload else {
        return Err("Conversation history is unavailable.".into());
    };
    let locator = payload_json
        .get("locator")
        .and_then(serde_json::Value::as_str)
        .ok_or("Conversation history is unavailable.")?;
    crate::sidecar::validate_pi_session(session_root, locator)
        .map(|(locator, _)| Some(locator))
        .map_err(|_| "Conversation history is unavailable. Start a new thread.".into())
}

pub struct PiRuntime {
    pub supervisor: SidecarSupervisor,
    pub wiring: PiRpcWiring,
}

/// The engine processes of a runtime, one slot per thread. A run takes its
/// thread's slot for its whole length, so runs in different threads drive
/// different processes at once. After a run, its process idles in the slot
/// for the thread's next read or run. The least recently used idle slots go
/// once more than `idle` are kept.
/// One thread's process slot.
pub type PiProcessSlot = Arc<Mutex<Option<PiRuntime>>>;

#[derive(Clone)]
pub struct PiProcesses {
    slots: Arc<Mutex<Vec<(String, PiProcessSlot)>>>,
    idle: usize,
}

impl Default for PiProcesses {
    fn default() -> Self {
        Self::with_idle_limit(crate::agent_runs::max_parallel_runs() + 1)
    }
}

impl PiProcesses {
    pub fn with_idle_limit(idle: usize) -> Self {
        Self {
            slots: Arc::new(Mutex::new(Vec::new())),
            idle: idle.max(1),
        }
    }

    /// The slot of one thread's process.
    pub fn slot(&self, thread: &str) -> PiProcessSlot {
        let mut slots = self
            .slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let slot = match slots.iter().position(|(held, _)| held == thread) {
            Some(index) => slots.remove(index).1,
            None => Arc::new(Mutex::new(None)),
        };
        slots.push((thread.to_owned(), Arc::clone(&slot)));
        let mut index = 0;
        while slots.len() > self.idle && index + 1 < slots.len() {
            // A slot a run holds stays; its process ends with the run's next start.
            let idle = slots[index]
                .1
                .try_lock()
                .map(|mut process| {
                    *process = None;
                })
                .is_ok();
            if idle {
                slots.remove(index);
            } else {
                index += 1;
            }
        }
        slot
    }

    /// How many thread slots are kept now.
    pub fn len(&self) -> usize {
        self.slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Opens a thread's saved conversation for a read, such as its context
/// usage. The launch is the one a reply in that thread starts with.
pub fn open_thread_conversation(
    app: &impl PiLaunchBoundaries,
    grant: &ChatGrant,
    locator: &PiSessionLocator,
) -> Result<(PiRuntime, Arc<PiRpcTransport>), String> {
    let root = app
        .pi_install_root()
        .map_err(|_| "The agent runtime is not installed.".to_string())?;
    let config = pi_launch_config(app, Some(&root), grant, Some(locator)).map_err(|error| {
        if matches!(
            error,
            PiLaunchError::MissingRoot | PiLaunchError::UnresolvableExecutable
        ) {
            "The agent runtime is not installed.".to_string()
        } else {
            "The agent runtime is unavailable.".to_string()
        }
    })?;
    let engine = PiRpcEngine::new(app.pi_session_root().ok());
    match engine.open(PiOpen {
        config,
        cancelled: &AtomicBool::new(false),
        observe: &mut |_| {},
    }) {
        Ok(opened) => Ok(opened),
        Err(OpenError::Spawn(_)) => Err("The agent runtime could not start.".into()),
        Err(_) => Err("The agent runtime did not become ready.".into()),
    }
}

/// Reads a thread's context usage from the process that holds its saved
/// conversation: the active run's when a run is in this thread, the idle
/// process the thread's last run left when it holds this conversation, or a
/// process this read starts. A started process stays in the thread's slot
/// for the next read. A thread with no saved conversation answers `None`.
#[allow(clippy::too_many_arguments)]
pub fn thread_context_usage(
    app: &impl PiLaunchBoundaries,
    storage: &SharedStorage,
    processes: &PiProcesses,
    active: &RunRegistry,
    thread_id: &str,
    subject: Option<&str>,
    grant: impl FnOnce() -> Result<ChatGrant, String>,
) -> Result<Option<ContextUsage>, String> {
    let session_root = app
        .pi_session_root()
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    let engine = PiRpcEngine::new(Some(session_root.clone()));
    let active_transport = active.thread_run(thread_id).and_then(|run_id| {
        active
            .with_run(&run_id, None, |run| {
                run.transport
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone()
            })
            .ok()
            .flatten()
    });
    if let Some(transport) = active_transport {
        return engine.context_usage(&transport).map(Some);
    }
    let Some(locator) = thread_session(storage, thread_id, subject, &session_root)? else {
        return Ok(None);
    };
    let holds = |transport: &PiRpcTransport| {
        transport
            .session_locator(&session_root, RPC_TIMEOUT.min(Duration::from_secs(5)))
            .is_ok_and(|held| held == locator)
    };
    let runtime = processes.slot(thread_id);
    let usage = match runtime.try_lock() {
        Ok(mut slot) => {
            if let Some(transport) = slot
                .as_ref()
                .and_then(|process| process.wiring.transport())
                .filter(|transport| holds(transport))
            {
                return engine.context_usage(&transport).map(Some);
            }
            *slot = None;
            let (process, transport) = open_thread_conversation(app, &grant()?, &locator)?;
            *slot = Some(process);
            engine.context_usage(&transport).map(Some)
        }
        Err(_) => {
            let (mut process, transport) = open_thread_conversation(app, &grant()?, &locator)?;
            let usage = engine.context_usage(&transport).map(Some);
            engine.stop(&mut process);
            usage
        }
    };
    usage
}

pub struct ResumeAttempt {
    result: Option<Sender<Result<(), String>>>,
}

impl ResumeAttempt {
    pub fn new(result: Option<Sender<Result<(), String>>>) -> Self {
        Self { result }
    }

    pub fn accepted(&mut self) {
        if let Some(result) = self.result.take() {
            let _ = result.send(Ok(()));
        }
    }
}

impl Drop for ResumeAttempt {
    fn drop(&mut self) {
        if let Some(result) = self.result.take() {
            let _ = result.send(Err("This reply could not be resumed. Try again.".into()));
        }
    }
}

pub fn attachment_error() -> String {
    "One or more selected files could not be added. Check the files and try again.".into()
}

pub fn attachment_delivery_error(error: AttachmentDeliveryError) -> String {
    match error {
        AttachmentDeliveryError::ImageSizeLimit { display_name } => format!(
            "{display_name} exceeds the 10 MB image limit. Choose a smaller image before sending again."
        ),
        AttachmentDeliveryError::ImageCountLimit { display_name } => format!(
            "{display_name} crosses the 10-image limit. Remove an image before sending again."
        ),
        AttachmentDeliveryError::ImageTotalSizeLimit { display_name } => format!(
            "{display_name} crosses the 20 MB total image limit. Remove images or choose smaller images before sending again."
        ),
        AttachmentDeliveryError::AmbiguousFormat { display_name } => format!(
            "{display_name} has an image format Muniment cannot verify. Choose a PNG, JPEG, GIF, or WebP image before sending again."
        ),
        AttachmentDeliveryError::Storage(_) | AttachmentDeliveryError::InvalidStoredLength => {
            attachment_error()
        }
    }
}

pub fn prepared_pi_images(
    storage: &SharedStorage,
    run_id: &str,
) -> Result<Vec<PiImageContent>, String> {
    let mut storage = storage.lock().map_err(|_| attachment_error())?;
    let events = storage
        .journal
        .events(run_id)
        .map_err(|_| attachment_error())?;
    prepare_pi_images(&storage.cas, &events)
        .map_err(attachment_delivery_error)
        .map(|images| {
            images
                .into_iter()
                .map(|image| PiImageContent::new(image.data, image.mime_type))
                .collect()
        })
}

pub fn prepared_pi_prompt<'a>(
    storage: &SharedStorage,
    run_id: &str,
    prompt: &'a str,
) -> Result<PromptCommand<'a>, String> {
    prepared_pi_images(storage, run_id).map(|images| PromptCommand::with_images(prompt, images))
}

#[derive(Debug)]
pub enum PreparedPromptError {
    Start,
    SessionRoot,
    Binding,
    FirstEventTimeout,
    Journal,
}

#[allow(clippy::too_many_arguments)]
pub fn coordinate_prepared_prompt<T>(
    sink: &impl ChatEventSink,
    journal: &SharedStorage,
    projector: &mut ChatProjector,
    run_id: &str,
    seq: &mut u64,
    subject: Option<&str>,
    submit: impl FnOnce(
        &mut dyn FnMut(&PiChatEvent) -> Result<bool, PreparedPromptError>,
    ) -> Result<(T, PiSessionLocator, Vec<PiChatEvent>), PreparedPromptError>,
) -> Result<(T, Vec<PiChatEvent>), PreparedPromptError> {
    let (handle, locator, buffered_events) = submit(&mut |event| {
        let (kind, payloads) = match event {
            PiChatEvent::PromptAccepted => ("model.prompt.accepted", vec![json!({})]),
            PiChatEvent::RoutingStage(stage) => {
                ("model.routing.stage", vec![json!({"stage": stage})])
            }
            PiChatEvent::TurnStarted => ("model.turn.started", vec![json!({})]),
            PiChatEvent::TextDelta(text) => (
                "model.stream.delta",
                split_model_stream_delta(text)
                    .map(model_stream_delta_payload)
                    .collect(),
            ),
            _ => return Ok(false),
        };
        for payload in payloads {
            append_emit(
                sink, journal, projector, run_id, seq, kind, payload, subject,
            )
            .map_err(|_| PreparedPromptError::Journal)?;
        }
        Ok(true)
    })?;
    append_emit(
        sink,
        journal,
        projector,
        run_id,
        seq,
        "runtime.pi_session.bound",
        json!({"run_id": run_id, "locator": locator.as_str()}),
        subject,
    )
    .map_err(|_| PreparedPromptError::Journal)?;
    Ok((handle, buffered_events))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::{Arc, Mutex};

    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use chrono::{SecondsFormat, Utc};
    use uuid::Uuid;

    use crate::attachment::{ingest_attachment, AttachmentMetadata};
    use crate::cas::LocalCas;
    use crate::journal::{EventEnvelope, EventPayload, Provenance, RunJournal};
    use crate::run_events::{ChatEvent, ChatStorage};

    #[derive(Default)]
    struct TestSink;

    impl ChatEventSink for TestSink {
        fn provenance(&self) -> (&str, &str) {
            ("test", "0.0.0")
        }

        fn deliver(&self, _event: ChatEvent) -> Result<(), ()> {
            Ok(())
        }
    }

    fn storage() -> (std::path::PathBuf, SharedStorage) {
        let root = std::env::temp_dir().join(format!("muniment-pi-execution-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&root).unwrap();
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(root.join("runs.sqlite3")).unwrap(),
            cas: LocalCas::open(&root.join("cas")).unwrap(),
        }));
        (root, storage)
    }

    fn attachment_event(run_id: &str, run_seq: u64) -> EventEnvelope {
        EventEnvelope {
            event_id: Uuid::now_v7().to_string(),
            run_id: run_id.into(),
            run_seq,
            event_type: String::new(),
            event_version: 0,
            envelope_version: 1,
            recorded_at: Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true),
            occurred_at: None,
            correlation_id: None,
            causation_id: None,
            payload: EventPayload::Inline {
                payload_json: json!({}),
            },
            provenance: Provenance {
                source: "pi-execution-test".into(),
                source_version: env!("CARGO_PKG_VERSION").into(),
                actor_id: None,
                device_id: None,
                rpc_request_id: None,
                capability_versions: None,
                extra: Default::default(),
            },
            extra: Default::default(),
        }
    }

    fn ingest(storage: &SharedStorage, run_id: &str, run_seq: u64, name: &str, bytes: &[u8]) {
        let mut storage = storage.lock().unwrap();
        let ChatStorage { journal, cas } = &mut *storage;
        ingest_attachment(
            cas,
            journal,
            run_seq - 1,
            &mut Cursor::new(bytes),
            AttachmentMetadata {
                display_name: name,
                byte_length: bytes.len() as u64,
                media_type: None,
            },
            |attachment| EventEnvelope {
                payload: EventPayload::Attachment { attachment },
                ..attachment_event(run_id, run_seq)
            },
        )
        .unwrap();
    }

    fn png() -> Vec<u8> {
        STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")
            .unwrap()
    }

    #[test]
    fn each_thread_has_its_own_process_slot_and_idle_slots_are_bounded() {
        let processes = PiProcesses::with_idle_limit(2);
        let first = processes.slot("thread-a");
        assert!(Arc::ptr_eq(&first, &processes.slot("thread-a")));
        assert!(!Arc::ptr_eq(&first, &processes.slot("thread-b")));
        // A slot a run holds is never evicted; the oldest idle one goes.
        let held = first.lock().unwrap();
        processes.slot("thread-c");
        assert_eq!(processes.len(), 2);
        drop(held);
        assert!(Arc::ptr_eq(&first, &processes.slot("thread-a")));
        processes.slot("thread-d");
        assert_eq!(processes.len(), 2);
    }

    #[test]
    fn follow_up_reopens_only_its_threads_previous_session() {
        let (root, storage) = storage();
        let first = Uuid::now_v7().to_string();
        let second = Uuid::now_v7().to_string();
        let other = Uuid::now_v7().to_string();
        let start = |run_id: &str| {
            let mut event = attachment_event(run_id, 1);
            event.event_type = "run.started".into();
            event.event_version = 1;
            event.provenance.actor_id = Some("owner".into());
            event
        };
        std::fs::write(root.join("conversation.jsonl"), "{}\n").unwrap();
        let thread = {
            let mut storage = storage.lock().unwrap();
            let thread = storage
                .journal
                .append_new_run("local", &start(&first))
                .unwrap();
            let mut binding = start(&first);
            binding.run_seq = 2;
            binding.event_type = "runtime.pi_session.bound".into();
            binding.payload = EventPayload::Inline {
                payload_json: json!({"run_id": first, "locator": "conversation.jsonl"}),
            };
            storage.journal.append(1, &binding).unwrap();
            storage
                .journal
                .append_new_run_in_thread("local", &thread, &start(&second))
                .unwrap();
            storage
                .journal
                .append_new_run("local", &start(&other))
                .unwrap();
            thread
        };
        assert_eq!(
            thread_session(&storage, &thread, Some("owner"), &root)
                .unwrap()
                .unwrap()
                .as_str(),
            "conversation.jsonl"
        );
        assert!(
            thread_session(&storage, "no-such-thread", Some("owner"), &root)
                .unwrap()
                .is_none()
        );
        assert!(
            previous_thread_session(&storage, &first, Some("owner"), &root)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            previous_thread_session(&storage, &second, Some("owner"), &root)
                .unwrap()
                .unwrap()
                .as_str(),
            "conversation.jsonl"
        );
        assert!(
            previous_thread_session(&storage, &other, Some("owner"), &root)
                .unwrap()
                .is_none()
        );
        assert!(previous_thread_session(&storage, &second, Some("another-owner"), &root).is_err());
        std::fs::remove_file(root.join("conversation.jsonl")).unwrap();
        assert!(previous_thread_session(&storage, &second, Some("owner"), &root).is_err());
        drop(storage);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn coordinate_prepared_prompt_submits_before_it_appends_coordinator_events() {
        let (root, storage) = storage();
        let run_id = Uuid::now_v7().to_string();
        let mut projector = ChatProjector::new();
        let mut seq = 0;
        append_emit(
            &TestSink,
            &storage,
            &mut projector,
            &run_id,
            &mut seq,
            "run.started",
            json!({}),
            Some("owner"),
        )
        .unwrap();
        ingest(&storage, &run_id, 2, "first.txt", b"first attachment");
        ingest(&storage, &run_id, 3, "second.txt", b"second attachment");
        for event in storage
            .lock()
            .unwrap()
            .journal
            .events(&run_id)
            .unwrap()
            .into_iter()
            .skip(1)
        {
            projector.apply(&event).unwrap();
        }
        seq = 3;
        std::fs::write(root.join("session.jsonl"), "{}\n").unwrap();
        let (locator, _) = crate::sidecar::validate_pi_session(&root, "session.jsonl").unwrap();

        let (handle, buffered) = coordinate_prepared_prompt(
            &TestSink,
            &storage,
            &mut projector,
            &run_id,
            &mut seq,
            Some("owner"),
            |emit| {
                let mut storage = storage.lock().unwrap();
                let events = storage.journal.events(&run_id).unwrap();
                assert_eq!(events.len(), 3);
                for event in &events[1..] {
                    let EventPayload::Attachment { attachment } = &event.payload else {
                        panic!("attachment payload")
                    };
                    storage.cas.verify(attachment.sha256()).unwrap();
                }
                drop(storage);
                assert!(emit(&PiChatEvent::PromptAccepted)?);
                assert!(emit(&PiChatEvent::TurnStarted)?);
                assert!(emit(&PiChatEvent::TextDelta("early text".into()))?);

                Ok(("handle", locator, vec![PiChatEvent::Completed]))
            },
        )
        .unwrap();

        assert_eq!(handle, "handle");
        assert_eq!(buffered, vec![PiChatEvent::Completed]);
        let events = storage.lock().unwrap().journal.events(&run_id).unwrap();
        assert_eq!(
            events
                .iter()
                .map(|event| event.event_type.as_str())
                .collect::<Vec<_>>(),
            [
                "run.started",
                "chat.attachment.ingested",
                "chat.attachment.ingested",
                "model.prompt.accepted",
                "model.turn.started",
                "model.stream.delta",
                "runtime.pi_session.bound"
            ]
        );
        drop(events);
        drop(storage);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn prepared_pi_images_preserve_journal_order_and_omit_unsupported_files() {
        let (root, storage) = storage();
        let run_id = Uuid::now_v7().to_string();
        let gif_data = "R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==";
        ingest(&storage, &run_id, 1, "renamed.bin", &png());
        ingest(
            &storage,
            &run_id,
            2,
            "notes.png",
            b"durable but not an image",
        );
        ingest(
            &storage,
            &run_id,
            3,
            "second.dat",
            &STANDARD.decode(gif_data).unwrap(),
        );

        assert_eq!(
            prepared_pi_images(&storage, &run_id).unwrap(),
            vec![
                PiImageContent::new(STANDARD.encode(png()), "image/png"),
                PiImageContent::new(gif_data, "image/gif"),
            ]
        );
        drop(storage);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn prepared_pi_prompt_keeps_the_prompt_and_adds_prepared_images() {
        let (root, storage) = storage();
        let run_id = Uuid::now_v7().to_string();
        ingest(&storage, &run_id, 1, "image.png", &png());

        let command = prepared_pi_prompt(&storage, &run_id, "Describe this image.").unwrap();

        assert_eq!(command.message, "Describe this image.");
        assert_eq!(
            command.images,
            vec![PiImageContent::new(STANDARD.encode(png()), "image/png")]
        );

        let text_only = prepared_pi_prompt(&storage, "run-without-images", "Hello.").unwrap();
        assert_eq!(text_only, PromptCommand::new("Hello."));
        drop(storage);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resume_attempt_reports_acceptance_and_every_early_return() {
        let (sender, receiver) = std::sync::mpsc::channel();
        drop(ResumeAttempt::new(Some(sender)));
        assert_eq!(
            receiver.recv().unwrap().unwrap_err(),
            "This reply could not be resumed. Try again."
        );

        let (sender, receiver) = std::sync::mpsc::channel();
        let mut attempt = ResumeAttempt::new(Some(sender));
        attempt.accepted();
        drop(attempt);
        assert_eq!(receiver.recv().unwrap(), Ok(()));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn delivery_errors_name_the_limit_and_recovery() {
        assert_eq!(
            attachment_delivery_error(AttachmentDeliveryError::ImageSizeLimit {
                display_name: "photo.jpg".into(),
            }),
            "photo.jpg exceeds the 10 MB image limit. Choose a smaller image before sending again."
        );
        assert_eq!(
            attachment_delivery_error(AttachmentDeliveryError::ImageCountLimit {
                display_name: "eleventh.png".into(),
            }),
            "eleventh.png crosses the 10-image limit. Remove an image before sending again."
        );
        assert_eq!(
            attachment_delivery_error(AttachmentDeliveryError::ImageTotalSizeLimit {
                display_name: "last.gif".into(),
            }),
            "last.gif crosses the 20 MB total image limit. Remove images or choose smaller images before sending again."
        );
        assert_eq!(
            attachment_delivery_error(AttachmentDeliveryError::AmbiguousFormat {
                display_name: "unclear.webp".into(),
            }),
            "unclear.webp has an image format Muniment cannot verify. Choose a PNG, JPEG, GIF, or WebP image before sending again."
        );
    }

    #[test]
    fn storage_and_stored_length_errors_keep_the_generic_message() {
        assert_eq!(
            attachment_delivery_error(AttachmentDeliveryError::Storage(
                crate::cas::CasError::InvalidHash("invalid".into()),
            )),
            attachment_error()
        );
        assert_eq!(
            attachment_delivery_error(AttachmentDeliveryError::InvalidStoredLength),
            attachment_error()
        );
    }
}
