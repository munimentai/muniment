//! Rust-owned, offline-only boundary around a native Parakeet recognizer.

use std::path::PathBuf;

use super::VerifiedParakeetRevision;

#[cfg(feature = "native-asr")]
use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};

pub const SAMPLE_RATE: u32 = 16_000;

#[derive(Clone, PartialEq, Eq)]
pub struct ParakeetModelPaths {
    pub encoder: PathBuf,
    pub decoder: PathBuf,
    pub joiner: PathBuf,
    pub tokens: PathBuf,
}

impl ParakeetModelPaths {
    fn from_verified(revision: &VerifiedParakeetRevision) -> Self {
        let root = revision.path();
        Self {
            encoder: root.join("encoder.int8.onnx"),
            decoder: root.join("decoder.int8.onnx"),
            joiner: root.join("joiner.int8.onnx"),
            tokens: root.join("tokens.txt"),
        }
    }
}

/// The in-process native C-API surface. Implementations must not retain the
/// model path references or sample slice passed to them.
pub trait OfflineAsrNative {
    type Handle;

    fn construct(&self, paths: &ParakeetModelPaths) -> Result<Self::Handle, NativeAsrError>;
    fn decode(&self, handle: &mut Self::Handle, samples: &[f32]) -> Result<String, NativeAsrError>;
    fn destroy(&self, handle: Self::Handle);
}

/// Pinned, in-process sherpa-onnx v1.13.2 C-API adapter used by desktop builds.
/// The wrapper owns the C recognizer; each decode owns a C stream and result.
#[cfg(feature = "native-asr")]
pub struct SherpaOnnxNative;

#[cfg(feature = "native-asr")]
impl OfflineAsrNative for SherpaOnnxNative {
    type Handle = OfflineRecognizer;

    fn construct(&self, paths: &ParakeetModelPaths) -> Result<Self::Handle, NativeAsrError> {
        fn path(path: &std::path::Path) -> Result<String, NativeAsrError> {
            path.to_str()
                .map(ToOwned::to_owned)
                .ok_or(NativeAsrError::ConstructionFailed)
        }

        let mut config = OfflineRecognizerConfig::default();
        config.model_config.transducer = OfflineTransducerModelConfig {
            encoder: Some(path(&paths.encoder)?),
            decoder: Some(path(&paths.decoder)?),
            joiner: Some(path(&paths.joiner)?),
        };
        config.model_config.tokens = Some(path(&paths.tokens)?);
        config.model_config.model_type = Some("nemo_transducer".into());
        config.model_config.provider = Some("cpu".into());
        config.model_config.num_threads = 2;
        config.decoding_method = Some("greedy_search".into());
        OfflineRecognizer::create(&config).ok_or(NativeAsrError::ConstructionFailed)
    }

    fn decode(&self, handle: &mut Self::Handle, samples: &[f32]) -> Result<String, NativeAsrError> {
        let stream = handle.create_stream();
        stream.accept_waveform(SAMPLE_RATE as i32, samples);
        handle.decode(&stream);
        stream
            .get_result()
            .map(|result| result.text)
            .ok_or(NativeAsrError::DecodeFailed)
    }

    fn destroy(&self, handle: Self::Handle) {
        drop(handle);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeAsrError {
    RuntimeUnavailable,
    IncompatibleRuntime,
    ConstructionFailed,
    DecodeFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsrRecognitionError {
    InvalidSamples,
    RuntimeUnavailable,
    IncompatibleRuntime,
    ConstructionFailed,
    DecodeFailed,
}

impl std::fmt::Display for AsrRecognitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidSamples => "ASR input must be non-empty normalized mono 16 kHz samples",
            Self::RuntimeUnavailable => "the offline ASR runtime is unavailable",
            Self::IncompatibleRuntime => "the offline ASR runtime is incompatible",
            Self::ConstructionFailed => "the offline ASR recognizer could not be constructed",
            Self::DecodeFailed => "offline speech recognition failed",
        };
        f.write_str(message)
    }
}

impl std::error::Error for AsrRecognitionError {}

impl From<NativeAsrError> for AsrRecognitionError {
    fn from(value: NativeAsrError) -> Self {
        match value {
            NativeAsrError::RuntimeUnavailable => Self::RuntimeUnavailable,
            NativeAsrError::IncompatibleRuntime => Self::IncompatibleRuntime,
            NativeAsrError::ConstructionFailed => Self::ConstructionFailed,
            NativeAsrError::DecodeFailed => Self::DecodeFailed,
        }
    }
}

/// Owns exactly one native recognizer and destroys it when dropped.
pub struct OfflineParakeetRecognizer<'a, N: OfflineAsrNative> {
    native: &'a N,
    handle: Option<N::Handle>,
}

impl<'a, N: OfflineAsrNative> OfflineParakeetRecognizer<'a, N> {
    pub fn new(
        native: &'a N,
        revision: &VerifiedParakeetRevision,
    ) -> Result<Self, AsrRecognitionError> {
        let paths = ParakeetModelPaths::from_verified(revision);
        let handle = native.construct(&paths)?;
        Ok(Self {
            native,
            handle: Some(handle),
        })
    }

