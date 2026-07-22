//! Bounded, resumable acquisition of the pinned Parakeet model set.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{
    verify_kokoro_artifact, verify_revision_stage, KokoroArtifactDescriptor,
    KokoroRevisionDescriptor, KokoroVerificationError,
};

const READ_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KokoroAcquisitionLimits {
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub deadline: Duration,
    pub max_attempts: u8,
}

impl Default for KokoroAcquisitionLimits {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            read_timeout: Duration::from_secs(30),
            deadline: Duration::from_secs(30 * 60),
            max_attempts: 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KokoroDownloadRequest {
    url: String,
    pub artifact_index: usize,
    pub offset: u64,
    pub limits: KokoroAcquisitionLimits,
}

impl KokoroDownloadRequest {
    pub fn url(&self) -> &str {
        &self.url
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn for_transport_test(
        url: String,
        offset: u64,
        limits: KokoroAcquisitionLimits,
    ) -> Self {
        Self {
            url,
            artifact_index: 0,
            offset,
            limits,
        }
    }
}

pub struct KokoroDownloadResponse<R> {
    pub status: u16,
    /// Inclusive response range `(first, last, complete_length)` for a 206.
    pub content_range: Option<(u64, u64, u64)>,
    pub body: R,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KokoroTransportError {
    Transient,
    Unavailable,
    Rejected,
}

pub trait KokoroDownloadTransport {
    type Body: Read;
    fn download(
        &mut self,
        request: &KokoroDownloadRequest,
    ) -> Result<KokoroDownloadResponse<Self::Body>, KokoroTransportError>;
}

pub trait KokoroCancellation {
    fn is_cancelled(&self) -> bool;
}
impl<F: Fn() -> bool> KokoroCancellation for F {
    fn is_cancelled(&self) -> bool {
        self()
    }
}

pub trait KokoroAcquisitionClock {
    fn now(&self) -> Duration;
}
impl<F: Fn() -> Duration> KokoroAcquisitionClock for F {
    fn now(&self) -> Duration {
        self()
    }
}

pub trait KokoroRetryWait {
    fn wait(&mut self, maximum_delay: Duration, cancellation: &dyn KokoroCancellation) -> bool;
}
impl<F> KokoroRetryWait for F
where
    F: FnMut(Duration, &dyn KokoroCancellation) -> bool,
{
    fn wait(&mut self, delay: Duration, cancellation: &dyn KokoroCancellation) -> bool {
        self(delay, cancellation)
    }
}

pub struct KokoroAcquisitionRuntime<'a, K, W> {
    pub clock: &'a K,
    pub retry_wait: &'a mut W,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KokoroDownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KokoroAcquisitionError {
    InvalidStage,
    InvalidLimits,
    Cancelled,
    Retryable,
    Unavailable,
    Rejected,
    InvalidResponse,
    TooLarge,
    Verification(KokoroVerificationError),
    Persistence,
}

impl std::fmt::Display for KokoroAcquisitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidStage => "Kokoro model stage is invalid",
            Self::InvalidLimits => "Kokoro model acquisition limits are invalid",
            Self::Cancelled => "Kokoro model download was cancelled",
            Self::Retryable => "Kokoro model download can be retried",
            Self::Unavailable => "Kokoro model artifact is unavailable",
            Self::Rejected => "Kokoro model download was rejected",
            Self::InvalidResponse => "Kokoro model server response is invalid",
            Self::TooLarge => "Kokoro model download exceeded its pinned size",
            Self::Verification(_) => "Kokoro model download failed verification",
            Self::Persistence => "Kokoro model stage could not be persisted",
        })
    }
}
impl std::error::Error for KokoroAcquisitionError {}

/// Returns the aggregate pinned bytes not yet present in a resumable stage.
pub fn remaining_stage_bytes(
    staging_root: &Path,
    install_id: &str,
    manifest: &KokoroRevisionDescriptor,
) -> Result<u64, KokoroAcquisitionError> {
    if !safe_component(install_id) {
        return Err(KokoroAcquisitionError::InvalidStage);
    }
    require_directory(staging_root)?;
    let stage = staging_root.join(install_id);
    match fs::symlink_metadata(&stage) {
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => return Err(KokoroAcquisitionError::InvalidStage),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return manifest
                .artifacts
                .iter()
                .try_fold(0_u64, |total, artifact| {
                    total
                        .checked_add(artifact.byte_size)
                        .ok_or(KokoroAcquisitionError::TooLarge)
                })
        }
        Err(_) => return Err(KokoroAcquisitionError::Persistence),
    }

    manifest
        .artifacts
        .iter()
        .try_fold(0_u64, |total, artifact| {
            let completed = stage.join(artifact.filename);
            let missing = match fs::symlink_metadata(&completed) {
                Ok(metadata) if !metadata.file_type().is_file() => {
                    return Err(KokoroAcquisitionError::InvalidStage)
                }
                Ok(_) if verify_kokoro_artifact(&completed, artifact).is_ok() => 0,
                Ok(_) => artifact
                    .byte_size
                    .checked_sub(strict_part_length(
                        &stage.join(format!("{}.part", artifact.filename)),
                        artifact.byte_size,
                    )?)
                    .ok_or(KokoroAcquisitionError::TooLarge)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => artifact
                    .byte_size
                    .checked_sub(strict_part_length(
                        &stage.join(format!("{}.part", artifact.filename)),
                        artifact.byte_size,
                    )?)
                    .ok_or(KokoroAcquisitionError::TooLarge)?,
                Err(_) => return Err(KokoroAcquisitionError::Persistence),
            };
            total
                .checked_add(missing)
                .ok_or(KokoroAcquisitionError::TooLarge)
        })
}

