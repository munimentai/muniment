//! Permission settings: the default modes and the unattended wait are last
//! writer wins. The deny and ask-first rules are sets keyed by the request's
//! hash, so a rule one member adds while another removes it stays. The
//! always-allow rules are one field per request hash, so the later of an
//! add and a removal wins. Whether bypass may be chosen stays on each
//! machine.

use serde_json::{json, Value};

use super::{hash, Adapter, Change, Context, Local, Slot};
use crate::permission_gate::settings::{self, PermissionSettings, MAX_GLOBAL_RULES};
use crate::permission_gate::{PermissionMode, PolicyRequest};

pub const CLASS: &str = "permission";
const SETTINGS: &str = "settings";
const RULES: &str = "rules";
const ALLOW: &str = "allow";
const DENY: &str = "deny";
const ASK_FIRST: &str = "ask_first";

pub(super) struct Permissions;

fn request_slots(local: &mut Local, requests: &[PolicyRequest], slot: impl Fn(String) -> Slot) {
    for request in requests {
        let value = json!(request);
        local.insert(slot(hash(&value)), value);
    }
}

fn apply_rule(list: &mut Vec<PolicyRequest>, element: &str, value: Option<&Value>) {
    let held = |request: &PolicyRequest| hash(&json!(request)) == element;
    match value.and_then(|value| serde_json::from_value::<PolicyRequest>(value.clone()).ok()) {
        Some(request) => {
            if !list.iter().any(held) && list.len() < MAX_GLOBAL_RULES {
                list.push(request);
            }
        }
        None if value.is_none() => list.retain(|request| !held(request)),
        None => {}
    }
}

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
        request_slots(&mut local, &current.deny, |element| {
            Slot::element(RULES, DENY, element)
        });
        request_slots(&mut local, &current.ask_first, |element| {
            Slot::element(RULES, ASK_FIRST, element)
        });
        request_slots(&mut local, &current.always_allow, |element| {
            Slot::field(ALLOW, element)
        });
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
                (RULES, DENY, Some(element)) => apply_rule(&mut current.deny, element, value),
                (RULES, ASK_FIRST, Some(element)) => {
                    apply_rule(&mut current.ask_first, element, value)
                }
                (ALLOW, element, None) => apply_rule(&mut current.always_allow, element, value),
                _ => {}
            }
        }
        settings::save(context.profile, current).map(drop)
    }
}
