//! Settings → Models, the router half: the account pools, what each account
//! has served, the routes, and the classifier.
//!
//! A credential enters through here and never leaves. The settings this
//! answers with carry an account's source and its counters, never its key.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use tauri::Manager;

use muniment_core::model_router::config::{
    self, Account, Classifier, Credential, Route, RouterConfig,
};
use muniment_core::model_router::family::FAMILIES;
use muniment_core::model_router::model_catalog;
use muniment_core::model_router::usage::{self, AccountUsage};
use muniment_core::model_router::{classify, options, pi_provider, served_models, server};

use crate::local_mode::{
    harness_agent_directory, lock_pi_auth_file, pi_models_file, read_json_for_update,
    write_json_for_update,
};

const READ_ERROR: &str =
    "Muniment cannot read the router settings. Check folder access, then retry.";
const SAVE_ERROR: &str =
    "Muniment cannot save the router settings. Check folder access, then retry.";
const START_ERROR: &str =
    "The router could not start. Check that nothing else holds the port, then retry.";
const CLASSIFIER_TIMEOUT: Duration = Duration::from_secs(10);

/// The running listener. One at a time, owned by the shell.
#[derive(Default)]
pub(crate) struct RouterState(Mutex<Option<server::Handle>>);

impl RouterState {
    pub(crate) fn new() -> Self {
        Self::default()
    }
}

/// One family a pool can hold.
#[derive(Debug, Serialize)]
pub(crate) struct FamilyView {
    id: &'static str,
    name: &'static str,
    base_url: &'static str,
}

/// One account, with what it has served. Never its credential.
#[derive(Debug, Serialize)]
pub(crate) struct AccountView {
    id: String,
    family: String,
    label: String,
    /// `key` or `account`.
    source: &'static str,
    base_url: Option<String>,
    models: Vec<String>,
    enabled: bool,
    weight: u32,
    requests: u64,
    input_tokens: u64,
    output_tokens: u64,
    errors: u64,
    last_used_ms: Option<i64>,
    last_error: Option<String>,
    cooldown_until_ms: Option<i64>,
    /// Turns this account has in flight right now.
    active: u32,
    /// The last 30 days as `[day, requests, input, output, errors]` rows,
    /// oldest first, for the usage bar.
    days: Vec<(String, u64, u64, u64, u64)>,
}

/// One model in the running: what the classifier chooses between.
#[derive(Debug, Serialize)]
pub(crate) struct OptionView {
    /// The name the classifier answers with, `family/model` unless renamed.
    key: String,
    /// The statement the classifier reads.
    description: String,
    family: String,
    model: String,
    /// The catalog's name for the model, empty when it describes none.
    name: String,
    /// `deep`, `balanced` or `fast`, empty outside the catalog.
    tier: String,
    /// US dollars per million tokens, zero outside the catalog.
    price: f64,
    output: f64,
    context: String,
    /// Whether the user's own words replaced the catalog statement.
    named: bool,
}

/// What the classifier is, with no key in it.
#[derive(Debug, Serialize)]
pub(crate) struct ClassifierView {
    kind: &'static str,
    model: String,
    /// The family a pooled classifier spends, empty for the other kinds.
    family: String,
    base_url: Option<String>,
    configured: bool,
}

/// Everything Settings → Models needs about the router.
#[derive(Debug, Serialize)]
pub(crate) struct RouterSettings {
    enabled: bool,
    running: bool,
    /// The loopback URL Pi holds, so a user can see where turns go.
    base_url: Option<String>,
    /// Whether Pi sends its turns to the router.
    is_default: bool,
    families: Vec<FamilyView>,
    accounts: Vec<AccountView>,
    /// Every model an enabled account serves, in the order the classifier sees.
    options: Vec<OptionView>,
    routes: Vec<Route>,
    fallback: Option<String>,
    min_confidence: f64,
    classifier: ClassifierView,
    served_models: Vec<String>,
}

fn agent() -> Result<PathBuf, String> {
    harness_agent_directory(READ_ERROR)
}

