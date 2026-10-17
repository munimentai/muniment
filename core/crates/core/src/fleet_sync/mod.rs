//! Fleet sync for the settings classes that hold no secrets: each class's
//! own storage mapped onto swarm records, both ways.
//!
//! - **Slots.** An adapter reads its storage as slots: a record key, a field
//!   and, for a set, an element. A machine-local value is never a slot, so it
//!   never leaves this runtime.
//! - **Baseline.** Per slot, this runtime keeps the hash of the local value
//!   and of the record value it last matched, in `fleet-sync/<class>.json`.
//!   A local value that moved off its hash is a local change and is written
//!   to the records. A record value that moved off its hash came from another
//!   member and is applied to the storage. Both directions settle on the same
//!   hashes, so an applied value is never written back.
//! - **First pass.** Passes wait until this runtime first heard from another
//!   member. The first pass of a class then takes the fleet's values and adds
//!   only the slots the fleet lacks, so a joining member never overwrites
//!   what the fleet chose.
//! - **Passes.** The swarm worker runs a class's pass after another member
//!   changed its records, after [`changed`], and every few seconds to catch
//!   edits made outside core.

mod memory;
mod permissions;
mod preferences;
mod projects;
mod retention;
mod routing;
mod search;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::swarm::{RecordChange, Records};

/// How often a pass runs for every class without a change notice.
const FULL_PASS: Duration = Duration::from_secs(5);
/// The records one page of a class scan reads.
const SCAN_PAGE: usize = 256;
const BASELINE_DIRECTORY: &str = "fleet-sync";

/// Every record class an adapter here keeps.
pub const CLASSES: [&str; 9] = [
    preferences::CLASS,
    permissions::CLASS,
    retention::CLASS,
    routing::ROUTE_CLASS,
    routing::CONNECTION_CLASS,
    search::CLASS,
    projects::PROJECT_CLASS,
    projects::MEMBER_CLASS,
    memory::CLASS,
];

pub use preferences::CLASS as PREFERENCES_CLASS;

/// One value an adapter keeps on records.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Slot {
    pub key: String,
    pub field: String,
    /// The element when the field is a set.
    pub element: Option<String>,
}

impl Slot {
    pub(crate) fn field(key: impl Into<String>, field: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            field: field.into(),
            element: None,
        }
    }

    pub(crate) fn element(
        key: impl Into<String>,
        set: impl Into<String>,
        element: impl Into<String>,
    ) -> Self {
        Self {
            key: key.into(),
            field: set.into(),
            element: Some(element.into()),
        }
    }

    fn id(&self) -> String {
        match &self.element {
            Some(element) => format!("{}\u{1f}{}\u{1f}{element}", self.key, self.field),
            None => format!("{}\u{1f}{}", self.key, self.field),
        }
    }

    fn parse(id: &str) -> Option<Self> {
        let mut parts = id.split('\u{1f}');
        let key = parts.next()?.to_owned();
        let field = parts.next()?.to_owned();
        let element = parts.next().map(str::to_owned);
        Some(Self {
            key,
            field,
            element,
        })
    }
}

/// A class's storage as slots.
pub(crate) type Local = BTreeMap<Slot, Value>;

/// What this runtime last matched for one slot.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Base {
    /// The hash of the local value, `None` when the storage holds none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<String>,
    /// The hash of the record value, `None` when the records hold none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    /// This runtime wrote the record value.
    #[serde(default)]
    pub mine: bool,
}

/// One record value to apply to the storage: `None` removes it.
#[derive(Clone, Debug)]
pub(crate) struct Change {
    pub slot: Slot,
    pub value: Option<Value>,
    pub previous: Option<Base>,
}

/// What an adapter reads and writes with.
pub(crate) struct Context<'a> {
    pub profile: &'a Path,
    pub records: &'a Records,
    pub origin: String,
    pub name: String,
}

/// One class's storage mapped onto records.
pub(crate) trait Adapter {
    /// The record class.
    fn class(&self) -> &'static str;
    /// The storage as slots, without machine-local values.
    fn read(&self, context: &Context<'_>) -> Result<Local, String>;
    /// Applies record values to the storage.
    fn write(&self, context: &Context<'_>, changes: &[Change]) -> Result<(), String>;
    /// Whether a key with no slots left deletes its record. Otherwise each
    /// field is removed alone.
    fn tombstones(&self) -> bool {
        true
    }
    /// The value written to the record for a local value.
    fn outgoing(&self, _slot: &Slot, value: Value, _previous: Option<&Base>) -> Value {
        value
    }
    /// The part of a record value that compares with a local value.
    fn comparable(&self, value: &Value) -> Value {
        value.clone()
    }
}

/// The hash a baseline keeps for a value.
pub(crate) fn hash(value: &Value) -> String {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    hex::encode(&Sha256::digest(&bytes)[..16])
}

#[derive(Default, Serialize, Deserialize)]
struct Baseline {
    slots: BTreeMap<String, Base>,
}

fn baseline_path(profile: &Path, class: &str) -> PathBuf {
    profile
        .join(BASELINE_DIRECTORY)
        .join(format!("{class}.json"))
}

fn load_baseline(profile: &Path, class: &str) -> Option<BTreeMap<Slot, Base>> {
    let bytes = std::fs::read(baseline_path(profile, class)).ok()?;
    let baseline: Baseline = serde_json::from_slice(&bytes).ok()?;
    Some(
        baseline
            .slots
            .into_iter()
            .filter_map(|(id, base)| Some((Slot::parse(&id)?, base)))
            .collect(),
    )
}

fn save_baseline(profile: &Path, class: &str, slots: &BTreeMap<Slot, Base>) -> Result<(), String> {
    let path = baseline_path(profile, class);
    let directory = path.parent().ok_or("The sync state path is invalid.")?;
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let baseline = Baseline {
        slots: slots
            .iter()
            .map(|(slot, base)| (slot.id(), base.clone()))
            .collect(),
    };
    let bytes = serde_json::to_vec(&baseline).map_err(|error| error.to_string())?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    crate::atomic_file::replace(&temporary, &path).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        error.to_string()
    })
}

