use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use muniment_core::asr::acquisition::{
    AsrAcquisitionError, AsrAcquisitionLimits, AsrAcquisitionRuntime, SOURCE_REPOSITORY,
};
use muniment_core::asr::install::install_parakeet_revision;
use muniment_core::asr::{
    AsrRecovery, AsrRevisionLifecycle, PARAKEET_MODEL_MANIFEST, PARAKEET_MODEL_MANIFESTS,
};
use muniment_core::model_acquisition_transport::NativeModelAcquisitionTransport;
use muniment_core::model_install::{required_free_bytes, ModelInstallError};
use muniment_core::model_install_native::{
    NativeAcquisitionClock, NativeAsrLifecycleBoundary, NativeAvailableSpace,
    NativeInstallCancellation, NativeInstallLock, NativeRetryWait,
};
use serde::Serialize;
use tauri::State;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum ParakeetInstallStatus {
    NotInstalled,
    Installing {
        completed_bytes: u64,
        total_bytes: u64,
    },
    Installed,
    Cancelled,
    Failed {
        category: &'static str,
        message: &'static str,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParakeetInstallFacts {
    identity: &'static str,
    revision: &'static str,
    source_repository: &'static str,
    total_download_bytes: u64,
    required_free_bytes: u64,
    speech_model_license: &'static str,
    voice_activity_model_license: &'static str,
}

#[tauri::command]
pub fn parakeet_install_facts() -> ParakeetInstallFacts {
    let total_download_bytes = PARAKEET_MODEL_MANIFEST
        .artifacts
        .iter()
        .map(|artifact| artifact.byte_size)
        .chain(
            PARAKEET_MODEL_MANIFEST
                .additional_artifact
                .iter()
                .map(|artifact| artifact.artifact.byte_size),
        )
        .try_fold(0_u64, u64::checked_add)
        .expect("the compiled Parakeet manifest size must fit in u64");
    ParakeetInstallFacts {
        identity: PARAKEET_MODEL_MANIFEST.identity,
        revision: PARAKEET_MODEL_MANIFEST.revision,
        source_repository: SOURCE_REPOSITORY,
        total_download_bytes,
        required_free_bytes: required_free_bytes(total_download_bytes)
            .expect("the compiled Parakeet manifest size must fit in u64"),
        speech_model_license: "CC BY 4.0",
        voice_activity_model_license: "MIT",
    }
}

#[derive(Clone, Copy, Debug)]
struct InstallFailure {
    category: &'static str,
    message: &'static str,
    cancelled: bool,
}

type ParakeetRunner = dyn Fn(&Path, &NativeInstallCancellation, &mut dyn FnMut(u64, u64)) -> Result<(), InstallFailure>
    + Send
    + Sync;

struct ParakeetActiveInstall {
    generation: u64,
    cancellation: NativeInstallCancellation,
}

struct ParakeetInner {
    generation: u64,
    state_version: u64,
    status: ParakeetInstallStatus,
    terminal_result: bool,
    completed_success: bool,
    active: Option<ParakeetActiveInstall>,
}

pub struct ParakeetInstallState {
    root: PathBuf,
    inner: Arc<Mutex<ParakeetInner>>,
    runner: Arc<ParakeetRunner>,
}

impl ParakeetInstallState {
    pub fn new(root: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(root.join("staging"))?;
        let status = inspect_parakeet(&root);
        Ok(Self::with_runner(
            root,
            status,
            Arc::new(run_native_parakeet_install),
        ))
    }

    fn with_runner(
        root: PathBuf,
        status: ParakeetInstallStatus,
        runner: Arc<ParakeetRunner>,
    ) -> Self {
        Self {
            root,
            inner: Arc::new(Mutex::new(ParakeetInner {
                generation: 0,
                state_version: 0,
                status,
                terminal_result: false,
                completed_success: false,
                active: None,
            })),
            runner,
        }
    }

    async fn status(&self) -> ParakeetInstallStatus {
        let state_version = {
            let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
            if inner.active.is_some() || inner.terminal_result {
                return inner.status.clone();
            }
            if inner.completed_success {
                inner.completed_success = false;
                return inner.status.clone();
            }
            inner.state_version
        };
        let root = self.root.clone();
        let inspected = tauri::async_runtime::spawn_blocking(move || inspect_parakeet(&root))
            .await
            .unwrap_or(ParakeetInstallStatus::Failed {
                category: "inspectionFailed",
                message: "The speech model installation could not be inspected.",
            });
        let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        if inner.active.is_none() && inner.state_version == state_version {
            inner.state_version = inner.state_version.wrapping_add(1);
            inner.status = inspected;
        }
        inner.status.clone()
    }

    fn start(&self) -> ParakeetInstallStatus {
        let (generation, cancellation) = {
            let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
            if inner.active.is_some() {
                return inner.status.clone();
            }
            inner.generation = inner.generation.wrapping_add(1);
            inner.state_version = inner.state_version.wrapping_add(1);
            let generation = inner.generation;
            let cancellation = NativeInstallCancellation::new();
            inner.active = Some(ParakeetActiveInstall {
                generation,
                cancellation: cancellation.clone(),
            });
            inner.terminal_result = false;
            inner.completed_success = false;
            inner.status = ParakeetInstallStatus::Installing {
                completed_bytes: 0,
                total_bytes: 0,
            };
            (generation, cancellation)
        };
        let root = self.root.clone();
        let inner = Arc::clone(&self.inner);
        let runner = Arc::clone(&self.runner);
        tauri::async_runtime::spawn_blocking(move || {
            let progress_inner = Arc::clone(&inner);
            let mut progress = move |completed_bytes, total_bytes| {
                update_progress(&progress_inner, generation, completed_bytes, total_bytes);
            };
            let result = runner(&root, &cancellation, &mut progress);
            let mut state = inner.lock().unwrap_or_else(|error| error.into_inner());
            if !matches!(state.active, Some(ref active) if active.generation == generation) {
                return;
            }
            state.active = None;
            state.state_version = state.state_version.wrapping_add(1);
            state.completed_success = result.is_ok();
            let (status, terminal_result) = match result {
                Ok(()) => (ParakeetInstallStatus::Installed, false),
                Err(error) if error.cancelled => (ParakeetInstallStatus::Cancelled, true),
                Err(error) => (
                    ParakeetInstallStatus::Failed {
                        category: error.category,
                        message: error.message,
                    },
                    true,
                ),
            };
            state.status = status;
            state.terminal_result = terminal_result;
        });
        ParakeetInstallStatus::Installing {
            completed_bytes: 0,
            total_bytes: 0,
        }
    }

    fn cancel(&self) -> ParakeetInstallStatus {
        let inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(active) = &inner.active {
            active.cancellation.cancel();
        }
        inner.status.clone()
    }
}

fn update_progress(
    inner: &Arc<Mutex<ParakeetInner>>,
    generation: u64,
    completed_bytes: u64,
    total_bytes: u64,
) {
    let mut state = inner.lock().unwrap_or_else(|error| error.into_inner());
    if !matches!(state.active, Some(ref active) if active.generation == generation) {
        return;
    }
    let ParakeetInstallStatus::Installing {
        completed_bytes: current_completed,
        total_bytes: current_total,
    } = &state.status
    else {
        return;
    };
    let current_completed = *current_completed;
    let current_total = *current_total;
    let total_bytes = total_bytes.max(current_total).max(current_completed);
    state.status = ParakeetInstallStatus::Installing {
        completed_bytes: completed_bytes.max(current_completed).min(total_bytes),
        total_bytes,
    };
}

pub(crate) fn parakeet_lifecycle(root: &Path) -> AsrRevisionLifecycle {
    AsrRevisionLifecycle::new(
        root.to_owned(),
        &PARAKEET_MODEL_MANIFESTS,
        &PARAKEET_MODEL_MANIFEST,
    )
    .expect("the compiled Parakeet manifest must be valid")
}

fn inspect_parakeet(root: &Path) -> ParakeetInstallStatus {
    match parakeet_lifecycle(root).recover(&NativeAsrLifecycleBoundary) {
        Ok(AsrRecovery::Current(_) | AsrRecovery::RestoredPrevious(_)) => {
            ParakeetInstallStatus::Installed
        }
        Ok(AsrRecovery::NotInstalled) => ParakeetInstallStatus::NotInstalled,
        Ok(AsrRecovery::RepairRequired) => ParakeetInstallStatus::Failed {
            category: "invalidInstall",
            message: "The installed speech model could not be verified.",
        },
        Err(_) => ParakeetInstallStatus::Failed {
            category: "inspectionFailed",
            message: "The speech model installation could not be inspected.",
        },
    }
}

fn run_native_parakeet_install(
    root: &Path,
    cancellation: &NativeInstallCancellation,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(), InstallFailure> {
    let staging = root.join("staging");
    let mut transport = NativeModelAcquisitionTransport::new();
    let clock = NativeAcquisitionClock::new();
    let mut retry = NativeRetryWait;
    let mut report_progress = |completed_bytes, total_bytes| {
        progress(completed_bytes, total_bytes);
    };
    let mut lock = NativeInstallLock::new(root.join("install.lock"));
    let mut space = NativeAvailableSpace::new(root);
    install_parakeet_revision(
        &staging,
        PARAKEET_MODEL_MANIFEST.identity,
        &PARAKEET_MODEL_MANIFEST,
        AsrAcquisitionLimits::default(),
        &mut transport,
        AsrAcquisitionRuntime {
            clock: &clock,
            retry_wait: &mut retry,
        },
        cancellation,
        &mut lock,
        &mut space,
        &parakeet_lifecycle(root),
        &NativeAsrLifecycleBoundary,
        &mut report_progress,
    )
    .map(|_| ())
    .map_err(redact_parakeet_failure)
}

fn redact_parakeet_failure(
    error: muniment_core::asr::install::ParakeetInstallError,
) -> InstallFailure {
    let cancelled = matches!(
        error,
        ModelInstallError::Cancelled
            | ModelInstallError::Acquisition(AsrAcquisitionError::Cancelled)
    );
    if cancelled {
        return InstallFailure {
            category: "cancelled",
            message: "Speech model installation was cancelled.",
            cancelled: true,
        };
    }
    let (category, message) = match error {
        ModelInstallError::StorageInsufficient { .. } => (
            "insufficientStorage",
            "There is not enough storage to install the speech model.",
        ),
        ModelInstallError::StorageUnknown | ModelInstallError::StorageRequirementOverflow => (
            "storageUnavailable",
            "Available storage could not be determined.",
        ),
        ModelInstallError::Acquisition(_) => {
            ("downloadFailed", "The speech model download failed.")
        }
        ModelInstallError::Publication(_) => (
            "publicationFailed",
            "The downloaded speech model could not be installed.",
        ),
        ModelInstallError::Lock(_) => (
            "installUnavailable",
            "Speech model installation is unavailable.",
        ),
        ModelInstallError::Cancelled => unreachable!(),
    };
    InstallFailure {
        category,
        message,
        cancelled: false,
    }
}

#[tauri::command]
pub fn parakeet_install_start(state: State<'_, ParakeetInstallState>) -> ParakeetInstallStatus {
    state.start()
}

#[tauri::command]
pub async fn parakeet_install_status(
    state: State<'_, ParakeetInstallState>,
) -> Result<ParakeetInstallStatus, String> {
    Ok(state.status().await)
}

#[tauri::command]
pub fn parakeet_install_cancel(state: State<'_, ParakeetInstallState>) -> ParakeetInstallStatus {
    state.cancel()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress_inner(generation: u64) -> Arc<Mutex<ParakeetInner>> {
        Arc::new(Mutex::new(ParakeetInner {
            generation,
            state_version: 0,
            status: ParakeetInstallStatus::Installing {
                completed_bytes: 0,
                total_bytes: 0,
            },
            terminal_result: false,
            completed_success: false,
            active: Some(ParakeetActiveInstall {
                generation,
                cancellation: NativeInstallCancellation::new(),
            }),
        }))
    }

    #[test]
    fn active_install_progress_is_monotonic_and_bounded() {
        let inner = progress_inner(2);

        update_progress(&inner, 2, 40, 100);
        update_progress(&inner, 2, 20, 80);
        update_progress(&inner, 2, 140, 100);

        assert_eq!(
            inner.lock().unwrap().status,
            ParakeetInstallStatus::Installing {
                completed_bytes: 100,
                total_bytes: 100,
            }
        );
    }

    #[test]
    fn stale_install_progress_cannot_change_the_active_generation() {
        let inner = progress_inner(3);

        update_progress(&inner, 3, 10, 100);
        update_progress(&inner, 2, 90, 100);

        assert_eq!(
            inner.lock().unwrap().status,
            ParakeetInstallStatus::Installing {
                completed_bytes: 10,
                total_bytes: 100,
            }
        );
    }

    #[test]
    fn install_facts_match_the_pinned_manifest() {
        let facts = parakeet_install_facts();
        let total_download_bytes = PARAKEET_MODEL_MANIFEST
            .artifacts
            .iter()
            .map(|artifact| artifact.byte_size)
            .sum::<u64>()
            + PARAKEET_MODEL_MANIFEST
                .additional_artifact
                .map(|artifact| artifact.artifact.byte_size)
                .unwrap_or(0);

        assert_eq!(facts.identity, PARAKEET_MODEL_MANIFEST.identity);
        assert_eq!(facts.revision, PARAKEET_MODEL_MANIFEST.revision);
        assert_eq!(facts.source_repository, SOURCE_REPOSITORY);
        assert_eq!(total_download_bytes, 672_384_307);
        assert_eq!(facts.total_download_bytes, total_download_bytes);
        assert_eq!(
            facts.required_free_bytes,
            required_free_bytes(total_download_bytes).unwrap()
        );
        assert_eq!(facts.required_free_bytes, 940_819_763);
        assert_eq!(facts.speech_model_license, "CC BY 4.0");
        assert_eq!(facts.voice_activity_model_license, "MIT");
    }
}
