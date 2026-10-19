//! The sidebar's arrangement of threads and projects.
//!
//! A project is a group of things: threads and agents. A thread in no
//! project is a project of its own, one row in the Projects list. A catalog
//! project groups its threads and the agents that work in it, and an agent
//! without a catalog project reads as a project with triggers. [`arrange`]
//! places each thread once: in its project, as its own row, or among the
//! archived threads. Each row states its status in words, and the list runs
//! newest activity first with pinned rows on top. A finished thread settles
//! once it is read and keeps its place, showing only its time.

use std::collections::{BTreeMap, BTreeSet};

use chrono::DateTime;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The id prefix of a project that stands for an agent without a project.
pub const AGENT_PROJECT_PREFIX: &str = "agent:";

/// One thread the client knows.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Thread {
    pub id: String,
    pub title: String,
    /// RFC 3339. Empty for a thread with no activity yet.
    pub updated_at: String,
    /// The machine's name, when the client shows machines.
    pub machine: Option<String>,
}

/// One catalog project: this computer's, or another member's with its owner.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub owner: Option<String>,
}

/// One routine of an agent, as `agent.list` answers it.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Routine {
    pub enabled: bool,
    pub proposed: bool,
    pub trigger: Value,
}

/// One agent, as `agent.list` answers it, with its home thread.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub project_id: Option<String>,
    pub home_thread_id: Option<String>,
    pub paused_reason: Option<String>,
    pub routines: Vec<Routine>,
}

/// Something that needs the user, from the notifications page.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Attention {
    /// `approval`, `input` or `limit`.
    pub kind: String,
    pub thread_id: Option<String>,
    pub agent_id: Option<String>,
}

/// What the client knows, for [`arrange`].
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Input {
    pub threads: Vec<Thread>,
    pub projects: Vec<Project>,
    /// Thread id to catalog project id.
    pub assignments: BTreeMap<String, String>,
    pub agents: Vec<Agent>,
    /// Thread id to agent id, for every thread of an agent.
    pub agent_threads: BTreeMap<String, String>,
    pub pinned: BTreeSet<String>,
    pub archived: BTreeSet<String>,
    /// Thread id to when the user last read it, in RFC 3339.
    pub read: BTreeMap<String, String>,
    /// A thread with no read time counts as read up to this time.
    pub read_baseline: Option<String>,
    /// Threads with a reply in flight.
    pub working: BTreeSet<String>,
    /// Threads whose last run failed.
    pub failed: BTreeSet<String>,
    pub needs_you: Vec<Attention>,
    /// The open thread, which counts as read.
    pub current: Option<String>,
    /// Words to search titles and project names for.
    pub query: String,
}

/// A row's status, most urgent first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    NeedsYou,
    Working,
    Failed,
    Done,
    Read,
}

/// One thread row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Row {
    pub thread_id: String,
    pub title: String,
    /// Empty for a thread in no project.
    pub project_id: String,
    /// Empty for a thread in no project.
    pub project_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine: Option<String>,
    pub updated_at: String,
    pub status: Status,
    /// The status in words, such as `Approve`, `Working` or `Done`. Empty
    /// for a read thread.
    pub status_label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// The thread is its agent's home thread, where its runs post.
    pub home: bool,
    pub pinned: bool,
}

/// What kind of project a Projects row is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A thread in no project, which is its own project.
    Thread,
    Project,
    /// An agent without a catalog project.
    Agent,
}

/// One Projects row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProjectRow {
    /// The project id, or the thread id of a thread row.
    pub id: String,
    pub name: String,
    pub kind: Kind,
    /// The triggers in words, such as `every day 9:00` or `webhook`. Empty
    /// for a project without triggers.
    pub triggers: String,
    /// The agents that run in the project.
    pub agent_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// The newest activity of its threads, in RFC 3339. Empty when it has
    /// none yet.
    pub updated_at: String,
    /// The most urgent status of its threads.
    pub status: Status,
    /// That status in words. Empty when every thread is read.
    pub status_label: String,
    /// It holds a pinned thread.
    pub pinned: bool,
    /// The project's threads, its home thread first, then pinned ones, then
    /// newest first. A thread row holds its one thread.
    pub threads: Vec<Row>,
}

