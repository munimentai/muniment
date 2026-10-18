//! The user's preferences: the appearance, browser and memory recall choices
//! a client shows, kept in `<profile>/preferences.json`. The memory recall
//! choice lives where chats read it, in the memory recall settings.
//!
//! Fleet-wide keys replicate to every swarm member. Machine-local keys, such
//! as the sidebar and the window, stay on this runtime. Every change sends
//! `preferences.changed` with the keys it changed.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use muniment_attach::EventName;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::memory_files::RecallSettings;

pub const PREFERENCES_FILE: &str = "preferences.json";
/// The keys every swarm member shares.
pub const FLEET_KEYS: [&str; 9] = [
    "theme",
    "agent-avatars",
    "type",
    "thinking-mark",
    "browser.search",
    "browser.homepage",
    MEMORY_RECALL,
    "notify",
    crate::notifications::READ_AT_KEY,
];
/// The keys that stay on this machine.
pub const LOCAL_KEYS: [&str; 6] = [
    "sidebar-collapsed",
    "sidebar-width",
    "window",
    "frame",
    "local-mode",
    "voice-shortcut",
];
/// The key backed by the memory recall settings.
pub const MEMORY_RECALL: &str = "memory-recall";
/// The prefix a client's own storage puts before each key.
const CLIENT_PREFIX: &str = "muniment.";
const MAX_VALUE_BYTES: usize = 16 * 1024;
const MAX_FILE_BYTES: u64 = 256 * 1024;

static WRITE: Mutex<()> = Mutex::new(());

/// Whether `key` is shared by every swarm member.
pub fn is_fleet_key(key: &str) -> bool {
    FLEET_KEYS.contains(&key)
}

fn known(key: &str) -> bool {
    FLEET_KEYS.contains(&key) || LOCAL_KEYS.contains(&key)
}

/// The key without the prefix a client's own storage adds.
fn canonical(key: &str) -> &str {
    key.strip_prefix(CLIENT_PREFIX).unwrap_or(key)
}

fn read_file(profile: &Path) -> Result<Map<String, Value>, String> {
    let path = profile.join(PREFERENCES_FILE);
    let unreadable = || "The preferences cannot be read.".to_owned();
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Ok(metadata) if metadata.is_file() && metadata.len() <= MAX_FILE_BYTES => {}
        _ => return Err(unreadable()),
    }
    let bytes = std::fs::read(&path).map_err(|_| unreadable())?;
    serde_json::from_slice(&bytes).map_err(|_| unreadable())
}

fn write_file(profile: &Path, values: &Map<String, Value>) -> Result<(), String> {
    let failed = || "The preferences cannot be saved.".to_owned();
    std::fs::create_dir_all(profile).map_err(|_| failed())?;
    let path = profile.join(PREFERENCES_FILE);
    if path.is_symlink() {
        return Err(failed());
    }
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = std::fs::write(
        &temporary,
        serde_json::to_vec_pretty(values).map_err(|_| failed())?,
    )
    .and_then(|()| crate::atomic_file::replace(&temporary, &path));
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(|_| failed())
}

/// Every stored preference by key.
pub fn values(profile: &Path) -> Result<BTreeMap<String, Value>, String> {
    let mut values: BTreeMap<String, Value> = read_file(profile)?
        .into_iter()
        .filter(|(key, _)| known(key) && key != MEMORY_RECALL)
        .collect();
    if let Some(recall) = crate::memory_files::recall_settings_stored(profile)? {
        values.insert(MEMORY_RECALL.into(), json!(recall));
    }
    Ok(values)
}

pub(crate) fn check(key: &str, value: &Value) -> Result<(), String> {
    if !known(key) {
        return Err(format!("{key} is not a preference."));
    }
    if value.to_string().len() > MAX_VALUE_BYTES {
        return Err(format!("The {key} preference is too large."));
    }
    if key == MEMORY_RECALL {
        serde_json::from_value::<RecallSettings>(value.clone())
            .map_err(|_| "The memory recall preference is invalid.".to_owned())?;
    }
    Ok(())
}

/// Sets or, with `None`, removes preferences. Answers the keys that changed
/// with their new values.
pub fn set(
    profile: &Path,
    changes: BTreeMap<String, Option<Value>>,
) -> Result<BTreeMap<String, Option<Value>>, String> {
    for (key, value) in &changes {
        match value {
            Some(value) => check(key, value)?,
            None if !known(key) => return Err(format!("{key} is not a preference.")),
            None => {}
        }
    }
    let _guard = WRITE.lock().map_err(|_| "The preferences are busy.")?;
    let mut file = read_file(profile)?;
    let before = values(profile)?;
    let mut changed = BTreeMap::new();
    for (key, value) in changes {
        if before.get(&key) == value.as_ref() {
            continue;
        }
        if key == MEMORY_RECALL {
            let recall = match &value {
                Some(value) => serde_json::from_value(value.clone())
                    .map_err(|_| "The memory recall preference is invalid.")?,
                None => RecallSettings::default(),
            };
            crate::memory_files::recall_settings_save(profile, recall)?;
        } else {
            match &value {
                Some(value) => file.insert(key.clone(), value.clone()),
                None => file.remove(&key),
            };
        }
        changed.insert(key, value);
    }
    if changed.keys().any(|key| key != MEMORY_RECALL) {
        write_file(profile, &file)?;
    }
    Ok(changed)
}

