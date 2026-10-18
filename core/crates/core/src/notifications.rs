//! Notifications: what needs the user, what runs now, and what finished.
//!
//! `notifications.list` answers one machine's agent runs, newest first, with
//! each agent's name, the approvals waiting anywhere in the swarm, the next
//! scheduled runs and the time the user last marked everything read.
//! [`merge`] joins the answers of several machines into the page: the items
//! that need the user (approvals, questions, runs a usage limit stopped and
//! machines that do not answer), the runs in flight, and the rest by day.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use chrono::{DateTime, Duration, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agent_runs::{Run, RunStatus, RunStore};
use crate::model_router::balance::PickError;

/// The preference that holds when the user last marked everything read.
pub const READ_AT_KEY: &str = "notifications-read-at";
const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 100;
/// The scheduled runs the empty page lists.
const UPCOMING: usize = 5;
/// How long a run a usage limit stopped stays among the items that need the user.
const LIMIT_STOP_HOURS: i64 = 24;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    #[serde(default)]
    limit: Option<usize>,
}

/// One run as the page lists it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunItem {
    #[serde(flatten)]
    pub run: Run,
    pub agent_name: String,
    /// A usage limit stopped the run.
    #[serde(default)]
    pub limit_stop: bool,
}

/// One scheduled run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Upcoming {
    pub agent_id: String,
    pub agent_name: String,
    pub routine_id: String,
    pub routine_name: String,
    pub at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<Machine>,
}

/// One machine's answer to `notifications.list`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Listing {
    pub runs: Vec<RunItem>,
    pub upcoming: Vec<Upcoming>,
    /// The approvals waiting on any swarm member, as `agent.approvals` lists them.
    pub approvals: Vec<Value>,
    /// The soonest reset of a spent subscription window on this machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_at: Option<String>,
}

/// Answers `notifications.list` for the runtime whose state is `profile`.
pub fn operation(profile: &Path, body: Value, now_ms: i64) -> Result<Value, String> {
    let request: Request =
        serde_json::from_value(body).map_err(|_| "The request is invalid.".to_string())?;
    let limit = request.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let listing = crate::agents::list(profile)?;
    let names: BTreeMap<&str, &str> = listing
        .agents
        .iter()
        .map(|agent| (agent.id.as_str(), agent.name.as_str()))
        .collect();
    let store = RunStore::open(profile)?;
    let mut runs = store.page(None, limit, None)?.runs;
    for run in store.active()? {
        if !runs.iter().any(|held| held.id == run.id) {
            runs.push(run);
        }
    }
    let stopped = PickError::UsageLimit.message();
    let runs = runs
        .into_iter()
        .map(|run| RunItem {
            agent_name: names
                .get(run.agent_id.as_str())
                .map(|name| (*name).to_owned())
                .unwrap_or_default(),
            limit_stop: run.status == RunStatus::Failed
                && run.error.as_deref().is_some_and(|error| error == stopped),
            run,
        })
        .collect();
    let mut upcoming: Vec<Upcoming> = listing
        .agents
        .iter()
        .filter(|agent| agent.paused_reason.is_none())
        .flat_map(|agent| {
            agent
                .routines
                .iter()
                .filter(|routine| routine.enabled)
                .filter_map(|routine| {
                    let at_ms = routine.next_fire_at?.saturating_mul(1000);
                    (at_ms >= now_ms).then(|| Upcoming {
                        agent_id: agent.id.clone(),
                        agent_name: agent.name.clone(),
                        routine_id: routine.id.clone(),
                        routine_name: routine.name.clone(),
                        at_ms,
                        machine: None,
                    })
                })
        })
        .collect();
    upcoming.sort_by_key(|entry| entry.at_ms);
    upcoming.truncate(UPCOMING);
    let approvals = crate::swarm::Swarm::for_profile(profile)
        .and_then(|swarm| {
            swarm
                .operation(muniment_attach::Operation::AgentApprovals, json!({}), None)
                .ok()
        })
        .and_then(|answer| answer.get("approvals").and_then(Value::as_array).cloned())
        .unwrap_or_default();
    let (windows, _) =
        crate::usage_report::account_windows(&crate::state_root::agent_directory(profile));
    let resets_at_ms = windows
        .iter()
        .filter(|window| window.limit_reached)
        .filter_map(|window| window.resets_at_ms)
        .filter(|at| *at > now_ms)
        .min();
    let read_at = crate::preferences::values(profile)
        .ok()
        .and_then(|values| values.get(READ_AT_KEY)?.as_str().map(str::to_owned));
    serde_json::to_value(Listing {
        runs,
        upcoming,
        approvals,
        resets_at_ms,
        read_at,
    })
    .map_err(|_| "Notifications could not be read.".to_string())
}