/// Every live slot the records hold for a class, with the record value.
fn remote_slots(records: &Records, class: &str) -> Result<Local, String> {
    let mut slots = Local::new();
    let mut after: Option<String> = None;
    loop {
        let page = records.scan(class, after.as_deref(), SCAN_PAGE)?;
        let done = page.len() < SCAN_PAGE;
        for record in page {
            after = Some(record.key.clone());
            for (field, value) in record.fields {
                slots.insert(Slot::field(&record.key, field), value.value);
            }
            for (set, elements) in record.sets {
                for (element, value) in elements {
                    slots.insert(Slot::element(&record.key, &set, element), value);
                }
            }
        }
        if done {
            return Ok(slots);
        }
    }
}

/// Runs one pass of `adapter`: local changes to the records, then record
/// changes to the storage. Answers whether the storage changed.
pub(crate) fn pass(adapter: &dyn Adapter, context: &Context<'_>) -> Result<bool, String> {
    let class = adapter.class();
    let records = context.records;
    let saved = load_baseline(context.profile, class);
    let first = saved.is_none();
    let mut base = saved.unwrap_or_default();
    let local = adapter.read(context)?;
    let mut remote = remote_slots(records, class)?;
    let remote_hash = |remote: &Local, slot: &Slot| {
        remote
            .get(slot)
            .map(|value| hash(&adapter.comparable(value)))
    };

    // Local changes go to the records.
    for (slot, value) in &local {
        let local_hash = hash(value);
        let previous = base.get(slot).cloned();
        if previous.as_ref().and_then(|base| base.local.as_ref()) == Some(&local_hash) {
            continue;
        }
        let held = remote_hash(&remote, slot);
        if held.as_ref() == Some(&local_hash) {
            base.insert(
                slot.clone(),
                Base {
                    local: Some(local_hash.clone()),
                    remote: Some(local_hash),
                    mine: previous.is_some_and(|base| base.mine),
                },
            );
            continue;
        }
        if first && held.is_some() {
            // The fleet's value wins on the first pass.
            continue;
        }
        let outgoing = adapter.outgoing(slot, value.clone(), previous.as_ref());
        match &slot.element {
            Some(element) => {
                records.set_add(class, &slot.key, &slot.field, element, outgoing.clone())?;
            }
            None => {
                records.put(class, &slot.key, &slot.field, outgoing.clone())?;
            }
        }
        remote.insert(slot.clone(), outgoing);
        base.insert(
            slot.clone(),
            Base {
                local: Some(local_hash.clone()),
                remote: Some(local_hash),
                mine: true,
            },
        );
    }
    let removed: Vec<Slot> = base
        .iter()
        .filter(|(slot, base)| base.local.is_some() && !local.contains_key(*slot))
        .map(|(slot, _)| slot.clone())
        .collect();
    let mut deleted_keys = BTreeSet::new();
    for slot in removed {
        let key_gone = !local.keys().any(|held| held.key == slot.key);
        if adapter.tombstones() && key_gone {
            if deleted_keys.insert(slot.key.clone()) {
                records.delete(class, &slot.key)?;
                remote.retain(|held, _| held.key != slot.key);
            }
        } else if let Some(element) = &slot.element {
            records.set_remove(class, &slot.key, &slot.field, element)?;
            remote.remove(&slot);
        } else {
            records.remove(class, &slot.key, &slot.field)?;
            remote.remove(&slot);
        }
        base.remove(&slot);
    }
    for key in &deleted_keys {
        base.retain(|slot, _| &slot.key != key);
    }

    // Record changes go to the storage.
    let mut changes = Vec::new();
    for (slot, value) in &remote {
        let held = hash(&adapter.comparable(value));
        let previous = base.get(slot).cloned();
        if previous.as_ref().and_then(|base| base.remote.as_ref()) == Some(&held) {
            continue;
        }
        if local.get(slot).map(hash).as_ref() == Some(&held) {
            base.insert(
                slot.clone(),
                Base {
                    local: Some(held.clone()),
                    remote: Some(held),
                    mine: false,
                },
            );
            continue;
        }
        changes.push(Change {
            slot: slot.clone(),
            value: Some(value.clone()),
            previous,
        });
    }
    let gone: Vec<Slot> = base
        .iter()
        .filter(|(slot, base)| base.remote.is_some() && !remote.contains_key(*slot))
        .map(|(slot, _)| slot.clone())
        .collect();
    for slot in gone {
        if local.contains_key(&slot) {
            changes.push(Change {
                previous: base.get(&slot).cloned(),
                slot,
                value: None,
            });
        } else {
            base.remove(&slot);
        }
    }
    let wrote = !changes.is_empty();
    if wrote {
        match adapter.write(context, &changes) {
            Ok(()) => {
                let after = adapter.read(context)?;
                for change in &changes {
                    let remote = change
                        .value
                        .as_ref()
                        .map(|value| hash(&adapter.comparable(value)));
                    let local = after.get(&change.slot).map(hash);
                    if remote.is_none() && local.is_none() {
                        base.remove(&change.slot);
                    } else {
                        base.insert(
                            change.slot.clone(),
                            Base {
                                local,
                                remote,
                                mine: false,
                            },
                        );
                    }
                }
                // A write can add slots, such as a conflict copy: they are
                // local changes for the next pass.
            }
            Err(error) => {
                crate::runtime_eprintln!(
                    "muniment-runtime: fleet sync of {class} could not apply: {error}"
                );
            }
        }
    }
    save_baseline(context.profile, class, &base)?;
    Ok(wrote)
}

