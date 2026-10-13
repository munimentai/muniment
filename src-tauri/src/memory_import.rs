//! Copies one assistant's memory files into Home under
//! `memory/imports/<assistant>/`. The webview names the assistant, never a
//! path: the roots come from the same registry the memory scan reads. The walk
//! reads names and sizes first, and a body is read only after the store fits
//! the copy caps.

use std::io::{self, Read};
use std::path::Path;
use std::time::{Duration, Instant};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{ambient_authority, fs::Dir};
use chrono::NaiveDate;
use muniment_core::{
    harness_scan::{self, Assistant, Environment, Platform, Roots, REGISTRY},
    home::{
        ONBOARDING_IMPORT_MAX_DOCUMENT_BYTES, ONBOARDING_IMPORT_MAX_ENTRIES,
        ONBOARDING_IMPORT_MAX_TOTAL_BYTES,
    },
    import_preview::{EntryKind, ExtractedEntry},
    memory_secret::reject_memory_secret,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::home::{publish_import, HomeImportErrorKind};

// The walk holds the memory scan's limits, so it finds the files the scan counted.
const MAX_DEPTH: usize = 6;
const MAX_ENTRIES: usize = 20_000;
const MAX_TIME: Duration = Duration::from_secs(2);
// These names stay out even inside a broadly counted directory, as in the scan.
const NEVER_NAMES: &[&str] = &["auth.json", ".credentials.json", ".env", "*.jsonl"];
// The write plan puts this frontmatter before each body: the source and the date.
const FRONTMATTER_BYTES: usize = "---\nsource: \nimport_date: \n---\n".len() + "2026-01-01".len();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LeftOutReason {
    Secret,
    NotText,
    TooLarge,
    Unreadable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftOut {
    source_name: String,
    reason: LeftOutReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantImportSuccess {
    imported_file_count: usize,
    already_copied_count: usize,
    left_out: Vec<LeftOut>,
    destination: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AssistantImportErrorKind {
    UnknownAssistant,
    NothingFound,
    TooLarge,
    SaveFailed,
    InvalidInput,
}

/// The message names no file text, so no memory body leaves this module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantImportError {
    kind: AssistantImportErrorKind,
    message: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    byte_total: Option<u64>,
}

impl AssistantImportError {
    fn new(kind: AssistantImportErrorKind) -> Self {
        let message = match kind {
            AssistantImportErrorKind::UnknownAssistant => "Muniment does not know this assistant.",
            AssistantImportErrorKind::NothingFound => "No memory files were found.",
            AssistantImportErrorKind::TooLarge => "The memory files are over the copy limits.",
            AssistantImportErrorKind::SaveFailed => "The memory files could not be copied.",
            AssistantImportErrorKind::InvalidInput => "The assistant memory location is invalid.",
        };
        Self {
            kind,
            message,
            file_count: None,
            byte_total: None,
        }
    }

    fn too_large(file_count: usize, byte_total: u64) -> Self {
        Self {
            file_count: Some(file_count),
            byte_total: Some(byte_total),
            ..Self::new(AssistantImportErrorKind::TooLarge)
        }
    }
}

/// Copies the named assistant's memory files into the configured Home.
#[tauri::command]
pub async fn memory_import_assistant(
    assistant_id: String,
) -> Result<AssistantImportSuccess, AssistantImportError> {
    let save_failed = || AssistantImportError::new(AssistantImportErrorKind::SaveFailed);
    let user_home = std::env::home_dir().ok_or_else(save_failed)?;
    let environment: Environment = std::env::vars_os().collect();
    tauri::async_runtime::spawn_blocking(move || {
        // The memory index reads this Home, so the copies reach recall.
        let profile = muniment_runtime::profile_directory().map_err(|_| save_failed())?;
        let home =
            muniment_core::home::initialize_default_home(&profile).map_err(|_| save_failed())?;
        import_assistant(
            &assistant_id,
            &user_home,
            &environment,
            Platform::current(),
            &home,
            chrono::Local::now().date_naive(),
        )
    })
    .await
    .map_err(|_| save_failed())?
}

struct Candidate {
    root: usize,
    relative: String,
    source_name: String,
    bytes: u64,
}

fn import_assistant(
    assistant_id: &str,
    user_home: &Path,
    environment: &Environment,
    platform: Platform,
    home: &Path,
    import_date: NaiveDate,
) -> Result<AssistantImportSuccess, AssistantImportError> {
    let assistant = REGISTRY
        .iter()
        .find(|assistant| assistant.id == assistant_id)
        .ok_or_else(|| AssistantImportError::new(AssistantImportErrorKind::UnknownAssistant))?;
    let roots = harness_scan::resolve_roots(assistant, user_home, environment, platform)
        .map_err(|_| AssistantImportError::new(AssistantImportErrorKind::InvalidInput))?;

    let mut candidates = Vec::new();
    for (index, root) in roots.iter().enumerate() {
        // One root keeps bare names. Several roots prefix each name with its folder.
        let label = if roots.len() == 1 {
            String::new()
        } else {
            format!(
                "{}/",
                root.file_name().unwrap_or_default().to_string_lossy()
            )
        };
        for (relative, bytes) in enumerate_root(assistant, root) {
            candidates.push(Candidate {
                root: index,
                source_name: format!("{label}{relative}"),
                relative,
                bytes,
            });
        }
    }
    if candidates.is_empty() {
        return Err(AssistantImportError::new(
            AssistantImportErrorKind::NothingFound,
        ));
    }

    // Sizes decide the caps before any body is read.
    let mut left_out = Vec::new();
    let mut fitting = Vec::new();
    let mut planned_total = 0u64;
    let mut byte_total = 0u64;
    for candidate in candidates {
        let planned = planned_bytes(assistant, &candidate.source_name, candidate.bytes);
        if planned > ONBOARDING_IMPORT_MAX_DOCUMENT_BYTES as u64 {
            left_out.push(LeftOut {
                source_name: candidate.source_name,
                reason: LeftOutReason::TooLarge,
            });
            continue;
        }
        planned_total = planned_total.saturating_add(planned);
        byte_total = byte_total.saturating_add(candidate.bytes);
        fitting.push(candidate);
    }
    if fitting.len() > ONBOARDING_IMPORT_MAX_ENTRIES
        || planned_total > ONBOARDING_IMPORT_MAX_TOTAL_BYTES as u64
    {
        return Err(AssistantImportError::too_large(fitting.len(), byte_total));
    }

    let mut entries = Vec::with_capacity(fitting.len());
    for candidate in &fitting {
        let text = match read_text(assistant, &roots[candidate.root], &candidate.relative) {
            Ok(text) => text,
            Err(reason) => {
                left_out.push(LeftOut {
                    source_name: candidate.source_name.clone(),
                    reason,
                });
                continue;
            }
        };
        // The write plan refuses an empty body, and an empty file holds no memory.
        if text.is_empty() {
            continue;
        }
        if planned_bytes(assistant, &candidate.source_name, text.len() as u64)
            > ONBOARDING_IMPORT_MAX_DOCUMENT_BYTES as u64
        {
            left_out.push(LeftOut {
                source_name: candidate.source_name.clone(),
                reason: LeftOutReason::TooLarge,
            });
            continue;
        }
        // The rejection carries a byte range, never the text, and nothing logs it.
        if reject_memory_secret(&text).is_err() {
            left_out.push(LeftOut {
                source_name: candidate.source_name.clone(),
                reason: LeftOutReason::Secret,
            });
            continue;
        }
        entries.push(ExtractedEntry {
            source_provenance: format!(
                "{}:v1:{:x}:{}",
                assistant.id,
                Sha256::digest(text.as_bytes()),
                candidate.source_name
            ),
            source_name: candidate.source_name.clone(),
            kind: entry_kind(&candidate.relative),
            text,
        });
    }

    let outcome = publish_import(home, &entries, assistant.id, import_date).map_err(|error| {
        match error.kind() {
            HomeImportErrorKind::TooLarge => {
                AssistantImportError::too_large(fitting.len(), byte_total)
            }
            HomeImportErrorKind::InvalidInput => {
                AssistantImportError::new(AssistantImportErrorKind::InvalidInput)
            }
            _ => AssistantImportError::new(AssistantImportErrorKind::SaveFailed),
        }
    })?;
    Ok(AssistantImportSuccess {
        imported_file_count: outcome.imported,
        already_copied_count: outcome.already_copied,
        left_out,
        destination: format!("memory/imports/{}", assistant.id),
    })
}

/// The bytes one copy takes in Home: the frontmatter and the body.
fn planned_bytes(assistant: &Assistant, source_name: &str, bytes: u64) -> u64 {
    let provenance = format!("{}:v1:{}:{source_name}", assistant.id, "0".repeat(64));
    let quoted = serde_json::to_string(&provenance).map_or(usize::MAX, |value| value.len());
    (FRONTMATTER_BYTES.saturating_add(quoted) as u64).saturating_add(bytes)
}

fn entry_kind(relative: &str) -> EntryKind {
    let extension = Path::new(relative)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("md" | "markdown") => EntryKind::Markdown,
        Some("json") => EntryKind::Json,
        _ => EntryKind::Text,
    }
}

/// Reads one body without following a symlink at any step, up to the
/// document cap and one byte more.
fn read_text(assistant: &Assistant, root: &Path, relative: &str) -> Result<String, LeftOutReason> {
    let open = || -> io::Result<cap_std::fs::File> {
        let mut options = cap_std::fs::OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        if matches!(assistant.roots, Roots::HomeFile(_)) {
            let parent = root.parent().ok_or_else(|| io::Error::other("no parent"))?;
            return Dir::open_ambient_dir(parent, ambient_authority())?
                .open_with(relative, &options);
        }
        let mut directory = open_root(root)?;
        let mut parts: Vec<_> = relative.split('/').collect();
        let name = parts.pop().unwrap_or_default();
        for part in parts {
            directory = directory.open_dir_nofollow(part)?;
        }
        directory.open_with(name, &options)
    };
    let mut bytes = Vec::new();
    open()
        .and_then(|file| {
            file.take(ONBOARDING_IMPORT_MAX_DOCUMENT_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
        })
        .map_err(|_| LeftOutReason::Unreadable)?;
    if bytes.len() > ONBOARDING_IMPORT_MAX_DOCUMENT_BYTES {
        return Err(LeftOutReason::TooLarge);
    }
    if bytes.contains(&0) {
        return Err(LeftOutReason::NotText);
    }
    String::from_utf8(bytes).map_err(|_| LeftOutReason::NotText)
}

/// Opens a root through its parent, so a root replaced by a symlink fails.
fn open_root(root: &Path) -> io::Result<Dir> {
    let (parent, name) = root
        .parent()
        .zip(root.file_name())
        .ok_or_else(|| io::Error::other("no parent"))?;
    Dir::open_ambient_dir(parent, ambient_authority())?.open_dir_nofollow(name)
}

/// Lists the counted files under one root by name and size, the way the
/// memory scan counts them.
fn enumerate_root(assistant: &Assistant, root: &Path) -> Vec<(String, u64)> {
    let mut walk = Walk {
        assistant,
        deadline: Instant::now() + MAX_TIME,
        visited: 0,
        files: Vec::new(),
    };
    let Ok(metadata) = std::fs::symlink_metadata(root) else {
        return Vec::new();
    };
    let file_root = matches!(assistant.roots, Roots::HomeFile(_));
    if (file_root && !metadata.is_file()) || (!file_root && !metadata.is_dir()) {
        return Vec::new();
    }
    if walk.repository_ancestor(root) {
        return Vec::new();
    }
    if metadata.is_dir() {
        if let Ok(directory) = open_root(root) {
            walk.walk(&directory, "", 0);
        }
    } else if let Some(name) = root.file_name().and_then(|name| name.to_str()) {
        walk.file(name, metadata.len());
    }
    walk.files.sort();
    walk.files
}

struct Walk<'a> {
    assistant: &'a Assistant,
    deadline: Instant,
    visited: usize,
    files: Vec<(String, u64)>,
}

impl Walk<'_> {
    fn timed_out(&self) -> bool {
        Instant::now() >= self.deadline
    }

    fn stopped(&self) -> bool {
        self.timed_out() || self.visited >= MAX_ENTRIES
    }

    fn repository_ancestor(&self, root: &Path) -> bool {
        root.parent()
            .into_iter()
            .flat_map(Path::ancestors)
            .any(|parent| {
                self.timed_out()
                    || !matches!(
                        std::fs::symlink_metadata(parent.join(".git")),
                        Err(error) if error.kind() == io::ErrorKind::NotFound
                    )
            })
    }

    fn file(&mut self, relative: &str, bytes: u64) {
        if !excluded(self.assistant, relative)
            && self
                .assistant
                .counted
                .iter()
                .any(|pattern| glob(pattern, relative))
        {
            self.files.push((relative.to_owned(), bytes));
        }
    }

    fn walk(&mut self, directory: &Dir, relative: &str, depth: usize) {
        if self.stopped() {
            return;
        }
        // A .git entry marks a repository or a worktree, and the walk never enters one.
        match directory.symlink_metadata(".git") {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            _ => return,
        }
        let Ok(mut entries) = directory.entries() else {
            return;
        };
        loop {
            if self.stopped() {
                return;
            }
            let Some(entry) = entries.next() else {
                return;
            };
            self.visited += 1;
            let Ok(entry) = entry else {
                continue;
            };
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let path = if relative.is_empty() {
                name.to_owned()
            } else {
                format!("{relative}/{name}")
            };
            if excluded(self.assistant, &path) {
                continue;
            }
            let Ok(metadata) = directory.symlink_metadata(name) else {
                continue;
            };
            if self.timed_out() {
                return;
            }
            if metadata.is_dir() {
                if depth + 1 >= MAX_DEPTH {
                    continue;
                }
                if let Ok(child) = directory.open_dir_nofollow(name) {
                    self.walk(&child, &path, depth + 1);
                }
            } else if metadata.is_file() {
                self.file(&path, metadata.len());
            }
        }
    }
}

// The pattern helpers below match the memory scan's, so both name the same files.
fn excluded(assistant: &Assistant, path: &str) -> bool {
    let path = path.to_ascii_lowercase();
    NEVER_NAMES
        .iter()
        .chain(assistant.never.iter())
        .any(|pattern| {
            if pattern.starts_with("~/") {
                return false;
            }
            let pattern = pattern.trim_end_matches('/');
            path.split('/').any(|part| glob(pattern, part)) || glob(pattern, &path)
        })
}

fn glob(pattern: &str, path: &str) -> bool {
    if let Some(directory) = pattern.strip_suffix('/') {
        return path
            .match_indices('/')
            .any(|(index, _)| glob(directory, &path[..index]));
    }
    let patterns: Vec<_> = pattern.split('/').collect();
    let parts: Vec<_> = path.split('/').collect();
    fn segments(patterns: &[&str], parts: &[&str]) -> bool {
        match patterns.split_first() {
            None => parts.is_empty(),
            Some((&"**", rest)) => (0..=parts.len()).any(|skip| segments(rest, &parts[skip..])),
            Some((pattern, rest)) => parts
                .split_first()
                .is_some_and(|(part, tail)| wildcard(pattern, part) && segments(rest, tail)),
        }
    }
    segments(&patterns, &parts)
}

fn wildcard(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        Some((prefix, suffix)) => name
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.ends_with(suffix)),
        None => pattern == name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "muniment-memory-import-{name}-{}",
                uuid::Uuid::now_v7()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn user(&self) -> PathBuf {
            self.0.join("user")
        }

        fn home(&self) -> PathBuf {
            self.0.join("home")
        }

        fn plant(&self, relative: &str, contents: &[u8]) -> PathBuf {
            let path = self.user().join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, contents).unwrap();
            path
        }

        fn import(&self, id: &str) -> Result<AssistantImportSuccess, AssistantImportError> {
            import_assistant(
                id,
                &self.user(),
                &Environment::new(),
                Platform::Linux,
                &self.home(),
                date(),
            )
        }

        fn copies(&self, id: &str) -> Vec<String> {
            let mut names: Vec<_> = fs::read_dir(self.home().join("memory/imports").join(id))
                .map(|entries| {
                    entries
                        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                        .collect()
                })
                .unwrap_or_default();
            names.sort();
            names
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fn open_up(path: &Path) {
                    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
                    if let Ok(entries) = fs::read_dir(path) {
                        for entry in entries.flatten() {
                            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                                open_up(&entry.path());
                            }
                        }
                    }
                }
                open_up(&self.0);
            }
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 7, 24).unwrap()
    }

    fn left_out(name: &str, reason: LeftOutReason) -> LeftOut {
        LeftOut {
            source_name: name.into(),
            reason,
        }
    }

    // The segments repeat one character each, so no scanner reads this file as a secret.
    fn token() -> String {
        format!("{}.{}.{}", "A".repeat(20), "B".repeat(18), "C".repeat(22))
    }

    #[test]
    fn copies_counted_files_with_source_frontmatter() {
        let root = TempRoot::new("copy");
        root.plant(".claude/CLAUDE.md", b"Prefer short answers.\n");
        root.plant(".claude/rules/style.md", b"Use tabs.\n");

        let result = root.import("claude-code").unwrap();

        assert_eq!(
            result,
            AssistantImportSuccess {
                imported_file_count: 2,
                already_copied_count: 0,
                left_out: Vec::new(),
                destination: "memory/imports/claude-code".into(),
            }
        );
        let copies = root.copies("claude-code");
        assert_eq!(copies.len(), 2);
        assert!(copies[0].starts_with("claude-md-") && copies[0].ends_with(".md"));
        assert!(copies[1].starts_with("rules-style-md-"));
        let copied = fs::read_to_string(
            root.home()
                .join("memory/imports/claude-code")
                .join(&copies[0]),
        )
        .unwrap();
        let digest = format!("{:x}", Sha256::digest(b"Prefer short answers.\n"));
        assert_eq!(
            copied,
            format!(
                "---\nsource: \"claude-code:v1:{digest}:CLAUDE.md\"\nimport_date: 2026-07-24\n---\nPrefer short answers.\n"
            )
        );
    }

    #[test]
    fn never_files_are_not_copied() {
        let root = TempRoot::new("never");
        root.plant(".claude/CLAUDE.md", b"memory\n");
        root.plant(".claude/.credentials.json", b"{}");
        root.plant(".claude/settings.json", b"{}");
        root.plant(".claude/agent-memory/auth.json", b"{}");
        root.plant(".claude/agent-memory/log.jsonl", b"{}");
        root.plant(".claude/agent-memory/.env/notes.md", b"hidden\n");

        let result = root.import("claude-code").unwrap();

        assert_eq!(result.imported_file_count, 1);
        assert!(result.left_out.is_empty());
        assert_eq!(root.copies("claude-code").len(), 1);
    }

    #[test]
    fn a_secret_file_is_left_out_by_name_and_the_secret_appears_nowhere() {
        let root = TempRoot::new("secret");
        root.plant(".claude/CLAUDE.md", b"memory\n");
        root.plant(".claude/rules/keys.md", format!("{}\n", token()).as_bytes());

        let result = root.import("claude-code").unwrap();

        assert_eq!(result.imported_file_count, 1);
        assert_eq!(
            result.left_out,
            vec![left_out("rules/keys.md", LeftOutReason::Secret)]
        );
        let json = serde_json::to_string(&result).unwrap();
        assert!(!json.contains(&token()[..20]));
        for name in root.copies("claude-code") {
            let copied =
                fs::read_to_string(root.home().join("memory/imports/claude-code").join(name))
                    .unwrap();
            assert!(!copied.contains(&token()[..20]));
        }
    }

    #[cfg(unix)]
    #[test]
    fn over_the_caps_returns_too_large_without_reading_a_body() {
        use std::os::unix::fs::PermissionsExt;
        let root = TempRoot::new("caps");
        for index in 0..129 {
            let path = root.plant(&format!(".claude/rules/rule-{index}.md"), b"rule\n");
            fs::set_permissions(path, fs::Permissions::from_mode(0o000)).unwrap();
        }

        let error = root.import("claude-code").unwrap_err();

        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            serde_json::json!({
                "kind": "tooLarge",
                "message": "The memory files are over the copy limits.",
                "fileCount": 129,
                "byteTotal": 129 * 5
            })
        );
        assert!(!root.home().exists());

        let root = TempRoot::new("bytes");
        for index in 0..5 {
            let path = root.plant(&format!(".claude/rules/rule-{index}.md"), &[b'a'; 60_000]);
            fs::set_permissions(path, fs::Permissions::from_mode(0o000)).unwrap();
        }
        let big = root.plant(".claude/CLAUDE.md", &[b'a'; 70_000]);
        fs::set_permissions(big, fs::Permissions::from_mode(0o000)).unwrap();

        let error = root.import("claude-code").unwrap_err();

        assert_eq!(
            (error.kind, error.file_count, error.byte_total),
            (AssistantImportErrorKind::TooLarge, Some(5), Some(300_000))
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_file_over_the_document_cap_is_left_out_without_a_read() {
        use std::os::unix::fs::PermissionsExt;
        let root = TempRoot::new("document");
        root.plant(".claude/CLAUDE.md", b"memory\n");
        let big = root.plant(".claude/rules/big.md", &[b'a'; 65_536]);
        fs::set_permissions(big, fs::Permissions::from_mode(0o000)).unwrap();

        let result = root.import("claude-code").unwrap();

        assert_eq!(result.imported_file_count, 1);
        assert_eq!(
            result.left_out,
            vec![left_out("rules/big.md", LeftOutReason::TooLarge)]
        );
    }

    #[test]
    fn a_second_run_copies_nothing_and_an_edited_source_copies_one_file() {
        let root = TempRoot::new("again");
        root.plant(".claude/CLAUDE.md", b"memory\n");
        root.plant(".claude/rules/style.md", b"Use tabs.\n");
        root.import("claude-code").unwrap();

        let again = root.import("claude-code").unwrap();
        assert_eq!(
            (again.imported_file_count, again.already_copied_count),
            (0, 2)
        );

        root.plant(".claude/rules/style.md", b"Use spaces.\n");
        let edited = root.import("claude-code").unwrap();
        assert_eq!(
            (edited.imported_file_count, edited.already_copied_count),
            (1, 1)
        );
        assert_eq!(root.copies("claude-code").len(), 3);
    }

    #[test]
    fn an_unknown_assistant_is_refused() {
        let root = TempRoot::new("unknown");
        root.plant(".claude/CLAUDE.md", b"memory\n");

        let error = root.import("../.claude").unwrap_err();

        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            serde_json::json!({
                "kind": "unknownAssistant",
                "message": "Muniment does not know this assistant."
            })
        );
        assert!(!root.home().exists());
    }

    #[test]
    fn an_empty_store_answers_nothing_found() {
        let root = TempRoot::new("empty");
        root.plant(".claude/settings.json", b"{}");

        let error = root.import("claude-code").unwrap_err();

        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            serde_json::json!({
                "kind": "nothingFound",
                "message": "No memory files were found."
            })
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_not_followed() {
        use std::os::unix::fs::symlink;
        let root = TempRoot::new("symlink");
        root.plant(".claude/CLAUDE.md", b"memory\n");
        let outside = root.plant("outside/secret.md", b"outside\n");
        symlink(&outside, root.user().join(".claude/rules.md")).unwrap();
        fs::create_dir_all(root.user().join(".claude/rules")).unwrap();
        symlink(&outside, root.user().join(".claude/rules/linked.md")).unwrap();
        symlink(
            outside.parent().unwrap(),
            root.user().join(".claude/agent-memory"),
        )
        .unwrap();

        let result = root.import("claude-code").unwrap();

        assert_eq!(result.imported_file_count, 1);
        assert!(result.left_out.is_empty());
    }

    #[test]
    fn a_file_that_is_not_text_is_left_out() {
        let root = TempRoot::new("binary");
        root.plant(".claude/CLAUDE.md", b"memory\n");
        root.plant(".claude/rules/latin.md", b"caf\xe9\n");
        root.plant(".claude/rules/nul.md", b"a\0b\n");

        let result = root.import("claude-code").unwrap();

        assert_eq!(result.imported_file_count, 1);
        assert_eq!(
            result.left_out,
            vec![
                left_out("rules/latin.md", LeftOutReason::NotText),
                left_out("rules/nul.md", LeftOutReason::NotText),
            ]
        );
    }

    #[test]
    fn several_roots_prefix_each_name_with_its_folder() {
        let root = TempRoot::new("roots");
        root.plant("Documents/Cline/Rules/style.md", b"Use tabs.\n");
        root.plant("Documents/Cline/Workflows/release.md", b"Tag it.\n");

        let result = root.import("cline").unwrap();

        assert_eq!(result.imported_file_count, 2);
        assert_eq!(
            root.copies("cline")
                .iter()
                .map(|name| name.rsplit_once('-').unwrap().0.to_owned())
                .collect::<Vec<_>>(),
            ["rules-style-md", "workflows-release-md"]
        );
    }

    #[test]
    fn the_walk_counts_the_files_the_memory_scan_counts() {
        let root = TempRoot::new("parity");
        for name in [
            ".claude/CLAUDE.md",
            ".claude/rules/a.md",
            ".claude/rules/nested/b.md",
            ".claude/projects/app/memory/c.md",
            ".claude/agent-memory/one/two/three/four/d.md",
            ".claude/agent-memory/one/two/three/four/five/e.md",
            ".claude/agent-memory/repo/f.md",
            ".claude/history.jsonl",
            ".codex/AGENTS.md",
            ".codex/memories/x/y.md",
            ".codex/sessions/s.md",
            ".grok/skills/one/SKILL.md",
            ".grok/skills/one/auth.json",
            ".hermes/skills/tool/run.sh",
            ".config/goose/memory/facts",
            ".config/AGENTS.md",
            "Documents/Cline/Rules/r.md",
            ".cline/skills/s/SKILL.md",
            ".codeium/windsurf/memories/global_rules.md",
        ] {
            root.plant(name, b"memory\n");
        }
        fs::create_dir_all(root.user().join(".claude/agent-memory/repo/.git")).unwrap();

        let report = harness_scan::scan(&root.user(), &Environment::new(), Platform::Linux);

        assert!(report.findings.iter().any(|finding| finding.file_count > 0));
        for finding in report.findings {
            let assistant = REGISTRY
                .iter()
                .find(|assistant| assistant.id == finding.assistant_id)
                .unwrap();
            assert_eq!(
                enumerate_root(assistant, &finding.root).len() as u64,
                finding.file_count,
                "{}",
                finding.assistant_id
            );
        }
    }

    #[test]
    fn success_and_error_serialize_in_camel_case() {
        let success = AssistantImportSuccess {
            imported_file_count: 12,
            already_copied_count: 9,
            left_out: vec![left_out("rules/keys.md", LeftOutReason::Secret)],
            destination: "memory/imports/claude-code".into(),
        };
        assert_eq!(
            serde_json::to_value(success).unwrap(),
            serde_json::json!({
                "importedFileCount": 12,
                "alreadyCopiedCount": 9,
                "leftOut": [{ "sourceName": "rules/keys.md", "reason": "secret" }],
                "destination": "memory/imports/claude-code"
            })
        );
        for (reason, name) in [
            (LeftOutReason::NotText, "notText"),
            (LeftOutReason::TooLarge, "tooLarge"),
            (LeftOutReason::Unreadable, "unreadable"),
        ] {
            assert_eq!(serde_json::to_value(reason).unwrap(), name);
        }
        assert_eq!(
            serde_json::to_value(AssistantImportError::new(
                AssistantImportErrorKind::SaveFailed
            ))
            .unwrap(),
            serde_json::json!({
                "kind": "saveFailed",
                "message": "The memory files could not be copied."
            })
        );
    }
}
