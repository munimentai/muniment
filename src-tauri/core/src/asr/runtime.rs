//! Safe ownership boundary for offline Parakeet recognition.

use std::path::{Path, PathBuf};

use super::PARAKEET_ARTIFACTS;

pub const ASR_SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParakeetModelPaths {
    pub encoder: PathBuf,
    pub decoder: PathBuf,
    pub joiner: PathBuf,
    pub tokens: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfflineRecognizerConfig {
    pub model: ParakeetModelPaths,
    pub provider: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfflineRecognizerError {
    InvalidAudio,
    ModelArtifactMissing,
    ModelArtifactNotFile,
    NativeCreationFailed,
    NativeDecodeFailed,
    NativeResultFailed,
    InvalidTranscript,
}

impl std::fmt::Display for OfflineRecognizerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidAudio => "ASR audio is invalid",
            Self::ModelArtifactMissing => "ASR model set is incomplete",
            Self::ModelArtifactNotFile => "ASR model set contains a non-file artifact",
            Self::NativeCreationFailed => "ASR recognizer could not be created",
            Self::NativeDecodeFailed => "ASR audio could not be decoded",
            Self::NativeResultFailed => "ASR result could not be read",
            Self::InvalidTranscript => "ASR returned invalid transcript text",
        };
        f.write_str(message)
    }
}

impl std::error::Error for OfflineRecognizerError {}

/// Opaque adapter-owned identifiers. They deliberately cannot expose native
/// pointers through the safe recognition API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecognizerHandle(pub usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamHandle(pub usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResultHandle(pub usize);

/// Safe protocol implemented by the private native adapter and test doubles.
/// Implementations copy result bytes into the supplied `Vec`.
pub trait OfflineRecognizerAdapter {
    fn create_recognizer(
        &mut self,
        config: &OfflineRecognizerConfig,
    ) -> Result<RecognizerHandle, OfflineRecognizerError>;
    fn delete_recognizer(&mut self, recognizer: RecognizerHandle);
    fn create_stream(
        &mut self,
        recognizer: RecognizerHandle,
    ) -> Result<StreamHandle, OfflineRecognizerError>;
    fn delete_stream(&mut self, stream: StreamHandle);
    fn accept_waveform(&mut self, stream: StreamHandle, sample_rate: u32, samples: &[f32]);
    fn decode(
        &mut self,
        recognizer: RecognizerHandle,
        stream: StreamHandle,
    ) -> Result<(), OfflineRecognizerError>;
    fn get_result(
        &mut self,
        recognizer: RecognizerHandle,
        stream: StreamHandle,
    ) -> Result<ResultHandle, OfflineRecognizerError>;
    fn copy_result_text(
        &mut self,
        result: ResultHandle,
        destination: &mut Vec<u8>,
    ) -> Result<(), OfflineRecognizerError>;
    fn delete_result(&mut self, result: ResultHandle);
}

pub struct OfflineRecognizer<A> {
    adapter: A,
}

impl<A: OfflineRecognizerAdapter> OfflineRecognizer<A> {
    pub fn new(adapter: A) -> Self {
        Self { adapter }
    }

    /// Decodes one utterance of normalized mono 16 kHz PCM. The model
    /// directory must be the verified-current directory returned by the ASR
    /// revision lifecycle.
    pub fn recognize(
        &mut self,
        model_directory: &Path,
        samples: &[f32],
    ) -> Result<String, OfflineRecognizerError> {
        if samples.iter().any(|sample| !sample.is_finite()) {
            return Err(OfflineRecognizerError::InvalidAudio);
        }
        let config = OfflineRecognizerConfig {
            model: model_paths(model_directory)?,
            provider: "cpu",
        };
        let recognizer = self.adapter.create_recognizer(&config)?;
        let stream = match self.adapter.create_stream(recognizer) {
            Ok(stream) => stream,
            Err(error) => {
                self.adapter.delete_recognizer(recognizer);
                return Err(error);
            }
        };

        self.adapter
            .accept_waveform(stream, ASR_SAMPLE_RATE, samples);
        let outcome = self.decode_and_copy(recognizer, stream);
        self.adapter.delete_stream(stream);
        self.adapter.delete_recognizer(recognizer);
        outcome
    }

    fn decode_and_copy(
        &mut self,
        recognizer: RecognizerHandle,
        stream: StreamHandle,
    ) -> Result<String, OfflineRecognizerError> {
        self.adapter.decode(recognizer, stream)?;
        let result = self.adapter.get_result(recognizer, stream)?;
        let mut bytes = Vec::new();
        let copied = self.adapter.copy_result_text(result, &mut bytes);
        self.adapter.delete_result(result);
        copied?;
        String::from_utf8(bytes).map_err(|_| OfflineRecognizerError::InvalidTranscript)
    }
}

