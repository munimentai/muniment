//! Writing a record back over its folder.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{
    hash_reader, CaptureRequest, Checkpoint, CheckpointError, CheckpointKind, CheckpointStore,
    FileEntry, SkipReason, StoreGuard,
};
use crate::cas::ContentHash;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnrestorableReason {
    /// The file was over the size bound when the record was made.
    TooLarge,
    /// A link, which a record does not follow.
    Link,
    /// The file could not be read when the record was made, or now.
    Unreadable,
    /// A folder now stands where the record holds a file.
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Unrestorable {
    pub path: String,
    pub reason: UnrestorableReason,
}

/// What a restore would do to a folder now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RestorePlan {
    pub checkpoint_id: String,
    pub folder: String,
    /// False when the record or the folder now is incomplete: the restore
    /// then writes no file.
    pub files_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<String>,
    /// Files written back: changed or removed since the record.
    pub restore: Vec<String>,
    /// Files made since the record, which the restore removes.
    pub remove: Vec<String>,
    pub cannot_restore: Vec<Unrestorable>,
    /// Folders a record leaves out, such as `.git` or `node_modules`.
    pub not_covered: Vec<String>,
    #[serde(skip)]
    pub create_directories: Vec<String>,
    #[serde(skip)]
    pub remove_directories: Vec<String>,
    /// Names this exact plan. A restore with another token refuses.
    pub token: String,
}

impl RestorePlan {
    pub fn changes_files(&self) -> bool {
        !self.restore.is_empty() || !self.remove.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FailedPath {
    pub path: String,
    pub reason: String,
}

/// What a restore did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RestoreReport {
    pub restored: Vec<String>,
    pub removed: Vec<String>,
    pub failed: Vec<FailedPath>,
    pub cannot_restore: Vec<Unrestorable>,
    pub not_covered: Vec<String>,
    /// The record of the folder just before the restore wrote over it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_checkpoint: Option<String>,
    /// True only when every planned file was written back or removed and
    /// verified, and nothing the record holds was left out.
    pub complete: bool,
}

/// Compares a record with its folder now.
pub fn plan_restore(
    store: &CheckpointStore,
    checkpoint: &Checkpoint,
    excluded: &[PathBuf],
) -> RestorePlan {
    let folder = Path::new(&checkpoint.folder);
    let current = store.observe(folder, excluded, checkpoint);
    let mut plan = RestorePlan {
        checkpoint_id: checkpoint.id.clone(),
        folder: checkpoint.folder.clone(),
        files_available: true,
        unavailable: None,
        restore: Vec::new(),
        remove: Vec::new(),
        cannot_restore: Vec::new(),
        not_covered: checkpoint
            .excluded
            .iter()
            .chain(&current.excluded)
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        create_directories: Vec::new(),
        remove_directories: Vec::new(),
        token: String::new(),
    };
    if let Some(incomplete) = checkpoint.incomplete.or(current.incomplete) {
        plan.files_available = false;
        plan.unavailable = Some(incomplete.sentence(store.limits()));
    } else {
        for (path, entry) in &checkpoint.files {
            if current.directories.contains(path) {
                plan.cannot_restore.push(Unrestorable {
                    path: path.clone(),
                    reason: UnrestorableReason::Blocked,
                });
                continue;
            }
            match current.files.get(path) {
                Some(now) if now.hash == entry.hash && now.executable == entry.executable => {}
                _ => plan.restore.push(path.clone()),
            }
        }
        for path in current.files.keys() {
            if checkpoint.files.contains_key(path) {
                continue;
            }
            match checkpoint.skipped_path(path) {
                Some(skipped) => {
                    let now = &current.files[path];
                    let unchanged = skipped.reason == SkipReason::TooLarge
                        && skipped.size == now.size
                        && skipped.modified_ns == now.modified_ns;
                    if !unchanged {
                        plan.cannot_restore.push(Unrestorable {
                            path: path.clone(),
                            reason: reason_of(skipped.reason),
                        });
                    }
                }
                None => plan.remove.push(path.clone()),
            }
        }
        for skipped in &current.skipped {
            if checkpoint.files.contains_key(&skipped.path) {
                continue;
            }
            match checkpoint.skipped_path(&skipped.path) {
                Some(before)
                    if before.reason == skipped.reason
                        && before.size == skipped.size
                        && before.modified_ns == skipped.modified_ns => {}
                Some(before) => plan.cannot_restore.push(Unrestorable {
                    path: skipped.path.clone(),
                    reason: reason_of(before.reason),
                }),
                None if matches!(skipped.reason, SkipReason::TooLarge | SkipReason::Link) => {
                    plan.remove.push(skipped.path.clone());
                }
                None => plan.cannot_restore.push(Unrestorable {
                    path: skipped.path.clone(),
                    reason: reason_of(skipped.reason),
                }),
            }
        }
        for skipped in &checkpoint.skipped {
            let present = current.files.contains_key(&skipped.path)
                || current.skipped_path(&skipped.path).is_some();
            if !present {
                plan.cannot_restore.push(Unrestorable {
                    path: skipped.path.clone(),
                    reason: reason_of(skipped.reason),
                });
            }
        }
        plan.create_directories = checkpoint
            .directories
            .difference(&current.directories)
            .cloned()
            .collect();
        plan.remove_directories = current
            .directories
            .difference(&checkpoint.directories)
            .cloned()
            .collect();
        plan.remove_directories.sort_by(|a, b| b.cmp(a));
    }
    plan.restore.sort();
    plan.remove.sort();
    plan.cannot_restore.sort_by(|a, b| a.path.cmp(&b.path));
    let mut digest = Sha256::new();
    digest.update(checkpoint.id.as_bytes());
    for path in &plan.restore {
        digest.update(b"\0w");
        digest.update(path.as_bytes());
        if let Some(now) = current.files.get(path) {
            digest.update(now.hash.as_str().as_bytes());
        }
    }
    for path in &plan.remove {
        digest.update(b"\0r");
        digest.update(path.as_bytes());
        if let Some(now) = current.files.get(path) {
            digest.update(now.hash.as_str().as_bytes());
        }
    }
    for item in &plan.cannot_restore {
        digest.update(b"\0c");
        digest.update(item.path.as_bytes());
    }
    digest.update([u8::from(plan.files_available)]);
    plan.token = hex::encode(digest.finalize());
    plan
}

fn reason_of(reason: SkipReason) -> UnrestorableReason {
    match reason {
        SkipReason::TooLarge => UnrestorableReason::TooLarge,
        SkipReason::Link => UnrestorableReason::Link,
        SkipReason::Unreadable | SkipReason::Special => UnrestorableReason::Unreadable,
    }
}

#[derive(Debug)]
pub enum RestoreError {
    /// The folder changed after the plan the caller showed.
    Changed,
    /// The record of the folder before the restore could not be saved, so
    /// nothing was written.
    SafetyRecord(CheckpointError),
}

impl std::fmt::Display for RestoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Changed => formatter.write_str("The folder changed after the preview."),
            Self::SafetyRecord(error) => {
                write!(formatter, "The folder could not be saved: {error}")
            }
        }
    }
}

