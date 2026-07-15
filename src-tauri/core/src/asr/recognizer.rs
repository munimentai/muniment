//! Narrow, utterance-final boundary around sherpa-onnx's unsafe C API wrapper.

use std::path::Path;

use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};

use super::{AsrLifecycleError, AsrRevisionLifecycle};

pub const SAMPLE_RATE: u32 = 16_000;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OfflineRecognitionError {
    Verification(AsrLifecycleError),
    InvalidSampleRate,
    NonFiniteSamples,
    SamplesNotNormalized,
    TooManySamples,
    NativeRecognizerUnavailable,
    NativeDecodeFailed,
}

impl std::fmt::Debug for OfflineRecognitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Verification(error) => f.debug_tuple("Verification").field(error).finish(),
            Self::InvalidSampleRate => f.write_str("InvalidSampleRate"),
            Self::NonFiniteSamples => f.write_str("NonFiniteSamples"),
            Self::SamplesNotNormalized => f.write_str("SamplesNotNormalized"),
            Self::TooManySamples => f.write_str("TooManySamples"),
            Self::NativeRecognizerUnavailable => f.write_str("NativeRecognizerUnavailable"),
            Self::NativeDecodeFailed => f.write_str("NativeDecodeFailed"),
        }
    }
}

impl std::fmt::Display for OfflineRecognitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Verification(_) => "the current ASR model failed verification",
            Self::InvalidSampleRate => "ASR input must be 16 kHz mono audio",
            Self::NonFiniteSamples => "ASR input contains an invalid sample",
            Self::SamplesNotNormalized => "ASR input samples must be normalized",
            Self::TooManySamples => "ASR input is too large",
            Self::NativeRecognizerUnavailable => "the offline recognizer could not be created",
            Self::NativeDecodeFailed => "offline recognition failed",
        };
        f.write_str(message)
    }
}

impl std::error::Error for OfflineRecognitionError {}

trait RecognizerBackend: Send + Sync {
    fn decode(&self, samples: &[f32]) -> Result<String, OfflineRecognitionError>;
}

struct SherpaBackend(OfflineRecognizer);

impl RecognizerBackend for SherpaBackend {
    fn decode(&self, samples: &[f32]) -> Result<String, OfflineRecognitionError> {
        // OfflineStream is an owning RAII handle in the pinned Rust C-API wrapper;
        // it is dropped on every return path, including a missing result.
        let stream = self.0.create_stream();
        stream.accept_waveform(SAMPLE_RATE as i32, samples);
        self.0.decode(&stream);
        stream
            .get_result()
            .map(|result| result.text)
            .ok_or(OfflineRecognitionError::NativeDecodeFailed)
    }
}

/// An offline recognizer bound to the lifecycle's verified current Parakeet revision.
///
/// Callers cannot provide model filenames or network endpoints. Construction first
/// resolves and re-verifies the pinned four-file manifest.
pub struct OfflineParakeetRecognizer {
    backend: Box<dyn RecognizerBackend>,
}

impl OfflineParakeetRecognizer {
    pub fn from_verified_current(
        lifecycle: &AsrRevisionLifecycle,
    ) -> Result<Self, OfflineRecognitionError> {
        let directory = lifecycle
            .resolve_current()
            .map_err(OfflineRecognitionError::Verification)?;
        let config = parakeet_config(&directory)?;
        let recognizer = OfflineRecognizer::create(&config)
            .ok_or(OfflineRecognitionError::NativeRecognizerUnavailable)?;
        Ok(Self {
            backend: Box::new(SherpaBackend(recognizer)),
        })
    }

    /// Decodes one complete mono utterance and returns its final transcript.
    pub fn decode(
        &self,
        sample_rate: u32,
        samples: &[f32],
    ) -> Result<String, OfflineRecognitionError> {
        if sample_rate != SAMPLE_RATE {
            return Err(OfflineRecognitionError::InvalidSampleRate);
        }
        if samples.len() > i32::MAX as usize {
            return Err(OfflineRecognitionError::TooManySamples);
        }
        if samples.iter().any(|sample| !sample.is_finite()) {
            return Err(OfflineRecognitionError::NonFiniteSamples);
        }
        if samples.iter().any(|sample| !(-1.0..=1.0).contains(sample)) {
            return Err(OfflineRecognitionError::SamplesNotNormalized);
        }
        self.backend.decode(samples)
    }
}

