#![cfg(unix)]
//! The agent run queue: parallel runs with one run per thread, leases
//! reconciled against the journal, silent routine replies, runs stuck at a
//! question, an agent's settings at launch, the owner of the threads the
//! queue creates, and the permission mode a new thread's first message picks.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use muniment_core::active_run::{queue_message, ChatDelivery, ChatQueueRequest};
use muniment_core::agent_runs::{
    self, NewRun, Run, RunStatus, RunStore, RunTrigger, TriggerKind, MAX_ATTEMPTS,
};
use muniment_core::agents::{self, Agent, CronSchedule, Routine, Trigger};
use muniment_core::serde_json::{self, json, Value};
use muniment_core::sidecar::pi_install::PiArtifactDescriptor;
use muniment_runtime_core::{open_profile_storage, RuntimeAttachState, RuntimeHost};

mod common;
use common::TemporaryProfile;

/// The tests change process-wide stub settings, so they run one at a time.
static ENVIRONMENT: Mutex<()> = Mutex::new(());

const STUB_SETTINGS: [&str; 11] = [
    "PI_RESUME_STUB_AGENT_GATE",
    "PI_RESUME_STUB_PERMISSION_MODE",
    "PI_RESUME_STUB_PLAN",
    "PI_RESUME_STUB_STEER_CAPTURE",
    "PI_RESUME_STUB_PERMISSION_CAPTURE",
    "PI_RESUME_STUB_REPLY",
    "PI_RESUME_STUB_EDITOR_REQUEST",
    "PI_RESUME_STUB_EDITOR_CAPTURE",
    "PI_RESUME_STUB_ARGS",
    "PI_RESUME_STUB_REQUESTS",
    "PI_RESUME_STUB_REASONING",
];

struct Setup {
    temporary: TemporaryProfile,
    state: Arc<RuntimeAttachState>,
    artifact: PiArtifactDescriptor,
}

impl Setup {
    fn new(name: &str, local: bool, host: RuntimeHost) -> Self {
        for name in STUB_SETTINGS {
            std::env::remove_var(name);
        }
        let temporary = TemporaryProfile::new(name, false);
        let profile = &temporary.profile;
        muniment_core::home::confirm_home(profile, &temporary.root.join("muniment")).unwrap();
        if local {
            std::fs::write(
                profile.join(muniment_core::local_mode::LOCAL_MODE_MARKER),
                "1",
            )
            .unwrap();
        }
        common::use_test_prompt_store();
        let artifact = common::stage_pi_stub(profile);
        let state = Arc::new(RuntimeAttachState::open(host, profile, profile).unwrap());
        Self {
            temporary,
            state,
            artifact,
        }
    }

    fn profile(&self) -> &Path {
        &self.temporary.profile
    }

    fn store(&self) -> RunStore {
        RunStore::open(self.profile()).unwrap()
    }

    fn tick(&self) {
        self.state
            .check_agent_schedules_with(self.state.boundaries().with_pi_artifact(self.artifact))
            .unwrap();
    }

    fn agent(&self, name: &str) -> Agent {
        agents::save(
            self.profile(),
            Agent {
                name: name.into(),
                instructions: format!("{name} does its work."),
                ..Agent::default()
            },
        )
        .unwrap()
    }

    fn run(&self, id: &str) -> Run {
        self.store().get(id).unwrap().unwrap()
    }

    fn live(&self) -> usize {
        self.state.run_registry().len()
    }

    /// Ends a run the stub holds open, through its steering input.
    fn finish(&self, run_id: &str) {
        let registry = self.state.run_registry();
        assert!(
            wait(|| queue_message(
                &registry,
                ChatQueueRequest {
                    run_id: run_id.into(),
                    workspace: None,
                    delivery: ChatDelivery::Steer,
                    message: "Finish now.".into(),
                },
            )
            .is_ok()),
            "the run never took steering input"
        );
        assert!(wait(|| !registry.contains(run_id)), "the run never ended");
    }
}

impl Drop for Setup {
    fn drop(&mut self) {
        // Runs the stub still holds end with the test.
        let registry = self.state.run_registry();
        for identity in registry.identities() {
            let _ = muniment_core::active_run::cancel_active_run(&registry, &identity.run_id, None);
        }
        wait(|| registry.is_empty());
        for name in STUB_SETTINGS {
            std::env::remove_var(name);
        }
    }
}

