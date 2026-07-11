//! Atomic, filesystem-only lifecycle for immutable ASR revisions.

use super::{verify_model_set, AsrArtifactManifest};
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
    NotInstalled,
    Ready,
    RepairRequired,
    MutationInProgress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleError {
    MutationInProgress,
    InvalidStage,
    VerificationFailed,
    UnsafeEntry,
    Storage { retryable: bool },
}

impl std::fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::MutationInProgress => "another process is managing ASR files",
            Self::InvalidStage => "the staged ASR revision is invalid",
            Self::VerificationFailed => "ASR model verification failed",
            Self::UnsafeEntry => "an unsafe ASR filesystem entry was rejected",
            Self::Storage { .. } => "the ASR filesystem operation failed",
        })
    }
}

impl std::error::Error for LifecycleError {}

pub trait ExclusiveLock {
    type Guard;
    fn try_acquire(&self) -> Result<Self::Guard, LifecycleError>;
}

pub trait AtomicReplace {
    /// Replaces `destination` without a reader-visible missing-file window.
    /// Implementors must provide replace-existing semantics on their platform.
    fn replace(&self, source: &Path, destination: &Path) -> Result<(), LifecycleError>;
    /// Atomically removes the pointer name when it exists.
    fn remove(&self, path: &Path) -> Result<(), LifecycleError>;
}

pub struct AsrLifecycle<'a, L, A> {
    root: PathBuf,
    manifest: &'a AsrArtifactManifest,
    known_manifests: &'a [&'a AsrArtifactManifest],
    lock: L,
    atomic: A,
}

impl<'a, L: ExclusiveLock, A: AtomicReplace> AsrLifecycle<'a, L, A> {
    pub fn new(
        root: impl Into<PathBuf>,
        manifest: &'a AsrArtifactManifest,
        lock: L,
        atomic: A,
    ) -> Self {
        Self {
            root: root.into(),
            manifest,
            known_manifests: &[],
            lock,
            atomic,
        }
    }

    /// Adds manifests for older revisions that pointers may still name.
    pub fn with_known_manifests(mut self, known_manifests: &'a [&'a AsrArtifactManifest]) -> Self {
        self.known_manifests = known_manifests;
        self
    }

    /// Resolves exactly one `current` snapshot. Staging is never inspected.
    pub fn resolve(&self) -> LifecycleState {
        match self.verified_pointer("current") {
            Ok(Some(_)) => LifecycleState::Ready,
            Ok(None) => LifecycleState::NotInstalled,
            Err(_) => LifecycleState::RepairRequired,
        }
    }

    pub fn publish(&self, stage_name: &str) -> Result<LifecycleState, LifecycleError> {
        let _guard = self.lock.try_acquire()?;
        self.validate_layout()?;
        let stage_name = safe_component(stage_name)?;
        let revision = safe_revision(self.manifest.revision)?;
        let stage = self.root.join("staging").join(stage_name);
        reject_symlink(&stage)?;
        fs::create_dir_all(self.root.join("revisions"))
            .map_err(|_| LifecycleError::Storage { retryable: true })?;
        let published = self.root.join("revisions").join(revision);
        if published.exists() {
            reject_symlink(&published)?;
            verify_model_set(&published, self.manifest)
                .map_err(|_| LifecycleError::VerificationFailed)?;
            if stage.exists() {
                verify_model_set(&stage, self.manifest)
                    .map_err(|_| LifecycleError::VerificationFailed)?;
                remove_owned_tree(&stage)?;
            }
        } else {
            verify_model_set(&stage, self.manifest)
                .map_err(|_| LifecycleError::VerificationFailed)?;
            fs::rename(&stage, &published)
                .map_err(|_| LifecycleError::Storage { retryable: true })?;
        }
        if let Some(old) = self.verified_pointer("current")? {
            self.write_pointer("previous", &old)?;
        }
        self.write_pointer("current", revision)?;
        Ok(LifecycleState::Ready)
    }

    pub fn recover(&self) -> Result<LifecycleState, LifecycleError> {
        let _guard = self.lock.try_acquire()?;
        self.validate_layout()?;
        if self.verified_pointer("current").ok().flatten().is_some() {
            return Ok(LifecycleState::Ready);
        }
        if let Some(previous) = self.verified_pointer("previous").ok().flatten() {
            self.write_pointer("current", &previous)?;
            return Ok(LifecycleState::Ready);
        }
        if !pointer_exists(&self.root.join("current"))?
            && !pointer_exists(&self.root.join("previous"))?
        {
            for manifest in
                std::iter::once(self.manifest).chain(self.known_manifests.iter().copied())
            {
                if safe_revision(manifest.revision).is_err() {
                    continue;
                }
                let published = self.root.join("revisions").join(manifest.revision);
                reject_symlink(&published)?;
                if published.exists() && verify_model_set(&published, manifest).is_ok() {
                    self.write_pointer("current", manifest.revision)?;
                    return Ok(LifecycleState::Ready);
                }
            }
        }
        Ok(
            if pointer_exists(&self.root.join("current"))?
                || pointer_exists(&self.root.join("previous"))?
                || directory_has_entries(&self.root.join("revisions"))?
            {
                LifecycleState::RepairRequired
            } else {
                LifecycleState::NotInstalled
            },
        )
    }

    pub fn rollback(&self) -> Result<LifecycleState, LifecycleError> {
        let _guard = self.lock.try_acquire()?;
        self.validate_layout()?;
        let previous = self
            .verified_pointer("previous")?
            .ok_or(LifecycleError::VerificationFailed)?;
        self.write_pointer("current", &previous)?;
        Ok(LifecycleState::Ready)
    }

    pub fn remove(&self) -> Result<LifecycleState, LifecycleError> {
        let _guard = self.lock.try_acquire()?;
        self.validate_layout().map_err(cleanup_error)?;
        // Inspect the complete owned tree before removing either usable pointer.
        for name in ["current", "previous", "revisions", "staging"] {
            preflight_owned(&self.root.join(name)).map_err(cleanup_error)?;
        }
        for pointer in ["current", "previous"] {
            let path = self.root.join(pointer);
            match fs::symlink_metadata(&path) {
                Ok(_) => self.atomic.remove(&path).map_err(cleanup_error)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(LifecycleError::Storage { retryable: true }),
            }
        }
        for directory in ["revisions", "staging"] {
            let path = self.root.join(directory);
            if path.exists() {
                remove_owned_tree(&path).map_err(cleanup_error)?;
            }
        }
        Ok(LifecycleState::NotInstalled)
    }

    fn verified_pointer(&self, name: &str) -> Result<Option<String>, LifecycleError> {
        validate_directory(&self.root, false)?;
        for directory in ["staging", "revisions"] {
            validate_directory(&self.root.join(directory), false)?;
        }
        let path = self.root.join(name);
        reject_symlink(&path)?;
        let value = match fs::read_to_string(path) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(LifecycleError::Storage { retryable: true }),
        };
        let revision = value.trim();
        safe_revision(revision).map_err(|_| LifecycleError::VerificationFailed)?;
        let manifest = self
            .manifest_for(revision)
            .ok_or(LifecycleError::VerificationFailed)?;
        if manifest.revision != revision {
            return Err(LifecycleError::VerificationFailed);
        }
        let directory = self.root.join("revisions").join(revision);
        reject_symlink(&directory)?;
        verify_model_set(&directory, manifest).map_err(|_| LifecycleError::VerificationFailed)?;
        Ok(Some(revision.to_owned()))
    }

    fn write_pointer(&self, name: &str, revision: &str) -> Result<(), LifecycleError> {
        fs::create_dir_all(&self.root).map_err(|_| LifecycleError::Storage { retryable: true })?;
        let temporary = self.root.join(format!(".{name}.tmp"));
        fs::write(&temporary, format!("{revision}\n"))
            .map_err(|_| LifecycleError::Storage { retryable: true })?;
        self.atomic.replace(&temporary, &self.root.join(name))
    }

    fn manifest_for(&self, revision: &str) -> Option<&AsrArtifactManifest> {
        std::iter::once(self.manifest)
            .chain(self.known_manifests.iter().copied())
            .find(|manifest| manifest.revision == revision)
    }

    fn validate_layout(&self) -> Result<(), LifecycleError> {
        validate_directory(&self.root, true)?;
        for name in ["staging", "revisions"] {
            validate_directory(&self.root.join(name), false)?;
        }
        Ok(())
    }
}

