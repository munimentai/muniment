use std::path::Path;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use muniment_core::active_run::RunRegistry;
use muniment_core::attach::RuntimeActivityRegistry;
use muniment_core::pi_execution::{IdleStop, PiProcesses};
use muniment_core::run_events::SharedStorage;
use muniment_core::session_thread::SessionThread;
use muniment_core::sidecar::pi_install::PiArtifactDescriptor;
use muniment_runtime_core::{
    accept_prompt, cancel_run, drive_prompt, open_profile_storage, RuntimeChatEventTarget,
    RuntimeHost,
};

mod common;
use common::{local_grant, stage_pi_stub, use_test_prompt_store, TemporaryProfile, CI_WAIT};

static ENVIRONMENT: Mutex<()> = Mutex::new(());

/// How long the stub takes to answer its first command, as Pi loads its
/// extensions before it does.
const START_DELAY_MS: u64 = 400;

struct Harness {
    temporary: TemporaryProfile,
    descriptor: PiArtifactDescriptor,
    storage: SharedStorage,
    processes: PiProcesses,
    activity: RuntimeActivityRegistry,
    active: RunRegistry,
    session_thread: SessionThread,
    sequence: usize,
}

/// One reply: the thread it ran in, how long it took to reach the stub's
/// prompt answer, and the phase it ended in.
struct Reply {
    thread: String,
    accepted_after: Duration,
    phase: String,
}

impl Harness {
    fn new(name: &str) -> Self {
        Self::with_processes(name, PiProcesses::default())
    }

    fn with_processes(name: &str, processes: PiProcesses) -> Self {
        use_test_prompt_store();
        let temporary = TemporaryProfile::new(name, true);
        let descriptor = stage_pi_stub(&temporary.root);
        std::env::set_var("PI_RESUME_STUB_SPAWNS", temporary.root.join("spawns"));
        std::env::set_var("PI_RESUME_STUB_PROMPT_PIDS", temporary.root.join("prompts"));
        std::env::set_var("PI_RESUME_STUB_START_DELAY_MS", START_DELAY_MS.to_string());
        std::env::set_var("PI_RESUME_STUB_SESSION_HEADER", "1");
        std::env::set_var(
            "PI_RESUME_STUB_PROMPT_FOLDERS",
            temporary.root.join("folders"),
        );
        let storage = open_profile_storage(&temporary.profile).unwrap();
        Self {
            temporary,
            descriptor,
            storage,
            processes,
            activity: RuntimeActivityRegistry::new(),
            active: RunRegistry::default(),
            session_thread: SessionThread::default(),
            sequence: 0,
        }
    }

    fn reply(&mut self, thread: Option<&str>, prompt: &str) -> Reply {
        self.sequence += 1;
        let run_id = format!("018f0000-0000-7000-8000-{:012}", 700 + self.sequence);
        let (sender, events) = mpsc::channel();
        let (accepted, launch) = accept_prompt(
            &RuntimeHost::default(),
            &self.temporary.profile,
            Arc::clone(&self.storage),
            self.processes.clone(),
            &self.activity,
            &self.temporary.config,
            run_id,
            prompt.into(),
            thread.map(str::to_owned),
            &self.session_thread,
            false,
            String::new(),
            None,
            Vec::new(),
            local_grant(),
            self.active.clone(),
            RuntimeChatEventTarget::Subscriber(Some(sender)),
            Some(self.descriptor),
        )
        .unwrap();
        let started = Instant::now();
        let driver = std::thread::spawn(move || drive_prompt(launch));
        let mut accepted_after = None;
        let mut phase = String::new();
        while let Ok(event) = events.recv_timeout(CI_WAIT) {
            if event.prompt_accepted && accepted_after.is_none() {
                accepted_after = Some(started.elapsed());
            }
            phase = event.phase;
        }
        driver.join().unwrap();
        Reply {
            thread: accepted.thread_id,
            accepted_after: accepted_after.unwrap_or(Duration::MAX),
            phase,
        }
    }

    fn spawns(&self) -> Vec<String> {
        lines(&self.temporary.root.join("spawns"))
    }

