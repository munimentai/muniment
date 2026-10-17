//! The runtime's permission settings: the default modes of new threads and
//! agents, whether `bypass` may be chosen, how long an unattended run waits
//! for an answer, and the global rules that hold in every thread. They live
//! in `<profile>/permission-settings.json`.

use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{PermissionMode, PolicyRequest, RunPermissions};
use crate::journal::{
    ThreadPermissionAnswerSource, ThreadPermissionDecision, ThreadPermissionPolicyDecided,
};

pub const SETTINGS_FILE: &str = "permission-settings.json";
/// How long an unattended run waits for an answer before its action is
/// denied, unless the settings name another time.
pub const DEFAULT_UNATTENDED_TIMEOUT_SECONDS: u64 = 4 * 60 * 60;
const MIN_UNATTENDED_TIMEOUT_SECONDS: u64 = 60;
const MAX_UNATTENDED_TIMEOUT_SECONDS: u64 = 7 * 24 * 60 * 60;
/// The most requests in each global rule list.
pub const MAX_GLOBAL_RULES: usize = 64;

static WRITE: Mutex<()> = Mutex::new(());

fn default_timeout() -> u64 {
    DEFAULT_UNATTENDED_TIMEOUT_SECONDS
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionSettings {
    #[serde(default)]
    pub default_thread_mode: PermissionMode,
    #[serde(default)]
    pub default_agent_mode: PermissionMode,
    /// Whether `bypass` may be chosen. Off unless the owner turns it on.
    #[serde(default)]
    pub bypass_enabled: bool,
    #[serde(default = "default_timeout")]
    pub unattended_timeout_seconds: u64,
    /// Exact requests every thread refuses.
    #[serde(default)]
    pub deny: Vec<PolicyRequest>,
    /// Exact requests every thread asks before.
    #[serde(default)]
    pub ask_first: Vec<PolicyRequest>,
    /// Exact requests every thread runs without asking.
    #[serde(default)]
    pub always_allow: Vec<PolicyRequest>,
}

impl Default for PermissionSettings {
    fn default() -> Self {
        Self {
            default_thread_mode: PermissionMode::Manual,
            default_agent_mode: PermissionMode::Manual,
            bypass_enabled: false,
            unattended_timeout_seconds: DEFAULT_UNATTENDED_TIMEOUT_SECONDS,
            deny: Vec::new(),
            ask_first: Vec::new(),
            always_allow: Vec::new(),
        }
    }
}

impl PermissionSettings {
    /// Why `mode` cannot be chosen now, or `None` when it can.
    pub fn unavailable(&self, mode: PermissionMode) -> Option<&'static str> {
        (mode == PermissionMode::Bypass && !self.bypass_enabled).then_some(
            "Bypass is off. Turn it on in the runtime's permission settings, and only on a container or a disposable machine.",
        )
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

    /// The global rules as pre-decided thread policies.
    pub fn policies(&self) -> Vec<ThreadPermissionPolicyDecided> {
        super::rule_policies("global", &self.deny, &self.ask_first, &self.always_allow)
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
        for requests in [&self.deny, &self.ask_first, &self.always_allow] {
            super::validate_rules(requests, MAX_GLOBAL_RULES)?;
        }
        Ok(())
    }
}

/// The runtime's permission settings. A missing or unreadable file reads
/// as the defaults: `manual` for threads and agents, bypass off.
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

/// The gate of a thread that belongs to no agent: the default thread mode
/// and the global rules.
pub fn thread_permissions(profile: &Path) -> RunPermissions {
    let settings = load(profile);
    RunPermissions {
        mode: settings.effective(settings.default_thread_mode),
        policies: settings.policies(),
        bypass_enabled: settings.bypass_enabled,
        ..RunPermissions::default()
    }
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
        default_thread_mode: Option<PermissionMode>,
        default_agent_mode: Option<PermissionMode>,
        bypass_enabled: Option<bool>,
        unattended_timeout_seconds: Option<u64>,
        deny: Option<Vec<PolicyRequest>>,
        ask_first: Option<Vec<PolicyRequest>>,
        always_allow: Option<Vec<PolicyRequest>>,
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
            let change: Save = serde_json::from_value(body).map_err(invalid)?;
            let mut settings = load(profile);
            if let Some(mode) = change.default_thread_mode {
                settings.default_thread_mode = mode;
            }
            if let Some(mode) = change.default_agent_mode {
                settings.default_agent_mode = mode;
            }
            if let Some(enabled) = change.bypass_enabled {
                settings.bypass_enabled = enabled;
            }
            if let Some(seconds) = change.unattended_timeout_seconds {
                settings.unattended_timeout_seconds = seconds;
            }
            if let Some(rules) = change.deny {
                settings.deny = rules;
            }
            if let Some(rules) = change.ask_first {
                settings.ask_first = rules;
            }
            if let Some(rules) = change.always_allow {
                settings.always_allow = rules;
            }
            Ok(json!(save(profile, settings)?))
        }
        _ => Err("The permission operation is unknown.".into()),
    }
}

/// Builds the pre-decided policies of one rule set, actor `actor`.
pub(super) fn decided(
    actor: &str,
    request: &PolicyRequest,
    decision: ThreadPermissionDecision,
) -> ThreadPermissionPolicyDecided {
    ThreadPermissionPolicyDecided {
        policy_id: uuid::Uuid::now_v7().to_string(),
        decision,
        request_kind: request.request_kind.clone(),
        resource: request.resource.clone(),
        actor: actor.to_owned(),
        answer_source: ThreadPermissionAnswerSource::Native,
        gate_id: None,
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
    fn defaults_are_manual_and_bypass_is_off_until_enabled() {
        let profile = profile();
        let settings = load(&profile);
        assert_eq!(settings.default_thread_mode, PermissionMode::Manual);
        assert_eq!(settings.default_agent_mode, PermissionMode::Manual);
        assert!(!settings.bypass_enabled);
        assert_eq!(settings.unattended_timeout_seconds, 4 * 3600);
        let listed = operation(&profile, "permission.modes", json!({})).unwrap();
        let bypass = listed["modes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|mode| mode["id"] == "bypass")
            .unwrap();
        assert_eq!(bypass["available"], false);
        assert!(bypass["reason"].as_str().unwrap().contains("container"));
        assert!(bypass["description"]
            .as_str()
            .unwrap()
            .contains("container"));
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
        let saved = operation(
            &profile,
            "permission.settings_save",
            json!({"bypass_enabled": true, "default_thread_mode": "bypass", "default_agent_mode": "edit_auto"}),
        )
        .unwrap();
        assert_eq!(saved["default_thread_mode"], "bypass");
        assert_eq!(saved["default_agent_mode"], "edit_auto");
        let listed = operation(&profile, "permission.modes", json!({})).unwrap();
        assert!(listed["modes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|mode| mode["available"] == true && mode.get("reason").is_none()));
        // Bypass cannot be turned off while a default still names it.
        assert!(operation(
            &profile,
            "permission.settings_save",
            json!({"bypass_enabled": false})
        )
        .is_err());
        assert!(operation(
            &profile,
            "permission.settings_save",
            json!({"unattended_timeout_seconds": 5})
        )
        .is_err());
        assert!(operation(&profile, "permission.settings", json!({"x": 1})).is_err());
        std::fs::remove_dir_all(profile).unwrap();
    }
}
