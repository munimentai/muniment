//! Where each Settings part keeps its values, for `settings.scope`: shared
//! across the fleet, kept on each machine, or kept on this computer, the
//! computer that shows Settings. A part is shared exactly when a fleet sync
//! record class carries it, and a shared part names each of its settings
//! that sync leaves on the machine. Each part carries the lines a client
//! shows beside it: `all` while one save changes every machine, `one` while
//! another machine is picked. `{machine}` in a line stands for the machine a
//! change reaches.
//!
//! A save that names `scope: "all"` changes every machine, so
//! [`check_save`] refuses it when it changes a setting that stays on one
//! machine.

use serde::Serialize;
use serde_json::{json, Value};

/// Where a part, or one setting of a shared part, keeps its values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Fleet sync carries it to every member.
    Fleet,
    /// Each machine keeps its own.
    Machine,
    /// This computer keeps it, whichever machine Settings names.
    ThisComputer,
}

/// Where a part keeps its values.
#[derive(Clone, Copy, Debug)]
pub enum Kept {
    /// The fleet sync record classes that carry the part.
    Synced(&'static [&'static str]),
    /// Each machine keeps its own.
    Machine,
    /// This computer keeps it.
    ThisComputer,
}

/// One setting of a shared part that fleet sync leaves on each machine.
#[derive(Clone, Copy, Debug)]
pub struct Local {
    /// The field a save names, or the setting's name when no save names it.
    pub field: &'static str,
    /// The setting's name in a refusal.
    pub label: &'static str,
    /// [`Scope::Machine`] or [`Scope::ThisComputer`].
    pub scope: Scope,
    /// The line the setting shows while every machine is in scope.
    pub note: &'static str,
}

/// One Settings part.
#[derive(Clone, Copy, Debug)]
pub struct Part {
    pub id: &'static str,
    /// The Settings page that shows the part.
    pub page: &'static str,
    pub kept: Kept,
    /// The settings of a shared part that stay on one machine.
    pub machine_local: &'static [Local],
    /// The line a shared part shows while every machine is in scope.
    pub note: &'static str,
    /// The operation that saves the part, when one takes `scope`.
    pub save: Option<&'static str>,
}

/// The record classes fleet sync carries that no Settings part shows:
/// projects and the project each thread belongs to.
pub const NOT_SETTINGS: [&str; 2] = ["project", "project-thread"];

const TOOLS_NOTE: &str =
    "What you add reaches every machine. On or off changes only this computer.";
const SEARCH_KEYS_NOTE: &str = "Keys stay on the machine where you add them.";
const TOOLS_LOCAL: [Local; 1] = [Local {
    field: "enabled",
    label: "On or off",
    scope: Scope::ThisComputer,
    note: "",
}];

/// Every Settings part that reads or changes a runtime's settings.
pub const PARTS: [Part; 15] = [
    Part {
        id: "models",
        page: "models",
        kept: Kept::Synced(&["route", "decision-model"]),
        machine_local: &[Local {
            field: "sign_ins",
            label: "Sign-ins",
            scope: Scope::Machine,
            note: "",
        }],
        note:
            "Sign-ins stay on the machine that holds them. Machines can share them with the others.",
        save: None,
    },
    Part {
        id: "mcp",
        page: "tools",
        kept: Kept::Synced(&["mcp"]),
        machine_local: &TOOLS_LOCAL,
        note: TOOLS_NOTE,
        save: None,
    },
    Part {
        id: "skill",
        page: "tools",
        kept: Kept::Synced(&["extension"]),
        machine_local: &TOOLS_LOCAL,
        note: TOOLS_NOTE,
        save: None,
    },
    Part {
        id: "plugin",
        page: "tools",
        kept: Kept::Synced(&["extension"]),
        machine_local: &TOOLS_LOCAL,
        note: TOOLS_NOTE,
        save: None,
    },
    Part {
        id: "search",
        page: "tools",
        kept: Kept::Synced(&["search"]),
        machine_local: &[Local {
            field: "keys",
            label: "Provider keys",
            scope: Scope::Machine,
            note: SEARCH_KEYS_NOTE,
        }],
        note: SEARCH_KEYS_NOTE,
        save: None,
    },
    Part {
        id: "permissions",
        page: "permissions",
        kept: Kept::Synced(&["permission"]),
        machine_local: &[Local {
            field: "bypass_enabled",
            label: "Allow bypass",
            scope: Scope::Machine,
            note: "Each machine keeps its own. Pick one machine to change it.",
        }],
        note: "",
        save: Some("permission.settings_save"),
    },
    Part {
        id: "profile",
        page: "account",
        kept: Kept::Synced(&["memory"]),
        machine_local: &[],
        note: "",
        save: None,
    },
    Part {
        id: "memory",
        page: "data",
        kept: Kept::Synced(&["memory"]),
        machine_local: &[],
        note: "",
        save: None,
    },
    Part {
        id: "context",
        page: "data",
        kept: Kept::Machine,
        machine_local: &[],
        note: "",
        save: None,
    },
    Part {
        id: "retention",
        page: "data",
        kept: Kept::Synced(&["retention"]),
        machine_local: &[],
        note: "",
        save: None,
    },
    Part {
        id: "storage",
        page: "data",
        kept: Kept::ThisComputer,
        machine_local: &[],
        note: "",
        save: None,
    },
    Part {
        id: "companies",
        page: "data",
        kept: Kept::ThisComputer,
        machine_local: &[],
        note: "",
        save: None,
    },
    Part {
        id: "appearance",
        page: "general",
        kept: Kept::Synced(&["preference"]),
        machine_local: &[],
        note: "",
        save: None,
    },
    Part {
        id: "time",
        page: "general",
        kept: Kept::ThisComputer,
        machine_local: &[],
        note: "",
        save: None,
    },
    Part {
        id: "notifications",
        page: "general",
        kept: Kept::Synced(&["preference"]),
        machine_local: &[],
        note: "",
        save: None,
    },
];

