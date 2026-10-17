//! User memory files: each file under `<Home>/memory` is a record keyed by
//! its path there, last writer wins. A record carries the hash of the
//! version its writer edited, so a member whose own edit lost to one made
//! without seeing it keeps its text as a conflict copy, `name (from
//! <machine>).md`, which then syncs like any file. The search index, recall
//! counts and pending changes stay on each machine; the index takes in the
//! changed files on its next build.

use serde_json::{json, Value};

use super::{Adapter, Base, Change, Context, Local, Slot};
use crate::memory_files;

pub const CLASS: &str = "memory";
const FIELD: &str = "file";

pub(super) struct Memory;

/// The name of the copy that keeps `relative`'s text, unused in `taken`.
fn conflict_name(relative: &str, machine: &str, taken: &Local) -> String {
    let machine: String = machine
        .chars()
        .filter(|c| !c.is_control() && !"/\\<>:\"|?*".contains(*c))
        .take(40)
        .collect();
    let stem = relative.strip_suffix(".md").unwrap_or(relative);
    let mut index = 1;
    loop {
        let suffix = if index == 1 {
            String::new()
        } else {
            format!(" {index}")
        };
        let name = format!("{stem} (from {machine}){suffix}.md");
        if !taken.keys().any(|slot| slot.key == name) {
            return name;
        }
        index += 1;
    }
}

/// Keeps `text`, this machine's version of `relative`, beside the version
/// that won. A fact's file is named by its id, so its copy is a new fact
/// whose title names the machine.
fn keep_copy(
    context: &Context<'_>,
    relative: &str,
    text: &str,
    taken: &Local,
) -> Result<(), String> {
    if let Some(name) = relative.strip_prefix("facts/") {
        if name
            .strip_suffix(".md")
            .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
        {
            let (heading, rest) = text.split_once('\n').unwrap_or((text, ""));
            let copy = format!("{heading} (from {})\n{rest}", context.name);
            let id = uuid::Uuid::new_v4();
            return memory_files::sync_write(context.profile, &format!("facts/{id}.md"), &copy);
        }
    }
    let name = conflict_name(relative, &context.name, taken);
    memory_files::sync_write(context.profile, &name, text)
}

impl Adapter for Memory {
    fn class(&self) -> &'static str {
        CLASS
    }

    fn read(&self, context: &Context<'_>) -> Result<Local, String> {
        if crate::home::configured_home(context.profile)
            .ok()
            .flatten()
            .is_none()
        {
            return Ok(Local::new());
        }
        Ok(memory_files::sync_files(context.profile)?
            .into_iter()
            .map(|(relative, text)| (Slot::field(relative, FIELD), json!({"content": text})))
            .collect())
    }

    fn outgoing(&self, _slot: &Slot, mut value: Value, previous: Option<&Base>) -> Value {
        value["base"] = json!(previous.and_then(|base| base.remote.clone()));
        value
    }

    fn comparable(&self, value: &Value) -> Value {
        json!({"content": value["content"]})
    }

    fn write(&self, context: &Context<'_>, changes: &[Change]) -> Result<(), String> {
        let local = self.read(context)?;
        for change in changes {
            if change.slot.field != FIELD {
                continue;
            }
            let relative = change.slot.key.as_str();
            let Some(value) = &change.value else {
                if let Err(error) = memory_files::sync_remove(context.profile, relative) {
                    crate::runtime_eprintln!(
                        "muniment-runtime: fleet sync could not remove a memory: {error}"
                    );
                }
                continue;
            };
            let Some(content) = value["content"].as_str() else {
                continue;
            };
            // This machine wrote the version that lost, and the winner did
            // not edit it: both edits stay. A file this machine never
            // synced, such as one it held before it joined, stays too.
            let concurrent = match change.previous.as_ref() {
                Some(previous) => {
                    previous.mine
                        && previous.remote.is_some()
                        && value["base"].as_str() != previous.remote.as_deref()
                }
                None => true,
            };
            if concurrent {
                if let Some(mine) = local
                    .get(&change.slot)
                    .and_then(|held| held["content"].as_str())
                    .filter(|mine| *mine != content)
                {
                    keep_copy(context, relative, mine, &local)?;
                }
            }
            if let Err(error) = memory_files::sync_write(context.profile, relative, content) {
                crate::runtime_eprintln!(
                    "muniment-runtime: fleet sync could not write a memory: {error}"
                );
            }
        }
        Ok(())
    }
}