fn load(agent: &Path) -> Result<RouterConfig, String> {
    config::load(agent).map_err(|_| READ_ERROR.to_string())
}

fn save(agent: &Path, config: &RouterConfig) -> Result<(), String> {
    config::save(agent, config).map_err(|_| SAVE_ERROR.to_string())
}

fn account_view(account: &Account, usage: Option<&AccountUsage>, active: u32) -> AccountView {
    AccountView {
        id: account.id.clone(),
        family: account.family.clone(),
        label: account.label.clone(),
        source: account.credential.source(),
        base_url: account.base_url.clone(),
        models: account.models.clone(),
        enabled: account.enabled,
        weight: account.weight,
        requests: usage.map(|entry| entry.requests).unwrap_or(0),
        input_tokens: usage.map(|entry| entry.input_tokens).unwrap_or(0),
        output_tokens: usage.map(|entry| entry.output_tokens).unwrap_or(0),
        errors: usage.map(|entry| entry.errors).unwrap_or(0),
        last_used_ms: usage.and_then(|entry| entry.last_used_ms),
        last_error: usage.and_then(|entry| entry.last_error.clone()),
        cooldown_until_ms: usage.and_then(|entry| entry.cooldown_until_ms),
        active,
        days: usage
            .map(|entry| {
                entry
                    .days
                    .iter()
                    .map(|(day, bucket)| {
                        (
                            day.clone(),
                            bucket.requests,
                            bucket.input_tokens,
                            bucket.output_tokens,
                            bucket.errors,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn classifier_view(classifier: &Classifier) -> ClassifierView {
    let (base_url, family) = match classifier {
        Classifier::None => (None, String::new()),
        Classifier::Typesafe { base_url, .. } => (base_url.clone(), String::new()),
        Classifier::Pooled { family, .. } => (None, family.clone()),
        Classifier::Endpoint { base_url, .. } => (Some(base_url.clone()), String::new()),
    };
    ClassifierView {
        kind: classifier.kind(),
        model: classifier.model().to_owned(),
        family,
        base_url,
        configured: classifier.ready(),
    }
}

fn settings(
    agent: &Path,
    running: Option<&server::Endpoint>,
    active: &std::collections::BTreeMap<String, u32>,
) -> Result<RouterSettings, String> {
    let config = load(agent)?;
    let ledger = usage::load(agent);
    let settings_file = agent.join("settings.json");
    let is_default = std::fs::read(&settings_file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Map<String, Value>>(&bytes).ok())
        .is_some_and(|settings| pi_provider::is_default(&settings));
    Ok(RouterSettings {
        enabled: config.enabled,
        running: running.is_some(),
        base_url: running.map(|endpoint| endpoint.base_url()),
        is_default,
        families: FAMILIES
            .iter()
            .map(|family| FamilyView {
                id: family.id,
                name: family.name,
                base_url: family.base_url,
            })
            .collect(),
        accounts: config
            .accounts
            .iter()
            .map(|account| {
                account_view(
                    account,
                    ledger.account(&account.id),
                    active.get(&account.id).copied().unwrap_or(0),
                )
            })
            .collect(),
        options: options(&config)
            .into_iter()
            .map(|option| {
                let entry = model_catalog::entry(&option.family, &option.model);
                let named = config.routes.iter().any(|route| {
                    route.family == option.family
                        && route.model == option.model
                        && !route.description.trim().is_empty()
                });
                OptionView {
                    key: option.key,
                    description: option.description,
                    family: option.family,
                    model: option.model,
                    name: entry.map(|entry| entry.name.to_owned()).unwrap_or_default(),
                    tier: entry.map(|entry| entry.tier.to_owned()).unwrap_or_default(),
                    price: entry.map(|entry| entry.price).unwrap_or(0.0),
                    output: entry.map(|entry| entry.output).unwrap_or(0.0),
                    context: entry
                        .map(|entry| entry.context.to_owned())
                        .unwrap_or_default(),
                    named,
                }
            })
            .collect(),
        routes: config.routes.clone(),
        fallback: config.fallback.clone(),
        min_confidence: config.min_confidence,
        classifier: classifier_view(&config.classifier),
        served_models: served_models(&config),
    })
}

/// The endpoint of the listener this shell is running, when it runs one.
fn running_endpoint(state: &RouterState) -> Option<server::Endpoint> {
    state
        .0
        .lock()
        .ok()?
        .as_ref()
        .map(|handle| handle.endpoint().clone())
}

/// The turns each account has in flight, empty while no listener runs.
fn running_active(state: &RouterState) -> std::collections::BTreeMap<String, u32> {
    state
        .0
        .lock()
        .ok()
        .and_then(|held| held.as_ref().map(|handle| handle.active()))
        .unwrap_or_default()
}

/// Starts the listener when the record says on, stops it when it says off,
/// and writes Pi's `models.json` either way.
fn apply(agent: &Path, state: &RouterState, config: &RouterConfig) -> Result<(), String> {
    let mut held = state.0.lock().map_err(|_| START_ERROR.to_string())?;
    if !config.enabled {
        if let Some(handle) = held.take() {
            handle.stop();
        }
        let _ = std::fs::remove_file(server::endpoint_path(agent));
        return write_models(agent, None, config);
    }
    if held.is_none() {
        let handle = server::start(agent.to_path_buf()).map_err(|_| START_ERROR.to_string())?;
        *held = Some(handle);
    }
    let endpoint = held
        .as_ref()
        .map(|handle| handle.endpoint().clone())
        .ok_or_else(|| START_ERROR.to_string())?;
    write_models(agent, Some(&endpoint), config)
}

/// Adds or removes the router's entry in Pi's `models.json`, under the same
/// lock every other writer of that file takes.
fn write_models(
    agent: &Path,
    endpoint: Option<&server::Endpoint>,
    config: &RouterConfig,
) -> Result<(), String> {
    let path = pi_models_file(agent);
    std::fs::create_dir_all(agent).map_err(|_| SAVE_ERROR.to_string())?;
    let _lock = lock_pi_auth_file(&path).map_err(|_| SAVE_ERROR.to_string())?;
    let mut models = read_json_for_update(&path).map_err(|_| SAVE_ERROR.to_string())?;
    match endpoint {
        Some(endpoint) => pi_provider::register(&mut models, endpoint, config),
        None => {
            if !pi_provider::unregister(&mut models) {
                return Ok(());
            }
        }
    }
    write_json_for_update(&path, &models).map_err(|_| SAVE_ERROR.to_string())
}

/// Starts the router on launch when its record says it is on.
pub(crate) fn restore<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let Ok(agent) = agent() else {
        return;
    };
    let Ok(config) = load(&agent) else {
        return;
    };
    if !config.enabled {
        return;
    }
    let state = app.state::<RouterState>();
    let _ = apply(&agent, &state, &config);
}

#[tauri::command]
pub(crate) fn model_router_settings(
    state: tauri::State<'_, RouterState>,
) -> Result<RouterSettings, String> {
    let agent = agent()?;
    settings(
        &agent,
        running_endpoint(&state).as_ref(),
        &running_active(&state),
    )
}

#[tauri::command]
pub(crate) fn model_router_set_enabled(
    state: tauri::State<'_, RouterState>,
    enabled: bool,
) -> Result<RouterSettings, String> {
    let agent = agent()?;
    let mut config = load(&agent)?;
    config.enabled = enabled;
    save(&agent, &config)?;
    apply(&agent, &state, &config)?;
    settings(
        &agent,
        running_endpoint(&state).as_ref(),
        &running_active(&state),
    )
}

#[tauri::command]
pub(crate) fn model_router_add_account(
    state: tauri::State<'_, RouterState>,
    family: String,
    label: String,
    key: String,
    base_url: Option<String>,
    models: Option<Vec<String>>,
) -> Result<RouterSettings, String> {
    let agent = agent()?;
    if muniment_core::model_router::family::family(&family).is_none() {
        return Err("Choose a provider the router serves.".into());
    }
    let key = key.trim().to_owned();
    if key.is_empty() || key.len() > 16 * 1024 {
        return Err("Enter the account's API key.".into());
    }
    let label = label.trim().to_owned();
    if label.is_empty() || label.len() > 120 {
        return Err("Name this account, so its usage is readable.".into());
    }
    let base_url = base_url
        .map(|url| url.trim().to_owned())
        .filter(|url| !url.is_empty());
    if let Some(url) = &base_url {
        let parsed = url::Url::parse(url).map_err(|_| "Enter a valid server URL.".to_string())?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host().is_none() {
            return Err("Enter a valid server URL.".into());
        }
    }
    let mut config = load(&agent)?;
    config.accounts.push(Account {
        id: uuid::Uuid::now_v7().to_string(),
        family,
        label,
        credential: Credential::ApiKey { key },
        base_url,
        models: models
            .unwrap_or_default()
            .into_iter()
            .map(|model| model.trim().to_owned())
            .filter(|model| !model.is_empty())
            .collect(),
        enabled: true,
        weight: 1,
    });
    save(&agent, &config)?;
    apply(&agent, &state, &config)?;
    settings(
        &agent,
        running_endpoint(&state).as_ref(),
        &running_active(&state),
    )
}

#[tauri::command]
pub(crate) fn model_router_update_account(
    state: tauri::State<'_, RouterState>,
    id: String,
    label: Option<String>,
    enabled: Option<bool>,
    weight: Option<u32>,
    models: Option<Vec<String>>,
) -> Result<RouterSettings, String> {
    let agent = agent()?;
    let mut config = load(&agent)?;
    let account = config
        .accounts
        .iter_mut()
        .find(|account| account.id == id)
        .ok_or_else(|| "That account is gone.".to_string())?;
    if let Some(label) = label {
        let label = label.trim().to_owned();
        if label.is_empty() || label.len() > 120 {
            return Err("Name this account, so its usage is readable.".into());
        }
        account.label = label;
    }
    if let Some(enabled) = enabled {
        account.enabled = enabled;
    }
    if let Some(weight) = weight {
        account.weight = weight.min(100);
    }
    if let Some(models) = models {
        account.models = models
            .into_iter()
            .map(|model| model.trim().to_owned())
            .filter(|model| !model.is_empty())
            .collect();
    }
    save(&agent, &config)?;
    apply(&agent, &state, &config)?;
    settings(
        &agent,
        running_endpoint(&state).as_ref(),
        &running_active(&state),
    )
}

#[tauri::command]
pub(crate) fn model_router_remove_account(
    state: tauri::State<'_, RouterState>,
    id: String,
) -> Result<RouterSettings, String> {
    let agent = agent()?;
    let mut config = load(&agent)?;
    config.accounts.retain(|account| account.id != id);
    save(&agent, &config)?;
    let mut ledger = usage::load(&agent);
    ledger.forget(&id);
    let _ = usage::save(&agent, &ledger);
    apply(&agent, &state, &config)?;
    settings(
        &agent,
        running_endpoint(&state).as_ref(),
        &running_active(&state),
    )
}

#[tauri::command]
pub(crate) fn model_router_save_routes(
    state: tauri::State<'_, RouterState>,
    routes: Vec<Route>,
    fallback: Option<String>,
    min_confidence: Option<f64>,
) -> Result<RouterSettings, String> {
    let agent = agent()?;
    let mut config = load(&agent)?;
    for route in &routes {
        if route.key.trim().is_empty() || route.key.len() > 60 {
            return Err("Give every model a short name.".into());
        }
        if route.model.trim().is_empty() {
            return Err("Name the model this describes.".into());
        }
        if muniment_core::model_router::family::family(&route.family).is_none() {
            return Err("Name a provider the router serves.".into());
        }
        if route.description.len() > 2_000 {
            return Err("Keep a model's statement under two thousand characters.".into());
        }
    }
    config.routes = routes;
    config.fallback = fallback;
    // One name is one model: the classifier answers with a name, and `auto`
    // is the name that asks it to pick.
    let running = options(&config);
    let mut keys: Vec<&str> = running.iter().map(|option| option.key.as_str()).collect();
    keys.sort_unstable();
    if keys.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err("Two models share a name. Give each its own.".into());
    }
    if keys.contains(&config::AUTO_MODEL) {
        return Err("The name auto asks the classifier to pick. Choose another.".into());
    }
    // The fallback names a model in the running, or none and the cheapest takes it.
    if let Some(named) = &config.fallback {
        if !running.iter().any(|option| &option.key == named) {
            return Err("The fallback must be a model in the running.".into());
        }
    }
    if let Some(confidence) = min_confidence {
        config.min_confidence = confidence.clamp(0.0, 1.0);
    }
    save(&agent, &config)?;
    apply(&agent, &state, &config)?;
    settings(
        &agent,
        running_endpoint(&state).as_ref(),
        &running_active(&state),
    )
}

#[tauri::command]
pub(crate) fn model_router_set_classifier(
    state: tauri::State<'_, RouterState>,
    kind: String,
    api_key: Option<String>,
    model: Option<String>,
    base_url: Option<String>,
    family: Option<String>,
) -> Result<RouterSettings, String> {
    let agent = agent()?;
    let mut config = load(&agent)?;
    let model = model
        .map(|model| model.trim().to_owned())
        .filter(|model| !model.is_empty())
        .unwrap_or_else(|| "jev-latest".to_owned());
    let key = api_key.map(|key| key.trim().to_owned()).unwrap_or_default();
    config.classifier = match kind.as_str() {
        "none" => Classifier::None,
        "typesafe" => {
            // A blank key on an already-configured classifier keeps the key
            // the user saved, because Settings never reads it back to show it.
            let key = if key.is_empty() {
                match &config.classifier {
                    Classifier::Typesafe { api_key, .. } => api_key.clone(),
                    _ => String::new(),
                }
            } else {
                key
            };
            if key.is_empty() {
                return Err("Enter your TypeSafe API key.".into());
            }
            Classifier::Typesafe {
                api_key: key,
                model,
                base_url: base_url
                    .map(|url| url.trim().to_owned())
                    .filter(|url| !url.is_empty()),
            }
        }
        "pooled" => {
            let family = family.unwrap_or_default();
            if muniment_core::model_router::family::family(&family).is_none() {
                return Err("Choose a provider the router serves.".into());
            }
            if !config
                .accounts
                .iter()
                .any(|account| account.family == family)
            {
                return Err("Add an account for that provider first.".into());
            }
            Classifier::Pooled { family, model }
        }
        "endpoint" => {
            let url = base_url
                .map(|url| url.trim().to_owned())
                .filter(|url| !url.is_empty())
                .ok_or_else(|| "Enter the classifier's URL.".to_string())?;
            let parsed =
                url::Url::parse(&url).map_err(|_| "Enter a valid classifier URL.".to_string())?;
            if !matches!(parsed.scheme(), "http" | "https") || parsed.host().is_none() {
                return Err("Enter a valid classifier URL.".into());
            }
            // A blank key keeps the one saved, as the TypeSafe form does.
            let key = if key.is_empty() {
                match &config.classifier {
                    Classifier::Endpoint {
                        api_key: Some(saved),
                        ..
                    } => saved.clone(),
                    _ => String::new(),
                }
            } else {
                key
            };
            Classifier::Endpoint {
                base_url: url,
                api_key: Some(key).filter(|key| !key.is_empty()),
                model,
            }
        }
        _ => return Err("Choose a classifier.".into()),
    };
    save(&agent, &config)?;
    apply(&agent, &state, &config)?;
    settings(
        &agent,
        running_endpoint(&state).as_ref(),
        &running_active(&state),
    )
}

#[tauri::command]
pub(crate) async fn model_router_test_classifier() -> Result<(), String> {
    let agent = agent()?;
    let config = load(&agent)?;
    tauri::async_runtime::spawn_blocking(move || {
        classify::check(&config.classifier, CLASSIFIER_TIMEOUT)
    })
    .await
    .map_err(|_| "The test did not finish. Try again.".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use muniment_core::model_router::usage::Ledger;

    fn account(id: &str) -> Account {
        Account {
            id: id.to_owned(),
            family: "openai".into(),
            label: "work".into(),
            credential: Credential::ApiKey {
                key: "sk-secret".into(),
            },
            base_url: None,
            models: Vec::new(),
            enabled: true,
            weight: 2,
        }
    }

    #[test]
    fn an_account_view_carries_its_counters_and_never_its_key() {
        let mut ledger = Ledger::default();
        ledger.record_success("a1", "2026-09-17", 1_000, 300, 40);
        ledger.record_error("a1", "2026-09-17", 2_000, "429 slow down", true);
        let view = account_view(&account("a1"), ledger.account("a1"), 2);
        let json = serde_json::to_string(&view).unwrap();
        assert!(!json.contains("sk-secret"), "{json}");
        assert_eq!(view.source, "key");
        assert_eq!(view.requests, 1);
        assert_eq!(view.input_tokens, 300);
        assert_eq!(view.errors, 1);
        assert_eq!(view.weight, 2);
        assert!(view.cooldown_until_ms.is_some());
        assert_eq!(view.days.len(), 1);
        assert_eq!(view.days[0].0, "2026-09-17");
        assert_eq!(view.active, 2);
    }

    #[test]
    fn an_account_that_has_served_nothing_reads_as_zero() {
        let view = account_view(&account("a1"), None, 0);
        assert_eq!(view.requests, 0);
        assert_eq!(view.last_used_ms, None);
        assert!(view.days.is_empty());
        assert_eq!(view.active, 0);
    }

    #[test]
    fn a_classifier_view_names_its_kind_and_never_its_key() {
        let view = classifier_view(&Classifier::Typesafe {
            api_key: "apikey_secret".into(),
            model: "jev-latest".into(),
            base_url: None,
        });
        let json = serde_json::to_string(&view).unwrap();
        assert!(!json.contains("apikey_secret"), "{json}");
        assert_eq!(view.kind, "typesafe");
        assert_eq!(view.model, "jev-latest");
        assert!(view.configured);

        let off = classifier_view(&Classifier::None);
        assert_eq!(off.kind, "none");
        assert!(!off.configured);
    }

    #[test]
    fn the_router_entry_lands_in_pi_models_and_leaves_when_it_is_turned_off() {
        let agent =
            std::env::temp_dir().join(format!("muniment-router-cmd-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&agent).unwrap();
        std::fs::write(
            agent.join("models.json"),
            serde_json::json!({ "providers": { "ollama": { "baseUrl": "http://localhost:11434/v1" } } })
                .to_string(),
        )
        .unwrap();
        let config = RouterConfig {
            enabled: true,
            routes: vec![Route {
                key: "fast".into(),
                description: "A short question".into(),
                family: "openai".into(),
                model: "gpt-5.6-mini".into(),
            }],
            fallback: Some("fast".into()),
            ..RouterConfig::default()
        };
        let endpoint = server::Endpoint {
            port: 8421,
            token: "t0ken".into(),
        };
        write_models(&agent, Some(&endpoint), &config).unwrap();
        let written: Value =
            serde_json::from_slice(&std::fs::read(agent.join("models.json")).unwrap()).unwrap();
        assert_eq!(
            written["providers"]["muniment-router"]["baseUrl"],
            "http://127.0.0.1:8421/v1"
        );
        assert!(written["providers"]["ollama"].is_object());

        write_models(&agent, None, &config).unwrap();
        let written: Value =
            serde_json::from_slice(&std::fs::read(agent.join("models.json")).unwrap()).unwrap();
        assert!(written["providers"].get("muniment-router").is_none());
        assert!(written["providers"]["ollama"].is_object());
    }
}
