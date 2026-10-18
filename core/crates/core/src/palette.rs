//! The command palette: what it finds and in what order. An empty query
//! lists what needs the user, then recent threads, then the actions that
//! apply now. A leading `>` limits the results to actions. Otherwise every
//! word of the query must match a result's title, project or synonyms, the
//! best match leads, and among equal matches the newest thread leads. Each
//! result carries its shortcut, if it has one, as `mod+n`, `mod+shift+u` or
//! `shift+tab`, where `mod` is Command on macOS and Control elsewhere.
use serde::{Deserialize, Serialize};

/// One thread the palette can open, as the sidebar arranges it.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Thread {
    pub thread_id: String,
    pub title: String,
    pub project_name: String,
    /// The status in words, such as `Approve` or `Working`. Empty for a
    /// read thread.
    pub status_label: String,
    /// RFC 3339.
    pub updated_at: String,
    /// The thread's row shortcut in the sidebar, such as `mod+2`.
    pub shortcut: Option<String>,
}

/// One page the host adds, such as a Settings page or part.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Page {
    pub id: String,
    pub title: String,
    /// The page that holds a part, such as `Settings`.
    pub detail: String,
    pub keywords: Vec<String>,
    pub icon: String,
    pub shortcut: Option<String>,
}

/// The open thread, for the actions that change it.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct OpenThread {
    pub thread_id: String,
    pub pinned: bool,
    /// The thread can move to another project: it belongs to no agent and
    /// no reply runs in it.
    pub movable: bool,
}

/// What the palette searches.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Input {
    pub query: String,
    /// The threads that need the user, first in the empty palette.
    pub needs_you: Vec<Thread>,
    /// Every other thread the window knows.
    pub threads: Vec<Thread>,
    /// Pages beside the app's own, such as Settings pages and parts.
    pub pages: Vec<Page>,
    pub thread: Option<OpenThread>,
    /// The machines a new thread can run on, this computer included.
    pub machines: usize,
    /// The composer's thread lists permission modes.
    pub modes: bool,
}

/// What a result is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Thread,
    Page,
    Action,
}

/// One result row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Item {
    pub kind: Kind,
    /// A thread id, a page id or an action id.
    pub id: String,
    pub title: String,
    /// A muted line beside the title, such as the project or a status.
    pub detail: String,
    /// A Lucide icon name.
    pub icon: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<String>,
}

/// A titled group of results.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Group {
    pub title: String,
    pub items: Vec<Item>,
}

struct Action {
    id: &'static str,
    title: &'static str,
    keywords: &'static [&'static str],
    icon: &'static str,
    shortcut: Option<&'static str>,
}

const ACTIONS: [Action; 8] = [
    Action {
        id: "new_thread",
        title: "New thread",
        keywords: &["chat", "compose", "start", "conversation"],
        icon: "square-pen",
        shortcut: Some("mod+n"),
    },
    Action {
        id: "new_project",
        title: "New project",
        keywords: &["folder", "create", "workspace"],
        icon: "folder-plus",
        shortcut: None,
    },
    Action {
        id: "new_agent",
        title: "New agent",
        keywords: &["bot", "automation", "trigger", "routine", "schedule"],
        icon: "bot",
        shortcut: None,
    },
    Action {
        id: "move_to_project",
        title: "Move to project",
        keywords: &["file", "folder", "organize", "project"],
        icon: "folder-input",
        shortcut: None,
    },
    Action {
        id: "pin",
        title: "Pin thread",
        keywords: &["star", "favorite", "keep", "unpin"],
        icon: "pin",
        shortcut: None,
    },
    Action {
        id: "switch_model",
        title: "Switch model",
        keywords: &["provider", "llm", "change model"],
        icon: "cpu",
        shortcut: None,
    },
    Action {
        id: "switch_machine",
        title: "Switch machine",
        keywords: &["runtime", "computer", "remote", "server"],
        icon: "monitor",
        shortcut: None,
    },
    Action {
        id: "toggle_permission_mode",
        title: "Toggle permission mode",
        keywords: &["permissions", "bypass", "plan", "ask", "approve"],
        icon: "shield-check",
        shortcut: Some("shift+tab"),
    },
];

/// The app's own pages, beside the ones the host adds.
/// An app page: its id, title, synonyms, icon and shortcut.
type AppPage = (
    &'static str,
    &'static str,
    &'static [&'static str],
    &'static str,
    Option<&'static str>,
);

