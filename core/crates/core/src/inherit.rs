//! Where each value a thread runs with comes from. A value is the thread's
//! own, its agent's, its project's, or the machine's default, and every view
//! that shows a value names its layer, such as `GPT-6 Astra · from project
//! Homelab`. A thread's own choice never writes back to a lower layer.
use crate::agents::{AgentModel, AgentTools};
use crate::projects::{DefaultModel, ToolPolicy};
use crate::thread_settings::{Seed, ThreadSettings};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The layer a value comes from, nearest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    Thread,
    Agent,
    Project,
    Default,
}

/// A thread's model and where it comes from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ThreadModel {
    /// None when no layer names a model.
    pub model: Option<DefaultModel>,
    pub layer: Layer,
    /// The project of a project layer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

/// The model a thread runs: its own choice, else its agent's, else the
/// machine's default. A model the thread started with from its project
/// reads as the project's until the user picks another.
pub fn model(
    settings: &ThreadSettings,
    agent: Option<&AgentModel>,
    default: Option<DefaultModel>,
) -> ThreadModel {
    if let Some(model) = &settings.model {
        let seeded = settings
            .seed
            .as_ref()
            .filter(|seed| seed.model.as_ref() == Some(model));
        return ThreadModel {
            model: Some(model.clone()),
            layer: if seeded.is_some() {
                Layer::Project
            } else {
                Layer::Thread
            },
            project_id: seeded.map(|seed| seed.project_id.clone()),
        };
    }
    if let Some(agent) = agent {
        return ThreadModel {
            model: Some(DefaultModel {
                provider: agent.provider.clone(),
                model: agent.id.clone(),
            }),
            layer: Layer::Agent,
            project_id: None,
        };
    }
    ThreadModel {
        model: default,
        layer: Layer::Default,
        project_id: None,
    }
}

/// The layer of a thread's permission mode, from core's `source` for it
/// (`thread`, `agent` or `default`), and the project of a project layer. A
/// mode the thread started with from its project reads as the project's
/// until the user picks another.
pub fn mode_layer(source: &str, mode: &str, seed: Option<&Seed>) -> (Layer, Option<String>) {
    match source {
        "thread" => match seed.filter(|seed| seed.permission_mode.as_deref() == Some(mode)) {
            Some(seed) => (Layer::Project, Some(seed.project_id.clone())),
            None => (Layer::Thread, None),
        },
        "agent" => (Layer::Agent, None),
        _ => (Layer::Default, None),
    }
}

/// The tools a run leaves out: its project's, with its agent's choices
/// over them.
pub fn tool_policy(project: &ToolPolicy, agent: Option<&AgentTools>) -> ToolPolicy {
    let empty = AgentTools::default();
    let agent = agent.unwrap_or(&empty);
    let allowed: BTreeSet<&String> = agent.allowed.iter().collect();
    let merge = |project: &[String], agent: &[String]| {
        let mut out: Vec<String> = Vec::new();
        for id in project.iter().chain(agent) {
            if !allowed.contains(id) && !out.contains(id) {
                out.push(id.clone());
            }
        }
        out
    };
    ToolPolicy {
        disabled_tools: merge(&project.disabled_tools, &agent.disabled),
        disabled_extensions: merge(&project.disabled_extensions, &agent.disabled_extensions),
    }
}

/// One built-in tool, MCP server, plugin or skill, as `agent.tool_catalog`
/// lists it.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct CatalogEntry {
    pub id: String,
    pub label: String,
    pub name: String,
    pub server: String,
}

/// What `agent.tool_catalog` answers.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Catalog {
    pub builtin: Vec<CatalogEntry>,
    pub mcp: Vec<CatalogEntry>,
    pub plugins: Vec<CatalogEntry>,
    pub skills: Vec<CatalogEntry>,
}

/// The kind of a tool row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Builtin,
    Mcp,
    Plugin,
    Skill,
}

/// One tool, whether a run may use it, and the layer that decides.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ToolRow {
    pub id: String,
    pub kind: ToolKind,
    pub label: String,
    pub allowed: bool,
    /// `agent` for an agent's own choice, `project` for a tool the project
    /// leaves out, else `default`.
    pub layer: Layer,
}

/// What [`tools`] reads: a machine's tool catalog, a project's policy and,
/// for an agent's card, the agent's choices.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct ToolsInput {
    pub catalog: Catalog,
    pub policy: ToolPolicy,
    pub agent: Option<AgentTools>,
}