/// Writes a record back over its folder, after saving the folder as it is
/// now. `token` must name the plan the person saw.
pub fn restore(
    store: &CheckpointStore,
    guard: &StoreGuard<'_>,
    checkpoint: &Checkpoint,
    excluded: &[PathBuf],
    token: &str,
) -> Result<RestoreReport, RestoreError> {
    let plan = plan_restore(store, checkpoint, excluded);
    if plan.token != token {
        return Err(RestoreError::Changed);
    }
    let mut report = RestoreReport {
        restored: Vec::new(),
        removed: Vec::new(),
        failed: Vec::new(),
        cannot_restore: plan.cannot_restore.clone(),
        not_covered: plan.not_covered.clone(),
        safety_checkpoint: None,
        complete: false,
    };
    if !plan.files_available || !plan.changes_files() {
        report.complete = plan.files_available && report.cannot_restore.is_empty();
        return Ok(report);
    }
    let safety = store
        .capture_locked(
            guard,
            &CaptureRequest {
                thread_id: &checkpoint.thread_id,
                run_id: &checkpoint.run_id,
                kind: CheckpointKind::Rewind,
                folder: Path::new(&checkpoint.folder),
                excluded,
                conversation: None,
            },
        )
        .map_err(RestoreError::SafetyRecord)?;
    report.safety_checkpoint = Some(safety.id);
    let root = Path::new(&checkpoint.folder);
    let mut removals: Vec<&String> = plan.remove.iter().collect();
    removals.sort_by(|a, b| b.cmp(a));
    for path in removals {
        match remove_file(root, path) {
            Ok(()) => report.removed.push(path.clone()),
            Err(reason) => report.failed.push(FailedPath {
                path: path.clone(),
                reason,
            }),
        }
    }
    for directory in &plan.create_directories {
        if let Err(reason) = safe_path(root, directory)
            .and_then(|path| fs::create_dir_all(&path).map_err(|error| io_reason(&error)))
        {
            report.failed.push(FailedPath {
                path: directory.clone(),
                reason,
            });
        }
    }
    for path in &plan.restore {
        let entry = &checkpoint.files[path];
        match write_entry(store, root, path, entry) {
            Ok(()) => report.restored.push(path.clone()),
            Err(reason) => report.failed.push(FailedPath {
                path: path.clone(),
                reason,
            }),
        }
    }
    for directory in &plan.remove_directories {
        if let Ok(path) = safe_path(root, directory) {
            let _ = fs::remove_dir(path);
        }
    }
    report.complete = report.failed.is_empty() && report.cannot_restore.is_empty();
    Ok(report)
}