/// Returns `staging/<id>` only after every pinned artifact verifies.
pub fn acquire_kokoro_stage<T, C, K, W>(
    staging_root: &Path,
    install_id: &str,
    manifest: &'static KokoroRevisionDescriptor,
    limits: KokoroAcquisitionLimits,
    transport: &mut T,
    runtime: KokoroAcquisitionRuntime<'_, K, W>,
    cancellation: &C,
) -> Result<PathBuf, KokoroAcquisitionError>
where
    T: KokoroDownloadTransport,
    C: KokoroCancellation,
    K: KokoroAcquisitionClock,
    W: KokoroRetryWait,
{
    acquire_kokoro_stage_with_progress(
        staging_root,
        install_id,
        manifest,
        limits,
        transport,
        runtime,
        cancellation,
        &mut |_| {},
    )
}

#[allow(clippy::too_many_arguments)]
pub fn acquire_kokoro_stage_with_progress<T, C, K, W>(
    staging_root: &Path,
    install_id: &str,
    manifest: &'static KokoroRevisionDescriptor,
    limits: KokoroAcquisitionLimits,
    transport: &mut T,
    runtime: KokoroAcquisitionRuntime<'_, K, W>,
    cancellation: &C,
    progress: &mut dyn FnMut(KokoroDownloadProgress),
) -> Result<PathBuf, KokoroAcquisitionError>
where
    T: KokoroDownloadTransport,
    C: KokoroCancellation,
    K: KokoroAcquisitionClock,
    W: KokoroRetryWait,
{
    if !safe_component(install_id) {
        return Err(KokoroAcquisitionError::InvalidStage);
    }
    if limits.max_attempts == 0
        || limits.connect_timeout.is_zero()
        || limits.read_timeout.is_zero()
        || limits.deadline.is_zero()
    {
        return Err(KokoroAcquisitionError::InvalidLimits);
    }
    let started_at = runtime.clock.now();
    require_directory(staging_root)?;
    let stage = staging_root.join(install_id);
    match fs::create_dir(&stage) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            require_directory(&stage)?
        }
        Err(_) => return Err(KokoroAcquisitionError::Persistence),
    }

    let total_bytes = manifest
        .artifacts
        .iter()
        .try_fold(0_u64, |total, artifact| {
            total.checked_add(artifact.byte_size)
        })
        .ok_or(KokoroAcquisitionError::TooLarge)?;
    for (artifact_index, artifact) in manifest.artifacts.iter().enumerate() {
        let completed_before = manifest.artifacts[..artifact_index]
            .iter()
            .try_fold(0_u64, |total, item| total.checked_add(item.byte_size))
            .ok_or(KokoroAcquisitionError::TooLarge)?;
        acquire_artifact(
            &stage,
            artifact_index,
            artifact,
            manifest,
            limits,
            transport,
            runtime.clock,
            runtime.retry_wait,
            started_at,
            cancellation,
            completed_before,
            total_bytes,
            progress,
        )?;
    }
    verify_revision_stage(&stage, manifest).map_err(KokoroAcquisitionError::Verification)?;
    progress(KokoroDownloadProgress {
        downloaded_bytes: total_bytes,
        total_bytes,
    });
    Ok(stage)
}

