//! Rust-owned, offline-only boundary around a native Parakeet recognizer.

use std::path::PathBuf;

use super::VerifiedParakeetRevision;

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

    struct FakeNative {
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
            Ok(())
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
        ] {
            let error = AsrRecognitionError::from(native_error);
            assert!(!format!("{error:?} {error}").contains(secret));
        }
    }
}
