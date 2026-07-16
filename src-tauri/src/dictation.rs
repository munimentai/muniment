use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use muniment_core::asr::utterance::{Utterance, UtteranceConfig};
use muniment_core::asr::{
    AsrLifecycleError, DictationPipeline, OfflineParakeetRecognizer, SileroVoiceActivityDetector,
    PARAKEET_MODEL_MANIFEST, VAD_SAMPLE_RATE,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime, State};

use crate::model_install::parakeet_lifecycle;
use crate::voice_capture::{VoiceCaptureError, VoiceCaptureState};

const POLL_INTERVAL: Duration = Duration::from_millis(20);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum DictationStatus {
    Idle,
    Starting,
    Running,
    Stopped,
    ModelNotInstalled {
        category: &'static str,
        message: &'static str,
    },
    Failed {
        category: &'static str,
        message: &'static str,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum DictationEvent {
    Transcript { text: String },
}

#[derive(Clone, Copy)]
struct DictationFailure {
    model_not_installed: bool,
    category: &'static str,
    message: &'static str,
}

type Emit = dyn Fn(DictationEvent) + Send + Sync;
type Running = dyn Fn() + Send + Sync;
type Runner = dyn Fn(&Path, &VoiceCaptureState, &AtomicBool, &Running, &Emit) -> Result<(), DictationFailure>
    + Send
    + Sync;

struct Inner {
    generation: u64,
    status: DictationStatus,
    stop: Option<Arc<AtomicBool>>,
}

pub struct DictationState {
    root: PathBuf,
    inner: Arc<Mutex<Inner>>,
    runner: Arc<Runner>,
}

impl DictationState {
    pub fn new(root: PathBuf) -> Self {
        Self::with_runner(root, Arc::new(run_native))
    }

    fn with_runner(root: PathBuf, runner: Arc<Runner>) -> Self {
        Self {
            root,
            inner: Arc::new(Mutex::new(Inner {
                generation: 0,
                status: DictationStatus::Idle,
                stop: None,
            })),
            runner,
        }
    }

    fn start<R: Runtime>(
        &self,
        app: AppHandle<R>,
        capture: Arc<VoiceCaptureState>,
    ) -> DictationStatus {
        let (generation, stop) = {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if inner.stop.is_some() {
                return inner.status.clone();
            }
            inner.generation = inner.generation.wrapping_add(1);
            inner.status = DictationStatus::Starting;
            let stop = Arc::new(AtomicBool::new(false));
            inner.stop = Some(stop.clone());
            (inner.generation, stop)
        };
        let root = self.root.clone();
        let runner = self.runner.clone();
        let inner = self.inner.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let running_inner = inner.clone();
            let running = move || set_status(&running_inner, generation, DictationStatus::Running);
            let emit = move |event| {
                let _ = app.emit("dictation-event", event);
            };
            let result = runner(&root, &capture, &stop, &running, &emit);
            let mut state = inner.lock().unwrap_or_else(|e| e.into_inner());
            if state.generation != generation {
                return;
            }
            state.stop = None;
            state.status = match result {
                Ok(()) => DictationStatus::Stopped,
                Err(error) if error.model_not_installed => DictationStatus::ModelNotInstalled {
                    category: error.category,
                    message: error.message,
                },
                Err(error) => DictationStatus::Failed {
                    category: error.category,
                    message: error.message,
                },
            };
        });
        DictationStatus::Starting
    }

    fn stop(&self) -> DictationStatus {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(stop) = &inner.stop {
            stop.store(true, Ordering::Release);
        }
        inner.status.clone()
    }
}

fn set_status(inner: &Mutex<Inner>, generation: u64, status: DictationStatus) {
    let mut inner = inner.lock().unwrap_or_else(|e| e.into_inner());
    if inner.generation == generation {
        inner.status = status;
    }
}

fn config() -> UtteranceConfig {
    UtteranceConfig {
        pre_roll_samples: 4_800,
        min_speech_samples: 4_000,
        trailing_silence_samples: 8_000,
        max_utterance_samples: 320_000,
        max_buffered_samples: 320_000,
    }
}

