use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use muniment_core::asr::level::VoiceLevelFrame;
use muniment_core::asr::partial::{PartialPacing, PartialTranscripts};
use muniment_core::asr::utterance::{Utterance, UtteranceConfig};
use muniment_core::asr::{
    AsrLifecycleError, AsrModelSetStamp, DictationPipeline, OfflineParakeetRecognizer,
    ParakeetDecoding, ParakeetHotwords, SileroVoiceActivityDetector, VadDecisionSource,
    PARAKEET_MODEL_MANIFEST, VAD_SAMPLE_RATE,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Runtime, State};

use crate::model_install::parakeet_lifecycle;
use crate::voice_capture::{CapturedPcm, VoiceCaptureError, VoiceCaptureState};
use crate::voice_output::{self, OutputMute, Tone};

const POLL_INTERVAL: Duration = Duration::from_millis(20);
/// The wait between two voice level events, about 30 a second.
const LEVEL_INTERVAL: Duration = Duration::from_millis(33);

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

/// The Voice settings a dictation reads when it starts.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DictationOptions {
    microphone: Option<String>,
    mute: bool,
    sound: bool,
    /// Custom words that bias recognition toward their spelling.
    words: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum DictationEvent {
    /// The final transcript of one utterance. It replaces the utterance's partial text.
    Transcript { text: String },
    /// The words of the utterance in progress so far. Empty text clears them.
    Partial { text: String },
    /// The speaker's voice loudness, from 0 for silence to 1.
    Level { level: f32 },
}

#[derive(Clone, Copy)]
struct DictationFailure {
    model_not_installed: bool,
    category: &'static str,
    message: &'static str,
}

type Emit = dyn Fn(DictationEvent) + Send + Sync;
type Running = dyn Fn() + Send + Sync;
type Runner = dyn Fn(
        &Path,
        &VoiceCaptureState,
        &[String],
        &AtomicBool,
        &Running,
        &Emit,
    ) -> Result<(), DictationFailure>
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

/// Tells whether two stamps name the same unchanged model files.
trait ModelStamp {
    fn matches(&self, other: &Self) -> bool;
}

impl ModelStamp for AsrModelSetStamp {
    fn matches(&self, other: &Self) -> bool {
        AsrModelSetStamp::matches(self, other)
    }
}

/// The loaded recognizer of the last session, the model files it read and
/// the decoding mode the session asked for.
struct LoadedRecognizer<R, S> {
    directory: PathBuf,
    stamp: S,
    decoding: ParakeetDecoding,
    recognizer: Arc<R>,
}

/// Keeps one recognizer across dictation sessions. A session reuses it only
/// while the verified current revision has the same directory and file stamps
/// and the session asks for the same decoding mode. Any other session drops the
/// loaded recognizer before it loads the next, so one encoder is resident.
struct RecognizerCache<R, S = AsrModelSetStamp>(Mutex<Option<LoadedRecognizer<R, S>>>);

impl<R, S> Default for RecognizerCache<R, S> {
    fn default() -> Self {
        Self(Mutex::new(None))
    }
}

impl<R, S: ModelStamp> RecognizerCache<R, S> {
    fn get<E>(
        &self,
        current: Result<(PathBuf, S), E>,
        decoding: ParakeetDecoding,
        load: impl FnOnce() -> Result<R, E>,
    ) -> Result<Arc<R>, E> {
        let mut slot = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let (directory, stamp) = match current {
            Ok(current) => current,
            Err(error) => {
                // A removed or failed model drops the loaded recognizer.
                *slot = None;
                return Err(error);
            }
        };
        if let Some(loaded) = slot.as_ref().filter(|loaded| {
            loaded.directory == directory
                && loaded.stamp.matches(&stamp)
                && loaded.decoding == decoding
        }) {
            return Ok(Arc::clone(&loaded.recognizer));
        }
        *slot = None;
        let recognizer = Arc::new(load()?);
        *slot = Some(LoadedRecognizer {
            directory,
            stamp,
            decoding,
            recognizer: Arc::clone(&recognizer),
        });
        Ok(recognizer)
    }
}

