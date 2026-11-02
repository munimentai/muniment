use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex, Once};
use std::time::{Duration, Instant};

use serde_json::json;

use crate::active_run::RunRegistry;
use crate::attachment::{prepare_pi_images, AttachmentDeliveryError};
use crate::chat_launch::ChatGrant;
use crate::engine::{AgentEngine, ContextUsage, OpenError, PiLaunch, PiOpen, PiRpcEngine};
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
    // A session another swarm member bound, whose file did not reach this
    // runtime, gives way to a new session; the thread keeps its history.
    let replicated = binding
        .provenance
        .extra
        .contains_key(crate::swarm::SWARM_ORIGIN);
    match crate::sidecar::validate_pi_session(session_root, locator) {
        Ok((locator, _)) => Ok(Some(locator)),
        Err(_) if replicated => Ok(None),
        Err(_) => Err("Conversation history is unavailable. Start a new thread.".into()),
    }
}

pub struct PiRuntime {
    pub supervisor: SidecarSupervisor,
    pub wiring: PiRpcWiring,
    /// Set while this idle process may take its thread's next reply.
    pub reuse: Option<crate::engine::PiReuse>,
    /// What keeps the files the process started with, such as the tool
    /// extension a read wrote, until the process ends.
    pub launch_files: Option<LaunchFiles>,
}

/// A hold on the files a process started with. Dropping it lets them go.
pub type LaunchFiles = Box<dyn Send + Sync>;

/// The engine processes of a runtime, one slot per thread. A run takes its
/// thread's slot for its whole length, so runs in different threads drive
/// different processes at once. After a run, its process idles in the slot
/// for the thread's next read or run. Once more than `idle` slots are kept,
/// empty slots go first, then read-only processes, then reply processes. A
/// reaper stops each marked idle process
/// when its idle window ends, and the oldest ones once more than the parked
/// limit wait. A slot a run holds is never touched, so no turn is stopped.
/// One thread's process slot.
pub type PiProcessSlot = Arc<Mutex<Option<PiRuntime>>>;

/// How many marked idle processes a runtime keeps at once.
pub const PARKED_LIMIT: usize = 3;

/// Why the slot manager stopped an idle process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdleStop {
    /// The process waited past its idle window.
    Idle,
    /// Its thread or the agent that owns the thread was deleted.
    Deleted,
    /// More marked processes waited than the parked limit.
    Limit,
    /// The runtime drains or stops.
    Shutdown,
    /// A new process took its slot, as a read or a reply found it unfit.
    Replaced,
    /// Its slot left the slot list, as more slots were kept than the limit.
    Evicted,
    /// A read started it and it could not wait for its thread's next reply.
    Unparked,
}

impl IdleStop {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Deleted => "deleted",
            Self::Limit => "limit",
            Self::Shutdown => "shutdown",
            Self::Replaced => "replaced",
            Self::Evicted => "evicted",
            Self::Unparked => "unparked",
        }
    }
}

/// Counts the moments a process may have gone idle, so each reaper looks
/// again then.
static IDLE_TURNS: (Mutex<u64>, Condvar) = (Mutex::new(0), Condvar::new());