/// Every tool of a catalog with whether the project, and the agent when
/// one is named, allows it, in catalog order: built-in tools, MCP servers,
/// plugins, then skills. An MCP server listed without an id has no
/// project choice and is left out.
pub fn tools(input: &ToolsInput) -> Vec<ToolRow> {
    let agent = input.agent.as_ref();
    let row = |id: &str, kind: ToolKind, label: &str| {
        let (project_list, agent_off) = match kind {
            ToolKind::Builtin => (
                &input.policy.disabled_tools,
                agent.is_some_and(|agent| agent.disabled.iter().any(|tool| tool == id)),
            ),
            _ => (
                &input.policy.disabled_extensions,
                agent.is_some_and(|agent| agent.disabled_extensions.iter().any(|ext| ext == id)),
            ),
        };
        let agent_on = agent.is_some_and(|agent| agent.allowed.iter().any(|entry| entry == id));
        let project_off = project_list.iter().any(|entry| entry == id);
        let (allowed, layer) = if agent_on {
            (true, Layer::Agent)
        } else if agent_off {
            (false, Layer::Agent)
        } else if project_off {
            (false, Layer::Project)
        } else {
            (true, Layer::Default)
        };
        ToolRow {
            id: id.to_owned(),
            kind,
            label: label.to_owned(),
            allowed,
            layer,
        }
    };
    let label = |entry: &CatalogEntry| {
        [&entry.label, &entry.name, &entry.server, &entry.id]
            .into_iter()
            .find(|text| !text.trim().is_empty())
            .cloned()
            .unwrap_or_default()
    };
    let catalog = &input.catalog;
    let groups = [
        (&catalog.builtin, ToolKind::Builtin),
        (&catalog.mcp, ToolKind::Mcp),
        (&catalog.plugins, ToolKind::Plugin),
        (&catalog.skills, ToolKind::Skill),
    ];
    groups
        .into_iter()
        .flat_map(|(entries, kind)| {
            entries
                .iter()
                .filter(|entry| !entry.id.is_empty())
                .map(move |entry| (entry, kind))
        })
        .map(|(entry, kind)| row(&entry.id, kind, &label(entry)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str) -> DefaultModel {
        DefaultModel {
            provider: "openai".into(),
            model: id.into(),
        }
    }

    fn seed() -> Seed {
        Seed {
            project_id: "p-homelab".into(),
            model: Some(model("gpt-6-astra")),
            permission_mode: Some("manual".into()),
            machine: None,
        }
    }

    #[test]
    fn a_thread_model_names_the_layer_it_comes_from() {
        let default = Some(model("gpt-6-luna"));
        let fresh = ThreadSettings::default();
        assert_eq!(model_layer(&fresh, None, default.clone()), Layer::Default);
        let agent = AgentModel {
            provider: "anthropic".into(),
            id: "opus".into(),
            thinking: None,
        };
        let from_agent = super::model(&fresh, Some(&agent), default.clone());
        assert_eq!(from_agent.layer, Layer::Agent);
        assert_eq!(from_agent.model.unwrap().model, "opus");
        let seeded = ThreadSettings {
            model: Some(model("gpt-6-astra")),
            seed: Some(seed()),
            ..ThreadSettings::default()
        };
        let from_project = super::model(&seeded, Some(&agent), default.clone());
        assert_eq!(from_project.layer, Layer::Project);
        assert_eq!(from_project.project_id.as_deref(), Some("p-homelab"));
        let picked = ThreadSettings {
            model: Some(model("gpt-6-nova")),
            ..seeded
        };
        let own = super::model(&picked, None, default);
        assert_eq!((own.layer, own.project_id), (Layer::Thread, None));
    }

    fn model_layer(
        settings: &ThreadSettings,
        agent: Option<&AgentModel>,
        default: Option<DefaultModel>,
    ) -> Layer {
        super::model(settings, agent, default).layer
    }

    #[test]
    fn a_seeded_mode_reads_as_the_project_until_the_user_picks_another() {
        let seed = seed();
        assert_eq!(
            mode_layer("thread", "manual", Some(&seed)),
            (Layer::Project, Some("p-homelab".into()))
        );
        assert_eq!(
            mode_layer("thread", "bypass", Some(&seed)),
            (Layer::Thread, None)
        );
        assert_eq!(mode_layer("agent", "manual", Some(&seed)).0, Layer::Agent);
        assert_eq!(mode_layer("default", "bypass", None).0, Layer::Default);
    }

    #[test]
    fn an_agent_turns_project_tools_on_and_off_over_its_project() {
        let project = ToolPolicy {
            disabled_tools: vec!["bash".into(), "write".into()],
            disabled_extensions: vec!["mcp-crm".into()],
        };
        let agent = AgentTools {
            disabled: vec!["edit".into()],
            disabled_extensions: vec!["mcp-mail".into()],
            allowed: vec!["bash".into(), "mcp-crm".into()],
        };
        let policy = tool_policy(&project, Some(&agent));
        assert_eq!(policy.disabled_tools, vec!["write", "edit"]);
        assert_eq!(policy.disabled_extensions, vec!["mcp-mail"]);
        assert_eq!(tool_policy(&project, None), project);

        let entry = |id: &str, label: &str| CatalogEntry {
            id: id.into(),
            label: label.into(),
            ..CatalogEntry::default()
        };
        let catalog = Catalog {
            builtin: vec![
                entry("bash", "Shell"),
                entry("write", "Write"),
                entry("edit", "Edit"),
                entry("read", "Read"),
            ],
            mcp: vec![
                entry("mcp-crm", "CRM"),
                entry("mcp-mail", "Mail"),
                CatalogEntry {
                    server: "unlisted".into(),
                    ..CatalogEntry::default()
                },
            ],
            plugins: vec![entry("plug", "Writer")],
            skills: vec![entry("plug:skills/a/SKILL.md", "Outline")],
        };
        let rows = tools(&ToolsInput {
            catalog: catalog.clone(),
            policy: project.clone(),
            agent: Some(agent),
        });
        let summary: Vec<(&str, bool, Layer)> = rows
            .iter()
            .map(|row| (row.id.as_str(), row.allowed, row.layer))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("bash", true, Layer::Agent),
                ("write", false, Layer::Project),
                ("edit", false, Layer::Agent),
                ("read", true, Layer::Default),
                ("mcp-crm", true, Layer::Agent),
                ("mcp-mail", false, Layer::Agent),
                ("plug", true, Layer::Default),
                ("plug:skills/a/SKILL.md", true, Layer::Default),
            ]
        );
        assert_eq!(rows[0].kind, ToolKind::Builtin);
        assert_eq!(rows[7].kind, ToolKind::Skill);
        let project_rows = tools(&ToolsInput {
            catalog,
            policy: project,
            agent: None,
        });
        assert_eq!(
            project_rows[0].layer,
            Layer::Project,
            "the project page reads its own choice"
        );
    }
}
