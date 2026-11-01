use std::path::Path;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use muniment_core::active_run::RunRegistry;
use muniment_core::attach::RuntimeActivityRegistry;
use muniment_core::pi_execution::PiProcesses;
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
            processes: PiProcesses::default(),
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
        ] {
            std::env::remove_var(variable);
        }
    }
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