/// The arranged sidebar.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Sidebar {
    /// The Projects list, pinned rows first, then newest activity first.
    pub projects: Vec<ProjectRow>,
    pub archived: Vec<Row>,
    /// Rows whose title or project matches the query, archived ones too.
    pub results: Vec<Row>,
}

fn millis(time: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(time)
        .ok()
        .map(|at| at.timestamp_millis())
}

fn clock(minute: &str, hour: &str) -> Option<String> {
    let minute: u8 = minute.parse().ok().filter(|m| *m < 60)?;
    let hour: u8 = hour.parse().ok().filter(|h| *h < 24)?;
    Some(format!("{hour}:{minute:02}"))
}

const WEEKDAYS: [&str; 7] = [
    "Sundays",
    "Mondays",
    "Tuesdays",
    "Wednesdays",
    "Thursdays",
    "Fridays",
    "Saturdays",
];

fn interval(minutes: u32) -> String {
    match minutes {
        60 => "every hour".into(),
        m if m < 60 => format!("every {m} minutes"),
        m => format!("every {} hours", m / 60),
    }
}

/// A five-field cron expression in words, such as `every day 9:00`.
pub fn schedule_words(expr: &str) -> String {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    let other = || "on a schedule".to_string();
    let [minute, hour, day, month, weekday] = fields[..] else {
        return other();
    };
    if day != "*" || month != "*" {
        return other();
    }
    if let Some(time) = clock(minute, hour) {
        return match weekday {
            "*" => format!("every day {time}"),
            "1-5" => format!("weekdays {time}"),
            w => match w.parse::<usize>() {
                Ok(n) if n <= 7 => format!("{} {time}", WEEKDAYS[n % 7]),
                _ => other(),
            },
        };
    }
    if weekday != "*" {
        return other();
    }
    let step = |field: &str| {
        field
            .strip_prefix("*/")
            .and_then(|n| n.parse::<u32>().ok())
            .filter(|n| *n > 0)
    };
    match (minute, hour) {
        (m, "*") if step(m).is_some_and(|n| n < 60 && 60 % n == 0) => interval(step(m).unwrap()),
        ("0", "*") => interval(60),
        ("0", h) if step(h).is_some_and(|n| n < 24 && 24 % n == 0) => {
            interval(step(h).unwrap() * 60)
        }
        _ => other(),
    }
}

fn trigger_words(trigger: &Value) -> String {
    match trigger["kind"].as_str() {
        Some("cron") => schedule_words(trigger["expr"].as_str().unwrap_or_default()),
        Some("webhook") => "webhook".into(),
        Some("mcp") => "tool check".into(),
        _ => String::new(),
    }
}

/// An agent's triggers in words: the first enabled trigger, with how many
/// more it has, or `paused`.
fn agent_triggers(agent: &Agent) -> String {
    let words: Vec<String> = agent
        .routines
        .iter()
        .filter(|routine| routine.enabled && !routine.proposed)
        .map(|routine| trigger_words(&routine.trigger))
        .filter(|words| !words.is_empty())
        .collect();
    if words.is_empty() {
        return String::new();
    }
    if agent.paused_reason.is_some() {
        return "paused".into();
    }
    match words.len() {
        1 => words[0].clone(),
        n => format!("{} +{}", words[0], n - 1),
    }
}

fn attention_label(kind: &str) -> &'static str {
    match kind {
        "approval" => "Approve",
        "input" => "Answer",
        "limit" => "Limit reached",
        _ => "Needs you",
    }
}

fn status_label(status: Status) -> &'static str {
    match status {
        Status::NeedsYou => "Needs you",
        Status::Working => "Working",
        Status::Failed => "Failed",
        Status::Done => "Done",
        // A settled thread shows only its time.
        Status::Read => "",
    }
}