    /// The process id that answered each prompt, by the prompt's text.
    fn prompt_process(&self, prompt: &str) -> String {
        lines(&self.temporary.root.join("prompts"))
            .into_iter()
            .find_map(|line| {
                let (process, text) = line.split_once(' ')?;
                (text == prompt).then(|| process.to_owned())
            })
            .unwrap_or_else(|| panic!("no process answered {prompt}"))
    }

    /// The folder the process worked in when it answered `prompt`.
    fn prompt_folder(&self, prompt: &str) -> std::path::PathBuf {
        lines(&self.temporary.root.join("folders"))
            .into_iter()
            .find_map(|line| {
                let (text, folder) = line.split_once('\t')?;
                (text == prompt).then(|| std::path::PathBuf::from(folder))
            })
            .unwrap_or_else(|| panic!("no folder answered {prompt}"))
    }

    /// The runtime's attach boundaries over this harness's slots, in local mode.
    fn boundaries(&self) -> muniment_runtime_core::RuntimeAttachBoundaries {
        std::fs::create_dir_all(&self.temporary.config).unwrap();
        std::fs::write(
            self.temporary
                .config
                .join(muniment_core::local_mode::LOCAL_MODE_MARKER),
            [],
        )
        .unwrap();
        muniment_runtime_core::RuntimeAttachBoundaries::new(
            Arc::clone(&self.storage),
            self.active.clone(),
            self.temporary.profile.clone(),
            self.temporary.config.clone(),
            self.processes.clone(),
            Arc::new(
                muniment_core::memory_runtime::ApplicationMemoryRuntime::new(
                    self.temporary.config.clone(),
                    self.temporary.profile.join("memory"),
                ),
            ),
            self.activity.clone(),
            RuntimeHost::default(),
            muniment_core::attach::SignedWorkspaceApproval::default(),
            Arc::new(SessionThread::default()),
            muniment_runtime_core::open_companion_registry(&self.temporary.profile).unwrap(),
        )
        .with_pi_artifact(self.descriptor)
    }

