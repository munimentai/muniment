//! Extension inventory items on records: MCP servers in class `mcp`, skills
//! and plugins in class `extension`, each keyed by its item id, last writer
//! wins. A package travels as a content-store object named by its hash, and
//! a member fetches it once the item is enabled or pinned there, checks the
//! hash and unpacks it under `extensions/packages`.
//!
//! - **Baseline.** Per item, this runtime keeps the hash of the synced
//!   fields as it last matched them locally and on the record, so a local
//!   edit goes to the record and a record change comes to the inventory.
//! - **Enabled.** An item arrives turned on or off as the member that saved
//!   it chose. A toggle here after that is this machine's own.
//! - **Availability.** A server whose command this machine lacks, and a
//!   package that has not arrived, show why and stay out of runs.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use rusqlite::{params, OptionalExtension};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::store::sql;
use super::{pins, Inner};

pub(super) const MCP_CLASS: &str = "mcp";
/// Skills and plugins; pins name their packages by this class too.
pub(super) const PACKAGE_CLASS: &str = "extension";
const MCP_FIELDS: [&str; 5] = ["kind", "name", "description", "source", "definition"];
const PACKAGE_FIELDS: [&str; 12] = [
    "kind",
    "name",
    "description",
    "source",
    "version",
    "skills",
    "extensions",
    "servers",
    "digest",
    "dependencies",
    "package",
    "size",
];
/// The largest package that travels.
const MAX_PACKAGE_BYTES: u64 = 64 * 1024 * 1024;
/// Folders a package leaves behind: a checkout's history and installed
/// dependencies, which belong to the machine that made them.
const SKIPPED: [&str; 3] = [".git", "node_modules", ".muniment-preview.json"];

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS fleet_items(
  class TEXT NOT NULL, id TEXT NOT NULL, local TEXT, remote TEXT, enabled INTEGER,
  PRIMARY KEY(class, id));
CREATE TABLE IF NOT EXISTS fleet_packages(
  id TEXT PRIMARY KEY, digest TEXT NOT NULL, hash TEXT NOT NULL, size INTEGER NOT NULL);
";

pub(super) fn initialize(inner: &Inner) -> Result<(), String> {
    inner.store.connect()?.execute_batch(SCHEMA).map_err(sql)
}

fn class_of(item: &Value) -> Option<&'static str> {
    match item["kind"].as_str()? {
        "mcp" => Some(MCP_CLASS),
        "skill" | "plugin" => Some(PACKAGE_CLASS),
        _ => None,
    }
}

fn fields_of(class: &str) -> &'static [&'static str] {
    if class == MCP_CLASS {
        &MCP_FIELDS
    } else {
        &PACKAGE_FIELDS
    }
}

fn digest(value: &Value) -> String {
    hex::encode(&Sha256::digest(value.to_string().as_bytes())[..16])
}

#[derive(Clone, Debug, Default)]
struct Base {
    local: Option<String>,
    remote: Option<String>,
    /// The `enabled` value sync last wrote here, `None` for a local item.
    enabled: Option<bool>,
}

fn baseline(inner: &Inner, class: &str) -> Result<BTreeMap<String, Base>, String> {
    let connection = inner.store.connect()?;
    let mut statement = connection
        .prepare("SELECT id, local, remote, enabled FROM fleet_items WHERE class=?1")
        .map_err(sql)?;
    let rows = statement
        .query_map([class], |row| {
            Ok((
                row.get::<_, String>(0)?,
                Base {
                    local: row.get(1)?,
                    remote: row.get(2)?,
                    enabled: row.get::<_, Option<i64>>(3)?.map(|enabled| enabled != 0),
                },
            ))
        })
        .map_err(sql)?;
    rows.collect::<Result<_, _>>().map_err(sql)
}

fn save_base(inner: &Inner, class: &str, id: &str, base: Option<&Base>) -> Result<(), String> {
    let connection = inner.store.connect()?;
    match base {
        Some(base) => connection.execute(
            "INSERT OR REPLACE INTO fleet_items(class, id, local, remote, enabled) VALUES(?1, ?2, ?3, ?4, ?5)",
            params![class, id, base.local, base.remote, base.enabled.map(i64::from)],
        ),
        None => connection.execute(
            "DELETE FROM fleet_items WHERE class=?1 AND id=?2",
            params![class, id],
        ),
    }
    .map(drop)
    .map_err(sql)
}