/// Which items the page lists below the ones that need the user.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum View {
    NeedsYou,
    #[default]
    All,
    Failed,
}

/// The page's filters.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    #[serde(default)]
    pub view: View,
    /// A machine id. None is every machine.
    #[serde(default)]
    pub machine: Option<String>,
    /// An agent id. None is every agent.
    #[serde(default)]
    pub agent: Option<String>,
    /// The client's offset from UTC, so days start at its midnight.
    #[serde(default)]
    pub utc_offset_minutes: i32,
}

/// The machine an item belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Machine {
    pub id: String,
    pub name: String,
}

/// One machine's answer for [`merge`].
#[derive(Clone, Debug)]
pub struct MachineAnswer {
    pub machine: Machine,
    pub listing: Result<Listing, String>,
}

/// One item that needs the user.
#[derive(Clone, Debug, Serialize)]
pub struct NeedsYou {
    /// `approval`, `input`, `limit` or `offline`.
    pub kind: String,
    pub id: String,
    pub machine: Machine,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<Run>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub at: String,
}

/// One row of the timeline.
#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    /// `running`, `done`, `needs_you`, `failed` or `cancelled`.
    pub status: String,
    pub machine: Machine,
    pub agent_name: String,
    pub run: Run,
    pub unread: bool,
}

/// The rows of one day: `today`, `yesterday` or `earlier`.
#[derive(Clone, Debug, Serialize)]
pub struct Day {
    pub day: String,
    pub entries: Vec<Entry>,
}

/// The counts the sidebar and the filter show.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Counts {
    /// Items that need the user, machines that do not answer left out.
    pub needs_you: usize,
    pub running: usize,
    pub failed: usize,
    pub unread: usize,
}

/// The joined page.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Page {
    pub needs_you: Vec<NeedsYou>,
    pub running: Vec<Entry>,
    pub days: Vec<Day>,
    pub upcoming: Vec<Upcoming>,
    pub counts: Counts,
    pub machines: Vec<Machine>,
    /// Every agent with a run on the page, for the agent filter.
    pub agents: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_at: Option<String>,
}

fn status(run: &Run) -> &'static str {
    match run.status {
        RunStatus::Queued | RunStatus::Leased | RunStatus::Running => "running",
        RunStatus::WaitingInput => "needs_you",
        RunStatus::Completed => "done",
        RunStatus::Failed | RunStatus::Interrupted => "failed",
        RunStatus::Cancelled => "cancelled",
    }
}

fn millis(time: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(time)
        .ok()
        .map(|at| at.timestamp_millis())
}