/// Loads a recognizer for `decoding`. A failed Hotwords load falls back to
/// Greedy, so custom words never stop dictation.
fn load_with_fallback<R, E>(
    decoding: ParakeetDecoding,
    load: impl Fn(ParakeetDecoding) -> Result<R, E>,
) -> Result<R, E> {
    match load(decoding) {
        Err(_) if decoding == ParakeetDecoding::Hotwords => {
            eprintln!("Custom words are unavailable. Dictation uses plain recognition.");
            load(ParakeetDecoding::Greedy)
        }
        result => result,
    }
}

impl DictationState {
    pub fn new(root: PathBuf) -> Self {
        let recognizers = Arc::new(RecognizerCache::default());
        Self::with_runner(
            root,
            Arc::new(move |root, capture, words, stop, running, emit| {
                run_native(root, &recognizers, capture, words, stop, running, emit)
            }),
        )
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
        options: DictationOptions,
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
            // The start tone plays before the microphone opens, so the
            // transcript never hears it, and before the output mutes.
            if options.sound {
                voice_output::play(Tone::Start);
            }
            let muted = options.mute.then(OutputMute::engage);
            let result = runner(&root, &capture, &options.words, &stop, &running, &emit);
            drop(muted);
            if options.sound && result.is_ok() {
                voice_output::play(Tone::Stop);
            }
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

fn installed_failure(root: &Path, error: AsrLifecycleError) -> DictationFailure {
    match error {
        AsrLifecycleError::RevisionMissing
            if matches!(root.join("current").try_exists(), Ok(false)) =>
        {
            DictationFailure {
                model_not_installed: true,
                category: "modelNotInstalled",
                message: "The speech model is not installed.",
            }
        }
        _ => failure(
            "invalidInstall",
            "The installed speech model could not be verified.",
        ),
    }
}

fn run_native(
    root: &Path,
    recognizers: &RecognizerCache<OfflineParakeetRecognizer>,
    capture: &VoiceCaptureState,
    words: &[String],
    stop: &AtomicBool,
    running: &Running,
    emit: &Emit,
) -> Result<(), DictationFailure> {
    let hotwords = ParakeetHotwords::new(words);
    let decoding = if hotwords.is_empty() {
        ParakeetDecoding::Greedy
    } else {
        ParakeetDecoding::Hotwords
    };
    let lifecycle = parakeet_lifecycle(root);
    // Verification reuses the recorded digest of each unchanged file, so a
    // session start hashes the model only after its files change.
    let current = lifecycle
        .resolve_current_stamped()
        .map_err(|error| installed_failure(root, error));
    let installed = current
        .as_ref()
        .map(|(directory, _)| directory.clone())
        .map_err(|error| *error);
    let recognizer = recognizers.get(current, decoding, || {
        load_with_fallback(decoding, |decoding| {
            OfflineParakeetRecognizer::from_verified_current_with_decoding(&lifecycle, decoding)
        })
        .map_err(|_| failure("modelUnavailable", "The speech model could not be loaded."))
    })?;
    let installed = installed?;
    let vad_artifact = PARAKEET_MODEL_MANIFEST
        .additional_artifact
        .expect("the installed manifest includes Silero VAD")
        .artifact;
    let vad =
        SileroVoiceActivityDetector::from_installed_path(installed.join(vad_artifact.filename))
            .map_err(|_| failure("modelUnavailable", "The speech model could not be loaded."))?;
    let pipeline = DictationPipeline::new(vad, config())
        .map_err(|_| failure("pipelineUnavailable", "Dictation could not be started."))?;
    let decoder = BiasedDecoder {
        recognizer: recognizer.as_ref(),
        hotwords,
    };
    let partials = PartialTranscripts::new(PartialPacing::default());
    run_coordinator(
        capture,
        pipeline,
        &decoder,
        Some(partials),
        stop,
        running,
        emit,
    )
}

trait CaptureSource {
    fn start(&self) -> Result<(), VoiceCaptureError>;
    fn take_samples(&self) -> Result<CapturedPcm, VoiceCaptureError>;
    fn stop_and_take_samples(&self) -> Result<CapturedPcm, VoiceCaptureError>;
    fn stop(&self) -> Result<(), VoiceCaptureError>;
    fn level(&self) -> Option<VoiceLevelFrame>;
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
    fn level(&self) -> Option<VoiceLevelFrame> {
        self.level()
    }
}

trait TranscriptDecoder {
    fn decode(&self, samples: &[f32]) -> Result<String, ()>;
}

/// Decodes each utterance biased toward the session's custom words. A Greedy
/// recognizer ignores them.
struct BiasedDecoder<'a> {
    recognizer: &'a OfflineParakeetRecognizer,
    hotwords: ParakeetHotwords,
}

impl TranscriptDecoder for BiasedDecoder<'_> {
    fn decode(&self, samples: &[f32]) -> Result<String, ()> {
        self.recognizer
            .decode_with_hotwords(VAD_SAMPLE_RATE, samples, &self.hotwords)
            .map_err(|_| ())
    }
}