fn wait(mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

fn enqueue(store: &RunStore, agent_id: &str, kind: TriggerKind) -> Run {
    store
        .enqueue(NewRun {
            agent_id: agent_id.into(),
            thread_id: None,
            trigger: RunTrigger {
                kind,
                routine_id: None,
                source: None,
            },
            prompt: "Do the work.".into(),
            hop: 0,
            origin_run_id: None,
        })
        .unwrap()
}

#[test]
fn three_runs_run_at_once_in_their_own_processes_and_a_fourth_waits() {
    let _environment = ENVIRONMENT
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let setup = Setup::new("agent-runs-parallel", true, RuntimeHost::default());
    let held = setup.temporary.root.join("steer.json");
    std::env::set_var("PI_RESUME_STUB_STEER_CAPTURE", &held);
    let events = agent_runs::subscribe(setup.profile());
    let [a, b, c, d] = ["Alpha", "Bravo", "Charlie", "Delta"].map(|name| setup.agent(name));
    for agent in [&a, &b, &c, &d] {
        agents::queue_run(setup.profile(), &agent.id).unwrap();
    }
    // A second run of Alpha waits for Alpha's thread, whatever places are free.
    let alpha_again = enqueue(&setup.store(), &a.id, TriggerKind::Cron);
    let latest = |agent: &Agent| {
        setup
            .store()
            .page(Some(&agent.id), 10, None)
            .unwrap()
            .runs
            .into_iter()
            .last()
            .unwrap()
    };
    let (run_a, run_b, run_c, run_d) = (latest(&a), latest(&b), latest(&c), latest(&d));

    setup.tick();
    assert_eq!(setup.live(), 3);
    for run in [&run_a, &run_b, &run_c] {
        assert_eq!(setup.run(&run.id).status, RunStatus::Running, "{run:?}");
    }
    assert_eq!(setup.run(&run_d.id).status, RunStatus::Queued);
    assert_eq!(setup.run(&alpha_again.id).status, RunStatus::Queued);
    let threads: std::collections::BTreeSet<_> = [&run_a, &run_b, &run_c]
        .iter()
        .map(|run| setup.run(&run.id).thread_id.unwrap())
        .collect();
    assert_eq!(threads.len(), 3, "each agent runs in its own thread");
    // Each live run drives its own engine process, which names its session
    // file after its process id.
    let sessions = muniment_core::chat_profile::ChatProfile::new(setup.profile()).pi_session_root();
    let processes = || {
        std::fs::read_dir(&sessions)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .filter(|entry| entry.file_name().to_string_lossy().starts_with("stub-"))
                    .count()
            })
            .unwrap_or(0)
    };
    assert!(
        wait(|| processes() == 3),
        "{} engine processes",
        processes()
    );

    // A free place starts the oldest queued run.
    setup.finish(&run_b.id);
    setup.tick();
    assert_eq!(setup.run(&run_b.id).status, RunStatus::Completed);
    assert_eq!(setup.run(&run_d.id).status, RunStatus::Running);
    assert_eq!(setup.live(), 3);

    // Alpha's second run waits while Alpha's first still replies.
    setup.finish(&run_c.id);
    setup.tick();
    assert_eq!(setup.live(), 2);
    assert_eq!(setup.run(&alpha_again.id).status, RunStatus::Queued);

    setup.finish(&run_a.id);
    setup.tick();
    let second = setup.run(&alpha_again.id);
    assert_eq!(second.status, RunStatus::Running);
    assert_eq!(second.thread_id, setup.run(&run_a.id).thread_id);

    setup.finish(&run_d.id);
    setup.finish(&alpha_again.id);
    setup.tick();
    for run in [&run_a, &run_b, &run_c, &run_d, &alpha_again] {
        assert_eq!(setup.run(&run.id).status, RunStatus::Completed);
    }
    // Every change reached the subscriber as an `agent.run_updated` body.
    let seen: Vec<Value> = events.try_iter().collect();
    let statuses: Vec<&str> = seen
        .iter()
        .filter(|event| event["id"] == run_b.id.as_str())
        .map(|event| event["status"].as_str().unwrap())
        .collect();
    for status in ["queued", "leased", "running", "completed"] {
        assert!(statuses.contains(&status), "{statuses:?}");
    }
    assert!(seen.iter().all(|event| event.get("prompt").is_none()));
}

#[test]
fn leases_are_reconciled_against_the_journal() {
    let _environment = ENVIRONMENT
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let setup = Setup::new("agent-runs-leases", true, RuntimeHost::default());
    let store = setup.store();
    let [never, started, exhausted, held] =
        ["Never", "Started", "Exhausted", "Held"].map(|name| setup.agent(name));
    let lease = |agent: &Agent, expired: bool, attempts: u32| {
        let run = enqueue(&store, &agent.id, TriggerKind::Manual);
        let mut leased = store.lease(&run.id, "an-earlier-runtime").unwrap().unwrap();
        leased.attempt = attempts;
        leased.status = RunStatus::Running;
        let now = agent_runs::now();
        leased.lease_expires_at = Some(agent_runs::timestamp(if expired {
            now - 10
        } else {
            now + 600
        }));
        store.save(&mut leased).unwrap();
        leased
    };
    // A run whose lease expired before it started runs again.
    let never = lease(&never, true, 1);
    // A run that started under the expired lease is interrupted.
    let started = lease(&started, true, 1);
    {
        let storage = open_profile_storage(setup.profile()).unwrap();
        muniment_core::run_preparation::prepare_new_run_with_session_thread(
            &storage,
            muniment_core::run_preparation::SessionThreadStart {
                tracker: &muniment_core::session_thread::SessionThread::default(),
                continue_existing: false,
            },
            &started.id,
            "local",
            None,
            Vec::new(),
            None,
            "test",
            "1",
            || Ok(()),
        )
        .unwrap();
    }
    // A run that never started after its last attempt fails.
    let exhausted = lease(&exhausted, true, MAX_ATTEMPTS);
    // Another runtime's live lease is left alone.
    let held = lease(&held, false, 1);

    setup.tick();
    let rerun = setup.run(&never.id);
    assert_eq!(rerun.attempt, 2);
    assert!(
        matches!(rerun.status, RunStatus::Running | RunStatus::Completed),
        "{rerun:?}"
    );
    let interrupted = setup.run(&started.id);
    assert_eq!(interrupted.status, RunStatus::Interrupted);
    assert!(interrupted.lease_owner.is_none());
    let failed = setup.run(&exhausted.id);
    assert_eq!(failed.status, RunStatus::Failed);
    assert_eq!(failed.error.as_deref(), Some("The run could not start."));
    let untouched = setup.run(&held.id);
    assert_eq!(untouched.status, RunStatus::Running);
    assert_eq!(untouched.lease_owner.as_deref(), Some("an-earlier-runtime"));
    assert!(wait(|| setup.live() == 0));
    setup.tick();
    assert_eq!(setup.run(&never.id).status, RunStatus::Completed);
    assert_eq!(
        agents::state(setup.profile()).unwrap().runs[&started.agent_id].status,
        "interrupted"
    );
}

