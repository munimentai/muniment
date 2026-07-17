use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use muniment_core::asr::utterance::{Utterance, UtteranceConfig};
use muniment_core::asr::{
    AsrLifecycleError, DictationPipeline, OfflineParakeetRecognizer, SileroVoiceActivityDetector,
    VadDecisionSource, PARAKEET_MODEL_MANIFEST, VAD_SAMPLE_RATE,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime, State};

use crate::model_install::parakeet_lifecycle;
use crate::voice_capture::{CapturedPcm, VoiceCaptureError, VoiceCaptureState};

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
    let pipeline = DictationPipeline::new(vad, config())
        .map_err(|_| failure("pipelineUnavailable", "Dictation could not be started."))?;
    run_coordinator(capture, pipeline, &recognizer, stop, running, emit)
}

trait CaptureSource {
    fn start(&self) -> Result<(), VoiceCaptureError>;
    fn take_samples(&self) -> Result<CapturedPcm, VoiceCaptureError>;
    fn stop_and_take_samples(&self) -> Result<CapturedPcm, VoiceCaptureError>;
    fn stop(&self) -> Result<(), VoiceCaptureError>;
}

impl CaptureSource for VoiceCaptureState {
    fn start(&self) -> Result<(), VoiceCaptureError> {
        self.start()
    }
    fn take_samples(&self) -> Result<CapturedPcm, VoiceCaptureError> {
        self.take_samples()
    }
    fn stop_and_take_samples(&self) -> Result<CapturedPcm, VoiceCaptureError> {
        self.stop_and_take_samples()
    }
    fn stop(&self) -> Result<(), VoiceCaptureError> {
        self.stop()
    }
}

trait TranscriptDecoder {
    fn decode(&self, samples: &[f32]) -> Result<String, ()>;
}

impl TranscriptDecoder for OfflineParakeetRecognizer {
    fn decode(&self, samples: &[f32]) -> Result<String, ()> {
        self.decode(VAD_SAMPLE_RATE, samples).map_err(|_| ())
    }
}

