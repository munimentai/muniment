//! Artifacts: the manifest of `<Home>/artifacts` is one record per file,
//! keyed by its path there, with `{hash, owner, size}`, last writer wins per
//! path. The bytes go into the content store on the member that wrote them
//! and reach another member only when it needs them: a pinned artifact, or
//! every artifact when `artifacts` is `all` on a machine that prefetches,
//! is written into that member's `artifacts` folder. Any other artifact is
//! read through `blob.read`, which fetches it.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use rusqlite::params;
use serde_json::json;

use super::store::sql;
use super::{pins, Inner};

pub(super) const CLASS: &str = "artifact";
/// The largest artifact file that travels.
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 240;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS fleet_artifacts(
  path TEXT PRIMARY KEY, hash TEXT NOT NULL, size INTEGER NOT NULL, mtime INTEGER NOT NULL,
  replica INTEGER NOT NULL);
";

pub(super) fn initialize(inner: &Inner) -> Result<(), String> {
    inner.store.connect()?.execute_batch(SCHEMA).map_err(sql)
}

/// What this runtime last knew of one local file: its hash at a size and
/// modification time, and whether sync wrote it from another member.
#[derive(Clone, Debug)]
struct Row {
    hash: String,
    size: u64,
    mtime: i64,
    replica: bool,
}

#[derive(Clone, Debug)]
struct Entry {
    hash: String,
    owner: String,
    size: u64,
}

fn rows(inner: &Inner) -> Result<BTreeMap<String, Row>, String> {
    let connection = inner.store.connect()?;
    let mut statement = connection
        .prepare("SELECT path, hash, size, mtime, replica FROM fleet_artifacts")
        .map_err(sql)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                Row {
                    hash: row.get(1)?,
                    size: row.get::<_, i64>(2)? as u64,
                    mtime: row.get(3)?,
                    replica: row.get::<_, i64>(4)? != 0,
                },
            ))
        })
        .map_err(sql)?;
    rows.collect::<Result<_, _>>().map_err(sql)
}

fn save_row(inner: &Inner, path: &str, row: &Row) -> Result<(), String> {
    inner
        .store
        .connect()?
        .execute(
            "INSERT OR REPLACE INTO fleet_artifacts(path, hash, size, mtime, replica) VALUES(?1, ?2, ?3, ?4, ?5)",
            params![path, row.hash, row.size as i64, row.mtime, row.replica as i64],
        )
        .map(drop)
        .map_err(sql)
}

fn forget_row(inner: &Inner, path: &str) -> Result<(), String> {
    inner
        .store
        .connect()?
        .execute("DELETE FROM fleet_artifacts WHERE path=?1", [path])
        .map(drop)
        .map_err(sql)
}

/// Whether a manifest path names a file under the artifacts folder.
fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_PATH_BYTES
        && !path.contains('\\')
        && Path::new(path).components().all(|component| {
            matches!(component, Component::Normal(part)
                if !part.to_string_lossy().starts_with('.'))
        })
}

fn modified_millis(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_millis() as i64)
}

/// Every regular file under `root` by its manifest path, with its size and
/// modification time. Hidden files, links and large files stay out.
fn walk(root: &Path) -> BTreeMap<String, (u64, i64)> {
    let mut files = BTreeMap::new();
    let mut pending = vec![(root.to_path_buf(), String::new())];
    while let Some((directory, prefix)) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let path = format!("{prefix}{name}");
            let Ok(metadata) = std::fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if metadata.is_dir() {
                pending.push((entry.path(), format!("{path}/")));
            } else if metadata.is_file() && metadata.len() <= MAX_FILE_BYTES && valid_path(&path) {
                files.insert(path, (metadata.len(), modified_millis(&metadata)));
            }
        }
    }
    files
}

fn entries(inner: &Inner) -> Result<BTreeMap<String, Entry>, String> {
    let mut entries = BTreeMap::new();
    for record in super::accounts::all(inner, CLASS)? {
        let (Some(hash), Some(owner)) = (
            record.value("hash").and_then(|value| value.as_str()),
            record.value("owner").and_then(|value| value.as_str()),
        ) else {
            continue;
        };
        if !valid_path(&record.key) || hash.parse::<crate::cas::ContentHash>().is_err() {
            continue;
        }
        entries.insert(
            record.key.clone(),
            Entry {
                hash: hash.to_owned(),
                owner: owner.to_owned(),
                size: record
                    .value("size")
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0),
            },
        );
    }
    Ok(entries)
}

fn root(inner: &Inner) -> Option<PathBuf> {
    crate::memory_files::home(&inner.profile)
        .ok()
        .map(|home| home.join("artifacts"))
}

