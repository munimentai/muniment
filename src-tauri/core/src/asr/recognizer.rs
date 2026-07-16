//! Safe ownership and validation boundary for offline Parakeet recognition.
//!
//! The native adapter is opt-in so ordinary core builds never link or download
//! a sherpa-onnx runtime. Tests exercise the same orchestration through a small
//! ABI seam.

#[cfg(any(test, feature = "native-sherpa-onnx"))]
use std::path::{Path, PathBuf};

#[cfg(feature = "native-sherpa-onnx")]
use super::verify_parakeet_model_set;
use super::AsrModelSetVerificationError;

pub const SAMPLE_RATE_HZ: u32 = 16_000;
pub const FEATURE_DIM: i32 = 80;
pub const NUM_THREADS: i32 = 2;

#[cfg(feature = "native-sherpa-onnx")]
mod native;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecognizerError {
    ModelSet(AsrModelSetVerificationError),
    InvalidSampleRate,
    InvalidSample,
    UtteranceTooLong,
    InvalidModelPath,
    RecognizerCreationFailed,
    StreamCreationFailed,
    DecodeFailed,
    ResultUnavailable,
    InvalidTranscript,
}

impl std::fmt::Display for RecognizerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::ModelSet(_) => "ASR model set failed verification",
            Self::InvalidSampleRate => "ASR input must be mono 16 kHz audio",
            Self::InvalidSample => "ASR input contains an invalid normalized sample",
            Self::UtteranceTooLong => "ASR utterance is too long",
            Self::InvalidModelPath => "ASR model path is not representable by the native runtime",
            Self::RecognizerCreationFailed => "ASR recognizer could not be created",
            Self::StreamCreationFailed => "ASR stream could not be created",
            Self::DecodeFailed => "ASR decode failed",
            Self::ResultUnavailable => "ASR result is unavailable",
            Self::InvalidTranscript => "ASR result is not valid UTF-8",
        };
        f.write_str(message)
    }
}

impl std::error::Error for RecognizerError {}

