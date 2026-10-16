//! Drives the agent run queue: fires cron routines, reconciles leased runs
//! against the journal and the live runs, reminds about and cancels runs
//! stuck at a question, and starts queued runs while run places are free.
//! Each started run goes through the same run boundaries as interactive work.
use crate::RuntimeAttachState;
use muniment_core::agent_routines::{self, Trigger};
use muniment_core::agent_runs::{
    self, NewRun, Run, RunStatus, RunStore, RunTrigger, TriggerKind, CANCEL_AFTER_SECONDS,
    LEASE_SECONDS, MAX_ATTEMPTS, REMIND_AFTER_SECONDS,
};
use muniment_core::agents::{self, Agent, AgentList};
use muniment_core::run_start::RunStartBoundaries;

/// The note a run carries when the runtime cancels it after a long wait.
pub const STUCK_CANCEL_REASON: &str =
    "The run waited 24 hours for an answer, so Muniment cancelled it.";

/// How a run ended in the journal, as the queue reads it.
enum JournalOutcome {
    /// The journal has no such run: it never started.
    Missing,
    Live {
        waiting: bool,
    },
    Ended {
        status: RunStatus,
        text: String,
        error: Option<String>,
    },
}

impl RuntimeAttachState {
    pub(crate) fn check_agent_schedules(&self) -> Result<(), String> {
        self.check_agent_schedules_with(self.boundaries())
    }

    /// One pass of the agent run queue through a caller-selected boundary.
    #[doc(hidden)]
    pub fn check_agent_schedules_with(
        &self,
        boundaries: crate::RuntimeAttachBoundaries,
    ) -> Result<(), String> {
        let _pass = self.schedule_pass();
        let profile = self.agent_profile();
        if muniment_core::home::configured_home(profile)
            .map_err(|e| e.to_string())?
            .is_none()
        {
            return Ok(());
        }
        let store = RunStore::open(profile)?;
        let listing = agents::list(profile)?;
        for error in &listing.errors {
            muniment_core::runtime_eprintln!(
                "agent_schedule: agent skipped folder={} error={}",
                error.folder,
                error.error
            );
        }
        self.fire_routines(&store, &listing, agent_runs::now())?;
        self.reconcile_runs(&store)?;
        self.dispatch_runs(&store, &listing, &boundaries)
    }

    /// Whether this runtime runs an agent's routines: the agent names no
    /// primary machine, or names this one, and its routines are not paused.
    fn runs_routines_of(&self, agent: &Agent) -> bool {
        if agent.paused_reason.is_some() {
            return false;
        }
        match &agent.primary_runtime {
            None => true,
            Some(primary) => self
                .remote_access()
                .is_some_and(|access| access.runtime_info().runtime_id == *primary),
        }
    }

    /// Queues one run for each cron routine that came due. A routine that
    /// missed several times while the runtime was off runs once, and a
    /// routine whose earlier run has not ended does not queue another.
    fn fire_routines(&self, store: &RunStore, listing: &AgentList, now: i64) -> Result<(), String> {
        let profile = self.agent_profile();
        let active = store.active()?;
        let mut fired = Vec::new();
        agents::with_state(profile, |state| {
            for agent in &listing.agents {
                let routines = state.routines.entry(agent.id.clone()).or_default();
                let runs_here = self.runs_routines_of(agent);
                for routine in &agent.routines {
                    let saved = routines.entry(routine.id.clone()).or_default();
                    let Trigger::Cron(schedule) = &routine.trigger else {
                        continue;
                    };
                    let key = agent_routines::schedule_key(routine);
                    if !routine.enabled || !runs_here {
                        saved.next_fire_at = None;
                        saved.key = key;
                        continue;
                    }
                    let next = || {
                        agent_routines::next_cron_after(schedule, agent_routines::from_unix(now))
                            .ok()
                            .map(|time| time.timestamp())
                    };
                    if saved.key != key || saved.next_fire_at.is_none() {
                        saved.key = key;
                        saved.next_fire_at = next();
                        continue;
                    }
                    if saved.next_fire_at.is_some_and(|due| due <= now) {
                        saved.next_fire_at = next();
                        let busy = active.iter().any(|run| {
                            run.agent_id == agent.id
                                && run.trigger.routine_id.as_deref() == Some(routine.id.as_str())
                        });
                        if !busy {
                            saved.last_fired_at = Some(agent_runs::timestamp(now));
                            fired.push((agent.id.clone(), routine.clone()));
                        }
                    }
                }
                let next = routines
                    .values()
                    .filter_map(|saved| saved.next_fire_at)
                    .min();
                state.runs.entry(agent.id.clone()).or_default().next_run = next;
            }
            Ok(())
        })?;
        for (agent_id, routine) in fired {
            let run = store.enqueue(NewRun {
                agent_id,
                thread_id: None,
                trigger: RunTrigger {
                    kind: TriggerKind::Cron,
                    routine_id: Some(routine.id.clone()),
                    source: None,
                },
                prompt: agent_routines::routine_prompt(&routine),
                hop: 0,
                origin_run_id: None,
            })?;
            agents::mirror_run(profile, &run)?;
        }
        Ok(())
    }

