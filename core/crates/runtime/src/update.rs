//! The runtime's side of an update a launcher installs: the drain before the
//! launcher stops it, and the resume of interrupted runs after it starts
//! again.
//!
//! A drain refuses new runs with the retryable `runtime_draining` error and
//! waits up to a grace period for the runtime to go idle. When the grace
//! period ends first, the runtime records the runs it is about to interrupt.
//! The next start resumes each of them from its Pi session, unless the run
//! waited on a permission gate or its thread already resumed
//! [`MAX_CONSECUTIVE_UPDATE_RESUMES`] times in a row.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use muniment_core::attach::{
    read_update_status, RuntimeActivity, RuntimeActivityRegistry, RuntimeUpdateState,
    UpdateCapability,
};
use muniment_core::serde_json;
use serde::{Deserialize, Serialize};

use crate::RuntimeAttachState;

/// The most times in a row the runtime resumes runs of one thread on its own.
pub const MAX_CONSECUTIVE_UPDATE_RESUMES: u32 = 3;
/// The note a resumed run shows in its thread.
pub const UPDATE_RESUME_NOTICE: &str =
    "Muniment updated while this reply ran, so it resumed where it stopped.";
/// The grace period a drain waits for the runtime to go idle by default.
pub const DEFAULT_UPDATE_GRACE: Duration = Duration::from_secs(120);

/// The state root's directory for update state, beside the launcher's
/// versions.
const UPDATE_DIRECTORY_NAME: &str = "runtime";
const RESUME_LEDGER_FILE: &str = "update-resume.json";
const DRAIN_POLL: Duration = Duration::from_millis(250);
/// How long a resume waits for the launcher to commit the new version, and
/// for an earlier resumed run to finish.
const RESUME_WAIT: Duration = Duration::from_secs(15 * 60);
const RESUME_POLL: Duration = Duration::from_secs(1);

/// A run an update interrupted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterruptedRun {
    pub run_id: String,
    pub thread_id: String,
    pub workspace: String,
    /// Whether the run waited on a permission gate when the runtime stopped.
    pub permission_gate: bool,
}

/// The automatic resumes in a row of one thread.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadResumes {
    /// The run the runtime last resumed in the thread.
    pub run_id: String,
    pub count: u32,
}

/// The runs the last update interrupted and the resume count of each thread.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeLedger {
    #[serde(default)]
    pub interrupted: Vec<InterruptedRun>,
    #[serde(default)]
    pub resumed: BTreeMap<String, ThreadResumes>,
}

/// What the runtime does with one interrupted run after the restart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResumeDecision {
    Resume,
    /// The run waited on a permission gate. It stays for its owner.
    SkipPermissionGate,
    /// The thread resumed on its own too many times in a row. The run stays
    /// for a manual resume.
    SkipLimit,
}

/// Decides each interrupted run, counts the resumes per thread and clears the
/// interrupted list. A resume of the run the thread last resumed adds one to
/// its count. Any other run of the thread starts the count again.
pub fn select_resumes(ledger: &mut ResumeLedger) -> Vec<(InterruptedRun, ResumeDecision)> {
    let mut decisions = Vec::new();
    for run in std::mem::take(&mut ledger.interrupted) {
        if run.permission_gate {
            decisions.push((run, ResumeDecision::SkipPermissionGate));
            continue;
        }
        let count = match ledger.resumed.get(&run.thread_id) {
            Some(previous) if previous.run_id == run.run_id => previous.count + 1,
            _ => 1,
        };
        if count > MAX_CONSECUTIVE_UPDATE_RESUMES {
            decisions.push((run, ResumeDecision::SkipLimit));
            continue;
        }
        ledger.resumed.insert(
            run.thread_id.clone(),
            ThreadResumes {
                run_id: run.run_id.clone(),
                count,
            },
        );
        decisions.push((run, ResumeDecision::Resume));
    }
    decisions
}

/// The ledger of `state`, or an empty one when none was saved or it does
/// not parse.
pub fn read_resume_ledger(state: &Path) -> ResumeLedger {
    std::fs::read(ledger_path(state))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Saves the ledger of `state` atomically.
pub fn write_resume_ledger(state: &Path, ledger: &ResumeLedger) -> std::io::Result<()> {
    let path = ledger_path(state);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec(ledger).map_err(std::io::Error::other)?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, bytes)?;
    muniment_core::atomic_file::replace(&temporary, &path)
}