/// Writes `bytes` at `path` under `root` through a temporary file.
fn write_file(root: &Path, path: &str, bytes: &[u8]) -> Result<Row, String> {
    let target = root.join(path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temporary = target.with_file_name(format!(".fleet-{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&temporary, bytes)
        .and_then(|()| crate::atomic_file::replace(&temporary, &target))
        .map_err(|error| {
            let _ = std::fs::remove_file(&temporary);
            error.to_string()
        })?;
    let metadata = std::fs::metadata(&target).map_err(|error| error.to_string())?;
    Ok(Row {
        hash: String::new(),
        size: metadata.len(),
        mtime: modified_millis(&metadata),
        replica: true,
    })
}

/// One pass: local files to the manifest, then the manifest to local files.
/// Answers whether it wrote or removed a local file.
pub(super) fn sync(inner: &Inner) -> Result<bool, String> {
    let origin = inner.origin()?;
    let Some(root) = root(inner) else {
        return Ok(false);
    };
    let name = inner.runtime_name().unwrap_or_default();
    let mut rows = rows(inner)?;
    let files = walk(&root);
    let mut manifest = entries(inner)?;
    let mut touched = false;

    // Local files to the manifest.
    for (path, (size, mtime)) in &files {
        let row = rows.get(path).cloned();
        let hash = match &row {
            Some(row) if row.size == *size && row.mtime == *mtime => row.hash.clone(),
            _ => {
                let Ok(bytes) = std::fs::read(root.join(path)) else {
                    continue;
                };
                pins::put(inner, &bytes)?
            }
        };
        let changed = row.as_ref().is_none_or(|row| row.hash != hash);
        let current = Row {
            hash: hash.clone(),
            size: *size,
            mtime: *mtime,
            replica: row.as_ref().is_some_and(|row| row.replica) && !changed,
        };
        if !changed {
            if row.as_ref().is_some_and(|row| row.mtime != *mtime) {
                save_row(inner, path, &current)?;
                rows.insert(path.clone(), current);
            }
            continue;
        }
        // A new or edited file is this machine's write.
        for (field, value) in [
            ("hash", json!(hash)),
            ("owner", json!(origin)),
            ("owner_name", json!(name)),
            ("size", json!(size)),
        ] {
            inner
                .records
                .put(&origin, inner.now(), CLASS, path, field, value, 0)?;
        }
        pins::note(inner, &hash, CLASS, path, *size, true)?;
        save_row(inner, path, &current)?;
        rows.insert(path.clone(), current);
        manifest.insert(
            path.clone(),
            Entry {
                hash,
                owner: origin.clone(),
                size: *size,
            },
        );
        inner.nudge();
    }
    // A file of this machine's that is gone leaves the manifest; a removed
    // copy of another member's file only stops being kept here.
    for (path, row) in rows.clone() {
        if files.contains_key(&path) {
            continue;
        }
        if !row.replica
            && manifest
                .get(&path)
                .is_some_and(|entry| entry.owner == origin && entry.hash == row.hash)
        {
            inner
                .records
                .delete(&origin, inner.now(), CLASS, &path, 0, None)?;
            manifest.remove(&path);
            inner.nudge();
        }
        forget_row(inner, &path)?;
        rows.remove(&path);
    }

    // The manifest to local files.
    let settings = pins::settings(inner);
    for (path, entry) in &manifest {
        if entry.owner == origin {
            continue;
        }
        pins::note(inner, &entry.hash, CLASS, path, entry.size, false)?;
        let row = rows.get(path);
        let present = files.contains_key(path);
        // A local file edited since sync last saw it is a pending local write.
        if present && row.is_none() {
            continue;
        }
        if row.is_some_and(|row| row.hash == entry.hash) {
            if row.is_some_and(|row| row.replica) {
                let wanted = pins::pinned(inner, CLASS, path)?
                    || (settings.artifacts == "all" && settings.prefetch);
                if !wanted && std::fs::remove_file(root.join(path)).is_ok() {
                    forget_row(inner, path)?;
                    touched = true;
                }
            }
            continue;
        }
        let wanted =
            pins::pinned(inner, CLASS, path)? || (settings.artifacts == "all" && settings.prefetch);
        if !wanted {
            // A copy of an older version goes; the new one comes on read.
            if present && row.is_some() {
                let _ = std::fs::remove_file(root.join(path));
                forget_row(inner, path)?;
                touched = true;
            }
            continue;
        }
        if !pins::want(inner, &entry.hash, entry.size)? {
            continue;
        }
        let Some(bytes) = pins::bytes(inner, &entry.hash)? else {
            continue;
        };
        let mut written = write_file(&root, path, &bytes)?;
        written.hash = entry.hash.clone();
        save_row(inner, path, &written)?;
        touched = true;
    }
    // A deleted record takes its unchanged copy with it.
    for (path, row) in rows {
        if row.replica && !manifest.contains_key(&path) {
            let local = std::fs::metadata(root.join(&path)).ok();
            if local.is_some_and(|metadata| {
                metadata.len() == row.size && modified_millis(&metadata) == row.mtime
            }) {
                let _ = std::fs::remove_file(root.join(&path));
                touched = true;
            }
            forget_row(inner, &path)?;
        }
    }
    Ok(touched)
}