impl Part {
    /// Where the part keeps its values: shared exactly when sync carries it.
    pub fn scope(&self) -> Scope {
        match self.kept {
            Kept::Synced(_) => Scope::Fleet,
            Kept::Machine => Scope::Machine,
            Kept::ThisComputer => Scope::ThisComputer,
        }
    }

    /// The record classes fleet sync carries the part in.
    pub fn classes(&self) -> &'static [&'static str] {
        match self.kept {
            Kept::Synced(classes) => classes,
            Kept::Machine | Kept::ThisComputer => &[],
        }
    }

    /// The line the part shows while every machine is in scope.
    pub fn all_note(&self) -> &'static str {
        match self.kept {
            Kept::Synced(_) => self.note,
            Kept::Machine => "This part changes only {machine}. Each machine keeps its own.",
            Kept::ThisComputer => {
                "This part changes only this computer. Each machine keeps its own."
            }
        }
    }

    /// The line the part shows while another machine is picked. A part this
    /// computer keeps, in whole or in part, says that the picked machine
    /// keeps its own.
    pub fn one_note(&self) -> &'static str {
        match self.kept {
            Kept::ThisComputer => {
                "These settings belong to this computer. {machine} keeps its own."
            }
            Kept::Synced(_)
                if self
                    .machine_local
                    .iter()
                    .any(|local| local.scope == Scope::ThisComputer) =>
            {
                self.note
            }
            Kept::Synced(_) | Kept::Machine => "",
        }
    }

    fn view(&self) -> Value {
        json!({
            "id": self.id,
            "page": self.page,
            "scope": self.scope(),
            "classes": self.classes(),
            "machine_local": self.machine_local.iter().map(|local| json!({
                "field": local.field,
                "scope": local.scope,
                "note": local.note,
            })).collect::<Vec<_>>(),
            "notes": {"all": self.all_note(), "one": self.one_note()},
        })
    }
}

/// The part `id` names.
pub fn part(id: &str) -> Option<&'static Part> {
    PARTS.iter().find(|part| part.id == id)
}

/// The `settings.scope` answer.
pub fn view() -> Value {
    json!({"parts": PARTS.iter().map(Part::view).collect::<Vec<_>>()})
}

/// Refuses `body`, a save of `operation`, when it names `scope: "all"` and
/// changes a setting that stays on one machine.
pub fn check_save(operation: &str, body: &Value) -> Result<(), String> {
    if body.get("scope").and_then(Value::as_str) != Some("all") {
        return Ok(());
    }
    let held = PARTS
        .iter()
        .filter(|part| part.save == Some(operation))
        .flat_map(|part| part.machine_local)
        .find(|local| body.get(local.field).is_some());
    match held {
        Some(local) if local.scope == Scope::ThisComputer => Err(format!(
            "{} stays on this computer. Pick one machine to change it.",
            local.label
        )),
        Some(local) => Err(format!(
            "{} stays on each machine. Pick one machine to change it.",
            local.label
        )),
        None => Ok(()),
    }
}

