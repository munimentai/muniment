//! Where a fact came from: the user in Settings or in an agent's profile, an
//! ordinary chat, or a run of an agent. A fact file written without an
//! origin gets a best-effort one from its source text when it is read.
use crate::memory_files::{Fact, PendingChange};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// Who wrote a fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginKind {
    /// The user, in Settings or in an agent's profile.
    User,
    /// A run of a thread that belongs to no agent.
    Chat,
    /// A run of an agent: a routine, a manual run, the agent's chat, or a
    /// message or handoff from another agent.
    Agent,
    /// A fact without a recorded origin whose source text names none.
    Unknown,
}

/// Where the user wrote a fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    Settings,
    AgentProfile,
    /// An imported agent template that carried the fact.
    AgentImport,
}

impl Surface {
    fn as_str(self) -> &'static str {
        match self {
            Self::Settings => "settings",
            Self::AgentProfile => "agent_profile",
            Self::AgentImport => "agent_import",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "settings" => Some(Self::Settings),
            "agent_profile" => Some(Self::AgentProfile),
            "agent_import" => Some(Self::AgentImport),
            _ => None,
        }
    }
}

/// The origin of one fact. A fact file keeps it on its `Origin:` line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    pub kind: OriginKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<Surface>,
    /// The agent that wrote the fact, or whose profile the user wrote it in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
}

const MAX_ID: usize = 200;

impl OriginKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Chat => "chat",
            Self::Agent => "agent",
            Self::Unknown => "unknown",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "user" => Some(Self::User),
            "chat" => Some(Self::Chat),
            "agent" => Some(Self::Agent),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

impl Origin {
    fn of(kind: OriginKind) -> Self {
        Self {
            kind,
            surface: None,
            agent_id: None,
            thread_id: None,
            run_id: None,
        }
    }

    /// A fact the user wrote in Settings, or in the profile of `agent`.
    pub fn user(agent: Option<&str>) -> Self {
        Self {
            surface: Some(if agent.is_some() {
                Surface::AgentProfile
            } else {
                Surface::Settings
            }),
            agent_id: agent.map(str::to_owned),
            ..Self::of(OriginKind::User)
        }
    }

    /// A fact an imported template of `agent` carried.
    pub fn imported(agent: &str) -> Self {
        Self {
            surface: Some(Surface::AgentImport),
            agent_id: Some(agent.to_owned()),
            ..Self::of(OriginKind::User)
        }
    }

    /// A fact written during `run` of `thread`: an agent fact when the agent
    /// run store or the agent threads name an agent for them, else a chat
    /// fact.
    pub fn of_run(profile: &Path, thread: &str, run: &str) -> Self {
        let agent = Resolver::new(profile, None).agent_of(Some(thread), Some(run));
        Self {
            kind: if agent.is_some() {
                OriginKind::Agent
            } else {
                OriginKind::Chat
            },
            agent_id: agent,
            thread_id: Some(thread.to_owned()),
            run_id: Some(run.to_owned()),
            ..Self::of(OriginKind::Chat)
        }
    }

    /// Whether every id fits on the `Origin:` line.
    pub(crate) fn is_valid(&self) -> bool {
        [&self.agent_id, &self.thread_id, &self.run_id]
            .into_iter()
            .flatten()
            .all(|id| {
                !id.is_empty()
                    && id.len() <= MAX_ID
                    && !id.contains(|c: char| c.is_whitespace() || c == '=')
            })
    }

    /// The `Origin:` line value: the kind, then `key=value` for each field.
    pub(crate) fn to_line(&self) -> String {
        let mut line = self.kind.as_str().to_owned();
        if let Some(surface) = self.surface {
            line.push_str(&format!(" surface={}", surface.as_str()));
        }
        for (key, value) in [
            ("agent", &self.agent_id),
            ("thread", &self.thread_id),
            ("run", &self.run_id),
        ] {
            if let Some(value) = value {
                line.push_str(&format!(" {key}={value}"));
            }
        }
        line
    }

