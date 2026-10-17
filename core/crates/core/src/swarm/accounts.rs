//! Account metadata: one record per model connection a member holds, keyed
//! `<holder>/<name>` with `name` as `pi:<provider>` or `router:<id>`. Only
//! the holder writes its records, and a reader takes a record only when the
//! holder wrote it. The secret itself travels sealed in `models:<holder>`.
//! Every member lists every account with its holder's machine name.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use super::{Inner, MemberRecord};
use crate::shared_models;

pub(super) const CLASS: &str = "account";
const FIELD: &str = "account";
const PAGE: usize = 256;

fn member_name(inner: &Inner, runtime_id: &str) -> Option<String> {
    inner
        .store
        .record::<MemberRecord>("members", "runtime_id", "record", runtime_id)
        .ok()
        .flatten()
        .map(|record| record.runtime_name)
        .filter(|name| !name.is_empty())
}

/// Every live record of `class`, page by page.
pub(super) fn all(inner: &Inner, class: &str) -> Result<Vec<super::Record>, String> {
    let mut records = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let page = inner.records.scan(class, after.as_deref(), PAGE)?;
        let done = page.len() < PAGE;
        after = page.last().map(|record| record.key.clone());
        records.extend(page.into_iter().filter(super::Record::is_live));
        if done {
            return Ok(records);
        }
    }
}

/// Writes this runtime's account records from its own connections and
/// deletes the records of connections it no longer holds.
pub(super) fn publish(inner: &Inner, sharing: bool) -> Result<(), String> {
    let origin = inner.origin()?;
    let agent = crate::state_root::agent_directory(&inner.profile);
    let name = inner
        .runtime_name()
        .or_else(|| member_name(inner, &origin))
        .unwrap_or_default();
    let mut own: BTreeMap<String, Value> = BTreeMap::new();
    for mut account in shared_models::own_accounts(&agent)? {
        let Some(id) = account["name"].as_str().map(str::to_owned) else {
            continue;
        };
        account["holder"] = origin.clone().into();
        account["holder_name"] = name.clone().into();
        account["shared"] = sharing.into();
        own.insert(format!("{origin}/{id}"), account);
    }
    let prefix = format!("{origin}/");
    let held: BTreeSet<String> = all(inner, CLASS)?
        .into_iter()
        .map(|record| record.key)
        .filter(|key| key.starts_with(&prefix))
        .collect();
    let mut changed = false;
    for (key, account) in &own {
        let change =
            inner
                .records
                .put(&origin, inner.now(), CLASS, key, FIELD, account.clone(), 0)?;
        changed |= change.is_some();
    }
    for key in held.difference(&own.keys().cloned().collect()) {
        changed |= inner
            .records
            .delete(&origin, inner.now(), CLASS, key, 0, None)?
            .is_some();
    }
    if changed {
        inner.nudge();
    }
    Ok(())
}

/// Every account in the fleet, as its holder wrote it, with the holder's
/// current machine name and whether this runtime can use it.
pub(super) fn list(inner: &Inner) -> Result<Vec<Value>, String> {
    let self_id = inner.origin()?;
    let mut accounts = Vec::new();
    for record in all(inner, CLASS)? {
        let Some((holder, name)) = record.key.split_once('/') else {
            continue;
        };
        let Some(field) = record.fields.get(FIELD) else {
            continue;
        };
        // A record another member wrote about this holder is not the
        // holder's word.
        if field.origin != holder || field.value["name"] != name {
            continue;
        }
        let mut account = field.value.clone();
        if let Some(current) = member_name(inner, holder) {
            account["holder_name"] = current.into();
        }
        let usable = holder == self_id
            || super::credentials::opened_keys(inner, &format!("models:{holder}"))?
                .iter()
                .any(|key| key == name);
        account["this_machine"] = (holder == self_id).into();
        account["usable_here"] = usable.into();
        accounts.push(account);
    }
    accounts.sort_by(|a, b| {
        (a["holder_name"].as_str(), a["name"].as_str())
            .cmp(&(b["holder_name"].as_str(), b["name"].as_str()))
    });
    Ok(accounts)
}

/// The account records as one JSON array, for `fleet.credentials`.
pub(super) fn view(inner: &Inner) -> Value {
    list(inner).map_or_else(|_| json!([]), Value::from)
}
