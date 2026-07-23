use cap_std::{ambient_authority, fs::Dir};
use fs2::FileExt;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fmt, fs,
    fs::OpenOptions,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

use crate::{
    import_preview::ExtractedEntry,
    llama::{
        OnboardingTriageReport, OnboardingTriageReportError, ONBOARDING_TRIAGE_MAX_OUTPUT_BYTES,
    },
};

const CONFIG_FILE: &str = "home.json";
const HOME_DIRECTORIES: [&str; 4] = ["memory", "agents", "projects", "sessions"];
const ONBOARDING_IMPORT_MAX_SLUG_BYTES: usize = 48;

/// Maximum number of approved originals in one onboarding write plan.
pub const ONBOARDING_IMPORT_MAX_ENTRIES: usize = 128;
/// Maximum size of any Markdown payload in one onboarding write plan.
pub const ONBOARDING_IMPORT_MAX_DOCUMENT_BYTES: usize = 64 * 1024;
/// Maximum combined size of all Markdown payloads in one onboarding write plan.
pub const ONBOARDING_IMPORT_MAX_TOTAL_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeWrite {
    /// A normalized, `/`-separated path relative to Muniment Home.
    pub relative_path: String,
    /// The complete UTF-8 Markdown file contents.
    pub contents: String,
}

impl HomeWrite {
    pub fn relative_path(&self) -> &Path {
        Path::new(&self.relative_path)
    }