/// Answers `settings.scope`.
pub fn operation(body: Value) -> Result<Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Empty {}
    serde_json::from_value::<Empty>(body).map_err(|_| "The request is invalid.".to_string())?;
    Ok(view())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    /// Every record class fleet sync carries for settings, projects and
    /// memory, and the extension inventory classes.
    fn synced() -> BTreeSet<&'static str> {
        crate::fleet_sync::CLASSES
            .into_iter()
            .chain(crate::swarm::EXTENSION_CLASSES)
            .collect()
    }

    #[test]
    fn every_synced_class_has_a_part_and_every_part_class_is_synced() {
        let synced = synced();
        let claimed: BTreeSet<&str> = PARTS.iter().flat_map(Part::classes).copied().collect();
        for class in &claimed {
            assert!(synced.contains(class), "no fleet sync class {class}");
            assert!(!NOT_SETTINGS.contains(class), "{class} is not a setting");
        }
        for class in &synced {
            assert!(
                claimed.contains(class) || NOT_SETTINGS.contains(class),
                "the synced class {class} has no Settings part"
            );
        }
    }

    #[test]
    fn each_part_says_where_it_stays() {
        let mut ids = BTreeSet::new();
        for part in &PARTS {
            assert!(ids.insert(part.id), "{} is listed twice", part.id);
            let shared = part.scope() == Scope::Fleet;
            assert_eq!(shared, !part.classes().is_empty(), "{}", part.id);
            // Only a shared part holds settings that stay on one machine,
            // and only those settings give a shared part a line.
            if !shared {
                assert!(part.machine_local.is_empty(), "{}", part.id);
                assert!(!part.all_note().is_empty(), "{}", part.id);
            }
            if shared && !part.note.is_empty() {
                assert!(!part.machine_local.is_empty(), "{}", part.id);
            }
            for local in part.machine_local {
                assert_ne!(local.scope, Scope::Fleet, "{}", local.field);
            }
        }
    }

    #[test]
    fn the_answer_names_each_parts_scope_and_lines() {
        let answer = operation(json!({})).unwrap();
        let parts = answer["parts"].as_array().unwrap();
        assert_eq!(parts.len(), PARTS.len());
        let find = |id: &str| parts.iter().find(|part| part["id"] == id).unwrap();
        assert_eq!(find("permissions")["scope"], "fleet");
        assert_eq!(find("permissions")["classes"], json!(["permission"]));
        assert_eq!(
            find("permissions")["machine_local"][0]["field"],
            "bypass_enabled"
        );
        assert_eq!(find("context")["scope"], "machine");
        assert_eq!(
            find("context")["notes"]["all"],
            "This part changes only {machine}. Each machine keeps its own."
        );
        assert_eq!(find("context")["notes"]["one"], "");
        assert_eq!(find("storage")["scope"], "this_computer");
        assert_eq!(
            find("storage")["notes"]["one"],
            "These settings belong to this computer. {machine} keeps its own."
        );
        // On or off stays on this computer, so the line holds for one machine too.
        assert_eq!(find("mcp")["notes"]["one"], TOOLS_NOTE);
        assert_eq!(find("models")["notes"]["one"], "");
        // Search keys stay on each machine, which the all machines line says.
        assert_eq!(
            find("search")["notes"],
            json!({"all": SEARCH_KEYS_NOTE, "one": ""})
        );
        assert_eq!(find("search")["machine_local"][0]["note"], SEARCH_KEYS_NOTE);
        assert_eq!(find("retention")["notes"], json!({"all": "", "one": ""}));
        assert!(operation(json!({"x": 1})).is_err());
    }

    #[test]
    fn a_save_to_every_machine_refuses_a_setting_that_stays_on_one() {
        let save = "permission.settings_save";
        assert!(check_save(save, &json!({"scope": "all", "default_agent_mode": "plan"})).is_ok());
        assert!(check_save(save, &json!({"scope": "machine", "bypass_enabled": false})).is_ok());
        assert!(check_save(save, &json!({"bypass_enabled": false})).is_ok());
        assert_eq!(
            check_save(save, &json!({"scope": "all", "bypass_enabled": false})).unwrap_err(),
            "Allow bypass stays on each machine. Pick one machine to change it."
        );
        // Every setting a saved part keeps on one machine is refused.
        for part in PARTS.iter().filter(|part| part.save.is_some()) {
            for local in part.machine_local {
                let mut body = json!({"scope": "all"});
                body[local.field] = json!(null);
                assert!(
                    check_save(part.save.unwrap(), &body).is_err(),
                    "{}",
                    local.field
                );
            }
        }
    }
}
