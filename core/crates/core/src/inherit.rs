//! Where each value a thread runs with comes from. A value is the thread's
//! own, its agent's, its project's, or the machine's default, and every view
//! that shows a value names its layer, such as `GPT-6 Astra · from project
//! Homelab`. A thread's own choice never writes back to a lower layer.
use crate::agents::AgentModel;
use crate::projects::DefaultModel;
use crate::thread_settings::{Seed, ThreadSettings};
use serde::{Deserialize, Serialize};

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
}
