//! Fleet blobs: the bytes of artifacts and extension packages, which travel
//! through the content store's want and chunk path when a member needs them.
//!
//! - **Pins.** A pin is per machine and per class: `(class, key)`, or
//!   `(class, "*")` for every item of a class. A pinned item is fetched and
//!   kept offline.
//! - **Offline settings.** `artifacts` is `pinned` or `all`, `budget_bytes`
//!   caps the fetched bytes this machine keeps, and `prefetch` says whether
//!   it fetches before a client asks. A laptop or desktop prefetches by
//!   default; a server or container fetches only what is pinned or read.
//! - **Eviction.** Fetched blobs past the budget leave the content store,
//!   least recently used first. Pinned blobs and this machine's own stay.

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::store::sql;
use super::sync::storage_lock;
use super::{lock, Inner};
use crate::cas::ContentHash;

/// The fetched bytes a machine keeps by default.
pub const DEFAULT_BUDGET_BYTES: u64 = 5 * 1024 * 1024 * 1024;
/// The most bytes one `blob.read` answers.
const READ_CHUNK: u64 = 256 * 1024;
/// The classes a pin names.
const CLASSES: [&str; 2] = [super::artifacts::CLASS, super::extensions::PACKAGE_CLASS];
const SETTINGS_KEY: &str = "fleet_offline";

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS fleet_blobs(
  hash TEXT PRIMARY KEY, class TEXT NOT NULL, key TEXT NOT NULL, size INTEGER NOT NULL,
  used_at INTEGER NOT NULL, own INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS fleet_pins(class TEXT NOT NULL, key TEXT NOT NULL, PRIMARY KEY(class, key));
";

pub(super) fn initialize(inner: &Inner) -> Result<(), String> {
    inner.store.connect()?.execute_batch(SCHEMA).map_err(sql)
}

/// What this machine keeps offline of the fleet's blobs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlobSettings {
    /// `pinned` or `all`.
    pub artifacts: String,
    pub budget_bytes: u64,
    pub prefetch: bool,
}

/// Whether this machine is a laptop or desktop, which prefetch by default.
fn desktop(inner: &Inner) -> bool {
    lock(&inner.identity)
        .as_ref()
        .and_then(|identity| identity.form_factor.clone())
        .is_some_and(|form| matches!(form.as_str(), "laptop" | "desktop" | "all_in_one"))
}

pub(super) fn settings(inner: &Inner) -> BlobSettings {
    inner
        .store
        .meta(SETTINGS_KEY)
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| BlobSettings {
            artifacts: "pinned".into(),
            budget_bytes: DEFAULT_BUDGET_BYTES,
            prefetch: desktop(inner),
        })
}

/// Whether this machine keeps `(class, key)` offline.
pub(super) fn pinned(inner: &Inner, class: &str, key: &str) -> Result<bool, String> {
    let found: Option<i64> = inner
        .store
        .connect()?
        .query_row(
            "SELECT 1 FROM fleet_pins WHERE class=?1 AND (key=?2 OR key='*')",
            params![class, key],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql)?;
    Ok(found.is_some())
}

fn now_millis(inner: &Inner) -> i64 {
    inner.now().timestamp_millis()
}

/// Notes a blob of `(class, key)`: one this machine made (`own`) never
/// leaves the content store.
pub(super) fn note(
    inner: &Inner,
    hash: &str,
    class: &str,
    key: &str,
    size: u64,
    own: bool,
) -> Result<(), String> {
    inner
        .store
        .connect()?
        .execute(
            "INSERT INTO fleet_blobs(hash, class, key, size, used_at, own) VALUES(?1, ?2, ?3, ?4, ?5, ?6) \
             ON CONFLICT(hash) DO UPDATE SET class=excluded.class, key=excluded.key, size=excluded.size, \
             own=MAX(fleet_blobs.own, excluded.own)",
            params![hash, class, key, size as i64, now_millis(inner), own as i64],
        )
        .map(drop)
        .map_err(sql)
}

/// Whether the content store holds `hash`.
pub(super) fn held(inner: &Inner, hash: &str) -> Result<bool, String> {
    let Ok(hash) = hash.parse::<ContentHash>() else {
        return Ok(false);
    };
    storage_lock(&inner.storage)?
        .cas
        .has(&hash)
        .map_err(|error| error.to_string())
}

/// Stores bytes this machine made in the content store.
pub(super) fn put(inner: &Inner, bytes: &[u8]) -> Result<String, String> {
    storage_lock(&inner.storage)?
        .cas
        .put(bytes)
        .map(|hash| hash.as_str().to_owned())
        .map_err(|error| error.to_string())
}

