//! Safe ownership boundary for offline Parakeet recognition.

use std::path::{Path, PathBuf};

use super::PARAKEET_ARTIFACTS;

pub const ASR_SAMPLE_RATE: u32 = 16_000;

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
        f.write_str(match self {
            Self::InvalidAudio => "ASR audio is invalid",
            Self::ModelArtifactMissing => "ASR model set is incomplete",
            Self::ModelArtifactNotFile => "ASR model set contains a non-file artifact",
            Self::NativeCreationFailed => "ASR recognizer could not be created",
            Self::NativeDecodeFailed => "ASR audio could not be decoded",
            Self::NativeResultFailed => "ASR result could not be read",
            Self::InvalidTranscript => "ASR returned invalid transcript text",
        })
    }
}

impl std::error::Error for OfflineRecognizerError {}

/// An offline recognizer whose implementation owns all native resources.
/// Native construction is added with the packaged v1.13.2 libraries; the
/// crate-private constructor keeps that dependency injectable in unit tests.
pub struct OfflineRecognizer {
    adapter: Box<dyn native_adapter::Adapter>,
}

impl OfflineRecognizer {
    #[cfg(test)]
    fn new(adapter: impl native_adapter::Adapter + 'static) -> Self {
        Self {
            adapter: Box::new(adapter),
        }
    }

