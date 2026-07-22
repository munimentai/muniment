use chrono::NaiveDate;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fmt, fs,
    fs::OpenOptions,
    io,
    path::{Component, Path, PathBuf},
};

use crate::{import_preview::ExtractedEntry, llama::OnboardingTriageReport};

const CONFIG_FILE: &str = "home.json";
const HOME_DIRECTORIES: [&str; 4] = ["memory", "agents", "projects", "sessions"];

/// Maximum number of approved originals in one onboarding plan.
pub const MAX_ONBOARDING_PLAN_ENTRIES: usize = 128;
/// Maximum UTF-8 bytes accepted in an untrusted source name.
pub const MAX_ONBOARDING_SOURCE_NAME_BYTES: usize = 1024;
/// Maximum bytes in any complete planned file.
pub const MAX_ONBOARDING_FILE_BYTES: usize = 4 * 1024 * 1024;
/// Maximum bytes across every complete file in a plan.
pub const MAX_ONBOARDING_PLAN_BYTES: usize = 16 * 1024 * 1024;
/// Maximum bytes in a generated filename component.
pub const MAX_ONBOARDING_FILENAME_BYTES: usize = 120;
/// Maximum bytes in a generated Home-relative path.
pub const MAX_ONBOARDING_PATH_BYTES: usize = 124;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeWrite {
    pub relative_path: PathBuf,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnboardingHomeWritePlan {
    pub writes: Vec<HomeWrite>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardingPlanError {
    EmptyEntries,
    TooManyEntries,
    InvalidReport,
    EmptySourceField,
    SourceNameTooLong,
    FileTooLarge,
    PlanTooLarge,
    FilenameTooLong,
    PathTooLong,
    DuplicateDestination,
}

/// Compiles confirmed onboarding content into an ordered, side-effect-free plan.
///
/// The supplied date is deliberately explicit: equal logical input and date
/// always produce exactly equal paths and bytes. Writes are ordered as report,
/// starter agents, then approved originals in their supplied order.
pub fn plan_onboarding_home_writes(
    report: &OnboardingTriageReport,
    approved_entries: &[ExtractedEntry],
    import_date: NaiveDate,
) -> Result<OnboardingHomeWritePlan, OnboardingPlanError> {
    if approved_entries.is_empty() {
        return Err(OnboardingPlanError::EmptyEntries);
    }
    if approved_entries.len() > MAX_ONBOARDING_PLAN_ENTRIES {
        return Err(OnboardingPlanError::TooManyEntries);
    }
    let report_markdown = format!(
        "## User type\n{}\n\n## Proposed Home layout\n{}\n\n## Starter agents\n{}\n",
        report.user_type,
        report.proposed_home_layout,
        report
            .starter_agents
            .iter()
            .map(|agent| format!("- {agent}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    if OnboardingTriageReport::parse(&report_markdown).as_ref() != Ok(report) {
        return Err(OnboardingPlanError::InvalidReport);
    }

    let mut writes = Vec::with_capacity(1 + report.starter_agents.len() + approved_entries.len());
    let mut destinations = BTreeSet::new();
    push_planned_write(
        &mut writes,
        &mut destinations,
        PathBuf::from("onboarding-report.md"),
        report_markdown.into_bytes(),
    )?;
    for agent in &report.starter_agents {
        let filename = markdown_filename(agent, "agent")?;
        push_planned_write(
            &mut writes,
            &mut destinations,
            Path::new("agents").join(filename),
            format!("{agent}\n").into_bytes(),
        )?;
    }
    for entry in approved_entries {
        if entry.source_name.trim().is_empty() || entry.source_provenance.trim().is_empty() {
            return Err(OnboardingPlanError::EmptySourceField);
        }
        if entry.source_name.len() > MAX_ONBOARDING_SOURCE_NAME_BYTES {
            return Err(OnboardingPlanError::SourceNameTooLong);
        }
        let filename = markdown_filename(&entry.source_name, "original")?;
        let source = serde_json::to_string(&entry.source_provenance)
            .expect("serializing provenance cannot fail");
        let source_name = serde_json::to_string(&entry.source_name)
            .expect("serializing a source name cannot fail");
        let mut bytes = format!(
            "---\nsource: {source}\nsource_name: {source_name}\nimport_date: {import_date}\n---\n\n"
        )
        .into_bytes();
        bytes.extend_from_slice(entry.text.as_bytes());
        push_planned_write(
            &mut writes,
            &mut destinations,
            Path::new("memory").join(filename),
            bytes,
        )?;
    }
    Ok(OnboardingHomeWritePlan { writes })
}

fn markdown_filename(untrusted: &str, fallback: &str) -> Result<String, OnboardingPlanError> {
    let mut slug = String::new();
    let mut separator = false;
    for character in untrusted.chars() {
        if character.is_ascii_alphanumeric() {
            if separator && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(character.to_ascii_lowercase());
            separator = false;
        } else {
            separator = true;
        }
    }
    if slug.is_empty() {
        slug.push_str(fallback);
    }
    let filename = format!("{slug}.md");
    if filename.len() > MAX_ONBOARDING_FILENAME_BYTES {
        return Err(OnboardingPlanError::FilenameTooLong);
    }
    Ok(filename)
}

fn push_planned_write(
    writes: &mut Vec<HomeWrite>,
    destinations: &mut BTreeSet<PathBuf>,
    relative_path: PathBuf,
    bytes: Vec<u8>,
) -> Result<(), OnboardingPlanError> {
    if relative_path.as_os_str().len() > MAX_ONBOARDING_PATH_BYTES {
        return Err(OnboardingPlanError::PathTooLong);
    }
    if bytes.len() > MAX_ONBOARDING_FILE_BYTES {
        return Err(OnboardingPlanError::FileTooLarge);
    }
    if !destinations.insert(relative_path.clone()) {
        return Err(OnboardingPlanError::DuplicateDestination);
    }
    let total = writes.iter().try_fold(bytes.len(), |total, write| {
        total.checked_add(write.bytes.len())
    });
    if total.is_none_or(|total| total > MAX_ONBOARDING_PLAN_BYTES) {
        return Err(OnboardingPlanError::PlanTooLarge);
    }
    writes.push(HomeWrite {
        relative_path,
        bytes,
    });
    Ok(())
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
    use super::{
        markdown_filename, plan_onboarding_home_writes, push_planned_write, replace_file,
        OnboardingPlanError, MAX_ONBOARDING_FILENAME_BYTES, MAX_ONBOARDING_FILE_BYTES,
        MAX_ONBOARDING_PATH_BYTES, MAX_ONBOARDING_PLAN_BYTES, MAX_ONBOARDING_PLAN_ENTRIES,
        MAX_ONBOARDING_SOURCE_NAME_BYTES,
    };
    use crate::{
        import_preview::{EntryKind, ExtractedEntry},
        llama::OnboardingTriageReport,
    };
    use chrono::NaiveDate;
    use std::{
        collections::BTreeSet,
        fs,
        path::{Component, Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    fn report() -> OnboardingTriageReport {
        OnboardingTriageReport::parse(
            "## User type\nResearcher\n\n## Proposed Home layout\nTopics.\n\n## Starter agents\n- Research Scout\n- Writing Partner\n",
        )
        .unwrap()
    }

    fn entry(name: &str, text: &str) -> ExtractedEntry {
        ExtractedEntry {
            source_name: name.into(),
            kind: EntryKind::Markdown,
            text: text.into(),
            source_provenance: format!("sha256:{name}"),
        }
    }

    #[test]
    fn onboarding_plan_is_deterministic_safe_and_verbatim() {
        let date = NaiveDate::from_ymd_opt(2026, 7, 21).unwrap();
        let entries = vec![entry("../../Notes.md", "exact\noriginal\n")];
        let first = plan_onboarding_home_writes(&report(), &entries, date).unwrap();
        let second = plan_onboarding_home_writes(&report(), &entries, date).unwrap();

        assert_eq!(first, second);
        assert_eq!(first.writes.len(), 4);
        assert_eq!(
            first.writes[0].relative_path,
            Path::new("onboarding-report.md")
        );
        assert_eq!(
            first.writes[1].relative_path,
            Path::new("agents/research-scout.md")
        );
        assert_eq!(
            first.writes[3].relative_path,
            Path::new("memory/notes-md.md")
        );
        assert!(first.writes.iter().all(|write| {
            write.relative_path.is_relative()
                && write
                    .relative_path
                    .components()
                    .all(|component| matches!(component, Component::Normal(_)))
        }));
        let original = &first.writes[3].bytes;
        let marker = b"---\n\n";
        let body = original
            .windows(marker.len())
            .position(|window| window == marker)
            .map(|position| &original[position + marker.len()..])
            .unwrap();
        assert_eq!(body, entries[0].text.as_bytes());
        assert!(String::from_utf8_lossy(original).contains("import_date: 2026-07-21"));
        assert!(String::from_utf8_lossy(original).contains("source: \"sha256:../../Notes.md\""));
    }

    #[test]
    fn onboarding_plan_rejects_empty_counts_and_collisions() {
        let date = NaiveDate::from_ymd_opt(2026, 7, 21).unwrap();
        assert_eq!(
            plan_onboarding_home_writes(&report(), &[], date),
            Err(OnboardingPlanError::EmptyEntries)
        );
        let too_many = vec![entry("same", "x"); MAX_ONBOARDING_PLAN_ENTRIES + 1];
        assert_eq!(
            plan_onboarding_home_writes(&report(), &too_many, date),
            Err(OnboardingPlanError::TooManyEntries)
        );
        assert_eq!(
            plan_onboarding_home_writes(
                &report(),
                &[entry("A/B", "one"), entry("a-b", "two")],
                date
            ),
            Err(OnboardingPlanError::DuplicateDestination)
        );
        assert!(plan_onboarding_home_writes(
            &report(),
            &(0..MAX_ONBOARDING_PLAN_ENTRIES)
                .map(|index| entry(&format!("source-{index}"), ""))
                .collect::<Vec<_>>(),
            date
        )
        .is_ok());
    }

    #[test]
    fn onboarding_plan_enforces_field_and_file_boundaries() {
        let date = NaiveDate::from_ymd_opt(2026, 7, 21).unwrap();
        let mut empty_provenance = entry("source", "body");
        empty_provenance.source_provenance = " ".into();
        assert_eq!(
            plan_onboarding_home_writes(&report(), &[empty_provenance], date),
            Err(OnboardingPlanError::EmptySourceField)
        );
        assert_eq!(
            plan_onboarding_home_writes(
                &report(),
                &[entry(&"n".repeat(MAX_ONBOARDING_SOURCE_NAME_BYTES + 1), "")],
                date
            ),
            Err(OnboardingPlanError::SourceNameTooLong)
        );

        let source_name_at_limit = "é".repeat(MAX_ONBOARDING_SOURCE_NAME_BYTES / 2);
        assert_eq!(source_name_at_limit.len(), MAX_ONBOARDING_SOURCE_NAME_BYTES);
        assert!(
            plan_onboarding_home_writes(&report(), &[entry(&source_name_at_limit, "")], date)
                .is_ok()
        );

        assert_eq!(
            markdown_filename(&"n".repeat(MAX_ONBOARDING_FILENAME_BYTES - 3), "fallback")
                .unwrap()
                .len(),
            MAX_ONBOARDING_FILENAME_BYTES
        );
        assert_eq!(
            markdown_filename(&"n".repeat(MAX_ONBOARDING_FILENAME_BYTES - 2), "fallback"),
            Err(OnboardingPlanError::FilenameTooLong)
        );
        assert_eq!(
            plan_onboarding_home_writes(&report(), &[entry(&"n".repeat(121), "")], date),
            Err(OnboardingPlanError::FilenameTooLong)
        );

        let mut writes = Vec::new();
        let mut destinations = BTreeSet::new();
        assert!(push_planned_write(
            &mut writes,
            &mut destinations,
            PathBuf::from("p".repeat(MAX_ONBOARDING_PATH_BYTES)),
            Vec::new(),
        )
        .is_ok());
        assert_eq!(
            plan_onboarding_home_writes(&report(), &[entry(&"n".repeat(116), "")], date),
            Err(OnboardingPlanError::PathTooLong)
        );
        assert_eq!(
            push_planned_write(
                &mut writes,
                &mut destinations,
                PathBuf::from("p".repeat(MAX_ONBOARDING_PATH_BYTES + 1)),
                Vec::new(),
            ),
            Err(OnboardingPlanError::PathTooLong)
        );

        let header_len = plan_onboarding_home_writes(&report(), &[entry("boundary", "")], date)
            .unwrap()
            .writes[3]
            .bytes
            .len();
        assert!(plan_onboarding_home_writes(
            &report(),
            &[entry(
                "boundary",
                &"x".repeat(MAX_ONBOARDING_FILE_BYTES - header_len)
            )],
            date
        )
        .is_ok());
        assert_eq!(
            plan_onboarding_home_writes(
                &report(),
                &[entry(
                    "boundary",
                    &"x".repeat(MAX_ONBOARDING_FILE_BYTES - header_len + 1)
                )],
                date
            ),
            Err(OnboardingPlanError::FileTooLarge)
        );

        let mut aggregate_entries = (0..5)
            .map(|index| entry(&format!("total-{index}"), ""))
            .collect::<Vec<_>>();
        let empty_plan = plan_onboarding_home_writes(&report(), &aggregate_entries, date).unwrap();
        let mut remaining = MAX_ONBOARDING_PLAN_BYTES
            - empty_plan
                .writes
                .iter()
                .map(|write| write.bytes.len())
                .sum::<usize>();
        for (index, write) in empty_plan.writes[3..].iter().enumerate() {
            let capacity = MAX_ONBOARDING_FILE_BYTES - write.bytes.len();
            let payload_len = remaining.min(capacity);
            aggregate_entries[index].text = "x".repeat(payload_len);
            remaining -= payload_len;
        }
        assert_eq!(remaining, 0);
        let exact_plan = plan_onboarding_home_writes(&report(), &aggregate_entries, date).unwrap();
        assert_eq!(
            exact_plan
                .writes
                .iter()
                .map(|write| write.bytes.len())
                .sum::<usize>(),
            MAX_ONBOARDING_PLAN_BYTES
        );

        aggregate_entries.last_mut().unwrap().text.push('x');
        assert_eq!(
            plan_onboarding_home_writes(&report(), &aggregate_entries, date),
            Err(OnboardingPlanError::PlanTooLarge)
        );
    }

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