/// The bytes of `hash` when the content store holds them and they match
/// the hash. A use moves the blob to the end of the eviction order.
pub(super) fn bytes(inner: &Inner, hash: &str) -> Result<Option<Vec<u8>>, String> {
    let Ok(parsed) = hash.parse::<ContentHash>() else {
        return Ok(None);
    };
    let bytes = match storage_lock(&inner.storage)?.cas.get_verified(&parsed) {
        Ok(bytes) => bytes,
        Err(crate::cas::CasError::NotFound(_)) => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    touch(inner, hash)?;
    Ok(Some(bytes))
}

fn touch(inner: &Inner, hash: &str) -> Result<(), String> {
    inner
        .store
        .connect()?
        .execute(
            "UPDATE fleet_blobs SET used_at=?1 WHERE hash=?2",
            params![now_millis(inner), hash],
        )
        .map(drop)
        .map_err(sql)
}

/// Asks the members for `hash` unless the content store holds it. Answers
/// whether it is held.
pub(super) fn want(inner: &Inner, hash: &str, size: u64) -> Result<bool, String> {
    // Asking for a blob is a use, so it arrives at the end of the eviction
    // order.
    touch(inner, hash)?;
    if held(inner, hash)? {
        return Ok(true);
    }
    if hash.parse::<ContentHash>().is_err() {
        return Err("The blob hash is invalid.".into());
    }
    let connection = inner.store.connect()?;
    connection
        .execute(
            "INSERT INTO blobs(hash, thread_id, total, requested, done) VALUES(?1, NULL, ?2, 1, 0) \
             ON CONFLICT(hash) DO UPDATE SET requested=1, done=0",
            params![hash, size as i64],
        )
        .map_err(sql)?;
    inner.nudge();
    Ok(false)
}

/// Removes fetched blobs past the budget, least recently used first, and
/// answers the hashes it removed. Pinned blobs and this machine's own stay.
pub(super) fn evict(inner: &Inner) -> Result<Vec<String>, String> {
    let budget = settings(inner).budget_bytes;
    let rows: Vec<(String, String, String, u64)> = {
        let connection = inner.store.connect()?;
        let mut statement = connection
            .prepare(
                "SELECT hash, class, key, size FROM fleet_blobs WHERE own=0 ORDER BY used_at DESC",
            )
            .map_err(sql)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get::<_, i64>(3)? as u64,
                ))
            })
            .map_err(sql)?;
        rows.collect::<Result<_, _>>().map_err(sql)?
    };
    let mut kept = 0u64;
    let mut evicted = Vec::new();
    for (hash, class, key, size) in rows {
        if !held(inner, &hash)? {
            continue;
        }
        if pinned(inner, &class, &key)? || kept + size <= budget {
            kept += size;
            continue;
        }
        if let Ok(parsed) = hash.parse::<ContentHash>() {
            storage_lock(&inner.storage)?
                .cas
                .remove(&parsed)
                .map_err(|error| error.to_string())?;
        }
        inner
            .store
            .connect()?
            .execute(
                "UPDATE blobs SET requested=0, done=0 WHERE hash=?1",
                [&hash],
            )
            .map_err(sql)?;
        evicted.push(hash);
    }
    Ok(evicted)
}

fn pins(inner: &Inner) -> Result<Vec<Value>, String> {
    let connection = inner.store.connect()?;
    let mut statement = connection
        .prepare("SELECT class, key FROM fleet_pins ORDER BY class, key")
        .map_err(sql)?;
    let rows = statement
        .query_map([], |row| {
            Ok(json!({"class": row.get::<_, String>(0)?, "key": row.get::<_, String>(1)?}))
        })
        .map_err(sql)?;
    rows.collect::<Result<_, _>>().map_err(sql)
}

/// `fleet.pins {}`: this machine's pins, its offline settings and how many
/// fetched bytes it keeps.
pub(super) fn pins_operation(inner: &Inner, body: Value) -> Result<Value, String> {
    super::empty(&body)?;
    view(inner)
}

