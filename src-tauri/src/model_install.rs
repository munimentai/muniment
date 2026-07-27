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
use muniment_core::import_preview::ExtractedEntry;
use muniment_core::llama::acquisition::ModelDownloadProgress;
use muniment_core::llama::acquisition::{
    ResidentModelAcquisitionError, ResidentModelAcquisitionLimits, ResidentModelAcquisitionRuntime,
};
use muniment_core::llama::install::install_resident_model_revision_with_progress;
use muniment_core::llama::lifecycle::{
    ResidentModelActivation, ResidentModelActivationBoundary, ResidentModelActivationFailure,
    ResidentModelRecovery, ResidentModelRevisionLifecycle, PINNED_RESIDENT_MODEL_REVISION,
    RESIDENT_MODEL_REVISIONS,
};
use muniment_core::llama::runtime::{acquire_runtime, resolve_runtime};
use muniment_core::llama::{
    DictationPolishRequest, DictationTransform, DictationTransformRequest, LlamaChatClient,
    LlamaChatError, LlamaServer, LlamaServerConfig, OnboardingTriageRequest,
    OnboardingTriageRequestError, OnboardingTriageResponse, ResidentModelDescriptor,
};
use muniment_core::model_acquisition_transport::NativeModelAcquisitionTransport;
use muniment_core::model_install::ModelInstallError;
use muniment_core::model_install_native::{
    NativeAcquisitionClock, NativeAsrLifecycleBoundary, NativeAvailableSpace,
    NativeInstallCancellation, NativeInstallLock, NativeResidentModelLifecycleBoundary,
    NativeRetryWait,
};
use muniment_core::sidecar::SidecarStatus;
use serde::Serialize;
use tauri::State;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum ResidentModelInstallStatus {
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
    pub status: ResidentModelInstallStatus,
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
    Test(String),
}

impl ServingServer {
    fn is_ready(&self) -> bool {
        match self {
            Self::Native(server) => server.supervisor().status() == SidecarStatus::Healthy,
            Self::Test(_) => true,
        }
    }

    fn base_url(&self) -> &str {
        match self {
            Self::Native(server) => server.base_url(),
            Self::Test(base_url) => base_url,
        }
    }
}
type Activator = dyn Fn(&Path) -> Result<ServingServer, InstallFailure> + Send + Sync;
type Inspector = dyn Fn(&Path) -> ResidentModelInstallStatus + Send + Sync;

struct ActiveInstall {
    generation: u64,
    cancellation: NativeInstallCancellation,
}

struct Inner {
    generation: u64,
    state_version: u64,
    status: ResidentModelInstallStatus,
    terminal_result: bool,
    completed_success: bool,
    active: Option<ActiveInstall>,
    progress: ModelDownloadProgress,
    server: Option<ServingServer>,
}

pub struct ResidentModelInstallState {
    root: PathBuf,
    inner: Arc<Mutex<Inner>>,
    runner: Arc<Runner>,
    background_retry: bool,
    activator: Arc<Activator>,
    inspector: Arc<Inspector>,
}

impl ResidentModelInstallState {
    pub fn new(root: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(root.join("staging"))?;
        let status = inspect(&root);
        let mut state = Self::with_runner(root, status, Arc::new(run_native_install));
        state.activator =
            Arc::new(|revision| activate_native_server(revision).map(ServingServer::Native));
        Ok(state.with_background_retry())
    }

