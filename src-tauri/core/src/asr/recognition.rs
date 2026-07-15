//! Rust-owned, offline-only boundary around a native Parakeet recognizer.

use std::path::PathBuf;

use super::VerifiedParakeetRevision;

#[cfg(feature = "native-asr")]
use super::sherpa_ffi;
#[cfg(feature = "native-asr")]
use std::ffi::{CStr, CString};
#[cfg(feature = "native-asr")]
use std::path::Path;
#[cfg(feature = "native-asr")]
use std::sync::{Arc, LazyLock};

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
pub struct SherpaOnnxNative {
    api: Result<Arc<SherpaApi>, NativeAsrError>,
}

#[cfg(feature = "native-asr")]
struct SherpaApi {
    _library: libloading::Library,
    create_recognizer: sherpa_ffi::CreateRecognizer,
    destroy_recognizer: sherpa_ffi::DestroyRecognizer,
    create_stream: sherpa_ffi::CreateStream,
    destroy_stream: sherpa_ffi::DestroyStream,
    accept_waveform: sherpa_ffi::AcceptWaveform,
    decode: sherpa_ffi::Decode,
    get_result: sherpa_ffi::GetResult,
    destroy_result: sherpa_ffi::DestroyResult,
}

#[cfg(feature = "native-asr")]
pub struct SherpaRecognizer {
    api: Arc<SherpaApi>,
    ptr: *const sherpa_ffi::Recognizer,
}

#[cfg(feature = "native-asr")]
impl Drop for SherpaRecognizer {
    fn drop(&mut self) {
        // SAFETY: this pointer was returned by this API instance and is owned here.
        unsafe { (self.api.destroy_recognizer)(self.ptr) };
    }
}

#[cfg(feature = "native-asr")]
impl SherpaOnnxNative {
    fn packaged_runtime_directory() -> Result<PathBuf, NativeAsrError> {
        let executable = std::env::current_exe().map_err(|_| NativeAsrError::RuntimeUnavailable)?;
        let directory = executable
            .parent()
            .ok_or(NativeAsrError::RuntimeUnavailable)?;
        #[cfg(target_os = "linux")]
        return Ok(directory.join("../lib/muniment"));
        #[cfg(target_os = "macos")]
        return Ok(directory.join("../Resources"));
        #[cfg(target_os = "windows")]
        return Ok(directory.to_owned());
        #[allow(unreachable_code)]
        Err(NativeAsrError::RuntimeUnavailable)
    }

    fn packaged() -> Self {
        Self::load_from(&Self::packaged_runtime_directory().unwrap_or_else(|_| PathBuf::new()))
    }

    /// Loads and validates the packaged C API without allowing model paths to
    /// influence native library resolution.
    pub fn load_from(runtime_directory: &Path) -> Self {
        Self {
            api: load_api(runtime_directory),
        }
    }
}

#[cfg(feature = "native-asr")]
fn load_api(directory: &Path) -> Result<Arc<SherpaApi>, NativeAsrError> {
    #[cfg(target_os = "windows")]
    const LIBRARY: &str = "sherpa-onnx-c-api.dll";
    #[cfg(target_os = "macos")]
    const LIBRARY: &str = "libsherpa-onnx-c-api.dylib";
    #[cfg(target_os = "linux")]
    const LIBRARY: &str = "libsherpa-onnx-c-api.so";

    let path = directory.join(LIBRARY);
    if !path.is_file() {
        return Err(NativeAsrError::RuntimeUnavailable);
    }
    // SAFETY: every symbol is copied as a function pointer while `library` is
    // retained by SherpaApi for longer than any handle or call.
    unsafe {
        let library =
            libloading::Library::new(&path).map_err(|_| NativeAsrError::IncompatibleRuntime)?;
        macro_rules! symbol {
            ($name:literal, $ty:ty) => {
                *library
                    .get::<$ty>(concat!($name, "\0").as_bytes())
                    .map_err(|_| NativeAsrError::IncompatibleRuntime)?
            };
        }
        let version = symbol!("SherpaOnnxGetVersionStr", sherpa_ffi::GetString);
        let commit = symbol!("SherpaOnnxGetGitSha1", sherpa_ffi::GetString);
        let version = version();
        let commit = commit();
        if version.is_null()
            || commit.is_null()
            || CStr::from_ptr(version).to_bytes() != b"1.13.2"
            || CStr::from_ptr(commit).to_bytes() != b"13d0ae6c"
        {
            return Err(NativeAsrError::IncompatibleRuntime);
        }
        Ok(Arc::new(SherpaApi {
            create_recognizer: symbol!(
                "SherpaOnnxCreateOfflineRecognizer",
                sherpa_ffi::CreateRecognizer
            ),
            destroy_recognizer: symbol!(
                "SherpaOnnxDestroyOfflineRecognizer",
                sherpa_ffi::DestroyRecognizer
            ),
            create_stream: symbol!("SherpaOnnxCreateOfflineStream", sherpa_ffi::CreateStream),
            destroy_stream: symbol!("SherpaOnnxDestroyOfflineStream", sherpa_ffi::DestroyStream),
            accept_waveform: symbol!(
                "SherpaOnnxAcceptWaveformOffline",
                sherpa_ffi::AcceptWaveform
            ),
            decode: symbol!("SherpaOnnxDecodeOfflineStream", sherpa_ffi::Decode),
            get_result: symbol!(
                "SherpaOnnxGetOfflineStreamResultAsJson",
                sherpa_ffi::GetResult
            ),
            destroy_result: symbol!(
                "SherpaOnnxDestroyOfflineStreamResultJson",
                sherpa_ffi::DestroyResult
            ),
            _library: library,
        }))
    }
}

