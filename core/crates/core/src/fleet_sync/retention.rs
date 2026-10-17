//! The thread retention choice: last writer wins. Each member applies it to
//! the threads it owns, and the deletions replicate as thread events.

use serde_json::json;

use super::{Adapter, Change, Context, Local, Slot};
use crate::retention_record::{
    read_retention_choice, write_retention_choice, RetentionChoice, RECORD_FILE,
};

pub const CLASS: &str = "retention";
const KEY: &str = "threads";
const FIELD: &str = "choice";

pub(super) struct Retention;

impl Adapter for Retention {
    fn class(&self) -> &'static str {
        CLASS
    }

    fn tombstones(&self) -> bool {
        false
    }

    fn read(&self, context: &Context<'_>) -> Result<Local, String> {
        Ok(read_retention_choice(context.profile)
            .map(|choice| (Slot::field(KEY, FIELD), json!(choice)))
            .into_iter()
            .collect())
    }

    fn write(&self, context: &Context<'_>, changes: &[Change]) -> Result<(), String> {
        for change in changes {
            let Some(value) = change.value.clone() else {
                // No recorded choice keeps every thread.
                match std::fs::remove_file(context.profile.join(RECORD_FILE)) {
                    Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                        return Err(error.to_string())
                    }
                    _ => continue,
                }
            };
            if let Ok(choice) = serde_json::from_value::<RetentionChoice>(value) {
                write_retention_choice(context.profile, choice)
                    .map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }
}