    pub(crate) fn from_line(line: &str) -> Option<Self> {
        let mut words = line.split_whitespace();
        let mut origin = Self::of(OriginKind::parse(words.next()?)?);
        for word in words {
            let (key, value) = word.split_once('=')?;
            let slot = match key {
                "surface" => {
                    origin.surface = Some(Surface::parse(value)?);
                    continue;
                }
                "agent" => &mut origin.agent_id,
                "thread" => &mut origin.thread_id,
                "run" => &mut origin.run_id,
                _ => return None,
            };
            if slot.is_some() {
                return None;
            }
            *slot = Some(value.to_owned());
        }
        origin.is_valid().then_some(origin)
    }
}

/// A fact as memory responses answer it: its origin, recorded or inferred,
/// and the label that names it.
#[derive(Clone, Debug, Serialize)]
pub struct DescribedFact {
    #[serde(flatten)]
    pub fact: Fact,
    pub origin_label: String,
}

/// A proposed change as memory responses answer it.
#[derive(Clone, Debug, Serialize)]
pub struct DescribedChange {
    #[serde(flatten)]
    pub change: PendingChange,
    pub origin_label: String,
}

/// Fills in origins and labels for facts read from the user's memory, or
/// from the memory of the agent `scope`.
pub fn describe_facts(profile: &Path, scope: Option<&str>, facts: Vec<Fact>) -> Vec<DescribedFact> {
    let mut resolver = Resolver::new(profile, scope);
    facts
        .into_iter()
        .map(|fact| resolver.describe(fact))
        .collect()
}

pub fn describe_fact(profile: &Path, scope: Option<&str>, fact: Fact) -> DescribedFact {
    Resolver::new(profile, scope).describe(fact)
}

pub fn describe_changes(
    profile: &Path,
    scope: Option<&str>,
    changes: Vec<PendingChange>,
) -> Vec<DescribedChange> {
    let mut resolver = Resolver::new(profile, scope);
    changes
        .into_iter()
        .map(|mut change| {
            let origin = resolver.resolve(&change.source, change.origin.as_ref());
            let origin_label = resolver.label(&origin, &change.source);
            change.origin = Some(origin);
            DescribedChange {
                change,
                origin_label,
            }
        })
        .collect()
}

/// Reads the agent records an origin needs once per response.
struct Resolver<'a> {
    profile: &'a Path,
    scope: Option<&'a str>,
    threads: Option<BTreeMap<String, String>>,
    names: BTreeMap<String, Option<String>>,
}

impl<'a> Resolver<'a> {
    fn new(profile: &'a Path, scope: Option<&'a str>) -> Self {
        Self {
            profile,
            scope,
            threads: None,
            names: BTreeMap::new(),
        }
    }

    fn describe(&mut self, mut fact: Fact) -> DescribedFact {
        let origin = self.resolve(&fact.source, fact.origin.as_ref());
        let origin_label = self.label(&origin, &fact.source);
        fact.origin = Some(origin);
        DescribedFact { fact, origin_label }
    }

    /// The agent of a run from the agent run store, or of a thread from the
    /// agent threads. A missing store means no agent ever ran.
    fn agent_of(&mut self, thread: Option<&str>, run: Option<&str>) -> Option<String> {
        if let Some(run) = run {
            if self.profile.join(crate::agent_runs::STORE_FILE).is_file() {
                if let Ok(Some(found)) =
                    crate::agent_runs::RunStore::open(self.profile).and_then(|store| store.get(run))
                {
                    return Some(found.agent_id);
                }
            }
        }
        let thread = thread?;
        let threads = self.threads.get_or_insert_with(|| {
            crate::agents::state(self.profile)
                .map(|state| state.threads)
                .unwrap_or_default()
        });
        threads.get(thread).cloned()
    }

