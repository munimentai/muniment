//! Preferences: one record per fleet-wide key, last writer wins. The
//! machine-local keys never become slots.

use std::collections::BTreeMap;

use super::{Adapter, Change, Context, Local, Slot};

pub const CLASS: &str = "preference";
const FIELD: &str = "value";

pub(super) struct Preferences;

impl Adapter for Preferences {
    fn class(&self) -> &'static str {
        CLASS
    }

    fn read(&self, context: &Context<'_>) -> Result<Local, String> {
        Ok(crate::preferences::values(context.profile)?
            .into_iter()
            .filter(|(key, _)| crate::preferences::is_fleet_key(key))
            .map(|(key, value)| (Slot::field(key, FIELD), value))
            .collect())
    }

    fn write(&self, context: &Context<'_>, changes: &[Change]) -> Result<(), String> {
        let changes: BTreeMap<String, Option<serde_json::Value>> = changes
            .iter()
            .filter(|change| {
                change.slot.field == FIELD
                    && crate::preferences::is_fleet_key(&change.slot.key)
                    && change.value.as_ref().is_none_or(|value| {
                        crate::preferences::check(&change.slot.key, value).is_ok()
                    })
            })
            .map(|change| (change.slot.key.clone(), change.value.clone()))
            .collect();
        let changed = crate::preferences::set(context.profile, changes)?;
        crate::preferences::announce(context.profile, &changed, "fleet");
        Ok(())
    }
}