#[cfg(feature = "native-asr")]
impl OfflineAsrNative for SherpaOnnxNative {
    type Handle = SherpaRecognizer;

    fn construct(&self, paths: &ParakeetModelPaths) -> Result<Self::Handle, NativeAsrError> {
        fn path(path: &Path) -> Result<CString, NativeAsrError> {
            CString::new(path.to_string_lossy().as_bytes())
                .map_err(|_| NativeAsrError::ConstructionFailed)
        }
        let api = self.api.as_ref().map_err(|error| *error)?.clone();
        let encoder = path(&paths.encoder)?;
        let decoder = path(&paths.decoder)?;
        let joiner = path(&paths.joiner)?;
        let tokens = path(&paths.tokens)?;
        let provider = CString::new("cpu").expect("literal");
        let model_type = CString::new("nemo_transducer").expect("literal");
        let decoding = CString::new("greedy_search").expect("literal");
        let config = sherpa_ffi::RecognizerConfig {
            feat_config: sherpa_ffi::FeatureConfig {
                sample_rate: SAMPLE_RATE as i32,
                feature_dim: 80,
            },
            model_config: sherpa_ffi::ModelConfig {
                transducer: sherpa_ffi::Transducer {
                    encoder: encoder.as_ptr(),
                    decoder: decoder.as_ptr(),
                    joiner: joiner.as_ptr(),
                },
                tokens: tokens.as_ptr(),
                model_type: model_type.as_ptr(),
                provider: provider.as_ptr(),
                num_threads: 2,
                ..Default::default()
            },
            decoding_method: decoding.as_ptr(),
            max_active_paths: 4,
            ..Default::default()
        };
        // SAFETY: configuration strings remain alive for the duration of the call.
        let ptr = unsafe { (api.create_recognizer)(&config) };
        if ptr.is_null() {
            Err(NativeAsrError::ConstructionFailed)
        } else {
            Ok(SherpaRecognizer { api, ptr })
        }
    }

    fn decode(&self, handle: &mut Self::Handle, samples: &[f32]) -> Result<String, NativeAsrError> {
        // SAFETY: all pointers belong to the same retained API instance.
        unsafe {
            let stream = (handle.api.create_stream)(handle.ptr);
            if stream.is_null() {
                return Err(NativeAsrError::DecodeFailed);
            }
            struct StreamGuard<'a> {
                api: &'a SherpaApi,
                ptr: *const sherpa_ffi::Stream,
            }
            impl Drop for StreamGuard<'_> {
                fn drop(&mut self) {
                    unsafe { (self.api.destroy_stream)(self.ptr) }
                }
            }
            let stream = StreamGuard {
                api: &handle.api,
                ptr: stream,
            };
            let length = i32::try_from(samples.len()).map_err(|_| NativeAsrError::DecodeFailed)?;
            (handle.api.accept_waveform)(stream.ptr, SAMPLE_RATE as i32, samples.as_ptr(), length);
            (handle.api.decode)(handle.ptr, stream.ptr);
            let result = (handle.api.get_result)(stream.ptr);
            if result.is_null() {
                return Err(NativeAsrError::DecodeFailed);
            }
            struct ResultGuard<'a> {
                api: &'a SherpaApi,
                ptr: *const std::ffi::c_char,
            }
            impl Drop for ResultGuard<'_> {
                fn drop(&mut self) {
                    unsafe { (self.api.destroy_result)(self.ptr) }
                }
            }
            let result = ResultGuard {
                api: &handle.api,
                ptr: result,
            };
            let json = CStr::from_ptr(result.ptr)
                .to_str()
                .map_err(|_| NativeAsrError::DecodeFailed)?;
            serde_json::from_str::<serde_json::Value>(json)
                .ok()
                .and_then(|value| value.get("text")?.as_str().map(ToOwned::to_owned))
                .ok_or(NativeAsrError::DecodeFailed)
        }
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
        static NATIVE: LazyLock<SherpaOnnxNative> = LazyLock::new(SherpaOnnxNative::packaged);
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
    fn production_loader_reports_absent_and_incompatible_runtime() {
        let missing = SherpaOnnxNative::load_from(Path::new("definitely-not-an-asr-runtime"));
        let revision = verified_for_test(Path::new("private/model"));
        assert!(matches!(
            OfflineParakeetRecognizer::new(&missing, &revision),
            Err(AsrRecognitionError::RuntimeUnavailable)
        ));

        let temporary =
            std::env::temp_dir().join(format!("muniment-incompatible-{}", std::process::id()));
        std::fs::create_dir_all(&temporary).unwrap();
        let library = temporary.join(if cfg!(target_os = "windows") {
            "sherpa-onnx-c-api.dll"
        } else if cfg!(target_os = "macos") {
            "libsherpa-onnx-c-api.dylib"
        } else {
            "libsherpa-onnx-c-api.so"
        });
        std::fs::copy(std::env::current_exe().unwrap(), &library).unwrap();
        let incompatible = SherpaOnnxNative::load_from(&temporary);
        assert!(matches!(
            OfflineParakeetRecognizer::new(&incompatible, &revision),
            Err(AsrRecognitionError::IncompatibleRuntime)
        ));
        std::fs::remove_dir_all(temporary).unwrap();
    }

    #[cfg(feature = "native-asr")]
    #[test]
    fn staged_runtime_has_the_pinned_compatible_c_api() {
        let Some(directory) = std::env::var_os("SHERPA_ONNX_LIB_DIR") else {
            return;
        };
        let native = SherpaOnnxNative::load_from(Path::new(&directory));
        assert_eq!(native.api.as_ref().map(|_| ()), Ok(()));
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
