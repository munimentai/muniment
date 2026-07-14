use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use muniment_core::llama::acquisition::{
    GemmaAcquisitionError, GemmaAcquisitionLimits, GemmaAcquisitionRuntime,
};
use muniment_core::llama::install::install_gemma_revision;
use muniment_core::llama::lifecycle::{
    GemmaRecovery, GemmaRevisionLifecycle, RESIDENT_GEMMA_REVISION, RESIDENT_GEMMA_REVISIONS,
};
use muniment_core::model_acquisition_transport::NativeModelAcquisitionTransport;
use muniment_core::model_install::ModelInstallError;
use muniment_core::model_install_native::{
    NativeAcquisitionClock, NativeAvailableSpace, NativeGemmaLifecycleBoundary,
    NativeInstallCancellation, NativeInstallLock, NativeRetryWait,
};
use serde::Serialize;
use tauri::State;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum GemmaInstallStatus {
    NotInstalled,
    Installing,
    Installed,
    Cancelled,
    Failed {
        category: &'static str,
        message: &'static str,
    },
}

#[derive(Clone, Copy, Debug)]
struct InstallFailure {
    category: &'static str,
    message: &'static str,
    cancelled: bool,
}

type Runner = dyn Fn(&Path, &NativeInstallCancellation) -> Result<(), InstallFailure> + Send + Sync;

struct ActiveInstall {
    generation: u64,
    cancellation: NativeInstallCancellation,
}

struct Inner {
    generation: u64,
    state_version: u64,
    status: GemmaInstallStatus,
    active: Option<ActiveInstall>,
}

pub struct GemmaInstallState {
    root: PathBuf,
    inner: Arc<Mutex<Inner>>,
    runner: Arc<Runner>,
}

impl GemmaInstallState {
    pub fn new(root: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(root.join("staging"))?;
        let status = inspect(&root);
        Ok(Self::with_runner(
            root,
            status,
            Arc::new(run_native_install),
        ))
    }

    fn with_runner(root: PathBuf, status: GemmaInstallStatus, runner: Arc<Runner>) -> Self {
        Self {
            root,
            inner: Arc::new(Mutex::new(Inner {
                generation: 0,
                state_version: 0,
                status,
                active: None,
            })),
            runner,
        }
    }

    fn cached_status(&self) -> GemmaInstallStatus {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .status
            .clone()
    }

    async fn status(&self) -> GemmaInstallStatus {
        let state_version = {
            let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if inner.active.is_some() {
                return inner.status.clone();
            }
            inner.state_version
        };

        let root = self.root.clone();
        let inspected = tauri::async_runtime::spawn_blocking(move || inspect(&root))
            .await
            .unwrap_or(GemmaInstallStatus::Failed {
                category: "inspectionFailed",
                message: "The model installation could not be inspected.",
            });
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.active.is_none() && inner.state_version == state_version {
            inner.state_version = inner.state_version.wrapping_add(1);
            inner.status = inspected;
        }
        inner.status.clone()
    }

    fn start(&self) -> GemmaInstallStatus {
        let (generation, cancellation) = {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if inner.active.is_some() {
                return inner.status.clone();
            }
            inner.generation = inner.generation.wrapping_add(1);
            inner.state_version = inner.state_version.wrapping_add(1);
            let generation = inner.generation;
            let cancellation = NativeInstallCancellation::new();
            inner.active = Some(ActiveInstall {
                generation,
                cancellation: cancellation.clone(),
            });
            inner.status = GemmaInstallStatus::Installing;
            (generation, cancellation)
        };

        let root = self.root.clone();
        let inner = Arc::clone(&self.inner);
        let runner = Arc::clone(&self.runner);
        tauri::async_runtime::spawn_blocking(move || {
            let result = runner(&root, &cancellation);
            let mut state = inner.lock().unwrap_or_else(|e| e.into_inner());
            if !matches!(state.active, Some(ref active) if active.generation == generation) {
                return;
            }
            state.active = None;
            state.state_version = state.state_version.wrapping_add(1);
            state.status = match result {
                Ok(()) => GemmaInstallStatus::Installed,
                Err(error) if error.cancelled => GemmaInstallStatus::Cancelled,
                Err(error) => GemmaInstallStatus::Failed {
                    category: error.category,
                    message: error.message,
                },
            };
        });
        GemmaInstallStatus::Installing
    }