/// Joins the answers of several machines into the page, as of `now_ms`.
/// The first answer's read time and approvals stand for the fleet: every
/// member reads the same swarm.
pub fn merge(answers: Vec<MachineAnswer>, filter: &Filter, now_ms: i64) -> Page {
    let mut page = Page::default();
    let mut approvals: BTreeMap<String, (Machine, Value)> = BTreeMap::new();
    let mut runs: Vec<(Machine, RunItem)> = Vec::new();
    let mut agents: BTreeMap<String, String> = BTreeMap::new();
    let wanted_machine = |id: &str| filter.machine.as_deref().is_none_or(|wanted| wanted == id);
    let wanted_agent = |id: &str| filter.agent.as_deref().is_none_or(|wanted| wanted == id);
    let mut resets_by_machine: BTreeMap<String, i64> = BTreeMap::new();
    let mut machine_names: BTreeMap<String, Machine> = BTreeMap::new();
    for answer in &answers {
        machine_names.insert(answer.machine.id.clone(), answer.machine.clone());
    }
    for answer in answers {
        page.machines.push(answer.machine.clone());
        let listing = match answer.listing {
            Ok(listing) => listing,
            Err(error) => {
                if wanted_machine(&answer.machine.id) {
                    page.needs_you.push(NeedsYou {
                        kind: "offline".into(),
                        id: answer.machine.id.clone(),
                        machine: answer.machine,
                        agent_id: None,
                        agent_name: None,
                        run: None,
                        approval: None,
                        resets_at_ms: None,
                        detail: Some(error),
                        at: String::new(),
                    });
                }
                continue;
            }
        };
        if page.read_at.is_none() {
            page.read_at = listing.read_at.clone();
        }
        for approval in listing.approvals {
            let Some(id) = approval["approval_id"].as_str().map(str::to_owned) else {
                continue;
            };
            let owner = approval["runtime_id"]
                .as_str()
                .and_then(|runtime| machine_names.get(runtime))
                .cloned()
                .unwrap_or_else(|| answer.machine.clone());
            approvals.entry(id).or_insert((owner, approval));
        }
        for item in listing.runs {
            agents
                .entry(item.run.agent_id.clone())
                .or_insert_with(|| item.agent_name.clone());
            runs.push((answer.machine.clone(), item));
        }
        if wanted_machine(&answer.machine.id) {
            for mut upcoming in listing.upcoming {
                if wanted_agent(&upcoming.agent_id) {
                    upcoming.machine = Some(answer.machine.clone());
                    page.upcoming.push(upcoming);
                }
            }
        }
        if let Some(resets) = listing.resets_at_ms {
            resets_by_machine.insert(answer.machine.id.clone(), resets);
        }
    }
    page.upcoming.sort_by_key(|entry| entry.at_ms);
    page.upcoming.truncate(UPCOMING);
    // A run that waits on a tool approval needs that approval, not a reply,
    // so it shows once, as the approval.
    let approved_runs: BTreeSet<String> = approvals
        .values()
        .filter(|(_, approval)| {
            approval["state"]
                .as_str()
                .is_none_or(|state| state == "pending")
        })
        .filter_map(|(_, approval)| approval["run_id"].as_str().map(str::to_owned))
        .collect();
    for (machine, approval) in approvals.into_values() {
        let agent = approval["agent_id"].as_str().unwrap_or_default().to_owned();
        if !wanted_machine(&machine.id) || !wanted_agent(&agent) {
            continue;
        }
        page.needs_you.push(NeedsYou {
            kind: "approval".into(),
            id: approval["approval_id"].as_str().unwrap_or_default().into(),
            machine,
            agent_name: approval["agent_name"].as_str().map(str::to_owned),
            agent_id: Some(agent),
            run: None,
            resets_at_ms: None,
            detail: approval["summary"].as_str().map(str::to_owned),
            at: approval["created_at"].as_str().unwrap_or_default().into(),
            approval: Some(approval),
        });
    }
    let read_at = page.read_at.as_deref().and_then(millis);
    let offset = Duration::minutes(i64::from(filter.utc_offset_minutes));
    let local_day = |ms: i64| {
        Utc.timestamp_millis_opt(ms)
            .single()
            .map(|at| (at + offset).date_naive())
    };
    let today = local_day(now_ms);
    runs.sort_by(|a, b| b.1.run.updated_at.cmp(&a.1.run.updated_at));
    let mut later_runs: BTreeSet<String> = BTreeSet::new();
    let stop_since = now_ms - Duration::hours(LIMIT_STOP_HOURS).num_milliseconds();
    for (machine, item) in runs {
        if !wanted_machine(&machine.id) || !wanted_agent(&item.run.agent_id) {
            continue;
        }
        let at = millis(&item.run.updated_at).unwrap_or(0);
        let state = status(&item.run);
        let newest_of_agent = later_runs.insert(format!("{}:{}", machine.id, item.run.agent_id));
        if item.run.status == RunStatus::WaitingInput && !approved_runs.contains(&item.run.id) {
            page.needs_you.push(NeedsYou {
                kind: "input".into(),
                id: item.run.id.clone(),
                machine: machine.clone(),
                agent_id: Some(item.run.agent_id.clone()),
                agent_name: Some(item.agent_name.clone()),
                detail: item.run.result_summary.clone(),
                at: item
                    .run
                    .waiting_since
                    .clone()
                    .unwrap_or_else(|| item.run.updated_at.clone()),
                run: Some(item.run.clone()),
                approval: None,
                resets_at_ms: None,
            });
        } else if item.limit_stop && newest_of_agent && at >= stop_since {
            page.needs_you.push(NeedsYou {
                kind: "limit".into(),
                id: item.run.id.clone(),
                machine: machine.clone(),
                agent_id: Some(item.run.agent_id.clone()),
                agent_name: Some(item.agent_name.clone()),
                detail: item.run.error.clone(),
                at: item.run.updated_at.clone(),
                resets_at_ms: resets_by_machine.get(&machine.id).copied(),
                run: Some(item.run.clone()),
                approval: None,
            });
        }
        let entry = Entry {
            status: state.into(),
            machine: machine.clone(),
            agent_name: item.agent_name.clone(),
            unread: read_at.is_none_or(|read| at > read) && state != "running",
            run: item.run,
        };
        if state == "running" {
            page.counts.running += 1;
            page.running.push(entry);
            continue;
        }
        if state == "failed" {
            page.counts.failed += 1;
        }
        if entry.unread {
            page.counts.unread += 1;
        }
        let shown = match filter.view {
            View::All => true,
            View::Failed => state == "failed",
            View::NeedsYou => false,
        };
        if !shown {
            continue;
        }
        let day = match local_day(at) {
            Some(day) if Some(day) == today => "today",
            Some(day) if today.and_then(|today| today.pred_opt()) == Some(day) => "yesterday",
            _ => "earlier",
        };
        match page.days.last_mut() {
            Some(held) if held.day == day => held.entries.push(entry),
            _ => page.days.push(Day {
                day: day.into(),
                entries: vec![entry],
            }),
        }
    }
    page.needs_you.sort_by(|a, b| b.at.cmp(&a.at));
    page.counts.needs_you = page
        .needs_you
        .iter()
        .filter(|item| item.kind != "offline")
        .count();
    page.agents = agents
        .into_iter()
        .map(|(id, name)| json!({"id": id, "name": name}))
        .collect();
    page
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runs::{RunTrigger, TriggerKind};

    const NOW: i64 = 1_800_000_000_000;

    fn time(ms: i64) -> String {
        crate::agent_runs::timestamp(ms / 1000)
    }

    fn run(id: &str, agent: &str, status: RunStatus, at_ms: i64) -> RunItem {
        RunItem {
            run: Run {
                id: id.into(),
                agent_id: agent.into(),
                thread_id: Some(format!("thread-{agent}")),
                trigger: RunTrigger {
                    kind: TriggerKind::Cron,
                    routine_id: None,
                    source: None,
                },
                status,
                attempt: 1,
                lease_owner: None,
                lease_expires_at: None,
                hop: 0,
                origin_run_id: None,
                created_at: time(at_ms),
                updated_at: time(at_ms),
                result_summary: Some("Wrote the briefing.".into()),
                silent: false,
                error: None,
                waiting_since: None,
                reminded_at: None,
                prompt: String::new(),
            },
            agent_name: format!("Agent {agent}"),
            limit_stop: false,
        }
    }

    fn machine(id: &str) -> Machine {
        Machine {
            id: id.into(),
            name: id.to_uppercase(),
        }
    }

    #[test]
    fn a_run_that_waits_on_an_approval_needs_the_user_once_as_the_approval() {
        let local = Listing {
            runs: vec![run("r1", "a", RunStatus::WaitingInput, NOW - 1000)],
            approvals: vec![json!({
                "approval_id": "r1:g1", "run_id": "r1", "state": "pending", "agent_id": "a",
                "agent_name": "Agent a", "summary": "web_search", "created_at": time(NOW - 500)
            })],
            ..Listing::default()
        };
        let page = merge(
            vec![MachineAnswer {
                machine: machine("local"),
                listing: Ok(local),
            }],
            &Filter::default(),
            NOW,
        );
        let kinds: Vec<&str> = page
            .needs_you
            .iter()
            .map(|item| item.kind.as_str())
            .collect();
        assert_eq!(kinds, vec!["approval"]);
        assert_eq!(page.counts.needs_you, 1);
    }

    #[test]
    fn the_page_pins_what_needs_the_user_and_groups_the_rest_by_day() {
        let hour = 60 * 60 * 1000;
        let mut stopped = run("r4", "c", RunStatus::Failed, NOW - 2 * hour);
        stopped.limit_stop = true;
        stopped.run.error = Some(PickError::UsageLimit.message());
        let local = Listing {
            runs: vec![
                run("r1", "a", RunStatus::Running, NOW - 1000),
                run("r2", "a", RunStatus::Completed, NOW - hour),
                run("r3", "b", RunStatus::WaitingInput, NOW - 3 * hour),
                stopped,
                run("r5", "b", RunStatus::Failed, NOW - 30 * hour),
            ],
            upcoming: vec![Upcoming {
                agent_id: "a".into(),
                agent_name: "Agent a".into(),
                routine_id: "daily".into(),
                routine_name: "Daily".into(),
                at_ms: NOW + hour,
                machine: None,
            }],
            approvals: vec![json!({
                "approval_id": "ap1", "runtime_id": "studio", "agent_id": "b",
                "agent_name": "Agent b", "summary": "Run git push", "created_at": time(NOW - 500)
            })],
            resets_at_ms: Some(NOW + hour),
            read_at: Some(time(NOW - 2 * hour + 1000)),
        };
        let page = merge(
            vec![
                MachineAnswer {
                    machine: machine("local"),
                    listing: Ok(local),
                },
                MachineAnswer {
                    machine: machine("studio"),
                    listing: Ok(Listing::default()),
                },
                MachineAnswer {
                    machine: machine("pod"),
                    listing: Err("That machine is offline.".into()),
                },
            ],
            &Filter::default(),
            NOW,
        );
        let kinds: Vec<&str> = page
            .needs_you
            .iter()
            .map(|item| item.kind.as_str())
            .collect();
        assert_eq!(kinds, vec!["approval", "limit", "input", "offline"]);
        assert_eq!(page.needs_you[0].machine.id, "studio");
        assert_eq!(page.needs_you[1].resets_at_ms, Some(NOW + 60 * 60 * 1000));
        assert_eq!(page.counts.needs_you, 3);
        assert_eq!(page.counts.running, 1);
        assert_eq!(page.counts.failed, 2);
        assert_eq!(page.running[0].run.id, "r1");
        let days: Vec<&str> = page.days.iter().map(|day| day.day.as_str()).collect();
        assert_eq!(days.first(), Some(&"today"));
        assert!(days.contains(&"yesterday") || days.contains(&"earlier"));
        let today = &page.days[0].entries;
        assert!(today
            .iter()
            .any(|entry| entry.run.id == "r2" && entry.unread));
        assert!(today
            .iter()
            .any(|entry| entry.run.id == "r3" && !entry.unread && entry.status == "needs_you"));
        assert_eq!(page.upcoming[0].machine.as_ref().unwrap().id, "local");
        assert_eq!(page.agents.len(), 3);

        let failed = merge(
            vec![MachineAnswer {
                machine: machine("local"),
                listing: Ok(Listing {
                    runs: vec![
                        run("r2", "a", RunStatus::Completed, NOW - hour),
                        run("r5", "b", RunStatus::Failed, NOW - hour),
                    ],
                    ..Listing::default()
                }),
            }],
            &Filter {
                view: View::Failed,
                agent: Some("b".into()),
                ..Filter::default()
            },
            NOW,
        );
        assert_eq!(failed.days.len(), 1);
        assert_eq!(failed.days[0].entries.len(), 1);
        assert_eq!(failed.days[0].entries[0].run.id, "r5");
    }

    #[test]
    fn the_request_takes_only_a_limit() {
        let request: Request = serde_json::from_value(json!({"limit": 5})).unwrap();
        assert_eq!(request.limit, Some(5));
        assert!(serde_json::from_value::<Request>(json!({"view": "all"})).is_err());
        let filter: Filter =
            serde_json::from_value(json!({"view": "needs_you", "machine": "local"})).unwrap();
        assert_eq!(filter.view, View::NeedsYou);
    }
}