fn model_paths(directory: &Path) -> Result<ParakeetModelPaths, OfflineRecognizerError> {
    let mut paths = Vec::with_capacity(PARAKEET_ARTIFACTS.len());
    for artifact in PARAKEET_ARTIFACTS {
        let path = directory.join(artifact.filename);
        let metadata = std::fs::metadata(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                OfflineRecognizerError::ModelArtifactMissing
            } else {
                OfflineRecognizerError::ModelArtifactNotFile
            }
        })?;
        if !metadata.is_file() {
            return Err(OfflineRecognizerError::ModelArtifactNotFile);
        }
        paths.push(path);
    }
    Ok(ParakeetModelPaths {
        encoder: paths.remove(0),
        decoder: paths.remove(0),
        joiner: paths.remove(0),
        tokens: paths.remove(0),
    })
}

// All future extern declarations, C strings, raw pointers, and unsafe calls
// belong in this adapter only. Native libraries are intentionally not linked
// by this binding-contract slice.
mod native_adapter {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    #[derive(Default)]
    struct FakeAdapter {
        config: Option<OfflineRecognizerConfig>,
        sample_rate: Option<u32>,
        sample_count: Option<usize>,
        text: Vec<u8>,
        fail_create: bool,
        fail_stream: bool,
        fail_decode: bool,
        fail_result: bool,
        fail_copy: bool,
        releases: HashMap<&'static str, usize>,
        events: Vec<&'static str>,
    }

    impl OfflineRecognizerAdapter for FakeAdapter {
        fn create_recognizer(
            &mut self,
            config: &OfflineRecognizerConfig,
        ) -> Result<RecognizerHandle, OfflineRecognizerError> {
            self.config = Some(config.clone());
            if self.fail_create {
                Err(OfflineRecognizerError::NativeCreationFailed)
            } else {
                Ok(RecognizerHandle(1))
            }
        }
        fn delete_recognizer(&mut self, _: RecognizerHandle) {
            *self.releases.entry("recognizer").or_default() += 1;
        }
        fn create_stream(
            &mut self,
            _: RecognizerHandle,
        ) -> Result<StreamHandle, OfflineRecognizerError> {
            if self.fail_stream {
                Err(OfflineRecognizerError::NativeCreationFailed)
            } else {
                Ok(StreamHandle(2))
            }
        }
        fn delete_stream(&mut self, _: StreamHandle) {
            *self.releases.entry("stream").or_default() += 1;
        }
        fn accept_waveform(&mut self, _: StreamHandle, rate: u32, samples: &[f32]) {
            self.sample_rate = Some(rate);
            self.sample_count = Some(samples.len());
        }
        fn decode(
            &mut self,
            _: RecognizerHandle,
            _: StreamHandle,
        ) -> Result<(), OfflineRecognizerError> {
            if self.fail_decode {
                Err(OfflineRecognizerError::NativeDecodeFailed)
            } else {
                Ok(())
            }
        }
        fn get_result(
            &mut self,
            _: RecognizerHandle,
            _: StreamHandle,
        ) -> Result<ResultHandle, OfflineRecognizerError> {
            if self.fail_result {
                Err(OfflineRecognizerError::NativeResultFailed)
            } else {
                Ok(ResultHandle(3))
            }
        }
        fn copy_result_text(
            &mut self,
            _: ResultHandle,
            destination: &mut Vec<u8>,
        ) -> Result<(), OfflineRecognizerError> {
            if self.fail_copy {
                return Err(OfflineRecognizerError::NativeResultFailed);
            }
            destination.extend_from_slice(&self.text);
            self.events.push("copy");
            Ok(())
        }
        fn delete_result(&mut self, _: ResultHandle) {
            self.events.push("release-result");
            *self.releases.entry("result").or_default() += 1;
        }
    }