    /// Decodes finite normalized mono 16 kHz PCM using a verified-current
    /// Parakeet model directory.
    pub fn recognize(
        &mut self,
        model_directory: &Path,
        samples: &[f32],
    ) -> Result<String, OfflineRecognizerError> {
        if samples.iter().any(|sample| !sample.is_finite()) {
            return Err(OfflineRecognizerError::InvalidAudio);
        }
        let paths = model_paths(model_directory)?;
        self.adapter.recognize(&paths, samples)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParakeetModelPaths {
    encoder: PathBuf,
    decoder: PathBuf,
    joiner: PathBuf,
    tokens: PathBuf,
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

/// The only module allowed to translate sherpa-onnx C API values. The seam is
/// shaped like v1.13.2 but uses opaque identifiers so tests neither link the
/// library nor contain raw pointers. A later linking slice implements `Capi`
/// with extern calls; its raw pointers and unsafe blocks stay in this module.
#[allow(dead_code)]
mod native_adapter {
    use super::{OfflineRecognizerError, ParakeetModelPaths, ASR_SAMPLE_RATE};

    #[derive(Clone, Copy)]
    pub(super) struct Handle(pub(super) usize);

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) struct TransducerConfig {
        pub encoder: String,
        pub decoder: String,
        pub joiner: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) struct RecognizerConfig {
        pub transducer: TransducerConfig,
        pub tokens: String,
        pub provider: &'static str,
        pub num_threads: i32,
        pub debug: i32,
    }

    pub(super) trait Capi {
        fn create_recognizer(&mut self, config: &RecognizerConfig) -> Option<Handle>;
        fn destroy_recognizer(&mut self, recognizer: Handle);
        fn create_stream(&mut self, recognizer: Handle) -> Option<Handle>;
        fn destroy_stream(&mut self, stream: Handle);
        fn accept_waveform(&mut self, stream: Handle, sample_rate: i32, samples: &[f32]);
        fn decode(&mut self, recognizer: Handle, stream: Handle) -> bool;
        fn get_result(&mut self, recognizer: Handle, stream: Handle) -> Option<Handle>;
        /// Copies the native NUL-terminated result text while its handle lives.
        fn copy_result_text(&mut self, result: Handle) -> Option<Vec<u8>>;
        fn destroy_result(&mut self, result: Handle);
    }

    pub(super) trait Adapter {
        fn recognize(
            &mut self,
            paths: &ParakeetModelPaths,
            samples: &[f32],
        ) -> Result<String, OfflineRecognizerError>;
    }

    pub(super) struct SherpaAdapter<C> {
        pub(super) capi: C,
    }

    impl<C> SherpaAdapter<C> {
        pub(super) fn new(capi: C) -> Self {
            Self { capi }
        }
    }

    impl<C: Capi> Adapter for SherpaAdapter<C> {
        fn recognize(
            &mut self,
            paths: &ParakeetModelPaths,
            samples: &[f32],
        ) -> Result<String, OfflineRecognizerError> {
            let config = RecognizerConfig {
                transducer: TransducerConfig {
                    encoder: path_text(&paths.encoder)?,
                    decoder: path_text(&paths.decoder)?,
                    joiner: path_text(&paths.joiner)?,
                },
                tokens: path_text(&paths.tokens)?,
                provider: "cpu",
                num_threads: 1,
                debug: 0,
            };
            let recognizer = self
                .capi
                .create_recognizer(&config)
                .ok_or(OfflineRecognizerError::NativeCreationFailed)?;
            let stream = match self.capi.create_stream(recognizer) {
                Some(stream) => stream,
                None => {
                    self.capi.destroy_recognizer(recognizer);
                    return Err(OfflineRecognizerError::NativeCreationFailed);
                }
            };
            self.capi
                .accept_waveform(stream, ASR_SAMPLE_RATE as i32, samples);
            let outcome = self.decode_and_copy(recognizer, stream);
            self.capi.destroy_stream(stream);
            self.capi.destroy_recognizer(recognizer);
            outcome
        }
    }

    impl<C: Capi> SherpaAdapter<C> {
        fn decode_and_copy(
            &mut self,
            recognizer: Handle,
            stream: Handle,
        ) -> Result<String, OfflineRecognizerError> {
            if !self.capi.decode(recognizer, stream) {
                return Err(OfflineRecognizerError::NativeDecodeFailed);
            }
            let result = self
                .capi
                .get_result(recognizer, stream)
                .ok_or(OfflineRecognizerError::NativeResultFailed)?;
            let bytes = self.capi.copy_result_text(result);
            self.capi.destroy_result(result);
            let bytes = bytes.ok_or(OfflineRecognizerError::NativeResultFailed)?;
            String::from_utf8(bytes).map_err(|_| OfflineRecognizerError::InvalidTranscript)
        }
    }

    fn path_text(path: &std::path::Path) -> Result<String, OfflineRecognizerError> {
        path.to_str()
            .map(str::to_owned)
            .ok_or(OfflineRecognizerError::NativeCreationFailed)
    }
}

#[cfg(test)]
mod tests {
    use super::native_adapter::{Capi, Handle, RecognizerConfig, SherpaAdapter};
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::fs;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    #[derive(Default)]
    struct FakeCapi {
        config: Option<RecognizerConfig>,
        sample_rate: Option<i32>,
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

    impl Capi for FakeCapi {
        fn create_recognizer(&mut self, config: &RecognizerConfig) -> Option<Handle> {
            self.config = Some(config.clone());
            (!self.fail_create).then_some(Handle(1))
        }
        fn destroy_recognizer(&mut self, _: Handle) {
            *self.releases.entry("recognizer").or_default() += 1;
        }
        fn create_stream(&mut self, _: Handle) -> Option<Handle> {
            (!self.fail_stream).then_some(Handle(2))
        }
        fn destroy_stream(&mut self, _: Handle) {
            *self.releases.entry("stream").or_default() += 1;
        }
        fn accept_waveform(&mut self, _: Handle, rate: i32, samples: &[f32]) {
            self.sample_rate = Some(rate);
            self.sample_count = Some(samples.len());
        }
        fn decode(&mut self, _: Handle, _: Handle) -> bool {
            !self.fail_decode
        }
        fn get_result(&mut self, _: Handle, _: Handle) -> Option<Handle> {
            (!self.fail_result).then_some(Handle(3))
        }
        fn copy_result_text(&mut self, _: Handle) -> Option<Vec<u8>> {
            if self.fail_copy {
                None
            } else {
                self.events.push("copy");
                Some(self.text.clone())
            }
        }
        fn destroy_result(&mut self, _: Handle) {
            self.events.push("release-result");
            *self.releases.entry("result").or_default() += 1;
        }
    }

    impl Capi for Rc<RefCell<FakeCapi>> {
        fn create_recognizer(&mut self, config: &RecognizerConfig) -> Option<Handle> {
            self.borrow_mut().create_recognizer(config)
        }
        fn destroy_recognizer(&mut self, handle: Handle) {
            self.borrow_mut().destroy_recognizer(handle)
        }
        fn create_stream(&mut self, handle: Handle) -> Option<Handle> {
            self.borrow_mut().create_stream(handle)
        }
        fn destroy_stream(&mut self, handle: Handle) {
            self.borrow_mut().destroy_stream(handle)
        }
        fn accept_waveform(&mut self, handle: Handle, rate: i32, samples: &[f32]) {
            self.borrow_mut().accept_waveform(handle, rate, samples)
        }
        fn decode(&mut self, recognizer: Handle, stream: Handle) -> bool {
            self.borrow_mut().decode(recognizer, stream)
        }
        fn get_result(&mut self, recognizer: Handle, stream: Handle) -> Option<Handle> {
            self.borrow_mut().get_result(recognizer, stream)
        }
        fn copy_result_text(&mut self, result: Handle) -> Option<Vec<u8>> {
            self.borrow_mut().copy_result_text(result)
        }
        fn destroy_result(&mut self, result: Handle) {
            self.borrow_mut().destroy_result(result)
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

    fn recognizer(capi: FakeCapi) -> (OfflineRecognizer, Rc<RefCell<FakeCapi>>) {
        let capi = Rc::new(RefCell::new(capi));
        (
            OfflineRecognizer::new(SherpaAdapter::new(capi.clone())),
            capi,
        )
    }

    #[test]
    fn maps_c_configuration_audio_and_copies_before_release() {
        let directory = fixture();
        let (mut recognizer, capi) = recognizer(FakeCapi {
            text: "héllo".as_bytes().to_vec(),
            ..Default::default()
        });
        assert_eq!(
            recognizer.recognize(&directory, &[0.0, -0.5, 0.5]),
            Ok("héllo".into())
        );
        let capi = capi.borrow();
        let config = capi.config.as_ref().unwrap();
        assert_eq!(config.provider, "cpu");
        assert_eq!(config.num_threads, 1);
        assert_eq!(config.debug, 0);
        assert_eq!(
            config.transducer.encoder,
            directory.join("encoder.int8.onnx").to_str().unwrap()
        );
        assert_eq!(
            config.transducer.decoder,
            directory.join("decoder.int8.onnx").to_str().unwrap()
        );
        assert_eq!(
            config.transducer.joiner,
            directory.join("joiner.int8.onnx").to_str().unwrap()
        );
        assert_eq!(
            config.tokens,
            directory.join("tokens.txt").to_str().unwrap()
        );
        assert_eq!(
            (capi.sample_rate, capi.sample_count),
            (Some(16_000), Some(3))
        );
        assert_eq!(capi.events, ["copy", "release-result"]);
        assert_eq!(
            capi.releases,
            HashMap::from([("result", 1), ("stream", 1), ("recognizer", 1)])
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn accepts_empty_transcript() {
        let directory = fixture();
        assert_eq!(
            recognizer(FakeCapi::default()).0.recognize(&directory, &[]),
            Ok(String::new())
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_audio_and_paths_before_c_api() {
        let directory = fixture();
        fs::remove_file(directory.join("tokens.txt")).unwrap();
        let (mut r, capi) = recognizer(FakeCapi::default());
        assert_eq!(
            r.recognize(&directory, &[0.0]),
            Err(OfflineRecognizerError::ModelArtifactMissing)
        );
        assert!(capi.borrow().config.is_none());
        fs::create_dir(directory.join("tokens.txt")).unwrap();
        assert_eq!(
            r.recognize(&directory, &[0.0]),
            Err(OfflineRecognizerError::ModelArtifactNotFile)
        );
        assert_eq!(
            r.recognize(&directory, &[f32::NAN]),
            Err(OfflineRecognizerError::InvalidAudio)
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn adapter_maps_native_failures_and_cleans_up_once() {
        let directory = fixture();
        let cases = [
            (
                FakeCapi {
                    fail_create: true,
                    ..Default::default()
                },
                OfflineRecognizerError::NativeCreationFailed,
                HashMap::new(),
            ),
            (
                FakeCapi {
                    fail_stream: true,
                    ..Default::default()
                },
                OfflineRecognizerError::NativeCreationFailed,
                HashMap::from([("recognizer", 1)]),
            ),
            (
                FakeCapi {
                    fail_decode: true,
                    ..Default::default()
                },
                OfflineRecognizerError::NativeDecodeFailed,
                HashMap::from([("stream", 1), ("recognizer", 1)]),
            ),
            (
                FakeCapi {
                    fail_result: true,
                    ..Default::default()
                },
                OfflineRecognizerError::NativeResultFailed,
                HashMap::from([("stream", 1), ("recognizer", 1)]),
            ),
        ];
        for (capi, error, releases) in cases {
            let (mut r, capi) = recognizer(capi);
            assert_eq!(r.recognize(&directory, &[0.0]), Err(error));
            assert_eq!(capi.borrow().releases, releases);
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn result_is_released_on_copy_and_utf8_failures() {
        let directory = fixture();
        for (capi, error) in [
            (
                FakeCapi {
                    fail_copy: true,
                    ..Default::default()
                },
                OfflineRecognizerError::NativeResultFailed,
            ),
            (
                FakeCapi {
                    text: vec![0xff],
                    ..Default::default()
                },
                OfflineRecognizerError::InvalidTranscript,
            ),
        ] {
            let (mut r, capi) = recognizer(capi);
            assert_eq!(r.recognize(&directory, &[0.0]), Err(error));
            assert_eq!(
                capi.borrow().releases,
                HashMap::from([("result", 1), ("stream", 1), ("recognizer", 1)])
            );
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn errors_are_redaction_safe() {
        for error in [
            OfflineRecognizerError::InvalidAudio,
            OfflineRecognizerError::ModelArtifactMissing,
            OfflineRecognizerError::ModelArtifactNotFile,
            OfflineRecognizerError::NativeCreationFailed,
            OfflineRecognizerError::NativeDecodeFailed,
            OfflineRecognizerError::NativeResultFailed,
            OfflineRecognizerError::InvalidTranscript,
        ] {
            assert!(!format!("{error:?} {error}").contains("/private/models/customer-name"));
        }
    }
}
