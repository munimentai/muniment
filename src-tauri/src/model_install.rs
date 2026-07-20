use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use muniment_core::asr::acquisition::{
    AsrAcquisitionError, AsrAcquisitionLimits, AsrAcquisitionRuntime,
};
use muniment_core::asr::install::install_parakeet_revision;
use muniment_core::asr::{
    AsrRecovery, AsrRevisionLifecycle, PARAKEET_MODEL_MANIFEST, PARAKEET_MODEL_MANIFESTS,
};
use muniment_core::llama::acquisition::ModelDownloadProgress;
use muniment_core::llama::acquisition::{
    GemmaAcquisitionError, GemmaAcquisitionLimits, GemmaAcquisitionRuntime,
};
use muniment_core::llama::install::install_gemma_revision_with_progress;
use muniment_core::llama::lifecycle::{
    GemmaActivation, GemmaActivationBoundary, GemmaActivationFailure, GemmaRecovery,
    GemmaRevisionLifecycle, RESIDENT_GEMMA_REVISION, RESIDENT_GEMMA_REVISIONS,
};
use muniment_core::llama::runtime::{acquire_runtime, resolve_runtime};
use muniment_core::llama::{LlamaServer, LlamaServerConfig, ResidentModelDescriptor};
use muniment_core::model_acquisition_transport::NativeModelAcquisitionTransport;
use muniment_core::model_install::ModelInstallError;
use muniment_core::model_install_native::{
    NativeAcquisitionClock, NativeAsrLifecycleBoundary, NativeAvailableSpace,
    NativeGemmaLifecycleBoundary, NativeInstallCancellation, NativeInstallLock, NativeRetryWait,
};
use muniment_core::sidecar::SidecarStatus;
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequiredModelAcquisitionStatus {
    pub status: GemmaInstallStatus,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    /// Directory selection and consent are intentionally never gated on AI.
    pub folder_setup_available: bool,
    pub ai_features_available: bool,
    pub retrying_in_background: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum ParakeetInstallStatus {
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

type ProgressSink = dyn Fn(ModelDownloadProgress) + Send + Sync;
type Runner = dyn Fn(&Path, &NativeInstallCancellation, &ProgressSink) -> Result<PathBuf, InstallFailure>
    + Send
    + Sync;
enum ServingServer {
    Native(LlamaServer),
    Test,
}

impl ServingServer {
    fn is_ready(&self) -> bool {
        match self {
            Self::Native(server) => server.supervisor().status() == SidecarStatus::Healthy,
            Self::Test => true,
        }
    }
}
type Activator = dyn Fn(&Path) -> Result<ServingServer, InstallFailure> + Send + Sync;
type Inspector = dyn Fn(&Path) -> GemmaInstallStatus + Send + Sync;

struct ActiveInstall {
    generation: u64,
    cancellation: NativeInstallCancellation,
}

struct Inner {
    generation: u64,
    state_version: u64,
    status: GemmaInstallStatus,
    terminal_result: bool,
    completed_success: bool,
    active: Option<ActiveInstall>,
    progress: ModelDownloadProgress,
    server: Option<ServingServer>,
}

pub struct GemmaInstallState {
    root: PathBuf,
    inner: Arc<Mutex<Inner>>,
    runner: Arc<Runner>,
    background_retry: bool,
    activator: Arc<Activator>,
    inspector: Arc<Inspector>,
}

impl GemmaInstallState {
    pub fn new(root: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(root.join("staging"))?;
        let status = inspect(&root);
        let mut state = Self::with_runner(root, status, Arc::new(run_native_install));
        state.activator =
            Arc::new(|revision| activate_native_server(revision).map(ServingServer::Native));
        Ok(state.with_background_retry())
    }

    fn with_runner(root: PathBuf, status: GemmaInstallStatus, runner: Arc<Runner>) -> Self {
        Self {
            root,
            inner: Arc::new(Mutex::new(Inner {
                generation: 0,
                state_version: 0,
                status,
                terminal_result: false,
                completed_success: false,
                active: None,
                progress: ModelDownloadProgress {
                    downloaded_bytes: 0,
                    total_bytes: RESIDENT_GEMMA_REVISION.model.byte_size,
                },
                server: None,
            })),
            runner,
            background_retry: false,
            activator: Arc::new(|_| Ok(ServingServer::Test)),
            inspector: Arc::new(inspect),
        }
    }

    fn with_background_retry(mut self) -> Self {
        self.background_retry = true;
        self
    }

    async fn status(&self) -> GemmaInstallStatus {
        let state_version = {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
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
        let inspector = Arc::clone(&self.inspector);
        let inspected = tauri::async_runtime::spawn_blocking(move || inspector(&root))
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

    pub(crate) fn start(&self) -> GemmaInstallStatus {
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
            inner.terminal_result = false;
            inner.completed_success = false;
            inner.status = GemmaInstallStatus::Installing;
            (generation, cancellation)
        };

        let root = self.root.clone();
        let inner = Arc::clone(&self.inner);
        let runner = Arc::clone(&self.runner);
        let activator = Arc::clone(&self.activator);
        let background_retry = self.background_retry;
        tauri::async_runtime::spawn_blocking(move || {
            let progress_inner = Arc::clone(&inner);
            let progress = move |update: ModelDownloadProgress| {
                let mut state = progress_inner.lock().unwrap_or_else(|e| e.into_inner());
                if matches!(state.active, Some(ref active) if active.generation == generation) {
                    state.progress.downloaded_bytes = state
                        .progress
                        .downloaded_bytes
                        .max(update.downloaded_bytes.min(update.total_bytes));
                    state.progress.total_bytes = update.total_bytes;
                }
            };
            let mut result =
                runner(&root, &cancellation, &progress).and_then(|revision| activator(&revision));
            let mut retry_delay = std::time::Duration::from_secs(5);
            while result.is_err() && background_retry && !cancellation.is_cancelled() {
                {
                    let mut state = inner.lock().unwrap_or_else(|e| e.into_inner());
                    if !matches!(state.active, Some(ref active) if active.generation == generation)
                    {
                        return;
                    }
                    let error = match &result {
                        Err(error) => error,
                        Ok(_) => unreachable!("the retry loop only runs after an error"),
                    };
                    state.status = GemmaInstallStatus::Failed {
                        category: error.category,
                        message: error.message,
                    };
                    state.state_version = state.state_version.wrapping_add(1);
                }
                for _ in 0..(retry_delay.as_millis() / 100) {
                    if cancellation.is_cancelled() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                if cancellation.is_cancelled() {
                    break;
                }
                {
                    let mut state = inner.lock().unwrap_or_else(|e| e.into_inner());
                    state.status = GemmaInstallStatus::Installing;
                    state.state_version = state.state_version.wrapping_add(1);
                }
                result = runner(&root, &cancellation, &progress)
                    .and_then(|revision| activator(&revision));
                retry_delay = retry_delay
                    .saturating_mul(2)
                    .min(std::time::Duration::from_secs(60));
            }
            if cancellation.is_cancelled() && result.is_err() {
                result = Err(InstallFailure {
                    category: "cancelled",
                    message: "Model installation was cancelled.",
                    cancelled: true,
                });
            }
            let mut state = inner.lock().unwrap_or_else(|e| e.into_inner());
            if !matches!(state.active, Some(ref active) if active.generation == generation) {
                return;
            }
            state.active = None;
            state.state_version = state.state_version.wrapping_add(1);
            let completed_success = result.is_ok();
            let (status, terminal_result) = match result {
                Ok(server) => {
                    state.progress.downloaded_bytes = state.progress.total_bytes;
                    state.server = Some(server);
                    (GemmaInstallStatus::Installed, false)
                }
                Err(error) if error.cancelled => (GemmaInstallStatus::Cancelled, true),
                Err(error) => (
                    GemmaInstallStatus::Failed {
                        category: error.category,
                        message: error.message,
                    },
                    true,
                ),
            };
            state.status = status;
            state.terminal_result = terminal_result;
            state.completed_success = completed_success;
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

    async fn acquisition_status(&self) -> RequiredModelAcquisitionStatus {
        let status = self.status().await;
        let total_bytes = RESIDENT_GEMMA_REVISION.model.byte_size;
        let (progress, serving) = {
            let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            (
                inner.progress,
                inner.server.as_ref().is_some_and(ServingServer::is_ready),
            )
        };
        let ai_features_available = status == GemmaInstallStatus::Installed && serving;
        RequiredModelAcquisitionStatus {
            downloaded_bytes: progress.downloaded_bytes.min(total_bytes),
            total_bytes,
            folder_setup_available: true,
            ai_features_available,
            retrying_in_background: self.background_retry
                && matches!(
                    status,
                    GemmaInstallStatus::Installing | GemmaInstallStatus::Failed { .. }
                ),
            status,
        }
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
    progress: &ProgressSink,
) -> Result<PathBuf, InstallFailure> {
    if let Ok(revision) = lifecycle(root).resolve_current() {
        progress(ModelDownloadProgress {
            downloaded_bytes: RESIDENT_GEMMA_REVISION.model.byte_size,
            total_bytes: RESIDENT_GEMMA_REVISION.model.byte_size,
        });
        return Ok(revision);
    }
    let staging = root.join("staging");
    let mut transport = NativeModelAcquisitionTransport::new();
    let clock = NativeAcquisitionClock::new();
    let mut retry = NativeRetryWait;
    let mut lock = NativeInstallLock::new(root.join("install.lock"));
    let mut space = NativeAvailableSpace::new(root);
    install_gemma_revision_with_progress(
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
        &mut |update| progress(update),
    )
    .map_err(redact_failure)
}

type RuntimeResolver = dyn Fn() -> Result<PathBuf, GemmaActivationFailure> + Send + Sync;

struct NativeActivation {
    resolve_runtime: Arc<RuntimeResolver>,
    model_descriptor: &'static ResidentModelDescriptor,
    port: u16,
    tolerate_startup_transport_errors: bool,
    health_interval: Option<std::time::Duration>,
}

impl NativeActivation {
    fn production(models_root: &Path) -> Self {
        let runtime_root = models_root.join("llama-server");
        Self {
            resolve_runtime: Arc::new(move || {
                let mut transport = NativeModelAcquisitionTransport::new();
                acquire_runtime(&runtime_root, &mut transport)
                    .map_err(|_| GemmaActivationFailure::Start)?;
                // Resolve again at the spawn boundary so post-install modification of
                // either retained evidence or extracted files fails closed.
                resolve_runtime(&runtime_root).map_err(|_| GemmaActivationFailure::Start)
            }),
            model_descriptor: &RESIDENT_GEMMA_REVISION.model,
            port: 32_391,
            tolerate_startup_transport_errors: false,
            health_interval: None,
        }
    }
}

impl GemmaActivationBoundary for NativeActivation {
    type Server = LlamaServer;

    fn launch(&self, model: &Path) -> Result<Self::Server, GemmaActivationFailure> {
        let executable = (self.resolve_runtime)()?;
        let mut config = LlamaServerConfig::new(executable, model, self.port)
            .with_model_descriptor(self.model_descriptor);
        if self.tolerate_startup_transport_errors {
            config = config.with_startup_transport_tolerance();
        }
        if let Some(interval) = self.health_interval {
            config = config.with_health_interval(interval);
        }
        LlamaServer::spawn(config).map_err(|_| GemmaActivationFailure::Start)
    }

    fn await_ready(&self, server: &mut Self::Server) -> Result<(), GemmaActivationFailure> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while std::time::Instant::now() < deadline {
            match server.supervisor().status() {
                SidecarStatus::Healthy => return Ok(()),
                SidecarStatus::Stopped | SidecarStatus::Failed => {
                    return Err(GemmaActivationFailure::ExitedBeforeReady)
                }
                _ => std::thread::sleep(std::time::Duration::from_millis(100)),
            }
        }
        Err(GemmaActivationFailure::Readiness)
    }
}

fn activate_native_server(revision: &Path) -> Result<LlamaServer, InstallFailure> {
    let models_root = revision
        .parent()
        .and_then(Path::parent)
        .ok_or(InstallFailure {
            category: "activationFailed",
            message: "The local AI service could not be started.",
            cancelled: false,
        })?;
    let lifecycle = lifecycle(models_root);
    let activation = NativeActivation::production(models_root);
    activate_with(&lifecycle, &activation)
}

fn activate_with(
    lifecycle: &GemmaRevisionLifecycle,
    activation: &NativeActivation,
) -> Result<LlamaServer, InstallFailure> {
    match lifecycle.activate(&NativeGemmaLifecycleBoundary, activation) {
        Ok(GemmaActivation::Active { server, .. } | GemmaActivation::RolledBack { server, .. }) => {
            Ok(server)
        }
        _ => Err(InstallFailure {
            category: "activationFailed",
            message: "The local AI service could not be started.",
            cancelled: false,
        }),
    }
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

type ParakeetRunner =
    dyn Fn(&Path, &NativeInstallCancellation) -> Result<(), InstallFailure> + Send + Sync;

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
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
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
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.active.is_none() && inner.state_version == state_version {
            inner.state_version = inner.state_version.wrapping_add(1);
            inner.status = inspected;
        }
        inner.status.clone()
    }

    fn start(&self) -> ParakeetInstallStatus {
        let (generation, cancellation) = {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
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
            inner.status = ParakeetInstallStatus::Installing;
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
        ParakeetInstallStatus::Installing
    }

    fn cancel(&self) -> ParakeetInstallStatus {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(active) = &inner.active {
            active.cancellation.cancel();
        }
        inner.status.clone()
    }
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
    inspect_parakeet_lifecycle(&parakeet_lifecycle(root))
}

fn inspect_parakeet_lifecycle(lifecycle: &AsrRevisionLifecycle) -> ParakeetInstallStatus {
    match lifecycle.recover(&NativeAsrLifecycleBoundary) {
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
) -> Result<(), InstallFailure> {
    let staging = root.join("staging");
    let mut transport = NativeModelAcquisitionTransport::new();
    let clock = NativeAcquisitionClock::new();
    let mut retry = NativeRetryWait;
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
pub fn gemma_install_start(state: State<'_, GemmaInstallState>) -> GemmaInstallStatus {
    state.start()
}

#[tauri::command]
pub async fn gemma_install_status(
    state: State<'_, GemmaInstallState>,
) -> Result<GemmaInstallStatus, String> {
    gemma_install_status_handler(&state).await
}

async fn gemma_install_status_handler(
    state: &GemmaInstallState,
) -> Result<GemmaInstallStatus, String> {
    Ok(state.status().await)
}

#[tauri::command]
pub fn gemma_install_cancel(state: State<'_, GemmaInstallState>) -> GemmaInstallStatus {
    state.cancel()
}

#[tauri::command]
pub async fn required_model_acquisition_status(
    state: State<'_, GemmaInstallState>,
) -> Result<RequiredModelAcquisitionStatus, String> {
    Ok(state.acquisition_status().await)
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
    use muniment_core::asr::{AsrArtifactDescriptor, AsrArtifactManifest};
    #[cfg(unix)]
    use muniment_core::llama::acquisition::{
        GemmaDownloadRequest, GemmaDownloadResponse, GemmaDownloadTransport, GemmaTransportError,
    };
    use muniment_core::llama::lifecycle::{GemmaNoticeDescriptor, GemmaRevisionDescriptor};
    #[cfg(unix)]
    use muniment_core::llama::runtime::{
        acquire_runtime_for, resolve_runtime_for, LlamaRuntimeDescriptor, RuntimeArchiveError,
        RuntimeDownloadRequest, RuntimeDownloadResponse, RuntimeDownloadTransport,
    };
    use muniment_core::llama::ResidentModelDescriptor;
    #[cfg(unix)]
    use sha2::{Digest, Sha256};
    #[cfg(unix)]
    use std::io::Cursor;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};
    use tauri::Manager;

    fn state(status: GemmaInstallStatus, runner: Arc<Runner>) -> GemmaInstallState {
        GemmaInstallState::with_runner(std::env::temp_dir(), status, runner)
    }

    fn app_with_state(state: GemmaInstallState) -> tauri::App<tauri::test::MockRuntime> {
        let app = tauri::test::mock_app();
        assert!(app.manage(state));
        app
    }

    fn app_with_parakeet_state(
        state: ParakeetInstallState,
    ) -> tauri::App<tauri::test::MockRuntime> {
        let app = tauri::test::mock_app();
        assert!(app.manage(state));
        app
    }

    fn public_status(app: &tauri::App<tauri::test::MockRuntime>) -> GemmaInstallStatus {
        tauri::async_runtime::block_on(gemma_install_status(app.state())).unwrap()
    }

    fn await_public_status(
        app: &tauri::App<tauri::test::MockRuntime>,
        expected: GemmaInstallStatus,
    ) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if public_status(app) == expected {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(public_status(app), expected);
    }

    fn parakeet_state(
        status: ParakeetInstallStatus,
        runner: Arc<ParakeetRunner>,
    ) -> ParakeetInstallState {
        ParakeetInstallState::with_runner(std::env::temp_dir(), status, runner)
    }

    fn parakeet_public_status(app: &tauri::App<tauri::test::MockRuntime>) -> ParakeetInstallStatus {
        tauri::async_runtime::block_on(parakeet_install_status(app.state())).unwrap()
    }

    fn await_parakeet_status(
        app: &tauri::App<tauri::test::MockRuntime>,
        expected: ParakeetInstallStatus,
    ) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if parakeet_public_status(app) == expected {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(parakeet_public_status(app), expected);
    }

    static TEST_MODEL: ResidentModelDescriptor = ResidentModelDescriptor {
        source_url: "https://example.invalid/model.gguf",
        license: "fixture",
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
    static TEST_ASR_ARTIFACTS: [AsrArtifactDescriptor; 4] = [
        AsrArtifactDescriptor {
            filename: "a",
            byte_size: 0,
            sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        },
        AsrArtifactDescriptor {
            filename: "b",
            byte_size: 0,
            sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        },
        AsrArtifactDescriptor {
            filename: "c",
            byte_size: 0,
            sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        },
        AsrArtifactDescriptor {
            filename: "d",
            byte_size: 0,
            sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        },
    ];
    static TEST_ASR_MANIFEST: AsrArtifactManifest = AsrArtifactManifest {
        identity: "parakeet-fixture-v1",
        revision: "test-revision",
        artifacts: &TEST_ASR_ARTIFACTS,
        additional_artifact: None,
    };
    static TEST_ASR_MANIFESTS: [&AsrArtifactManifest; 1] = [&TEST_ASR_MANIFEST];

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
    fn real_background_failure_is_fail_open_for_onboarding_and_closed_for_ai() {
        let root = TestRoot::new();
        let attempts = Arc::new(AtomicUsize::new(0));
        let runner_attempts = Arc::clone(&attempts);
        let state = GemmaInstallState::with_runner(
            root.0.clone(),
            GemmaInstallStatus::NotInstalled,
            Arc::new(move |_, _, _| {
                runner_attempts.fetch_add(1, Ordering::SeqCst);
                Err(InstallFailure {
                    category: "downloadFailed",
                    message: "The model download failed.",
                    cancelled: false,
                })
            }),
        )
        .with_background_retry();
        state.start();

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if matches!(
                tauri::async_runtime::block_on(state.status()),
                GemmaInstallStatus::Failed { .. }
            ) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        let snapshot = tauri::async_runtime::block_on(state.acquisition_status());
        assert!(matches!(snapshot.status, GemmaInstallStatus::Failed { .. }));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert!(snapshot.folder_setup_available);
        assert!(!snapshot.ai_features_available);
        assert!(snapshot.retrying_in_background);
        assert_eq!(snapshot.downloaded_bytes, 0);
        assert_eq!(
            snapshot.total_bytes,
            RESIDENT_GEMMA_REVISION.model.byte_size
        );
        state.cancel();
    }

    #[test]
    fn startup_download_is_independent_of_onboarding_progress() {
        let entered = Arc::new(std::sync::Barrier::new(2));
        let release = Arc::new(std::sync::Barrier::new(2));
        let runner = {
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            Arc::new(
                move |root: &Path, _: &NativeInstallCancellation, _: &ProgressSink| {
                    entered.wait();
                    release.wait();
                    Ok(root.to_owned())
                },
            )
        };
        let state = state(GemmaInstallStatus::NotInstalled, runner);

        assert_eq!(state.start(), GemmaInstallStatus::Installing);
        entered.wait();
        let snapshot = tauri::async_runtime::block_on(state.acquisition_status());
        assert_eq!(snapshot.status, GemmaInstallStatus::Installing);
        assert!(snapshot.folder_setup_available);
        assert!(!snapshot.ai_features_available);
        release.wait();
    }

    #[test]
    fn active_job_progress_is_monotonic_through_publication_and_serving() {
        let published = Arc::new(std::sync::Barrier::new(2));
        let release = Arc::new(std::sync::Barrier::new(2));
        let runner = {
            let published = Arc::clone(&published);
            let release = Arc::clone(&release);
            Arc::new(
                move |root: &Path, _: &NativeInstallCancellation, progress: &ProgressSink| {
                    progress(ModelDownloadProgress {
                        downloaded_bytes: 7,
                        total_bytes: 10,
                    });
                    progress(ModelDownloadProgress {
                        downloaded_bytes: 3,
                        total_bytes: 10,
                    });
                    progress(ModelDownloadProgress {
                        downloaded_bytes: 10,
                        total_bytes: 10,
                    });
                    published.wait();
                    release.wait();
                    Ok(root.to_owned())
                },
            )
        };
        let mut state = state(GemmaInstallStatus::NotInstalled, runner);
        state.inner.lock().unwrap().progress.total_bytes = 10;
        state.inspector = Arc::new(|_| GemmaInstallStatus::Installed);
        state.start();
        published.wait();

        let during_publication = tauri::async_runtime::block_on(state.acquisition_status());
        assert_eq!(during_publication.downloaded_bytes, 10);
        assert!(!during_publication.ai_features_available);
        release.wait();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline
            && !tauri::async_runtime::block_on(state.acquisition_status()).ai_features_available
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(tauri::async_runtime::block_on(state.acquisition_status()).ai_features_available);
    }

    #[cfg(unix)]
    #[test]
    fn production_composition_acquires_model_and_runtime_and_tracks_live_readiness() {
        struct ModelTransport;
        impl GemmaDownloadTransport for ModelTransport {
            type Body = Cursor<Vec<u8>>;

            fn download(
                &mut self,
                request: &GemmaDownloadRequest,
            ) -> Result<GemmaDownloadResponse<Self::Body>, GemmaTransportError> {
                assert_eq!(request.offset, 1, "the staged model download must resume");
                Ok(GemmaDownloadResponse {
                    status: 206,
                    content_range: Some((1, 2, 3)),
                    body: Cursor::new(b"bc".to_vec()),
                })
            }
        }

        struct RuntimeTransport(Vec<u8>);
        impl RuntimeDownloadTransport for RuntimeTransport {
            type Body = Cursor<Vec<u8>>;

            fn download(
                &mut self,
                request: &RuntimeDownloadRequest,
            ) -> Result<RuntimeDownloadResponse<Self::Body>, RuntimeArchiveError> {
                assert_eq!(request.offset, 0);
                Ok(RuntimeDownloadResponse {
                    status: 200,
                    content_range: None,
                    body: Cursor::new(self.0.clone()),
                })
            }
        }

        let root = TestRoot::new();
        let stage = root.0.join("staging").join(TEST_REVISION.identity);
        fs::create_dir_all(&stage).unwrap();
        fs::write(stage.join(format!("{}.part", TEST_MODEL.filename)), b"a").unwrap();

        let runner_root = root.0.clone();
        let runner = Arc::new(
            move |_: &Path, cancellation: &NativeInstallCancellation, progress: &ProgressSink| {
                let mut transport = ModelTransport;
                let mut retry = NativeRetryWait;
                let clock = NativeAcquisitionClock::new();
                let mut lock = NativeInstallLock::new(runner_root.join("install.lock"));
                let mut space = NativeAvailableSpace::new(&runner_root);
                install_gemma_revision_with_progress(
                    &runner_root.join("staging"),
                    TEST_REVISION.identity,
                    &TEST_REVISION,
                    GemmaAcquisitionLimits::default(),
                    &mut transport,
                    GemmaAcquisitionRuntime {
                        clock: &clock,
                        retry_wait: &mut retry,
                    },
                    cancellation,
                    &mut lock,
                    &mut space,
                    &GemmaRevisionLifecycle::new(
                        runner_root.clone(),
                        &TEST_REVISIONS,
                        &TEST_REVISION,
                    )
                    .unwrap(),
                    &NativeGemmaLifecycleBoundary,
                    &mut |update| progress(update),
                )
                .map_err(redact_failure)
            },
        );

        let runtime_root = root.0.join("llama-server");
        let archive_path = root.0.join("runtime.tar.gz");
        let encoder = flate2::write::GzEncoder::new(
            fs::File::create(&archive_path).unwrap(),
            flate2::Compression::default(),
        );
        let mut archive = tar::Builder::new(encoder);
        let script = br#"#!/usr/bin/env python3
import http.server, os, pathlib, sys, threading
args = sys.argv[1:]
port = args[args.index('--port') + 1]
pathlib.Path('/tmp/muniment-llama-args-' + port).write_text('\n'.join(args))
class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        body = b'{"status":"ok"}'
        self.send_response(200); self.send_header('Content-Length', str(len(body)))
        self.end_headers(); self.wfile.write(body)
    def log_message(self, *_): pass
server = http.server.HTTPServer(('127.0.0.1', int(port)), Handler)
threading.Timer(1, lambda: os._exit(23)).start()
server.serve_forever()
"#;
        let mut header = tar::Header::new_gnu();
        header.set_size(script.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        archive
            .append_data(&mut header, "llama-fixture/llama-server", &script[..])
            .unwrap();
        archive.into_inner().unwrap().finish().unwrap();
        let runtime_bytes = fs::read(&archive_path).unwrap();
        let runtime_hash =
            Box::leak(format!("{:x}", Sha256::digest(&runtime_bytes)).into_boxed_str());
        let runtime_descriptor: &'static LlamaRuntimeDescriptor =
            Box::leak(Box::new(LlamaRuntimeDescriptor {
                revision: "fixture",
                archive: "runtime.tar.gz",
                byte_size: runtime_bytes.len() as u64,
                sha256: runtime_hash,
                top_level: "llama-fixture",
                executable: "llama-fixture/llama-server",
            }));
        let port = std::net::TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let activation = NativeActivation {
            resolve_runtime: Arc::new(move || {
                acquire_runtime_for(
                    &runtime_root,
                    "https://example.invalid/runtime",
                    runtime_descriptor,
                    &mut RuntimeTransport(runtime_bytes.clone()),
                )
                .map_err(|_| GemmaActivationFailure::Start)
            }),
            model_descriptor: &TEST_MODEL,
            port,
            tolerate_startup_transport_errors: true,
            health_interval: Some(Duration::from_millis(10)),
        };
        let activation = Arc::new(activation);
        let activation_lifecycle = root.lifecycle();
        let mut state = GemmaInstallState::with_runner(
            root.0.clone(),
            GemmaInstallStatus::NotInstalled,
            runner,
        );
        state.activator = Arc::new(move |_| {
            activate_with(&activation_lifecycle, &activation).map(ServingServer::Native)
        });
        state.inspector = Arc::new(|root| {
            inspect_lifecycle(
                &GemmaRevisionLifecycle::new(root.to_owned(), &TEST_REVISIONS, &TEST_REVISION)
                    .unwrap(),
            )
        });
        state.start();

        let ready_deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < ready_deadline
            && !tauri::async_runtime::block_on(state.acquisition_status()).ai_features_available
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        let ready_status = tauri::async_runtime::block_on(state.acquisition_status());
        assert!(
            ready_status.ai_features_available,
            "native activation did not become available: {ready_status:?}"
        );
        let executable = resolve_runtime_for(&root.0.join("llama-server"), runtime_descriptor)
            .expect("the owned runtime must resolve after verified publication");
        assert!(executable.ends_with("llama-fixture/llama-server"));
        let arguments_path = PathBuf::from(format!("/tmp/muniment-llama-args-{port}"));
        let arguments_deadline = Instant::now() + Duration::from_secs(2);
        while !arguments_path.exists() && Instant::now() < arguments_deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let arguments = fs::read_to_string(&arguments_path).unwrap();
        assert!(arguments.contains("--model"));
        assert!(arguments.contains(
            root.0
                .join("revisions/test-revision/model.gguf")
                .to_string_lossy()
                .as_ref()
        ));
        assert!(arguments.contains("--alias\nfixture"));

        let exit_deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < exit_deadline
            && tauri::async_runtime::block_on(state.acquisition_status()).ai_features_available
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !tauri::async_runtime::block_on(state.acquisition_status()).ai_features_available,
            "availability must turn false when the supervised native server exits"
        );
        fs::remove_file(arguments_path).unwrap();
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
    fn public_status_command_exposes_successful_completion() {
        let calls = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(std::sync::Barrier::new(2));
        let runner = {
            let calls = calls.clone();
            let release = release.clone();
            Arc::new(
                move |root: &Path, _: &NativeInstallCancellation, _: &ProgressSink| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    release.wait();
                    Ok(root.to_owned())
                },
            )
        };
        let app = app_with_state(state(GemmaInstallStatus::NotInstalled, runner));
        let state = app.state::<GemmaInstallState>();
        assert_eq!(state.start(), GemmaInstallStatus::Installing);
        assert_eq!(state.start(), GemmaInstallStatus::Installing);
        release.wait();
        await_public_status(&app, GemmaInstallStatus::Installed);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn public_status_command_preserves_redacted_failure() {
        let runner = Arc::new(
            |_: &Path, _: &NativeInstallCancellation, _: &ProgressSink| {
                Err(InstallFailure {
                    category: "downloadFailed",
                    message: "The model download failed.",
                    cancelled: false,
                })
            },
        );
        let app = app_with_state(state(GemmaInstallStatus::NotInstalled, runner));
        let state = app.state::<GemmaInstallState>();
        state.start();
        let expected = GemmaInstallStatus::Failed {
            category: "downloadFailed",
            message: "The model download failed.",
        };
        await_public_status(&app, expected.clone());
        assert_eq!(public_status(&app), expected);
    }

    #[test]
    fn public_status_command_preserves_cancelled_completion() {
        let runner = Arc::new(
            |_: &Path, cancellation: &NativeInstallCancellation, _: &ProgressSink| {
                while !cancellation.is_cancelled() {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(InstallFailure {
                    category: "cancelled",
                    message: "Model installation was cancelled.",
                    cancelled: true,
                })
            },
        );
        let app = app_with_state(state(GemmaInstallStatus::NotInstalled, runner));
        let state = app.state::<GemmaInstallState>();
        state.start();
        assert_eq!(state.cancel(), GemmaInstallStatus::Installing);
        await_public_status(&app, GemmaInstallStatus::Cancelled);
        assert_eq!(public_status(&app), GemmaInstallStatus::Cancelled);
    }

    #[test]
    fn parakeet_status_serialization_is_closed_and_redacted() {
        assert_eq!(
            serde_json::to_value(ParakeetInstallStatus::Failed {
                category: "downloadFailed",
                message: "The speech model download failed.",
            })
            .unwrap(),
            serde_json::json!({
                "state": "failed",
                "category": "downloadFailed",
                "message": "The speech model download failed."
            })
        );
        let failure = redact_parakeet_failure(ModelInstallError::Acquisition(
            AsrAcquisitionError::Persistence,
        ));
        assert_eq!(failure.category, "downloadFailed");
        assert_eq!(failure.message, "The speech model download failed.");
    }

    #[test]
    fn parakeet_duplicate_start_launches_one_worker_and_publishes_success() {
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
        let app =
            app_with_parakeet_state(parakeet_state(ParakeetInstallStatus::NotInstalled, runner));
        let state = app.state::<ParakeetInstallState>();
        assert_eq!(state.start(), ParakeetInstallStatus::Installing);
        assert_eq!(state.start(), ParakeetInstallStatus::Installing);
        release.wait();
        await_parakeet_status(&app, ParakeetInstallStatus::Installed);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn parakeet_cancel_is_idempotent_before_and_during_worker_observation() {
        let runner = Arc::new(|_: &Path, cancellation: &NativeInstallCancellation| {
            while !cancellation.is_cancelled() {
                std::thread::yield_now();
            }
            Err(InstallFailure {
                category: "cancelled",
                message: "Speech model installation was cancelled.",
                cancelled: true,
            })
        });
        let app =
            app_with_parakeet_state(parakeet_state(ParakeetInstallStatus::NotInstalled, runner));
        let state = app.state::<ParakeetInstallState>();
        assert_eq!(state.cancel(), ParakeetInstallStatus::NotInstalled);
        state.start();
        assert_eq!(state.cancel(), ParakeetInstallStatus::Installing);
        assert_eq!(state.cancel(), ParakeetInstallStatus::Installing);
        await_parakeet_status(&app, ParakeetInstallStatus::Cancelled);
    }

    #[test]
    fn parakeet_restart_detection_verifies_active_revision() {
        let root = TestRoot::new();
        let lifecycle =
            AsrRevisionLifecycle::new(root.0.clone(), &TEST_ASR_MANIFESTS, &TEST_ASR_MANIFEST)
                .unwrap();
        let revision = root.0.join("revisions/test-revision");
        fs::create_dir_all(&revision).unwrap();
        for artifact in TEST_ASR_ARTIFACTS {
            fs::write(revision.join(artifact.filename), b"").unwrap();
        }
        fs::write(
            root.0.join("current"),
            "muniment-asr-pointer-v1\nparakeet-fixture-v1\ntest-revision\n",
        )
        .unwrap();
        assert_eq!(
            inspect_parakeet_lifecycle(&lifecycle),
            ParakeetInstallStatus::Installed
        );
        fs::remove_file(revision.join("a")).unwrap();
        assert_eq!(
            inspect_parakeet_lifecycle(&lifecycle),
            ParakeetInstallStatus::Failed {
                category: "invalidInstall",
                message: "The installed speech model could not be verified.",
            }
        );
    }
}