fn run_coordinator<C: CaptureSource, D: VadDecisionSource, R: TranscriptDecoder>(
    capture: &C,
    mut pipeline: DictationPipeline<D>,
    recognizer: &R,
    stop: &AtomicBool,
    running: &Running,
    emit: &Emit,
) -> Result<(), DictationFailure> {
    capture.start().map_err(redact_capture)?;
    running();
    let result = (|| {
        while !stop.load(Ordering::Acquire) {
            let batch = capture.take_samples().map_err(redact_capture)?;
            push_and_recognize(&mut pipeline, recognizer, &batch.samples, emit)?;
            if batch.discontinuity_after {
                pipeline.discontinuity();
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        let batch = capture.stop_and_take_samples().map_err(redact_capture)?;
        push_and_recognize(&mut pipeline, recognizer, &batch.samples, emit)?;
        if batch.discontinuity_after {
            pipeline.discontinuity();
        }
        if let Some(utterance) = pipeline.flush() {
            recognize(recognizer, utterance, emit)?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = capture.stop();
    }
    result
}

fn push_and_recognize<D: VadDecisionSource, R: TranscriptDecoder>(
    pipeline: &mut DictationPipeline<D>,
    recognizer: &R,
    samples: &[f32],
    emit: &Emit,
) -> Result<(), DictationFailure> {
    match pipeline.push(samples) {
        Ok(utterances) => recognize_all(recognizer, utterances, emit),
        Err(error) => {
            recognize_all(recognizer, error.emitted, emit)?;
            Err(failure(
                "audioFailed",
                "Dictation audio could not be processed.",
            ))
        }
    }
}

fn recognize_all<R: TranscriptDecoder>(
    recognizer: &R,
    utterances: Vec<Utterance>,
    emit: &Emit,
) -> Result<(), DictationFailure> {
    for utterance in utterances {
        recognize(recognizer, utterance, emit)?;
    }
    Ok(())
}

fn recognize<R: TranscriptDecoder>(
    recognizer: &R,
    utterance: Utterance,
    emit: &Emit,
) -> Result<(), DictationFailure> {
    let text = recognizer
        .decode(&utterance.samples)
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
    use muniment_core::asr::utterance::VoiceActivity;
    use muniment_core::asr::{VadError, VAD_FRAME_SIZE};
    use std::collections::VecDeque;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    use tauri::{Listener, Manager};

    struct FakeCapture {
        batches: Mutex<VecDeque<CapturedPcm>>,
        stopped: Mutex<Option<CapturedPcm>>,
    }

    impl CaptureSource for FakeCapture {
        fn start(&self) -> Result<(), VoiceCaptureError> {
            Ok(())
        }
        fn take_samples(&self) -> Result<CapturedPcm, VoiceCaptureError> {
            Ok(self
                .batches
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(CapturedPcm {
                    samples: Vec::new(),
                    discontinuity_after: false,
                }))
        }
        fn stop_and_take_samples(&self) -> Result<CapturedPcm, VoiceCaptureError> {
            Ok(self.stopped.lock().unwrap().take().unwrap_or(CapturedPcm {
                samples: Vec::new(),
                discontinuity_after: false,
            }))
        }
        fn stop(&self) -> Result<(), VoiceCaptureError> {
            Ok(())
        }
    }

    struct FakeVad {
        decisions: VecDeque<Result<VoiceActivity, VadError>>,
        discontinuities: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl VadDecisionSource for FakeVad {
        fn detect(&mut self, _: &[f32]) -> Result<VoiceActivity, VadError> {
            self.decisions
                .pop_front()
                .unwrap_or(Ok(VoiceActivity::NonSpeech))
        }
        fn discontinuity(&mut self) {
            self.discontinuities.fetch_add(1, Ordering::Relaxed);
        }
    }

    struct FakeDecoder {
        lengths: Arc<Mutex<Vec<usize>>>,
    }

    impl TranscriptDecoder for FakeDecoder {
        fn decode(&self, samples: &[f32]) -> Result<String, ()> {
            let mut lengths = self.lengths.lock().unwrap();
            lengths.push(samples.len());
            Ok(format!("decoded-{}", lengths.len()))
        }
    }

    fn pcm(frames: usize) -> Vec<f32> {
        vec![0.25; frames * VAD_FRAME_SIZE]
    }

    fn vad(speech: usize, silence: usize) -> VecDeque<Result<VoiceActivity, VadError>> {
        std::iter::repeat_n(Ok(VoiceActivity::Speech), speech)
            .chain(std::iter::repeat_n(Ok(VoiceActivity::NonSpeech), silence))
            .collect()
    }

    fn coordinator_runner(
        capture: FakeCapture,
        decisions: VecDeque<Result<VoiceActivity, VadError>>,
        lengths: Arc<Mutex<Vec<usize>>>,
        discontinuities: Arc<std::sync::atomic::AtomicUsize>,
    ) -> Arc<Runner> {
        let capture = Arc::new(capture);
        Arc::new(
            move |_: &Path, _: &VoiceCaptureState, stop, running, emit| {
                let pipeline = DictationPipeline::new(
                    FakeVad {
                        decisions: decisions.clone(),
                        discontinuities: discontinuities.clone(),
                    },
                    config(),
                )
                .unwrap();
                run_coordinator(
                    capture.as_ref(),
                    pipeline,
                    &FakeDecoder {
                        lengths: lengths.clone(),
                    },
                    stop,
                    running,
                    emit,
                )
            },
        )
    }

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
        let lengths = Arc::new(Mutex::new(Vec::new()));
        let decisions = vad(8, 16)
            .into_iter()
            .chain(std::iter::repeat_n(Ok(VoiceActivity::Speech), 8))
            .collect();
        let runner = coordinator_runner(
            FakeCapture {
                batches: Mutex::new(VecDeque::from([CapturedPcm {
                    samples: pcm(24),
                    discontinuity_after: false,
                }])),
                stopped: Mutex::new(Some(CapturedPcm {
                    samples: pcm(8),
                    discontinuity_after: false,
                })),
            },
            decisions,
            lengths.clone(),
            Arc::new(std::sync::atomic::AtomicUsize::new(0)),
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
            r#"{"type":"transcript","text":"decoded-1"}"#
        );
        assert_eq!(dictation_stop(app.state()), DictationStatus::Running);
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            r#"{"type":"transcript","text":"decoded-2"}"#
        );
        wait_status(&app, DictationStatus::Stopped);
        assert_eq!(lengths.lock().unwrap().len(), 2);
        assert_eq!(dictation_stop(app.state()), DictationStatus::Stopped);
    }

    #[test]
    fn overflow_breaks_audio_before_trailing_flush() {
        let lengths = Arc::new(Mutex::new(Vec::new()));
        let discontinuities = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let runner = coordinator_runner(
            FakeCapture {
                batches: Mutex::new(VecDeque::from([CapturedPcm {
                    samples: pcm(8),
                    discontinuity_after: true,
                }])),
                stopped: Mutex::new(Some(CapturedPcm {
                    samples: pcm(8),
                    discontinuity_after: false,
                })),
            },
            vad(16, 0),
            lengths.clone(),
            discontinuities.clone(),
        );
        let app = app_with_runner(runner);
        dictation_start(app.handle().clone(), app.state(), app.state());
        wait_status(&app, DictationStatus::Running);
        std::thread::sleep(Duration::from_millis(30));
        dictation_stop(app.state());
        wait_status(&app, DictationStatus::Stopped);
        assert_eq!(*lengths.lock().unwrap(), vec![8 * VAD_FRAME_SIZE]);
        assert_eq!(discontinuities.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn push_error_decodes_prior_emission_and_redacts_backend_detail() {
        let lengths = Arc::new(Mutex::new(Vec::new()));
        let mut decisions = vad(8, 16);
        decisions.push_back(Err(VadError::NativeDetectorUnavailable));
        let runner = coordinator_runner(
            FakeCapture {
                batches: Mutex::new(VecDeque::from([CapturedPcm {
                    samples: pcm(25),
                    discontinuity_after: false,
                }])),
                stopped: Mutex::new(None),
            },
            decisions,
            lengths.clone(),
            Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        );
        let app = app_with_runner(runner);
        let (tx, rx) = mpsc::channel();
        app.handle().listen("dictation-event", move |event| {
            let _ = tx.send(event.payload().to_owned());
        });
        dictation_start(app.handle().clone(), app.state(), app.state());
        let expected = DictationStatus::Failed {
            category: "audioFailed",
            message: "Dictation audio could not be processed.",
        };
        wait_status(&app, expected.clone());
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            r#"{"type":"transcript","text":"decoded-1"}"#
        );
        assert_eq!(lengths.lock().unwrap().len(), 1);
        let json = serde_json::to_string(&expected).unwrap();
        assert!(!json.contains("NativeDetectorUnavailable") && !json.contains("onnx"));
    }
}
