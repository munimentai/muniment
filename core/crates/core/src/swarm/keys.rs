//! The keys of decision-model connections and web search providers, sealed
//! as `keys:<owner>`. The fields without secrets replicate as records; the
//! key of each connection or provider travels sealed and lands where the
//! receiver holds no key of its own. A connection to a loopback address
//! reaches only its own machine, so its key stays there.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::search_settings::{self, ProviderKey};

const CONNECTIONS_FILE: &str = "classifier-connections.json";

fn digest(value: &Value) -> String {
    hex::encode(&Sha256::digest(value.to_string().as_bytes())[..16])
}

fn agent(profile: &Path) -> std::path::PathBuf {
    crate::state_root::agent_directory(profile)
}

fn read_connections(agent: &Path) -> Result<Option<Vec<Value>>, String> {
    match std::fs::read(agent.join(CONNECTIONS_FILE)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| "The decision model connections cannot be read.".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("The decision model connections cannot be read.".into()),
    }
}

fn loopback(classifier: &Value) -> bool {
    classifier["base_url"]
        .as_str()
        .is_some_and(muniment_router::constraints::loopback)
}

fn connection_key(connection: &Value) -> Option<&str> {
    connection["classifier"]["api_key"]
        .as_str()
        .filter(|key| !key.trim().is_empty())
}

/// What one key slot is named by: `decision:<id>` or `search:<provider>`.
type Written = BTreeMap<String, String>;

/// This runtime's own keys, without those another member's secret wrote
/// here: `{decision_models: {id: key}, search: {provider: key}}` and the
/// names of its slots.
pub(super) fn own(profile: &Path, received: &Written) -> Result<(Value, Vec<String>), String> {
    let mine = |slot: &str, value: &Value| received.get(slot) != Some(&digest(value));
    let mut decisions = Map::new();
    for connection in read_connections(&agent(profile))?.unwrap_or_default() {
        let (Some(id), Some(key)) = (connection["id"].as_str(), connection_key(&connection)) else {
            continue;
        };
        if loopback(&connection["classifier"]) {
            continue;
        }
        let value = Value::from(key);
        if mine(&format!("decision:{id}"), &value) {
            decisions.insert(id.to_owned(), value);
        }
    }
    let mut search = Map::new();
    if search_settings::settings_path(profile).is_file() {
        let settings = search_settings::load(profile).map_err(|error| error.to_string())?;
        for (provider, key) in settings.keys {
            let value = serde_json::to_value(&key).map_err(|error| error.to_string())?;
            if mine(&format!("search:{provider}"), &value) {
                search.insert(provider, value);
            }
        }
    }
    let names = decisions
        .keys()
        .map(|id| format!("decision:{id}"))
        .chain(search.keys().map(|provider| format!("search:{provider}")))
        .collect();
    Ok((
        json!({"decision_models": decisions, "search": search}),
        names,
    ))
}

/// Applies one owner's keys in place of those it shared before: each lands
/// where this runtime holds no key or holds the one the owner's secret
/// wrote. Answers what it wrote, and whether a key still waits for its
/// connection's record.
pub(super) fn apply(
    profile: &Path,
    keys: &Value,
    previous: &Written,
) -> Result<(Written, bool), String> {
    let mut written = Written::new();
    let mut waiting = false;
    // Decision-model connections.
    let agent = agent(profile);
    let incoming = keys["decision_models"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    if let Some(mut connections) = read_connections(&agent)? {
        let mut changed = false;
        for connection in &mut connections {
            let Some(id) = connection["id"].as_str().map(str::to_owned) else {
                continue;
            };
            let slot = format!("decision:{id}");
            if loopback(&connection["classifier"]) {
                continue;
            }
            let held = connection_key(connection).map(Value::from);
            let ours = held
                .as_ref()
                .is_some_and(|held| previous.get(&slot) == Some(&digest(held)));
            if held.is_some() && !ours {
                continue;
            }
            match incoming.get(&id) {
                Some(key) => {
                    if held.as_ref() != Some(key) {
                        connection["classifier"]["api_key"] = key.clone();
                        changed = true;
                    }
                    written.insert(slot, digest(key));
                }
                None if ours => {
                    connection["classifier"]["api_key"] = json!("");
                    changed = true;
                }
                None => {}
            }
        }
        if changed {
            let bytes = serde_json::to_vec_pretty(&connections).map_err(|e| e.to_string())?;
            crate::model_router::config::write_private(&agent.join(CONNECTIONS_FILE), &bytes)
                .map_err(|error| error.to_string())?;
        }
    }
    waiting |= incoming
        .keys()
        .any(|id| !written.contains_key(&format!("decision:{id}")));
    // Web search providers.
    let incoming = keys["search"].as_object().cloned().unwrap_or_default();
    let had_search = previous.keys().any(|slot| slot.starts_with("search:"));
    if !incoming.is_empty() || had_search {
        let mut settings = search_settings::load(profile).map_err(|error| error.to_string())?;
        let before = settings.clone();
        for (provider, value) in &incoming {
            let slot = format!("search:{provider}");
            let held = settings
                .keys
                .get(provider)
                .and_then(|key| serde_json::to_value(key).ok());
            let ours = held
                .as_ref()
                .is_some_and(|held| previous.get(&slot) == Some(&digest(held)));
            if held.is_some() && !ours {
                continue;
            }
            let Ok(key) = serde_json::from_value::<ProviderKey>(value.clone()) else {
                continue;
            };
            if settings.set_key(provider, key).is_ok() {
                written.insert(slot, digest(value));
            }
        }
        for (slot, held_digest) in previous {
            let Some(provider) = slot.strip_prefix("search:") else {
                continue;
            };
            if incoming.contains_key(provider) {
                continue;
            }
            let held = settings
                .keys
                .get(provider)
                .and_then(|key| serde_json::to_value(key).ok());
            if held.is_some_and(|held| digest(&held) == *held_digest) {
                let _ = settings.remove_key(provider);
            }
        }
        if settings != before {
            search_settings::save(profile, &settings).map_err(|error| error.to_string())?;
            crate::pi_settings::store_web_search(profile).map_err(|error| error.to_string())?;
        }
    }
    Ok((written, waiting))
}