/// Packs a package folder as a tar archive with sorted entries and no
/// times, owners or links, so one folder always packs to one hash.
pub(super) fn pack(base: &Path) -> Result<Vec<u8>, String> {
    let mut files = Vec::new();
    let mut pending = vec![(base.to_path_buf(), String::new())];
    while let Some((directory, prefix)) = pending.pop() {
        let entries = std::fs::read_dir(&directory).map_err(|error| error.to_string())?;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if SKIPPED.contains(&name.as_str()) {
                continue;
            }
            let metadata = std::fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
            let path = format!("{prefix}{name}");
            if metadata.is_dir() {
                pending.push((entry.path(), format!("{path}/")));
            } else if metadata.is_file() {
                #[cfg(unix)]
                let executable = {
                    use std::os::unix::fs::PermissionsExt;
                    metadata.permissions().mode() & 0o111 != 0
                };
                #[cfg(not(unix))]
                let executable = false;
                files.push((path, entry.path(), executable));
            }
        }
    }
    files.sort();
    let mut builder = tar::Builder::new(Vec::new());
    let mut total = 0u64;
    for (path, source, executable) in files {
        let bytes = std::fs::read(&source).map_err(|error| error.to_string())?;
        total += bytes.len() as u64;
        if total > MAX_PACKAGE_BYTES {
            return Err("The package is too large to share.".into());
        }
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(if executable { 0o755 } else { 0o644 });
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        header.set_entry_type(tar::EntryType::Regular);
        builder
            .append_data(&mut header, &path, bytes.as_slice())
            .map_err(|error| error.to_string())?;
    }
    builder.into_inner().map_err(|error| error.to_string())
}