#[test]
fn a_routine_run_that_replies_no_response_posts_nothing() {
    let _environment = ENVIRONMENT
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let setup = Setup::new("agent-runs-silent", true, RuntimeHost::default());
    std::env::set_var("PI_RESUME_STUB_REPLY", "NO_RESPONSE");
    let mut agent = setup.agent("Watcher");
    agent.routines.push(Routine {
        id: "check".into(),
        name: "Check".into(),
        enabled: true,
        trigger: Trigger::Cron(CronSchedule {
            expr: "*/5 * * * *".into(),
            tz: "UTC".into(),
        }),
        prompt: "Report new items.".into(),
        created_from: agents::CreatedFrom::User,
        last_fired_at: None,
        last_result: None,
        proposed: false,
        last_poll: None,
        next_fire_at: None,
    });
    let agent = agents::save(setup.profile(), agent).unwrap();
    setup.tick();
    let next = agents::state(setup.profile()).unwrap().routines[&agent.id]["check"].next_fire_at;
    assert!(next.is_some_and(|next| next > agent_runs::now()));
    agents::with_state(setup.profile(), |state| {
        state
            .routines
            .get_mut(&agent.id)
            .unwrap()
            .get_mut("check")
            .unwrap()
            .next_fire_at = Some(agent_runs::now() - 30);
        Ok(())
    })
    .unwrap();
    setup.tick();
    let routine_run = setup.store().latest(&agent.id).unwrap().unwrap();
    assert_eq!(routine_run.trigger.kind, TriggerKind::Cron);
    assert_eq!(routine_run.trigger.routine_id.as_deref(), Some("check"));
    assert!(routine_run.prompt.contains("Report new items."));
    assert!(routine_run.prompt.contains("NO_RESPONSE"));
    assert!(wait(|| setup.live() == 0));
    setup.tick();
    let silent = setup.run(&routine_run.id);
    assert_eq!(silent.status, RunStatus::Completed);
    assert!(silent.silent);
    assert_eq!(silent.result_summary, None);
    let read = agents::read(setup.profile(), &agent.id).unwrap();
    assert!(read.routines[0].last_fired_at.is_some());
    assert_eq!(
        read.routines[0].last_result.as_ref().unwrap(),
        &agents::RoutineResult {
            run_id: routine_run.id.clone(),
            status: "completed".into(),
            silent: true
        }
    );

    // A manual run's reply is never silent.
    agents::queue_run(setup.profile(), &agent.id).unwrap();
    setup.tick();
    let manual = setup.store().latest(&agent.id).unwrap().unwrap();
    assert!(wait(|| setup.live() == 0));
    setup.tick();
    let manual = setup.run(&manual.id);
    assert!(!manual.silent);
    assert_eq!(manual.result_summary.as_deref(), Some("NO_RESPONSE"));

    // The silent run posts nothing to the thread; the manual run does.
    let thread = manual.thread_id.clone().unwrap();
    let page = muniment_runtime_core::thread_page(
        setup.profile(),
        open_profile_storage(setup.profile()).unwrap(),
        &setup.state.run_registry(),
        None,
        thread,
        10,
        None,
    )
    .unwrap();
    let runs: Vec<&str> = page
        .entries
        .iter()
        .map(|entry| entry.run_id.as_str())
        .collect();
    assert_eq!(runs, [manual.id.as_str()]);
}

#[test]
fn a_run_stuck_at_a_question_is_reminded_then_cancelled() {
    let _environment = ENVIRONMENT
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let setup = Setup::new("agent-runs-stuck", true, RuntimeHost::default());
    std::env::set_var(
        "PI_RESUME_STUB_PERMISSION_CAPTURE",
        setup.temporary.root.join("answers.jsonl"),
    );
    let agent = setup.agent("Asker");
    agents::queue_run(setup.profile(), &agent.id).unwrap();
    let run = setup.store().latest(&agent.id).unwrap().unwrap();
    setup.tick();
    assert!(wait(|| {
        setup.tick();
        setup.run(&run.id).status == RunStatus::WaitingInput
    }));
    let waiting = setup.run(&run.id);
    assert!(waiting.waiting_since.is_some());
    assert!(waiting.reminded_at.is_none());
    let events = agent_runs::subscribe(setup.profile());
    let age = |hours: i64| {
        let mut run = setup.run(&run.id);
        run.waiting_since = Some(agent_runs::timestamp(agent_runs::now() - hours * 3600));
        setup.store().save(&mut run).unwrap();
    };
    age(5);
    setup.tick();
    let reminded = setup.run(&run.id);
    assert_eq!(reminded.status, RunStatus::WaitingInput);
    assert!(reminded.reminded_at.is_some());
    assert!(events
        .try_iter()
        .any(|event| event["id"] == run.id.as_str() && event["reminded_at"].is_string()));
    age(25);
    setup.tick();
    assert!(wait(|| setup.live() == 0));
    setup.tick();
    let cancelled = setup.run(&run.id);
    assert_eq!(cancelled.status, RunStatus::Cancelled);
    assert_eq!(
        cancelled.error.as_deref(),
        Some(muniment_runtime_core::STUCK_CANCEL_REASON)
    );
}

