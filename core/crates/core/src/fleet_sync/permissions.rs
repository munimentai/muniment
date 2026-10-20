//! Permission settings: the default modes and the unattended wait are last
//! writer wins. Whether bypass may be chosen stays on each machine. A slot
//! this adapter does not name, such as a rule list an older member sends,
//! changes nothing.

use serde_json::{json, Value};

use super::{Adapter, Change, Context, Local, Slot};
use crate::permission_gate::settings::{self, PermissionSettings};
use crate::permission_gate::PermissionMode;

pub const CLASS: &str = "permission";
const SETTINGS: &str = "settings";

pub(super) struct Permissions;

impl Adapter for Permissions {
    fn class(&self) -> &'static str {
        CLASS
    }

    fn tombstones(&self) -> bool {
        false
    }

    fn read(&self, context: &Context<'_>) -> Result<Local, String> {
        let mut local = Local::new();
        // Defaults are not choices: only a saved file takes part.
        if !context.profile.join(settings::SETTINGS_FILE).is_file() {
            return Ok(local);
        }
        let current = settings::load(context.profile);
        local.insert(
            Slot::field(SETTINGS, "default_thread_mode"),
            json!(current.default_thread_mode),
        );
        local.insert(
            Slot::field(SETTINGS, "default_agent_mode"),
            json!(current.default_agent_mode),
        );
        local.insert(
            Slot::field(SETTINGS, "unattended_timeout_seconds"),
            json!(current.unattended_timeout_seconds),
        );
        Ok(local)
    }

    fn write(&self, context: &Context<'_>, changes: &[Change]) -> Result<(), String> {
        let mut current: PermissionSettings = settings::load(context.profile);
        for change in changes {
            let slot = &change.slot;
            let value = change.value.as_ref();
            match (slot.key.as_str(), slot.field.as_str(), &slot.element) {
                (SETTINGS, "default_thread_mode" | "default_agent_mode", None) => {
                    let Some(mode) = value.and_then(|value| {
                        serde_json::from_value::<PermissionMode>(value.clone()).ok()
                    }) else {
                        continue;
                    };
                    // A mode this machine does not allow keeps the local one.
                    if current.unavailable(mode).is_some() {
                        continue;
                    }
                    if slot.field == "default_thread_mode" {
                        current.default_thread_mode = mode;
                    } else {
                        current.default_agent_mode = mode;
                    }
                }
                (SETTINGS, "unattended_timeout_seconds", None) => {
                    if let Some(seconds) = value.and_then(Value::as_u64) {
                        current.unattended_timeout_seconds = seconds;
                    }
                }
                _ => {}
            }
        }
        settings::save(context.profile, current).map(drop)
    }
}