fn parakeet_config(directory: &Path) -> Result<OfflineRecognizerConfig, OfflineRecognitionError> {
    let path = |filename: &str| {
        directory
            .join(filename)
            .to_str()
            .map(str::to_owned)
            .ok_or(OfflineRecognitionError::NativeRecognizerUnavailable)
    };
    let mut config = OfflineRecognizerConfig::default();
    config.model_config.transducer = OfflineTransducerModelConfig {
        encoder: Some(path("encoder.int8.onnx")?),
        decoder: Some(path("decoder.int8.onnx")?),
        joiner: Some(path("joiner.int8.onnx")?),
    };
    config.model_config.tokens = Some(path("tokens.txt")?);
    config.model_config.provider = Some("cpu".into());
    config.model_config.model_type = Some("nemo_transducer".into());
    config.model_config.num_threads = 2;
    config.decoding_method = Some("greedy_search".into());
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asr::{AsrArtifactDescriptor, AsrArtifactManifest};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    struct FakeBackend {
        calls: Arc<AtomicUsize>,
        fail: bool,
    }

    impl RecognizerBackend for FakeBackend {
        fn decode(&self, _samples: &[f32]) -> Result<String, OfflineRecognitionError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if self.fail {
                Err(OfflineRecognitionError::NativeDecodeFailed)
            } else {
                Ok("fixture transcript".into())
            }
        }
    }

    #[test]
    fn maps_only_the_four_pinned_parakeet_paths_and_cpu_configuration() {
        let config = parakeet_config(Path::new("verified-revision")).unwrap();
        assert_eq!(
            config.model_config.transducer.encoder.as_deref(),
            Some("verified-revision/encoder.int8.onnx")
        );
        assert_eq!(
            config.model_config.transducer.decoder.as_deref(),
            Some("verified-revision/decoder.int8.onnx")
        );
        assert_eq!(
            config.model_config.transducer.joiner.as_deref(),
            Some("verified-revision/joiner.int8.onnx")
        );
        assert_eq!(
            config.model_config.tokens.as_deref(),
            Some("verified-revision/tokens.txt")
        );
        assert_eq!(config.model_config.provider.as_deref(), Some("cpu"));
        assert_eq!(
            config.model_config.model_type.as_deref(),
            Some("nemo_transducer")
        );
    }

    #[test]
    fn invalid_audio_is_rejected_before_native_code() {
        let calls = Arc::new(AtomicUsize::new(0));
        let recognizer = OfflineParakeetRecognizer {
            backend: Box::new(FakeBackend {
                calls: calls.clone(),
                fail: false,
            }),
        };
        assert_eq!(
            recognizer.decode(44_100, &[0.0]),
            Err(OfflineRecognitionError::InvalidSampleRate)
        );
        assert_eq!(
            recognizer.decode(SAMPLE_RATE, &[f32::NAN]),
            Err(OfflineRecognitionError::NonFiniteSamples)
        );
        assert_eq!(
            recognizer.decode(SAMPLE_RATE, &[1.01]),
            Err(OfflineRecognitionError::SamplesNotNormalized)
        );
        assert_eq!(calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn native_failure_is_typed_and_backend_resources_are_released() {
        let calls = Arc::new(AtomicUsize::new(0));
        let recognizer = OfflineParakeetRecognizer {
            backend: Box::new(FakeBackend {
                calls: calls.clone(),
                fail: true,
            }),
        };
        assert_eq!(
            recognizer.decode(SAMPLE_RATE, &[0.0]),
            Err(OfflineRecognitionError::NativeDecodeFailed)
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        drop(recognizer);
        assert_eq!(Arc::strong_count(&calls), 1);
    }

    #[test]
    fn deterministic_fixture_returns_one_final_transcript() {
        let calls = Arc::new(AtomicUsize::new(0));
        let recognizer = OfflineParakeetRecognizer {
            backend: Box::new(FakeBackend { calls, fail: false }),
        };
        assert_eq!(
            recognizer.decode(SAMPLE_RATE, &[0.0; 160]).unwrap(),
            "fixture transcript"
        );
    }

    #[test]
    fn pinned_native_runtime_smoke_rejects_an_empty_configuration() {
        // Exercises the actual dynamically linked v1.13.2 C boundary without
        // downloading a model fixture. A missing model family is deterministic.
        assert!(OfflineRecognizer::create(&OfflineRecognizerConfig::default()).is_none());
    }

    #[test]
    fn construction_refuses_an_unverified_current_revision() {
        static FILES: [AsrArtifactDescriptor; 4] = [
            AsrArtifactDescriptor {
                filename: "encoder.int8.onnx",
                byte_size: 1,
                sha256: "00",
            },
            AsrArtifactDescriptor {
                filename: "decoder.int8.onnx",
                byte_size: 1,
                sha256: "00",
            },
            AsrArtifactDescriptor {
                filename: "joiner.int8.onnx",
                byte_size: 1,
                sha256: "00",
            },
            AsrArtifactDescriptor {
                filename: "tokens.txt",
                byte_size: 1,
                sha256: "00",
            },
        ];
        static MANIFEST: AsrArtifactManifest = AsrArtifactManifest {
            identity: "fixture",
            revision: "bad",
            artifacts: &FILES,
        };
        static MANIFESTS: [&AsrArtifactManifest; 1] = [&MANIFEST];
        let root = std::env::temp_dir().join(format!(
            "muniment-recognizer-refusal-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("revisions/bad")).unwrap();
        std::fs::write(
            root.join("current"),
            "muniment-asr-pointer-v1\nfixture\nbad\n",
        )
        .unwrap();
        let lifecycle = AsrRevisionLifecycle::new(root.clone(), &MANIFESTS, &MANIFEST).unwrap();

        assert!(matches!(
            OfflineParakeetRecognizer::from_verified_current(&lifecycle),
            Err(OfflineRecognitionError::Verification(_))
        ));
        std::fs::remove_dir_all(root).unwrap();
    }
}