fn run_native(
    root: &Path,
    capture: &VoiceCaptureState,
    stop: &AtomicBool,
    running: &Running,
    emit: &Emit,
) -> Result<(), DictationFailure> {
    let lifecycle = parakeet_lifecycle(root);
    let installed = match lifecycle.resolve_current() {
        Ok(installed) => installed,
        Err(AsrLifecycleError::RevisionMissing)
            if matches!(root.join("current").try_exists(), Ok(false)) =>
        {
            return Err(DictationFailure {
                model_not_installed: true,
                category: "modelNotInstalled",
                message: "The speech model is not installed.",
            });
        }
        Err(_) => {
            return Err(failure(
                "invalidInstall",
                "The installed speech model could not be verified.",
            ));
        }
    };
    let vad_artifact = PARAKEET_MODEL_MANIFEST
        .additional_artifact
        .expect("the installed manifest includes Silero VAD")
        .artifact;
    let vad =
        SileroVoiceActivityDetector::from_installed_path(installed.join(vad_artifact.filename))
            .map_err(|_| failure("modelUnavailable", "The speech model could not be loaded."))?;
    let recognizer = OfflineParakeetRecognizer::from_verified_current(&lifecycle)
        .map_err(|_| failure("modelUnavailable", "The speech model could not be loaded."))?;
    let mut pipeline = DictationPipeline::new(vad, config())
        .map_err(|_| failure("pipelineUnavailable", "Dictation could not be started."))?;
    capture.start().map_err(redact_capture)?;
    running();
    let result = (|| {
        while !stop.load(Ordering::Acquire) {
            let samples = capture.take_samples().map_err(redact_capture)?;
            recognize_all(
                &recognizer,
                pipeline.push(&samples).map_err(|_| {
                    failure("audioFailed", "Dictation audio could not be processed.")
                })?,
                emit,
            )?;
            std::thread::sleep(POLL_INTERVAL);
        }
        let samples = capture.stop_and_take_samples().map_err(redact_capture)?;
        recognize_all(
            &recognizer,
            pipeline
                .push(&samples)
                .map_err(|_| failure("audioFailed", "Dictation audio could not be processed."))?,
            emit,
        )?;
        if let Some(utterance) = pipeline.flush() {
            recognize(&recognizer, utterance, emit)?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = capture.stop();
    }
    result
}

fn recognize_all(
    recognizer: &OfflineParakeetRecognizer,
    utterances: Vec<Utterance>,
    emit: &Emit,
) -> Result<(), DictationFailure> {
    for utterance in utterances {
        recognize(recognizer, utterance, emit)?;
    }
    Ok(())
}

fn recognize(
    recognizer: &OfflineParakeetRecognizer,
    utterance: Utterance,
    emit: &Emit,
) -> Result<(), DictationFailure> {
    let text = recognizer
        .decode(VAD_SAMPLE_RATE, &utterance.samples)
        .map_err(|_| failure("recognitionFailed", "Speech recognition failed."))?;
    emit(DictationEvent::Transcript { text });
    Ok(())
}

fn failure(category: &'static str, message: &'static str) -> DictationFailure {
    DictationFailure {
        model_not_installed: false,
        category,
        message,
    }
}

fn redact_capture(error: VoiceCaptureError) -> DictationFailure {
    match error {
        VoiceCaptureError::NoInputDevice => failure("noInputDevice", "No microphone is available."),
        VoiceCaptureError::AlreadyRunning => {
            failure("captureBusy", "The microphone is already in use.")
        }
        _ => failure("captureFailed", "Microphone capture failed."),
    }
}

#[tauri::command]
pub fn dictation_start<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, DictationState>,
    capture: State<'_, Arc<VoiceCaptureState>>,
) -> DictationStatus {
    state.start(app, capture.inner().clone())
}

#[tauri::command]
pub fn dictation_stop(state: State<'_, DictationState>) -> DictationStatus {
    state.stop()
}

#[tauri::command]
pub fn dictation_status(state: State<'_, DictationState>) -> DictationStatus {
    state
        .inner
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .status
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    use tauri::{Listener, Manager};

    fn app_with_runner(runner: Arc<Runner>) -> tauri::App<tauri::test::MockRuntime> {
        let app = tauri::test::mock_app();
        assert!(app.manage(DictationState::with_runner(PathBuf::new(), runner)));
        assert!(app.manage(Arc::new(VoiceCaptureState::new())));
        app
    }

    fn wait_status(app: &tauri::App<tauri::test::MockRuntime>, expected: DictationStatus) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if app.state::<DictationState>().inner.lock().unwrap().status == expected {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            app.state::<DictationState>().inner.lock().unwrap().status,
            expected
        );
    }

    #[test]
    fn start_emits_transcript_and_stop_emits_trailing_flush() {
        let runner = Arc::new(
            |_: &Path, _: &VoiceCaptureState, stop: &AtomicBool, running: &Running, emit: &Emit| {
                running();
                emit(DictationEvent::Transcript {
                    text: "first".into(),
                });
                while !stop.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(2));
                }
                emit(DictationEvent::Transcript {
                    text: "trailing".into(),
                });
                Ok(())
            },
        );
        let app = app_with_runner(runner);
        let (tx, rx) = mpsc::channel();
        app.handle().listen("dictation-event", move |event| {
            let _ = tx.send(event.payload().to_owned());
        });
        assert_eq!(
            dictation_start(app.handle().clone(), app.state(), app.state()),
            DictationStatus::Starting
        );
        wait_status(&app, DictationStatus::Running);
        assert_eq!(
            dictation_start(app.handle().clone(), app.state(), app.state()),
            DictationStatus::Running
        );
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            r#"{"type":"transcript","text":"first"}"#
        );
        assert_eq!(dictation_stop(app.state()), DictationStatus::Running);
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            r#"{"type":"transcript","text":"trailing"}"#
        );
        wait_status(&app, DictationStatus::Stopped);
        assert_eq!(dictation_stop(app.state()), DictationStatus::Stopped);
    }

    #[test]
    fn failure_statuses_are_closed_and_redacted() {
        let runner = Arc::new(
            |_: &Path, _: &VoiceCaptureState, _: &AtomicBool, _: &Running, _: &Emit| {
                Err(DictationFailure {
                    model_not_installed: true,
                    category: "modelNotInstalled",
                    message: "The speech model is not installed.",
                })
            },
        );
        let app = app_with_runner(runner);
        dictation_start(app.handle().clone(), app.state(), app.state());
        let expected = DictationStatus::ModelNotInstalled {
            category: "modelNotInstalled",
            message: "The speech model is not installed.",
        };
        wait_status(&app, expected.clone());
        let json = serde_json::to_string(&expected).unwrap();
        assert_eq!(
            json,
            r#"{"state":"modelNotInstalled","category":"modelNotInstalled","message":"The speech model is not installed."}"#
        );
        assert!(!json.contains('/') && !json.contains("onnx"));
    }
}