#[allow(clippy::too_many_arguments)]
fn acquire_artifact<T, C, K, W>(
    stage: &Path,
    artifact_index: usize,
    artifact: &KokoroArtifactDescriptor,
    _manifest: &KokoroRevisionDescriptor,
    limits: KokoroAcquisitionLimits,
    transport: &mut T,
    clock: &K,
    retry_wait: &mut W,
    started_at: Duration,
    cancellation: &C,
    completed_before: u64,
    total_bytes: u64,
    progress: &mut dyn FnMut(KokoroDownloadProgress),
) -> Result<(), KokoroAcquisitionError>
where
    T: KokoroDownloadTransport,
    C: KokoroCancellation,
    K: KokoroAcquisitionClock,
    W: KokoroRetryWait,
{
    let completed = stage.join(artifact.filename);
    match fs::symlink_metadata(&completed) {
        Ok(metadata)
            if metadata.file_type().is_file()
                && verify_kokoro_artifact(&completed, artifact).is_ok() =>
        {
            return Ok(())
        }
        Ok(_) => remove_file(&completed)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(KokoroAcquisitionError::Persistence),
    }
    let part = stage.join(format!("{}.part", artifact.filename));

    for attempt in 0..limits.max_attempts {
        if cancellation.is_cancelled() {
            return Err(KokoroAcquisitionError::Cancelled);
        }
        let remaining = remaining_budget(clock, started_at, limits.deadline)?;
        let offset = part_length(&part, artifact.byte_size)?;
        if offset == artifact.byte_size {
            return finish_artifact(&part, &completed, artifact);
        }
        let request = KokoroDownloadRequest {
            url: artifact.source_url.to_owned(),
            artifact_index,
            offset,
            limits: KokoroAcquisitionLimits {
                connect_timeout: limits.connect_timeout.min(remaining),
                read_timeout: limits.read_timeout.min(remaining),
                deadline: remaining,
                max_attempts: limits.max_attempts,
            },
        };
        let response = match transport.download(&request) {
            Ok(response) => response,
            Err(KokoroTransportError::Transient) if attempt + 1 < limits.max_attempts => {
                wait_before_retry(
                    retry_wait,
                    clock,
                    started_at,
                    limits.deadline,
                    attempt,
                    cancellation,
                )?;
                continue;
            }
            Err(KokoroTransportError::Transient) => return Err(KokoroAcquisitionError::Retryable),
            Err(KokoroTransportError::Unavailable) => {
                return Err(KokoroAcquisitionError::Unavailable)
            }
            Err(KokoroTransportError::Rejected) => return Err(KokoroAcquisitionError::Rejected),
        };
        let append = match validate_response(&response, offset, artifact.byte_size) {
            Ok(value) => value,
            Err(KokoroAcquisitionError::InvalidResponse) => {
                remove_file(&part)?;
                if attempt + 1 < limits.max_attempts {
                    wait_before_retry(
                        retry_wait,
                        clock,
                        started_at,
                        limits.deadline,
                        attempt,
                        cancellation,
                    )?;
                    continue;
                }
                return Err(KokoroAcquisitionError::Retryable);
            }
            Err(KokoroAcquisitionError::Retryable) if attempt + 1 < limits.max_attempts => {
                wait_before_retry(
                    retry_wait,
                    clock,
                    started_at,
                    limits.deadline,
                    attempt,
                    cancellation,
                )?;
                continue;
            }
            Err(error) => return Err(error),
        };
        if !append {
            remove_file(&part)?;
        }
        match stream_response(
            response.body,
            &part,
            artifact.byte_size,
            clock,
            started_at,
            limits.deadline,
            cancellation,
            completed_before,
            total_bytes,
            progress,
        ) {
            Ok(true) => return finish_artifact(&part, &completed, artifact),
            Ok(false) | Err(KokoroAcquisitionError::Retryable)
                if attempt + 1 < limits.max_attempts =>
            {
                wait_before_retry(
                    retry_wait,
                    clock,
                    started_at,
                    limits.deadline,
                    attempt,
                    cancellation,
                )?;
            }
            Ok(false) | Err(KokoroAcquisitionError::Retryable) => {
                return Err(KokoroAcquisitionError::Retryable)
            }
            Err(error) => return Err(error),
        }
    }
    Err(KokoroAcquisitionError::Retryable)
}

fn validate_response<R>(
    response: &KokoroDownloadResponse<R>,
    offset: u64,
    expected: u64,
) -> Result<bool, KokoroAcquisitionError> {
    match response.status {
        200 if response.content_range.is_none() => Ok(false),
        206 => match response.content_range {
            Some((first, last, total))
                if first == offset
                    && total == expected
                    && last >= first
                    && last == expected.saturating_sub(1) =>
            {
                Ok(true)
            }
            _ => Err(KokoroAcquisitionError::InvalidResponse),
        },
        416 => Err(KokoroAcquisitionError::InvalidResponse),
        500..=599 => Err(KokoroAcquisitionError::Retryable),
        404 | 410 => Err(KokoroAcquisitionError::Unavailable),
        _ => Err(KokoroAcquisitionError::Rejected),
    }
}