fn newest_first(rows: &mut [Row]) {
    rows.sort_by(|a, b| {
        millis(&b.updated_at)
            .cmp(&millis(&a.updated_at))
            .then_with(|| a.thread_id.cmp(&b.thread_id))
    });
}

/// Arranges the sidebar from what the client knows.
pub fn arrange(input: &Input) -> Sidebar {
    let agents: BTreeMap<&str, &Agent> = input
        .agents
        .iter()
        .map(|agent| (agent.id.as_str(), agent))
        .collect();
    let catalog: BTreeMap<&str, &Project> = input
        .projects
        .iter()
        .map(|project| (project.id.as_str(), project))
        .collect();
    let mut agent_threads = input.agent_threads.clone();
    for agent in &input.agents {
        if let Some(home) = &agent.home_thread_id {
            agent_threads.insert(home.clone(), agent.id.clone());
        }
    }
    let agent_project = |agent: &Agent| match &agent.project_id {
        Some(project) if catalog.contains_key(project.as_str()) => project.clone(),
        _ => format!("{AGENT_PROJECT_PREFIX}{}", agent.id),
    };

    // Every catalog project and every agent without a catalog project, with
    // the triggers of the agents in each. Threads in no project join later.
    let mut projects: BTreeMap<String, ProjectRow> = BTreeMap::new();
    let row = |id: &str, name: &str, kind, owner: Option<String>| ProjectRow {
        id: id.into(),
        name: name.into(),
        kind,
        triggers: String::new(),
        agent_ids: Vec::new(),
        owner,
        updated_at: String::new(),
        status: Status::Read,
        status_label: String::new(),
        pinned: false,
        threads: Vec::new(),
    };
    for project in &input.projects {
        projects.insert(
            project.id.clone(),
            row(
                &project.id,
                &project.name,
                Kind::Project,
                project.owner.clone(),
            ),
        );
    }
    for agent in &input.agents {
        let id = agent_project(agent);
        let entry = projects
            .entry(id.clone())
            .or_insert_with(|| row(&id, &agent.name, Kind::Agent, None));
        entry.agent_ids.push(agent.id.clone());
        let words = agent_triggers(agent);
        if !words.is_empty() {
            entry.triggers = if entry.triggers.is_empty() {
                words
            } else {
                format!("{}, {words}", entry.triggers)
            };
        }
    }

    let mut waiting: BTreeMap<String, &'static str> = BTreeMap::new();
    for item in &input.needs_you {
        let thread = item.thread_id.clone().or_else(|| {
            let agent = agents.get(item.agent_id.as_deref()?)?;
            agent.home_thread_id.clone()
        });
        if let Some(thread) = thread {
            waiting.entry(thread).or_insert(attention_label(&item.kind));
        }
    }
    let baseline = input.read_baseline.as_deref().and_then(millis);

    let mut sidebar = Sidebar::default();
    let mut alone: Vec<ProjectRow> = Vec::new();
    let query = input.query.trim().to_lowercase();
    let mut seen = BTreeSet::new();
    // An agent's home thread the client has not listed yet still shows
    // under its project, named for the agent.
    let homes: Vec<Thread> = input
        .agents
        .iter()
        .filter_map(|agent| {
            let id = agent.home_thread_id.as_ref()?;
            (!input.threads.iter().any(|thread| &thread.id == id)).then(|| Thread {
                id: id.clone(),
                ..Thread::default()
            })
        })
        .collect();
    for thread in input.threads.iter().chain(&homes) {
        if thread.id.is_empty() || !seen.insert(thread.id.as_str()) {
            continue;
        }
        let agent = agent_threads
            .get(&thread.id)
            .and_then(|id| agents.get(id.as_str()));
        let project_id = match agent {
            Some(agent) => Some(agent_project(agent)),
            None => input
                .assignments
                .get(&thread.id)
                .filter(|project| catalog.contains_key(project.as_str()))
                .cloned(),
        };
        let current = input.current.as_deref() == Some(thread.id.as_str());
        let updated = millis(&thread.updated_at);
        let read_at = input
            .read
            .get(&thread.id)
            .and_then(|at| millis(at))
            .or(baseline);
        let read = current
            || updated.is_none()
            || read_at.zip(updated).is_some_and(|(read, at)| read >= at);
        let (status, label) = if let Some(label) = waiting.get(&thread.id) {
            (Status::NeedsYou, *label)
        } else if input.working.contains(&thread.id) {
            (Status::Working, status_label(Status::Working))
        } else if input.failed.contains(&thread.id) && !read {
            (Status::Failed, status_label(Status::Failed))
        } else if read {
            (Status::Read, status_label(Status::Read))
        } else {
            (Status::Done, status_label(Status::Done))
        };
        let project_name = project_id
            .as_ref()
            .and_then(|id| projects.get(id))
            .map(|project| project.name.clone())
            .unwrap_or_default();
        let title = match (thread.title.trim(), agent) {
            ("", Some(agent)) => agent.name.clone(),
            ("", None) => "New thread".into(),
            (title, _) => title.into(),
        };
        let entry = Row {
            thread_id: thread.id.clone(),
            title,
            project_id: project_id.clone().unwrap_or_default(),
            project_name,
            machine: thread.machine.clone(),
            updated_at: thread.updated_at.clone(),
            status,
            status_label: label.into(),
            agent_id: agent.map(|agent| agent.id.clone()),
            home: agent.is_some_and(|agent| agent.home_thread_id.as_ref() == Some(&thread.id)),
            pinned: input.pinned.contains(&thread.id),
        };
        if !query.is_empty()
            && (entry.title.to_lowercase().contains(&query)
                || entry.project_name.to_lowercase().contains(&query))
        {
            sidebar.results.push(entry.clone());
        }
        if input.archived.contains(&thread.id) {
            sidebar.archived.push(entry);
            continue;
        }
        match project_id.and_then(|id| projects.get_mut(&id)) {
            Some(project) => project.threads.push(entry),
            None => {
                let mut own = row(&entry.thread_id, &entry.title, Kind::Thread, None);
                own.threads.push(entry);
                alone.push(own);
            }
        }
    }
    newest_first(&mut sidebar.archived);
    newest_first(&mut sidebar.results);
    let mut listed: Vec<ProjectRow> = projects
        .into_values()
        .chain(alone)
        .map(|mut project| {
            newest_first(&mut project.threads);
            project.threads.sort_by_key(|row| (!row.home, !row.pinned));
            if let Some(newest) = project
                .threads
                .iter()
                .max_by_key(|row| millis(&row.updated_at))
                .filter(|row| millis(&row.updated_at).is_some())
            {
                project.updated_at = newest.updated_at.clone();
            }
            if let Some(urgent) = project.threads.iter().min_by_key(|row| row.status) {
                project.status = urgent.status;
                project.status_label = urgent.status_label.clone();
            }
            project.pinned = project.threads.iter().any(|row| row.pinned);
            project
        })
        .collect();
    listed.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then_with(|| millis(&b.updated_at).cmp(&millis(&a.updated_at)))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.id.cmp(&b.id))
    });
    sidebar.projects = listed;
    sidebar
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn thread(id: &str, title: &str, at: &str) -> Thread {
        Thread {
            id: id.into(),
            title: title.into(),
            updated_at: at.into(),
            machine: None,
        }
    }

    fn ids(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|row| row.thread_id.as_str()).collect()
    }

    fn names(sidebar: &Sidebar) -> Vec<&str> {
        sidebar
            .projects
            .iter()
            .map(|project| project.name.as_str())
            .collect()
    }

    fn project<'a>(sidebar: &'a Sidebar, id: &str) -> &'a ProjectRow {
        sidebar
            .projects
            .iter()
            .find(|project| project.id == id)
            .unwrap()
    }

    fn cron(expr: &str) -> Routine {
        Routine {
            enabled: true,
            proposed: false,
            trigger: json!({ "kind": "cron", "expr": expr, "tz": "UTC" }),
        }
    }

    #[test]
    fn a_thread_in_no_project_is_its_own_project_row() {
        let input = Input {
            threads: vec![
                thread("a", "Trip ideas", "2026-10-18T10:00:00Z"),
                thread("b", "Fix sync", "2026-10-18T11:00:00Z"),
                thread("c", "", "2026-10-18T12:00:00Z"),
                thread("d", "Lost", "2026-10-18T09:00:00Z"),
            ],
            projects: vec![Project {
                id: "p1".into(),
                name: "Muniment".into(),
                owner: None,
            }],
            assignments: BTreeMap::from([("b".into(), "p1".into()), ("d".into(), "gone".into())]),
            agents: vec![Agent {
                id: "watch".into(),
                name: "Pricing watch".into(),
                home_thread_id: Some("c".into()),
                routines: vec![cron("0 9 * * *")],
                ..Agent::default()
            }],
            read_baseline: Some("2026-10-19T00:00:00Z".into()),
            ..Input::default()
        };
        let sidebar = arrange(&input);
        assert_eq!(
            names(&sidebar),
            vec!["Pricing watch", "Muniment", "Trip ideas", "Lost"],
            "newest activity first"
        );
        let own = project(&sidebar, "a");
        assert_eq!(own.kind, Kind::Thread);
        assert_eq!(ids(&own.threads), vec!["a"]);
        assert_eq!(own.threads[0].project_id, "");
        assert_eq!(own.threads[0].project_name, "");
        assert_eq!(
            project(&sidebar, "d").kind,
            Kind::Thread,
            "a thread assigned to a missing project is in no project"
        );
        assert_eq!(ids(&project(&sidebar, "p1").threads), vec!["b"]);
        let agent = project(&sidebar, "agent:watch");
        assert_eq!(agent.kind, Kind::Agent);
        assert_eq!(ids(&agent.threads), vec!["c"]);
        assert_eq!(agent.triggers, "every day 9:00");
        assert_eq!(agent.threads[0].title, "Pricing watch");
        assert!(agent.threads[0].home);
        assert_eq!(
            project(&sidebar, "p1").triggers,
            "",
            "a project without agents has no triggers"
        );
    }

    #[test]
    fn an_agent_home_thread_the_client_has_not_listed_shows_under_its_project() {
        let input = Input {
            agents: vec![Agent {
                id: "scout".into(),
                name: "Scout".into(),
                home_thread_id: Some("home".into()),
                ..Agent::default()
            }],
            ..Input::default()
        };
        let sidebar = arrange(&input);
        assert_eq!(sidebar.projects.len(), 1);
        let project = &sidebar.projects[0];
        assert_eq!(ids(&project.threads), vec!["home"]);
        assert_eq!(project.threads[0].title, "Scout");
        assert_eq!(project.status, Status::Read);
        assert_eq!(project.updated_at, "");
    }

    #[test]
    fn an_agent_in_a_catalog_project_gives_that_project_its_triggers() {
        let input = Input {
            threads: vec![thread("h", "Runs", "2026-10-18T10:00:00Z")],
            projects: vec![Project {
                id: "p1".into(),
                name: "Homelab".into(),
                owner: None,
            }],
            agents: vec![Agent {
                id: "deploy".into(),
                name: "Deploy".into(),
                project_id: Some("p1".into()),
                home_thread_id: Some("h".into()),
                routines: vec![Routine {
                    enabled: true,
                    proposed: false,
                    trigger: json!({ "kind": "webhook", "hook_id": "x" }),
                }],
                ..Agent::default()
            }],
            ..Input::default()
        };
        let sidebar = arrange(&input);
        assert_eq!(
            sidebar.projects.len(),
            1,
            "the agent groups into its project"
        );
        assert_eq!(sidebar.projects[0].id, "p1");
        assert_eq!(sidebar.projects[0].triggers, "webhook");
        assert_eq!(sidebar.projects[0].agent_ids, vec!["deploy"]);
        assert_eq!(sidebar.projects[0].threads[0].project_name, "Homelab");
    }

    #[test]
    fn each_thread_shows_once_with_its_status_on_its_row() {
        let input = Input {
            threads: vec![
                thread("ask", "Deploy", "2026-10-18T10:00:00Z"),
                thread("run", "Build", "2026-10-18T10:01:00Z"),
                thread("pin", "Notes", "2026-10-17T10:02:00Z"),
                thread("new", "Report", "2026-10-18T10:03:00Z"),
                thread("old", "Done before", "2026-10-17T10:00:00Z"),
                thread("bad", "Broke", "2026-10-18T10:04:00Z"),
                thread("gone", "Archived", "2026-10-18T10:05:00Z"),
            ],
            agents: vec![Agent {
                id: "bot".into(),
                name: "Bot".into(),
                home_thread_id: Some("ask".into()),
                ..Agent::default()
            }],
            needs_you: vec![Attention {
                kind: "approval".into(),
                thread_id: None,
                agent_id: Some("bot".into()),
            }],
            working: BTreeSet::from(["run".into()]),
            pinned: BTreeSet::from(["pin".into()]),
            failed: BTreeSet::from(["bad".into()]),
            archived: BTreeSet::from(["gone".into()]),
            read: BTreeMap::from([
                ("old".into(), "2026-10-17T11:00:00Z".into()),
                ("pin".into(), "2026-10-17T11:00:00Z".into()),
            ]),
            ..Input::default()
        };
        let sidebar = arrange(&input);
        assert_eq!(
            sidebar
                .projects
                .iter()
                .map(|project| project.id.as_str())
                .collect::<Vec<_>>(),
            vec!["pin", "bad", "new", "run", "agent:bot", "old"],
            "pinned rows first, then newest activity first"
        );
        let shown: Vec<&str> = sidebar
            .projects
            .iter()
            .flat_map(|project| ids(&project.threads))
            .collect();
        assert_eq!(shown.len(), 6, "each thread shows once: {shown:?}");
        let label = |id: &str| project(&sidebar, id).status_label.as_str();
        assert_eq!(label("agent:bot"), "Approve");
        assert_eq!(project(&sidebar, "agent:bot").status, Status::NeedsYou);
        assert_eq!(label("run"), "Working");
        assert_eq!(label("bad"), "Failed");
        assert_eq!(label("new"), "Done");
        assert_eq!(label("old"), "", "a settled row shows only its time");
        assert!(project(&sidebar, "pin").pinned);
        assert_eq!(ids(&sidebar.archived), vec!["gone"]);
    }

    #[test]
    fn a_project_takes_its_newest_activity_and_most_urgent_status() {
        let input = Input {
            threads: vec![
                thread("a", "One", "2026-10-18T09:00:00Z"),
                thread("b", "Two", "2026-10-18T11:00:00Z"),
                thread("c", "Three", "2026-10-18T10:00:00Z"),
                thread("solo", "Solo", "2026-10-18T10:30:00Z"),
            ],
            projects: vec![Project {
                id: "p".into(),
                name: "Work".into(),
                owner: None,
            }],
            assignments: BTreeMap::from([
                ("a".into(), "p".into()),
                ("b".into(), "p".into()),
                ("c".into(), "p".into()),
            ]),
            working: BTreeSet::from(["a".into()]),
            pinned: BTreeSet::from(["c".into()]),
            read_baseline: Some("2026-10-19T00:00:00Z".into()),
            ..Input::default()
        };
        let sidebar = arrange(&input);
        let work = project(&sidebar, "p");
        assert_eq!(work.updated_at, "2026-10-18T11:00:00Z");
        assert_eq!(work.status, Status::Working);
        assert_eq!(work.status_label, "Working");
        assert!(work.pinned, "a project with a pinned thread is pinned");
        assert_eq!(ids(&work.threads), vec!["c", "b", "a"]);
        assert_eq!(names(&sidebar), vec!["Work", "Solo"]);
    }

    #[test]
    fn reading_a_thread_settles_it_in_place() {
        let mut input = Input {
            threads: vec![
                thread("a", "One", "2026-10-18T10:00:00Z"),
                thread("b", "Two", "2026-10-18T09:00:00Z"),
            ],
            read: BTreeMap::from([("b".into(), "2026-10-18T09:30:00Z".into())]),
            ..Input::default()
        };
        let sidebar = arrange(&input);
        assert_eq!(names(&sidebar), vec!["One", "Two"]);
        assert_eq!(sidebar.projects[0].status_label, "Done");
        assert_eq!(sidebar.projects[1].status_label, "");
        input.current = Some("a".into());
        let sidebar = arrange(&input);
        assert_eq!(names(&sidebar), vec!["One", "Two"], "no row moves on read");
        assert_eq!(sidebar.projects[0].status, Status::Read);
    }

    #[test]
    fn the_baseline_settles_threads_read_before_read_times_were_kept() {
        let input = Input {
            threads: vec![
                thread("before", "Old", "2026-10-01T10:00:00Z"),
                thread("after", "New", "2026-10-18T10:00:00Z"),
            ],
            read_baseline: Some("2026-10-10T00:00:00Z".into()),
            ..Input::default()
        };
        let sidebar = arrange(&input);
        assert_eq!(project(&sidebar, "before").status, Status::Read);
        assert_eq!(project(&sidebar, "after").status, Status::Done);
    }

    #[test]
    fn search_matches_titles_and_projects_with_archives() {
        let input = Input {
            threads: vec![
                thread("a", "Vendor terms", "2026-10-18T10:00:00Z"),
                thread("b", "Lunch", "2026-10-18T09:00:00Z"),
                thread("c", "Budget", "2026-10-18T08:00:00Z"),
            ],
            projects: vec![Project {
                id: "p".into(),
                name: "Vendors".into(),
                owner: None,
            }],
            assignments: BTreeMap::from([("c".into(), "p".into())]),
            archived: BTreeSet::from(["a".into()]),
            query: " VENDOR ".into(),
            ..Input::default()
        };
        assert_eq!(ids(&arrange(&input).results), vec!["a", "c"]);
    }

    /// The probe page shows this arrangement. `MUNIMENT_WRITE_FIXTURES=1`
    /// writes it again after a change to [`arrange`].
    #[test]
    fn the_probe_fixture_matches_the_arrangement() {
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../protocol-fixtures/sidebar");
        let input: Input =
            serde_json::from_slice(&std::fs::read(folder.join("input.json")).unwrap()).unwrap();
        let arranged = serde_json::to_string_pretty(&arrange(&input)).unwrap() + "\n";
        if std::env::var_os("MUNIMENT_WRITE_FIXTURES").is_some() {
            std::fs::write(folder.join("arranged.json"), &arranged).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(folder.join("arranged.json")).unwrap(),
            arranged
        );
    }

    #[test]
    fn schedules_read_in_words() {
        assert_eq!(schedule_words("0 9 * * *"), "every day 9:00");
        assert_eq!(schedule_words("30 7 * * 1-5"), "weekdays 7:30");
        assert_eq!(schedule_words("0 8 * * 1"), "Mondays 8:00");
        assert_eq!(schedule_words("0 8 * * 7"), "Sundays 8:00");
        assert_eq!(schedule_words("*/15 * * * *"), "every 15 minutes");
        assert_eq!(schedule_words("0 * * * *"), "every hour");
        assert_eq!(schedule_words("0 */6 * * *"), "every 6 hours");
        assert_eq!(schedule_words("0 9 1 * *"), "on a schedule");
        let agent = Agent {
            routines: vec![cron("0 9 * * *"), cron("0 * * * *")],
            ..Agent::default()
        };
        assert_eq!(agent_triggers(&agent), "every day 9:00 +1");
        let paused = Agent {
            paused_reason: Some("absence".into()),
            ..agent
        };
        assert_eq!(agent_triggers(&paused), "paused");
    }
}