    fn cancel(&self) -> GemmaInstallStatus {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(active) = &inner.active {
            active.cancellation.cancel();
        }
        inner.status.clone()
    }
}

fn lifecycle(root: &Path) -> GemmaRevisionLifecycle {
    GemmaRevisionLifecycle::new(
        root.to_owned(),
        &RESIDENT_GEMMA_REVISIONS,
        &RESIDENT_GEMMA_REVISION,
    )
    .expect("the compiled resident Gemma descriptor must be valid")
}

fn inspect(root: &Path) -> GemmaInstallStatus {
    inspect_lifecycle(&lifecycle(root))
}

fn inspect_lifecycle(lifecycle: &GemmaRevisionLifecycle) -> GemmaInstallStatus {
    match lifecycle.recover(&NativeGemmaLifecycleBoundary) {
        Ok(GemmaRecovery::Current(_) | GemmaRecovery::RestoredPrevious(_)) => {
            GemmaInstallStatus::Installed
        }
        Ok(GemmaRecovery::NotInstalled) => GemmaInstallStatus::NotInstalled,
        Ok(GemmaRecovery::RepairRequired) => GemmaInstallStatus::Failed {
            category: "invalidInstall",
            message: "The installed model could not be verified.",
        },
        Err(_) => GemmaInstallStatus::Failed {
            category: "inspectionFailed",
            message: "The model installation could not be inspected.",
        },
    }
}

fn run_native_install(
    root: &Path,
    cancellation: &NativeInstallCancellation,
) -> Result<(), InstallFailure> {
    let staging = root.join("staging");
    let mut transport = NativeModelAcquisitionTransport::new();
    let clock = NativeAcquisitionClock::new();
    let mut retry = NativeRetryWait;
    let mut lock = NativeInstallLock::new(root.join("install.lock"));
    let mut space = NativeAvailableSpace::new(root);
    install_gemma_revision(
        &staging,
        RESIDENT_GEMMA_REVISION.identity,
        &RESIDENT_GEMMA_REVISION,
        GemmaAcquisitionLimits::default(),
        &mut transport,
        GemmaAcquisitionRuntime {
            clock: &clock,
            retry_wait: &mut retry,
        },
        cancellation,
        &mut lock,
        &mut space,
        &lifecycle(root),
        &NativeGemmaLifecycleBoundary,
    )
    .map(|_| ())
    .map_err(redact_failure)
}

fn redact_failure(error: muniment_core::llama::install::GemmaInstallError) -> InstallFailure {
    let cancelled = matches!(
        error,
        ModelInstallError::Cancelled
            | ModelInstallError::Acquisition(GemmaAcquisitionError::Cancelled)
    );
    if cancelled {
        InstallFailure {
            category: "cancelled",
            message: "Model installation was cancelled.",
            cancelled: true,
        }
    } else {
        let (category, message) = match error {
            ModelInstallError::StorageInsufficient { .. } => (
                "insufficientStorage",
                "There is not enough storage to install the model.",
            ),
            ModelInstallError::StorageUnknown | ModelInstallError::StorageRequirementOverflow => (
                "storageUnavailable",
                "Available storage could not be determined.",
            ),
            ModelInstallError::Acquisition(_) => ("downloadFailed", "The model download failed."),
            ModelInstallError::Publication(_) => (
                "publicationFailed",
                "The downloaded model could not be installed.",
            ),
            ModelInstallError::Lock(_) => {
                ("installUnavailable", "Model installation is unavailable.")
            }
            ModelInstallError::Cancelled => unreachable!(),
        };
        InstallFailure {
            category,
            message,
            cancelled: false,
        }
    }
}

#[tauri::command]
pub fn gemma_install_start(state: State<'_, GemmaInstallState>) -> GemmaInstallStatus {
    state.start()
}

#[tauri::command]
pub async fn gemma_install_status(state: State<'_, GemmaInstallState>) -> GemmaInstallStatus {
    state.status().await
}

#[tauri::command]
pub fn gemma_install_cancel(state: State<'_, GemmaInstallState>) -> GemmaInstallStatus {
    state.cancel()
}

#[cfg(test)]
mod tests {
    use super::*;
    use muniment_core::llama::lifecycle::{GemmaNoticeDescriptor, GemmaRevisionDescriptor};
    use muniment_core::llama::ResidentModelDescriptor;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    fn state(status: GemmaInstallStatus, runner: Arc<Runner>) -> GemmaInstallState {
        GemmaInstallState::with_runner(std::env::temp_dir(), status, runner)
    }