fn idle_turns() -> u64 {
    *IDLE_TURNS
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Wakes every reaper to look at its slots again.
pub fn wake_reapers() {
    let mut turns = IDLE_TURNS
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *turns = turns.wrapping_add(1);
    IDLE_TURNS.1.notify_all();
}

/// Wakes the reapers when it drops. A run holds one from before it locks
/// its slot, so the wake comes after the slot is free again.
pub struct ReaperWake;

impl Drop for ReaperWake {
    fn drop(&mut self) {
        wake_reapers();
    }
}

/// Waits until a wake after `seen`, or until `deadline`.
fn wait_for_wake(seen: u64, deadline: Option<Instant>) {
    let mut turns = IDLE_TURNS
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    while *turns == seen {
        match deadline {
            None => {
                turns = IDLE_TURNS
                    .1
                    .wait(turns)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
            Some(deadline) => {
                let now = Instant::now();
                if now >= deadline {
                    return;
                }
                turns = IDLE_TURNS
                    .1
                    .wait_timeout(turns, deadline - now)
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .0;
            }
        }
    }
}

struct Slots {
    list: Mutex<Vec<(String, PiProcessSlot)>>,
    idle: usize,
    window: Duration,
    parked: usize,
    reaper: Once,
}

impl Drop for Slots {
    fn drop(&mut self) {
        // The reaper ends once it finds the slots gone.
        wake_reapers();
    }
}

#[derive(Clone)]
pub struct PiProcesses {
    slots: Arc<Slots>,
}

impl Default for PiProcesses {
    fn default() -> Self {
        Self::with_idle_limit(crate::agent_runs::max_parallel_runs() + 1)
    }
}

impl PiProcesses {
    pub fn with_idle_limit(idle: usize) -> Self {
        Self::with_reaper(idle, crate::engine::REUSE_IDLE_WINDOW, PARKED_LIMIT)
    }

    /// Slots whose marked idle processes stop after `window`, and of which
    /// at most `parked` keep a marked idle process.
    pub fn with_reaper(idle: usize, window: Duration, parked: usize) -> Self {
        Self {
            slots: Arc::new(Slots {
                list: Mutex::new(Vec::new()),
                idle: idle.max(1),
                window,
                parked: parked.max(1),
                reaper: Once::new(),
            }),
        }
    }

    fn list(&self) -> std::sync::MutexGuard<'_, Vec<(String, PiProcessSlot)>> {
        self.slots
            .list
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The slot of one thread's process. Past the slot limit, the slots that
    /// go are empty ones first, then those of processes only a read started,
    /// then those a reply left, the longest idle of each first. The slot just
    /// asked for and a slot a run holds stay.
    pub fn slot(&self, thread: &str) -> PiProcessSlot {
        self.start_reaper();
        let mut slots = self.list();
        let slot = match slots.iter().position(|(held, _)| held == thread) {
            Some(index) => slots.remove(index).1,
            None => Arc::new(Mutex::new(None)),
        };
        slots.push((thread.to_owned(), Arc::clone(&slot)));
        let excess = slots.len().saturating_sub(self.slots.idle);
        let mut evicted = Vec::new();
        if excess > 0 {
            let newest = slots.len() - 1;
            let mut order: Vec<_> = slots[..newest]
                .iter()
                .enumerate()
                .filter_map(|(index, (_, held))| {
                    // A slot a run holds stays; its process ends with the run's next start.
                    let process = held.try_lock().ok()?;
                    let rank = match process.as_ref().map(|process| process.reuse.as_ref()) {
                        None => (0, None),
                        Some(Some(reuse)) if reuse.read_only() => (1, Some(reuse.parked_at())),
                        Some(reuse) => (2, reuse.map(crate::engine::PiReuse::parked_at)),
                    };
                    Some((rank, index))
                })
                .collect();
            order.sort();
            let mut gone = Vec::new();
            for (_, index) in order {
                if gone.len() == excess {
                    break;
                }
                let Ok(mut process) = slots[index].1.try_lock() else {
                    continue;
                };
                let taken = process.take();
                drop(process);
                gone.push(index);
                evicted.extend(taken.map(|process| (slots[index].0.clone(), process)));
            }
            gone.sort_unstable();
            for index in gone.into_iter().rev() {
                slots.remove(index);
            }
        }
        drop(slots);
        for (held, process) in evicted {
            stop_idle(&held, process, IdleStop::Evicted);
        }
        slot
    }

    /// How many thread slots are kept now.
    pub fn len(&self) -> usize {
        self.list().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How many marked idle processes wait now.
    pub fn parked(&self) -> usize {
        let slots: Vec<_> = self
            .list()
            .iter()
            .map(|(_, slot)| Arc::clone(slot))
            .collect();
        slots
            .iter()
            .filter(|slot| {
                slot.try_lock()
                    .is_ok_and(|process| parked_at(&process).is_some())
            })
            .count()
    }

    /// Stops the idle process of each named thread and forgets its slot. A
    /// slot a run holds keeps its process.
    pub fn stop_threads<'a>(&self, threads: impl IntoIterator<Item = &'a str>, reason: IdleStop) {
        for thread in threads {
            let slot = {
                let mut slots = self.list();
                let Some(index) = slots.iter().position(|(held, _)| held == thread) else {
                    continue;
                };
                let slot = Arc::clone(&slots[index].1);
                let Ok(mut process) = slot.try_lock() else {
                    continue;
                };
                let taken = process.take();
                drop(process);
                slots.remove(index);
                taken
            };
            if let Some(process) = slot {
                stop_idle(thread, process, reason);
            }
        }
    }

    /// Stops every idle process, as the runtime drains or stops. A slot a
    /// run holds keeps its process.
    pub fn stop_all(&self, reason: IdleStop) {
        let threads: Vec<String> = self
            .list()
            .iter()
            .map(|(thread, _)| thread.clone())
            .collect();
        self.stop_threads(threads.iter().map(String::as_str), reason);
    }

    fn start_reaper(&self) {
        self.slots.reaper.call_once(|| {
            let slots = Arc::downgrade(&self.slots);
            let _ = std::thread::Builder::new()
                .name("pi-reaper".into())
                .spawn(move || reap(slots));
        });
    }
}

/// When the process in a slot was marked idle, if it is.
fn parked_at(process: &Option<PiRuntime>) -> Option<Instant> {
    process
        .as_ref()
        .and_then(|process| process.reuse.as_ref())
        .map(crate::engine::PiReuse::parked_at)
}

fn stop_idle(thread: &str, mut process: PiRuntime, reason: IdleStop) {
    let _ = process.supervisor.shutdown();
    crate::runtime_eprintln!(
        "muniment-runtime: pi_reuse stopped reason={} thread_id={thread}",
        reason.as_str()
    );
}

/// Stops the idle process a reply found in its slot and cannot take.
pub fn stop_replaced(run_id: &str, mut process: PiRuntime) {
    let _ = process.supervisor.shutdown();
    log_replaced(run_id);
}

/// Names the idle process a reply's reuse check stopped.
pub fn log_replaced(run_id: &str) {
    crate::runtime_eprintln!(
        "muniment-runtime: pi_reuse stopped reason={} run_id={run_id}",
        IdleStop::Replaced.as_str()
    );
}

/// Stops each marked idle process whose window ended, then the ones past
/// the parked limit, read-only ones first, and sleeps until the next window ends or a run
/// frees its slot.
fn reap(slots: std::sync::Weak<Slots>) {
    loop {
        let seen = idle_turns();
        let Some(held) = slots.upgrade() else {
            return;
        };
        let next = sweep(&held, Instant::now());
        drop(held);
        wait_for_wake(seen, next);
    }
}

fn sweep(slots: &Slots, now: Instant) -> Option<Instant> {
    let list: Vec<_> = slots
        .list
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let take_if = |slot: &PiProcessSlot, keep: &dyn Fn(Instant) -> bool| {
        let mut process = slot.try_lock().ok()?;
        let at = parked_at(&process)?;
        if keep(at) {
            return None;
        }
        process.take()
    };
    let mut parked = Vec::new();
    for (thread, slot) in list {
        let expired = |at: Instant| now.saturating_duration_since(at) >= slots.window;
        if let Some(process) = take_if(&slot, &|at| !expired(at)) {
            stop_idle(&thread, process, IdleStop::Idle);
            continue;
        }
        if let Some((at, read)) = slot.try_lock().ok().and_then(|process| {
            let reuse = process.as_ref()?.reuse.as_ref()?;
            Some((reuse.parked_at(), reuse.read_only()))
        }) {
            parked.push((at, read, thread, slot));
        }
    }
    let next = parked.iter().map(|(at, ..)| *at + slots.window).min();
    // The newest process stays: its thread is the one in use. Of the
    // others, a process only a read used stops before one a reply left,
    // and the oldest of each first.
    parked.sort_by_key(|(at, ..)| *at);
    let newest = parked.pop();
    parked.sort_by_key(|(at, read, ..)| (!*read, *at));
    let excess = (parked.len() + usize::from(newest.is_some())).saturating_sub(slots.parked);
    for (at, _, thread, slot) in parked.drain(..excess) {
        if let Some(process) = take_if(&slot, &|current| current != at) {
            stop_idle(&thread, process, IdleStop::Limit);
        }
    }
    next
}

/// Opens a thread's saved conversation for a read, such as its context
/// usage. The launch is the one a reply in that thread starts with.
pub fn open_thread_conversation(
    app: &impl PiLaunchBoundaries,
    grant: &ChatGrant,
    locator: &PiSessionLocator,
) -> Result<(PiRuntime, Arc<PiRpcTransport>, PiLaunch), String> {
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
        config: config.clone(),
        cancelled: &AtomicBool::new(false),
        observe: &mut |_| {},
    }) {
        // The launch is read once Pi is ready, as a reply reads its own.
        Ok((process, transport)) => Ok((process, transport, PiLaunch::ready(config))),
        Err(OpenError::Spawn(_)) => Err("The agent runtime could not start.".into()),
        Err(_) => Err("The agent runtime did not become ready.".into()),
    }
}