#[allow(clippy::too_many_arguments)]
fn stream_response<R: Read, C: KokoroCancellation, K: KokoroAcquisitionClock>(
    mut body: R,
    part: &Path,
    expected: u64,
    clock: &K,
    started_at: Duration,
    deadline: Duration,
    cancellation: &C,
    completed_before: u64,
    total_bytes: u64,
    progress: &mut dyn FnMut(KokoroDownloadProgress),
) -> Result<bool, KokoroAcquisitionError> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(part)
        .map_err(|_| KokoroAcquisitionError::Persistence)?;
    let mut total = file
        .metadata()
        .map_err(|_| KokoroAcquisitionError::Persistence)?
        .len();
    let mut buffer = [0_u8; READ_BUFFER_BYTES];
    loop {
        if cancellation.is_cancelled() {
            file.flush()
                .map_err(|_| KokoroAcquisitionError::Persistence)?;
            return Err(KokoroAcquisitionError::Cancelled);
        }
        remaining_budget(clock, started_at, deadline)?;
        let count = body
            .read(&mut buffer)
            .map_err(|_| KokoroAcquisitionError::Retryable)?;
        remaining_budget(clock, started_at, deadline)?;
        if count == 0 {
            file.flush()
                .map_err(|_| KokoroAcquisitionError::Persistence)?;
            return Ok(total == expected);
        }
        total = total
            .checked_add(count as u64)
            .ok_or(KokoroAcquisitionError::TooLarge)?;
        if total > expected {
            drop(file);
            remove_file(part)?;
            return Err(KokoroAcquisitionError::TooLarge);
        }
        file.write_all(&buffer[..count])
            .map_err(|_| KokoroAcquisitionError::Persistence)?;
        progress(KokoroDownloadProgress {
            downloaded_bytes: completed_before + total,
            total_bytes,
        });
    }
}

fn finish_artifact(
    part: &Path,
    completed: &Path,
    artifact: &KokoroArtifactDescriptor,
) -> Result<(), KokoroAcquisitionError> {
    if let Err(error) = verify_kokoro_artifact(part, artifact) {
        remove_file(part)?;
        return Err(KokoroAcquisitionError::Verification(error));
    }
    fs::rename(part, completed).map_err(|_| KokoroAcquisitionError::Persistence)
}

fn wait_before_retry<W: KokoroRetryWait, K: KokoroAcquisitionClock>(
    retry_wait: &mut W,
    clock: &K,
    started_at: Duration,
    deadline: Duration,
    attempt: u8,
    cancellation: &dyn KokoroCancellation,
) -> Result<(), KokoroAcquisitionError> {
    if cancellation.is_cancelled() {
        return Err(KokoroAcquisitionError::Cancelled);
    }
    let remaining = remaining_budget(clock, started_at, deadline)?;
    let delay = Duration::from_secs(1_u64 << attempt.min(2)).min(remaining);
    if !retry_wait.wait(delay, cancellation) || cancellation.is_cancelled() {
        return Err(KokoroAcquisitionError::Cancelled);
    }
    remaining_budget(clock, started_at, deadline).map(|_| ())
}

fn remaining_budget<K: KokoroAcquisitionClock>(
    clock: &K,
    started_at: Duration,
    deadline: Duration,
) -> Result<Duration, KokoroAcquisitionError> {
    deadline
        .checked_sub(clock.now().saturating_sub(started_at))
        .filter(|value| !value.is_zero())
        .ok_or(KokoroAcquisitionError::Retryable)
}

fn part_length(path: &Path, maximum: u64) -> Result<u64, KokoroAcquisitionError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(_) => return Err(KokoroAcquisitionError::Persistence),
    };
    if !metadata.file_type().is_file() {
        return Err(KokoroAcquisitionError::InvalidStage);
    }
    if metadata.len() > maximum {
        remove_file(path)?;
        return Ok(0);
    }
    Ok(metadata.len())
}

fn strict_part_length(path: &Path, maximum: u64) -> Result<u64, KokoroAcquisitionError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(_) => return Err(KokoroAcquisitionError::Persistence),
    };
    if !metadata.file_type().is_file() {
        return Err(KokoroAcquisitionError::InvalidStage);
    }
    if metadata.len() > maximum {
        return Err(KokoroAcquisitionError::TooLarge);
    }
    Ok(metadata.len())
}

fn remove_file(path: &Path) -> Result<(), KokoroAcquisitionError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(KokoroAcquisitionError::Persistence),
    }
}

fn require_directory(path: &Path) -> Result<(), KokoroAcquisitionError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(()),
        _ => Err(KokoroAcquisitionError::InvalidStage),
    }
}
fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains(['/', '\\'])
        && !Path::new(value).is_absolute()
}