    fn with_runner(root: PathBuf, status: ResidentModelInstallStatus, runner: Arc<Runner>) -> Self {
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
                    total_bytes: PINNED_RESIDENT_MODEL_REVISION.model.byte_size,
                },
                server: None,
            })),
            runner,
            background_retry: false,
            activator: Arc::new(|_| Ok(ServingServer::Test(String::new()))),
            inspector: Arc::new(inspect),
        }
    }

    fn with_background_retry(mut self) -> Self {
        self.background_retry = true;
        self
    }

    async fn status(&self) -> ResidentModelInstallStatus {
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
            .unwrap_or(ResidentModelInstallStatus::Failed {
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

    pub(crate) fn start(&self) -> ResidentModelInstallStatus {
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
            inner.status = ResidentModelInstallStatus::Installing;
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
                    state.status = ResidentModelInstallStatus::Failed {
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
                    state.status = ResidentModelInstallStatus::Installing;
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
                    (ResidentModelInstallStatus::Installed, false)
                }
                Err(error) if error.cancelled => (ResidentModelInstallStatus::Cancelled, true),
                Err(error) => (
                    ResidentModelInstallStatus::Failed {
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
        ResidentModelInstallStatus::Installing
    }

    fn cancel(&self) -> ResidentModelInstallStatus {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(active) = &inner.active {
            active.cancellation.cancel();
        }
        inner.status.clone()
    }

    pub(crate) async fn acquisition_status(&self) -> RequiredModelAcquisitionStatus {
        let status = self.status().await;
        let total_bytes = PINNED_RESIDENT_MODEL_REVISION.model.byte_size;
        let (progress, serving) = {
            let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            (
                inner.progress,
                inner.server.as_ref().is_some_and(ServingServer::is_ready),
            )
        };
        let ai_features_available = status == ResidentModelInstallStatus::Installed && serving;
        RequiredModelAcquisitionStatus {
            downloaded_bytes: progress.downloaded_bytes.min(total_bytes),
            total_bytes,
            folder_setup_available: true,
            ai_features_available,
            retrying_in_background: self.background_retry
                && matches!(
                    status,
                    ResidentModelInstallStatus::Installing
                        | ResidentModelInstallStatus::Failed { .. }
                ),
            status,
        }
    }

    fn polish_dictation(&self, transcript: String) -> Result<String, DictationPolishFailure> {
        Self::polish_dictation_with_inner(Arc::clone(&self.inner), transcript)
    }

    fn polish_dictation_with_inner(
        inner: Arc<Mutex<Inner>>,
        transcript: String,
    ) -> Result<String, DictationPolishFailure> {
        if transcript.trim().is_empty() {
            return Err(DictationPolishFailure::invalid_transcript());
        }
        let base_url = {
            let guard = inner.lock().unwrap_or_else(|e| e.into_inner());
            let server = guard
                .server
                .as_ref()
                .filter(|server| server.is_ready())
                .ok_or_else(DictationPolishFailure::unavailable)?;
            server.base_url().to_owned()
        };
        let result = LlamaChatClient::new(base_url, std::time::Duration::from_secs(45))
            .and_then(|client| client.polish_dictation(&DictationPolishRequest::new(transcript)));
        match result {
            Ok(response) => Ok(response.polished_text),
            Err(LlamaChatError::Transport(_)) => Err(DictationPolishFailure::unavailable()),
            Err(_) => {
                let guard = inner.lock().unwrap_or_else(|e| e.into_inner());
                if !guard.server.as_ref().is_some_and(ServingServer::is_ready) {
                    Err(DictationPolishFailure::unavailable())
                } else {
                    Err(DictationPolishFailure::request_failed())
                }
            }
        }
    }

    fn transform_dictation_with_inner(
        inner: Arc<Mutex<Inner>>,
        transcript: String,
        transform: DictationTransform,
    ) -> Result<String, DictationTransformFailure> {
        if transcript.trim().is_empty() {
            return Err(DictationTransformFailure::invalid_transcript());
        }
        let base_url = {
            let guard = inner.lock().unwrap_or_else(|e| e.into_inner());
            let server = guard
                .server
                .as_ref()
                .filter(|server| server.is_ready())
                .ok_or_else(DictationTransformFailure::unavailable)?;
            server.base_url().to_owned()
        };
        let result =
            LlamaChatClient::new(base_url, std::time::Duration::from_secs(45)).and_then(|client| {
                client.transform_dictation(&DictationTransformRequest::new(transform, transcript))
            });
        match result {
            Ok(response) => Ok(response.transformed_text),
            Err(LlamaChatError::Transport(_)) => Err(DictationTransformFailure::unavailable()),
            Err(_) => {
                let guard = inner.lock().unwrap_or_else(|e| e.into_inner());
                if !guard.server.as_ref().is_some_and(ServingServer::is_ready) {
                    Err(DictationTransformFailure::unavailable())
                } else {
                    Err(DictationTransformFailure::request_failed())
                }
            }
        }
    }

    fn triage_onboarding_with_inner(
        inner: Arc<Mutex<Inner>>,
        entries: Vec<ExtractedEntry>,
    ) -> Result<OnboardingTriageResponse, OnboardingTriageFailure> {
        let request =
            OnboardingTriageRequest::new(entries).map_err(OnboardingTriageFailure::input)?;
        let base_url = {
            let guard = inner.lock().unwrap_or_else(|e| e.into_inner());
            guard
                .server
                .as_ref()
                .filter(|server| server.is_ready())
                .ok_or_else(OnboardingTriageFailure::unavailable)?
                .base_url()
                .to_owned()
        };
        let result = LlamaChatClient::new(base_url, std::time::Duration::from_secs(45))
            .and_then(|client| client.triage_onboarding(&request));
        match result {
            Ok(response) => Ok(response),
            Err(LlamaChatError::Transport(_)) => Err(OnboardingTriageFailure::transport()),
            Err(
                LlamaChatError::InvalidResponse(_)
                | LlamaChatError::MalformedJson
                | LlamaChatError::BodyTooLarge { .. },
            ) => Err(OnboardingTriageFailure::invalid_response()),
            Err(_) => Err(OnboardingTriageFailure::request_failed()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingTriageFailure {
    kind: &'static str,
    message: &'static str,
}

impl OnboardingTriageFailure {
    fn input(error: OnboardingTriageRequestError) -> Self {
        match error {
            OnboardingTriageRequestError::Empty => Self {
                kind: "empty",
                message: "Select at least one approved entry.",
            },
            OnboardingTriageRequestError::TooManyEntries => Self {
                kind: "tooManyEntries",
                message: "Too many approved entries were selected.",
            },
            OnboardingTriageRequestError::EmptySourceField => Self {
                kind: "malformedSource",
                message: "An approved entry has invalid source information.",
            },
            OnboardingTriageRequestError::TooLarge => Self {
                kind: "inputTooLarge",
                message: "The approved entries are too large to triage.",
            },
        }
    }

    fn unavailable() -> Self {
        Self {
            kind: "localAiUnavailable",
            message: "Local AI is unavailable.",
        }
    }
    fn transport() -> Self {
        Self {
            kind: "transportFailed",
            message: "Local AI could not be reached.",
        }
    }
    fn invalid_response() -> Self {
        Self {
            kind: "invalidModelResponse",
            message: "Local AI returned an invalid triage report.",
        }
    }
    fn request_failed() -> Self {
        Self {
            kind: "requestFailed",
            message: "The onboarding triage request failed.",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationPolishFailure {
    category: &'static str,
    message: &'static str,
}

impl DictationPolishFailure {
    fn invalid_transcript() -> Self {
        Self {
            category: "invalidTranscript",
            message: "The dictation transcript cannot be empty.",
        }
    }

    fn unavailable() -> Self {
        Self {
            category: "localAiUnavailable",
            message: "Local AI is unavailable.",
        }
    }

    fn request_failed() -> Self {
        Self {
            category: "polishRequestFailed",
            message: "The dictation transcript could not be polished.",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationTransformFailure {
    category: &'static str,
    message: &'static str,
}

impl DictationTransformFailure {
    fn invalid_transcript() -> Self {
        Self {
            category: "invalidTranscript",
            message: "The dictation transcript cannot be empty.",
        }
    }

    fn unavailable() -> Self {
        Self {
            category: "localAiUnavailable",
            message: "Local AI is unavailable.",
        }
    }

    fn request_failed() -> Self {
        Self {
            category: "transformRequestFailed",
            message: "The dictation transcript could not be transformed.",
        }
    }
}

fn lifecycle(root: &Path) -> ResidentModelRevisionLifecycle {
    ResidentModelRevisionLifecycle::new(
        root.to_owned(),
        &RESIDENT_MODEL_REVISIONS,
        &PINNED_RESIDENT_MODEL_REVISION,
    )
    .expect("the compiled resident-model descriptor must be valid")
}

fn inspect(root: &Path) -> ResidentModelInstallStatus {
    inspect_lifecycle(&lifecycle(root))
}

fn inspect_lifecycle(lifecycle: &ResidentModelRevisionLifecycle) -> ResidentModelInstallStatus {
    match lifecycle.recover(&NativeResidentModelLifecycleBoundary) {
        Ok(ResidentModelRecovery::Current(_) | ResidentModelRecovery::RestoredPrevious(_)) => {
            ResidentModelInstallStatus::Installed
        }
        Ok(ResidentModelRecovery::NotInstalled) => ResidentModelInstallStatus::NotInstalled,
        Ok(ResidentModelRecovery::RepairRequired) => ResidentModelInstallStatus::Failed {
            category: "invalidInstall",
            message: "The installed model could not be verified.",
        },
        Err(_) => ResidentModelInstallStatus::Failed {
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
            downloaded_bytes: PINNED_RESIDENT_MODEL_REVISION.model.byte_size,
            total_bytes: PINNED_RESIDENT_MODEL_REVISION.model.byte_size,
        });
        return Ok(revision);
    }
    let staging = root.join("staging");
    let mut transport = NativeModelAcquisitionTransport::new();
    let clock = NativeAcquisitionClock::new();
    let mut retry = NativeRetryWait;
    let mut lock = NativeInstallLock::new(root.join("install.lock"));
    let mut space = NativeAvailableSpace::new(root);
    install_resident_model_revision_with_progress(
        &staging,
        PINNED_RESIDENT_MODEL_REVISION.identity,
        &PINNED_RESIDENT_MODEL_REVISION,
        ResidentModelAcquisitionLimits::default(),
        &mut transport,
        ResidentModelAcquisitionRuntime {
            clock: &clock,
            retry_wait: &mut retry,
        },
        cancellation,
        &mut lock,
        &mut space,
        &lifecycle(root),
        &NativeResidentModelLifecycleBoundary,
        &mut |update| progress(update),
    )
    .map_err(redact_failure)
}

type RuntimeResolver = dyn Fn() -> Result<PathBuf, ResidentModelActivationFailure> + Send + Sync;

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
                    .map_err(|_| ResidentModelActivationFailure::Start)?;
                // Resolve again at the spawn boundary so post-install modification of
                // either retained evidence or extracted files fails closed.
                resolve_runtime(&runtime_root).map_err(|_| ResidentModelActivationFailure::Start)
            }),
            model_descriptor: &PINNED_RESIDENT_MODEL_REVISION.model,
            port: 32_391,
            tolerate_startup_transport_errors: false,
            health_interval: None,
        }
    }
}

impl ResidentModelActivationBoundary for NativeActivation {
    type Server = LlamaServer;

    fn launch(&self, model: &Path) -> Result<Self::Server, ResidentModelActivationFailure> {
        let executable = (self.resolve_runtime)()?;
        let mut config = LlamaServerConfig::new(executable, model, self.port)
            .with_model_descriptor(self.model_descriptor);
        if self.tolerate_startup_transport_errors {
            config = config.with_startup_transport_tolerance();
        }
        if let Some(interval) = self.health_interval {
            config = config.with_health_interval(interval);
        }
        LlamaServer::spawn(config).map_err(|_| ResidentModelActivationFailure::Start)
    }

    fn await_ready(&self, server: &mut Self::Server) -> Result<(), ResidentModelActivationFailure> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while std::time::Instant::now() < deadline {
            match server.supervisor().status() {
                SidecarStatus::Healthy => return Ok(()),
                SidecarStatus::Stopped | SidecarStatus::Failed => {
                    return Err(ResidentModelActivationFailure::ExitedBeforeReady)
                }
                _ => std::thread::sleep(std::time::Duration::from_millis(100)),
            }
        }
        Err(ResidentModelActivationFailure::Readiness)
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
    lifecycle: &ResidentModelRevisionLifecycle,
    activation: &NativeActivation,
) -> Result<LlamaServer, InstallFailure> {
    match lifecycle.activate(&NativeResidentModelLifecycleBoundary, activation) {
        Ok(
            ResidentModelActivation::Active { server, .. }
            | ResidentModelActivation::RolledBack { server, .. },
        ) => Ok(server),
        _ => Err(InstallFailure {
            category: "activationFailed",
            message: "The local AI service could not be started.",
            cancelled: false,
        }),
    }
}

fn redact_failure(
    error: muniment_core::llama::install::ResidentModelInstallError,
) -> InstallFailure {
    let cancelled = matches!(
        error,
        ModelInstallError::Cancelled
            | ModelInstallError::Acquisition(ResidentModelAcquisitionError::Cancelled)
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
pub fn gemma_install_start(
    state: State<'_, ResidentModelInstallState>,
) -> ResidentModelInstallStatus {
    state.start()
}

#[tauri::command]
pub async fn gemma_install_status(
    state: State<'_, ResidentModelInstallState>,
) -> Result<ResidentModelInstallStatus, String> {
    gemma_install_status_handler(&state).await
}

async fn gemma_install_status_handler(
    state: &ResidentModelInstallState,
) -> Result<ResidentModelInstallStatus, String> {
    Ok(state.status().await)
}

#[tauri::command]
pub fn gemma_install_cancel(
    state: State<'_, ResidentModelInstallState>,
) -> ResidentModelInstallStatus {
    state.cancel()
}

#[tauri::command]
pub async fn required_model_acquisition_status(
    state: State<'_, ResidentModelInstallState>,
) -> Result<RequiredModelAcquisitionStatus, String> {
    Ok(state.acquisition_status().await)
}

#[tauri::command]
pub async fn dictation_polish(
    transcript: String,
    state: State<'_, ResidentModelInstallState>,
) -> Result<String, DictationPolishFailure> {
    let inner = Arc::clone(&state.inner);
    tauri::async_runtime::spawn_blocking(move || {
        ResidentModelInstallState::polish_dictation_with_inner(inner, transcript)
    })
    .await
    .unwrap_or_else(|_| Err(DictationPolishFailure::request_failed()))
}

#[tauri::command]
pub async fn dictation_transform(
    transcript: String,
    transform: DictationTransform,
    state: State<'_, ResidentModelInstallState>,
) -> Result<String, DictationTransformFailure> {
    let inner = Arc::clone(&state.inner);
    tauri::async_runtime::spawn_blocking(move || {
        ResidentModelInstallState::transform_dictation_with_inner(inner, transcript, transform)
    })
    .await
    .unwrap_or_else(|_| Err(DictationTransformFailure::request_failed()))
}

#[tauri::command]
pub async fn onboarding_triage(
    entries: Vec<ExtractedEntry>,
    state: State<'_, ResidentModelInstallState>,
) -> Result<OnboardingTriageResponse, OnboardingTriageFailure> {
    let inner = Arc::clone(&state.inner);
    tauri::async_runtime::spawn_blocking(move || {
        ResidentModelInstallState::triage_onboarding_with_inner(inner, entries)
    })
    .await
    .unwrap_or_else(|_| Err(OnboardingTriageFailure::request_failed()))
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
        ResidentModelDownloadRequest, ResidentModelDownloadResponse,
        ResidentModelDownloadTransport, ResidentModelTransportError,
    };
    use muniment_core::llama::lifecycle::{
        ResidentModelNoticeDescriptor, ResidentModelRevisionDescriptor,
    };
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
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};
    use tauri::Manager;

    fn state(status: ResidentModelInstallStatus, runner: Arc<Runner>) -> ResidentModelInstallState {
        ResidentModelInstallState::with_runner(std::env::temp_dir(), status, runner)
    }

    fn app_with_state(state: ResidentModelInstallState) -> tauri::App<tauri::test::MockRuntime> {
        let app = tauri::test::mock_app();
        assert!(app.manage(state));
        app
    }

    fn polish_fixture(body: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            loop {
                let count = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..count]);
                let Some(headers_end) = request.windows(4).position(|part| part == b"\r\n\r\n")
                else {
                    continue;
                };
                let headers = String::from_utf8_lossy(&request[..headers_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|value| value.parse::<usize>().ok())
                    })
                    .unwrap();
                if request.len() >= headers_end + 4 + content_length {
                    break;
                }
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8(request).unwrap()
        });
        (url, worker)
    }

    fn extracted(source_name: &str, provenance: &str, text: &str) -> ExtractedEntry {
        ExtractedEntry {
            source_name: source_name.to_owned(),
            kind: muniment_core::import_preview::EntryKind::Markdown,
            text: text.to_owned(),
            source_provenance: provenance.to_owned(),
        }
    }

    fn app_with_parakeet_state(
        state: ParakeetInstallState,
    ) -> tauri::App<tauri::test::MockRuntime> {
        let app = tauri::test::mock_app();
        assert!(app.manage(state));
        app
    }

    fn public_status(app: &tauri::App<tauri::test::MockRuntime>) -> ResidentModelInstallStatus {
        tauri::async_runtime::block_on(gemma_install_status(app.state())).unwrap()
    }

    fn await_public_status(
        app: &tauri::App<tauri::test::MockRuntime>,
        expected: ResidentModelInstallStatus,
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

    #[test]
    fn dictation_polish_uses_the_retained_ready_server() {
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"Meet me at noon."}}]}"#;
        let (url, worker) = polish_fixture(body);
        let state = state(
            ResidentModelInstallStatus::Installed,
            Arc::new(|root, _, _| Ok(root.into())),
        );
        state.inner.lock().unwrap().server = Some(ServingServer::Test(url));
        let app = app_with_state(state);

        assert_eq!(
            tauri::async_runtime::block_on(dictation_polish(
                "um meet me at noon".into(),
                app.state(),
            ))
            .unwrap(),
            "Meet me at noon."
        );
        let request = worker.join().unwrap();
        assert!(request.starts_with("POST /v1/chat/completions "));
        assert!(request.contains("um meet me at noon"));
    }

    #[test]
    fn dictation_polish_treats_transport_failure_as_unavailable_with_stale_health() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let state = state(
            ResidentModelInstallStatus::Installed,
            Arc::new(|root, _, _| Ok(root.into())),
        );
        state.inner.lock().unwrap().server = Some(ServingServer::Test(url));
        let app = app_with_state(state);

        assert_eq!(
            tauri::async_runtime::block_on(dictation_polish("hello".into(), app.state()))
                .unwrap_err(),
            DictationPolishFailure::unavailable()
        );
    }

    #[test]
    fn dictation_polish_rejects_unavailable_and_empty_inputs_with_typed_errors() {
        let runner_calls = Arc::new(AtomicUsize::new(0));
        for status in [
            ResidentModelInstallStatus::NotInstalled,
            ResidentModelInstallStatus::Installing,
            ResidentModelInstallStatus::Failed {
                category: "activationFailed",
                message: "The model server could not be started.",
            },
            ResidentModelInstallStatus::Installed,
        ] {
            let calls = Arc::clone(&runner_calls);
            let state = state(
                status,
                Arc::new(move |root, _, _| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(root.into())
                }),
            );
            assert_eq!(
                state.polish_dictation("hello".into()).unwrap_err(),
                DictationPolishFailure::unavailable()
            );
        }
        assert_eq!(runner_calls.load(Ordering::SeqCst), 0);

        let state = state(
            ResidentModelInstallStatus::Installed,
            Arc::new(|root, _, _| Ok(root.into())),
        );
        state.inner.lock().unwrap().server =
            Some(ServingServer::Test("http://127.0.0.1:1".to_owned()));
        assert_eq!(
            state.polish_dictation(" \n\t ".into()).unwrap_err(),
            DictationPolishFailure::invalid_transcript()
        );

        let malformed_body = r#"{"choices":[]}"#;
        let (url, worker) = polish_fixture(malformed_body);
        state.inner.lock().unwrap().server = Some(ServingServer::Test(url));
        assert_eq!(
            state.polish_dictation("hello".into()).unwrap_err(),
            DictationPolishFailure::request_failed()
        );
        worker.join().unwrap();
    }

    #[test]
    fn dictation_transform_uses_the_retained_ready_server_and_selected_transform() {
        let body =
            r#"{"choices":[{"message":{"role":"assistant","content":"A formal response."}}]}"#;
        let (url, worker) = polish_fixture(body);
        let state = state(
            ResidentModelInstallStatus::Installed,
            Arc::new(|root, _, _| Ok(root.into())),
        );
        state.inner.lock().unwrap().server = Some(ServingServer::Test(url));
        let app = app_with_state(state);

        assert_eq!(
            tauri::async_runtime::block_on(dictation_transform(
                "this is casual".into(),
                DictationTransform::Formal,
                app.state(),
            ))
            .unwrap(),
            "A formal response."
        );
        let request = worker.join().unwrap();
        assert!(request.starts_with("POST /v1/chat/completions "));
        assert!(request.contains("Rewrite the transcript in a formal, professional tone."));
        assert!(request.contains("this is casual"));
    }

    #[test]
    fn dictation_transform_maps_transport_failure_to_unavailable() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let state = state(
            ResidentModelInstallStatus::Installed,
            Arc::new(|root, _, _| Ok(root.into())),
        );
        state.inner.lock().unwrap().server = Some(ServingServer::Test(url));
        let app = app_with_state(state);

        assert_eq!(
            tauri::async_runtime::block_on(dictation_transform(
                "private transcript".into(),
                DictationTransform::Short,
                app.state(),
            ))
            .unwrap_err(),
            DictationTransformFailure::unavailable()
        );
    }

    #[test]
    fn dictation_transform_rejects_empty_input_and_unavailable_server() {
        let state = state(
            ResidentModelInstallStatus::Installed,
            Arc::new(|root, _, _| Ok(root.into())),
        );
        state.inner.lock().unwrap().server = Some(ServingServer::Test(
            "http://127.0.0.1:1/private-model.gguf".to_owned(),
        ));

        let empty_error = ResidentModelInstallState::transform_dictation_with_inner(
            Arc::clone(&state.inner),
            " \n\t ".into(),
            DictationTransform::KeyPoints,
        )
        .unwrap_err();
        assert_eq!(empty_error, DictationTransformFailure::invalid_transcript());

        state.inner.lock().unwrap().server = None;
        let unavailable_error = ResidentModelInstallState::transform_dictation_with_inner(
            Arc::clone(&state.inner),
            "private transcript".into(),
            DictationTransform::Long,
        )
        .unwrap_err();
        assert_eq!(unavailable_error, DictationTransformFailure::unavailable());
        let serialized = serde_json::to_string(&unavailable_error).unwrap();
        assert!(!serialized.contains("private transcript"));
        assert!(!serialized.contains("private-model.gguf"));
        assert!(!serialized.contains("127.0.0.1"));
    }

    #[test]
    fn dictation_transform_maps_malformed_model_response_to_request_failure() {
        let (url, worker) = polish_fixture(r#"{"choices":[]}"#);
        let state = state(
            ResidentModelInstallStatus::Installed,
            Arc::new(|root, _, _| Ok(root.into())),
        );
        state.inner.lock().unwrap().server = Some(ServingServer::Test(url));

        let error = ResidentModelInstallState::transform_dictation_with_inner(
            Arc::clone(&state.inner),
            "private transcript".into(),
            DictationTransform::Short,
        )
        .unwrap_err();
        assert_eq!(error, DictationTransformFailure::request_failed());
        let serialized = serde_json::to_string(&error).unwrap();
        assert!(!serialized.contains("private transcript"));
        assert!(!serialized.contains("choices"));
        worker.join().unwrap();
    }

    #[test]
    fn onboarding_triage_uses_approved_entries_and_returns_structured_report() {
        let body = r###"{"choices":[{"message":{"role":"assistant","content":"## User type\nDeveloper\n## Proposed Home layout\nProject folders\n## Starter agents\n- Researcher\n- Writer"}}],"usage":{"prompt_tokens":12,"completion_tokens":8,"total_tokens":20}}"###;
        let (url, worker) = polish_fixture(body);
        let state = state(
            ResidentModelInstallStatus::Installed,
            Arc::new(|root, _, _| Ok(root.into())),
        );
        state.inner.lock().unwrap().server = Some(ServingServer::Test(url));
        let app = app_with_state(state);

        let response = tauri::async_runtime::block_on(onboarding_triage(
            vec![extracted("notes.md", "sha256:approved", "approved body")],
            app.state(),
        ))
        .unwrap();
        assert_eq!(response.report.user_type, "Developer");
        assert_eq!(response.report.starter_agents, ["Researcher", "Writer"]);
        assert_eq!(response.usage.unwrap().total_tokens, Some(20));
        let request = worker.join().unwrap();
        assert!(request.contains("approved body"));
        assert!(request.contains("sha256:approved"));
    }

    #[test]
    fn onboarding_triage_rejects_unavailable_model_without_request() {
        let state = state(
            ResidentModelInstallStatus::Installed,
            Arc::new(|root, _, _| Ok(root.into())),
        );
        let app = app_with_state(state);
        let error = tauri::async_runtime::block_on(onboarding_triage(
            vec![extracted("notes.md", "sha256:x", "body")],
            app.state(),
        ))
        .unwrap_err();
        assert_eq!(error, OnboardingTriageFailure::unavailable());
    }

    #[test]
    fn onboarding_triage_rejects_invalid_input_before_model_request() {
        let state = state(
            ResidentModelInstallStatus::Installed,
            Arc::new(|root, _, _| Ok(root.into())),
        );
        state.inner.lock().unwrap().server =
            Some(ServingServer::Test("http://127.0.0.1:1".to_owned()));
        let app = app_with_state(state);
        let error =
            tauri::async_runtime::block_on(onboarding_triage(Vec::new(), app.state())).unwrap_err();
        assert_eq!(
            error,
            OnboardingTriageFailure::input(OnboardingTriageRequestError::Empty)
        );
    }

    #[test]
    fn onboarding_triage_maps_invalid_report_to_redacted_typed_error() {
        let body = r###"{"choices":[{"message":{"role":"assistant","content":"## User type\nDeveloper"}}]}"###;
        let (url, worker) = polish_fixture(body);
        let state = state(
            ResidentModelInstallStatus::Installed,
            Arc::new(|root, _, _| Ok(root.into())),
        );
        state.inner.lock().unwrap().server = Some(ServingServer::Test(url));
        let app = app_with_state(state);
        let error = tauri::async_runtime::block_on(onboarding_triage(
            vec![extracted("notes.md", "sha256:x", "body")],
            app.state(),
        ))
        .unwrap_err();
        assert_eq!(error, OnboardingTriageFailure::invalid_response());
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({"kind":"invalidModelResponse","message":"Local AI returned an invalid triage report."})
        );
        worker.join().unwrap();
    }
    static TEST_REVISION: ResidentModelRevisionDescriptor = ResidentModelRevisionDescriptor {
        identity: "gemma-fixture-v1",
        revision: "test-revision",
        model: &TEST_MODEL,
        notice: ResidentModelNoticeDescriptor {
            filename: "NOTICE.txt",
            contents: b"notice",
        },
    };
    static TEST_REVISIONS: [&ResidentModelRevisionDescriptor; 1] = [&TEST_REVISION];
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

        fn lifecycle(&self) -> ResidentModelRevisionLifecycle {
            ResidentModelRevisionLifecycle::new(self.0.clone(), &TEST_REVISIONS, &TEST_REVISION)
                .unwrap()
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
            ResidentModelInstallStatus::NotInstalled
        );
    }

    #[test]
    fn real_background_failure_is_fail_open_for_onboarding_and_closed_for_ai() {
        let root = TestRoot::new();
        let attempts = Arc::new(AtomicUsize::new(0));
        let runner_attempts = Arc::clone(&attempts);
        let state = ResidentModelInstallState::with_runner(
            root.0.clone(),
            ResidentModelInstallStatus::NotInstalled,
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
                ResidentModelInstallStatus::Failed { .. }
            ) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        let snapshot = tauri::async_runtime::block_on(state.acquisition_status());
        assert!(matches!(
            snapshot.status,
            ResidentModelInstallStatus::Failed { .. }
        ));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert!(snapshot.folder_setup_available);
        assert!(!snapshot.ai_features_available);
        assert!(snapshot.retrying_in_background);
        assert_eq!(snapshot.downloaded_bytes, 0);
        assert_eq!(
            snapshot.total_bytes,
            PINNED_RESIDENT_MODEL_REVISION.model.byte_size
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
        let state = state(ResidentModelInstallStatus::NotInstalled, runner);

        assert_eq!(state.start(), ResidentModelInstallStatus::Installing);
        entered.wait();
        let snapshot = tauri::async_runtime::block_on(state.acquisition_status());
        assert_eq!(snapshot.status, ResidentModelInstallStatus::Installing);
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
        let mut state = state(ResidentModelInstallStatus::NotInstalled, runner);
        state.inner.lock().unwrap().progress.total_bytes = 10;
        state.inspector = Arc::new(|_| ResidentModelInstallStatus::Installed);
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
        impl ResidentModelDownloadTransport for ModelTransport {
            type Body = Cursor<Vec<u8>>;

            fn download(
                &mut self,
                request: &ResidentModelDownloadRequest,
            ) -> Result<ResidentModelDownloadResponse<Self::Body>, ResidentModelTransportError>
            {
                assert_eq!(request.offset, 1, "the staged model download must resume");
                Ok(ResidentModelDownloadResponse {
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
                install_resident_model_revision_with_progress(
                    &runner_root.join("staging"),
                    TEST_REVISION.identity,
                    &TEST_REVISION,
                    ResidentModelAcquisitionLimits::default(),
                    &mut transport,
                    ResidentModelAcquisitionRuntime {
                        clock: &clock,
                        retry_wait: &mut retry,
                    },
                    cancellation,
                    &mut lock,
                    &mut space,
                    &ResidentModelRevisionLifecycle::new(
                        runner_root.clone(),
                        &TEST_REVISIONS,
                        &TEST_REVISION,
                    )
                    .unwrap(),
                    &NativeResidentModelLifecycleBoundary,
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
                .map_err(|_| ResidentModelActivationFailure::Start)
            }),
            model_descriptor: &TEST_MODEL,
            port,
            tolerate_startup_transport_errors: true,
            health_interval: Some(Duration::from_millis(10)),
        };
        let activation = Arc::new(activation);
        let activation_lifecycle = root.lifecycle();
        let mut state = ResidentModelInstallState::with_runner(
            root.0.clone(),
            ResidentModelInstallStatus::NotInstalled,
            runner,
        );
        state.activator = Arc::new(move |_| {
            activate_with(&activation_lifecycle, &activation).map(ServingServer::Native)
        });
        state.inspector = Arc::new(|root| {
            inspect_lifecycle(
                &ResidentModelRevisionLifecycle::new(
                    root.to_owned(),
                    &TEST_REVISIONS,
                    &TEST_REVISION,
                )
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
        assert_eq!(
            state.polish_dictation("hello".into()).unwrap_err(),
            DictationPolishFailure::unavailable(),
            "polish must fail as unavailable after the supervised process exits"
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
            ResidentModelInstallStatus::Installed
        );

        fs::remove_file(revision.join(TEST_MODEL.filename)).unwrap();
        assert_eq!(
            inspect_lifecycle(&root.lifecycle()),
            ResidentModelInstallStatus::Failed {
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
        let app = app_with_state(state(ResidentModelInstallStatus::NotInstalled, runner));
        let state = app.state::<ResidentModelInstallState>();
        assert_eq!(state.start(), ResidentModelInstallStatus::Installing);
        assert_eq!(state.start(), ResidentModelInstallStatus::Installing);
        release.wait();
        await_public_status(&app, ResidentModelInstallStatus::Installed);
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
        let app = app_with_state(state(ResidentModelInstallStatus::NotInstalled, runner));
        let state = app.state::<ResidentModelInstallState>();
        state.start();
        let expected = ResidentModelInstallStatus::Failed {
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
        let app = app_with_state(state(ResidentModelInstallStatus::NotInstalled, runner));
        let state = app.state::<ResidentModelInstallState>();
        state.start();
        assert_eq!(state.cancel(), ResidentModelInstallStatus::Installing);
        await_public_status(&app, ResidentModelInstallStatus::Cancelled);
        assert_eq!(public_status(&app), ResidentModelInstallStatus::Cancelled);
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