/// Reads a thread's context usage from the process that holds its saved
/// conversation: the active run's when a run is in this thread, the idle
/// process the thread's last run left when it holds this conversation, or a
/// process this read starts. A started process waits in the thread's slot,
/// marked for the thread's next read or reply, so the reaper stops it like
/// any idle process. One that cannot be marked stops after the read. A
/// thread with no saved conversation answers `None`.
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
    // Dropped after the slot's lock, so the reaper sees a process this read
    // leaves marked.
    let _reaper_wake = ReaperWake;
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
            if let Some(process) = slot.take() {
                stop_idle(thread_id, process, IdleStop::Replaced);
            }
            // The read starts the launch the thread's next reply starts, so
            // that reply takes this process.
            let grant = grant()?;
            let launch_files = app.prepare_thread_launch(&grant);
            let (mut process, transport, launch) = open_thread_conversation(app, &grant, &locator)?;
            process.launch_files = launch_files;
            let usage = engine.context_usage(&transport).map(Some);
            engine.park_read(&mut process, launch);
            if process.reuse.is_some() {
                *slot = Some(process);
            } else {
                stop_idle(thread_id, process, IdleStop::Unparked);
            }
            usage
        }
        Err(_) => {
            let (process, transport, _) = open_thread_conversation(app, &grant()?, &locator)?;
            let usage = engine.context_usage(&transport).map(Some);
            stop_idle(thread_id, process, IdleStop::Unparked);
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
        let root = crate::state_root::test_root()
            .join(format!("muniment-pi-execution-{}", Uuid::now_v7()));
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
    fn stopping_idle_processes_names_its_reason_and_leaves_held_slots() {
        assert_eq!(
            [
                IdleStop::Idle,
                IdleStop::Deleted,
                IdleStop::Limit,
                IdleStop::Shutdown,
                IdleStop::Replaced,
                IdleStop::Evicted,
                IdleStop::Unparked
            ]
            .map(IdleStop::as_str),
            ["idle", "deleted", "limit", "shutdown", "replaced", "evicted", "unparked"]
        );
        let processes = PiProcesses::with_idle_limit(4);
        // A thread with no slot gets none from a stop.
        processes.stop_threads(["unknown"], IdleStop::Deleted);
        assert!(processes.is_empty());
        let held = processes.slot("held");
        processes.slot("free");
        let lock = held.lock().unwrap();
        processes.stop_all(IdleStop::Shutdown);
        // The slot a run holds stays, and the free one goes.
        assert_eq!(processes.len(), 1);
        drop(lock);
        assert!(Arc::ptr_eq(&held, &processes.slot("held")));
        assert_eq!(processes.parked(), 0);
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