    pub fn bytes(&self) -> &[u8] {
        self.contents.as_bytes()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingHomeWritePlan {
    pub writes: Vec<HomeWrite>,
}

impl OnboardingHomeWritePlan {
    pub fn writes(&self) -> &[HomeWrite] {
        &self.writes
    }
}

/// An explicit import time accepted by the pure plan compiler.
pub trait OnboardingImportTimestamp {
    fn import_date(self) -> Result<chrono::NaiveDate, OnboardingHomeWritePlanError>;
}

impl OnboardingImportTimestamp for chrono::NaiveDate {
    fn import_date(self) -> Result<chrono::NaiveDate, OnboardingHomeWritePlanError> {
        Ok(self)
    }
}

impl OnboardingImportTimestamp for &str {
    fn import_date(self) -> Result<chrono::NaiveDate, OnboardingHomeWritePlanError> {
        chrono::DateTime::parse_from_rfc3339(self)
            .map(|timestamp| timestamp.date_naive())
            .map_err(|_| OnboardingHomeWritePlanError::InvalidTimestamp)
    }
}

/// Stable failure modes for compiling a confirmed onboarding proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardingHomeWritePlanError {
    EmptyInput,
    InvalidTimestamp,
    InvalidReport,
    UnsafeMetadata,
    TooManyEntries,
    DocumentBytesExceeded,
    DestinationCollision,
    TotalBytesExceeded,
}

impl fmt::Display for OnboardingHomeWritePlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::EmptyInput => "The onboarding import input is empty.",
            Self::InvalidTimestamp => "The onboarding import timestamp is invalid.",
            Self::InvalidReport => "The onboarding report is invalid.",
            Self::UnsafeMetadata => "The onboarding import metadata is unsafe.",
            Self::TooManyEntries => "The onboarding import contains too many entries.",
            Self::DocumentBytesExceeded => "An onboarding import document is too large.",
            Self::DestinationCollision => "The onboarding import destinations collide.",
            Self::TotalBytesExceeded => "The onboarding import plan is too large.",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for OnboardingHomeWritePlanError {}

/// Compiles complete, bounded Markdown writes without accessing the filesystem.
pub fn compile_onboarding_home_write_plan<T: OnboardingImportTimestamp>(
    report: &OnboardingTriageReport,
    approved_entries: &[ExtractedEntry],
    import_timestamp: T,
) -> Result<OnboardingHomeWritePlan, OnboardingHomeWritePlanError> {
    let import_date = import_timestamp
        .import_date()?
        .format("%Y-%m-%d")
        .to_string();
    if report.user_type.trim().is_empty()
        || report.proposed_home_layout.trim().is_empty()
        || report.starter_agents.is_empty()
        || report
            .starter_agents
            .iter()
            .any(|agent| agent.trim().is_empty())
    {
        return Err(OnboardingHomeWritePlanError::EmptyInput);
    }
    if !(2..=3).contains(&report.starter_agents.len()) {
        return Err(OnboardingHomeWritePlanError::InvalidReport);
    }
    let report_len = "## User type\n\n\n\n## Proposed Home layout\n\n\n\n## Starter agents\n\n\n"
        .len()
        .checked_add(report.user_type.len())
        .and_then(|length| length.checked_add(report.proposed_home_layout.len()))
        .and_then(|length| {
            report
                .starter_agents
                .iter()
                .try_fold(length, |length, agent| {
                    length.checked_add(2)?.checked_add(agent.len())
                })
        })
        .and_then(|length| length.checked_add(report.starter_agents.len() - 1))
        .ok_or(OnboardingHomeWritePlanError::InvalidReport)?;
    if report_len > ONBOARDING_TRIAGE_MAX_OUTPUT_BYTES {
        return Err(OnboardingHomeWritePlanError::InvalidReport);
    }
    let report_markdown = format!(
        "## User type\n\n{}\n\n## Proposed Home layout\n\n{}\n\n## Starter agents\n\n{}\n",
        report.user_type,
        report.proposed_home_layout,
        report
            .starter_agents
            .iter()
            .map(|agent| format!("- {agent}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    match OnboardingTriageReport::parse(&report_markdown) {
        Ok(parsed) if parsed == *report => {}
        Err(OnboardingTriageReportError::EmptySection) => {
            return Err(OnboardingHomeWritePlanError::EmptyInput);
        }
        _ => return Err(OnboardingHomeWritePlanError::InvalidReport),
    }
    if approved_entries.is_empty()
        || approved_entries.iter().any(|entry| {
            entry.source_name.trim().is_empty()
                || entry.source_provenance.trim().is_empty()
                || entry.text.is_empty()
        })
    {
        return Err(OnboardingHomeWritePlanError::EmptyInput);
    }
    if approved_entries.len() > ONBOARDING_IMPORT_MAX_ENTRIES {
        return Err(OnboardingHomeWritePlanError::TooManyEntries);
    }

    let mut destinations =
        Vec::with_capacity(1 + report.starter_agents.len() + approved_entries.len());
    destinations.push(format!("memory/onboarding-report-{import_date}.md"));
    let mut total_bytes = 0;
    add_payload_len(&mut total_bytes, report_markdown.len())?;

    for (index, agent) in report.starter_agents.iter().enumerate() {
        reject_unsafe_metadata(agent)?;
        let mut identity = agent.as_bytes().to_vec();
        identity.extend_from_slice(&(index as u64).to_be_bytes());
        destinations.push(format!(
            "agents/agent-{}-{}.md",
            safe_slug(agent),
            short_digest(&identity)
        ));
        add_payload_len(
            &mut total_bytes,
            3usize
                .checked_add(agent.len())
                .ok_or(OnboardingHomeWritePlanError::DocumentBytesExceeded)?,
        )?;
    }

    for (index, entry) in approved_entries.iter().enumerate() {
        reject_unsafe_metadata(&entry.source_name)?;
        reject_unsafe_metadata(&entry.source_provenance)?;
        let mut identity = entry.source_provenance.as_bytes().to_vec();
        identity.push(0);
        identity.extend_from_slice(entry.source_name.as_bytes());
        identity.extend_from_slice(&(index as u64).to_be_bytes());
        destinations.push(format!(
            "memory/imports/{import_date}-{}-{}.md",
            safe_slug(&entry.source_name),
            short_digest(&identity)
        ));
        let provenance_len = json_string_len(&entry.source_provenance)?;
        let frontmatter_len = "---\nsource: \nimport_date: \n---\n"
            .len()
            .checked_add(provenance_len)
            .and_then(|length| length.checked_add(import_date.len()))
            .ok_or(OnboardingHomeWritePlanError::DocumentBytesExceeded)?;
        add_payload_len(
            &mut total_bytes,
            frontmatter_len
                .checked_add(entry.text.len())
                .ok_or(OnboardingHomeWritePlanError::DocumentBytesExceeded)?,
        )?;
    }

    let mut unique_destinations = BTreeSet::new();
    if destinations
        .iter()
        .any(|path| !unique_destinations.insert(path.clone()))
    {
        return Err(OnboardingHomeWritePlanError::DestinationCollision);
    }

    let mut writes = Vec::with_capacity(destinations.len());
    let mut destinations = destinations.into_iter();
    writes.push(HomeWrite {
        relative_path: destinations.next().expect("report destination exists"),
        contents: report_markdown,
    });
    for agent in &report.starter_agents {
        writes.push(HomeWrite {
            relative_path: destinations.next().expect("agent destination exists"),
            contents: format!("# {agent}\n"),
        });
    }
    for entry in approved_entries {
        let mut contents = format!(
            "---\nsource: {}\nimport_date: {}\n---\n",
            yaml_string(&entry.source_provenance),
            import_date
        );
        contents.push_str(&entry.text);
        writes.push(HomeWrite {
            relative_path: destinations.next().expect("entry destination exists"),
            contents,
        });
    }
    Ok(OnboardingHomeWritePlan { writes })
}

fn add_payload_len(
    total: &mut usize,
    payload_len: usize,
) -> Result<(), OnboardingHomeWritePlanError> {
    if payload_len > ONBOARDING_IMPORT_MAX_DOCUMENT_BYTES {
        return Err(OnboardingHomeWritePlanError::DocumentBytesExceeded);
    }
    *total = total
        .checked_add(payload_len)
        .ok_or(OnboardingHomeWritePlanError::TotalBytesExceeded)?;
    if *total > ONBOARDING_IMPORT_MAX_TOTAL_BYTES {
        return Err(OnboardingHomeWritePlanError::TotalBytesExceeded);
    }
    Ok(())
}

fn reject_unsafe_metadata(value: &str) -> Result<(), OnboardingHomeWritePlanError> {
    if value.chars().any(|character| character == '\0') {
        Err(OnboardingHomeWritePlanError::UnsafeMetadata)
    } else {
        Ok(())
    }
}

fn short_digest(value: &[u8]) -> String {
    let digest = format!("{:x}", Sha256::digest(value));
    digest[..12].to_owned()
}

fn json_string_len(value: &str) -> Result<usize, OnboardingHomeWritePlanError> {
    struct ByteCounter(usize);

    impl io::Write for ByteCounter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .ok_or_else(|| io::Error::other("serialized string length overflow"))?;
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let mut counter = ByteCounter(0);
    serde_json::to_writer(&mut counter, value)
        .map_err(|_| OnboardingHomeWritePlanError::TotalBytesExceeded)?;
    Ok(counter.0)
}

fn safe_slug(input: &str) -> String {
    let mut slug = String::new();
    let mut separator = false;
    for character in input.chars() {
        if character.is_ascii_alphanumeric() {
            if slug.len() == ONBOARDING_IMPORT_MAX_SLUG_BYTES {
                break;
            }
            slug.push(character.to_ascii_lowercase());
            separator = false;
        } else if !slug.is_empty() && !separator && slug.len() < ONBOARDING_IMPORT_MAX_SLUG_BYTES {
            slug.push('-');
            separator = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "item".to_owned()
    } else {
        slug
    }
}

fn yaml_string(value: &str) -> String {
    serde_json::to_string(value).expect("serializing a string cannot fail")
}

#[cfg(test)]
mod onboarding_write_plan_tests {
    use super::*;
    use crate::import_preview::EntryKind;

    const TIMESTAMP: &str = "2026-07-21T15:04:05Z";

    fn report() -> OnboardingTriageReport {
        OnboardingTriageReport {
            user_type: "Independent researcher".to_owned(),
            proposed_home_layout: "Organize work by topic.".to_owned(),
            starter_agents: vec!["Research Scout".to_owned(), "Writing Partner".to_owned()],
        }
    }

    fn entry(name: &str, provenance: &str, text: &str) -> ExtractedEntry {
        ExtractedEntry {
            source_name: name.to_owned(),
            kind: EntryKind::Markdown,
            text: text.to_owned(),
            source_provenance: provenance.to_owned(),
        }
    }

    #[test]
    fn plans_are_ordered_serializable_and_byte_deterministic() {
        let entries = vec![entry("notes.md", "export:notes.md", "Hello\n")];
        let first = compile_onboarding_home_write_plan(&report(), &entries, TIMESTAMP).unwrap();
        let second = compile_onboarding_home_write_plan(&report(), &entries, TIMESTAMP).unwrap();

        assert_eq!(first, second);
        assert_eq!(
            serde_json::to_vec(&first).unwrap(),
            serde_json::to_vec(&second).unwrap()
        );
        assert_eq!(
            first.writes[0].relative_path,
            "memory/onboarding-report-2026-07-21.md"
        );
        assert!(first.writes[1].relative_path.starts_with("agents/"));
        assert!(first.writes[2].relative_path.starts_with("agents/"));
        assert!(first.writes[3].relative_path.starts_with("memory/imports/"));
    }

    #[test]
    fn duplicate_and_path_like_names_are_safely_disambiguated() {
        let mut report = report();
        report.starter_agents = vec!["CON/../Scout".to_owned(), "con\\..\\scout".to_owned()];
        let entries = vec![
            entry("../../CON.md", "export:first", "one"),
            entry("../../CON.md", "export:second", "two"),
        ];
        let plan = compile_onboarding_home_write_plan(&report, &entries, TIMESTAMP).unwrap();
        let mut case_folded = BTreeSet::new();

        for write in &plan.writes {
            assert!(
                write.relative_path.starts_with("memory/")
                    || write.relative_path.starts_with("agents/")
            );
            assert!(!write.relative_path.contains(".."));
            assert!(!write.relative_path.contains('\\'));
            assert!(case_folded.insert(write.relative_path.to_ascii_lowercase()));
        }
    }

    #[test]
    fn frontmatter_is_escaped_and_original_body_is_verbatim() {
        let original = "Unicode: café 🦀\n\n---\nmultiline\r\n";
        let provenance = "archive.zip\n---\nsource: forged\nquote: \"yes\"";
        let plan = compile_onboarding_home_write_plan(
            &report(),
            &[entry("memory.md", provenance, original)],
            TIMESTAMP,
        )
        .unwrap();
        let imported = &plan.writes.last().unwrap().contents;
        let (_, body) = imported
            .strip_prefix("---\n")
            .unwrap()
            .split_once("\n---\n")
            .unwrap();

        assert_eq!(body.as_bytes(), original.as_bytes());
        assert!(imported.starts_with("---\nsource: \"archive.zip\\n---\\nsource: forged"));
        assert!(imported.contains("\nimport_date: 2026-07-21\n---\n"));
    }

    #[test]
    fn rejects_invalid_timestamp_and_unsafe_metadata() {
        let entries = [entry("notes.md", "export:notes.md", "text")];
        assert_eq!(
            compile_onboarding_home_write_plan(&report(), &entries, "2026-07-21"),
            Err(OnboardingHomeWritePlanError::InvalidTimestamp)
        );
        assert_eq!(
            compile_onboarding_home_write_plan(
                &report(),
                &[entry("notes.md", "bad\0source", "text")],
                TIMESTAMP,
            ),
            Err(OnboardingHomeWritePlanError::UnsafeMetadata)
        );
    }

    #[test]
    fn enforces_entry_document_and_aggregate_bounds_without_truncation() {
        let too_many = (0..=ONBOARDING_IMPORT_MAX_ENTRIES)
            .map(|index| entry(&format!("{index}.md"), &format!("source:{index}"), "x"))
            .collect::<Vec<_>>();
        assert_eq!(
            compile_onboarding_home_write_plan(&report(), &too_many, TIMESTAMP),
            Err(OnboardingHomeWritePlanError::TooManyEntries)
        );

        let oversized = "é".repeat(ONBOARDING_IMPORT_MAX_DOCUMENT_BYTES / 2);
        assert_eq!(
            compile_onboarding_home_write_plan(
                &report(),
                &[entry("large.md", "source:large", &oversized)],
                TIMESTAMP,
            ),
            Err(OnboardingHomeWritePlanError::DocumentBytesExceeded)
        );

        let aggregate = (0..5)
            .map(|index| {
                entry(
                    &format!("{index}.md"),
                    &format!("source:{index}"),
                    &"x".repeat(60_000),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            compile_onboarding_home_write_plan(&report(), &aggregate, TIMESTAMP),
            Err(OnboardingHomeWritePlanError::TotalBytesExceeded)
        );
    }
}

#[derive(Debug)]
pub struct HomeError {
    message: &'static str,
    source: Option<io::Error>,
}

impl HomeError {
    fn io(message: &'static str, source: io::Error) -> Self {
        Self {
            message,
            source: Some(source),
        }
    }

    fn invalid(message: &'static str) -> Self {
        Self {
            message,
            source: None,
        }
    }
}

impl fmt::Display for HomeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for HomeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| source as &(dyn std::error::Error + 'static))
    }
}

#[derive(Deserialize, Serialize)]
struct HomeConfig {
    location: PathBuf,
}

pub fn configured_home(config_dir: &Path) -> Result<Option<PathBuf>, HomeError> {
    let bytes = match fs::read(config_dir.join(CONFIG_FILE)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(HomeError::io(
                "The saved Home location could not be read.",
                error,
            ))
        }
    };
    let config: HomeConfig = serde_json::from_slice(&bytes)
        .map_err(|_| HomeError::invalid("The saved Home location is invalid."))?;
    validate_home(&config.location)?;
    Ok(Some(config.location))
}

pub fn confirm_home(config_dir: &Path, home: &Path) -> Result<(), HomeError> {
    validate_home(home)?;
    validate_config_location(config_dir, home)?;
    scaffold_home(home)?;
    persist_home(config_dir, home)
}

fn validate_config_location(config_dir: &Path, home: &Path) -> Result<(), HomeError> {
    let config_dir = normalized_absolute(config_dir)?;
    let home = normalized_absolute(home)?;
    if config_dir.starts_with(&home) {
        return Err(HomeError::invalid(
            "Muniment Home cannot contain the app configuration folder.",
        ));
    }
    Ok(())
}

fn normalized_absolute(path: &Path) -> Result<PathBuf, HomeError> {
    if !path.is_absolute() {
        return Err(HomeError::invalid("The folder location is invalid."));
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(HomeError::invalid("The folder location is invalid."));
                }
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    let mut existing = normalized.as_path();
    let mut missing = Vec::new();
    loop {
        match fs::canonicalize(existing) {
            Ok(mut resolved) => {
                for component in missing.iter().rev() {
                    resolved.push(component);
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing.push(
                    existing
                        .file_name()
                        .ok_or_else(|| HomeError::invalid("The folder location is invalid."))?,
                );
                existing = existing
                    .parent()
                    .ok_or_else(|| HomeError::invalid("The folder location is invalid."))?;
            }
            Err(error) => {
                return Err(HomeError::io(
                    "The folder location could not be inspected.",
                    error,
                ))
            }
        }
    }
}

pub fn scaffold_home(home: &Path) -> Result<(), HomeError> {
    create_visible_directory(home)?;
    for name in HOME_DIRECTORIES {
        create_visible_directory(&home.join(name))?;
    }
    Ok(())
}

/// Persists a confirmed onboarding plan without replacing existing Home content.
///
/// `authentication_root` must be app-controlled storage outside `home` and its
/// parent so a writer to Home cannot forge a recovery journal.
pub fn persist_onboarding_home_write_plan(
    authentication_root: &Path,
    home: &Path,
    plan: &OnboardingHomeWritePlan,
) -> Result<(), HomeError> {
    persist_onboarding_home_write_plan_with_hook(
        authentication_root,
        home,
        plan,
        &mut |_, _| Ok(()),
    )
}

/// Stable boundaries exposed for deterministic filesystem transaction tests.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardingPersistHook {
    CreateTransactionOwner,
    SyncTransactionOwner,
    CreateTransaction,
    WriteJournal,
    SyncJournal,
    ReplaceJournal,
    CreateDirectory,
    WriteCommitDecision,
    SyncCommitDecision,
    SyncCommitDirectory,
    RetireAnchor,
    RetireDirectory,
    RetireJournal,
    RetireTransaction,
    RetireTransactionOwner,
    WritePayload,
    SyncPayload,
    AfterStaging,
    AfterPublish,
    BeforeTemporaryRemoval,
    BeforeDirectorySync,
    BeforeRollback,
    Publish,
    RemoveTemporary,
    SyncDirectory,
    RollbackFile,
    RollbackDirectory,
    RollbackSync,
}

#[derive(Serialize, Deserialize)]
struct OnboardingRecoveryManifest {
    committed: bool,
    entries: Vec<OnboardingRecoveryEntry>,
    created_directories: Vec<OnboardingRecoveryDirectory>,
}

#[derive(Serialize, Deserialize)]
struct AuthenticatedOnboardingRecoveryManifest {
    manifest: OnboardingRecoveryManifest,
    authentication: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct OnboardingRecoveryEntry {
    parent: PathBuf,
    destination: std::ffi::OsString,
    temporary: std::ffi::OsString,
    anchor: std::ffi::OsString,
    digest: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct OnboardingRecoveryDirectory {
    path: PathBuf,
    identity: Option<FileIdentity>,
    staged: std::ffi::OsString,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct FileIdentity {
    first: u64,
    second: u64,
}

/// Test seam for simulating changes and transient failures at transaction boundaries.
#[doc(hidden)]
pub fn persist_onboarding_home_write_plan_with_hook(
    authentication_root: &Path,
    home: &Path,
    plan: &OnboardingHomeWritePlan,
    hook: &mut dyn FnMut(OnboardingPersistHook, usize) -> io::Result<()>,
) -> Result<(), HomeError> {
    validate_home(home)?;
    let authentication_metadata = fs::symlink_metadata(authentication_root).map_err(|error| {
        HomeError::io(
            "The onboarding import authentication storage could not be inspected.",
            error,
        )
    })?;
    if authentication_metadata.file_type().is_symlink() || !authentication_metadata.is_dir() {
        return Err(HomeError::invalid(
            "The onboarding import authentication storage is unsafe.",
        ));
    }
    #[cfg(windows)]
    let authentication_metadata = Dir::open_ambient_dir(authentication_root, ambient_authority())
        .and_then(|directory| directory.metadata("."))
        .map_err(|error| {
            HomeError::io(
                "The onboarding import authentication storage could not be inspected.",
                error,
            )
        })?;
    let authentication_root = normalized_absolute(authentication_root)?;
    let home_parent = normalized_absolute(
        home.parent()
            .ok_or_else(|| HomeError::invalid("The onboarding import Home is invalid."))?,
    )?;
    if authentication_root.starts_with(&home_parent) {
        return Err(HomeError::invalid(
            "The onboarding import authentication storage is unsafe.",
        ));
    }
    let metadata = fs::symlink_metadata(home).map_err(|error| {
        HomeError::io("The onboarding import Home could not be inspected.", error)
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(HomeError::invalid(
            "The onboarding import Home is not a directory.",
        ));
    }
    #[cfg(windows)]
    let metadata = Dir::open_ambient_dir(home, ambient_authority())
        .and_then(|directory| directory.metadata("."))
        .map_err(|error| {
            HomeError::io("The onboarding import Home could not be inspected.", error)
        })?;
    // Keep validation and locking behavior identical to the public entry point.
    let mut destinations = BTreeSet::new();
    if plan.writes.is_empty() || plan.writes.len() > ONBOARDING_IMPORT_MAX_ENTRIES + 4 {
        return Err(HomeError::invalid("The onboarding import plan is invalid."));
    }
    let mut total = 0usize;
    for write in &plan.writes {
        validate_onboarding_destination(&write.relative_path)?;
        if !destinations.insert(write.relative_path.clone()) {
            return Err(HomeError::invalid(
                "The onboarding import destinations collide.",
            ));
        }
        total = total
            .checked_add(write.bytes().len())
            .ok_or_else(|| HomeError::invalid("The onboarding import plan is too large."))?;
        if write.bytes().len() > ONBOARDING_IMPORT_MAX_DOCUMENT_BYTES
            || total > ONBOARDING_IMPORT_MAX_TOTAL_BYTES
        {
            return Err(HomeError::invalid(
                "The onboarding import plan is too large.",
            ));
        }
    }
    let parent_path = home.parent().unwrap();
    let home_name = home.file_name().unwrap();
    let parent = Dir::open_ambient_dir(parent_path, ambient_authority()).map_err(|error| {
        HomeError::io(
            "The onboarding import Home parent could not be opened.",
            error,
        )
    })?;
    let authentication =
        Dir::open_ambient_dir(&authentication_root, ambient_authority()).map_err(|error| {
            HomeError::io(
                "The onboarding import authentication storage could not be opened.",
                error,
            )
        })?;
    if !same_home_file(
        &authentication_metadata,
        &authentication.metadata(".").map_err(|error| {
            HomeError::io(
                "The onboarding import authentication storage could not be inspected.",
                error,
            )
        })?,
    ) {
        return Err(HomeError::invalid(
            "The onboarding import authentication storage changed while it was opened.",
        ));
    }
    let directory = parent
        .open_dir(home_name)
        .map_err(|error| HomeError::io("The onboarding import Home could not be opened.", error))?;
    if !same_home_file(
        &metadata,
        &directory.metadata(".").map_err(|error| {
            HomeError::io("The onboarding import Home could not be inspected.", error)
        })?,
    ) {
        return Err(HomeError::invalid(
            "The onboarding import Home changed while it was opened.",
        ));
    }
    let mut options = cap_std::fs::OpenOptions::new();
    options.create(true).read(true).write(true);
    let lock = directory
        .open_with(".onboarding-import.lock", &options)
        .map_err(|error| HomeError::io("The onboarding import could not be locked.", error))?
        .into_std();
    lock.lock_exclusive()
        .map_err(|error| HomeError::io("The onboarding import could not be locked.", error))?;
    recover_onboarding_transactions(&authentication, &directory, hook).map_err(|error| {
        HomeError::io(
            "A previous onboarding import could not be recovered.",
            error,
        )
    })?;
    persist_onboarding_plan_locked(&authentication, &parent, home_name, &directory, plan, hook)
}

fn validate_onboarding_destination(relative: &str) -> Result<(), HomeError> {
    let path = Path::new(relative);
    let canonical = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    if relative.is_empty()
        || relative.contains('\\')
        || path.is_absolute()
        || path.extension().and_then(|value| value.to_str()) != Some("md")
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || !HOME_DIRECTORIES
            .iter()
            .any(|directory| path.starts_with(directory))
        || path.components().count() < 2
        || canonical != relative
    {
        return Err(HomeError::invalid(
            "An onboarding import destination is unsafe.",
        ));
    }
    Ok(())
}

fn open_anchored_directory(home: &Dir, path: &Path) -> io::Result<Dir> {
    let mut current = home.try_clone()?;
    for component in path.components() {
        let name = component.as_os_str();
        let metadata = current.symlink_metadata(name)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unsafe import ancestor",
            ));
        }
        let next = current.open_dir(name)?;
        if !same_file(&metadata, &next.metadata(".")?) {
            return Err(io::Error::other("import ancestor changed"));
        }
        current = next;
    }
    Ok(current)
}

fn recover_onboarding_transactions(
    authentication_root: &Dir,
    home: &Dir,
    hook: &mut dyn FnMut(OnboardingPersistHook, usize) -> io::Result<()>,
) -> io::Result<()> {
    let all_names = home
        .entries()?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .collect::<Vec<_>>();
    let names = all_names
        .iter()
        .filter(|name| {
            let name = name.to_string_lossy();
            name.starts_with(".onboarding-import-")
                && (name.ends_with(".txn") || name.ends_with(".retired"))
        })
        .cloned()
        .collect::<Vec<_>>();
    for name in &names {
        let owner_name = onboarding_transaction_owner_name(name);
        authenticate_onboarding_transaction(authentication_root, home, &owner_name)?;
    }
    for name in names {
        let owner_name = onboarding_transaction_owner_name(&name);
        let metadata = home.symlink_metadata(&name)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unsafe onboarding transaction",
            ));
        }
        let transaction = match home.open_dir(&name) {
            Ok(directory) => directory,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if !same_file(&metadata, &transaction.metadata(".")?) {
            return Err(io::Error::other("onboarding transaction changed"));
        }
        if name.to_string_lossy().ends_with(".retired") {
            hook(OnboardingPersistHook::RetireTransaction, 0)?;
            home.remove_dir(&name)?;
            hook(OnboardingPersistHook::RetireTransactionOwner, 0)?;
            home.remove_file(&owner_name)?;
            authentication_root.remove_file(&owner_name)?;
            home.open(".")?.sync_all()?;
            authentication_root.open(".")?.sync_all()?;
            continue;
        }
        let committed = match transaction.symlink_metadata("committed") {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => true,
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unsafe commit marker",
                ))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => return Err(error),
        };
        let manifest_metadata = match transaction.symlink_metadata("manifest.json") {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                Some(metadata)
            }
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unsafe onboarding manifest",
                ))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        let manifest_file = match manifest_metadata {
            Some(metadata) => {
                let file = transaction.open("manifest.json")?;
                if !same_file(&metadata, &file.metadata()?) {
                    return Err(io::Error::other("onboarding manifest changed"));
                }
                file
            }
            None => {
                // The durable authenticated owner predates the directory, so every
                // partially-created initial journal is recognizable here.
                match transaction.remove_file("manifest.next") {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
                hook(OnboardingPersistHook::RetireTransaction, 0)?;
                home.remove_dir(&name)?;
                hook(OnboardingPersistHook::RetireTransactionOwner, 0)?;
                home.remove_file(&owner_name)?;
                authentication_root.remove_file(&owner_name)?;
                home.open(".")?.sync_all()?;
                authentication_root.open(".")?.sync_all()?;
                continue;
            }
        };
        // Only the atomically installed manifest is authoritative. A failed
        // replacement may leave manifest.next empty or partially written.
        match transaction.remove_file("manifest.next") {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let authenticated: AuthenticatedOnboardingRecoveryManifest =
            serde_json::from_reader(manifest_file)?;
        let authentication_key =
            authenticate_onboarding_transaction(authentication_root, home, &owner_name)?;
        authenticate_onboarding_manifest(&authenticated, &authentication_key)?;
        let manifest = authenticated.manifest;
        for (index, entry) in manifest.entries.iter().enumerate() {
            let destination = entry.parent.join(&entry.destination);
            if destination.to_str().is_none()
                || destination
                    .to_str()
                    .map(validate_onboarding_destination)
                    .transpose()
                    .is_err()
                || entry.temporary != std::ffi::OsString::from(format!("payload-{index}"))
                || entry.anchor != entry.temporary
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unsafe onboarding manifest paths",
                ));
            }
        }
        for (index, directory) in manifest.created_directories.iter().enumerate() {
            if directory
                .path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
                || !HOME_DIRECTORIES
                    .iter()
                    .any(|allowed| directory.path.starts_with(allowed))
                || directory.staged != std::ffi::OsString::from(format!("directory-{index}"))
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unsafe onboarding directory manifest",
                ));
            }
        }
        let committed = committed || manifest.committed;
        for (index, entry) in manifest.entries.iter().enumerate() {
            if committed {
                break;
            }
            let directory = match open_anchored_directory(home, &entry.parent) {
                Ok(directory) => directory,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            let anchor_metadata = match transaction.symlink_metadata(&entry.anchor) {
                Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                    metadata
                }
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "unsafe payload anchor",
                    ))
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            let anchor = transaction.open(&entry.anchor)?;
            if !same_file(&anchor_metadata, &anchor.metadata()?) {
                return Err(io::Error::other("payload anchor changed"));
            }
            let mut payload = anchor.try_clone()?;
            let mut digest = Sha256::new();
            io::copy(&mut payload, &mut digest)?;
            if digest.finalize().as_slice() != entry.digest {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unauthenticated payload anchor",
                ));
            }
            let identity = anchor.metadata()?;
            for child in directory.entries()? {
                let child = child?;
                let child_name = child.file_name();
                if let Ok(metadata) = directory.symlink_metadata(&child_name) {
                    if metadata.is_file() && same_file(&metadata, &identity) {
                        hook(OnboardingPersistHook::RollbackFile, index)?;
                        directory.remove_file(&child_name)?;
                    }
                }
            }
            directory.open(".")?.sync_all()?;
        }
        for directory in manifest.created_directories.iter().rev() {
            if committed {
                break;
            }
            let Some(parent_path) = directory.path.parent() else {
                continue;
            };
            let Some(directory_name) = directory.path.file_name() else {
                continue;
            };
            let parent = match open_anchored_directory(home, parent_path) {
                Ok(parent) => parent,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            match parent.symlink_metadata(directory_name) {
                Ok(metadata)
                    if metadata.is_dir() && directory.identity == file_identity(&metadata) =>
                {
                    hook(OnboardingPersistHook::RollbackDirectory, 0)?;
                    match parent.remove_dir(directory_name) {
                        Ok(()) => {}
                        Err(error) if error.kind() == io::ErrorKind::DirectoryNotEmpty => {}
                        Err(error) => return Err(error),
                    }
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            match transaction.remove_dir(&directory.staged) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) if error.kind() == io::ErrorKind::DirectoryNotEmpty => {}
                Err(error) => return Err(error),
            }
        }
        for entry in &manifest.entries {
            match transaction.remove_file(&entry.anchor) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        match transaction.remove_file("manifest.next") {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        match transaction.remove_file("committed") {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        transaction.remove_file("manifest.json")?;
        let retired_name =
            std::ffi::OsString::from(name.to_string_lossy().replace(".txn", ".retired"));
        home.rename(&name, home, &retired_name)?;
        hook(OnboardingPersistHook::RetireTransaction, 0)?;
        home.remove_dir(&retired_name)?;
        hook(OnboardingPersistHook::RetireTransactionOwner, 0)?;
        home.remove_file(&owner_name)?;
        authentication_root.remove_file(&owner_name)?;
        hook(OnboardingPersistHook::RollbackSync, 0)?;
        home.open(".")?.sync_all()?;
        authentication_root.open(".")?.sync_all()?;
    }
    for name in all_names {
        let value = name.to_string_lossy();
        if !value.starts_with(".onboarding-import-") || !value.ends_with(".owner") {
            continue;
        }
        match authenticate_onboarding_transaction(authentication_root, home, &name) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        }
        let stem = value.trim_end_matches(".owner");
        let transaction = std::ffi::OsString::from(format!("{stem}.txn"));
        let retired = std::ffi::OsString::from(format!("{stem}.retired"));
        let transaction_exists = match home.symlink_metadata(&transaction) {
            Ok(_) => true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => return Err(error),
        };
        let retired_exists = match home.symlink_metadata(&retired) {
            Ok(_) => true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => return Err(error),
        };
        if !transaction_exists && !retired_exists {
            hook(OnboardingPersistHook::RetireTransactionOwner, 0)?;
            home.remove_file(&name)?;
            authentication_root.remove_file(&name)?;
            home.open(".")?.sync_all()?;
            authentication_root.open(".")?.sync_all()?;
        }
    }
    Ok(())
}

fn onboarding_transaction_owner_name(name: &std::ffi::OsStr) -> std::ffi::OsString {
    let name = name.to_string_lossy();
    let stem = name
        .strip_suffix(".txn")
        .or_else(|| name.strip_suffix(".retired"))
        .unwrap_or(&name);
    std::ffi::OsString::from(format!("{stem}.owner"))
}

fn authenticate_onboarding_transaction(
    authentication_root: &Dir,
    home: &Dir,
    owner_name: &std::ffi::OsStr,
) -> io::Result<Vec<u8>> {
    let owner_metadata = home.symlink_metadata(owner_name)?;
    let key_metadata = authentication_root.symlink_metadata(owner_name)?;
    if owner_metadata.file_type().is_symlink()
        || !owner_metadata.is_file()
        || key_metadata.file_type().is_symlink()
        || !key_metadata.is_file()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "foreign onboarding transaction",
        ));
    }
    let mut key = Vec::new();
    authentication_root
        .open(owner_name)?
        .read_to_end(&mut key)?;
    let mut proof = Vec::new();
    home.open(owner_name)?.read_to_end(&mut proof)?;
    let mut authentication = Hmac::<Sha256>::new_from_slice(&key)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid owner key"))?;
    authentication.update(owner_name.to_string_lossy().as_bytes());
    authentication.verify_slice(&proof).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "foreign onboarding transaction",
        )
    })?;
    Ok(key)
}