impl From<AsrModelSetVerificationError> for RecognizerError {
    fn from(value: AsrModelSetVerificationError) -> Self {
        Self::ModelSet(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(any(test, feature = "native-sherpa-onnx"))]
struct OfflineConfig {
    pub encoder: PathBuf,
    pub decoder: PathBuf,
    pub joiner: PathBuf,
    pub tokens: PathBuf,
    pub provider: &'static str,
    pub model_type: &'static str,
    pub decoding_method: &'static str,
    pub sample_rate: i32,
    pub feature_dim: i32,
    pub num_threads: i32,
}

/// Opaque identifiers are values owned by an ABI implementation, never native
/// pointers exposed through the safe recognizer API.
#[cfg(any(test, feature = "native-sherpa-onnx"))]
trait OfflineAbi {
    type Recognizer: Copy;
    type Stream: Copy;
    type Result: Copy;

    fn create_recognizer(&self, config: &OfflineConfig) -> Option<Self::Recognizer>;
    fn destroy_recognizer(&self, recognizer: Self::Recognizer);
    fn create_stream(&self, recognizer: Self::Recognizer) -> Option<Self::Stream>;
    fn destroy_stream(&self, stream: Self::Stream);
    fn accept_waveform(
        &self,
        stream: Self::Stream,
        sample_rate: i32,
        samples: &[f32],
        sample_count: i32,
    );
    fn decode(&self, recognizer: Self::Recognizer, stream: Self::Stream);
    fn get_result(
        &self,
        recognizer: Self::Recognizer,
        stream: Self::Stream,
    ) -> Option<Self::Result>;
    fn copy_result_text(&self, result: Self::Result) -> Option<Vec<u8>>;
    fn destroy_result(&self, result: Self::Result);
}

/// Performs one utterance-final decode after validating the complete pinned
/// model set and the C ABI input contract.
#[cfg(feature = "native-sherpa-onnx")]
pub fn recognize_parakeet(
    model_directory: &Path,
    sample_rate_hz: u32,
    samples: &[f32],
) -> Result<String, RecognizerError> {
    recognize_parakeet_with_abi(
        &native::SherpaOnnxAbi,
        model_directory,
        sample_rate_hz,
        samples,
        |directory| verify_parakeet_model_set(directory).map_err(RecognizerError::from),
    )
}

#[cfg(any(test, feature = "native-sherpa-onnx"))]
fn recognize_parakeet_with_abi(
    abi: &impl OfflineAbi,
    model_directory: &Path,
    sample_rate_hz: u32,
    samples: &[f32],
    verify: impl FnOnce(&Path) -> Result<(), RecognizerError>,
) -> Result<String, RecognizerError> {
    validate_input(sample_rate_hz, samples)?;
    verify(model_directory)?;
    let config = make_config(model_directory)?;

    let recognizer = abi
        .create_recognizer(&config)
        .ok_or(RecognizerError::RecognizerCreationFailed)?;
    let stream = match abi.create_stream(recognizer) {
        Some(stream) => stream,
        None => {
            abi.destroy_recognizer(recognizer);
            return Err(RecognizerError::StreamCreationFailed);
        }
    };

    let sample_count =
        i32::try_from(samples.len()).map_err(|_| RecognizerError::UtteranceTooLong)?;
    abi.accept_waveform(stream, SAMPLE_RATE_HZ as i32, samples, sample_count);
    abi.decode(recognizer, stream);
    // The pinned decode call returns void. Its only observable failure is that
    // no result snapshot is produced afterward.
    let result = match abi.get_result(recognizer, stream) {
        Some(result) => result,
        None => {
            abi.destroy_stream(stream);
            abi.destroy_recognizer(recognizer);
            return Err(RecognizerError::DecodeFailed);
        }
    };
    let transcript = abi
        .copy_result_text(result)
        .ok_or(RecognizerError::ResultUnavailable)
        .and_then(|text| {
            std::str::from_utf8(&text)
                .map(str::to_owned)
                .map_err(|_| RecognizerError::InvalidTranscript)
        });
    abi.destroy_result(result);
    abi.destroy_stream(stream);
    abi.destroy_recognizer(recognizer);
    transcript
}

#[cfg(any(test, feature = "native-sherpa-onnx"))]
fn validate_input(sample_rate_hz: u32, samples: &[f32]) -> Result<(), RecognizerError> {
    if sample_rate_hz != SAMPLE_RATE_HZ {
        return Err(RecognizerError::InvalidSampleRate);
    }
    validate_sample_count(samples.len())?;
    if samples
        .iter()
        .any(|sample| !sample.is_finite() || !(-1.0..=1.0).contains(sample))
    {
        return Err(RecognizerError::InvalidSample);
    }
    Ok(())
}

#[cfg(any(test, feature = "native-sherpa-onnx"))]
fn validate_sample_count(sample_count: usize) -> Result<(), RecognizerError> {
    if sample_count > i32::MAX as usize {
        return Err(RecognizerError::UtteranceTooLong);
    }
    Ok(())
}

#[cfg(any(test, feature = "native-sherpa-onnx"))]
fn make_config(directory: &Path) -> Result<OfflineConfig, RecognizerError> {
    let paths = [
        directory.join("encoder.int8.onnx"),
        directory.join("decoder.int8.onnx"),
        directory.join("joiner.int8.onnx"),
        directory.join("tokens.txt"),
    ];
    if paths
        .iter()
        .any(|path| path.to_str().is_none() || path.to_string_lossy().as_bytes().contains(&0))
    {
        return Err(RecognizerError::InvalidModelPath);
    }
    Ok(OfflineConfig {
        encoder: paths[0].clone(),
        decoder: paths[1].clone(),
        joiner: paths[2].clone(),
        tokens: paths[3].clone(),
        provider: "cpu",
        model_type: "nemo_transducer",
        decoding_method: "greedy_search",
        sample_rate: SAMPLE_RATE_HZ as i32,
        feature_dim: FEATURE_DIM,
        num_threads: NUM_THREADS,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Clone, Copy)]
    enum Failure {
        None,
        Recognizer,
        Stream,
        Decode,
        Result,
        Text,
    }
    struct Fake {
        failure: Failure,
        text: Vec<u8>,
        events: RefCell<Vec<&'static str>>,
        config: RefCell<Option<OfflineConfig>>,
        samples: RefCell<Vec<f32>>,
    }
    impl Fake {
        fn new(failure: Failure) -> Self {
            Self {
                failure,
                text: b"owned transcript".to_vec(),
                events: RefCell::new(vec![]),
                config: RefCell::new(None),
                samples: RefCell::new(vec![]),
            }
        }
    }
    impl OfflineAbi for Fake {
        type Recognizer = u8;
        type Stream = u8;
        type Result = u8;
        fn create_recognizer(&self, c: &OfflineConfig) -> Option<u8> {
            self.events.borrow_mut().push("create recognizer");
            *self.config.borrow_mut() = Some(c.clone());
            (!matches!(self.failure, Failure::Recognizer)).then_some(1)
        }
        fn destroy_recognizer(&self, _: u8) {
            self.events.borrow_mut().push("destroy recognizer")
        }
        fn create_stream(&self, _: u8) -> Option<u8> {
            self.events.borrow_mut().push("create stream");
            (!matches!(self.failure, Failure::Stream)).then_some(2)
        }
        fn destroy_stream(&self, _: u8) {
            self.events.borrow_mut().push("destroy stream")
        }
        fn accept_waveform(&self, _: u8, rate: i32, samples: &[f32], sample_count: i32) {
            assert_eq!(rate, 16_000);
            assert_eq!(sample_count, samples.len() as i32);
            self.events.borrow_mut().push("accept");
            self.samples.borrow_mut().extend_from_slice(samples)
        }
        fn decode(&self, _: u8, _: u8) {
            self.events.borrow_mut().push("decode");
        }
        fn get_result(&self, recognizer: u8, stream: u8) -> Option<u8> {
            assert_eq!((recognizer, stream), (1, 2));
            self.events.borrow_mut().push("get result");
            (!matches!(self.failure, Failure::Decode | Failure::Result)).then_some(3)
        }
        fn copy_result_text(&self, _: u8) -> Option<Vec<u8>> {
            self.events.borrow_mut().push("copy text");
            (!matches!(self.failure, Failure::Text)).then_some(self.text.clone())
        }
        fn destroy_result(&self, _: u8) {
            self.events.borrow_mut().push("destroy result")
        }
    }
    fn run(fake: &Fake, samples: &[f32]) -> Result<String, RecognizerError> {
        recognize_parakeet_with_abi(fake, Path::new("models"), 16_000, samples, |_| Ok(()))
    }

    #[test]
    fn carries_config_samples_and_owns_text() {
        let fake = Fake::new(Failure::None);
        let samples = [-1.0, 0.25, 1.0];
        let text = run(&fake, &samples).unwrap();
        assert_eq!(text, "owned transcript");
        assert_eq!(&*fake.samples.borrow(), &samples);
        let c = fake.config.borrow();
        let c = c.as_ref().unwrap();
        assert_eq!(c.encoder, Path::new("models/encoder.int8.onnx"));
        assert_eq!(c.decoder, Path::new("models/decoder.int8.onnx"));
        assert_eq!(c.joiner, Path::new("models/joiner.int8.onnx"));
        assert_eq!(c.tokens, Path::new("models/tokens.txt"));
        assert_eq!(
            (
                c.provider,
                c.model_type,
                c.decoding_method,
                c.sample_rate,
                c.feature_dim,
                c.num_threads
            ),
            ("cpu", "nemo_transducer", "greedy_search", 16_000, 80, 2)
        );
        assert_eq!(
            &*fake.events.borrow(),
            &[
                "create recognizer",
                "create stream",
                "accept",
                "decode",
                "get result",
                "copy text",
                "destroy result",
                "destroy stream",
                "destroy recognizer"
            ]
        );
    }
    #[test]
    fn validation_never_calls_abi() {
        for (rate, samples, expected) in [
            (8_000, vec![0.0], RecognizerError::InvalidSampleRate),
            (16_000, vec![f32::NAN], RecognizerError::InvalidSample),
            (16_000, vec![1.01], RecognizerError::InvalidSample),
        ] {
            let fake = Fake::new(Failure::None);
            assert_eq!(
                recognize_parakeet_with_abi(&fake, Path::new("models"), rate, &samples, |_| Ok(()))
                    .unwrap_err(),
                expected
            );
            assert!(fake.events.borrow().is_empty());
        }
        assert_eq!(
            validate_sample_count(i32::MAX as usize + 1),
            Err(RecognizerError::UtteranceTooLong)
        );
    }
    #[test]
    fn verification_never_calls_abi() {
        let fake = Fake::new(Failure::None);
        let error = recognize_parakeet_with_abi(&fake, Path::new("models"), 16_000, &[], |_| {
            Err(RecognizerError::ModelSet(
                AsrModelSetVerificationError::Missing,
            ))
        })
        .unwrap_err();
        assert!(matches!(error, RecognizerError::ModelSet(_)));
        assert!(fake.events.borrow().is_empty());
    }
    #[test]
    fn cleans_up_each_failure_stage() {
        let cases = [
            (Failure::Recognizer, vec!["create recognizer"]),
            (
                Failure::Stream,
                vec!["create recognizer", "create stream", "destroy recognizer"],
            ),
            (
                Failure::Decode,
                vec![
                    "create recognizer",
                    "create stream",
                    "accept",
                    "decode",
                    "get result",
                    "destroy stream",
                    "destroy recognizer",
                ],
            ),
            (
                Failure::Result,
                vec![
                    "create recognizer",
                    "create stream",
                    "accept",
                    "decode",
                    "get result",
                    "destroy stream",
                    "destroy recognizer",
                ],
            ),
            (
                Failure::Text,
                vec![
                    "create recognizer",
                    "create stream",
                    "accept",
                    "decode",
                    "get result",
                    "copy text",
                    "destroy result",
                    "destroy stream",
                    "destroy recognizer",
                ],
            ),
        ];
        for (failure, events) in cases {
            let fake = Fake::new(failure);
            assert!(run(&fake, &[]).is_err());
            assert_eq!(*fake.events.borrow(), events);
        }
    }
    #[test]
    fn invalid_utf8_is_typed_and_cleaned_up() {
        let mut fake = Fake::new(Failure::None);
        fake.text = vec![0xff];
        assert_eq!(
            run(&fake, &[]).unwrap_err(),
            RecognizerError::InvalidTranscript
        );
        assert_eq!(
            &fake.events.borrow()[6..],
            &["destroy result", "destroy stream", "destroy recognizer"]
        );
    }
}
