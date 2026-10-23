//! Per-turn checkpoints of a thread's working folder and its saved
//! conversation.
//!
//! Before a run starts, the runtime records every file of the run's folder and
//! the thread's Pi session file in a content-addressed store. After the run it
//! records the folder again. A turn's changes are the difference between the
//! two records, whichever tool made them, a file tool or a shell command.
//! Rewind writes a turn's "before" record back.
//!
//! A record is bounded in files, bytes and time. A folder over a bound gives
//! an incomplete record, which never restores files, so a restore never
//! removes a file the record did not see.

mod effects;
pub mod ops;
mod restore;
mod run;
mod turn;

pub use effects::{irreversible_effect, IrreversibleEffect, IrreversibleKind};
pub use restore::{
    plan_restore, restore, restore_conversation, FailedPath, RestoreError, RestorePlan,
    RestoreReport, Unrestorable, UnrestorableReason,
};
pub use run::{excluded_folders, protected_folders, shared, RunCheckpoints, CHECKPOINT_FOLDER};
pub use turn::{
    revert_file, revert_hunk, turn_changes, RevertError, RevertOutcome, TurnChanges, TurnFile,
    TurnFileState,
};

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::cas::{CasError, ContentHash, LocalCas};

pub const SCHEMA_VERSION: u32 = 1;

/// Folders a record leaves out by name: version control data, dependency
/// installs and caches that their tools rebuild. A restore leaves them as
/// they are and says so.
pub const EXCLUDED_DIRECTORIES: [&str; 7] = [
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "target",
    ".venv",
    "__pycache__",
];

/// The most skipped paths one record names. The count covers the rest.
const MAX_NAMED_SKIPS: usize = 500;
/// A file modified this close to the previous record is hashed again: its
/// size and time can match while its bytes differ.
const RACY_WINDOW_NS: u64 = 2_000_000_000;
/// Captures between two sweeps of old records.
const PRUNE_EVERY: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckpointLimits {
    /// The most files one record holds.
    pub max_files: usize,
    /// A larger file is named in the record but not kept.
    pub max_file_bytes: u64,
    /// The most new bytes one record reads.
    pub max_record_bytes: u64,
    /// How long one record may take.
    pub time_budget: Duration,
    /// The most records one thread keeps, newest first.
    pub keep_per_thread: usize,
    /// Older records go.
    pub max_age: Duration,
    /// The store's object bytes. The oldest records go past it.
    pub max_store_bytes: u64,
}