    /// The recorded origin, or one inferred from the source text of a fact
    /// written before origins were recorded.
    fn resolve(&mut self, source: &str, recorded: Option<&Origin>) -> Origin {
        if let Some(origin) = recorded {
            return origin.clone();
        }
        let source = source.trim();
        if source.eq_ignore_ascii_case("Added in settings") {
            return Origin::user(self.scope);
        }
        if source == "Imported agent template" {
            if let Some(agent) = self.scope {
                return Origin::imported(agent);
            }
        }
        let Some((thread, run)) = legacy_run(source) else {
            return Origin::of(OriginKind::Unknown);
        };
        // A fact in an agent's own memory came from that agent's runs.
        let agent = self
            .agent_of(Some(thread), run)
            .or_else(|| self.scope.map(str::to_owned));
        Origin {
            kind: if agent.is_some() {
                OriginKind::Agent
            } else {
                OriginKind::Chat
            },
            agent_id: agent,
            thread_id: Some(thread.to_owned()),
            run_id: run.map(str::to_owned),
            surface: None,
        }
    }

    fn label(&mut self, origin: &Origin, source: &str) -> String {
        let by_agent =
            |resolver: &mut Self| match origin.agent_id.as_deref().and_then(|id| resolver.name(id))
            {
                Some(name) => format!("Added by {name} agent"),
                None => "Added by an agent".to_owned(),
            };
        match (origin.kind, origin.surface) {
            (OriginKind::User, Some(Surface::AgentProfile | Surface::AgentImport)) => {
                by_agent(self)
            }
            (OriginKind::User, _) => "Added in Settings".to_owned(),
            (OriginKind::Agent, _) => by_agent(self),
            (OriginKind::Chat, _) => "Added in a chat".to_owned(),
            (OriginKind::Unknown, _) => source.trim().to_owned(),
        }
    }

    /// The agent's current name, or none for a deleted agent.
    fn name(&mut self, id: &str) -> Option<String> {
        let profile = self.profile;
        self.names
            .entry(id.to_owned())
            .or_insert_with(|| {
                crate::agents::get(profile, id)
                    .ok()
                    .map(|agent| agent.name.trim().to_owned())
                    .filter(|name| !name.is_empty())
            })
            .clone()
    }
}