    /// Reads a thread's context usage, as opening the thread does, and
    /// waits for the answer.
    fn read_context(&self, thread: &str) {
        use muniment_core::run_start::RunAttachBoundaries;

        let boundaries = self.boundaries();
        let deadline = Instant::now() + CI_WAIT;
        loop {
            let snapshot = boundaries.thread_context_usage("local", thread).unwrap();
            if !snapshot.reading && snapshot.usage.is_some() {
                return;
            }
            assert!(Instant::now() < deadline, "{snapshot:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn agent_directory(&self) -> std::path::PathBuf {
        let state = muniment_core::state_root::state_directory().unwrap();
        muniment_core::state_root::agent_directory(&state)
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        for variable in [
            "PI_RESUME_STUB_SPAWNS",
            "PI_RESUME_STUB_PROMPT_PIDS",
            "PI_RESUME_STUB_START_DELAY_MS",
            "PI_RESUME_STUB_SESSION_HEADER",
            "PI_RESUME_STUB_PROMPT_FOLDERS",
            "PI_RESUME_STUB_PROMPT_DELAY_MS",
        ] {
            std::env::remove_var(variable);
        }
    }
}

/// Whether the process with this id still runs.
fn alive(pid: &str) -> bool {
    std::process::Command::new("kill")
        .args(["-0", pid])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Waits until the process with this id ends, and answers whether it did.
fn ends(pid: &str) -> bool {
    let deadline = Instant::now() + CI_WAIT;
    while Instant::now() < deadline {
        if !alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_threads_next_reply_takes_its_idle_process_until_its_launch_changes() {
    let _environment = ENVIRONMENT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut harness = Harness::new("pi-reuse");

    // The first reply names the thread's folder after it ends. Its idle
    // process follows the renamed folder and takes the second reply.
    let first = harness.reply(None, "first");
    assert_eq!(first.phase, "complete");
    let thread = first.thread.clone();
    let renamed = harness.reply(Some(&thread), "second");
    assert_eq!(renamed.phase, "complete");
    assert_eq!(harness.spawns().len(), 1, "{:?}", harness.spawns());
    let process = harness.prompt_process("first");
    assert_eq!(harness.prompt_process("second"), process);
    let started = harness.prompt_folder("first");
    let moved = harness.prompt_folder("second");
    assert!(started
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("Untitled-"));
    assert_ne!(started, moved);
    assert!(!started.exists());
    assert!(moved.is_dir());
    assert_eq!(
        moved.parent().unwrap().canonicalize().unwrap(),
        started.parent().unwrap().canonicalize().unwrap()
    );

    // The next replies take the idle process, and start without waiting on Pi.
    let warm = harness.reply(Some(&thread), "third");
    let again = harness.reply(Some(&thread), "fourth");
    assert_eq!(
        (warm.phase.as_str(), again.phase.as_str()),
        ("complete", "complete")
    );
    assert_eq!(harness.spawns().len(), 1, "{:?}", harness.spawns());
    assert_eq!(harness.prompt_process("third"), process);
    assert_eq!(harness.prompt_process("fourth"), process);
    assert_eq!(harness.prompt_folder("fourth"), moved);
    eprintln!(
        "pi_reuse latency cold_ms={} renamed_ms={} warm_ms={} warm_again_ms={}",
        first.accepted_after.as_millis(),
        renamed.accepted_after.as_millis(),
        warm.accepted_after.as_millis(),
        again.accepted_after.as_millis()
    );
    assert!(first.accepted_after >= Duration::from_millis(START_DELAY_MS));
    assert!(renamed.accepted_after < Duration::from_millis(START_DELAY_MS));
    assert!(warm.accepted_after < Duration::from_millis(START_DELAY_MS));

    // A file Pi reads at start that changed starts a new process.
    std::fs::create_dir_all(harness.agent_directory()).unwrap();
    std::fs::write(harness.agent_directory().join("AGENTS.md"), "Be brief.").unwrap();
    assert_eq!(harness.reply(Some(&thread), "fifth").phase, "complete");
    assert_eq!(harness.spawns().len(), 2);
    assert_ne!(harness.prompt_process("fifth"), process);

    // A model change applies to the next reply in a new process.
    std::fs::write(
        harness.agent_directory().join("settings.json"),
        r#"{"defaultProvider":"stub","defaultModel":"other-model"}"#,
    )
    .unwrap();
    assert_eq!(harness.reply(Some(&thread), "sixth").phase, "complete");
    assert_eq!(harness.spawns().len(), 3);
    let model_process = harness.prompt_process("sixth");
    assert_eq!(harness.reply(Some(&thread), "seventh").phase, "complete");
    assert_eq!(harness.prompt_process("seventh"), model_process);
    assert_eq!(harness.spawns().len(), 3);

    // An idle process that died is replaced, and the reply still answers.
    // The supervisor's restart of the dead process is a new generation the
    // reply never takes, because it holds no conversation of the thread.
    let pid: i32 = model_process.parse().unwrap();
    assert!(std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()
        .unwrap()
        .success());
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(harness.reply(Some(&thread), "eighth").phase, "complete");
    let replaced = harness.prompt_process("eighth");
    assert_ne!(replaced, model_process);
    assert_eq!(harness.spawns().last(), Some(&replaced));
}

#[test]
fn a_stopped_reply_leaves_no_process_and_threads_never_share_one() {
    let _environment = ENVIRONMENT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut harness = Harness::new("pi-reuse-stop");
    let first = harness.reply(None, "a first");
    let thread_a = first.thread.clone();
    harness.reply(Some(&thread_a), "a second");
    let other = harness.reply(None, "b first");
    let thread_b = other.thread.clone();
    assert_ne!(thread_a, thread_b);
    harness.reply(Some(&thread_b), "b second");
    assert_eq!(harness.spawns().len(), 2);

    // Each thread's next reply takes its own idle process.
    harness.reply(Some(&thread_a), "a third");
    harness.reply(Some(&thread_b), "b third");
    assert_eq!(harness.spawns().len(), 2);
    let process_a = harness.prompt_process("a third");
    let process_b = harness.prompt_process("b third");
    assert_ne!(process_a, process_b);
    assert_eq!(harness.prompt_process("a second"), process_a);
    assert_eq!(harness.prompt_process("b second"), process_b);

    // A reply the user stops leaves its process unmarked, so the thread's
    // next reply starts a new one.
    let active = harness.active.clone();
    let held_thread = thread_a.clone();
    let prompts = harness.temporary.root.join("prompts");
    let stopper = std::thread::spawn(move || {
        let deadline = Instant::now() + CI_WAIT;
        while Instant::now() < deadline {
            let asked = lines(&prompts)
                .iter()
                .any(|line| line.ends_with("[hold the run] a held"));
            if let Some(run) = active.thread_run(&held_thread).filter(|_| asked) {
                return cancel_run(active.clone(), local_grant().workspace, run);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Err("the held reply never reached the process".into())
    });
    let held = harness.reply(Some(&thread_a), "[hold the run] a held");
    stopper.join().unwrap().unwrap();
    assert_ne!(held.phase, "complete");
    assert_eq!(harness.prompt_process("[hold the run] a held"), process_a);
    assert_eq!(harness.reply(Some(&thread_a), "a after").phase, "complete");
    assert_ne!(harness.prompt_process("a after"), process_a);
    assert_eq!(harness.spawns().len(), 3);
}

#[test]
fn an_idle_process_stops_when_its_window_ends_and_never_during_a_turn() {
    let _environment = ENVIRONMENT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let window = Duration::from_millis(1000);
    let processes = PiProcesses::with_reaper(4, window, 3);
    let mut harness = Harness::with_processes("pi-reuse-idle", processes);
    // Each turn runs twice as long as the idle window. The stub reads the
    // delay when it starts.
    std::env::set_var(
        "PI_RESUME_STUB_PROMPT_DELAY_MS",
        (window.as_millis() * 2).to_string(),
    );
    let first = harness.reply(None, "first");
    assert_eq!(first.phase, "complete");
    let thread = first.thread.clone();
    let process = harness.prompt_process("first");
    assert_eq!(harness.processes.parked(), 1);

    // A turn that runs past the window of the process it took keeps it.
    assert_eq!(harness.reply(Some(&thread), "slow").phase, "complete");
    assert_eq!(harness.prompt_process("slow"), process);
    assert_eq!(harness.spawns().len(), 1);
    assert!(alive(&process));

    // Once the process waits past its window, it stops.
    assert!(ends(&process), "the idle process still runs");
    assert_eq!(harness.processes.parked(), 0);
    assert_eq!(harness.reply(Some(&thread), "after").phase, "complete");
    assert_ne!(harness.prompt_process("after"), process);
    assert_eq!(harness.spawns().len(), 2);
}

#[test]
fn a_deleted_thread_stops_its_idle_process() {
    use muniment_core::run_start::RunAttachBoundaries;

    let _environment = ENVIRONMENT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut harness = Harness::new("pi-reuse-delete");
    std::fs::create_dir_all(&harness.temporary.config).unwrap();
    std::fs::write(
        harness
            .temporary
            .config
            .join(muniment_core::local_mode::LOCAL_MODE_MARKER),
        [],
    )
    .unwrap();
    let kept = harness.reply(None, "kept");
    let deleted = harness.reply(None, "deleted");
    let kept_process = harness.prompt_process("kept");
    let deleted_process = harness.prompt_process("deleted");
    assert_eq!(harness.processes.parked(), 2);

    let boundaries = muniment_runtime_core::RuntimeAttachBoundaries::new(
        Arc::clone(&harness.storage),
        harness.active.clone(),
        harness.temporary.profile.clone(),
        harness.temporary.config.clone(),
        harness.processes.clone(),
        Arc::new(
            muniment_core::memory_runtime::ApplicationMemoryRuntime::new(
                harness.temporary.config.clone(),
                harness.temporary.profile.join("memory"),
            ),
        ),
        harness.activity.clone(),
        RuntimeHost::default(),
        muniment_core::attach::SignedWorkspaceApproval::default(),
        Arc::new(SessionThread::default()),
        muniment_runtime_core::open_companion_registry(&harness.temporary.profile).unwrap(),
    );
    let provenance = muniment_core::journal::Provenance {
        source: "muniment-attach".into(),
        source_version: "test".into(),
        actor_id: None,
        device_id: None,
        rpc_request_id: None,
        capability_versions: None,
        extra: Default::default(),
    };
    boundaries
        .delete_thread(&deleted.thread, provenance)
        .unwrap();
    assert!(!alive(&deleted_process));
    assert!(alive(&kept_process));
    assert_eq!(harness.processes.parked(), 1);
    assert_eq!(
        harness.reply(Some(&kept.thread), "kept again").phase,
        "complete"
    );
    assert_eq!(harness.prompt_process("kept again"), kept_process);
}

#[test]
fn the_oldest_idle_process_stops_past_the_parked_limit() {
    let _environment = ENVIRONMENT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let processes = PiProcesses::with_reaper(8, Duration::from_secs(600), 2);
    let mut harness = Harness::with_processes("pi-reuse-limit", processes);
    let mut threads = Vec::new();
    for prompt in ["one", "two", "three"] {
        let reply = harness.reply(None, prompt);
        assert_eq!(reply.phase, "complete");
        threads.push(reply.thread);
    }
    let one = harness.prompt_process("one");
    assert!(ends(&one), "the oldest idle process still runs");
    assert!(alive(&harness.prompt_process("two")));
    assert!(alive(&harness.prompt_process("three")));
    assert_eq!(harness.processes.parked(), 2);

    // The limit counts idle processes only: a thread's next reply takes its own.
    assert_eq!(
        harness.reply(Some(&threads[2]), "three again").phase,
        "complete"
    );
    assert_eq!(
        harness.prompt_process("three again"),
        harness.prompt_process("three")
    );
    assert_eq!(harness.spawns().len(), 3);
}

/// A thread with a saved conversation and no process: its first reply ran,
/// and its process stopped since.
fn thread_without_process(harness: &mut Harness) -> String {
    let first = harness.reply(None, "first");
    assert_eq!(first.phase, "complete");
    harness.processes.stop_all(IdleStop::Shutdown);
    assert!(ends(&harness.prompt_process("first")));
    assert!(harness.processes.is_empty());
    first.thread
}

#[test]
fn opening_a_thread_starts_the_process_its_next_reply_takes() {
    let _environment = ENVIRONMENT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut harness = Harness::new("pi-reuse-context");
    let thread = thread_without_process(&mut harness);

    // Opening the thread reads its context in a new process, which waits
    // marked in the thread's slot, never unmarked.
    harness.read_context(&thread);
    assert_eq!(harness.spawns().len(), 2, "{:?}", harness.spawns());
    let read = harness.spawns().last().unwrap().clone();
    assert!(alive(&read));
    assert_eq!(harness.processes.len(), 1);
    assert_eq!(harness.processes.parked(), 1);

    // The read started the launch the reply starts, so the next read and
    // the first reply take that process.
    harness.read_context(&thread);
    assert_eq!(harness.spawns().len(), 2);
    let reply = harness.reply(Some(&thread), "second");
    assert_eq!(reply.phase, "complete");
    assert_eq!(harness.prompt_process("second"), read);
    assert_eq!(harness.spawns().len(), 2);
    assert!(reply.accepted_after < Duration::from_millis(START_DELAY_MS));
    assert_eq!(harness.processes.parked(), 1);
}

#[test]
fn a_reply_replaces_a_context_reads_process_whose_launch_differs() {
    let _environment = ENVIRONMENT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut harness = Harness::new("pi-reuse-context-replace");
    let thread = thread_without_process(&mut harness);

    // A file Pi reads at start changes after the read, so the reply stops
    // the read's process and starts its own.
    harness.read_context(&thread);
    let read = harness.spawns().last().unwrap().clone();
    assert_eq!(harness.processes.parked(), 1);
    // The agent directory outlives this harness, so the file goes again.
    let rules = harness.agent_directory().join("AGENTS.md");
    std::fs::create_dir_all(harness.agent_directory()).unwrap();
    std::fs::write(&rules, "Answer in one line.").unwrap();
    let reply = harness.reply(Some(&thread), "second");
    std::fs::remove_file(&rules).unwrap();
    assert_eq!(reply.phase, "complete");
    assert_ne!(harness.prompt_process("second"), read);
    assert!(ends(&read), "the replaced process still runs");
    assert_eq!(harness.spawns().len(), 3);
    assert_eq!(harness.processes.parked(), 1);
}

#[test]
fn past_the_parked_limit_a_read_only_process_stops_before_a_replys() {
    let _environment = ENVIRONMENT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let processes = PiProcesses::with_reaper(8, Duration::from_secs(600), 2);
    let mut harness = Harness::with_processes("pi-reuse-context-limit", processes);
    let opened = thread_without_process(&mut harness);

    // A reply leaves the oldest process, a read the next one, and another
    // reply the newest. The read's process stops, though it is not the oldest.
    assert_eq!(harness.reply(None, "oldest").phase, "complete");
    harness.read_context(&opened);
    let read = harness.spawns().last().unwrap().clone();
    assert_eq!(harness.processes.parked(), 2);
    assert_eq!(harness.reply(None, "newest").phase, "complete");
    assert!(ends(&read), "the read-only process still runs");
    assert!(alive(&harness.prompt_process("oldest")));
    assert!(alive(&harness.prompt_process("newest")));
    assert_eq!(harness.processes.parked(), 2);

    // A read's process a reply took counts once, as that reply's process.
    harness.read_context(&opened);
    let taken = harness.spawns().last().unwrap().clone();
    assert_eq!(
        harness.reply(Some(&opened), "opened again").phase,
        "complete"
    );
    assert_eq!(harness.prompt_process("opened again"), taken);
    assert!(ends(&harness.prompt_process("oldest")));
    assert!(alive(&harness.prompt_process("newest")));
    assert!(alive(&taken));
    assert_eq!(harness.processes.parked(), 2);
}

#[test]
fn a_context_reads_process_stops_when_its_window_ends() {
    let _environment = ENVIRONMENT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let processes = PiProcesses::with_reaper(4, Duration::from_millis(1000), 3);
    let mut harness = Harness::with_processes("pi-reuse-context-idle", processes);
    let thread = thread_without_process(&mut harness);

    harness.read_context(&thread);
    let read = harness.spawns().last().unwrap().clone();
    assert_eq!(harness.spawns().len(), 2);
    assert_eq!(harness.processes.parked(), 1);
    assert!(ends(&read), "the context read's process still runs");
    assert_eq!(harness.processes.parked(), 0);

    // Shutdown stops a read's process like any idle one.
    harness.read_context(&thread);
    let again = harness.spawns().last().unwrap().clone();
    assert_ne!(again, read);
    assert!(alive(&again));
    harness.processes.stop_all(IdleStop::Shutdown);
    assert!(ends(&again), "shutdown left the context read's process");
}

#[test]
fn past_the_slot_limit_empty_slots_go_then_read_only_processes_then_the_oldest_reply() {
    let _environment = ENVIRONMENT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let processes = PiProcesses::with_reaper(4, Duration::from_secs(600), 8);
    let mut harness = Harness::with_processes("pi-reuse-slots", processes);
    let first_read = thread_without_process(&mut harness);
    let second_read = thread_without_process(&mut harness);

    // The user replies in one thread, opens two others, and one slot stays
    // empty. The reply's slot is the oldest.
    assert_eq!(harness.reply(None, "replied").phase, "complete");
    let replied = harness.prompt_process("replied");
    harness.read_context(&first_read);
    let first = harness.spawns().last().unwrap().clone();
    harness.read_context(&second_read);
    let second = harness.spawns().last().unwrap().clone();
    harness.processes.slot("empty");
    assert_eq!(harness.processes.len(), 4);
    assert_eq!(harness.processes.parked(), 3);

    // A new thread's slot pushes the empty slot out, though it is the newest.
    assert_eq!(harness.reply(None, "next").phase, "complete");
    assert_eq!(harness.processes.len(), 4);
    assert_eq!(harness.processes.parked(), 4);
    assert!(alive(&replied) && alive(&first) && alive(&second));

    // Then the read-only processes stop, the oldest first, while the
    // thread the user replied in keeps its process.
    assert_eq!(harness.reply(None, "later").phase, "complete");
    assert!(ends(&first), "the oldest read-only process still runs");
    assert!(alive(&second) && alive(&replied));
    assert_eq!(harness.reply(None, "latest").phase, "complete");
    assert!(ends(&second), "the read-only process still runs");
    assert!(alive(&replied));

    // With no read-only process left, the oldest reply's process goes.
    assert_eq!(harness.reply(None, "last").phase, "complete");
    assert!(ends(&replied), "the oldest reply process still runs");
    for prompt in ["next", "later", "latest", "last"] {
        assert!(alive(&harness.prompt_process(prompt)), "{prompt}");
    }
    assert_eq!(harness.processes.len(), 4);
}