impl Default for CheckpointLimits {
    fn default() -> Self {
        Self {
            max_files: 20_000,
            max_file_bytes: 16 * 1024 * 1024,
            max_record_bytes: 512 * 1024 * 1024,
            time_budget: Duration::from_secs(5),
            keep_per_thread: 200,
            max_age: Duration::from_secs(30 * 24 * 60 * 60),
            max_store_bytes: 4 * 1024 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointKind {
    /// The folder and conversation as a run found them.
    Before,
    /// The folder as a run left it.
    After,
    /// The folder just before a rewind wrote over it.
    Rewind,
}

impl CheckpointKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Before => "before",
            Self::After => "after",
            Self::Rewind => "rewind",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub hash: ContentHash,
    pub size: u64,
    pub modified_ns: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub executable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    TooLarge,
    Link,
    Special,
    Unreadable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedPath {
    pub path: String,
    pub reason: SkipReason,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub modified_ns: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Incomplete {
    TooManyFiles,
    TooManyBytes,
    TimeBudget,
    FolderUnavailable,
}

impl Incomplete {
    /// One sentence a person reads in place of the files.
    pub fn sentence(self, limits: &CheckpointLimits) -> String {
        match self {
            Self::TooManyFiles => format!(
                "The folder holds more than {} files, so its files were not saved.",
                limits.max_files
            ),
            Self::TooManyBytes => format!(
                "The folder holds more than {} MB of files, so its files were not saved.",
                limits.max_record_bytes / (1024 * 1024)
            ),
            Self::TimeBudget => {
                "Saving the folder took too long, so its files were not saved.".into()
            }
            Self::FolderUnavailable => {
                "The folder could not be read, so its files were not saved.".into()
            }
        }
    }
}

/// The thread's Pi session file as a run found it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationEntry {
    pub locator: String,
    pub hash: ContentHash,
    pub size: u64,
}

/// One record of a folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint {
    pub schema_version: u32,
    pub id: String,
    pub thread_id: String,
    pub run_id: String,
    pub kind: CheckpointKind,
    /// The folder's canonical path.
    pub folder: String,
    pub created_at_ms: u64,
    /// Files by their `/`-separated path under the folder.
    pub files: BTreeMap<String, FileEntry>,
    pub directories: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<SkippedPath>,
    #[serde(default)]
    pub skipped_count: u64,
    /// Excluded folders the walk met, by path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incomplete: Option<Incomplete>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation: Option<ConversationEntry>,
}

impl Checkpoint {
    pub fn skipped_path(&self, path: &str) -> Option<&SkippedPath> {
        self.skipped.iter().find(|skipped| skipped.path == path)
    }
}

#[derive(Debug)]
pub enum CheckpointError {
    Io(io::Error),
    Cas(CasError),
    Corrupt(String),
    InvalidId,
}

impl fmt::Display for CheckpointError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "checkpoint I/O error: {error}"),
            Self::Cas(error) => write!(formatter, "checkpoint store error: {error}"),
            Self::Corrupt(detail) => write!(formatter, "checkpoint record is invalid: {detail}"),
            Self::InvalidId => formatter.write_str("checkpoint id is invalid"),
        }
    }
}

impl std::error::Error for CheckpointError {}

impl From<io::Error> for CheckpointError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<CasError> for CheckpointError {
    fn from(error: CasError) -> Self {
        Self::Cas(error)
    }
}

/// What one capture records.
pub struct CaptureRequest<'a> {
    pub thread_id: &'a str,
    pub run_id: &'a str,
    pub kind: CheckpointKind,
    pub folder: &'a Path,
    /// Folders under `folder` the record leaves out, such as Muniment's own
    /// memory and session folders when the run works in Home.
    pub excluded: &'a [PathBuf],
    /// The thread's session file: its locator and its path.
    pub conversation: Option<(&'a str, &'a Path)>,
}

/// A folder's newest record: its time and its files.
type RecentFiles = (u64, BTreeMap<String, FileEntry>);

/// The records and objects of every thread's checkpoints, under one folder.
pub struct CheckpointStore {
    root: PathBuf,
    cas: LocalCas,
    limits: CheckpointLimits,
    /// Serializes captures, restores and sweeps, so a sweep never removes an
    /// object a capture just stored and a run never records a folder while a
    /// restore writes it.
    lock: Mutex<()>,
    /// The files of the newest record of each folder, to skip hashing a file
    /// whose size and time did not change.
    recent: Mutex<HashMap<String, RecentFiles>>,
    captures: AtomicUsize,
}

impl fmt::Debug for CheckpointStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CheckpointStore")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

/// The store's lock, held across one restore or revert.
pub struct StoreGuard<'a>(#[allow(dead_code)] MutexGuard<'a, ()>);

impl CheckpointStore {
    pub fn open(root: &Path, limits: CheckpointLimits) -> Result<Self, CheckpointError> {
        fs::create_dir_all(root.join("records"))?;
        Ok(Self {
            root: root.to_owned(),
            cas: LocalCas::open(root)?,
            limits,
            lock: Mutex::new(()),
            recent: Mutex::new(HashMap::new()),
            captures: AtomicUsize::new(0),
        })
    }

    pub fn limits(&self) -> &CheckpointLimits {
        &self.limits
    }