/// Runs one dictation session. With `partials`, the utterance in progress is
/// decoded again at their pace and its words go out as partial events.
fn run_coordinator<C: CaptureSource + Sync, D: VadDecisionSource, R: TranscriptDecoder>(
    capture: &C,
    mut pipeline: DictationPipeline<D>,
    recognizer: &R,
    mut partials: Option<PartialTranscripts>,
    stop: &AtomicBool,
    running: &Running,
    emit: &Emit,
) -> Result<(), DictationFailure> {
    capture.start().map_err(redact_capture)?;
    running();
    let levels_done = AtomicBool::new(false);
    let result = std::thread::scope(|scope| {
        // Decodes hold the loop below, so the levels go out from their own thread.
        scope.spawn(|| send_levels(capture, &levels_done, emit));
        let result = run_capture(
            capture,
            &mut pipeline,
            recognizer,
            &mut partials,
            stop,
            emit,
        );
        levels_done.store(true, Ordering::Release);
        result
    });
    if result.is_err() {
        let _ = capture.stop();
    }
    result
}

/// Sends each new voice level of the capture until `done`.
fn send_levels<C: CaptureSource>(capture: &C, done: &AtomicBool, emit: &Emit) {
    let mut sent = None;
    while !done.load(Ordering::Acquire) {
        if let Some(frame) = capture.level().filter(|frame| sent != Some(frame.sequence)) {
            sent = Some(frame.sequence);
            emit(DictationEvent::Level { level: frame.level });
        }
        std::thread::sleep(LEVEL_INTERVAL);
    }
}

