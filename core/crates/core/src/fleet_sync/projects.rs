//! Projects: each project is a record `{name, owner_runtime, path}`, last
//! writer wins per field, and each thread's project is a record keyed by the
//! thread. Files never sync: the path names the folder on the owner, and the
//! other members list the project with its files there.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::{Adapter, Change, Context, Local, Slot};
use crate::projects::{self, RemoteProject};

pub const PROJECT_CLASS: &str = "project";
pub const MEMBER_CLASS: &str = "project-thread";
const MEMBER_FIELD: &str = "project";

pub(super) struct Projects;

impl Adapter for Projects {
    fn class(&self) -> &'static str {
        PROJECT_CLASS
    }

    fn read(&self, context: &Context<'_>) -> Result<Local, String> {
        let catalog = projects::saved(context.profile)?;
        let mut local = Local::new();
        for (id, name) in &catalog.projects {
            let path = projects::folder(context.profile, id)
                .map(|folder| folder.to_string_lossy().into_owned())
                .unwrap_or_default();
            local.insert(Slot::field(id, "name"), json!(name));
            local.insert(Slot::field(id, "owner_runtime"), json!(context.origin));
            local.insert(Slot::field(id, "path"), json!(path));
        }
        for (id, project) in &catalog.remote {
            if catalog.projects.contains_key(id) {
                continue;
            }
            local.insert(Slot::field(id, "name"), json!(project.name));
            local.insert(
                Slot::field(id, "owner_runtime"),
                json!(project.owner_runtime),
            );
            local.insert(Slot::field(id, "path"), json!(project.path));
        }
        Ok(local)
    }

    fn write(&self, context: &Context<'_>, changes: &[Change]) -> Result<(), String> {
        let mut keys: Vec<&str> = changes
            .iter()
            .map(|change| change.slot.key.as_str())
            .collect();
        keys.dedup();
        let owned = projects::saved(context.profile)?.projects;
        for id in keys {
            let record = context.records.get(PROJECT_CLASS, id)?;
            let fields: BTreeMap<String, Value> = record
                .map(|record| {
                    record
                        .fields
                        .into_iter()
                        .map(|(name, field)| (name, field.value))
                        .collect()
                })
                .unwrap_or_default();
            let text = |name: &str| fields.get(name).and_then(Value::as_str).map(str::to_owned);
            if let Some(held) = owned.get(id) {
                match text("name") {
                    Some(name) if &name != held => {
                        // The owner renames its folder to match.
                        if let Err(error) = projects::rename(context.profile, id, &name) {
                            crate::runtime_eprintln!(
                                "muniment-runtime: fleet sync could not rename a project: {error}"
                            );
                        }
                    }
                    Some(_) => {}
                    None => {
                        // The owner decides whether its project exists, so
                        // it writes the project again.
                        for (field, value) in [
                            ("name", json!(held)),
                            ("owner_runtime", json!(context.origin)),
                        ] {
                            context.records.put(PROJECT_CLASS, id, field, value)?;
                        }
                    }
                }
                continue;
            }
            let project = match (text("name"), text("owner_runtime")) {
                (Some(name), Some(owner_runtime)) => Some(RemoteProject {
                    name,
                    owner_runtime,
                    path: text("path").unwrap_or_default(),
                }),
                _ => None,
            };
            projects::update(context.profile, |catalog| match project {
                Some(project) => {
                    catalog.remote.insert(id.to_owned(), project.clone()) != Some(project)
                }
                None => catalog.remote.remove(id).is_some(),
            })?;
        }
        Ok(())
    }
}

pub(super) struct Members;

impl Adapter for Members {
    fn class(&self) -> &'static str {
        MEMBER_CLASS
    }

    fn read(&self, context: &Context<'_>) -> Result<Local, String> {
        Ok(projects::saved(context.profile)?
            .threads
            .into_iter()
            .map(|(thread, project)| (Slot::field(thread, MEMBER_FIELD), json!(project)))
            .collect())
    }

    fn write(&self, context: &Context<'_>, changes: &[Change]) -> Result<(), String> {
        projects::update(context.profile, |catalog| {
            let mut changed = false;
            for change in changes {
                if change.slot.field != MEMBER_FIELD {
                    continue;
                }
                let thread = change.slot.key.clone();
                changed |= match change.value.as_ref().and_then(Value::as_str) {
                    Some(project) => {
                        catalog
                            .threads
                            .insert(thread, project.to_owned())
                            .as_deref()
                            != Some(project)
                    }
                    None => catalog.threads.remove(&thread).is_some(),
                };
            }
            changed
        })
    }
}