const PAGES: [AppPage; 6] = [
    (
        "artifacts",
        "Artifacts",
        &["pages", "html", "documents"],
        "file-code",
        Some("mod+j"),
    ),
    ("agents", "Agents", &["bots", "automations"], "bot", None),
    (
        "projects",
        "Projects",
        &["folders", "workspaces"],
        "layout-dashboard",
        None,
    ),
    (
        "usage",
        "Usage",
        &["limits", "tokens", "cost", "spend", "allowance"],
        "chart-column",
        Some("mod+shift+u"),
    ),
    (
        "notifications",
        "Notifications",
        &["approvals", "inbox", "alerts", "needs you"],
        "bell",
        Some("mod+shift+a"),
    ),
    (
        "settings",
        "Settings",
        &["preferences", "options"],
        "settings",
        Some("mod+,"),
    ),
];

/// The most threads a group lists.
const THREADS: usize = 8;

/// How well `text` matches one query word: a whole-title prefix, a word
/// prefix, a substring, then, for a word of three letters or more, its
/// letters in order.
fn word_score(text: &str, word: &str) -> u32 {
    let text = text.to_lowercase();
    if text.starts_with(word) {
        return 100;
    }
    if text
        .split(|c: char| !c.is_alphanumeric())
        .any(|part| part.starts_with(word))
    {
        return 80;
    }
    if text.contains(word) {
        return 60;
    }
    let mut letters = text.chars();
    if word.chars().count() >= 3 && word.chars().all(|wanted| letters.any(|c| c == wanted)) {
        return 20;
    }
    0
}

/// The score of a result for every query word, or None when a word matches
/// none of its fields. The title counts fully, the rest at half.
fn score(words: &[String], title: &str, others: &[&str]) -> Option<u32> {
    let mut total = 0;
    for word in words {
        let own = word_score(title, word);
        let other = others
            .iter()
            .map(|text| word_score(text, word) / 2)
            .max()
            .unwrap_or(0);
        let best = own.max(other);
        if best == 0 {
            return None;
        }
        total += best;
    }
    Some(total)
}

fn actions(input: &Input) -> Vec<(Item, &'static [&'static str])> {
    ACTIONS
        .iter()
        .filter(|action| match action.id {
            "move_to_project" => input.thread.as_ref().is_some_and(|thread| thread.movable),
            "pin" => input.thread.is_some(),
            "switch_machine" => input.machines > 1,
            "toggle_permission_mode" => input.modes,
            _ => true,
        })
        .map(|action| {
            let pinned = action.id == "pin" && input.thread.as_ref().is_some_and(|t| t.pinned);
            (
                Item {
                    kind: Kind::Action,
                    id: action.id.into(),
                    title: if pinned { "Unpin thread" } else { action.title }.into(),
                    detail: String::new(),
                    icon: action.icon.into(),
                    shortcut: action.shortcut.map(str::to_owned),
                },
                action.keywords,
            )
        })
        .collect()
}

fn thread_item(thread: &Thread) -> Item {
    let detail = [thread.status_label.as_str(), thread.project_name.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    Item {
        kind: Kind::Thread,
        id: thread.thread_id.clone(),
        title: if thread.title.trim().is_empty() {
            "New thread".into()
        } else {
            thread.title.clone()
        },
        detail,
        icon: "message-square".into(),
        shortcut: thread.shortcut.clone(),
    }
}

fn newest_first(threads: &mut [&Thread]) {
    threads.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| a.thread_id.cmp(&b.thread_id))
    });
}