/// Sends `preferences.changed` for `changed`, from this runtime's own
/// clients (`local`) or from another member (`fleet`).
pub(crate) fn announce(
    profile: &Path,
    changed: &BTreeMap<String, Option<Value>>,
    source: &str,
) -> bool {
    if changed.is_empty() {
        return false;
    }
    if changed.keys().any(|key| is_fleet_key(key)) && source == "local" {
        crate::fleet_sync::changed(profile, crate::fleet_sync::PREFERENCES_CLASS);
    }
    if let Some(swarm) = crate::swarm::Swarm::for_profile(profile) {
        swarm.records().emit(
            EventName::PreferencesChanged,
            json!({"values": changed, "source": source}),
        );
    }
    true
}

fn view(profile: &Path) -> Result<Value, String> {
    Ok(json!({
        "values": values(profile)?,
        "fleet_keys": FLEET_KEYS,
        "local_keys": LOCAL_KEYS,
    }))
}

/// A value from a client's own storage: text that holds JSON for the memory
/// recall key, else the text itself.
fn imported(key: &str, value: Value) -> Value {
    match (key, value) {
        (MEMORY_RECALL, Value::String(text)) => serde_json::from_str(&text).unwrap_or(Value::Null),
        (_, value) => value,
    }
}

/// Answers `preferences.get`, `preferences.set` and `preferences.import`.
pub fn operation(profile: &Path, operation: &str, body: Value) -> Result<Value, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Empty {}
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Set {
        values: BTreeMap<String, Option<Value>>,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Import {
        values: BTreeMap<String, Value>,
    }
    let invalid = |_| "The request is invalid.".to_string();
    match operation {
        "preferences.get" => {
            serde_json::from_value::<Empty>(body).map_err(invalid)?;
            view(profile)
        }
        "preferences.set" => {
            let request: Set = serde_json::from_value(body).map_err(invalid)?;
            let changes = request
                .values
                .into_iter()
                .map(|(key, value)| (canonical(&key).to_owned(), value))
                .collect();
            let changed = set(profile, changes)?;
            announce(profile, &changed, "local");
            view(profile)
        }
        "preferences.import" => {
            // A one-time move from a client's own storage: a key core
            // already holds keeps its value.
            let request: Import = serde_json::from_value(body).map_err(invalid)?;
            let held = values(profile)?;
            let mut changes = BTreeMap::new();
            let mut skipped = Vec::new();
            for (name, value) in request.values {
                let key = canonical(&name).to_owned();
                let value = imported(&key, value);
                if held.contains_key(&key) || check(&key, &value).is_err() {
                    skipped.push(name);
                    continue;
                }
                changes.insert(key, Some(value));
            }
            let changed = set(profile, changes)?;
            announce(profile, &changed, "local");
            let mut answer = view(profile)?;
            answer["imported"] = json!(changed.keys().collect::<Vec<_>>());
            answer["skipped"] = json!(skipped);
            Ok(answer)
        }
        _ => Err("The preferences operation is unknown.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn set_get_and_import_keep_core_values_and_refuse_unknown_keys() {
        let profile = profile();
        let answer = operation(
            &profile,
            "preferences.set",
            json!({"values": {"theme": "dark", "muniment.sidebar-width": 280}}),
        )
        .unwrap();
        assert_eq!(answer["values"]["theme"], "dark");
        assert_eq!(answer["values"]["sidebar-width"], 280);
        assert!(operation(
            &profile,
            "preferences.set",
            json!({"values": {"catalog-archive": "x"}})
        )
        .is_err());
        let imported = operation(
            &profile,
            "preferences.import",
            json!({"values": {
                "muniment.theme": "light",
                "muniment.type": "{\"size\":15}",
                "muniment.memory-recall": "{\"learn\":false,\"use\":true,\"ask\":true}",
                "muniment.thread-organization": "[]"
            }}),
        )
        .unwrap();
        assert_eq!(imported["values"]["theme"], "dark");
        assert_eq!(imported["values"]["type"], "{\"size\":15}");
        assert_eq!(imported["values"]["memory-recall"]["learn"], false);
        assert_eq!(imported["imported"], json!(["memory-recall", "type"]));
        assert_eq!(
            imported["skipped"],
            json!(["muniment.theme", "muniment.thread-organization"])
        );
        assert!(
            !crate::memory_files::recall_settings(&profile)
                .unwrap()
                .learn
        );
        let removed = operation(
            &profile,
            "preferences.set",
            json!({"values": {"theme": null}}),
        )
        .unwrap();
        assert!(removed["values"].get("theme").is_none());
        std::fs::remove_dir_all(profile).unwrap();
    }
}
