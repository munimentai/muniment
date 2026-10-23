//! Model routing: each router route is a record keyed by its name, last
//! writer wins per field. Decision-model connections are records keyed by
//! their id with the fields that hold no secret; a connection's key stays
//! where it was entered. A connection to a loopback address reaches only its
//! own machine, so it stays there.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{json, Map, Value};

use super::{Adapter, Change, Context, Local, Slot};
use crate::model_router::config::{self, Classifier, Route};

pub const ROUTE_CLASS: &str = "route";
pub const CONNECTION_CLASS: &str = "decision-model";
const CONNECTIONS_FILE: &str = "classifier-connections.json";
const ROUTE_FIELDS: [&str; 4] = ["description", "family", "model", "position"];
const CONNECTION_FIELDS: [&str; 3] = ["name", "catalog_id", "classifier"];
/// The classifier fields that hold a secret.
const SECRET_FIELDS: [&str; 1] = ["api_key"];

fn agent(context: &Context<'_>) -> std::path::PathBuf {
    crate::state_root::agent_directory(context.profile)
}

pub(super) struct Routes;

impl Adapter for Routes {
    fn class(&self) -> &'static str {
        ROUTE_CLASS
    }

    fn read(&self, context: &Context<'_>) -> Result<Local, String> {
        let agent = agent(context);
        if !config::config_path(&agent).is_file() {
            return Ok(Local::new());
        }
        let router = config::load(&agent).map_err(|error| error.to_string())?;
        let mut local = Local::new();
        for (position, route) in router.routes.iter().enumerate() {
            local.insert(
                Slot::field(&route.key, "description"),
                json!(route.description),
            );
            local.insert(Slot::field(&route.key, "family"), json!(route.family));
            local.insert(Slot::field(&route.key, "model"), json!(route.model));
            local.insert(Slot::field(&route.key, "position"), json!(position));
        }
        Ok(local)
    }

    fn write(&self, context: &Context<'_>, changes: &[Change]) -> Result<(), String> {
        let agent = agent(context);
        let mut router = config::load(&agent).map_err(|error| error.to_string())?;
        let mut routes: BTreeMap<String, Map<String, Value>> = router
            .routes
            .iter()
            .enumerate()
            .map(|(position, route)| {
                let mut fields = Map::new();
                fields.insert("description".into(), json!(route.description));
                fields.insert("family".into(), json!(route.family));
                fields.insert("model".into(), json!(route.model));
                fields.insert("position".into(), json!(position));
                (route.key.clone(), fields)
            })
            .collect();
        for change in changes {
            if !ROUTE_FIELDS.contains(&change.slot.field.as_str()) {
                continue;
            }
            let fields = routes.entry(change.slot.key.clone()).or_default();
            match &change.value {
                Some(value) => fields.insert(change.slot.field.clone(), value.clone()),
                None => fields.remove(&change.slot.field),
            };
        }
        let mut ordered: Vec<(u64, Route)> = routes
            .into_iter()
            .filter_map(|(key, fields)| {
                let text = |name: &str| fields.get(name)?.as_str().map(str::to_owned);
                let route = Route {
                    key,
                    description: text("description")?,
                    family: text("family")?,
                    model: text("model")?,
                };
                crate::model_router::family::family(&route.family)?;
                let position = fields
                    .get("position")
                    .and_then(Value::as_u64)
                    .unwrap_or(u64::MAX);
                Some((position, route))
            })
            .collect();
        ordered.sort_by(|a, b| (a.0, &a.1.key).cmp(&(b.0, &b.1.key)));
        router.routes = ordered.into_iter().map(|(_, route)| route).collect();
        let keys: BTreeSet<&str> = router
            .routes
            .iter()
            .map(|route| route.key.as_str())
            .collect();
        if router
            .fallback
            .as_deref()
            .is_some_and(|fallback| !keys.contains(fallback))
        {
            router.fallback = None;
        }
        config::save(&agent, &router).map_err(|error| error.to_string())
    }
}

/// Whether a URL reaches only the machine it runs on.
fn loopback(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    match parsed.host() {
        Some(url::Host::Domain(domain)) => {
            domain.eq_ignore_ascii_case("localhost") || domain.ends_with(".localhost")
        }
        Some(url::Host::Ipv4(address)) => address.is_loopback() || address.is_unspecified(),
        Some(url::Host::Ipv6(address)) => address.is_loopback() || address.is_unspecified(),
        None => false,
    }
}

