//! The runtime's permission settings: the default modes of new threads and
//! agents, whether `bypass` may be chosen, and how long an unattended run
//! waits for an answer. They live in `<profile>/permission-settings.json`.

use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{PermissionMode, RunPermissions};

pub const SETTINGS_FILE: &str = "permission-settings.json";
/// How long an unattended run waits for an answer before its action is
/// denied, unless the settings name another time.
pub const DEFAULT_UNATTENDED_TIMEOUT_SECONDS: u64 = 4 * 60 * 60;
const MIN_UNATTENDED_TIMEOUT_SECONDS: u64 = 60;
const MAX_UNATTENDED_TIMEOUT_SECONDS: u64 = 7 * 24 * 60 * 60;

static WRITE: Mutex<()> = Mutex::new(());

fn default_timeout() -> u64 {
    DEFAULT_UNATTENDED_TIMEOUT_SECONDS
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "StoredSettings")]
pub struct PermissionSettings {
    pub default_thread_mode: PermissionMode,
    pub default_agent_mode: PermissionMode,
    /// Whether `bypass` may be chosen. On unless the owner turns it off.
    pub bypass_enabled: bool,
    pub unattended_timeout_seconds: u64,
}

/// The settings file as written. A field it leaves out takes its default:
/// bypass on, and each default mode `bypass` while bypass is on, else
/// `manual`. The rule lists an older file holds are read and dropped.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredSettings {
    default_thread_mode: Option<PermissionMode>,
    default_agent_mode: Option<PermissionMode>,
    bypass_enabled: Option<bool>,
    #[serde(default = "default_timeout")]
    unattended_timeout_seconds: u64,
    #[serde(default, rename = "deny")]
    _deny: serde::de::IgnoredAny,
    #[serde(default, rename = "ask_first")]
    _ask_first: serde::de::IgnoredAny,
    #[serde(default, rename = "always_allow")]
    _always_allow: serde::de::IgnoredAny,
}

impl From<StoredSettings> for PermissionSettings {
    fn from(stored: StoredSettings) -> Self {
        let bypass_enabled = stored.bypass_enabled.unwrap_or(true);
        let fallback = fallback_mode(bypass_enabled);
        Self {
            default_thread_mode: stored.default_thread_mode.unwrap_or(fallback),
            default_agent_mode: stored.default_agent_mode.unwrap_or(fallback),
            bypass_enabled,
            unattended_timeout_seconds: stored.unattended_timeout_seconds,
        }
    }
}

/// The default mode while bypass is `enabled` or off.
fn fallback_mode(enabled: bool) -> PermissionMode {
    if enabled {
        PermissionMode::Bypass
    } else {
        PermissionMode::Manual
    }
}

impl Default for PermissionSettings {
    fn default() -> Self {
        Self {
            default_thread_mode: PermissionMode::Bypass,
            default_agent_mode: PermissionMode::Bypass,
            bypass_enabled: true,
            unattended_timeout_seconds: DEFAULT_UNATTENDED_TIMEOUT_SECONDS,
        }
    }
}

impl PermissionSettings {
    /// Why `mode` cannot be chosen now, or `None` when it can.
    pub fn unavailable(&self, mode: PermissionMode) -> Option<&'static str> {
        (mode == PermissionMode::Bypass && !self.bypass_enabled)
            .then_some("Bypass is off. Turn it on in the runtime's permission settings.")
    }

    /// The mode a run uses when `mode` is chosen: an unavailable mode runs
    /// as `manual`.
    pub fn effective(&self, mode: PermissionMode) -> PermissionMode {
        if self.unavailable(mode).is_some() {
            PermissionMode::Manual
        } else {
            mode
        }
    }

    fn validate(&self) -> Result<(), String> {
        for mode in [self.default_thread_mode, self.default_agent_mode] {
            if let Some(reason) = self.unavailable(mode) {
                return Err(reason.into());
            }
        }
        if !(MIN_UNATTENDED_TIMEOUT_SECONDS..=MAX_UNATTENDED_TIMEOUT_SECONDS)
            .contains(&self.unattended_timeout_seconds)
        {
            return Err("Choose an unattended wait between one minute and seven days.".into());
        }
        Ok(())
    }
}