/// Writes the thread's saved conversation back as a record found it.
pub fn restore_conversation(
    store: &CheckpointStore,
    session_root: &Path,
    conversation: &super::ConversationEntry,
) -> Result<(), String> {
    let locator = conversation.locator.as_str();
    if locator.is_empty()
        || locator.contains('/')
        || locator.contains('\\')
        || !locator.ends_with(".jsonl")
    {
        return Err("The saved conversation has an invalid name.".into());
    }
    let destination = session_root.join(locator);
    write_object(store, &destination, &conversation.hash, false)
}

/// The absolute path of a record path under `root`, refused when it leaves
/// the folder or a folder on its way is a link.
pub(super) fn safe_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let relative_path = Path::new(relative);
    if relative.is_empty()
        || relative.contains('\\')
        || !relative_path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err("The path leaves the folder.".into());
    }
    let mut current = root.to_owned();
    let mut components = relative_path.components().peekable();
    while let Some(component) = components.next() {
        current.push(component);
        if components.peek().is_none() {
            break;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err("A folder on its path is a link.".into());
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err("A file stands where its folder belongs.".into());
            }
            _ => {}
        }
    }
    Ok(current)
}

pub(super) fn remove_file(root: &Path, relative: &str) -> Result<(), String> {
    let path = safe_path(root, relative)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_reason(&error)),
    }
}

pub(super) fn write_entry(
    store: &CheckpointStore,
    root: &Path,
    relative: &str,
    entry: &FileEntry,
) -> Result<(), String> {
    let path = safe_path(root, relative)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| io_reason(&error))?;
    }
    write_object(store, &path, &entry.hash, entry.executable)
}

/// Writes bytes beside `path` and moves them over it, then reads them back.
pub(super) fn write_bytes(path: &Path, bytes: &[u8], executable: bool) -> Result<(), String> {
    let temporary = temporary_beside(path)?;
    let result = (|| {
        fs::write(&temporary, bytes).map_err(|error| io_reason(&error))?;
        set_mode(&temporary, path, executable)?;
        crate::atomic_file::replace(&temporary, path).map_err(|error| io_reason(&error))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    let written = fs::read(path).map_err(|error| io_reason(&error))?;
    if written != bytes {
        return Err("The file read back differently after the write.".into());
    }
    Ok(())
}

fn write_object(
    store: &CheckpointStore,
    path: &Path,
    hash: &ContentHash,
    executable: bool,
) -> Result<(), String> {
    let mut object = store
        .open_object(hash)
        .map_err(|_| "The saved copy is missing.".to_string())?;
    let temporary = temporary_beside(path)?;
    let result = (|| {
        let mut file = fs::File::create(&temporary).map_err(|error| io_reason(&error))?;
        io::copy(&mut object, &mut file).map_err(|error| io_reason(&error))?;
        file.sync_all().map_err(|error| io_reason(&error))?;
        drop(file);
        set_mode(&temporary, path, executable)?;
        crate::atomic_file::replace(&temporary, path).map_err(|error| io_reason(&error))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    let written = fs::File::open(path)
        .and_then(|mut file| hash_reader(&mut file))
        .map_err(|error| io_reason(&error))?;
    if &written != hash {
        return Err("The file read back differently after the write.".into());
    }
    Ok(())
}

fn temporary_beside(path: &Path) -> Result<PathBuf, String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "The path has no file name.".to_string())?;
    Ok(path.with_file_name(format!(
        ".{name}.muniment-restore-{}",
        uuid::Uuid::new_v4().simple()
    )))
}

/// Gives the new file the old file's mode, with the record's execute bits,
/// or the usual mode for a file that no longer exists.
#[cfg(unix)]
fn set_mode(temporary: &Path, destination: &Path, executable: bool) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let base = fs::metadata(destination)
        .map(|metadata| metadata.permissions().mode() & 0o7777)
        .unwrap_or(0o644);
    let mode = if executable {
        base | ((base & 0o444) >> 2)
    } else {
        base & !0o111
    };
    fs::set_permissions(temporary, fs::Permissions::from_mode(mode))
        .map_err(|error| io_reason(&error))
}

#[cfg(not(unix))]
fn set_mode(_temporary: &Path, _destination: &Path, _executable: bool) -> Result<(), String> {
    Ok(())
}

pub(super) fn io_reason(error: &io::Error) -> String {
    match error.kind() {
        io::ErrorKind::PermissionDenied => "Permission denied.".into(),
        io::ErrorKind::NotFound => "The file or its folder is missing.".into(),
        _ => {
            let text = error.to_string();
            let mut sentence = text
                .split(" (os error")
                .next()
                .unwrap_or(&text)
                .trim()
                .to_owned();
            if let Some(first) = sentence.get(0..1) {
                sentence.replace_range(0..1, &first.to_uppercase());
            }
            if !sentence.ends_with('.') {
                sentence.push('.');
            }
            sentence
        }
    }
}
