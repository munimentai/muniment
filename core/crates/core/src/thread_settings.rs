//! What each thread keeps for itself: the model it runs, and the project
//! defaults it started with. A thread's model never changes the machine's
//! default model or its project's defaults, and a project's defaults reach a
//! thread once, when the thread starts in it.
use crate::projects::DefaultModel;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path, sync::Mutex};

static WRITE: Mutex<()> = Mutex::new(());
const FILE: &str = "thread-settings.json";
/// The most threads the file keeps. The ones changed longest ago go first.
const MAX_THREADS: usize = 5000;

/// The project defaults a thread started with.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seed {
    /// A catalog project id.
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<DefaultModel>,
    /// A permission mode id from `permission.modes`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    /// The runtime id of the machine the project starts its threads on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<String>,
}

/// One thread's own settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadSettings {
    /// The model the thread runs. None runs its agent's model or the
    /// machine's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<DefaultModel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<Seed>,
    /// When the settings last changed, in RFC 3339.
    #[serde(default)]
    pub changed_at: String,
}

fn load(profile: &Path) -> Result<BTreeMap<String, ThreadSettings>, String> {
    match fs::read(profile.join(FILE)) {
        Ok(bytes) => {
            let mut all: BTreeMap<String, ThreadSettings> = serde_json::from_slice(&bytes)
                .map_err(|_| "Thread settings cannot be read.".to_string())?;
            // A thread seeded by the retired Chats project keeps its model as
            // its own: it belongs to no project.
            for settings in all.values_mut() {
                if settings
                    .seed
                    .as_ref()
                    .is_some_and(|seed| seed.project_id == crate::projects::RETIRED_CHATS)
                {
                    settings.seed = None;
                }
            }
            Ok(all)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(_) => Err("Thread settings cannot be read.".into()),
    }
}

fn change(
    profile: &Path,
    thread: &str,
    edit: impl FnOnce(&mut ThreadSettings),
) -> Result<ThreadSettings, String> {
    if thread.is_empty()
        || thread.len() > 128
        || !thread
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err("The thread identifier is invalid.".into());
    }
    let _guard = WRITE.lock().map_err(|_| "Thread settings are busy.")?;
    let mut all = load(profile)?;
    let entry = all.entry(thread.to_owned()).or_default();
    edit(entry);
    entry.changed_at = chrono::Utc::now().to_rfc3339();
    let settings = entry.clone();
    if all.len() > MAX_THREADS {
        let mut times: Vec<(String, String)> = all
            .iter()
            .map(|(id, entry)| (entry.changed_at.clone(), id.clone()))
            .collect();
        times.sort();
        for (_, id) in times.into_iter().take(all.len() - MAX_THREADS) {
            all.remove(&id);
        }
    }
    let failed = || "Thread settings cannot be saved.".to_owned();
    fs::create_dir_all(profile).map_err(|_| failed())?;
    let path = profile.join(FILE);
    if path.is_symlink() {
        return Err(failed());
    }
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = fs::write(&temporary, serde_json::to_vec(&all).map_err(|_| failed())?)
        .and_then(|()| crate::atomic_file::replace(&temporary, &path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|_| failed())?;
    Ok(settings)
}

/// A thread's settings. A thread with none has the default.
pub fn read(profile: &Path, thread: &str) -> Result<ThreadSettings, String> {
    Ok(load(profile)?.remove(thread).unwrap_or_default())
}

/// Sets the model a thread runs. None returns it to the model it started
/// with: its project's, or no model of its own.
pub fn set_model(
    profile: &Path,
    thread: &str,
    model: Option<DefaultModel>,
) -> Result<ThreadSettings, String> {
    change(profile, thread, |settings| {
        settings.model =
            model.or_else(|| settings.seed.as_ref().and_then(|seed| seed.model.clone()));
    })
}

/// Starts a thread with its project's defaults. The thread runs the
/// project's model from now on, until the user picks another.
pub fn seed(profile: &Path, thread: &str, seed: Seed) -> Result<ThreadSettings, String> {
    change(profile, thread, |settings| {
        settings.model = seed.model.clone();
        settings.seed = Some(seed);
    })
}

/// A `provider/model` value as a model.
fn split(value: &str) -> Option<DefaultModel> {
    let (provider, model) = value.split_once('/')?;
    Some(DefaultModel {
        provider: provider.into(),
        model: model.into(),
    })
}

/// The model a thread runs and its layer, from the thread's settings, its
/// agent's model and the machine's default, `default` as `provider/model`.
pub fn resolve(
    profile: &Path,
    thread: &str,
    agent: Option<&crate::agents::AgentModel>,
    default: Option<&str>,
) -> Result<(ThreadSettings, crate::inherit::ThreadModel), String> {
    let settings = read(profile, thread)?;
    let resolved = crate::inherit::model(&settings, agent, default.and_then(split));
    Ok((settings, resolved))
}

/// The model a run of `thread` answers with, as `provider/model`.
pub fn run_model(
    profile: &Path,
    thread: &str,
    agent: Option<&crate::agents::AgentModel>,
    default: Option<&str>,
) -> Option<String> {
    let resolved = match resolve(profile, thread, agent, default) {
        Ok((_, resolved)) => resolved,
        // Unreadable settings leave the thread on its agent's or the default model.
        Err(_) => crate::inherit::model(&ThreadSettings::default(), agent, default.and_then(split)),
    };
    resolved
        .model
        .map(|model| format!("{}/{}", model.provider, model.model))
}

/// Answers `thread_settings` and `thread_settings_set` for the runtime
/// whose default model is `default`, as `provider/model`.
pub fn operation(
    profile: &Path,
    set: bool,
    body: serde_json::Value,
    default: Option<&str>,
) -> Result<serde_json::Value, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Request {
        thread_id: String,
        #[serde(default)]
        model: Option<DefaultModel>,
        /// Returns the thread to the model it started with.
        #[serde(default)]
        reset: bool,
        #[serde(default)]
        seed: Option<Seed>,
    }
    let request: Request =
        serde_json::from_value(body).map_err(|_| "The request is invalid.".to_string())?;
    if !set && (request.model.is_some() || request.reset || request.seed.is_some()) {
        return Err("The request is invalid.".into());
    }
    if let Some(model) = &request.model {
        if model.provider.trim().is_empty()
            || model.model.trim().is_empty()
            || model.provider.len() > 128
            || model.model.len() > 512
        {
            return Err("Choose a model the picker lists.".into());
        }
    }
    if let Some(seed) = request.seed {
        seed_thread(profile, &request.thread_id, seed)?;
    }
    if request.model.is_some() || request.reset {
        set_model(profile, &request.thread_id, request.model)?;
    }
    let agent = crate::agents::thread_agent(profile, &request.thread_id)?;
    let (settings, resolved) = resolve(
        profile,
        &request.thread_id,
        agent.as_ref().and_then(|agent| agent.model.as_ref()),
        default,
    )?;
    let mut answer = serde_json::json!({
        "thread_id": request.thread_id,
        "model": resolved.model,
        "layer": resolved.layer,
    });
    if let Some(project) = resolved.project_id {
        answer["project_id"] = serde_json::json!(project);
    }
    if let Some(seed) = settings.seed {
        answer["seed"] = serde_json::json!(seed);
    }
    Ok(answer)
}