fn safe_component(value: &str) -> Result<&str, LifecycleError> {
    let mut components = Path::new(value).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) if !value.is_empty() => Ok(value),
        _ => Err(LifecycleError::InvalidStage),
    }
}

fn safe_revision(value: &str) -> Result<&str, LifecycleError> {
    if value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(value)
    } else {
        Err(LifecycleError::InvalidStage)
    }
}

fn validate_directory(path: &Path, required: bool) -> Result<(), LifecycleError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(LifecycleError::UnsafeEntry),
        Err(error) if !required && error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) if required && error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|_| LifecycleError::Storage { retryable: true })
        }
        Err(_) => Err(LifecycleError::Storage { retryable: true }),
    }
}

fn cleanup_error(_: LifecycleError) -> LifecycleError {
    LifecycleError::Storage { retryable: true }
}

fn preflight_owned(path: &Path) -> Result<(), LifecycleError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(LifecycleError::Storage { retryable: true }),
    };
    if metadata.file_type().is_symlink() {
        return Err(LifecycleError::UnsafeEntry);
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path).map_err(|_| LifecycleError::Storage { retryable: true })? {
            preflight_owned(
                &entry
                    .map_err(|_| LifecycleError::Storage { retryable: true })?
                    .path(),
            )?;
        }
    }
    Ok(())
}

fn reject_symlink(path: &Path) -> Result<(), LifecycleError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(LifecycleError::UnsafeEntry),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(LifecycleError::Storage { retryable: true }),
    }
}

fn pointer_exists(path: &Path) -> Result<bool, LifecycleError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(LifecycleError::Storage { retryable: true }),
    }
}

fn directory_has_entries(path: &Path) -> Result<bool, LifecycleError> {
    match fs::read_dir(path) {
        Ok(mut entries) => entries
            .next()
            .transpose()
            .map(|entry| entry.is_some())
            .map_err(|_| LifecycleError::Storage { retryable: true }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(LifecycleError::Storage { retryable: true }),
    }
}

fn remove_owned_tree(path: &Path) -> Result<(), LifecycleError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| LifecycleError::Storage { retryable: true })?;
    if metadata.file_type().is_symlink() {
        return Err(LifecycleError::UnsafeEntry);
    }
    if metadata.is_file() {
        return fs::remove_file(path).map_err(|_| LifecycleError::Storage { retryable: true });
    }
    for entry in fs::read_dir(path).map_err(|_| LifecycleError::Storage { retryable: true })? {
        let entry = entry.map_err(|_| LifecycleError::Storage { retryable: true })?;
        let metadata = entry
            .file_type()
            .map_err(|_| LifecycleError::Storage { retryable: true })?;
        if metadata.is_symlink() {
            return Err(LifecycleError::UnsafeEntry);
        }
        remove_owned_tree(&entry.path())?;
    }
    fs::remove_dir(path).map_err(|_| LifecycleError::Storage { retryable: true })
}