    fn journal_outcome(&self, run_id: &str) -> Result<JournalOutcome, String> {
        use muniment_core::journal::reducer::{ChatProjector, RunStatus as Journal};
        let events = self.agent_events(run_id)?;
        if events.is_empty() {
            return Ok(JournalOutcome::Missing);
        }
        let mut projector = ChatProjector::new();
        for event in &events {
            projector
                .apply(event)
                .map_err(|error| format!("The run journal cannot be read: {error}"))?;
        }
        let projection = projector
            .projection()
            .map_err(|error| format!("The run journal cannot be read: {error}"))?;
        let ended = |status: RunStatus, error: Option<String>| JournalOutcome::Ended {
            status,
            text: projection.text.clone(),
            error,
        };
        Ok(match &projection.status {
            Some(Journal::Completed) => ended(RunStatus::Completed, None),
            Some(Journal::Cancelled) => ended(RunStatus::Cancelled, None),
            Some(Journal::Failed { reason }) => ended(RunStatus::Failed, reason.clone()),
            Some(Journal::NeedsAttention(_)) => ended(
                RunStatus::Interrupted,
                Some("The run stopped before it finished. Open its thread to resume it.".into()),
            ),
            _ => JournalOutcome::Live {
                waiting: projection.pending_permission.is_some(),
            },
        })
    }

    /// Brings each unfinished run's record in line with the live runs and
    /// the journal, and renews the leases of runs this runtime drives.
    fn reconcile_runs(&self, store: &RunStore) -> Result<(), String> {
        let profile = self.agent_profile();
        let registry = self.run_registry();
        let owner = self.instance_id().to_owned();
        let now = agent_runs::now();
        for run in store.active()? {
            if run.status == RunStatus::Queued {
                continue;
            }
            let mut next = run.clone();
            let live = registry.contains(&run.id);
            let expired = run
                .lease_expires_at
                .as_deref()
                .and_then(agent_runs::seconds)
                .is_none_or(|expires| expires <= now);
            let mine = run.lease_owner.as_deref() == Some(owner.as_str());
            if !live && !mine && !expired {
                // Another runtime process may still drive it.
                continue;
            }
            match self.journal_outcome(&run.id)? {
                JournalOutcome::Missing if live => {}
                JournalOutcome::Missing => {
                    next.lease_owner = None;
                    next.lease_expires_at = None;
                    if run.attempt >= MAX_ATTEMPTS {
                        next.status = RunStatus::Failed;
                        next.error = Some("The run could not start.".into());
                    } else {
                        next.status = RunStatus::Queued;
                    }
                }
                JournalOutcome::Live { waiting } if live => {
                    next.lease_owner = Some(owner.clone());
                    next.lease_expires_at = Some(agent_runs::timestamp(now + LEASE_SECONDS));
                    if waiting {
                        next.status = RunStatus::WaitingInput;
                        let since = next
                            .waiting_since
                            .get_or_insert_with(|| agent_runs::timestamp(now))
                            .clone();
                        let waited = agent_runs::seconds(&since).map_or(0, |since| now - since);
                        if waited >= CANCEL_AFTER_SECONDS {
                            next.error = Some(STUCK_CANCEL_REASON.into());
                            let _ = muniment_core::active_run::cancel_active_run(
                                &registry, &run.id, None,
                            );
                        } else if waited >= REMIND_AFTER_SECONDS && next.reminded_at.is_none() {
                            next.reminded_at = Some(agent_runs::timestamp(now));
                        }
                    } else {
                        next.status = RunStatus::Running;
                        next.waiting_since = None;
                        next.reminded_at = None;
                    }
                }
                JournalOutcome::Live { .. } => {
                    // The run started and its driver is gone. An update
                    // resume or the user's resume continues it.
                    next.status = RunStatus::Interrupted;
                    next.lease_owner = None;
                    next.lease_expires_at = None;
                    next.error = Some(
                        "The runtime stopped before this run finished. Open its thread to resume it."
                            .into(),
                    );
                }
                JournalOutcome::Ended { .. } if live => {}
                JournalOutcome::Ended {
                    status,
                    text,
                    error,
                } => {
                    next.status = status;
                    next.lease_owner = None;
                    next.lease_expires_at = None;
                    next.waiting_since = None;
                    if next.error.is_none() {
                        next.error = error;
                    }
                    let text = text.trim();
                    next.silent = status == RunStatus::Completed
                        && run.trigger.kind.routine()
                        && agent_routines::is_silent_reply(text);
                    next.result_summary =
                        (!text.is_empty() && !next.silent).then(|| text.to_owned());
                }
            }
            if next != run {
                store.save(&mut next)?;
                agents::mirror_run(profile, &next)?;
            }
        }
        Ok(())
    }