/// The thread and run a source written before origins names:
/// `extracted thread:<id> run:<id>` from the after-run pass, or
/// `thread:<id> run:<id>` from the model's memory save.
fn legacy_run(source: &str) -> Option<(&str, Option<&str>)> {
    let rest = source.strip_prefix("extracted ").unwrap_or(source);
    let mut words = rest.split_whitespace();
    let thread = words.next()?.strip_prefix("thread:")?;
    let run = match words.next() {
        Some(word) => Some(word.strip_prefix("run:")?),
        None => None,
    };
    if words.next().is_some() || thread.is_empty() || run.is_some_and(str::is_empty) {
        return None;
    }
    Some((thread, run))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_files::{self, Trust};
    use std::fs;

    struct Fixture {
        root: std::path::PathBuf,
        profile: std::path::PathBuf,
        home: std::path::PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
            fs::create_dir_all(&root).unwrap();
            let profile = root.join("private");
            let home = root.join("muniment");
            crate::home::confirm_home(&profile, &home).unwrap();
            Self {
                root,
                profile,
                home,
            }
        }

        fn agent(&self, name: &str) -> String {
            crate::agents::save(
                &self.profile,
                crate::agents::Agent {
                    name: name.into(),
                    instructions: "Summarize the morning.".into(),
                    ..Default::default()
                },
            )
            .unwrap()
            .id
        }

        fn write(&self, home: &Path, source: &str) -> String {
            let id = uuid::Uuid::new_v4().to_string();
            fs::create_dir_all(home.join("memory/facts")).unwrap();
            fs::write(
                home.join(format!("memory/facts/{id}.md")),
                format!("# {source}\n\nSource: {source}\nTrust: user\n\nA fact.\n"),
            )
            .unwrap();
            id
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn origin(kind: OriginKind) -> Origin {
        Origin::of(kind)
    }

    #[test]
    fn origin_lines_round_trip_and_refuse_malformed_text() {
        let agent = Origin {
            agent_id: Some("a1".into()),
            thread_id: Some("t1".into()),
            run_id: Some("r1".into()),
            ..origin(OriginKind::Agent)
        };
        assert_eq!(agent.to_line(), "agent agent=a1 thread=t1 run=r1");
        assert_eq!(Origin::user(None).to_line(), "user surface=settings");
        assert_eq!(
            Origin::user(Some("a1")).to_line(),
            "user surface=agent_profile agent=a1"
        );
        for origin in [agent, Origin::user(None), Origin::user(Some("a1"))] {
            assert_eq!(Origin::from_line(&origin.to_line()), Some(origin));
        }
        for line in [
            "",
            "admin",
            "agent agent",
            "agent agent=",
            "agent agent=a agent=b",
            "user surface=desk",
            "chat color=blue",
        ] {
            assert_eq!(Origin::from_line(line), None, "{line}");
        }
        assert!(!Origin {
            thread_id: Some("two words".into()),
            ..origin(OriginKind::Chat)
        }
        .is_valid());
    }

    #[test]
    fn a_saved_origin_reaches_the_file_and_reads_back() {
        let fixture = Fixture::new();
        let saved = memory_files::fact_save(
            &fixture.profile,
            memory_files::Fact {
                id: String::new(),
                title: "Editor".into(),
                content: "Uses Helix.".into(),
                source: "Added in settings".into(),
                trust: Trust::External,
                origin: Some(origin(OriginKind::Agent)),
            },
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(fixture.home.join(format!("memory/facts/{}.md", saved.id)))
                .unwrap(),
            "# Editor\n\nSource: Added in settings\nTrust: user\nOrigin: user surface=settings\n\nUses Helix.\n"
        );
        let facts = memory_files::facts(&fixture.profile).unwrap();
        assert_eq!(facts[0].origin, Some(Origin::user(None)));
        let described = describe_facts(&fixture.profile, None, facts);
        assert_eq!(described[0].origin_label, "Added in Settings");
        let value = serde_json::to_value(&described[0]).unwrap();
        assert_eq!(
            value["origin"],
            serde_json::json!({"kind": "user", "surface": "settings"})
        );
        assert_eq!(value["origin_label"], "Added in Settings");
        assert!(memory_files::fact_save_in(
            &fixture.profile,
            &fixture.home,
            memory_files::Fact {
                id: String::new(),
                title: "Bad".into(),
                content: "Bad origin.".into(),
                source: "test".into(),
                trust: Trust::User,
                origin: Some(Origin {
                    run_id: Some("a b".into()),
                    ..origin(OriginKind::Chat)
                }),
            },
        )
        .is_err());
    }

    #[test]
    fn old_facts_get_an_origin_from_their_source_text() {
        let fixture = Fixture::new();
        let agent = fixture.agent("Tauri Morning");
        crate::agents::assign(&fixture.profile, "agent-thread", &agent).unwrap();
        let run = crate::agents::run_now(&fixture.profile, &agent, None, Some("desktop")).unwrap();
        let settings = fixture.write(&fixture.home, "Added in settings");
        let agent_thread = fixture.write(&fixture.home, "extracted thread:agent-thread run:r1");
        let agent_run = fixture.write(&fixture.home, &format!("thread:routine run:{}", run.id));
        let chat = fixture.write(&fixture.home, "extracted thread:chat-thread run:r2");
        let other = fixture.write(&fixture.home, "Saved from the old importer");
        let described = describe_facts(
            &fixture.profile,
            None,
            memory_files::facts_in(&fixture.home).unwrap(),
        );
        let find = |id: &str| described.iter().find(|fact| fact.fact.id == id).unwrap();
        assert_eq!(find(&settings).fact.origin, Some(Origin::user(None)));
        assert_eq!(find(&settings).origin_label, "Added in Settings");
        assert_eq!(
            find(&agent_thread).fact.origin,
            Some(Origin {
                agent_id: Some(agent.clone()),
                thread_id: Some("agent-thread".into()),
                run_id: Some("r1".into()),
                ..origin(OriginKind::Agent)
            })
        );
        assert_eq!(
            find(&agent_thread).origin_label,
            "Added by Tauri Morning agent"
        );
        let by_run = find(&agent_run).fact.origin.clone().unwrap();
        assert_eq!(
            (by_run.kind, by_run.agent_id.as_deref()),
            (OriginKind::Agent, Some(agent.as_str()))
        );
        assert_eq!(
            find(&chat).fact.origin,
            Some(Origin {
                thread_id: Some("chat-thread".into()),
                run_id: Some("r2".into()),
                ..origin(OriginKind::Chat)
            })
        );
        assert_eq!(find(&chat).origin_label, "Added in a chat");
        assert_eq!(find(&other).fact.origin, Some(origin(OriginKind::Unknown)));
        assert_eq!(find(&other).origin_label, "Saved from the old importer");
        // Reading infers the origin without rewriting the file.
        assert!(
            !fs::read_to_string(fixture.home.join(format!("memory/facts/{settings}.md")))
                .unwrap()
                .contains("Origin:")
        );

        // In an agent's own memory, a settings fact was added in its profile
        // and a run fact came from that agent even when no record names it.
        let (agent_home, _) = crate::agents::memory_paths(&fixture.profile, &agent).unwrap();
        let profile_fact = fixture.write(&agent_home, "Added in settings");
        let unmapped = fixture.write(&agent_home, "thread:gone run:gone");
        let described = describe_facts(
            &fixture.profile,
            Some(&agent),
            memory_files::facts_in(&agent_home).unwrap(),
        );
        let find = |id: &str| described.iter().find(|fact| fact.fact.id == id).unwrap();
        assert_eq!(
            find(&profile_fact).fact.origin,
            Some(Origin::user(Some(&agent)))
        );
        assert_eq!(
            find(&profile_fact).origin_label,
            "Added by Tauri Morning agent"
        );
        assert_eq!(
            find(&unmapped).fact.origin.as_ref().unwrap().agent_id,
            Some(agent.clone())
        );
    }

    #[test]
    fn a_deleted_agent_is_named_as_an_agent() {
        let fixture = Fixture::new();
        let agent = fixture.agent("Tauri Morning");
        let written = Origin {
            agent_id: Some(agent.clone()),
            ..origin(OriginKind::Agent)
        };
        let label = |origin: &Origin| Resolver::new(&fixture.profile, None).label(origin, "unused");
        assert_eq!(label(&written), "Added by Tauri Morning agent");
        assert_eq!(
            label(&Origin::user(Some(&agent))),
            "Added by Tauri Morning agent"
        );
        assert_eq!(
            label(&Origin::imported(&agent)),
            "Added by Tauri Morning agent"
        );
        crate::agents::delete(&fixture.profile, &agent).unwrap();
        assert_eq!(label(&written), "Added by an agent");
        assert_eq!(label(&Origin::user(Some(&agent))), "Added by an agent");
    }

    #[test]
    fn a_run_origin_names_its_agent_from_the_run_store_or_the_agent_threads() {
        let fixture = Fixture::new();
        assert_eq!(
            Origin::of_run(&fixture.profile, "chat", "r1"),
            Origin {
                thread_id: Some("chat".into()),
                run_id: Some("r1".into()),
                ..origin(OriginKind::Chat)
            }
        );
        assert!(!fixture.profile.join(crate::agent_runs::STORE_FILE).exists());
        let agent = fixture.agent("Tauri Morning");
        crate::agents::assign(&fixture.profile, "agent-chat", &agent).unwrap();
        let by_thread = Origin::of_run(&fixture.profile, "agent-chat", "r2");
        assert_eq!(
            (by_thread.kind, by_thread.agent_id.as_deref()),
            (OriginKind::Agent, Some(agent.as_str()))
        );
        let run = crate::agents::run_now(&fixture.profile, &agent, None, Some("desktop")).unwrap();
        let by_run = Origin::of_run(&fixture.profile, "unassigned", &run.id);
        assert_eq!(
            (by_run.kind, by_run.agent_id.as_deref()),
            (OriginKind::Agent, Some(agent.as_str()))
        );
    }
}