fn write_onboarding_manifest(
    transaction: &Dir,
    manifest: &OnboardingRecoveryManifest,
    authentication_key: &[u8],
    _initial: bool,
    hook: &mut dyn FnMut(OnboardingPersistHook, usize) -> io::Result<()>,
) -> io::Result<()> {
    let name = "manifest.next";
    let mut options = cap_std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    let mut file = transaction.open_with(name, &options)?;
    hook(OnboardingPersistHook::WriteJournal, 0)?;
    let manifest_bytes = serde_json::to_vec(manifest)?;
    let mut authentication = Hmac::<Sha256>::new_from_slice(authentication_key)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid owner key"))?;
    authentication.update(&manifest_bytes);
    let authenticated = AuthenticatedOnboardingRecoveryManifest {
        manifest: serde_json::from_slice(&manifest_bytes)?,
        authentication: authentication.finalize().into_bytes().to_vec(),
    };
    io::Write::write_all(&mut file, &serde_json::to_vec(&authenticated)?)?;
    hook(OnboardingPersistHook::SyncJournal, 0)?;
    file.sync_all()?;
    hook(OnboardingPersistHook::ReplaceJournal, 0)?;
    transaction.rename(name, transaction, "manifest.json")?;
    transaction.open(".")?.sync_all()
}