/// The palette's groups for a query.
pub fn rank(input: &Input) -> Vec<Group> {
    let query = input.query.trim();
    let (actions_only, query) = match query.strip_prefix('>') {
        Some(rest) => (true, rest.trim()),
        None => (false, query),
    };
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let mut seen = std::collections::BTreeSet::new();
    let mut threads: Vec<&Thread> = input
        .needs_you
        .iter()
        .chain(&input.threads)
        .filter(|thread| !thread.thread_id.is_empty() && seen.insert(thread.thread_id.as_str()))
        .collect();
    let needs_you: Vec<&Thread> = threads
        .iter()
        .copied()
        .filter(|thread| {
            input
                .needs_you
                .iter()
                .any(|n| n.thread_id == thread.thread_id)
        })
        .collect();
    let group = |title: &str, items: Vec<Item>| Group {
        title: title.into(),
        items,
    };
    if words.is_empty() {
        let actions: Vec<Item> = actions(input).into_iter().map(|(item, _)| item).collect();
        if actions_only {
            return vec![group("Actions", actions)];
        }
        let mut recent: Vec<&Thread> = threads
            .iter()
            .copied()
            .filter(|thread| !needs_you.iter().any(|n| n.thread_id == thread.thread_id))
            .collect();
        newest_first(&mut recent);
        return [
            group(
                "Needs you",
                needs_you.into_iter().map(thread_item).collect(),
            ),
            group(
                "Recent threads",
                recent.into_iter().take(THREADS).map(thread_item).collect(),
            ),
            group("Actions", actions),
        ]
        .into_iter()
        .filter(|group| !group.items.is_empty())
        .collect();
    }

    let mut scored_actions: Vec<(u32, Item)> = actions(input)
        .into_iter()
        .filter_map(|(item, keywords)| {
            score(&words, &item.title, keywords).map(|score| (score, item))
        })
        .collect();
    scored_actions.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    let mut groups: Vec<(u32, Group)> = Vec::new();
    let best = |items: &[(u32, Item)]| items.first().map_or(0, |(score, _)| *score);
    if !actions_only {
        newest_first(&mut threads);
        let mut scored_threads: Vec<(u32, Item)> = threads
            .iter()
            .filter_map(|thread| {
                score(
                    &words,
                    &thread.title,
                    &[&thread.project_name, &thread.status_label],
                )
                .map(|score| (score, thread_item(thread)))
            })
            .collect();
        // A stable sort keeps the newest first among equal matches.
        scored_threads.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        scored_threads.truncate(THREADS);
        let pages = PAGES
            .iter()
            .map(|(id, title, keywords, icon, shortcut)| {
                (
                    Item {
                        kind: Kind::Page,
                        id: (*id).into(),
                        title: (*title).into(),
                        detail: String::new(),
                        icon: (*icon).into(),
                        shortcut: shortcut.map(str::to_owned),
                    },
                    keywords.iter().map(|k| (*k).to_owned()).collect::<Vec<_>>(),
                )
            })
            .chain(input.pages.iter().map(|page| {
                (
                    Item {
                        kind: Kind::Page,
                        id: page.id.clone(),
                        title: page.title.clone(),
                        detail: page.detail.clone(),
                        icon: page.icon.clone(),
                        shortcut: page.shortcut.clone(),
                    },
                    page.keywords.clone(),
                )
            }));
        let mut scored_pages: Vec<(u32, Item)> = pages
            .filter_map(|(item, keywords)| {
                let mut others: Vec<&str> = keywords.iter().map(String::as_str).collect();
                others.push(&item.detail);
                score(&words, &item.title, &others).map(|score| (score, item))
            })
            .collect();
        scored_pages.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        for (title, items) in [("Threads", scored_threads), ("Pages", scored_pages)] {
            if !items.is_empty() {
                let top = best(&items);
                groups.push((
                    top,
                    group(title, items.into_iter().map(|(_, i)| i).collect()),
                ));
            }
        }
    }
    if !scored_actions.is_empty() {
        let top = best(&scored_actions);
        groups.push((
            top,
            group(
                "Actions",
                scored_actions.into_iter().map(|(_, item)| item).collect(),
            ),
        ));
    }
    // The group with the best match leads. Equal groups keep threads, pages,
    // then actions.
    groups.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    groups.into_iter().map(|(_, group)| group).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(id: &str, title: &str, project: &str, at: &str) -> Thread {
        Thread {
            thread_id: id.into(),
            title: title.into(),
            project_name: project.into(),
            updated_at: at.into(),
            ..Thread::default()
        }
    }

    fn input(query: &str) -> Input {
        Input {
            query: query.into(),
            needs_you: vec![Thread {
                status_label: "Approve".into(),
                ..thread("t-deploy", "Deploy runs", "Homelab", "2026-10-18T10:00:00Z")
            }],
            threads: vec![
                thread("t-deploy", "Deploy runs", "Homelab", "2026-10-18T10:00:00Z"),
                thread("t-old", "Release notes", "Muniment", "2026-10-10T10:00:00Z"),
                thread("t-new", "Release plan", "Muniment", "2026-10-18T12:00:00Z"),
                thread("t-trip", "Trip ideas", "Chats", "2026-10-17T12:00:00Z"),
            ],
            pages: vec![Page {
                id: "permissions".into(),
                title: "Permissions".into(),
                detail: "Settings".into(),
                keywords: vec!["bypass".into()],
                icon: "shield-check".into(),
                shortcut: None,
            }],
            thread: Some(OpenThread {
                thread_id: "t-trip".into(),
                pinned: true,
                movable: true,
            }),
            machines: 1,
            modes: true,
        }
    }

    fn titles(group: &Group) -> Vec<&str> {
        group.items.iter().map(|item| item.title.as_str()).collect()
    }

    #[test]
    fn the_empty_palette_lists_needs_you_then_recent_threads_then_actions() {
        let groups = rank(&input(""));
        let names: Vec<&str> = groups.iter().map(|group| group.title.as_str()).collect();
        assert_eq!(names, ["Needs you", "Recent threads", "Actions"]);
        assert_eq!(titles(&groups[0]), ["Deploy runs"]);
        assert_eq!(groups[0].items[0].detail, "Approve · Homelab");
        assert_eq!(
            titles(&groups[1]),
            ["Release plan", "Trip ideas", "Release notes"]
        );
        assert_eq!(
            titles(&groups[2]),
            [
                "New thread",
                "New project",
                "New agent",
                "Move to project",
                "Unpin thread",
                "Switch model",
                "Toggle permission mode"
            ]
        );
        assert_eq!(groups[2].items[0].shortcut.as_deref(), Some("mod+n"));
    }

    #[test]
    fn a_leading_angle_bracket_limits_results_to_actions() {
        let groups = rank(&input("> new"));
        assert_eq!(groups.len(), 1);
        assert_eq!(
            titles(&groups[0]),
            ["New thread", "New project", "New agent"]
        );
        let all = rank(&input(">"));
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].title, "Actions");
    }

    #[test]
    fn a_query_matches_titles_projects_and_synonyms_with_the_newest_first() {
        let groups = rank(&input("release"));
        assert_eq!(groups[0].title, "Threads");
        assert_eq!(titles(&groups[0]), ["Release plan", "Release notes"]);
        let by_project = rank(&input("homelab"));
        assert_eq!(titles(&by_project[0]), ["Deploy runs"]);
        let synonym = rank(&input("spend"));
        assert_eq!(titles(&synonym[0]), ["Usage"]);
        assert_eq!(synonym[0].items[0].shortcut.as_deref(), Some("mod+shift+u"));
        let settings_part = rank(&input("bypass"));
        let pages = settings_part.iter().find(|g| g.title == "Pages").unwrap();
        assert_eq!(titles(pages), ["Permissions"]);
        let both = rank(&input("rel not"));
        assert_eq!(titles(&both[0]), ["Release notes"]);
        assert!(rank(&input("zzzz")).is_empty());
    }

    /// The probe page shows these answers. `MUNIMENT_WRITE_FIXTURES=1`
    /// writes them again after a change to [`rank`].
    #[test]
    fn the_probe_fixture_matches_the_ranking() {
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../protocol-fixtures/palette");
        let base: serde_json::Value =
            serde_json::from_slice(&std::fs::read(folder.join("input.json")).unwrap()).unwrap();
        let mut ranked = serde_json::Map::new();
        for query in ["", ">", "re", "set", "> mo"] {
            let mut input = base.clone();
            input["query"] = serde_json::json!(query);
            let input: Input = serde_json::from_value(input).unwrap();
            ranked.insert(query.into(), serde_json::to_value(rank(&input)).unwrap());
        }
        let ranked = serde_json::to_string_pretty(&ranked).unwrap() + "\n";
        if std::env::var_os("MUNIMENT_WRITE_FIXTURES").is_some() {
            std::fs::write(folder.join("ranked.json"), &ranked).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(folder.join("ranked.json")).unwrap(),
            ranked
        );
    }

    #[test]
    fn actions_follow_what_applies_now() {
        let mut quiet = input("");
        quiet.thread = None;
        quiet.modes = false;
        quiet.machines = 2;
        let groups = rank(&quiet);
        let actions = groups.iter().find(|g| g.title == "Actions").unwrap();
        assert_eq!(
            titles(actions),
            [
                "New thread",
                "New project",
                "New agent",
                "Switch model",
                "Switch machine"
            ]
        );
        let settings = rank(&input("set"));
        assert_eq!(settings[0].title, "Pages");
        assert_eq!(settings[0].items[0].title, "Settings");
    }
}