fn ledger_path(state: &Path) -> PathBuf {
    state.join(UPDATE_DIRECTORY_NAME).join(RESUME_LEDGER_FILE)
}

/// The wire names of the activities that block a quiet stop, in field order.
pub fn blocker_names(activity: RuntimeActivity) -> Vec<String> {
    [
        (activity.active_run, "active_run"),
        (activity.pending_permission_gate, "pending_permission_gate"),
        (
            activity.authentication_operation,
            "authentication_operation",
        ),
        (activity.session_refresh, "session_refresh"),
        (
            activity.in_flight_external_effect,
            "in_flight_external_effect",
        ),
    ]
    .into_iter()
    .filter(|(blocked, _)| *blocked)
    .map(|(_, name)| name.to_owned())
    .collect()
}

/// How a drain ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DrainOutcome {
    /// The runtime went idle within the grace period.
    Idle,
    /// The grace period ended while this work still ran.
    TimedOut { blockers: Vec<String> },
}

/// Waits up to `grace` for `registry` to report no blocking activity, and
/// calls `changed` with the blockers each time they change.
pub fn wait_for_quiesce(
    registry: &RuntimeActivityRegistry,
    grace: Duration,
    poll: Duration,
    mut changed: impl FnMut(&[String]),
) -> DrainOutcome {
    let deadline = Instant::now() + grace;
    let mut last = None;
    loop {
        let blockers = blocker_names(registry.snapshot());
        if last.as_ref() != Some(&blockers) {
            changed(&blockers);
            last = Some(blockers.clone());
        }
        if blockers.is_empty() {
            return DrainOutcome::Idle;
        }
        let now = Instant::now();
        if now >= deadline {
            return DrainOutcome::TimedOut { blockers };
        }
        std::thread::sleep(poll.min(deadline - now));
    }
}

impl RuntimeAttachState {
    /// Drains the runtime for an update: new runs fail with the retryable
    /// `runtime_draining` error, and the call waits up to `grace` for the
    /// runtime to go idle while the update status names what it waits for.
    /// When the grace period ends first, the runs still active are recorded
    /// for a resume after the restart. The drain does not end: the host stops
    /// the runtime next.
    pub fn drain_for_update(&self, grace: Duration) -> DrainOutcome {
        self.drain_state().set();
        let access = self.remote_access();
        let target = access
            .as_ref()
            .and_then(|access| access.requested_update())
            .map(|request| request.version);
        muniment_core::runtime_eprintln!(
            "muniment-runtime: update drain started grace_seconds={} target_version={}",
            grace.as_secs(),
            target.as_deref().unwrap_or("none")
        );
        let outcome = wait_for_quiesce(&self.runtime_activity(), grace, DRAIN_POLL, |blockers| {
            if let Some(access) = &access {
                if let Err(error) = access.set_update_status(
                    RuntimeUpdateState::Draining,
                    target.as_deref(),
                    blockers.to_vec(),
                    None,
                ) {
                    muniment_core::runtime_eprintln!(
                        "muniment-runtime: update status not saved: {error}"
                    );
                }
            }
        });
        match &outcome {
            DrainOutcome::Idle => {
                muniment_core::runtime_eprintln!("muniment-runtime: update drain outcome=idle");
            }
            DrainOutcome::TimedOut { blockers } => {
                let recorded = self.record_interrupted_runs();
                muniment_core::runtime_eprintln!(
                    "muniment-runtime: update drain outcome=timed_out blockers={} interrupted_runs={recorded}",
                    blockers.join(",")
                );
            }
        }
        outcome
    }