/// Unpacks the package `bytes` under `packages` after checking them against
/// `hash`, and answers its folder. A package that does not match its hash,
/// or holds a path outside its folder or anything but files, installs
/// nothing.
pub(super) fn install(packages: &Path, hash: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    if hex::encode(Sha256::digest(bytes)) != hash {
        return Err("The package does not match its hash.".into());
    }
    let folder = packages.join(format!("fleet-{}", &hash[..16]));
    if folder.is_dir() {
        return Ok(folder);
    }
    std::fs::create_dir_all(packages).map_err(|error| error.to_string())?;
    let staging = packages.join(format!(".fleet-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<(), String> {
        let mut archive = tar::Archive::new(bytes);
        for entry in archive.entries().map_err(|error| error.to_string())? {
            let mut entry = entry.map_err(|error| error.to_string())?;
            if entry.header().entry_type() != tar::EntryType::Regular {
                return Err("The package holds something other than files.".into());
            }
            let path = entry
                .path()
                .map_err(|error| error.to_string())?
                .into_owned();
            if path.as_os_str().is_empty()
                || !path
                    .components()
                    .all(|component| matches!(component, Component::Normal(_)))
            {
                return Err("The package holds a path outside its folder.".into());
            }
            let target = staging.join(&path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            let mut contents = Vec::new();
            entry
                .read_to_end(&mut contents)
                .map_err(|error| error.to_string())?;
            std::fs::write(&target, &contents).map_err(|error| error.to_string())?;
            #[cfg(unix)]
            if entry.header().mode().is_ok_and(|mode| mode & 0o111 != 0) {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))
                    .map_err(|error| error.to_string())?;
            }
        }
        std::fs::create_dir_all(&staging).map_err(|error| error.to_string())?;
        std::fs::rename(&staging, &folder).map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result.map(|()| folder)
}

/// The package hash and size of a skill or plugin this machine installed,
/// packing its folder once per digest.
fn own_package(inner: &Inner, item: &Value) -> Result<Option<(String, u64)>, String> {
    let (Some(id), Some(base)) = (item["id"].as_str(), item["base"].as_str()) else {
        return Ok(None);
    };
    let digest = item["digest"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| item["version"].to_string());
    let cached: Option<(String, i64)> = inner
        .store
        .connect()?
        .query_row(
            "SELECT hash, size FROM fleet_packages WHERE id=?1 AND digest=?2",
            params![id, digest],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(sql)?;
    if let Some((hash, size)) = cached {
        if pins::held(inner, &hash)? {
            return Ok(Some((hash, size as u64)));
        }
    }
    if !Path::new(base).is_dir() {
        return Ok(None);
    }
    let bytes = match pack(Path::new(base)) {
        Ok(bytes) => bytes,
        Err(error) => {
            crate::runtime_eprintln!("muniment-runtime: extension {id} stays here: {error}");
            return Ok(None);
        }
    };
    let hash = pins::put(inner, &bytes)?;
    let size = bytes.len() as u64;
    pins::note(inner, &hash, PACKAGE_CLASS, id, size, true)?;
    inner
        .store
        .connect()?
        .execute(
            "INSERT OR REPLACE INTO fleet_packages(id, digest, hash, size) VALUES(?1, ?2, ?3, ?4)",
            params![id, digest, hash, size as i64],
        )
        .map_err(sql)?;
    Ok(Some((hash, size)))
}

/// The fields of a local item that sync: those the class names. A skill or
/// plugin made here gets its package; one that came from another member
/// keeps the package its record named.
fn synced(inner: &Inner, class: &str, item: &Value) -> Result<Map<String, Value>, String> {
    let mut fields = Map::new();
    for field in fields_of(class) {
        if let Some(value) = item.get(*field).filter(|value| !value.is_null()) {
            fields.insert((*field).to_owned(), value.clone());
        }
    }
    if class == PACKAGE_CLASS && item.get("fleet").is_none() {
        fields.remove("package");
        fields.remove("size");
        if let Some((hash, size)) = own_package(inner, item)? {
            fields.insert("package".into(), hash.into());
            fields.insert("size".into(), size.into());
        }
    }
    Ok(fields)
}

fn record_fields(class: &str, record: &super::Record) -> Map<String, Value> {
    fields_of(class)
        .iter()
        .filter_map(|field| Some(((*field).to_owned(), record.value(field)?.clone())))
        .collect()
}

/// One pass of both classes: local edits to the records, record changes to
/// the inventory, then the packages enabled items still lack. Answers
/// whether the inventory changed.
pub(super) fn sync(inner: &Inner) -> Result<bool, String> {
    let origin = inner.origin()?;
    let name = inner.runtime_name().unwrap_or_default();
    let mut changed = false;
    for class in [MCP_CLASS, PACKAGE_CLASS] {
        changed |= sync_class(inner, class, &origin, &name)?;
    }
    changed |= install_pending(inner)?;
    Ok(changed)
}

fn sync_class(inner: &Inner, class: &str, origin: &str, name: &str) -> Result<bool, String> {
    let profile = &inner.profile;
    let state = crate::extend::inventory(profile)?;
    let mut local: BTreeMap<String, Value> = BTreeMap::new();
    for item in state["items"].as_array().into_iter().flatten() {
        if let (Some(id), Some(item_class)) = (item["id"].as_str(), class_of(item)) {
            if item_class == class {
                local.insert(id.to_owned(), item.clone());
            }
        }
    }
    let mut base = baseline(inner, class)?;
    let load = || -> Result<BTreeMap<String, super::Record>, String> {
        Ok(super::accounts::all(inner, class)?
            .into_iter()
            .map(|record| (record.key.clone(), record))
            .collect())
    };
    let records = load()?;

    // Local edits go to the records.
    for (id, item) in &local {
        let fields = synced(inner, class, item)?;
        let hash = digest(&Value::Object(fields.clone()));
        let held = base.entry(id.clone()).or_default();
        if held.local.as_deref() == Some(hash.as_str()) {
            continue;
        }
        let remote = records
            .get(id)
            .map(|record| digest(&Value::Object(record_fields(class, record))));
        if remote.as_deref() != Some(hash.as_str()) {
            let mut values = fields;
            values.insert("enabled".into(), json!(item["enabled"] != false));
            values.insert("saved_by".into(), json!(origin));
            values.insert("saved_by_name".into(), json!(name));
            for (field, value) in values {
                inner
                    .records
                    .put(origin, inner.now(), class, id, &field, value, 0)?;
            }
            inner.nudge();
        }
        held.local = Some(hash.clone());
        held.remote = Some(hash);
        save_base(inner, class, id, Some(held))?;
    }
    for (id, held) in base.clone() {
        if held.local.is_some() && !local.contains_key(&id) {
            if records.contains_key(&id) {
                inner
                    .records
                    .delete(origin, inner.now(), class, &id, 0, None)?;
                inner.nudge();
            }
            save_base(inner, class, &id, None)?;
            base.remove(&id);
        }
    }

    // Record changes come to the inventory, with the writes above in place.
    let records = load()?;
    let mut updates: Vec<(String, Option<Value>, Base)> = Vec::new();
    for (id, record) in &records {
        if !record.is_live() || id.is_empty() || id.len() > 128 {
            continue;
        }
        let fields = record_fields(class, record);
        let remote = digest(&Value::Object(fields.clone()));
        let held = base.get(id).cloned().unwrap_or_default();
        if held.remote.as_deref() == Some(remote.as_str()) {
            continue;
        }
        let existing = local.get(id);
        if existing.is_some_and(|item| {
            synced(inner, class, item).is_ok_and(|fields| digest(&Value::Object(fields)) == remote)
        }) {
            save_base(
                inner,
                class,
                id,
                Some(&Base {
                    local: Some(remote.clone()),
                    remote: Some(remote),
                    enabled: held.enabled,
                }),
            )?;
            continue;
        }
        let saved_by = record
            .value("saved_by")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if saved_by == origin {
            continue;
        }
        let mut item = Value::Object(fields);
        item["id"] = json!(id);
        item["fleet"] = json!({
            "origin": saved_by,
            "name": record.value("saved_by_name").cloned().unwrap_or(Value::Null),
        });
        let offered = record.value("enabled") != Some(&Value::Bool(false));
        // A toggle made here since sync last wrote the item stays.
        let overridden = existing.is_some_and(|item| {
            held.enabled
                .is_some_and(|applied| (item["enabled"] != false) != applied)
        });
        let enabled = if overridden {
            existing.is_some_and(|item| item["enabled"] != false)
        } else {
            offered
        };
        item["enabled"] = json!(enabled);
        if let Some(existing) = existing {
            for keep in ["lastCheck", "icon"] {
                if let Some(value) = existing.get(keep) {
                    item[keep] = value.clone();
                }
            }
            if existing["package"] == item["package"] {
                if let Some(base) = existing.get("base") {
                    item["base"] = base.clone();
                }
            }
        }
        if class == PACKAGE_CLASS && item.get("base").is_none() {
            item["base"] = Value::Null;
        }
        updates.push((
            id.clone(),
            Some(item),
            Base {
                local: Some(remote.clone()),
                remote: Some(remote),
                enabled: Some(enabled),
            },
        ));
    }
    // A deleted record takes the unchanged item with it.
    for (id, held) in &base {
        if held.remote.is_none() || records.get(id).is_some_and(super::Record::is_live) {
            continue;
        }
        let unchanged = local.get(id).is_none_or(|item| {
            synced(inner, class, item)
                .is_ok_and(|fields| Some(digest(&Value::Object(fields))) == held.local)
        });
        if unchanged {
            updates.push((id.clone(), None, Base::default()));
        }
    }
    if updates.is_empty() {
        return Ok(false);
    }
    crate::extend::update_inventory(profile, |state| {
        let items = state["items"]
            .as_array_mut()
            .ok_or("Extension settings are invalid.")?;
        for (id, item, _) in &updates {
            items.retain(|held| held["id"] != id.as_str());
            if let Some(item) = item {
                items.push(item.clone());
            }
        }
        Ok(true)
    })?;
    for (id, item, held) in updates {
        save_base(inner, class, &id, item.as_ref().map(|_| &held))?;
    }
    Ok(true)
}

/// Fetches and installs the packages of the skills and plugins from other
/// members that are enabled or pinned here and lack their folder.
fn install_pending(inner: &Inner) -> Result<bool, String> {
    let profile = &inner.profile;
    let state = crate::extend::inventory(profile)?;
    let packages = profile.join("extensions/packages");
    let mut installed: Vec<(String, String, PathBuf)> = Vec::new();
    for item in state["items"].as_array().into_iter().flatten() {
        if item.get("fleet").is_none() || class_of(item) != Some(PACKAGE_CLASS) {
            continue;
        }
        let (Some(id), Some(hash)) = (item["id"].as_str(), item["package"].as_str()) else {
            continue;
        };
        if item["base"]
            .as_str()
            .is_some_and(|base| Path::new(base).is_dir())
        {
            continue;
        }
        if hash.len() != 64 {
            continue;
        }
        let size = item["size"].as_u64().unwrap_or(0);
        pins::note(inner, hash, PACKAGE_CLASS, id, size, false)?;
        let wanted = item["enabled"] != false || pins::pinned(inner, PACKAGE_CLASS, id)?;
        if !wanted || !pins::want(inner, hash, size)? {
            continue;
        }
        let Some(bytes) = pins::bytes(inner, hash)? else {
            continue;
        };
        match install(&packages, hash, &bytes) {
            Ok(folder) => installed.push((id.to_owned(), hash.to_owned(), folder)),
            Err(error) => {
                crate::runtime_eprintln!("muniment-runtime: package of {id} refused: {error}")
            }
        }
    }
    if installed.is_empty() {
        return Ok(false);
    }
    crate::extend::update_inventory(profile, |state| {
        let mut changed = false;
        for item in state["items"].as_array_mut().into_iter().flatten() {
            for (id, hash, folder) in &installed {
                if item["id"] == id.as_str() && item["package"] == hash.as_str() {
                    item["base"] = json!(folder);
                    changed = true;
                }
            }
        }
        Ok(changed)
    })?;
    Ok(true)
}