fn seed_thread(profile: &Path, thread: &str, seed: Seed) -> Result<ThreadSettings, String> {
    if seed.project_id.trim().is_empty() || seed.project_id.len() > 128 {
        return Err("The project id is invalid.".into());
    }
    self::seed(profile, thread, seed)
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

    #[test]
    fn a_thread_keeps_its_model_and_resets_to_the_one_it_started_with() {
        let profile =
            std::env::temp_dir().join(format!("muniment-thread-settings-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&profile).unwrap();
        assert_eq!(read(&profile, "t-1").unwrap(), ThreadSettings::default());
        let seeded = seed(
            &profile,
            "t-1",
            Seed {
                project_id: "p-1".into(),
                model: Some(model("gpt-6-astra")),
                permission_mode: Some("manual".into()),
                machine: None,
            },
        )
        .unwrap();
        assert_eq!(seeded.model, Some(model("gpt-6-astra")));
        set_model(&profile, "t-1", Some(model("gpt-6-luna"))).unwrap();
        set_model(&profile, "t-2", Some(model("gpt-6-nova"))).unwrap();
        assert_eq!(
            read(&profile, "t-1").unwrap().model,
            Some(model("gpt-6-luna"))
        );
        assert_eq!(
            read(&profile, "t-2").unwrap().model,
            Some(model("gpt-6-nova"))
        );
        assert_eq!(
            set_model(&profile, "t-1", None).unwrap().model,
            Some(model("gpt-6-astra"))
        );
        assert_eq!(set_model(&profile, "t-2", None).unwrap().model, None);
        assert!(set_model(&profile, "../t", None).is_err());
        let _ = fs::remove_dir_all(profile);
    }

    #[test]
    fn a_seed_from_the_retired_chats_project_leaves_the_model_as_the_threads_own() {
        let profile =
            std::env::temp_dir().join(format!("muniment-thread-chats-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&profile).unwrap();
        seed(
            &profile,
            "t-1",
            Seed {
                project_id: crate::projects::RETIRED_CHATS.into(),
                model: Some(model("gpt-6-astra")),
                permission_mode: None,
                machine: None,
            },
        )
        .unwrap();
        let settings = read(&profile, "t-1").unwrap();
        assert_eq!(settings.seed, None);
        assert_eq!(settings.model, Some(model("gpt-6-astra")));
        let _ = fs::remove_dir_all(profile);
    }

    #[test]
    fn the_operation_names_the_layer_and_never_moves_the_default() {
        let profile =
            std::env::temp_dir().join(format!("muniment-thread-op-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&profile).unwrap();
        let default = Some("openai/gpt-6-luna");
        let read = |thread: &str| {
            operation(
                &profile,
                false,
                serde_json::json!({"thread_id": thread}),
                default,
            )
            .unwrap()
        };
        let fresh = read("t-1");
        assert_eq!(fresh["layer"], "default");
        assert_eq!(fresh["model"]["model"], "gpt-6-luna");
        let seeded = operation(
            &profile,
            true,
            serde_json::json!({"thread_id": "t-1", "seed": {"project_id": "p-homelab",
                "model": {"provider": "openai", "model": "gpt-6-astra"}, "permission_mode": "manual"}}),
            default,
        )
        .unwrap();
        assert_eq!(
            (&seeded["layer"], &seeded["project_id"]),
            (
                &serde_json::json!("project"),
                &serde_json::json!("p-homelab")
            )
        );
        assert_eq!(seeded["seed"]["permission_mode"], "manual");
        let picked = operation(
            &profile,
            true,
            serde_json::json!({"thread_id": "t-1", "model": {"provider": "anthropic", "model": "opus"}}),
            default,
        )
        .unwrap();
        assert_eq!(picked["layer"], "thread");
        assert_eq!(read("t-2")["model"]["model"], "gpt-6-luna");
        assert_eq!(
            run_model(&profile, "t-1", None, default).as_deref(),
            Some("anthropic/opus")
        );
        let reset = operation(
            &profile,
            true,
            serde_json::json!({"thread_id": "t-1", "reset": true}),
            default,
        )
        .unwrap();
        assert_eq!(reset["model"]["model"], "gpt-6-astra");
        assert!(operation(
            &profile,
            false,
            serde_json::json!({"thread_id": "t-1", "reset": true}),
            default
        )
        .is_err());
        assert!(operation(
            &profile,
            true,
            serde_json::json!({"thread_id": "t-1", "model": {"provider": "", "model": "x"}}),
            default
        )
        .is_err());
        let _ = fs::remove_dir_all(profile);
    }
}