    fn fixture() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "muniment-asr-runtime-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        for artifact in PARAKEET_ARTIFACTS {
            fs::write(path.join(artifact.filename), []).unwrap();
        }
        path
    }

    #[test]
    fn maps_paths_audio_and_copies_utf8_before_releasing_every_handle() {
        let directory = fixture();
        let mut recognizer = OfflineRecognizer::new(FakeAdapter {
            text: "héllo".as_bytes().to_vec(),
            ..Default::default()
        });
        assert_eq!(
            recognizer.recognize(&directory, &[0.0, -0.5, 0.5]).unwrap(),
            "héllo"
        );
        let adapter = &recognizer.adapter;
        let config = adapter.config.as_ref().unwrap();
        let paths = &config.model;
        assert_eq!(config.provider, "cpu");
        assert_eq!(paths.encoder, directory.join("encoder.int8.onnx"));
        assert_eq!(paths.decoder, directory.join("decoder.int8.onnx"));
        assert_eq!(paths.joiner, directory.join("joiner.int8.onnx"));
        assert_eq!(paths.tokens, directory.join("tokens.txt"));
        assert_eq!(
            (adapter.sample_rate, adapter.sample_count),
            (Some(ASR_SAMPLE_RATE), Some(3))
        );
        assert_eq!(adapter.events, ["copy", "release-result"]);
        assert_eq!(
            adapter.releases,
            HashMap::from([("result", 1), ("stream", 1), ("recognizer", 1)])
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn accepts_an_empty_transcript() {
        let directory = fixture();
        let mut recognizer = OfflineRecognizer::new(FakeAdapter::default());
        assert_eq!(recognizer.recognize(&directory, &[]).unwrap(), "");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_invalid_input_before_native_creation() {
        let directory = fixture();
        fs::remove_file(directory.join("tokens.txt")).unwrap();
        let mut recognizer = OfflineRecognizer::new(FakeAdapter::default());
        assert_eq!(
            recognizer.recognize(&directory, &[0.0]),
            Err(OfflineRecognizerError::ModelArtifactMissing)
        );
        assert!(recognizer.adapter.config.is_none());
        fs::create_dir(directory.join("tokens.txt")).unwrap();
        assert_eq!(
            recognizer.recognize(&directory, &[0.0]),
            Err(OfflineRecognizerError::ModelArtifactNotFile)
        );
        assert_eq!(
            recognizer.recognize(&directory, &[f32::NAN]),
            Err(OfflineRecognizerError::InvalidAudio)
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn maps_native_failures_and_cleans_up_deterministically() {
        let directory = fixture();
        for (adapter, expected, releases) in [
            (
                FakeAdapter {
                    fail_stream: true,
                    ..Default::default()
                },
                OfflineRecognizerError::NativeCreationFailed,
                HashMap::from([("recognizer", 1)]),
            ),
            (
                FakeAdapter {
                    fail_create: true,
                    ..Default::default()
                },
                OfflineRecognizerError::NativeCreationFailed,
                HashMap::new(),
            ),
            (
                FakeAdapter {
                    fail_decode: true,
                    ..Default::default()
                },
                OfflineRecognizerError::NativeDecodeFailed,
                HashMap::from([("stream", 1), ("recognizer", 1)]),
            ),
            (
                FakeAdapter {
                    fail_result: true,
                    ..Default::default()
                },
                OfflineRecognizerError::NativeResultFailed,
                HashMap::from([("stream", 1), ("recognizer", 1)]),
            ),
        ] {
            let mut recognizer = OfflineRecognizer::new(adapter);
            assert_eq!(recognizer.recognize(&directory, &[0.0]), Err(expected));
            assert_eq!(recognizer.adapter.releases, releases);
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn releases_result_when_copy_or_utf8_validation_fails() {
        let directory = fixture();
        for (adapter, expected) in [
            (
                FakeAdapter {
                    fail_copy: true,
                    ..Default::default()
                },
                OfflineRecognizerError::NativeResultFailed,
            ),
            (
                FakeAdapter {
                    text: vec![0xff],
                    ..Default::default()
                },
                OfflineRecognizerError::InvalidTranscript,
            ),
        ] {
            let mut recognizer = OfflineRecognizer::new(adapter);
            assert_eq!(recognizer.recognize(&directory, &[0.0]), Err(expected));
            assert_eq!(
                recognizer.adapter.releases,
                HashMap::from([("result", 1), ("stream", 1), ("recognizer", 1)])
            );
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn errors_do_not_disclose_model_paths_or_audio() {
        let secret = "/private/models/customer-name";
        for error in [
            OfflineRecognizerError::InvalidAudio,
            OfflineRecognizerError::ModelArtifactMissing,
            OfflineRecognizerError::ModelArtifactNotFile,
            OfflineRecognizerError::NativeCreationFailed,
            OfflineRecognizerError::NativeDecodeFailed,
            OfflineRecognizerError::NativeResultFailed,
            OfflineRecognizerError::InvalidTranscript,
        ] {
            assert!(!format!("{error:?} {error}").contains(secret));
        }
    }
}