    pub fn lock(&self) -> StoreGuard<'_> {
        StoreGuard(
            self.lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    /// Records a folder and saves the record.
    pub fn capture(&self, request: &CaptureRequest<'_>) -> Result<Checkpoint, CheckpointError> {
        let guard = self.lock();
        let checkpoint = self.capture_locked(&guard, request)?;
        drop(guard);
        if request.kind == CheckpointKind::After
            && self
                .captures
                .fetch_add(1, Ordering::Relaxed)
                .is_multiple_of(PRUNE_EVERY)
        {
            let _ = self.prune(SystemTime::now());
        }
        Ok(checkpoint)
    }

    /// Records a folder while the caller holds the store's lock.
    pub fn capture_locked(
        &self,
        _guard: &StoreGuard<'_>,
        request: &CaptureRequest<'_>,
    ) -> Result<Checkpoint, CheckpointError> {
        if !safe_id(request.thread_id) || !safe_id(request.run_id) {
            return Err(CheckpointError::InvalidId);
        }
        let folder = fs::canonicalize(request.folder).unwrap_or_else(|_| request.folder.into());
        let key = folder.to_string_lossy().into_owned();
        let previous = self
            .recent
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&key)
            .cloned();
        let mut checkpoint = scan(
            &folder,
            request.excluded,
            previous.as_ref().map(|(at, files)| (*at, files)),
            &self.limits,
            Some(&self.cas),
        );
        checkpoint.id = uuid::Uuid::now_v7().to_string();
        checkpoint.thread_id = request.thread_id.to_owned();
        checkpoint.run_id = request.run_id.to_owned();
        checkpoint.kind = request.kind;
        if let Some((locator, path)) = request.conversation {
            if let Ok(mut file) = fs::File::open(path) {
                let size = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
                let hash = self.cas.put_reader(&mut file)?;
                checkpoint.conversation = Some(ConversationEntry {
                    locator: locator.to_owned(),
                    hash,
                    size,
                });
            }
        }
        self.write_record(&checkpoint)?;
        if checkpoint.incomplete.is_none() {
            self.recent
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(key, (checkpoint.created_at_ms, checkpoint.files.clone()));
        }
        Ok(checkpoint)
    }

    /// Reads a folder's current state against a record without storing
    /// anything.
    pub fn observe(&self, folder: &Path, excluded: &[PathBuf], against: &Checkpoint) -> Checkpoint {
        scan(
            folder,
            excluded,
            Some((against.created_at_ms, &against.files)),
            &self.limits,
            None,
        )
    }

    /// The record of one run, if it has one.
    pub fn record(
        &self,
        thread_id: &str,
        run_id: &str,
        kind: CheckpointKind,
    ) -> Result<Option<Checkpoint>, CheckpointError> {
        if !safe_id(thread_id) || !safe_id(run_id) {
            return Err(CheckpointError::InvalidId);
        }
        let path = self.record_path(thread_id, &format!("{run_id}.{}", kind.as_str()));
        read_record(&path)
    }

    /// The bytes of one stored object.
    pub fn read_object(&self, hash: &ContentHash) -> Result<Vec<u8>, CheckpointError> {
        Ok(self.cas.get_verified(hash)?)
    }

    pub(crate) fn open_object(&self, hash: &ContentHash) -> Result<fs::File, CheckpointError> {
        self.cas
            .open_object(hash)?
            .ok_or_else(|| CheckpointError::Cas(CasError::NotFound(hash.clone())))
    }