fn run_capture<C: CaptureSource, D: VadDecisionSource, R: TranscriptDecoder>(
    capture: &C,
    pipeline: &mut DictationPipeline<D>,
    recognizer: &R,
    partials: &mut Option<PartialTranscripts>,
    stop: &AtomicBool,
    emit: &Emit,
) -> Result<(), DictationFailure> {
    while !stop.load(Ordering::Acquire) {
        let batch = capture.take_samples().map_err(redact_capture)?;
        push_and_recognize(
            pipeline,
            recognizer,
            &batch.samples,
            partials.as_mut(),
            emit,
        )?;
        if batch.discontinuity_after {
            pipeline.discontinuity();
            end_partial(partials.as_mut(), emit);
        }
        if let Some(partials) = partials.as_mut() {
            recognize_partial(pipeline, recognizer, partials, emit);
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    let batch = capture.stop_and_take_samples().map_err(redact_capture)?;
    push_and_recognize(
        pipeline,
        recognizer,
        &batch.samples,
        partials.as_mut(),
        emit,
    )?;
    if batch.discontinuity_after {
        pipeline.discontinuity();
    }
    if let Some(utterance) = pipeline.flush() {
        recognize(recognizer, utterance, emit)?;
    }
    // A session that ends without a final transcript leaves no partial text.
    end_partial(partials.as_mut(), emit);
    Ok(())
}

fn push_and_recognize<D: VadDecisionSource, R: TranscriptDecoder>(
    pipeline: &mut DictationPipeline<D>,
    recognizer: &R,
    samples: &[f32],
    partials: Option<&mut PartialTranscripts>,
    emit: &Emit,
) -> Result<(), DictationFailure> {
    let pushed = pipeline.push(samples);
    let emitted = match &pushed {
        Ok(utterances) => !utterances.is_empty(),
        Err(error) => !error.emitted.is_empty(),
    };
    // The final transcript replaces the partial text of its utterance.
    if let (true, Some(partials)) = (emitted, partials) {
        partials.ended();
    }
    match pushed {
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

/// Decodes the utterance in progress when it is due and sends its words when
/// they changed. A failed partial decode is skipped: the final decode reports.
fn recognize_partial<D: VadDecisionSource, R: TranscriptDecoder>(
    pipeline: &DictationPipeline<D>,
    recognizer: &R,
    partials: &mut PartialTranscripts,
    emit: &Emit,
) {
    let Some(samples) = pipeline.in_progress() else {
        return;
    };
    let started = Instant::now();
    if !partials.due(started, samples.len()) {
        return;
    }
    let Ok(text) = recognizer.decode(samples) else {
        return;
    };
    if let Some(text) = partials.decoded(started, started.elapsed(), samples.len(), text) {
        emit(DictationEvent::Partial { text });
    }
}

/// Clears the partial text of an utterance that ends without a final transcript.
fn end_partial(partials: Option<&mut PartialTranscripts>, emit: &Emit) {
    if partials.is_some_and(|partials| partials.ended()) {
        emit(DictationEvent::Partial {
            text: String::new(),
        });
    }
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
        VoiceCaptureError::StartupTimedOut => failure(
            "captureTimeout",
            "The microphone did not start. Check your input device and try again.",
        ),
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
    options: Option<DictationOptions>,
) -> DictationStatus {
    let options = options.unwrap_or_default();
    capture.set_input(options.microphone.clone());
    state.start(app, capture.inner().clone(), options)
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

    #[derive(Clone, Copy, PartialEq)]
    struct TestStamp(Option<u8>);

    impl ModelStamp for TestStamp {
        fn matches(&self, other: &Self) -> bool {
            self.0.is_some() && self == other
        }
    }

    #[test]
    fn recognizers_are_reused_until_the_model_changes_or_fails() {
        let cache = RecognizerCache::<u32, TestStamp>::default();
        let loads = std::cell::Cell::new(0);
        let load = || {
            loads.set(loads.get() + 1);
            Ok::<u32, ()>(loads.get())
        };
        let current = |stamp| Ok((PathBuf::from("rev"), TestStamp(stamp)));
        let greedy = ParakeetDecoding::Greedy;

        assert_eq!(*cache.get(current(Some(1)), greedy, load).unwrap(), 1);
        assert_eq!(*cache.get(current(Some(1)), greedy, load).unwrap(), 1);
        assert_eq!(*cache.get(current(Some(2)), greedy, load).unwrap(), 2);
        assert_eq!(
            *cache
                .get(
                    Ok((PathBuf::from("other"), TestStamp(Some(2)))),
                    greedy,
                    load
                )
                .unwrap(),
            3
        );
        assert_eq!(*cache.get(current(None), greedy, load).unwrap(), 4);
        assert_eq!(*cache.get(current(None), greedy, load).unwrap(), 5);
        assert!(cache.get(Err(()), greedy, load).is_err());
        assert!(cache.0.lock().unwrap().is_none());
        assert!(cache.get(current(Some(3)), greedy, || Err(())).is_err());
        assert!(cache.0.lock().unwrap().is_none());
        assert_eq!(*cache.get(current(Some(3)), greedy, load).unwrap(), 6);
    }

    #[test]
    fn a_decoding_change_drops_the_loaded_recognizer_before_the_reload() {
        let cache = RecognizerCache::<u32, TestStamp>::default();
        let loads = std::cell::Cell::new(0);
        let current = || Ok::<_, ()>((PathBuf::from("rev"), TestStamp(Some(1))));
        let mut previous = std::sync::Weak::new();
        for (decoding, expected) in [
            (ParakeetDecoding::Greedy, 1),
            (ParakeetDecoding::Hotwords, 2),
            (ParakeetDecoding::Hotwords, 2),
            (ParakeetDecoding::Greedy, 3),
        ] {
            let recognizer = cache
                .get(current(), decoding, || {
                    // The cache holds the only other handle, so the previous
                    // recognizer is gone once the cache drops it.
                    assert!(previous.upgrade().is_none());
                    loads.set(loads.get() + 1);
                    Ok(loads.get())
                })
                .unwrap();
            assert_eq!(*recognizer, expected);
            previous = Arc::downgrade(&recognizer);
        }
        assert_eq!(loads.get(), 3);
    }

    #[test]
    fn a_failed_hotwords_load_falls_back_to_greedy() {
        let attempts = Mutex::new(Vec::new());
        let load = |fail_hotwords: bool| {
            let attempts = &attempts;
            move |decoding| {
                attempts.lock().unwrap().push(decoding);
                if fail_hotwords && decoding == ParakeetDecoding::Hotwords {
                    Err("native")
                } else {
                    Ok(decoding)
                }
            }
        };
        assert_eq!(
            load_with_fallback(ParakeetDecoding::Hotwords, load(true)),
            Ok(ParakeetDecoding::Greedy)
        );
        assert_eq!(
            load_with_fallback(ParakeetDecoding::Hotwords, load(false)),
            Ok(ParakeetDecoding::Hotwords)
        );
        assert_eq!(
            load_with_fallback(ParakeetDecoding::Greedy, load(true)),
            Ok(ParakeetDecoding::Greedy)
        );
        assert_eq!(
            load_with_fallback(ParakeetDecoding::Greedy, |_| Err::<(), _>("native")),
            Err("native")
        );
        assert_eq!(
            *attempts.lock().unwrap(),
            vec![
                ParakeetDecoding::Hotwords,
                ParakeetDecoding::Greedy,
                ParakeetDecoding::Hotwords,
                ParakeetDecoding::Greedy,
            ]
        );
    }

    #[test]
    fn options_carry_custom_words_and_default_to_none() {
        let options: DictationOptions = serde_json::from_str(
            r#"{"microphone":null,"mute":true,"sound":false,"words":["Muniment","Zylaxis"]}"#,
        )
        .unwrap();
        assert_eq!(options.words, ["Muniment", "Zylaxis"]);
        assert!(options.mute);
        let options: DictationOptions = serde_json::from_str(r#"{"mute":false}"#).unwrap();
        assert!(options.words.is_empty());
    }
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
        fn level(&self) -> Option<VoiceLevelFrame> {
            None
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
        paced_runner(capture, decisions, lengths, discontinuities, None)
    }

    fn paced_runner(
        capture: FakeCapture,
        decisions: VecDeque<Result<VoiceActivity, VadError>>,
        lengths: Arc<Mutex<Vec<usize>>>,
        discontinuities: Arc<std::sync::atomic::AtomicUsize>,
        pacing: Option<PartialPacing>,
    ) -> Arc<Runner> {
        let capture = Arc::new(capture);
        Arc::new(
            move |_: &Path, _: &VoiceCaptureState, _: &[String], stop, running, emit| {
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
                    pacing.map(PartialTranscripts::new),
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
            dictation_start(app.handle().clone(), app.state(), app.state(), None),
            DictationStatus::Starting
        );
        wait_status(&app, DictationStatus::Running);
        assert_eq!(
            dictation_start(app.handle().clone(), app.state(), app.state(), None),
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
    fn start_passes_custom_words_to_the_runner() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorded = seen.clone();
        let app = app_with_runner(Arc::new(
            move |_: &Path, _: &VoiceCaptureState, words: &[String], _, _, _| {
                recorded.lock().unwrap().push(words.to_vec());
                Ok(())
            },
        ));
        let options: DictationOptions = serde_json::from_str(r#"{"words":["Zylaxis"]}"#).unwrap();
        dictation_start(
            app.handle().clone(),
            app.state(),
            app.state(),
            Some(options),
        );
        wait_status(&app, DictationStatus::Stopped);
        dictation_start(app.handle().clone(), app.state(), app.state(), None);
        wait_status(&app, DictationStatus::Stopped);
        let deadline = Instant::now() + Duration::from_secs(2);
        while seen.lock().unwrap().len() < 2 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            *seen.lock().unwrap(),
            vec![vec!["Zylaxis".to_owned()], Vec::new()]
        );
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
        dictation_start(app.handle().clone(), app.state(), app.state(), None);
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
        dictation_start(app.handle().clone(), app.state(), app.state(), None);
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

    fn eager() -> Option<PartialPacing> {
        Some(PartialPacing {
            min_interval: Duration::ZERO,
            max_interval: Duration::ZERO,
            load_factor: 1,
            min_new_samples: 1,
        })
    }

    fn events(app: &tauri::App<tauri::test::MockRuntime>) -> mpsc::Receiver<String> {
        let (tx, rx) = mpsc::channel();
        app.handle().listen("dictation-event", move |event| {
            let _ = tx.send(event.payload().to_owned());
        });
        rx
    }

    #[test]
    fn the_utterance_in_progress_sends_partial_text_until_its_final_transcript() {
        let lengths = Arc::new(Mutex::new(Vec::new()));
        let batch = |frames| CapturedPcm {
            samples: pcm(frames),
            discontinuity_after: false,
        };
        let runner = paced_runner(
            FakeCapture {
                batches: Mutex::new(VecDeque::from([batch(10), batch(4), batch(16)])),
                stopped: Mutex::new(None),
            },
            vad(14, 16),
            lengths.clone(),
            Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            eager(),
        );
        let app = app_with_runner(runner);
        let rx = events(&app);
        dictation_start(app.handle().clone(), app.state(), app.state(), None);
        let received: Vec<_> = (0..3)
            .map(|_| rx.recv_timeout(Duration::from_secs(1)).unwrap())
            .collect();
        assert_eq!(
            received,
            [
                r#"{"type":"partial","text":"decoded-1"}"#,
                r#"{"type":"partial","text":"decoded-2"}"#,
                r#"{"type":"transcript","text":"decoded-3"}"#,
            ]
        );
        dictation_stop(app.state());
        wait_status(&app, DictationStatus::Stopped);
        assert!(rx.try_recv().is_err());
        assert_eq!(
            *lengths.lock().unwrap(),
            vec![
                10 * VAD_FRAME_SIZE,
                14 * VAD_FRAME_SIZE,
                14 * VAD_FRAME_SIZE + 8_000
            ]
        );
    }

    #[test]
    fn lost_audio_clears_the_partial_text() {
        let runner = paced_runner(
            FakeCapture {
                batches: Mutex::new(VecDeque::from([
                    CapturedPcm {
                        samples: pcm(10),
                        discontinuity_after: false,
                    },
                    CapturedPcm {
                        samples: Vec::new(),
                        discontinuity_after: true,
                    },
                ])),
                stopped: Mutex::new(None),
            },
            vad(10, 0),
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            eager(),
        );
        let app = app_with_runner(runner);
        let rx = events(&app);
        dictation_start(app.handle().clone(), app.state(), app.state(), None);
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            r#"{"type":"partial","text":"decoded-1"}"#
        );
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            r#"{"type":"partial","text":""}"#
        );
        dictation_stop(app.state());
        wait_status(&app, DictationStatus::Stopped);
        assert!(rx.try_recv().is_err());
    }

    /// A capture whose levels a test stores.
    struct LevelCapture(muniment_core::asr::level::VoiceLevelSlot);

    impl CaptureSource for LevelCapture {
        fn start(&self) -> Result<(), VoiceCaptureError> {
            Ok(())
        }
        fn take_samples(&self) -> Result<CapturedPcm, VoiceCaptureError> {
            Ok(CapturedPcm {
                samples: Vec::new(),
                discontinuity_after: false,
            })
        }
        fn stop_and_take_samples(&self) -> Result<CapturedPcm, VoiceCaptureError> {
            self.take_samples()
        }
        fn stop(&self) -> Result<(), VoiceCaptureError> {
            Ok(())
        }
        fn level(&self) -> Option<VoiceLevelFrame> {
            self.0.load()
        }
    }

    #[test]
    fn each_new_voice_level_goes_out_once_until_the_session_ends() {
        let capture = LevelCapture(muniment_core::asr::level::VoiceLevelSlot::new());
        let done = AtomicBool::new(false);
        let (tx, rx) = mpsc::channel();
        let emit = move |event| {
            let _ = tx.send(event);
        };
        std::thread::scope(|scope| {
            scope.spawn(|| send_levels(&capture, &done, &emit));
            std::thread::sleep(LEVEL_INTERVAL * 2);
            assert!(rx.try_recv().is_err());
            capture.0.store(0.5);
            assert_eq!(
                rx.recv_timeout(Duration::from_secs(1)).unwrap(),
                DictationEvent::Level { level: 0.5 }
            );
            std::thread::sleep(LEVEL_INTERVAL * 3);
            assert!(rx.try_recv().is_err());
            capture.0.store(0.25);
            assert_eq!(
                rx.recv_timeout(Duration::from_secs(1)).unwrap(),
                DictationEvent::Level { level: 0.25 }
            );
            done.store(true, Ordering::Release);
        });
        capture.0.store(1.0);
        assert!(rx.recv_timeout(LEVEL_INTERVAL * 3).is_err());
        assert_eq!(
            serde_json::to_string(&DictationEvent::Level { level: 0.5 }).unwrap(),
            r#"{"type":"level","level":0.5}"#
        );
    }
}
