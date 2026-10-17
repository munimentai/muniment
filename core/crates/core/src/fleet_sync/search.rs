//! Web search settings: the provider order and the built-in engines, last
//! writer wins per field. Provider keys stay where they were entered, and the
//! search server's endpoint file stays on its machine.

use serde_json::json;

use super::{Adapter, Change, Context, Local, Slot};
use crate::search_settings::{self, OrderEntry};

pub const CLASS: &str = "search";
const KEY: &str = "settings";

pub(super) struct Search;

impl Adapter for Search {
    fn class(&self) -> &'static str {
        CLASS
    }

    fn tombstones(&self) -> bool {
        false
    }

    fn read(&self, context: &Context<'_>) -> Result<Local, String> {
        // Defaults are not choices: only a saved record takes part.
        if !search_settings::settings_path(context.profile).is_file() {
            return Ok(Local::new());
        }
        let settings = search_settings::load(context.profile).map_err(|error| error.to_string())?;
        Ok(Local::from([
            (Slot::field(KEY, "order"), json!(settings.order)),
            (Slot::field(KEY, "engines"), json!(settings.engines)),
        ]))
    }

    fn write(&self, context: &Context<'_>, changes: &[Change]) -> Result<(), String> {
        let mut settings =
            search_settings::load(context.profile).map_err(|error| error.to_string())?;
        for change in changes {
            let Some(value) = change.value.clone() else {
                continue;
            };
            match change.slot.field.as_str() {
                "order" => {
                    if let Ok(order) = serde_json::from_value::<Vec<OrderEntry>>(value) {
                        // A provider without a key here leaves the order
                        // until its key arrives.
                        settings.order = order;
                        settings.normalize();
                    }
                }
                "engines" => {
                    if let Ok(engines) = serde_json::from_value::<Vec<String>>(value) {
                        let _ = settings.set_engines(engines);
                    }
                }
                _ => {}
            }
        }
        search_settings::save(context.profile, &settings).map_err(|error| error.to_string())?;
        crate::pi_settings::store_web_search(context.profile).map_err(|error| error.to_string())
    }
}