fn authenticate_onboarding_manifest(
    authenticated: &AuthenticatedOnboardingRecoveryManifest,
    authentication_key: &[u8],
) -> io::Result<()> {
    let mut authentication = Hmac::<Sha256>::new_from_slice(authentication_key)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid owner key"))?;
    authentication.update(&serde_json::to_vec(&authenticated.manifest)?);
    authentication
        .verify_slice(&authenticated.authentication)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "unauthenticated manifest"))
}

fn persist_onboarding_plan_locked(
    authentication_root: &Dir,
    home_parent: &Dir,
    home_name: &std::ffi::OsStr,
    home: &Dir,
    plan: &OnboardingHomeWritePlan,
    hook: &mut dyn FnMut(OnboardingPersistHook, usize) -> io::Result<()>,
) -> Result<(), HomeError> {
    struct Staged {
        directory: Dir,
        destination: std::ffi::OsString,
        temporary: std::ffi::OsString,
        file: cap_std::fs::File,
        published: bool,
    }

    struct AnchoredDirectory {
        parent: Dir,
        name: std::ffi::OsString,
        directory: Dir,
        created: bool,
    }

    let mut directories: Vec<AnchoredDirectory> = Vec::new();
    let mut staged: Vec<Staged> = Vec::new();
    let transaction_name =
        std::ffi::OsString::from(format!(".onboarding-import-{}.txn", uuid::Uuid::new_v4()));
    let owner_name = onboarding_transaction_owner_name(&transaction_name);
    let mut authentication_key = [0u8; 32];
    getrandom::fill(&mut authentication_key).map_err(|error| {
        HomeError::io(
            "The onboarding import transaction could not be authenticated.",
            io::Error::other(error.to_string()),
        )
    })?;
    hook(OnboardingPersistHook::CreateTransactionOwner, 0).map_err(|error| {
        HomeError::io(
            "The onboarding import transaction could not be authenticated.",
            error,
        )
    })?;
    let mut owner_options = cap_std::fs::OpenOptions::new();
    owner_options.create_new(true).write(true);
    let mut key_file = authentication_root
        .open_with(&owner_name, &owner_options)
        .map_err(|error| {
            HomeError::io(
                "The onboarding import transaction could not be authenticated.",
                error,
            )
        })?;
    io::Write::write_all(&mut key_file, &authentication_key).map_err(|error| {
        HomeError::io(
            "The onboarding import transaction could not be authenticated.",
            error,
        )
    })?;
    key_file.sync_all().map_err(|error| {
        HomeError::io(
            "The onboarding import transaction could not be authenticated.",
            error,
        )
    })?;
    let mut authentication = Hmac::<Sha256>::new_from_slice(&authentication_key).unwrap();
    authentication.update(owner_name.to_string_lossy().as_bytes());
    let mut owner_file = home
        .open_with(&owner_name, &owner_options)
        .map_err(|error| {
            HomeError::io(
                "The onboarding import transaction could not be authenticated.",
                error,
            )
        })?;
    io::Write::write_all(
        &mut owner_file,
        authentication.finalize().into_bytes().as_slice(),
    )
    .map_err(|error| {
        HomeError::io(
            "The onboarding import transaction could not be authenticated.",
            error,
        )
    })?;
    owner_file.sync_all().map_err(|error| {
        HomeError::io(
            "The onboarding import transaction could not be authenticated.",
            error,
        )
    })?;
    hook(OnboardingPersistHook::SyncTransactionOwner, 0).map_err(|error| {
        HomeError::io(
            "The onboarding import transaction could not be authenticated.",
            error,
        )
    })?;
    home.open(".")
        .and_then(|directory| directory.sync_all())
        .and_then(|()| authentication_root.open(".")?.sync_all())
        .map_err(|error| {
            HomeError::io(
                "The onboarding import transaction could not be authenticated.",
                error,
            )
        })?;
    hook(OnboardingPersistHook::CreateTransaction, 0).map_err(|error| {
        HomeError::io(
            "The onboarding import transaction could not be created.",
            error,
        )
    })?;
    home.create_dir(&transaction_name).map_err(|error| {
        HomeError::io(
            "The onboarding import transaction could not be created.",
            error,
        )
    })?;
    let transaction = home.open_dir(&transaction_name).map_err(|error| {
        HomeError::io(
            "The onboarding import transaction could not be opened.",
            error,
        )
    })?;
    transaction
        .open(".")
        .and_then(|directory| directory.sync_all())
        .map_err(|error| {
            HomeError::io(
                "The onboarding import transaction could not be authenticated.",
                error,
            )
        })?;
    let mut manifest = OnboardingRecoveryManifest {
        committed: false,
        entries: plan
            .writes
            .iter()
            .enumerate()
            .map(|(index, write)| OnboardingRecoveryEntry {
                parent: write.relative_path().parent().unwrap().to_path_buf(),
                destination: write.relative_path().file_name().unwrap().to_os_string(),
                temporary: std::ffi::OsString::from(format!("payload-{index}")),
                anchor: std::ffi::OsString::from(format!("payload-{index}")),
                digest: Sha256::digest(write.bytes()).to_vec(),
            })
            .collect(),
        created_directories: Vec::new(),
    };
    if let Err(error) =
        write_onboarding_manifest(&transaction, &manifest, &authentication_key, true, hook)
    {
        // The durable authenticated owner makes even a partially-created initial
        // journal recognizable and safely replayable by the next locked invocation.
        return Err(HomeError::io(
            "The onboarding import transaction could not be journaled.",
            error,
        ));
    }
    let result = (|| -> io::Result<()> {
        for (write_index, write) in plan.writes.iter().enumerate() {
            let mut current = home.try_clone()?;
            let mut parent_path = PathBuf::new();
            for component in write.relative_path().parent().unwrap().components() {
                let name = component.as_os_str();
                parent_path.push(name);
                match current.symlink_metadata(name) {
                    Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "unsafe import ancestor",
                        ));
                    }
                    Ok(metadata) => {
                        let next = current.open_dir(name)?;
                        if !same_file(&metadata, &next.metadata(".")?) {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidInput,
                                "import ancestor changed",
                            ));
                        }
                        directories.push(AnchoredDirectory {
                            parent: current,
                            name: name.to_os_string(),
                            directory: next.try_clone()?,
                            created: false,
                        });
                        current = next;
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        let staged_name = std::ffi::OsString::from(format!(
                            "directory-{}",
                            manifest.created_directories.len()
                        ));
                        manifest
                            .created_directories
                            .push(OnboardingRecoveryDirectory {
                                path: parent_path.clone(),
                                identity: None,
                                staged: staged_name.clone(),
                            });
                        write_onboarding_manifest(
                            &transaction,
                            &manifest,
                            &authentication_key,
                            false,
                            hook,
                        )?;
                        hook(OnboardingPersistHook::CreateDirectory, 0)?;
                        transaction.create_dir(&staged_name)?;
                        let next = transaction.open_dir(&staged_name)?;
                        manifest.created_directories.last_mut().unwrap().identity =
                            file_identity(&next.metadata(".")?);
                        write_onboarding_manifest(
                            &transaction,
                            &manifest,
                            &authentication_key,
                            false,
                            hook,
                        )?;
                        transaction.rename(&staged_name, &current, name)?;
                        directories.push(AnchoredDirectory {
                            parent: current,
                            name: name.to_os_string(),
                            directory: next.try_clone()?,
                            created: true,
                        });
                        current = next;
                    }
                    Err(error) => return Err(error),
                }
            }
            let destination = write.relative_path().file_name().unwrap().to_os_string();
            match current.symlink_metadata(&destination) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "import destination exists",
                    ))
                }
                Err(error) => return Err(error),
            }
            let temporary = std::ffi::OsString::from(format!("payload-{write_index}"));
            let mut options = cap_std::fs::OpenOptions::new();
            options.create_new(true).write(true);
            let mut file = transaction.open_with(&temporary, &options)?;
            hook(OnboardingPersistHook::WritePayload, write_index)?;
            io::Write::write_all(&mut file, write.bytes())?;
            hook(OnboardingPersistHook::SyncPayload, write_index)?;
            file.sync_all()?;
            staged.push(Staged {
                directory: current,
                destination,
                temporary,
                file,
                published: false,
            });
        }

        home.open(".")?.sync_all()?;

        hook(OnboardingPersistHook::AfterStaging, 0)?;

        // Recheck the complete destination set immediately before publication;
        // staging can take long enough for a non-cooperating writer to appear.
        for entry in &staged {
            match entry.directory.symlink_metadata(&entry.destination) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "import destination exists",
                    ));
                }
                Err(error) => return Err(error),
            }
        }

        for (index, entry) in staged.iter_mut().enumerate() {
            hook(OnboardingPersistHook::Publish, index)?;
            transaction.hard_link(&entry.temporary, &entry.directory, &entry.destination)?;
            entry.published = true;
            hook(OnboardingPersistHook::AfterPublish, index)?;
        }
        for anchored in &directories {
            let visible = anchored.parent.symlink_metadata(&anchored.name)?;
            if visible.file_type().is_symlink()
                || !same_file(&visible, &anchored.directory.metadata(".")?)
            {
                return Err(io::Error::other("import ancestor changed"));
            }
        }
        for entry in &staged {
            let visible = entry.directory.symlink_metadata(&entry.destination)?;
            if !same_file(&visible, &entry.file.metadata()?) {
                return Err(io::Error::other("import destination changed"));
            }
        }
        let visible_home = home_parent.symlink_metadata(home_name)?;
        if visible_home.file_type().is_symlink() || !same_file(&visible_home, &home.metadata(".")?)
        {
            return Err(io::Error::other("import Home changed"));
        }
        hook(OnboardingPersistHook::BeforeTemporaryRemoval, 0)?;
        for (index, _entry) in staged.iter().enumerate() {
            hook(OnboardingPersistHook::RemoveTemporary, index)?;
        }
        hook(OnboardingPersistHook::BeforeDirectorySync, 0)?;
        for (index, entry) in staged.iter().enumerate() {
            hook(OnboardingPersistHook::SyncDirectory, index)?;
            entry.directory.open(".")?.sync_all()?;
        }
        home.open(".")?.sync_all()?;
        // This is the transaction's success linearization point. Recheck the
        // visible namespace after all publication and durability work.
        for anchored in &directories {
            let visible = anchored.parent.symlink_metadata(&anchored.name)?;
            if visible.file_type().is_symlink()
                || !same_file(&visible, &anchored.directory.metadata(".")?)
            {
                return Err(io::Error::other("import ancestor changed"));
            }
        }
        for entry in &staged {
            let visible = entry.directory.symlink_metadata(&entry.destination)?;
            if !same_file(&visible, &entry.file.metadata()?) {
                return Err(io::Error::other("import destination changed"));
            }
        }
        let visible_home = home_parent.symlink_metadata(home_name)?;
        if visible_home.file_type().is_symlink() || !same_file(&visible_home, &home.metadata(".")?)
        {
            return Err(io::Error::other("import Home changed"));
        }
        Ok(())
    })();

    if result.is_err() {
        let mut cleanup = || -> io::Result<()> {
            hook(OnboardingPersistHook::BeforeRollback, 0)?;
            let mut cleanup_error: Option<io::Error> = None;
            for (index, entry) in staged.iter().enumerate().rev() {
                if entry.published {
                    match entry.directory.symlink_metadata(&entry.destination) {
                        Ok(metadata) => match entry.file.metadata() {
                            Ok(created_metadata) if same_file(&metadata, &created_metadata) => {
                                hook(OnboardingPersistHook::RollbackFile, index)?;
                                if let Err(error) = entry.directory.remove_file(&entry.destination)
                                {
                                    cleanup_error.get_or_insert(error);
                                }
                            }
                            Ok(_) => {}
                            Err(error) => {
                                cleanup_error.get_or_insert(error);
                            }
                        },
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                        Err(error) => {
                            cleanup_error.get_or_insert(error);
                        }
                    }
                }
                // A racer may rename our published link. Remove every link in the
                // anchored destination directory that still has our payload identity.
                let created_metadata = entry.file.metadata()?;
                for child in entry.directory.entries()? {
                    let child = child?;
                    let name = child.file_name();
                    if name == entry.temporary {
                        continue;
                    }
                    if let Ok(metadata) = entry.directory.symlink_metadata(&name) {
                        if metadata.is_file() && same_file(&metadata, &created_metadata) {
                            hook(OnboardingPersistHook::RollbackFile, index)?;
                            if let Err(error) = entry.directory.remove_file(&name) {
                                cleanup_error.get_or_insert(error);
                            }
                        }
                    }
                }
                hook(OnboardingPersistHook::RemoveTemporary, index)?;
                if let Err(error) = entry.directory.remove_file(&entry.temporary) {
                    if error.kind() != io::ErrorKind::NotFound {
                        cleanup_error.get_or_insert(error);
                    }
                }
            }
            for anchored in directories.iter().rev().filter(|entry| entry.created) {
                let is_exact = match anchored.parent.symlink_metadata(&anchored.name) {
                    Ok(metadata) => {
                        !metadata.file_type().is_symlink()
                            && same_file(&metadata, &anchored.directory.metadata(".")?)
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => false,
                    Err(error) => return Err(error),
                };
                if is_exact {
                    hook(OnboardingPersistHook::RollbackDirectory, 0)?;
                    if let Err(error) = anchored.parent.remove_dir(&anchored.name) {
                        if error.kind() != io::ErrorKind::NotFound
                            && error.kind() != io::ErrorKind::DirectoryNotEmpty
                        {
                            cleanup_error.get_or_insert(error);
                        }
                    }
                }
            }
            hook(OnboardingPersistHook::RollbackSync, 0)?;
            home.open(".")
                .and_then(|directory| directory.sync_all())
                .err()
                .map(|error| cleanup_error.get_or_insert(error));
            cleanup_error.map_or(Ok(()), Err)
        };
        // Cleanup operations are idempotent and identity checked. Retry transient
        // failures before returning so an injected or short-lived error cannot
        // expose a partial transaction.
        let mut cleanup_error = None;
        for _ in 0..8 {
            match cleanup() {
                Ok(()) => {
                    cleanup_error = None;
                    break;
                }
                Err(error) => cleanup_error = Some(error),
            }
        }
        if let Some(error) = cleanup_error {
            return Err(HomeError::io(
                "The onboarding import rollback failed.",
                error,
            ));
        }
    }
    if result.is_ok() {
        // Creating this marker is the commit decision. Once it exists, recovery
        // only retires the journal and anchors and never removes destinations.
        let decision = (|| -> io::Result<()> {
            hook(OnboardingPersistHook::WriteCommitDecision, 0)?;
            let mut options = cap_std::fs::OpenOptions::new();
            options.create_new(true).write(true);
            let marker = transaction.open_with("committed", &options)?;
            hook(OnboardingPersistHook::SyncCommitDecision, 0)?;
            marker.sync_all()?;
            hook(OnboardingPersistHook::SyncCommitDirectory, 0)?;
            transaction.open(".")?.sync_all()
        })();
        match decision {
            Ok(()) => {}
            Err(error) => {
                // No durable decision exists, so complete rollback before the
                // failing call returns. A different later plan cannot erase a
                // publication that this call temporarily exposed.
                match transaction.remove_file("committed") {
                    Ok(()) => transaction
                        .open(".")
                        .and_then(|directory| directory.sync_all())
                        .map_err(|sync_error| {
                            HomeError::io("The onboarding import rollback failed.", sync_error)
                        })?,
                    Err(remove_error) if remove_error.kind() == io::ErrorKind::NotFound => {}
                    Err(remove_error) => {
                        return Err(HomeError::io(
                            "The onboarding import rollback failed.",
                            remove_error,
                        ))
                    }
                }
                let mut recovery_hook = |_, _| Ok(());
                recover_onboarding_transactions(authentication_root, home, &mut recovery_hook)
                    .map_err(|recovery_error| {
                        HomeError::io("The onboarding import rollback failed.", recovery_error)
                    })?;
                return Err(HomeError::io(
                    "The onboarding import transaction could not be completed.",
                    error,
                ));
            }
        }
    }

    // Journal retirement is itself replayable. Any failure leaves manifest.json
    // present until every payload anchor is gone, so a later locked invocation
    // can deterministically finish the same committed or aborted decision.
    let retirement = (|| -> io::Result<()> {
        if result.is_err() {
            for directory in manifest.created_directories.iter().rev() {
                hook(OnboardingPersistHook::RetireDirectory, 0)?;
                match transaction.remove_dir(&directory.staged) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
        }
        for (index, entry) in manifest.entries.iter().enumerate() {
            hook(OnboardingPersistHook::RetireAnchor, index)?;
            match transaction.remove_file(&entry.anchor) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        match transaction.remove_file("manifest.next") {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        match transaction.remove_file("committed") {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        hook(OnboardingPersistHook::RetireJournal, 0)?;
        transaction.remove_file("manifest.json")?;
        let retired_name = std::ffi::OsString::from(
            transaction_name
                .to_string_lossy()
                .replace(".txn", ".retired"),
        );
        home.rename(&transaction_name, home, &retired_name)?;
        hook(OnboardingPersistHook::RetireTransaction, 0)?;
        home.remove_dir(&retired_name)?;
        hook(OnboardingPersistHook::RetireTransactionOwner, 0)?;
        home.remove_file(&owner_name)?;
        authentication_root.remove_file(&owner_name)?;
        home.open(".")?.sync_all()?;
        authentication_root.open(".")?.sync_all()
    })();
    if let Err(error) = retirement {
        return Err(HomeError::io(
            "The onboarding import journal could not be retired.",
            error,
        ));
    }
    result.map_err(|error| HomeError::io("The onboarding import could not be saved.", error))
}

#[cfg(unix)]
fn same_file(left: &cap_std::fs::Metadata, right: &cap_std::fs::Metadata) -> bool {
    use cap_std::fs::MetadataExt;
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(unix)]
fn file_identity(metadata: &cap_std::fs::Metadata) -> Option<FileIdentity> {
    use cap_std::fs::MetadataExt;
    Some(FileIdentity {
        first: metadata.dev(),
        second: metadata.ino(),
    })
}

#[cfg(unix)]
fn same_home_file(left: &fs::Metadata, right: &cap_std::fs::Metadata) -> bool {
    use cap_std::fs::MetadataExt as CapMetadataExt;
    use std::os::unix::fs::MetadataExt as StdMetadataExt;
    StdMetadataExt::dev(left) == CapMetadataExt::dev(right)
        && StdMetadataExt::ino(left) == CapMetadataExt::ino(right)
}

#[cfg(windows)]
fn same_file(left: &cap_std::fs::Metadata, right: &cap_std::fs::Metadata) -> bool {
    use cap_fs_ext::MetadataExt;
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(windows)]
fn file_identity(metadata: &cap_std::fs::Metadata) -> Option<FileIdentity> {
    use cap_fs_ext::MetadataExt;
    Some(FileIdentity {
        first: metadata.dev(),
        second: metadata.ino(),
    })
}

#[cfg(windows)]
fn same_home_file(left: &cap_std::fs::Metadata, right: &cap_std::fs::Metadata) -> bool {
    same_file(left, right)
}

fn validate_home(home: &Path) -> Result<(), HomeError> {
    if !home.is_absolute() || home.parent().is_none() {
        return Err(HomeError::invalid(
            "Choose an absolute folder other than a filesystem root.",
        ));
    }
    let name = home
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| HomeError::invalid("The Home folder location is invalid."))?;
    if name.starts_with('.') {
        return Err(HomeError::invalid(
            "Muniment Home must be a visible folder, not a dot-directory.",
        ));
    }
    Ok(())
}

fn create_visible_directory(path: &Path) -> Result<(), HomeError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => Err(
            HomeError::invalid("A Muniment Home scaffold path is not a directory."),
        ),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir(path)
            .map_err(|error| HomeError::io("Muniment Home could not be created.", error)),
        Err(error) => Err(HomeError::io(
            "Muniment Home could not be inspected.",
            error,
        )),
    }
}

fn persist_home(config_dir: &Path, home: &Path) -> Result<(), HomeError> {
    fs::create_dir_all(config_dir)
        .map_err(|error| HomeError::io("The Home selection could not be saved.", error))?;
    let contents = serde_json::to_vec_pretty(&HomeConfig {
        location: home.to_path_buf(),
    })
    .map_err(|_| HomeError::invalid("The Home selection could not be saved."))?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(config_dir.join("home.lock"))
        .map_err(|error| HomeError::io("The Home selection could not be saved.", error))?;
    lock.lock_exclusive()
        .map_err(|error| HomeError::io("The Home selection could not be saved.", error))?;

    let destination = config_dir.join(CONFIG_FILE);
    let temporary = config_dir.join(format!("home.json.{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        io::Write::write_all(&mut file, &contents)?;
        file.sync_all()?;
        replace_file(&temporary, &destination)?;
        sync_directory(config_dir)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|error| HomeError::io("The Home selection could not be saved.", error))
}

#[cfg(not(target_os = "windows"))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(not(target_os = "windows"))]
fn sync_directory(path: &Path) -> io::Result<()> {
    fs::File::open(path)?.sync_all()
}

#[cfg(target_os = "windows")]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    if destination.exists() {
        // Windows rename does not replace an existing file atomically.
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;
        let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let destination: Vec<u16> = destination
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        let replaced = unsafe {
            ReplaceFileW(
                destination.as_ptr(),
                source.as_ptr(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        if replaced == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    } else {
        fs::rename(source, destination)
    }
}

#[cfg(target_os = "windows")]
fn sync_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::replace_file;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn failed_replacement_preserves_the_previous_file() {
        let root = std::env::temp_dir().join(format!(
            "muniment-home-replace-failure-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let destination = root.join("home.json");
        fs::write(&destination, b"previous configuration").unwrap();

        assert!(replace_file(&root.join("missing.tmp"), &destination).is_err());
        assert_eq!(fs::read(&destination).unwrap(), b"previous configuration");
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_rollback_identity_does_not_match_distinct_objects() {
        use super::{file_identity, same_file};
        use cap_std::{ambient_authority, fs::Dir};
        use std::os::windows::io::{AsRawHandle, RawHandle};
        use windows_sys::Win32::{Foundation::FILETIME, Storage::FileSystem::SetFileTime};

        fn set_creation_time(handle: RawHandle, value: u64) {
            let time = FILETIME {
                dwLowDateTime: value as u32,
                dwHighDateTime: (value >> 32) as u32,
            };
            let result = unsafe { SetFileTime(handle, &time, std::ptr::null(), std::ptr::null()) };
            assert_ne!(result, 0, "setting matching Windows creation times failed");
        }

        let root = std::env::temp_dir().join(format!(
            "muniment-home-windows-identity-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("anchor"), b"same").unwrap();
        fs::write(root.join("replacement"), b"same").unwrap();
        fs::create_dir(root.join("created")).unwrap();
        fs::create_dir(root.join("replacement-directory")).unwrap();

        let directory = Dir::open_ambient_dir(&root, ambient_authority()).unwrap();
        let anchor = directory.open("anchor").unwrap();
        let replacement = directory.open("replacement").unwrap();
        let created = directory.open_dir("created").unwrap();
        let replacement_directory = directory.open_dir("replacement-directory").unwrap();
        let matching_time = 132_000_000_000_000_000;
        set_creation_time(anchor.as_raw_handle(), matching_time);
        set_creation_time(replacement.as_raw_handle(), matching_time);
        set_creation_time(created.as_raw_handle(), matching_time);
        set_creation_time(replacement_directory.as_raw_handle(), matching_time);

        let anchor_metadata = anchor.metadata().unwrap();
        let replacement_metadata = replacement.metadata().unwrap();
        let created_metadata = created.metadata(".").unwrap();
        let replacement_directory_metadata = replacement_directory.metadata(".").unwrap();
        use cap_std::fs::MetadataExt;
        assert_eq!(
            (
                anchor_metadata.file_attributes(),
                anchor_metadata.creation_time(),
                anchor_metadata.file_size()
            ),
            (
                replacement_metadata.file_attributes(),
                replacement_metadata.creation_time(),
                replacement_metadata.file_size()
            )
        );
        assert_eq!(
            (
                created_metadata.file_attributes(),
                created_metadata.creation_time(),
                created_metadata.file_size()
            ),
            (
                replacement_directory_metadata.file_attributes(),
                replacement_directory_metadata.creation_time(),
                replacement_directory_metadata.file_size()
            )
        );

        // These are the exact predicates used before recovery removes a payload
        // link or a transaction-created directory. Distinct replacements must
        // fail both predicates even when every field used by the old tuple matches.
        assert!(!same_file(&anchor_metadata, &replacement_metadata));
        assert_ne!(
            file_identity(&created_metadata),
            file_identity(&replacement_directory_metadata)
        );
        if same_file(&anchor_metadata, &replacement_metadata) {
            directory.remove_file("replacement").unwrap();
        }
        if file_identity(&created_metadata) == file_identity(&replacement_directory_metadata) {
            directory.remove_dir("replacement-directory").unwrap();
        }
        assert!(root.join("replacement").exists());
        assert!(root.join("replacement-directory").exists());

        drop(replacement_directory);
        drop(created);
        drop(replacement);
        drop(anchor);
        drop(directory);
        fs::remove_dir_all(root).unwrap();
    }
}