fn adapters() -> Vec<Box<dyn Adapter>> {
    vec![
        Box::new(preferences::Preferences),
        Box::new(permissions::Permissions),
        Box::new(retention::Retention),
        Box::new(routing::Routes),
        Box::new(routing::Connections),
        Box::new(search::Search),
        Box::new(projects::Projects),
        Box::new(projects::Members),
        Box::new(memory::Memory),
    ]
}

struct State {
    changes: Receiver<RecordChange>,
    due: BTreeSet<&'static str>,
    last_full: Option<Instant>,
}

static STATES: Mutex<Option<HashMap<PathBuf, State>>> = Mutex::new(None);
/// Passes never overlap, so a slot is read and written by one at a time.
static PASS: Mutex<()> = Mutex::new(());

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Marks `class` as changed on this runtime, so the next pass writes it.
pub fn changed(profile: &Path, class: &'static str) {
    if let Some(state) = lock(&STATES)
        .as_mut()
        .and_then(|states| states.get_mut(profile))
    {
        state.due.insert(class);
    }
}

/// The classes due for a pass: those another member changed, those marked
/// changed here, and all of them every [`FULL_PASS`].
fn due(records: &Records) -> Vec<&'static str> {
    let mut states = lock(&STATES);
    let state = states
        .get_or_insert_with(HashMap::new)
        .entry(records.profile().to_path_buf())
        .or_insert_with(|| State {
            changes: records.subscribe(),
            due: BTreeSet::new(),
            last_full: None,
        });
    while let Ok(change) = state.changes.try_recv() {
        if !change.local {
            if let Some(class) = CLASSES.iter().find(|class| **class == change.class) {
                state.due.insert(class);
            }
        }
    }
    if state
        .last_full
        .is_none_or(|last| last.elapsed() >= FULL_PASS)
    {
        state.last_full = Some(Instant::now());
        state.due.extend(CLASSES);
    }
    std::mem::take(&mut state.due).into_iter().collect()
}

/// One round of the swarm worker: a pass for each class that is due. A
/// runtime that never heard from another member waits, so its first pass
/// sees the fleet's values.
pub(crate) fn tick(records: &Records) {
    if records.origin().is_none() || !records.has_synced() {
        return;
    }
    let classes = due(records);
    if !classes.is_empty() {
        run(records, &classes);
    }
}

/// Runs a pass of every class now. Answers the classes whose storage
/// changed.
#[cfg(test)]
pub(crate) fn sync_all(records: &Records) -> Vec<&'static str> {
    run(records, &CLASSES)
}

fn run(records: &Records, classes: &[&'static str]) -> Vec<&'static str> {
    let Some(origin) = records.origin() else {
        return Vec::new();
    };
    let _pass = lock(&PASS);
    let context = Context {
        profile: records.profile(),
        records,
        name: records
            .runtime_name()
            .unwrap_or_else(|| "another machine".into()),
        origin,
    };
    let mut applied = Vec::new();
    for adapter in adapters() {
        if !classes.contains(&adapter.class()) {
            continue;
        }
        match pass(adapter.as_ref(), &context) {
            Ok(true) => {
                applied.push(adapter.class());
                records.settings_applied(adapter.class());
            }
            Ok(false) => {}
            Err(error) => crate::runtime_eprintln!(
                "muniment-runtime: fleet sync of {} failed: {error}",
                adapter.class()
            ),
        }
    }
    applied
}
