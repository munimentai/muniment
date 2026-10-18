//! One turn's changes: the difference between a run's "before" and "after"
//! records, and putting one file or one hunk of it back.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use muniment_code_diff::{CodeDiff, DiffFile, DiffHunk, DiffStatus};
use serde::Serialize;

use super::restore::{io_reason, remove_file, safe_path, write_bytes, write_entry};
use super::{hash_reader, Checkpoint, CheckpointStore, FileEntry, StoreGuard};
use crate::cas::ContentHash;

/// The most files one turn lists. The count covers the rest.
const MAX_LISTED_FILES: usize = 200;
/// The most bytes one turn's diff answers.
const MAX_DIFF_ANSWER_BYTES: usize = 512 * 1024;
/// A larger side shows counts only.
const MAX_DIFF_BYTES: u64 = 1024 * 1024;
/// The line diff compares every line pair, so two large sides show counts
/// only.
const MAX_LINE_PAIRS: usize = 4_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnFileState {
    /// The file is as the turn left it.
    Applied,
    /// The file is as it was before the turn.
    Reverted,
    /// The file changed again after the turn.
    ChangedSince,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TurnFile {
    pub path: String,
    pub status: DiffStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additions: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deletions: Option<u64>,
    pub binary: bool,
    pub state: TurnFileState,
    /// Per hunk of the diff: whether the turn's lines are still in the file.
    pub hunks_applied: Vec<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TurnChanges {
    pub run_id: String,
    pub folder: String,
    /// False when the turn has no complete pair of records.
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<String>,
    pub files: Vec<TurnFile>,
    /// Changed files past the listed ones.
    pub more: usize,
    pub diff: CodeDiff,
}

/// The files a turn changed, with the line diff of each text file.
pub fn turn_changes(
    store: &CheckpointStore,
    before: &Checkpoint,
    after: &Checkpoint,
) -> TurnChanges {
    let mut changes = TurnChanges {
        run_id: before.run_id.clone(),
        folder: after.folder.clone(),
        available: true,
        unavailable: None,
        files: Vec::new(),
        more: 0,
        diff: CodeDiff {
            schema_version: 1,
            id: format!("turn-{}", before.run_id),
            files: Vec::new(),
            truncated: false,
        },
    };
    if let Some(incomplete) = before.incomplete.or(after.incomplete) {
        changes.available = false;
        changes.unavailable = Some(incomplete.sentence(store.limits()));
        return changes;
    }
    if before.folder != after.folder {
        changes.available = false;
        changes.unavailable = Some("The turn moved to another folder.".into());
        return changes;
    }
    let paths: BTreeSet<&String> = before.files.keys().chain(after.files.keys()).collect();
    let changed: Vec<&String> = paths
        .into_iter()
        .filter(|path| {
            let (old, new) = (before.files.get(*path), after.files.get(*path));
            old.map(|entry| (&entry.hash, entry.executable))
                != new.map(|entry| (&entry.hash, entry.executable))
        })
        .collect();
    changes.more = changed.len().saturating_sub(MAX_LISTED_FILES);
    let root = Path::new(&after.folder);
    for path in changed.into_iter().take(MAX_LISTED_FILES) {
        let old = before.files.get(path);
        let new = after.files.get(path);
        let mut file = diff_file(store, path, old, new);
        let current = current_hash(root, path);
        let state = if current.as_ref() == new.map(|entry| &entry.hash) {
            TurnFileState::Applied
        } else if current.as_ref() == old.map(|entry| &entry.hash) {
            TurnFileState::Reverted
        } else {
            TurnFileState::ChangedSince
        };
        let hunks_applied = match state {
            TurnFileState::Applied => vec![true; file.hunks.len()],
            TurnFileState::Reverted => vec![false; file.hunks.len()],
            TurnFileState::ChangedSince => {
                let now = safe_path(root, path)
                    .ok()
                    .and_then(|path| fs::read(path).ok());
                hunk_states(store, old, new, now.as_deref(), file.hunks.len())
            }
        };
        let counted = !file.binary && (old.is_none() || new.is_none() || !file.hunks.is_empty());
        let (additions, deletions) = if counted {
            line_counts(&file)
        } else {
            (None, None)
        };
        let status = file.status;
        let binary = file.binary;
        if !counted {
            file.hunks.clear();
        }
        changes.files.push(TurnFile {
            path: path.clone(),
            status,
            additions,
            deletions,
            binary,
            state,
            hunks_applied,
        });
        changes.diff.files.push(file);
    }
    changes.diff.truncated = changes.more > 0;
    // An answer travels in one frame, so the last hunks give way first.
    while serde_json::to_vec(&changes.diff).map_or(0, |bytes| bytes.len()) > MAX_DIFF_ANSWER_BYTES {
        let Some(file) = changes
            .diff
            .files
            .iter_mut()
            .rev()
            .find(|file| !file.hunks.is_empty())
        else {
            break;
        };
        file.hunks.pop();
        changes.diff.truncated = true;
    }
    changes
}

fn line_counts(file: &DiffFile) -> (Option<u64>, Option<u64>) {
    use muniment_code_diff::DiffLineKind;
    let count = |kind| {
        file.hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .filter(|line| line.kind == kind)
            .count() as u64
    };
    (
        Some(count(DiffLineKind::Addition)),
        Some(count(DiffLineKind::Deletion)),
    )
}

fn current_hash(root: &Path, path: &str) -> Option<ContentHash> {
    let path = safe_path(root, path).ok()?;
    let metadata = fs::symlink_metadata(&path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    fs::File::open(path)
        .and_then(|mut file| hash_reader(&mut file))
        .ok()
}

fn read_entry(store: &CheckpointStore, entry: Option<&FileEntry>) -> Option<Vec<u8>> {
    match entry {
        Some(entry) => store.read_object(&entry.hash).ok(),
        None => Some(Vec::new()),
    }
}

/// One file's diff. A file too large to compare, or not UTF-8, carries no
/// hunks.
fn diff_file(
    store: &CheckpointStore,
    path: &str,
    old: Option<&FileEntry>,
    new: Option<&FileEntry>,
) -> DiffFile {
    let status = match (old, new) {
        (None, Some(_)) => DiffStatus::Added,
        (Some(_), None) => DiffStatus::Deleted,
        _ => DiffStatus::Modified,
    };
    let empty = DiffFile {
        old_path: old.map(|_| path.to_owned()),
        new_path: new.map(|_| path.to_owned()),
        status,
        old_mode: None,
        new_mode: None,
        binary: false,
        hunks: Vec::new(),
    };
    let sizes = [old, new].map(|entry| entry.map_or(0, |entry| entry.size));
    if sizes.iter().any(|size| *size > MAX_DIFF_BYTES) {
        return empty;
    }
    let (Some(old_bytes), Some(new_bytes)) = (read_entry(store, old), read_entry(store, new))
    else {
        return empty;
    };
    let text = |bytes: &[u8]| !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok();
    if !text(&old_bytes) || !text(&new_bytes) {
        return DiffFile {
            binary: true,
            ..empty
        };
    }
    let lines = |bytes: &[u8]| bytes.split_inclusive(|byte| *byte == b'\n').count();
    if lines(&old_bytes).saturating_mul(lines(&new_bytes)) > MAX_LINE_PAIRS {
        return empty;
    }
    let mut current = BTreeMap::new();
    let mut staged = BTreeMap::new();
    if old.is_some() {
        current.insert(path.to_owned(), old_bytes);
    }
    if new.is_some() {
        staged.insert(path.to_owned(), new_bytes);
    }
    match crate::code_diff::compute_code_diff(&current, &staged) {
        Ok(mut diff) if diff.files.len() == 1 => {
            let mut file = diff.files.remove(0);
            file.old_path = old.map(|_| path.to_owned());
            file.new_path = new.map(|_| path.to_owned());
            file.status = status;
            file
        }
        _ => empty,
    }
}

/// The raw lines of a text, each with its line ending.
fn raw_lines(bytes: &[u8]) -> Vec<&[u8]> {
    bytes.split_inclusive(|byte| *byte == b'\n').collect()
}

/// A hunk's span on one side: its first line index and its line count.
fn span(start: u64, count: u64) -> (usize, usize) {
    let first = if count == 0 { start } else { start - 1 };
    (first as usize, count as usize)
}

/// A hunk's lines before the turn, its lines after, and where the after
/// lines start.
type HunkSides<'a> = (Vec<&'a [u8]>, Vec<&'a [u8]>, usize);

/// The text of a hunk on each side: the lines before and after the turn.
fn hunk_sides<'a>(
    hunk: &DiffHunk,
    old_lines: &[&'a [u8]],
    new_lines: &[&'a [u8]],
) -> Option<HunkSides<'a>> {
    let (old_first, old_count) = span(hunk.old_start, hunk.old_count);
    let (new_first, new_count) = span(hunk.new_start, hunk.new_count);
    let old_side = old_lines.get(old_first..old_first + old_count)?.to_vec();
    let new_side = new_lines.get(new_first..new_first + new_count)?.to_vec();
    Some((old_side, new_side, new_first))
}

/// Where `needle` sits in `lines`, nearest `expected` when it sits in several
/// places.
fn locate(lines: &[&[u8]], needle: &[&[u8]], expected: usize) -> Option<usize> {
    if needle.is_empty() || needle.len() > lines.len() {
        return None;
    }
    (0..=lines.len() - needle.len())
        .filter(|start| lines[*start..*start + needle.len()] == *needle)
        .min_by_key(|start| start.abs_diff(expected))
}

fn hunk_states(
    store: &CheckpointStore,
    old: Option<&FileEntry>,
    new: Option<&FileEntry>,
    now: Option<&[u8]>,
    count: usize,
) -> Vec<bool> {
    let (Some(old_bytes), Some(new_bytes), Some(now)) =
        (read_entry(store, old), read_entry(store, new), now)
    else {
        return vec![false; count];
    };
    let Some(diff) = text_diff(&old_bytes, &new_bytes) else {
        return vec![false; count];
    };
    let (old_lines, new_lines, now_lines) =
        (raw_lines(&old_bytes), raw_lines(&new_bytes), raw_lines(now));
    diff.iter()
        .map(|hunk| {
            hunk_sides(hunk, &old_lines, &new_lines)
                .is_some_and(|(_, new_side, first)| locate(&now_lines, &new_side, first).is_some())
        })
        .collect()
}

fn text_diff(old: &[u8], new: &[u8]) -> Option<Vec<DiffHunk>> {
    let text = |bytes: &[u8]| !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok();
    let lines = |bytes: &[u8]| bytes.split_inclusive(|byte| *byte == b'\n').count();
    if old.len() as u64 > MAX_DIFF_BYTES
        || new.len() as u64 > MAX_DIFF_BYTES
        || !text(old)
        || !text(new)
        || lines(old).saturating_mul(lines(new)) > MAX_LINE_PAIRS
    {
        return None;
    }
    let mut current = BTreeMap::new();
    let mut staged = BTreeMap::new();
    current.insert(String::from("file"), old.to_vec());
    staged.insert(String::from("file"), new.to_vec());
    let diff = crate::code_diff::compute_code_diff(&current, &staged).ok()?;
    diff.files.into_iter().next().map(|file| file.hunks)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevertError {
    /// The turn has no complete pair of records.
    Unavailable(String),
    /// The turn did not change this file or hunk.
    NotChanged,
    /// The file changed after the turn, so a revert would lose that change.
    ChangedSince,
    /// A hunk revert needs a text file on both sides.
    WholeFileOnly,
    Write(String),
}

impl std::fmt::Display for RevertError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(reason) => formatter.write_str(reason),
            Self::NotChanged => formatter.write_str("This turn did not change that file."),
            Self::ChangedSince => formatter
                .write_str("The file changed after this turn. Revert it by hand or rewind."),
            Self::WholeFileOnly => formatter.write_str("Revert this file as a whole."),
            Self::Write(reason) => write!(formatter, "The file could not be written. {reason}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RevertOutcome {
    pub path: String,
    pub state: TurnFileState,
}

fn check_pair(
    store: &CheckpointStore,
    before: &Checkpoint,
    after: &Checkpoint,
) -> Result<(), RevertError> {
    if let Some(incomplete) = before.incomplete.or(after.incomplete) {
        return Err(RevertError::Unavailable(
            incomplete.sentence(store.limits()),
        ));
    }
    if before.folder != after.folder {
        return Err(RevertError::Unavailable(
            "The turn moved to another folder.".into(),
        ));
    }
    Ok(())
}

/// Puts one file back as it was before the turn, when it is still as the
/// turn left it.
pub fn revert_file(
    store: &CheckpointStore,
    _guard: &StoreGuard<'_>,
    before: &Checkpoint,
    after: &Checkpoint,
    path: &str,
) -> Result<RevertOutcome, RevertError> {
    check_pair(store, before, after)?;
    let old = before.files.get(path);
    let new = after.files.get(path);
    if old.map(|entry| &entry.hash) == new.map(|entry| &entry.hash) {
        return Err(RevertError::NotChanged);
    }
    let root = Path::new(&after.folder);
    if current_hash(root, path).as_ref() != new.map(|entry| &entry.hash) {
        return Err(RevertError::ChangedSince);
    }
    match old {
        Some(entry) => write_entry(store, root, path, entry),
        None => remove_file(root, path),
    }
    .map_err(RevertError::Write)?;
    Ok(RevertOutcome {
        path: path.to_owned(),
        state: TurnFileState::Reverted,
    })
}

/// Puts one hunk of a text file back as it was before the turn. The hunk's
/// lines from the turn must still stand in the file.
pub fn revert_hunk(
    store: &CheckpointStore,
    _guard: &StoreGuard<'_>,
    before: &Checkpoint,
    after: &Checkpoint,
    path: &str,
    hunk: usize,
) -> Result<RevertOutcome, RevertError> {
    check_pair(store, before, after)?;
    let (Some(old), Some(new)) = (before.files.get(path), after.files.get(path)) else {
        return Err(RevertError::WholeFileOnly);
    };
    if old.hash == new.hash {
        return Err(RevertError::NotChanged);
    }
    let old_bytes = store
        .read_object(&old.hash)
        .map_err(|error| RevertError::Write(error.to_string()))?;
    let new_bytes = store
        .read_object(&new.hash)
        .map_err(|error| RevertError::Write(error.to_string()))?;
    let hunks = text_diff(&old_bytes, &new_bytes).ok_or(RevertError::WholeFileOnly)?;
    let target = hunks.get(hunk).ok_or(RevertError::NotChanged)?;
    let (old_lines, new_lines) = (raw_lines(&old_bytes), raw_lines(&new_bytes));
    let (old_side, new_side, expected) =
        hunk_sides(target, &old_lines, &new_lines).ok_or(RevertError::WholeFileOnly)?;
    if new_side.is_empty() {
        return Err(RevertError::WholeFileOnly);
    }
    let root = Path::new(&after.folder);
    let destination = safe_path(root, path).map_err(RevertError::Write)?;
    let now = fs::read(&destination).map_err(|_| RevertError::ChangedSince)?;
    let now_lines = raw_lines(&now);
    let start = locate(&now_lines, &new_side, expected).ok_or(RevertError::ChangedSince)?;
    let mut reverted = Vec::with_capacity(now.len());
    for line in now_lines[..start]
        .iter()
        .chain(&old_side)
        .chain(&now_lines[start + new_side.len()..])
    {
        reverted.extend_from_slice(line);
    }
    let executable = fs::metadata(&destination)
        .map(|metadata| super::executable(&metadata))
        .map_err(|error| RevertError::Write(io_reason(&error)))?;
    write_bytes(&destination, &reverted, executable).map_err(RevertError::Write)?;
    let state = if reverted == old_bytes {
        TurnFileState::Reverted
    } else {
        TurnFileState::ChangedSince
    };
    Ok(RevertOutcome {
        path: path.to_owned(),
        state,
    })
}
