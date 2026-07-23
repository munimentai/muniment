use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fmt, fs,
    fs::OpenOptions,
    io::{self, Write},
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
const ONBOARDING_IMPORT_LOCK_FILE: &str = ".onboarding-import.lock";
const ONBOARDING_IMPORT_MAX_PLAN_WRITES: usize = ONBOARDING_IMPORT_MAX_ENTRIES + 4;

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

/// Stable failure modes for persisting a confirmed onboarding write plan.
#[derive(Debug)]
pub enum OnboardingHomePersistenceError {
    InvalidHome,
    InvalidPlan,
    TooManyEntries,
    DocumentBytesExceeded,
    TotalBytesExceeded,
    DestinationConflict { relative_path: String },
    Io(io::Error),
}

impl fmt::Display for OnboardingHomePersistenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHome => formatter.write_str("The Muniment Home is invalid."),
            Self::InvalidPlan => formatter.write_str("The onboarding write plan is invalid."),
            Self::TooManyEntries => {
                formatter.write_str("The onboarding write plan contains too many entries.")
            }
            Self::DocumentBytesExceeded => {
                formatter.write_str("An onboarding write plan document is too large.")
            }
            Self::TotalBytesExceeded => {
                formatter.write_str("The onboarding write plan is too large.")
            }
            Self::DestinationConflict { relative_path } => {
                write!(
                    formatter,
                    "The onboarding destination already exists: {relative_path}"
                )
            }
            Self::Io(_) => formatter.write_str("The onboarding write plan could not be saved."),
        }
    }
}

impl std::error::Error for OnboardingHomePersistenceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for OnboardingHomePersistenceError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Persists a confirmed write plan without replacing any existing Home entry.
pub fn persist_onboarding_home_write_plan(
    home: &Path,
    plan: &OnboardingHomeWritePlan,
) -> Result<(), OnboardingHomePersistenceError> {
    let relative_destinations = validate_persistence_plan(plan)?;
    let home = validate_persistence_home(home)?;
    let destinations = relative_destinations
        .into_iter()
        .map(|(relative_path, path)| (relative_path, home.join(path)))
        .collect::<Vec<_>>();
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(home.join(ONBOARDING_IMPORT_LOCK_FILE))?;
    lock.lock_exclusive()?;

    for (relative_path, destination) in &destinations {
        match fs::symlink_metadata(destination) {
            Ok(_) => {
                return Err(OnboardingHomePersistenceError::DestinationConflict {
                    relative_path: relative_path.clone(),
                })
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }

    let mut temporaries = Vec::with_capacity(plan.writes.len());
    let result = (|| {
        for ((_, destination), write) in destinations.iter().zip(&plan.writes) {
            let parent = destination
                .parent()
                .ok_or(OnboardingHomePersistenceError::InvalidPlan)?;
            create_import_parent_directories(&home, parent)?;
            let temporary = parent.join(format!(
                ".{}.{}.tmp",
                destination
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or(OnboardingHomePersistenceError::InvalidPlan)?,
                uuid::Uuid::new_v4()
            ));
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)?;
            temporaries.push(temporary);
            file.write_all(write.bytes())?;
            file.flush()?;
            file.sync_all()?;
        }

        for ((relative_path, destination), temporary) in destinations.iter().zip(&temporaries) {
            publish_new_file(temporary, destination).map_err(|error| {
                if error.kind() == io::ErrorKind::AlreadyExists {
                    OnboardingHomePersistenceError::DestinationConflict {
                        relative_path: relative_path.clone(),
                    }
                } else {
                    error.into()
                }
            })?;
            sync_directory(
                destination
                    .parent()
                    .ok_or(OnboardingHomePersistenceError::InvalidPlan)?,
            )?;
        }
        Ok(())
    })();

    for temporary in temporaries {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn validate_persistence_plan(
    plan: &OnboardingHomeWritePlan,
) -> Result<Vec<(String, PathBuf)>, OnboardingHomePersistenceError> {
    if plan.writes.is_empty() {
        return Err(OnboardingHomePersistenceError::InvalidPlan);
    }
    if plan.writes.len() > ONBOARDING_IMPORT_MAX_PLAN_WRITES {
        return Err(OnboardingHomePersistenceError::TooManyEntries);
    }
    let mut unique = BTreeSet::new();
    let mut total_bytes = 0usize;
    let mut relative_paths = Vec::with_capacity(plan.writes.len());
    for write in &plan.writes {
        let path = Path::new(&write.relative_path);
        let mut segments = write.relative_path.split('/');
        if write.relative_path.is_empty()
            || write.relative_path.contains('\0')
            || write.relative_path.contains('\\')
            || write.relative_path.starts_with('/')
            || segments
                .next()
                .is_some_and(|segment| segment.ends_with(':'))
            || segments.any(|segment| segment.is_empty() || segment == "." || segment == "..")
            || path.is_absolute()
            || !write.relative_path.ends_with(".md")
            || path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
            || !unique.insert(write.relative_path.to_ascii_lowercase())
        {
            return Err(OnboardingHomePersistenceError::InvalidPlan);
        }
        if write.bytes().len() > ONBOARDING_IMPORT_MAX_DOCUMENT_BYTES {
            return Err(OnboardingHomePersistenceError::DocumentBytesExceeded);
        }
        total_bytes = total_bytes
            .checked_add(write.bytes().len())
            .ok_or(OnboardingHomePersistenceError::TotalBytesExceeded)?;
        if total_bytes > ONBOARDING_IMPORT_MAX_TOTAL_BYTES {
            return Err(OnboardingHomePersistenceError::TotalBytesExceeded);
        }
        relative_paths.push((write.relative_path.clone(), path.to_path_buf()));
    }
    Ok(relative_paths)
}

fn validate_persistence_home(home: &Path) -> Result<PathBuf, OnboardingHomePersistenceError> {
    validate_home(home).map_err(|_| OnboardingHomePersistenceError::InvalidHome)?;
    let selected_metadata =
        fs::symlink_metadata(home).map_err(|_| OnboardingHomePersistenceError::InvalidHome)?;
    if selected_metadata.file_type().is_symlink() || !selected_metadata.is_dir() {
        return Err(OnboardingHomePersistenceError::InvalidHome);
    }
    let home = fs::canonicalize(home).map_err(|_| OnboardingHomePersistenceError::InvalidHome)?;
    validate_home(&home).map_err(|_| OnboardingHomePersistenceError::InvalidHome)?;
    Ok(home)
}

fn create_import_parent_directories(
    home: &Path,
    parent: &Path,
) -> Result<(), OnboardingHomePersistenceError> {
    let relative = parent
        .strip_prefix(home)
        .map_err(|_| OnboardingHomePersistenceError::InvalidPlan)?;
    let mut current = home.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => return Err(OnboardingHomePersistenceError::InvalidPlan),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&current)?;
                sync_directory(
                    current
                        .parent()
                        .ok_or(OnboardingHomePersistenceError::InvalidPlan)?,
                )?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn publish_new_file(temporary: &Path, destination: &Path) -> io::Result<()> {
    fs::hard_link(temporary, destination)
}

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
}