    /// Every record of a thread, oldest first.
    pub fn thread_records(&self, thread_id: &str) -> Result<Vec<Checkpoint>, CheckpointError> {
        if !safe_id(thread_id) {
            return Err(CheckpointError::InvalidId);
        }
        let directory = self.root.join("records").join(thread_id);
        let mut records = Vec::new();
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(records),
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            let path = entry?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            if let Ok(Some(record)) = read_record(&path) {
                records.push(record);
            }
        }
        records.sort_by(|a, b| (a.created_at_ms, &a.id).cmp(&(b.created_at_ms, &b.id)));
        Ok(records)
    }

    /// Removes the records of a thread, as when the thread is deleted.
    pub fn forget_thread(&self, thread_id: &str) -> Result<(), CheckpointError> {
        if !safe_id(thread_id) {
            return Err(CheckpointError::InvalidId);
        }
        let _guard = self.lock();
        match fs::remove_dir_all(self.root.join("records").join(thread_id)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    /// Removes records past the age and count bounds, then the oldest records
    /// while the objects pass the store bound, then every object no record
    /// names. Returns how many records went.
    pub fn prune(&self, now: SystemTime) -> Result<usize, CheckpointError> {
        let _guard = self.lock();
        let now_ms = millis(now);
        let max_age_ms = self.limits.max_age.as_millis() as u64;
        let mut kept: Vec<(PathBuf, Checkpoint)> = Vec::new();
        let mut removed = 0;
        let threads = match fs::read_dir(self.root.join("records")) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error.into()),
        };
        for thread in threads {
            let thread = thread?.path();
            if !thread.is_dir() {
                continue;
            }
            let mut records = Vec::new();
            for entry in fs::read_dir(&thread)? {
                let path = entry?.path();
                match read_record(&path) {
                    Ok(Some(record)) => records.push((path, record)),
                    _ => {
                        let _ = fs::remove_file(&path);
                        removed += 1;
                    }
                }
            }
            records.sort_by_key(|record| std::cmp::Reverse(record.1.created_at_ms));
            for (index, (path, record)) in records.into_iter().enumerate() {
                if index >= self.limits.keep_per_thread
                    || now_ms.saturating_sub(record.created_at_ms) > max_age_ms
                {
                    let _ = fs::remove_file(&path);
                    removed += 1;
                } else {
                    kept.push((path, record));
                }
            }
            let _ = fs::remove_dir(&thread);
        }
        kept.sort_by_key(|record| record.1.created_at_ms);
        let mut sizes: HashMap<ContentHash, u64> = HashMap::new();
        for (_, record) in &kept {
            for (hash, size) in record_objects(record) {
                sizes.insert(hash, size);
            }
        }
        let mut total: u64 = sizes.values().sum();
        let mut oldest = 0;
        while total > self.limits.max_store_bytes && oldest < kept.len() {
            let _ = fs::remove_file(&kept[oldest].0);
            removed += 1;
            oldest += 1;
            let mut remaining: HashMap<ContentHash, u64> = HashMap::new();
            for (_, record) in &kept[oldest..] {
                for (hash, size) in record_objects(record) {
                    remaining.insert(hash, size);
                }
            }
            total = remaining.values().sum();
        }
        let keep: HashSet<ContentHash> = kept[oldest..]
            .iter()
            .flat_map(|(_, record)| record_objects(record).map(|(hash, _)| hash))
            .collect();
        self.cas.collect_unreferenced(&keep)?;
        self.recent
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        Ok(removed)
    }

    pub(crate) fn write_record(&self, checkpoint: &Checkpoint) -> Result<(), CheckpointError> {
        let name = match checkpoint.kind {
            CheckpointKind::Rewind => format!("rewind-{}", checkpoint.id),
            kind => format!("{}.{}", checkpoint.run_id, kind.as_str()),
        };
        let path = self.record_path(&checkpoint.thread_id, &name);
        fs::create_dir_all(path.parent().expect("a record path has a parent"))?;
        let temporary = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec(checkpoint)
            .map_err(|error| CheckpointError::Corrupt(error.to_string()))?;
        fs::write(&temporary, bytes)?;
        crate::atomic_file::replace(&temporary, &path)?;
        Ok(())
    }

    fn record_path(&self, thread_id: &str, name: &str) -> PathBuf {
        self.root
            .join("records")
            .join(thread_id)
            .join(format!("{name}.json"))
    }
}

fn record_objects(record: &Checkpoint) -> impl Iterator<Item = (ContentHash, u64)> + '_ {
    record
        .files
        .values()
        .map(|entry| (entry.hash.clone(), entry.size))
        .chain(
            record
                .conversation
                .iter()
                .map(|conversation| (conversation.hash.clone(), conversation.size)),
        )
}