    /// Decodes one complete mono, 16 kHz utterance. The type deliberately has
    /// no networking, process, listener, or IPC capability.
    pub fn decode_final(&mut self, samples: &[f32]) -> Result<String, AsrRecognitionError> {
        if samples.is_empty()
            || samples
                .iter()
                .any(|sample| !sample.is_finite() || !(-1.0..=1.0).contains(sample))
        {
            return Err(AsrRecognitionError::InvalidSamples);
        }
        self.native
            .decode(self.handle.as_mut().expect("recognizer handle"), samples)
            .map_err(Into::into)
    }
}

#[cfg(feature = "native-asr")]
impl OfflineParakeetRecognizer<'static, SherpaOnnxNative> {
    /// Production construction entry point. Only the lifecycle-issued verified
    /// revision can supply model paths; no webview path crosses this boundary.
    pub fn from_verified_revision(
        revision: &VerifiedParakeetRevision,
    ) -> Result<Self, AsrRecognitionError> {
        // The adapter is stateless and lives for the process lifetime.
        static NATIVE: SherpaOnnxNative = SherpaOnnxNative;
        Self::new(&NATIVE, revision)
    }
}

impl<N: OfflineAsrNative> Drop for OfflineParakeetRecognizer<'_, N> {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            self.native.destroy(handle);
        }
    }
}

#[cfg(test)]
pub(crate) fn verified_for_test(path: &std::path::Path) -> VerifiedParakeetRevision {
    VerifiedParakeetRevision(path.to_owned())
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::path::Path;

    use super::*;

    #[cfg(feature = "native-asr")]
    #[test]
    fn loads_the_pinned_native_runtime() {
        use std::ffi::CStr;

        // SAFETY: these pinned C-API functions return process-lifetime strings.
        let version = unsafe { CStr::from_ptr(sherpa_onnx_sys::SherpaOnnxGetVersionStr()) };
        let commit = unsafe { CStr::from_ptr(sherpa_onnx_sys::SherpaOnnxGetGitSha1()) };
        assert_eq!(version.to_bytes(), b"1.13.2");
        assert_eq!(commit.to_bytes(), b"13d0ae6c");
    }

    struct FakeNative {
        construction: Result<(), NativeAsrError>,
        result: RefCell<Result<String, NativeAsrError>>,
        destroyed: Cell<bool>,
    }

    impl OfflineAsrNative for FakeNative {
        type Handle = ();

        fn construct(&self, paths: &ParakeetModelPaths) -> Result<Self::Handle, NativeAsrError> {
            assert!(paths.encoder.ends_with("encoder.int8.onnx"));
            assert!(paths.decoder.ends_with("decoder.int8.onnx"));
            assert!(paths.joiner.ends_with("joiner.int8.onnx"));
            assert!(paths.tokens.ends_with("tokens.txt"));
            self.construction
        }

        fn decode(&self, _: &mut (), _: &[f32]) -> Result<String, NativeAsrError> {
            self.result.borrow().clone()
        }

        fn destroy(&self, _: ()) {
            self.destroyed.set(true);
        }
    }

    #[test]
    fn constructs_decodes_and_destroys_deterministically() {
        let native = FakeNative {
            construction: Ok(()),
            result: RefCell::new(Ok("offline words".into())),
            destroyed: Cell::new(false),
        };
        {
            let revision = verified_for_test(Path::new("/private/model/revision"));
            let mut recognizer = OfflineParakeetRecognizer::new(&native, &revision).unwrap();
            assert_eq!(
                recognizer.decode_final(&[0.0, -1.0, 1.0]).unwrap(),
                "offline words"
            );
        }
        assert!(native.destroyed.get());
    }

    #[test]
    fn rejects_malformed_samples_before_native_decode() {
        let native = FakeNative {
            construction: Ok(()),
            result: RefCell::new(Ok(String::new())),
            destroyed: Cell::new(false),
        };
        let revision = verified_for_test(Path::new("model"));
        let mut recognizer = OfflineParakeetRecognizer::new(&native, &revision).unwrap();
        for samples in [&[][..], &[1.01][..], &[f32::NAN][..]] {
            assert_eq!(
                recognizer.decode_final(samples),
                Err(AsrRecognitionError::InvalidSamples)
            );
        }
    }

    #[test]
    fn native_failures_are_typed_and_paths_are_redacted() {
        let native = FakeNative {
            construction: Ok(()),
            result: RefCell::new(Err(NativeAsrError::DecodeFailed)),
            destroyed: Cell::new(false),
        };
        let secret = "/Users/person/private/parakeet";
        let revision = verified_for_test(Path::new(secret));
        let mut recognizer = OfflineParakeetRecognizer::new(&native, &revision).unwrap();
        let error = recognizer.decode_final(&[0.0]).unwrap_err();
        assert_eq!(error, AsrRecognitionError::DecodeFailed);
        assert!(!format!("{error:?} {error}").contains(secret));

        for native_error in [
            NativeAsrError::RuntimeUnavailable,
            NativeAsrError::IncompatibleRuntime,
            NativeAsrError::ConstructionFailed,
        ] {
            let native = FakeNative {
                construction: Err(native_error),
                result: RefCell::new(Ok(String::new())),
                destroyed: Cell::new(false),
            };
            let error = match OfflineParakeetRecognizer::new(&native, &revision) {
                Ok(_) => panic!("construction unexpectedly succeeded"),
                Err(error) => error,
            };
            assert!(!native.destroyed.get());
            assert!(!format!("{error:?} {error}").contains(secret));
        }
    }
}