    /// Adds the active run to the resume ledger and answers how many runs it
    /// recorded.
    fn record_interrupted_runs(&self) -> usize {
        let Some((run_id, workspace)) = self.active_run_identity() else {
            return 0;
        };
        let thread_id = self
            .agent_storage()
            .lock()
            .ok()
            .and_then(|storage| storage.journal.run_thread_id(&run_id).ok().flatten());
        let Some(thread_id) = thread_id else {
            return 0;
        };
        let permission_gate = self.runtime_activity().snapshot().pending_permission_gate;
        let state = self.agent_profile();
        let mut ledger = read_resume_ledger(state);
        ledger.interrupted.retain(|run| run.run_id != run_id);
        ledger.interrupted.push(InterruptedRun {
            run_id,
            thread_id,
            workspace,
            permission_gate,
        });
        match write_resume_ledger(state, &ledger) {
            Ok(()) => 1,
            Err(error) => {
                muniment_core::runtime_eprintln!(
                    "muniment-runtime: interrupted runs not recorded: {error}"
                );
                0
            }
        }
    }

    /// Resumes the runs the last update interrupted, on a thread of its own.
    /// It waits until the launcher commits this version, so a version that
    /// fails its trial resumes nothing, and resumes one run at a time.
    pub fn resume_after_update(self: &Arc<Self>) {
        if read_resume_ledger(self.agent_profile())
            .interrupted
            .is_empty()
        {
            return;
        }
        let state = Arc::clone(self);
        std::thread::spawn(move || state.resume_interrupted_runs());
    }

    fn resume_interrupted_runs(&self) {
        let profile = self.agent_profile().to_path_buf();
        let launcher = self
            .remote_access()
            .is_some_and(|access| access.update_capability() == UpdateCapability::Launcher);
        if launcher {
            let remote = profile.join("remote");
            let committed = wait_until(RESUME_WAIT, || {
                read_update_status(&remote)
                    .is_none_or(|status| status.state != RuntimeUpdateState::Restarting)
            });
            if !committed {
                muniment_core::runtime_eprintln!(
                    "muniment-runtime: update resume skipped reason=uncommitted"
                );
                return;
            }
        }
        self.resume_interrupted_runs_with(self.boundaries());
    }

    /// Resumes the runs the ledger names through `boundaries`, at once and
    /// on the calling thread.
    #[doc(hidden)]
    pub fn resume_interrupted_runs_with(&self, boundaries: crate::RuntimeAttachBoundaries) {
        let profile = self.agent_profile().to_path_buf();
        let mut ledger = read_resume_ledger(&profile);
        let decisions = select_resumes(&mut ledger);
        // The counts are saved before any resume, so a crash during one never
        // resumes the thread past the limit.
        if let Err(error) = write_resume_ledger(&profile, &ledger) {
            muniment_core::runtime_eprintln!(
                "muniment-runtime: update resume skipped reason=ledger error={error}"
            );
            return;
        }
        for (run, decision) in decisions {
            let outcome = match decision {
                ResumeDecision::SkipPermissionGate => "skipped_permission_gate".to_owned(),
                ResumeDecision::SkipLimit => "skipped_limit".to_owned(),
                ResumeDecision::Resume => {
                    if !wait_until(RESUME_WAIT, || self.active_run_identity().is_none()) {
                        "skipped_busy".to_owned()
                    } else {
                        match boundaries.resume_run_with_notice(
                            &run.workspace,
                            &run.run_id,
                            Some(UPDATE_RESUME_NOTICE.to_owned()),
                        ) {
                            Ok(_) => "resumed".to_owned(),
                            Err(error) => format!("failed error={error:?}"),
                        }
                    }
                }
            };
            muniment_core::runtime_eprintln!(
                "muniment-runtime: update resume run_id={} thread_id={} outcome={outcome}",
                run.run_id,
                run.thread_id
            );
        }
    }
}