fn read_record(path: &Path) -> Result<Option<Checkpoint>, CheckpointError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let record: Checkpoint = serde_json::from_slice(&bytes)
        .map_err(|error| CheckpointError::Corrupt(error.to_string()))?;
    if record.schema_version != SCHEMA_VERSION {
        return Err(CheckpointError::Corrupt(
            "unsupported schema version".into(),
        ));
    }
    Ok(Some(record))
}

/// Thread and run ids name record files, so they hold only id characters.
fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// Whether two folders are one, or one holds the other.
pub fn folders_overlap(a: &Path, b: &Path) -> bool {
    let a = fs::canonicalize(a).unwrap_or_else(|_| a.to_owned());
    let b = fs::canonicalize(b).unwrap_or_else(|_| b.to_owned());
    a.starts_with(&b) || b.starts_with(&a)
}

pub(crate) fn millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

fn modified_ns(metadata: &fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|elapsed| elapsed.as_nanos() as u64)
        .unwrap_or(0)
}

#[cfg(unix)]
pub(crate) fn executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
pub(crate) fn executable(_metadata: &fs::Metadata) -> bool {
    false
}

pub(crate) fn hash_reader(reader: &mut impl Read) -> io::Result<ContentHash> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hex::encode(hasher.finalize())
        .parse()
        .expect("a SHA-256 digest is a content hash"))
}