fn machine_local(classifier: &Value) -> bool {
    classifier["base_url"].as_str().is_some_and(loopback)
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

/// The connections a missing file stands for: the active dedicated
/// classifier, as the runtime lists it before the first save.
fn implied_connections(agent: &Path) -> Vec<Value> {
    let Ok(router) = config::load(agent) else {
        return Vec::new();
    };
    let active = router.classifier;
    if !active.ready() || matches!(active, Classifier::Pooled { .. }) {
        return Vec::new();
    }
    let catalog = if matches!(active, Classifier::Typesafe { .. }) {
        "jev"
    } else {
        "custom"
    };
    vec![
        json!({"id": "existing", "name": active.model(), "catalog_id": catalog, "classifier": active}),
    ]
}

fn without_secrets(classifier: &Value) -> Value {
    let mut classifier = classifier.clone();
    if let Some(object) = classifier.as_object_mut() {
        for field in SECRET_FIELDS {
            object.remove(field);
        }
    }
    classifier
}

pub(super) struct Connections;

impl Adapter for Connections {
    fn class(&self) -> &'static str {
        CONNECTION_CLASS
    }

    fn read(&self, context: &Context<'_>) -> Result<Local, String> {
        let mut local = Local::new();
        for connection in read_connections(&agent(context))?.unwrap_or_default() {
            let Some(id) = connection["id"].as_str() else {
                continue;
            };
            let classifier = &connection["classifier"];
            if machine_local(classifier) {
                continue;
            }
            local.insert(Slot::field(id, "name"), connection["name"].clone());
            local.insert(
                Slot::field(id, "catalog_id"),
                connection["catalog_id"].clone(),
            );
            local.insert(Slot::field(id, "classifier"), without_secrets(classifier));
        }
        Ok(local)
    }

    fn write(&self, context: &Context<'_>, changes: &[Change]) -> Result<(), String> {
        let agent = agent(context);
        let mut connections =
            read_connections(&agent)?.unwrap_or_else(|| implied_connections(&agent));
        for change in changes {
            if !CONNECTION_FIELDS.contains(&change.slot.field.as_str()) {
                continue;
            }
            let id = change.slot.key.as_str();
            let index = connections
                .iter()
                .position(|connection| connection["id"] == id);
            if index.is_some_and(|index| machine_local(&connections[index]["classifier"])) {
                continue;
            }
            match (&change.value, index) {
                (None, Some(index)) => {
                    if change.slot.field == "classifier" {
                        connections.remove(index);
                    }
                }
                (None, None) => {}
                (Some(value), index) => {
                    if change.slot.field == "classifier" && machine_local(value) {
                        continue;
                    }
                    let index = index.unwrap_or_else(|| {
                        connections.push(json!({"id": id, "name": "", "catalog_id": "custom", "classifier": {"kind": "none"}}));
                        connections.len() - 1
                    });
                    let connection = &mut connections[index];
                    if change.slot.field == "classifier" {
                        let mut incoming = value.clone();
                        let held = &connection["classifier"];
                        // The key entered on this machine stays with the
                        // same kind of connection.
                        if held["kind"] == incoming["kind"] {
                            for field in SECRET_FIELDS {
                                if let Some(secret) = held.get(field) {
                                    incoming[field] = secret.clone();
                                }
                            }
                        }
                        if incoming["kind"] == "typesafe" && incoming.get("api_key").is_none() {
                            incoming["api_key"] = json!("");
                        }
                        connection["classifier"] = incoming;
                    } else {
                        connection[change.slot.field.as_str()] = value.clone();
                    }
                }
            }
        }
        // Only connections that parse as the runtime reads them are kept.
        connections.retain(|connection| {
            connection["id"].is_string()
                && serde_json::from_value::<Classifier>(connection["classifier"].clone()).is_ok()
        });
        std::fs::create_dir_all(&agent).map_err(|error| error.to_string())?;
        let bytes = serde_json::to_vec_pretty(&connections).map_err(|error| error.to_string())?;
        config::write_private(&agent.join(CONNECTIONS_FILE), &bytes)
            .map_err(|error| error.to_string())
    }
}