/// Polls `done` until it holds or `limit` passes, and answers whether it held.
fn wait_until(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + limit;
    loop {
        if done() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(RESUME_POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(run_id: &str, thread_id: &str, permission_gate: bool) -> InterruptedRun {
        InterruptedRun {
            run_id: run_id.into(),
            thread_id: thread_id.into(),
            workspace: "local".into(),
            permission_gate,
        }
    }

    #[test]
    fn a_run_at_a_permission_gate_stays_for_its_owner() {
        let mut ledger = ResumeLedger {
            interrupted: vec![run("r1", "t1", true), run("r2", "t2", false)],
            ..ResumeLedger::default()
        };
        let decisions = select_resumes(&mut ledger);
        assert_eq!(
            decisions
                .iter()
                .map(|(run, decision)| (run.run_id.as_str(), *decision))
                .collect::<Vec<_>>(),
            vec![
                ("r1", ResumeDecision::SkipPermissionGate),
                ("r2", ResumeDecision::Resume)
            ]
        );
        assert!(ledger.interrupted.is_empty());
        assert!(!ledger.resumed.contains_key("t1"));
        assert_eq!(ledger.resumed["t2"].count, 1);
    }

    #[test]
    fn a_thread_resumes_on_its_own_at_most_three_times_in_a_row() {
        let mut ledger = ResumeLedger::default();
        for expected in 1..=MAX_CONSECUTIVE_UPDATE_RESUMES {
            ledger.interrupted.push(run("r1", "t1", false));
            assert_eq!(
                select_resumes(&mut ledger)[0].1,
                ResumeDecision::Resume,
                "resume {expected}"
            );
            assert_eq!(ledger.resumed["t1"].count, expected);
        }
        ledger.interrupted.push(run("r1", "t1", false));
        assert_eq!(select_resumes(&mut ledger)[0].1, ResumeDecision::SkipLimit);
        assert_eq!(ledger.resumed["t1"].count, MAX_CONSECUTIVE_UPDATE_RESUMES);

        // A new run in the thread starts the count again.
        ledger.interrupted.push(run("r2", "t1", false));
        assert_eq!(select_resumes(&mut ledger)[0].1, ResumeDecision::Resume);
        assert_eq!(ledger.resumed["t1"].count, 1);
    }

    #[test]
    fn the_ledger_survives_a_restart() {
        let state =
            std::env::temp_dir().join(format!("muniment-update-ledger-{}", uuid::Uuid::now_v7()));
        assert_eq!(read_resume_ledger(&state), ResumeLedger::default());
        let ledger = ResumeLedger {
            interrupted: vec![run("r1", "t1", false)],
            ..ResumeLedger::default()
        };
        write_resume_ledger(&state, &ledger).unwrap();
        assert_eq!(read_resume_ledger(&state), ledger);
        std::fs::remove_dir_all(state).unwrap();
    }

    #[test]
    fn a_drain_ends_when_the_runtime_goes_idle() {
        let registry = RuntimeActivityRegistry::new();
        let run = registry.mark_active_run();
        let gate = registry.mark_pending_permission_gate();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            drop(gate);
            std::thread::sleep(Duration::from_millis(100));
            drop(run);
        });
        let mut seen = Vec::new();
        let outcome = wait_for_quiesce(
            &registry,
            Duration::from_secs(10),
            Duration::from_millis(10),
            |blockers| seen.push(blockers.to_vec()),
        );
        release.join().unwrap();
        assert_eq!(outcome, DrainOutcome::Idle);
        assert_eq!(
            seen,
            vec![
                vec![
                    "active_run".to_owned(),
                    "pending_permission_gate".to_owned()
                ],
                vec!["active_run".to_owned()],
                vec![],
            ]
        );
    }

    #[test]
    fn a_drain_gives_up_when_the_grace_period_ends() {
        let registry = RuntimeActivityRegistry::new();
        let _run = registry.mark_active_run();
        let started = Instant::now();
        let outcome = wait_for_quiesce(
            &registry,
            Duration::from_millis(150),
            Duration::from_millis(10),
            |_| {},
        );
        assert!(started.elapsed() >= Duration::from_millis(150));
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(
            outcome,
            DrainOutcome::TimedOut {
                blockers: vec!["active_run".to_owned()]
            }
        );
    }

    #[test]
    fn blockers_use_their_wire_names() {
        assert_eq!(
            blocker_names(RuntimeActivity::default()),
            Vec::<String>::new()
        );
        assert_eq!(
            blocker_names(RuntimeActivity {
                active_run: true,
                pending_permission_gate: true,
                authentication_operation: true,
                session_refresh: true,
                in_flight_external_effect: true,
            }),
            vec![
                "active_run",
                "pending_permission_gate",
                "authentication_operation",
                "session_refresh",
                "in_flight_external_effect"
            ]
        );
    }
}