#[test]
fn an_agent_runs_on_its_model_and_thinking_without_disabled_tools_behind_its_gate() {
    let _environment = ENVIRONMENT
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let setup = Setup::new("agent-runs-settings", true, RuntimeHost::default());
    let root = setup.temporary.root.clone();
    std::env::set_var("PI_RESUME_STUB_ARGS", root.join("args.txt"));
    std::env::set_var("PI_RESUME_STUB_REQUESTS", root.join("requests.jsonl"));
    std::env::set_var("PI_RESUME_STUB_REASONING", "1");
    std::env::set_var("PI_RESUME_STUB_EDITOR_CAPTURE", root.join("editor.jsonl"));
    std::env::set_var(
        "PI_RESUME_STUB_EDITOR_REQUEST",
        json!({
            "title": "muniment:permission-check",
            "prefill": {"tool": "bash", "input": {"command": "rm -rf build"}}
        })
        .to_string(),
    );
    let agent: Agent = serde_json::from_value(json!({
        "name": "Careful",
        "instructions": "Work carefully.",
        "model": {"provider": "stub", "id": "stub-model", "thinking": "high"},
        "tools": {"disabled": ["powershell"]},
        "permissions": {"mode": "ask", "always_allow": [{
            "request_kind": "tool",
            "resource": {"type": "tool", "name": "mcp__crm__list", "input": {"stage": "new"}}
        }]}
    }))
    .unwrap();
    let agent = agents::save(setup.profile(), agent).unwrap();
    agents::queue_run(setup.profile(), &agent.id).unwrap();
    setup.tick();
    assert!(wait(|| setup.live() == 0));

    let args = std::fs::read_to_string(root.join("args.txt")).unwrap();
    assert!(
        args.contains("--provider\nstub\n--model\nstub-model\n"),
        "{args}"
    );
    assert!(args.contains("--exclude-tools\nbg_run_pi_attested,powershell\n"));
    assert!(args.contains("agent-permissions.mjs"));
    let requests = std::fs::read_to_string(root.join("requests.jsonl")).unwrap();
    assert!(
        requests
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .any(|request| request["type"] == "set_thinking_level" && request["level"] == "high"),
        "{requests}"
    );
    // The runtime decides the call from the agent's profile.
    let answer: Value = serde_json::from_str(
        std::fs::read_to_string(root.join("editor.jsonl"))
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    let verdict: Value = serde_json::from_str(answer["value"].as_str().unwrap()).unwrap();
    assert_eq!(verdict["decision"], "ask");

    // A call the profile's pre-decided policy allows runs without a question.
    std::env::set_var(
        "PI_RESUME_STUB_EDITOR_REQUEST",
        json!({
            "title": "muniment:permission-check",
            "prefill": {"tool": "mcp__crm__list", "input": {"stage": "new"}}
        })
        .to_string(),
    );
    std::fs::remove_file(root.join("editor.jsonl")).unwrap();
    setup.tick();
    agents::queue_run(setup.profile(), &agent.id).unwrap();
    setup.tick();
    assert!(wait(|| setup.live() == 0));
    let answer: Value = serde_json::from_str(
        std::fs::read_to_string(root.join("editor.jsonl"))
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    let verdict: Value = serde_json::from_str(answer["value"].as_str().unwrap()).unwrap();
    assert_eq!(verdict["decision"], "allow");
}

#[test]
fn an_agent_run_answers_the_permission_mode_its_gate_asks_before_the_prompt_starts() {
    let _environment = ENVIRONMENT
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let setup = Setup::new(
        "agent-runs-mode-before-prompt",
        true,
        RuntimeHost::default(),
    );
    let capture = setup.temporary.root.join("editor.jsonl");
    std::env::set_var("PI_RESUME_STUB_PERMISSION_MODE", "1");
    std::env::set_var("PI_RESUME_STUB_EDITOR_CAPTURE", &capture);
    let agent: Agent = serde_json::from_value(json!({
        "name": "Planner",
        "instructions": "Plan first.",
        "permissions": {"mode": "ask"}
    }))
    .unwrap();
    let agent = agents::save(setup.profile(), agent).unwrap();
    let run = enqueue(&setup.store(), &agent.id, TriggerKind::Manual);
    setup.tick();
    assert!(wait(|| setup.live() == 0));
    setup.tick();

    assert_eq!(setup.run(&run.id).status, RunStatus::Completed);
    let answer: Value = serde_json::from_str(
        std::fs::read_to_string(&capture)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(answer["id"], "permission-mode-1");
    let mode: Value = serde_json::from_str(answer["value"].as_str().unwrap()).unwrap();
    assert!(mode["mode"].is_string(), "{mode}");
}

/// A host whose account is signed in as one subject.
struct SignedIn;

impl muniment_runtime_core::host::AccountService for SignedIn {
    fn session(
        &self,
        _: &muniment_core::attach::RuntimeActivityRegistry,
    ) -> Result<muniment_runtime_core::host::AccountSession, String> {
        Ok(muniment_runtime_core::host::AccountSession {
            tokens: Some(muniment_core::account::TokenSet {
                access_token: "token".into(),
                refresh_token: None,
                expires_at: None,
                subject: Some("owner-subject".into()),
            }),
            entitlement_snapshot: None,
        })
    }
}

#[test]
fn a_thread_the_queue_creates_belongs_to_the_signed_in_owner() {
    let _environment = ENVIRONMENT
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let host = RuntimeHost {
        account: Arc::new(SignedIn),
        ..RuntimeHost::default()
    };
    let setup = Setup::new("agent-runs-owner", false, host);
    setup
        .state
        .signed_workspace_approval()
        .record("workspace-a".into());
    let agent = setup.agent("Owned");
    agents::queue_run(setup.profile(), &agent.id).unwrap();
    setup.tick();
    // This host has no chat grants, so the run cannot start, but its thread
    // already belongs to the owner.
    let run = setup.store().latest(&agent.id).unwrap().unwrap();
    assert_eq!(run.status, RunStatus::Failed);
    let thread = run.thread_id.unwrap();
    let storage = open_profile_storage(setup.profile()).unwrap();
    let events = storage
        .lock()
        .unwrap()
        .journal
        .thread_events(&thread)
        .unwrap();
    let created = &events[0];
    assert_eq!(created.event_type, "thread.created");
    assert_eq!(
        created.provenance.actor_id.as_deref(),
        Some("owner-subject")
    );
    assert_eq!(created.provenance.extra["attach_profile"], "desktop-owner");
    // The thread names the agent and when it ran, not the run's prompt.
    let renamed = &events[1];
    assert_eq!(renamed.event_type, "thread.title.renamed");
    let renamed = serde_json::to_value(renamed).unwrap();
    let title = renamed["payload_json"]["title"].as_str().unwrap();
    assert!(title.starts_with("Owned · "), "{title}");
}

#[test]
fn allow_for_this_thread_journals_a_thread_policy_the_next_check_reads() {
    let _environment = ENVIRONMENT
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let setup = Setup::new("agent-runs-thread-policy", true, RuntimeHost::default());
    let root = setup.temporary.root.clone();
    std::env::set_var("PI_RESUME_STUB_EDITOR_CAPTURE", root.join("editor.jsonl"));
    let call = json!({"tool": "mcp__crm__archive", "input": {"id": "lead-9"}});
    std::env::set_var(
        "PI_RESUME_STUB_EDITOR_REQUEST",
        json!({"title": "muniment:permission-allow", "prefill": call}).to_string(),
    );
    let agent: Agent = serde_json::from_value(json!({
        "name": "Strict",
        "instructions": "Ask first.",
        "permissions": {"mode": "strict"}
    }))
    .unwrap();
    let agent = agents::save(setup.profile(), agent).unwrap();
    agents::queue_run(setup.profile(), &agent.id).unwrap();
    setup.tick();
    assert!(wait(|| setup.live() == 0));
    setup.tick();
    let run = setup.store().latest(&agent.id).unwrap().unwrap();
    let thread = run.thread_id.unwrap();
    let storage = open_profile_storage(setup.profile()).unwrap();
    let policies = storage
        .lock()
        .unwrap()
        .journal
        .active_thread_permission_policies(&thread)
        .unwrap();
    assert_eq!(policies.len(), 1);
    assert_eq!(
        policies[0].resource,
        muniment_core::journal::ThreadPermissionResource::Tool {
            name: "mcp__crm__archive".into(),
            input: json!({"id": "lead-9"}),
        }
    );
    drop(storage);

    // The next run in the thread finds the thread's policy.
    std::fs::remove_file(root.join("editor.jsonl")).unwrap();
    std::env::set_var(
        "PI_RESUME_STUB_EDITOR_REQUEST",
        json!({"title": "muniment:permission-check", "prefill": call}).to_string(),
    );
    agents::queue_run(setup.profile(), &agent.id).unwrap();
    setup.tick();
    assert!(wait(|| setup.live() == 0));
    let answer: Value = serde_json::from_str(
        std::fs::read_to_string(root.join("editor.jsonl"))
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    let verdict: Value = serde_json::from_str(answer["value"].as_str().unwrap()).unwrap();
    assert_eq!(verdict["decision"], "allow");
}

/// Runs one agent run whose stub sends `title` with `call` to the agent
/// permission bridge, and answers the runtime's reply.
fn bridge_answer(setup: &Setup, agent: &str, title: &str, call: &Value) -> Value {
    let capture = setup.temporary.root.join("editor.jsonl");
    let _ = std::fs::remove_file(&capture);
    std::env::set_var("PI_RESUME_STUB_EDITOR_CAPTURE", &capture);
    std::env::set_var(
        "PI_RESUME_STUB_EDITOR_REQUEST",
        json!({"title": title, "prefill": call}).to_string(),
    );
    agents::queue_run(setup.profile(), agent).unwrap();
    setup.tick();
    assert!(wait(|| setup.live() == 0));
    setup.tick();
    let answer: Value = serde_json::from_str(
        std::fs::read_to_string(&capture)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    serde_json::from_str(answer["value"].as_str().unwrap()).unwrap()
}

#[test]
fn always_allow_for_this_agent_saves_the_exact_rule_unless_ask_first_covers_it() {
    let _environment = ENVIRONMENT
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let setup = Setup::new("agent-runs-always-allow", true, RuntimeHost::default());
    let delete = json!({"tool": "mcp__crm__delete", "input": {"id": "lead-1"}});
    let agent: Agent = serde_json::from_value(json!({
        "name": "Releaser",
        "instructions": "Release carefully.",
        "permissions": {"mode": "ask", "ask_first": [{
            "request_kind": "tool",
            "resource": {"type": "tool", "name": "mcp__crm__delete", "input": {"id": "lead-1"}}
        }]}
    }))
    .unwrap();
    let agent = agents::save(setup.profile(), agent).unwrap();
    let push = json!({"tool": "bash", "input": {"command": "git push origin main"}});

    // The check names the exact request, a summary, and the gate's options.
    let verdict = bridge_answer(&setup, &agent.id, "muniment:permission-check", &push);
    assert_eq!(verdict["decision"], "ask");
    let push_request = verdict["request"].clone();
    assert_eq!(push_request["request_kind"], "command");
    assert_eq!(
        verdict["request"]["resource"]["arguments"],
        json!(["bash", "git push origin main"])
    );
    assert!(verdict["summary"]
        .as_str()
        .unwrap()
        .starts_with("bash `git push origin main` in "));
    assert!(verdict["title"]
        .as_str()
        .unwrap()
        .starts_with("Allow bash `git push origin main`"));
    assert_eq!(
        verdict["options"],
        json!([
            "Allow once",
            "Allow for this thread",
            "Always allow for this agent",
            "Deny"
        ])
    );
    // An ask-first request never offers the agent-wide answer.
    let verdict = bridge_answer(&setup, &agent.id, "muniment:permission-check", &delete);
    assert_eq!(verdict["decision"], "ask");
    assert_eq!(
        verdict["options"],
        json!(["Allow once", "Allow for this thread", "Deny"])
    );

    // "Always allow for this agent" appends the exact rule to the profile
    // and journals it in the thread for the run that asked.
    let saved = bridge_answer(&setup, &agent.id, "muniment:permission-always", &push);
    assert_eq!(saved["saved"], true, "{saved}");
    let always = agents::get(setup.profile(), &agent.id)
        .unwrap()
        .permissions
        .unwrap()
        .always_allow;
    assert_eq!(always.len(), 1);
    assert_eq!(serde_json::to_value(&always[0]).unwrap(), push_request);
    let thread = setup
        .store()
        .latest(&agent.id)
        .unwrap()
        .unwrap()
        .thread_id
        .unwrap();
    let storage = open_profile_storage(setup.profile()).unwrap();
    assert!(!storage
        .lock()
        .unwrap()
        .journal
        .active_thread_permission_policies(&thread)
        .unwrap()
        .is_empty());
    drop(storage);
    // Ask-first outranks it: the save is refused and the profile keeps one rule.
    let refused = bridge_answer(&setup, &agent.id, "muniment:permission-always", &delete);
    assert_eq!(refused["saved"], false, "{refused}");
    assert!(refused["reason"].as_str().unwrap().contains("ask-first"));
    assert_eq!(
        agents::get(setup.profile(), &agent.id)
            .unwrap()
            .permissions
            .unwrap()
            .always_allow
            .len(),
        1
    );
}

#[test]
fn always_allow_saves_a_global_rule_that_later_runs_in_other_threads_read() {
    let _environment = ENVIRONMENT
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let setup = Setup::new("agent-runs-always-global", true, RuntimeHost::default());
    let ask = |name: &str| -> Agent {
        let agent: Agent = serde_json::from_value(json!({
            "name": name,
            "instructions": "Release carefully.",
            "permissions": {"mode": "ask"}
        }))
        .unwrap();
        agents::save(setup.profile(), agent).unwrap()
    };
    let first = ask("Releaser");
    let second = ask("Reviewer");
    // A tool call's rule names the tool and its input, not the agent's folder.
    let push = json!({"tool": "mcp__crm__archive", "input": {"id": "lead-9"}});
    assert_eq!(
        bridge_answer(&setup, &second.id, "muniment:permission-check", &push)["decision"],
        "ask"
    );

    // "Always allow" saves the exact request to the global rules on disk,
    // which every later run reads at launch and fleet sync carries.
    let saved = bridge_answer(
        &setup,
        &first.id,
        "muniment:permission-always-global",
        &push,
    );
    assert_eq!(saved["saved"], true, "{saved}");
    let settings = muniment_core::permission_gate::settings::load(setup.profile());
    assert_eq!(settings.always_allow.len(), 1);
    assert!(agents::get(setup.profile(), &first.id)
        .unwrap()
        .permissions
        .unwrap()
        .always_allow
        .is_empty());

    // Another agent's thread, in a run that starts after the save, runs it.
    assert_eq!(
        bridge_answer(&setup, &second.id, "muniment:permission-check", &push)["decision"],
        "allow"
    );
    // A second answer for the same request keeps one rule.
    let again = bridge_answer(
        &setup,
        &first.id,
        "muniment:permission-always-global",
        &push,
    );
    assert_eq!(again["saved"], true, "{again}");
    assert_eq!(
        muniment_core::permission_gate::settings::load(setup.profile())
            .always_allow
            .len(),
        1
    );
}

#[test]
fn a_failed_run_keeps_its_inputs_without_secrets_and_replays_against_the_current_agent() {
    let _environment = ENVIRONMENT
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    // A host with no chat grants fails every run before it starts.
    let host = RuntimeHost {
        account: Arc::new(SignedIn),
        ..RuntimeHost::default()
    };
    let setup = Setup::new("agent-runs-replay", false, host);
    setup
        .state
        .signed_workspace_approval()
        .record("workspace-a".into());
    let agent = setup.agent("Fragile");
    let prompt = "Deploy with key sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789 now.";
    let run = agents::run_now(setup.profile(), &agent.id, Some(prompt), Some("test")).unwrap();
    setup.tick();
    assert!(wait(|| setup.run(&run.id).status == RunStatus::Failed));

    let listed =
        agents::operation(setup.profile(), "agent.replays", json!({"id": agent.id})).unwrap();
    let replays = listed["replays"].as_array().unwrap();
    assert_eq!(replays.len(), 1, "{listed}");
    let replay = &replays[0];
    assert_eq!(replay["replay_id"], run.id.as_str());
    assert_eq!(replay["trigger"]["kind"], "manual");
    assert_eq!(replay["redacted"], true);
    assert_eq!(replay["definition_changed"], false);
    assert!(replay["error"].is_string());
    let preview = replay["prompt_preview"].as_str().unwrap();
    assert!(preview.starts_with("Deploy with key ") && !preview.contains("sk-ant-api03"));
    let file = agents::folder(setup.profile(), &agent.id)
        .unwrap()
        .join("replays")
        .join(format!("{}.json", run.id));
    assert!(!std::fs::read_to_string(&file)
        .unwrap()
        .contains("sk-ant-api03"));

    // The avatar shows the failure.
    let listing = agents::operation(setup.profile(), "agent.list", json!({})).unwrap();
    let activity = &listing["agents"][0]["activity"];
    assert_eq!(activity["state"], "error", "{listing}");
    assert!(activity["action"].is_string());

    // A changed agent replays the same inputs; the new run names the old one.
    let mut changed = agents::get(setup.profile(), &agent.id).unwrap();
    changed.instructions = "Deploy only after the checks pass.".into();
    agents::save(setup.profile(), changed).unwrap();
    let answer = agents::operation(
        setup.profile(),
        "agent.replay",
        json!({"id": agent.id, "replay_id": run.id}),
    )
    .unwrap();
    let replayed = &answer["run"];
    assert_eq!(replayed["origin_run_id"], run.id.as_str());
    assert_eq!(replayed["trigger"]["kind"], "manual");
    assert_eq!(replayed["trigger"]["source"], "replay");
    assert_eq!(answer["replay"]["definition_changed"], true);
    assert_eq!(answer["replay"]["replayed_by"][0], replayed["id"]);
    let queued = setup.run(replayed["id"].as_str().unwrap());
    assert!(queued.prompt.starts_with("Deploy with key ") && !queued.prompt.contains("sk-ant"));
    let listing = agents::operation(setup.profile(), "agent.list", json!({})).unwrap();
    assert_eq!(listing["agents"][0]["activity"]["state"], "running");
    assert!(agents::operation(
        setup.profile(),
        "agent.replay",
        json!({"id": agent.id, "replay_id": "not-a-replay"}),
    )
    .is_err());

    // The library keeps the newest replays only.
    for _ in 0..muniment_core::agent_replays::MAX_REPLAYS + 2 {
        let mut failed = setup.run(&run.id);
        failed.id = uuid::Uuid::now_v7().to_string();
        muniment_core::agent_replays::record(setup.profile(), &failed).unwrap();
    }
    let listed =
        agents::operation(setup.profile(), "agent.replays", json!({"id": agent.id})).unwrap();
    assert_eq!(
        listed["replays"].as_array().unwrap().len(),
        muniment_core::agent_replays::MAX_REPLAYS
    );
    // Deleting the agent removes its replays.
    let mut done = setup.run(queued.id.as_str());
    done.status = RunStatus::Cancelled;
    setup.store().save(&mut done).unwrap();
    let folder = agents::folder(setup.profile(), &agent.id).unwrap();
    agents::delete_agent(setup.profile(), &agent.id, false).unwrap();
    assert!(!folder.exists());
}

/// Runs one permission operation through the runtime's attach boundary.
fn permission_operation(setup: &Setup, operation: &str, body: Value) -> Result<Value, String> {
    use muniment_core::run_start::RunAttachBoundaries;
    let operation: muniment_core::attach::Operation =
        serde_json::from_value(json!(operation)).unwrap();
    setup
        .state
        .boundaries()
        .agent_operation(operation, body)
        .map_err(|error| format!("{error:?}"))
}

#[test]
fn a_mode_sent_with_a_new_threads_first_message_decides_its_first_tool_call() {
    use muniment_core::permission_gate::PermissionMode;
    use muniment_core::run_start::RunAttachBoundaries;
    let _environment = ENVIRONMENT
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let setup = Setup::new("composer-first-mode", true, RuntimeHost::default());
    let capture = setup.temporary.root.join("editor.jsonl");
    std::env::set_var("PI_RESUME_STUB_EDITOR_CAPTURE", &capture);
    // The run's first step is a tool call the gate decides.
    std::env::set_var(
        "PI_RESUME_STUB_EDITOR_REQUEST",
        json!({
            "title": "muniment:permission-check",
            "prefill": {"tool": "bash", "input": {"command": "touch notes.txt"}}
        })
        .to_string(),
    );
    let boundaries = setup.state.boundaries().with_pi_artifact(setup.artifact);
    let submit = |mode: Option<PermissionMode>| {
        boundaries.submit_run(
            "local",
            "Make the notes file.".into(),
            Vec::new(),
            None,
            None,
            mode,
        )
    };
    let first_call = |mode: Option<PermissionMode>| -> (Value, String) {
        let _ = std::fs::remove_file(&capture);
        let accepted = submit(mode).unwrap();
        assert!(wait(|| setup.live() == 0));
        let answer: Value = serde_json::from_str(
            std::fs::read_to_string(&capture)
                .unwrap()
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        (
            serde_json::from_str(answer["value"].as_str().unwrap()).unwrap(),
            accepted.thread_id,
        )
    };

    // A new thread without a picked mode starts in the default, bypass, and
    // carries it as its own mode.
    let (verdict, thread) = first_call(None);
    assert_eq!(verdict["decision"], "allow", "{verdict}");
    let mode = permission_operation(
        &setup,
        "thread.permission_mode",
        json!({"thread_id": thread}),
    )
    .unwrap();
    assert_eq!(mode["mode"], "bypass");
    assert_eq!(mode["source"], "thread");
    assert_eq!(mode["set_by"], "user");

    // A mode sent with the first message is the thread's mode before its
    // run starts, so it decides the run's first tool call.
    let (verdict, thread) = first_call(Some(PermissionMode::Manual));
    assert_eq!(verdict["decision"], "ask", "{verdict}");
    let mode = permission_operation(
        &setup,
        "thread.permission_mode",
        json!({"thread_id": thread}),
    )
    .unwrap();
    assert_eq!(mode["mode"], "manual");
    assert_eq!(mode["source"], "thread");
    assert_eq!(mode["set_by"], "user");
    let (verdict, _) = first_call(Some(PermissionMode::DontAsk));
    assert_eq!(verdict["decision"], "deny", "{verdict}");

    // A new agent is saved in the default agent mode, bypass.
    let agent = setup.agent("Fresh");
    assert_eq!(
        agent.permissions.as_ref().and_then(|rules| rules.mode),
        Some(PermissionMode::Bypass)
    );

    // A thread created before its first message starts in the default too,
    // unless it is an agent's thread, which follows its agent.
    let provenance: muniment_core::journal::Provenance = serde_json::from_value(
        json!({"source": "test", "source_version": "0", "attach_profile": "desktop-owner"}),
    )
    .unwrap();
    let created = |agent: Option<&str>| {
        let thread = boundaries
            .create_thread("local", provenance.clone())
            .unwrap();
        if let Some(agent) = agent {
            agents::assign(setup.profile(), &thread, agent).unwrap();
        }
        boundaries
            .submit_run(
                "local",
                "Make the notes file.".into(),
                Vec::new(),
                Some(thread.clone()),
                None,
                None,
            )
            .unwrap();
        assert!(wait(|| setup.live() == 0));
        permission_operation(
            &setup,
            "thread.permission_mode",
            json!({"thread_id": thread}),
        )
        .unwrap()
    };
    let mode = created(None);
    assert_eq!(
        (&mode["mode"], &mode["source"]),
        (&json!("bypass"), &json!("thread"))
    );
    let planner: Agent = serde_json::from_value(json!({
        "name": "Planner",
        "instructions": "Plan first.",
        "permissions": {"mode": "plan"}
    }))
    .unwrap();
    let planner = agents::save(setup.profile(), planner).unwrap();
    let mode = created(Some(&planner.id));
    assert_eq!(
        (&mode["mode"], &mode["source"]),
        (&json!("plan"), &json!("agent"))
    );

    // With bypass off, new threads and agents start in manual, an existing
    // thread keeps its mode, and bypass is refused.
    permission_operation(
        &setup,
        "permission.settings_save",
        json!({"bypass_enabled": false}),
    )
    .unwrap();
    let (verdict, thread) = first_call(None);
    assert_eq!(verdict["decision"], "ask", "{verdict}");
    let mode = permission_operation(
        &setup,
        "thread.permission_mode",
        json!({"thread_id": thread}),
    )
    .unwrap();
    assert_eq!(mode["mode"], "manual");
    assert_eq!(mode["source"], "thread");
    let agent = setup.agent("Careful");
    assert_eq!(
        agent.permissions.and_then(|rules| rules.mode),
        Some(PermissionMode::Manual)
    );
    let refused = submit(Some(PermissionMode::Bypass))
        .unwrap_err()
        .into_message();
    assert!(refused.contains("Bypass is off"), "{refused}");
    assert_eq!(setup.live(), 0);
}

#[test]
fn a_plan_waits_as_an_approval_and_its_answer_switches_the_thread_mode() {
    let _environment = ENVIRONMENT
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let setup = Setup::new("agent-runs-plan", true, RuntimeHost::default());
    let plan = "1. Read the export script.\n2. Add a dry-run flag.";
    std::env::set_var("PI_RESUME_STUB_PLAN", plan);
    let agent: Agent = serde_json::from_value(json!({
        "name": "Planner",
        "instructions": "Plan before you change anything.",
        "permissions": {"mode": "plan"}
    }))
    .unwrap();
    let agent = agents::save(setup.profile(), agent).unwrap();
    agents::queue_run(setup.profile(), &agent.id).unwrap();
    let run = setup.store().latest(&agent.id).unwrap().unwrap();
    assert!(wait(|| {
        setup.tick();
        setup.run(&run.id).status == RunStatus::WaitingInput
    }));
    let thread = setup.run(&run.id).thread_id.unwrap();
    let mode = permission_operation(
        &setup,
        "thread.permission_mode",
        json!({"thread_id": thread}),
    )
    .unwrap();
    assert_eq!(mode["mode"], "plan");
    assert_eq!(mode["source"], "agent");
    assert_eq!(mode["agent_id"], agent.id.as_str());

    // The plan waits as a pending approval every member and the phone see.
    let waiting =
        permission_operation(&setup, "thread.plan", json!({"thread_id": thread})).unwrap();
    assert_eq!(waiting["plan"]["plan"], plan);
    assert_eq!(waiting["plan"]["run_id"], run.id.as_str());
    assert_eq!(
        waiting["plan"]["answers"],
        json!(["plan_manual", "plan_edit_auto", "plan_auto", "plan_keep"])
    );
    setup.state.swarm().tick().unwrap();
    let approvals = setup
        .state
        .swarm()
        .operation(
            muniment_core::attach::Operation::AgentApprovals,
            json!({}),
            None,
        )
        .unwrap();
    let approval = &approvals["approvals"][0];
    assert_eq!(approval["plan"], plan);
    assert_eq!(approval["answers"], waiting["plan"]["answers"]);
    assert!(approval.get("rule").is_none());

    // Approving it in a mode journals that mode for the thread.
    assert!(permission_operation(
        &setup,
        "thread.plan_answer",
        json!({"thread_id": thread, "answer": "always_allow_agent"})
    )
    .is_err());
    let answered = permission_operation(
        &setup,
        "thread.plan_answer",
        json!({"thread_id": thread, "answer": "plan_edit_auto"}),
    )
    .unwrap();
    assert_eq!(answered["status"], "answered");
    assert!(wait(|| setup.live() == 0));
    setup.tick();
    assert_eq!(setup.run(&run.id).status, RunStatus::Completed);
    let mode = permission_operation(
        &setup,
        "thread.permission_mode",
        json!({"thread_id": thread}),
    )
    .unwrap();
    assert_eq!(mode["mode"], "edit_auto");
    assert_eq!(mode["source"], "thread");
    assert_eq!(mode["set_by"], "plan");
    assert_eq!(
        permission_operation(&setup, "thread.plan", json!({"thread_id": thread})).unwrap(),
        json!({"plan": null})
    );

    // A person sets the mode too, but never to bypass while it is off.
    permission_operation(
        &setup,
        "permission.settings_save",
        json!({"bypass_enabled": false}),
    )
    .unwrap();
    let refused = permission_operation(
        &setup,
        "thread.permission_mode_set",
        json!({"thread_id": thread, "mode": "bypass"}),
    )
    .unwrap_err();
    assert!(refused.contains("Bypass is off"), "{refused}");
    let set = permission_operation(
        &setup,
        "thread.permission_mode_set",
        json!({"thread_id": thread, "mode": "dont_ask"}),
    )
    .unwrap();
    assert_eq!(set["mode"], "dont_ask");
    assert_eq!(set["set_by"], "user");
    let modes = permission_operation(&setup, "permission.modes", json!({})).unwrap();
    assert_eq!(modes["modes"].as_array().unwrap().len(), 6);
    assert_eq!(modes["default_agent_mode"], "manual");
}

#[test]
fn nobody_answers_an_unattended_gate_so_its_action_is_denied_and_the_run_pauses() {
    let _environment = ENVIRONMENT
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let setup = Setup::new("agent-runs-unattended", true, RuntimeHost::default());
    let answers = setup.temporary.root.join("answers.jsonl");
    std::env::set_var("PI_RESUME_STUB_PERMISSION_CAPTURE", &answers);
    // The wait is a runtime setting.
    permission_operation(
        &setup,
        "permission.settings_save",
        json!({"unattended_timeout_seconds": 3600}),
    )
    .unwrap();
    let agent = setup.agent("Night shift");
    let run = enqueue(&setup.store(), &agent.id, TriggerKind::Cron);
    assert!(wait(|| {
        setup.tick();
        setup.run(&run.id).status == RunStatus::WaitingInput
    }));
    let age = |hours: i64| {
        let mut waiting = setup.run(&run.id);
        waiting.waiting_since = Some(agent_runs::timestamp(agent_runs::now() - hours * 3600));
        setup.store().save(&mut waiting).unwrap();
    };
    // Within the wait nothing happens.
    age(0);
    setup.tick();
    assert_eq!(setup.run(&run.id).status, RunStatus::WaitingInput);
    age(2);
    setup.tick();
    assert!(wait(|| setup.live() == 0));
    setup.tick();
    let paused = setup.run(&run.id);
    assert_eq!(paused.status, RunStatus::Cancelled);
    assert_eq!(
        paused.error.as_deref(),
        Some(muniment_runtime_core::UNATTENDED_DENY_REASON)
    );
    // The gate closed with no answer, which the run reads as unanswered,
    // never as approval.
    let answered: Vec<Value> = std::fs::read_to_string(&answers)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(answered.len(), 1, "{answered:?}");
    assert_eq!(answered[0]["cancelled"], true);
    assert!(answered[0].get("confirmed").is_none());
}