    /// Starts queued runs, oldest first, while run places are free. A run
    /// waits while another run replies in its agent's thread.
    fn dispatch_runs(
        &self,
        store: &RunStore,
        listing: &AgentList,
        boundaries: &crate::RuntimeAttachBoundaries,
    ) -> Result<(), String> {
        // A runtime that drains for an update starts nothing new. Queued runs
        // wait in the store for the next version.
        if self.drain_state().is_set() {
            return Ok(());
        }
        let profile = self.agent_profile();
        let registry = self.run_registry();
        let owner = self.instance_id().to_owned();
        let active = store.active()?;
        for queued in active.iter().filter(|run| run.status == RunStatus::Queued) {
            if registry.len() >= registry.limit() {
                break;
            }
            let Some(agent) = listing
                .agents
                .iter()
                .find(|agent| agent.id == queued.agent_id)
            else {
                let mut failed = queued.clone();
                failed.status = RunStatus::Failed;
                failed.error = Some("The agent no longer exists.".into());
                store.save(&mut failed)?;
                continue;
            };
            if queued.trigger.kind.routine() && !self.runs_routines_of(agent) {
                continue;
            }
            let thread = match &queued.thread_id {
                Some(thread) => Some(thread.clone()),
                None => agents::with_state(profile, |state| {
                    Ok(agents::ensure_primary(state, &agent.id))
                })?,
            };
            if let Some(thread) = &thread {
                let thread_busy = registry.thread_run(thread).is_some()
                    || active.iter().any(|run| {
                        run.id != queued.id
                            && run.status != RunStatus::Queued
                            && run.thread_id.as_deref() == Some(thread.as_str())
                    });
                if thread_busy {
                    continue;
                }
            }
            let Some(workspace) = boundaries
                .signed_workspace_approval()
                .approval()
                .map(|approval| approval.workspace)
            else {
                let mut waiting = queued.clone();
                waiting.error =
                    Some("Open Muniment and connect a model before running agents.".into());
                store.save(&mut waiting)?;
                continue;
            };
            let Some(mut run) = store.lease(&queued.id, &owner)? else {
                continue;
            };
            match self.start_run(&mut run, agent, thread, &workspace, boundaries) {
                Ok(()) => {
                    run.status = RunStatus::Running;
                    run.error = None;
                }
                Err(error)
                    if error == muniment_core::active_run::THREAD_BUSY
                        || error.starts_with("Muniment is already running") =>
                {
                    run.status = RunStatus::Queued;
                    run.lease_owner = None;
                    run.lease_expires_at = None;
                }
                Err(error) => {
                    run.status = RunStatus::Failed;
                    run.lease_owner = None;
                    run.lease_expires_at = None;
                    run.error = Some(error);
                }
            }
            store.save(&mut run)?;
            agents::mirror_run(profile, &run)?;
        }
        Ok(())
    }

    /// Starts one leased run in its agent's thread, creating the thread for
    /// the owner when the agent has none yet.
    fn start_run(
        &self,
        run: &mut Run,
        agent: &Agent,
        thread: Option<String>,
        workspace: &str,
        boundaries: &crate::RuntimeAttachBoundaries,
    ) -> Result<(), String> {
        let profile = self.agent_profile();
        let thread = match thread {
            Some(thread) => thread,
            None => {
                let owner = boundaries
                    .fresh_tokens()
                    .map_err(|error| error.into_message())?
                    .subject;
                let attach_profile = boundaries
                    .signed_workspace_approval()
                    .approval()
                    .map(|approval| approval.profile)
                    .ok_or("Open Muniment and connect a model before running agents.")?;
                crate::service::create_owned_thread(
                    self.agent_storage(),
                    workspace.to_owned(),
                    attach_profile,
                    owner,
                    self.runtime_version(),
                )?
            }
        };
        agents::with_state(profile, |state| {
            state
                .primary_threads
                .entry(agent.id.clone())
                .or_insert_with(|| thread.clone());
            state.threads.insert(thread.clone(), agent.id.clone());
            Ok(())
        })?;
        if let Some(project) = &agent.project_id {
            muniment_core::projects::assign(profile, &thread, project)?;
        }
        muniment_core::projects::workspace(profile, &thread)?;
        run.thread_id = Some(thread.clone());
        run.lease_expires_at = Some(agent_runs::timestamp(agent_runs::now() + LEASE_SECONDS));
        // The record names the thread before the run starts, so a restart
        // reconciles the run against the journal under the same id.
        RunStore::open(profile)?.save(run)?;
        boundaries
            .submit_run_with_id(
                run.id.clone(),
                workspace,
                run.prompt.clone(),
                Vec::new(),
                Some(thread),
                None,
            )
            .map(drop)
            .map_err(|error| error.into_message())
    }
}