fn view(inner: &Inner) -> Result<Value, String> {
    let (held_bytes, own_bytes): (i64, i64) = inner
        .store
        .connect()?
        .query_row(
            "SELECT COALESCE(SUM(CASE WHEN own=0 THEN size END), 0), COALESCE(SUM(CASE WHEN own=1 THEN size END), 0) FROM fleet_blobs",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(sql)?;
    Ok(json!({
        "pins": pins(inner)?,
        "offline": settings(inner),
        "usage": {"fetched_bytes": held_bytes, "own_bytes": own_bytes},
    }))
}

/// `fleet.pin_set {pins?, offline?}`: pins or unpins items for this
/// machine, `{class, key, pinned}` each, and changes its offline settings.
/// Answers what `fleet.pins` answers.
pub(super) fn pin_set_operation(inner: &Inner, body: Value) -> Result<Value, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Pin {
        class: String,
        key: String,
        pinned: bool,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Offline {
        #[serde(default)]
        artifacts: Option<String>,
        #[serde(default)]
        budget_bytes: Option<u64>,
        #[serde(default)]
        prefetch: Option<bool>,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Body {
        #[serde(default)]
        pins: Vec<Pin>,
        #[serde(default)]
        offline: Option<Offline>,
    }
    let body: Body = serde_json::from_value(body).map_err(|_| "The request is invalid.")?;
    if body.pins.len() > 256 {
        return Err("Pin at most 256 items at once.".into());
    }
    for pin in &body.pins {
        if !CLASSES.contains(&pin.class.as_str()) || pin.key.is_empty() || pin.key.len() > 1024 {
            return Err(format!(
                "Pin an item of {} by its key, or \"*\" for the whole class.",
                CLASSES.join(" or ")
            ));
        }
    }
    let mut settings = settings(inner);
    if let Some(offline) = body.offline {
        if let Some(artifacts) = offline.artifacts {
            if artifacts != "pinned" && artifacts != "all" {
                return Err("Keep pinned artifacts or all artifacts offline.".into());
            }
            settings.artifacts = artifacts;
        }
        if let Some(budget) = offline.budget_bytes {
            settings.budget_bytes = budget;
        }
        if let Some(prefetch) = offline.prefetch {
            settings.prefetch = prefetch;
        }
        inner.store.set_meta(
            SETTINGS_KEY,
            Some(&serde_json::to_string(&settings).map_err(|error| error.to_string())?),
        )?;
    }
    let connection = inner.store.connect()?;
    for pin in &body.pins {
        if pin.pinned {
            connection
                .execute(
                    "INSERT OR IGNORE INTO fleet_pins(class, key) VALUES(?1, ?2)",
                    params![pin.class, pin.key],
                )
                .map_err(sql)?;
        } else {
            connection
                .execute(
                    "DELETE FROM fleet_pins WHERE class=?1 AND key=?2",
                    params![pin.class, pin.key],
                )
                .map_err(sql)?;
        }
    }
    inner.nudge();
    view(inner)
}

/// The hashes a client may read: blobs the fleet's records or threads name.
fn readable(inner: &Inner, hash: &str) -> Result<bool, String> {
    let connection = inner.store.connect()?;
    let known: Option<i64> = connection
        .query_row(
            "SELECT 1 FROM fleet_blobs WHERE hash=?1 UNION SELECT 1 FROM blobs WHERE hash=?1",
            [hash],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql)?;
    Ok(known.is_some())
}

/// `blob.read {hash, offset?}`: up to 256 KiB of a blob the fleet's records
/// or threads name, from `offset`. A blob this machine lacks is fetched,
/// and the answer says `fetching` until it arrives.
pub(super) fn read_operation(inner: &Inner, body: Value) -> Result<Value, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Body {
        hash: String,
        #[serde(default)]
        offset: u64,
    }
    let body: Body = serde_json::from_value(body).map_err(|_| "The request is invalid.")?;
    let hash = body
        .hash
        .parse::<ContentHash>()
        .map_err(|_| "The blob hash is invalid.")?;
    if !readable(inner, hash.as_str())? {
        return Err("No fleet item names that blob.".into());
    }
    if !held(inner, hash.as_str())? {
        let size: Option<i64> = inner
            .store
            .connect()?
            .query_row(
                "SELECT size FROM fleet_blobs WHERE hash=?1",
                [hash.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql)?;
        want(inner, hash.as_str(), size.unwrap_or(0).max(0) as u64)?;
        return Ok(json!({"hash": hash, "status": "fetching"}));
    }
    touch(inner, hash.as_str())?;
    let storage = storage_lock(&inner.storage)?;
    let total = storage
        .cas
        .open_object(&hash)
        .map_err(|error| error.to_string())?
        .and_then(|file| file.metadata().ok())
        .map_or(0, |metadata| metadata.len());
    if body.offset > total {
        return Err("The offset is past the end of the blob.".into());
    }
    let length = READ_CHUNK.min(total - body.offset);
    let data = storage
        .cas
        .read_range(&hash, body.offset, length)
        .map_err(|error| error.to_string())?;
    Ok(json!({
        "hash": hash,
        "status": "held",
        "offset": body.offset,
        "total": total,
        "data": STANDARD.encode(data),
        "done": body.offset + length >= total,
    }))
}

/// The hashes of `class` this machine fetched, for tests and the view.
#[cfg(test)]
pub(super) fn fetched(
    inner: &Inner,
    class: &str,
) -> Result<std::collections::BTreeSet<String>, String> {
    let connection = inner.store.connect()?;
    let mut statement = connection
        .prepare("SELECT hash FROM fleet_blobs WHERE class=?1 AND own=0")
        .map_err(sql)?;
    let rows = statement
        .query_map([class], |row| row.get::<_, String>(0))
        .map_err(sql)?;
    let mut hashes = std::collections::BTreeSet::new();
    for hash in rows {
        let hash = hash.map_err(sql)?;
        if held(inner, &hash)? {
            hashes.insert(hash);
        }
    }
    Ok(hashes)
}