/// The runtime's permission settings. A missing or unreadable file reads
/// as the defaults: `bypass` for threads and agents, bypass on.
pub fn load(profile: &Path) -> PermissionSettings {
    let path = profile.join(SETTINGS_FILE);
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            crate::runtime_eprintln!("muniment-runtime: permission settings unreadable");
            PermissionSettings::default()
        }),
        Err(_) => PermissionSettings::default(),
    }
}

/// Saves the settings after they pass validation.
pub fn save(profile: &Path, settings: PermissionSettings) -> Result<PermissionSettings, String> {
    settings.validate()?;
    let _guard = WRITE.lock().map_err(|_| "Permission settings are busy.")?;
    let path = profile.join(SETTINGS_FILE);
    if path.is_symlink() {
        return Err("Permission settings cannot be saved.".into());
    }
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        std::fs::create_dir_all(profile).map_err(|e| e.to_string())?;
        std::fs::write(
            &temp,
            serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        crate::atomic_file::replace(&temp, &path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result.map_err(|_| "Permission settings cannot be saved.".to_string())?;
    Ok(settings)
}

/// The modes for a picker, in menu order, with whether each can be chosen.
pub fn modes(settings: &PermissionSettings) -> Value {
    let modes: Vec<Value> = PermissionMode::ALL
        .iter()
        .map(|mode| {
            let mut entry = json!({
                "id": mode.id(),
                "title": mode.title(),
                "description": mode.description(),
                "available": settings.unavailable(*mode).is_none(),
            });
            if let Some(reason) = settings.unavailable(*mode) {
                entry["reason"] = json!(reason);
            }
            entry
        })
        .collect();
    json!({
        "modes": modes,
        "default_thread_mode": settings.default_thread_mode,
        "default_agent_mode": settings.default_agent_mode,
    })
}

/// The mode a new thread starts in, journaled when the thread is created.
pub fn new_thread_mode(profile: &Path) -> PermissionMode {
    let settings = load(profile);
    settings.effective(settings.default_thread_mode)
}

/// The mode a new agent is saved in when its save names none.
pub fn new_agent_mode(profile: &Path) -> PermissionMode {
    let settings = load(profile);
    settings.effective(settings.default_agent_mode)
}

/// The gate of a thread that belongs to no agent: the default thread mode.
pub fn thread_permissions(profile: &Path) -> RunPermissions {
    let settings = load(profile);
    RunPermissions {
        mode: settings.effective(settings.default_thread_mode),
        bypass_enabled: settings.bypass_enabled,
        ..RunPermissions::default()
    }
}

/// The machines one `permission.settings_save` changes. `machine` changes
/// the runtime that answers, and fleet sync carries the shared settings on.
/// `all` changes only the shared settings, so the change reaches every
/// member, and refuses a change to a setting that `settings.scope` keeps on
/// each machine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Scope {
    #[default]
    Machine,
    All,
}

/// Answers `permission.modes`, `permission.settings` and
/// `permission.settings_save`.
pub fn operation(profile: &Path, operation: &str, body: Value) -> Result<Value, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Empty {}
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Save {
        #[serde(default)]
        scope: Scope,
        default_thread_mode: Option<PermissionMode>,
        default_agent_mode: Option<PermissionMode>,
        bypass_enabled: Option<bool>,
        unattended_timeout_seconds: Option<u64>,
    }
    let invalid = |_| "The request is invalid.".to_string();
    match operation {
        "permission.modes" => {
            serde_json::from_value::<Empty>(body).map_err(invalid)?;
            Ok(modes(&load(profile)))
        }
        "permission.settings" => {
            serde_json::from_value::<Empty>(body).map_err(invalid)?;
            Ok(json!(load(profile)))
        }
        "permission.settings_save" => {
            let change: Save = serde_json::from_value(body.clone()).map_err(invalid)?;
            if change.scope == Scope::All {
                crate::settings_scope::check_save(operation, &body)?;
            }
            let mut settings = load(profile);
            if let Some(mode) = change.default_thread_mode {
                settings.default_thread_mode = mode;
            }
            if let Some(mode) = change.default_agent_mode {
                settings.default_agent_mode = mode;
            }
            if let Some(enabled) = change.bypass_enabled {
                settings.bypass_enabled = enabled;
                // Turning bypass off moves a default that names it, and
                // that this change does not set, to `manual`.
                if !enabled {
                    if change.default_thread_mode.is_none()
                        && settings.default_thread_mode == PermissionMode::Bypass
                    {
                        settings.default_thread_mode = PermissionMode::Manual;
                    }
                    if change.default_agent_mode.is_none()
                        && settings.default_agent_mode == PermissionMode::Bypass
                    {
                        settings.default_agent_mode = PermissionMode::Manual;
                    }
                }
            }
            if let Some(seconds) = change.unattended_timeout_seconds {
                settings.unattended_timeout_seconds = seconds;
            }
            let saved = save(profile, settings)?;
            crate::fleet_sync::changed(profile, crate::fleet_sync::PERMISSIONS_CLASS);
            Ok(json!(saved))
        }
        _ => Err("The permission operation is unknown.".into()),
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
    fn an_older_file_with_rule_lists_loads_and_a_save_drops_them() {
        let profile = profile();
        let push = json!({"request_kind": "command", "resource": {"type": "command",
            "arguments": ["bash", "git push"], "working_directory": "/work"}});
        std::fs::write(
            profile.join(SETTINGS_FILE),
            json!({"default_thread_mode": "auto", "deny": [push], "ask_first": [push],
                "always_allow": [push]})
            .to_string(),
        )
        .unwrap();
        let settings = load(&profile);
        assert_eq!(settings.default_thread_mode, PermissionMode::Auto);
        save(&profile, settings).unwrap();
        let written: Value =
            serde_json::from_slice(&std::fs::read(profile.join(SETTINGS_FILE)).unwrap()).unwrap();
        for field in ["deny", "ask_first", "always_allow"] {
            assert!(written.get(field).is_none(), "{field}");
        }
        // A save that still names a rule list is refused.
        assert!(operation(
            &profile,
            "permission.settings_save",
            json!({"always_allow": [push]})
        )
        .is_err());
        std::fs::remove_dir_all(profile).unwrap();
    }

    #[test]
    fn defaults_are_bypass_and_turning_bypass_off_falls_back_to_manual() {
        let profile = profile();
        let settings = load(&profile);
        assert_eq!(settings.default_thread_mode, PermissionMode::Bypass);
        assert_eq!(settings.default_agent_mode, PermissionMode::Bypass);
        assert!(settings.bypass_enabled);
        assert_eq!(settings.unattended_timeout_seconds, 4 * 3600);
        assert_eq!(
            settings.effective(PermissionMode::Bypass),
            PermissionMode::Bypass
        );
        let listed = operation(&profile, "permission.modes", json!({})).unwrap();
        assert_eq!(listed["default_thread_mode"], "bypass");
        assert_eq!(listed["default_agent_mode"], "bypass");
        assert!(listed["modes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|mode| mode["available"] == true && mode.get("reason").is_none()));
        let bypass = |listed: &Value| {
            listed["modes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|mode| mode["id"] == "bypass")
                .unwrap()
                .clone()
        };
        let description = bypass(&listed)["description"].as_str().unwrap().to_owned();
        assert!(!description.contains("unavailable") && !description.contains("container"));

        // Turning bypass off moves both defaults to manual.
        let saved = operation(
            &profile,
            "permission.settings_save",
            json!({"bypass_enabled": false}),
        )
        .unwrap();
        assert_eq!(saved["bypass_enabled"], false);
        assert_eq!(saved["default_thread_mode"], "manual");
        assert_eq!(saved["default_agent_mode"], "manual");
        let listed = operation(&profile, "permission.modes", json!({})).unwrap();
        assert_eq!(bypass(&listed)["available"], false);
        assert!(bypass(&listed)["reason"]
            .as_str()
            .unwrap()
            .contains("Bypass is off"));
        let settings = load(&profile);
        assert_eq!(
            settings.effective(PermissionMode::Bypass),
            PermissionMode::Manual
        );
        let refused = operation(
            &profile,
            "permission.settings_save",
            json!({"default_thread_mode": "bypass"}),
        );
        assert!(refused.unwrap_err().contains("Bypass is off"));
        // A default the same change names is never moved.
        assert!(operation(
            &profile,
            "permission.settings_save",
            json!({"bypass_enabled": false, "default_agent_mode": "bypass"})
        )
        .is_err());

        // Turning it on again leaves the saved defaults as they are.
        let saved = operation(
            &profile,
            "permission.settings_save",
            json!({"bypass_enabled": true, "default_agent_mode": "edit_auto"}),
        )
        .unwrap();
        assert_eq!(saved["default_thread_mode"], "manual");
        assert_eq!(saved["default_agent_mode"], "edit_auto");
        assert!(operation(
            &profile,
            "permission.settings_save",
            json!({"unattended_timeout_seconds": 5})
        )
        .is_err());
        assert!(operation(&profile, "permission.settings", json!({"x": 1})).is_err());
        std::fs::remove_dir_all(profile).unwrap();
    }

    #[test]
    fn saved_settings_stay_and_a_field_left_out_takes_its_default() {
        let profile = profile();
        // A file a person saved keeps its modes and bypass switch.
        std::fs::write(
            profile.join(SETTINGS_FILE),
            r#"{"default_thread_mode": "manual", "default_agent_mode": "auto", "bypass_enabled": false}"#,
        )
        .unwrap();
        let settings = load(&profile);
        assert_eq!(settings.default_thread_mode, PermissionMode::Manual);
        assert_eq!(settings.default_agent_mode, PermissionMode::Auto);
        assert!(!settings.bypass_enabled);
        // A file without the modes follows its bypass switch.
        std::fs::write(
            profile.join(SETTINGS_FILE),
            r#"{"bypass_enabled": false, "unattended_timeout_seconds": 600}"#,
        )
        .unwrap();
        let settings = load(&profile);
        assert_eq!(settings.default_thread_mode, PermissionMode::Manual);
        assert_eq!(settings.default_agent_mode, PermissionMode::Manual);
        assert_eq!(settings.unattended_timeout_seconds, 600);
        std::fs::write(
            profile.join(SETTINGS_FILE),
            r#"{"default_agent_mode": "plan"}"#,
        )
        .unwrap();
        let settings = load(&profile);
        assert!(settings.bypass_enabled);
        assert_eq!(settings.default_thread_mode, PermissionMode::Bypass);
        assert_eq!(settings.default_agent_mode, PermissionMode::Plan);
        // An unknown field still makes the file unreadable.
        std::fs::write(profile.join(SETTINGS_FILE), r#"{"x": 1}"#).unwrap();
        assert_eq!(load(&profile), PermissionSettings::default());
        std::fs::remove_dir_all(profile).unwrap();
    }

    #[test]
    fn the_all_scope_changes_shared_settings_and_refuses_a_machine_local_one() {
        let profile = profile();
        let saved = operation(
            &profile,
            "permission.settings_save",
            json!({"scope": "all", "default_agent_mode": "plan"}),
        )
        .unwrap();
        assert_eq!(saved["default_agent_mode"], "plan");
        let refused = operation(
            &profile,
            "permission.settings_save",
            json!({"scope": "all", "bypass_enabled": false}),
        )
        .unwrap_err();
        assert!(refused.contains("each machine"));
        assert!(load(&profile).bypass_enabled);
        // A refused save changes none of its settings.
        assert!(operation(
            &profile,
            "permission.settings_save",
            json!({"scope": "all", "default_thread_mode": "plan", "bypass_enabled": false}),
        )
        .is_err());
        assert_eq!(load(&profile).default_thread_mode, PermissionMode::Bypass);
        // One machine takes the change.
        let saved = operation(
            &profile,
            "permission.settings_save",
            json!({"scope": "machine", "bypass_enabled": false}),
        )
        .unwrap();
        assert_eq!(saved["bypass_enabled"], false);
        std::fs::remove_dir_all(profile).unwrap();
    }
}