/// Walks one folder. With `cas`, every new file's bytes are stored; without
/// it, the walk only hashes them. A file whose size and time match
/// `previous`, and which changed before that record, keeps its hash.
fn scan(
    folder: &Path,
    excluded: &[PathBuf],
    previous: Option<(u64, &BTreeMap<String, FileEntry>)>,
    limits: &CheckpointLimits,
    cas: Option<&LocalCas>,
) -> Checkpoint {
    let started = Instant::now();
    let created_at_ms = millis(SystemTime::now());
    let mut checkpoint = Checkpoint {
        schema_version: SCHEMA_VERSION,
        id: String::new(),
        thread_id: String::new(),
        run_id: String::new(),
        kind: CheckpointKind::Before,
        folder: folder.to_string_lossy().into_owned(),
        created_at_ms,
        files: BTreeMap::new(),
        directories: BTreeSet::new(),
        skipped: Vec::new(),
        skipped_count: 0,
        excluded: Vec::new(),
        incomplete: None,
        conversation: None,
    };
    let root = match fs::canonicalize(folder) {
        Ok(root) if root.is_dir() => root,
        _ => {
            checkpoint.incomplete = Some(Incomplete::FolderUnavailable);
            return checkpoint;
        }
    };
    checkpoint.folder = root.to_string_lossy().into_owned();
    let excluded: Vec<PathBuf> = excluded
        .iter()
        .map(|path| fs::canonicalize(path).unwrap_or_else(|_| path.clone()))
        .collect();
    let racy_after = previous.map_or(0, |(at, _)| {
        at.saturating_mul(1_000_000).saturating_sub(RACY_WINDOW_NS)
    });
    let mut read_bytes: u64 = 0;
    let mut stack = vec![String::new()];
    let skip = |checkpoint: &mut Checkpoint, skipped: SkippedPath| {
        checkpoint.skipped_count += 1;
        if checkpoint.skipped.len() < MAX_NAMED_SKIPS {
            checkpoint.skipped.push(skipped);
        }
    };
    'walk: while let Some(relative) = stack.pop() {
        let directory = if relative.is_empty() {
            root.clone()
        } else {
            root.join(&relative)
        };
        let mut entries: Vec<_> = match fs::read_dir(&directory) {
            Ok(entries) => entries.filter_map(Result::ok).collect(),
            Err(_) if relative.is_empty() => {
                checkpoint.incomplete = Some(Incomplete::FolderUnavailable);
                break;
            }
            Err(_) => {
                skip(
                    &mut checkpoint,
                    SkippedPath {
                        path: relative.clone(),
                        reason: SkipReason::Unreadable,
                        size: 0,
                        modified_ns: 0,
                    },
                );
                continue;
            }
        };
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            if started.elapsed() > limits.time_budget {
                checkpoint.incomplete = Some(Incomplete::TimeBudget);
                break 'walk;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                skip(
                    &mut checkpoint,
                    SkippedPath {
                        path: format!("{relative}/{}", entry.file_name().to_string_lossy())
                            .trim_start_matches('/')
                            .to_owned(),
                        reason: SkipReason::Unreadable,
                        size: 0,
                        modified_ns: 0,
                    },
                );
                continue;
            };
            let path = if relative.is_empty() {
                name.clone()
            } else {
                format!("{relative}/{name}")
            };
            let absolute = directory.join(&name);
            if excluded.iter().any(|excluded| excluded == &absolute) {
                checkpoint.excluded.push(path);
                continue;
            }
            let Ok(metadata) = fs::symlink_metadata(&absolute) else {
                skip(
                    &mut checkpoint,
                    SkippedPath {
                        path,
                        reason: SkipReason::Unreadable,
                        size: 0,
                        modified_ns: 0,
                    },
                );
                continue;
            };
            let file_type = metadata.file_type();
            if file_type.is_symlink() {
                skip(
                    &mut checkpoint,
                    SkippedPath {
                        path,
                        reason: SkipReason::Link,
                        size: 0,
                        modified_ns: modified_ns(&metadata),
                    },
                );
            } else if file_type.is_dir() {
                if EXCLUDED_DIRECTORIES.contains(&name.as_str()) {
                    checkpoint.excluded.push(path);
                } else {
                    checkpoint.directories.insert(path.clone());
                    stack.push(path);
                }
            } else if file_type.is_file() {
                if checkpoint.files.len() >= limits.max_files {
                    checkpoint.incomplete = Some(Incomplete::TooManyFiles);
                    break 'walk;
                }
                let size = metadata.len();
                let modified = modified_ns(&metadata);
                if size > limits.max_file_bytes {
                    skip(
                        &mut checkpoint,
                        SkippedPath {
                            path,
                            reason: SkipReason::TooLarge,
                            size,
                            modified_ns: modified,
                        },
                    );
                    continue;
                }
                let known = previous
                    .and_then(|(_, files)| files.get(&path))
                    .filter(|entry| {
                        entry.size == size
                            && entry.modified_ns == modified
                            && modified < racy_after
                            && cas.is_none_or(|cas| cas.has(&entry.hash).unwrap_or(false))
                    })
                    .map(|entry| entry.hash.clone());
                let hash = match known {
                    Some(hash) => hash,
                    None => {
                        read_bytes = read_bytes.saturating_add(size);
                        if read_bytes > limits.max_record_bytes {
                            checkpoint.incomplete = Some(Incomplete::TooManyBytes);
                            break 'walk;
                        }
                        let stored = fs::File::open(&absolute).map_err(CasError::from).and_then(
                            |mut file| match cas {
                                Some(cas) => cas.put_reader(&mut file),
                                None => hash_reader(&mut file).map_err(CasError::from),
                            },
                        );
                        match stored {
                            Ok(hash) => hash,
                            Err(_) => {
                                skip(
                                    &mut checkpoint,
                                    SkippedPath {
                                        path,
                                        reason: SkipReason::Unreadable,
                                        size,
                                        modified_ns: modified,
                                    },
                                );
                                continue;
                            }
                        }
                    }
                };
                checkpoint.files.insert(
                    path,
                    FileEntry {
                        hash,
                        size,
                        modified_ns: modified,
                        executable: executable(&metadata),
                    },
                );
            } else {
                skip(
                    &mut checkpoint,
                    SkippedPath {
                        path,
                        reason: SkipReason::Special,
                        size: 0,
                        modified_ns: 0,
                    },
                );
            }
        }
    }
    checkpoint
}

#[cfg(test)]
mod tests;