    fn await_status(state: &GemmaInstallState, expected: GemmaInstallStatus) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if state.cached_status() == expected {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(state.cached_status(), expected);
    }

    static TEST_MODEL: ResidentModelDescriptor = ResidentModelDescriptor {
        filename: "model.gguf",
        byte_size: 3,
        sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        alias: "fixture",
        context_tokens: 1,
    };
    static TEST_REVISION: GemmaRevisionDescriptor = GemmaRevisionDescriptor {
        identity: "gemma-fixture-v1",
        revision: "test-revision",
        model: &TEST_MODEL,
        notice: GemmaNoticeDescriptor {
            filename: "NOTICE.txt",
            contents: b"notice",
        },
    };
    static TEST_REVISIONS: [&GemmaRevisionDescriptor; 1] = [&TEST_REVISION];

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let root = std::env::temp_dir()
                .join(format!("muniment-model-install-{}", uuid::Uuid::now_v7()));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }

        fn lifecycle(&self) -> GemmaRevisionLifecycle {
            GemmaRevisionLifecycle::new(self.0.clone(), &TEST_REVISIONS, &TEST_REVISION).unwrap()
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn filesystem_inspection_reports_not_installed() {
        let root = TestRoot::new();
        assert_eq!(
            inspect_lifecycle(&root.lifecycle()),
            GemmaInstallStatus::NotInstalled
        );
    }

    #[test]
    fn filesystem_inspection_verifies_installed_revision() {
        let root = TestRoot::new();
        let revision = root.0.join("revisions").join(TEST_REVISION.revision);
        fs::create_dir_all(&revision).unwrap();
        fs::write(revision.join(TEST_MODEL.filename), b"abc").unwrap();
        fs::write(
            revision.join(TEST_REVISION.notice.filename),
            TEST_REVISION.notice.contents,
        )
        .unwrap();
        fs::write(
            root.0.join("current"),
            "muniment-gemma-pointer-v1\ngemma-fixture-v1\ntest-revision\n",
        )
        .unwrap();
        assert_eq!(
            inspect_lifecycle(&root.lifecycle()),
            GemmaInstallStatus::Installed
        );

        fs::remove_file(revision.join(TEST_MODEL.filename)).unwrap();
        assert_eq!(
            inspect_lifecycle(&root.lifecycle()),
            GemmaInstallStatus::Failed {
                category: "invalidInstall",
                message: "The installed model could not be verified.",
            }
        );
    }

    #[test]
    fn start_is_single_flight_and_completes_successfully() {
        let calls = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(std::sync::Barrier::new(2));
        let runner = {
            let calls = calls.clone();
            let release = release.clone();
            Arc::new(move |_: &Path, _: &NativeInstallCancellation| {
                calls.fetch_add(1, Ordering::SeqCst);
                release.wait();
                Ok(())
            })
        };
        let state = state(GemmaInstallStatus::NotInstalled, runner);
        assert_eq!(state.start(), GemmaInstallStatus::Installing);
        assert_eq!(state.start(), GemmaInstallStatus::Installing);
        release.wait();
        await_status(&state, GemmaInstallStatus::Installed);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn failure_is_redacted() {
        let runner = Arc::new(|_: &Path, _: &NativeInstallCancellation| {
            Err(InstallFailure {
                category: "downloadFailed",
                message: "The model download failed.",
                cancelled: false,
            })
        });
        let state = state(GemmaInstallStatus::NotInstalled, runner);
        state.start();
        await_status(
            &state,
            GemmaInstallStatus::Failed {
                category: "downloadFailed",
                message: "The model download failed.",
            },
        );
    }

    #[test]
    fn cancel_signals_the_active_operation() {
        let runner = Arc::new(|_: &Path, cancellation: &NativeInstallCancellation| {
            while !cancellation.is_cancelled() {
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(InstallFailure {
                category: "cancelled",
                message: "Model installation was cancelled.",
                cancelled: true,
            })
        });
        let state = state(GemmaInstallStatus::NotInstalled, runner);
        state.start();
        assert_eq!(state.cancel(), GemmaInstallStatus::Installing);
        await_status(&state, GemmaInstallStatus::Cancelled);
    }
}
