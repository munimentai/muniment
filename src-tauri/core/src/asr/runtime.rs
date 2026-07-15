//! Safe ownership boundary for offline Parakeet recognition.

use std::path::{Path, PathBuf};

use super::{VerifiedParakeetModelSet, PARAKEET_ARTIFACTS};

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
    /// Constructs a recognizer from a safe implementation of the pinned
    /// sherpa-onnx C API. Raw FFI values remain an implementation detail of
    /// the implementation and the private adapter.
    pub fn new(capi: impl SherpaOfflineApi + 'static) -> Self {
        Self {
            adapter: Box::new(native_adapter::SherpaAdapter::new(capi)),
        }
    }

    /// Decodes finite normalized mono 16 kHz PCM using a verified-current
    /// Parakeet model directory.
    pub fn recognize(
        &mut self,
        model_set: &VerifiedParakeetModelSet,
        samples: &[f32],
    ) -> Result<String, OfflineRecognizerError> {
        if samples.iter().any(|sample| !sample.is_finite()) {
            return Err(OfflineRecognizerError::InvalidAudio);
        }
        let paths = model_paths(model_set.directory())?;
        self.adapter.recognize(&paths, samples)
    }
}

/// Safe injection seam for the pinned sherpa-onnx v1.13.2 offline API.
/// Implementations may own native handles, but no pointer crosses this trait.
pub trait SherpaOfflineApi {
    type Recognizer: Copy;
    type Stream: Copy;
    type Result: Copy;

    fn create_recognizer(&mut self, config: &SherpaRecognizerConfig) -> Option<Self::Recognizer>;
    fn destroy_recognizer(&mut self, recognizer: Self::Recognizer);
    fn create_stream(&mut self, recognizer: Self::Recognizer) -> Option<Self::Stream>;
    fn destroy_stream(&mut self, stream: Self::Stream);
    fn accept_waveform(&mut self, stream: Self::Stream, sample_rate: i32, samples: &[f32]);
    fn decode(&mut self, recognizer: Self::Recognizer, stream: Self::Stream) -> bool;
    fn get_result(
        &mut self,
        recognizer: Self::Recognizer,
        stream: Self::Stream,
    ) -> Option<Self::Result>;
    /// Exposes result bytes only for the duration of `copy`.
    fn read_result_text(&mut self, result: Self::Result, copy: &mut dyn FnMut(&[u8])) -> bool;
    fn destroy_result(&mut self, result: Self::Result);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SherpaTransducerConfig {
    pub encoder: String,
    pub decoder: String,
    pub joiner: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SherpaRecognizerConfig {
    pub transducer: SherpaTransducerConfig,
    pub tokens: String,
    pub provider: &'static str,
    pub num_threads: i32,
    pub debug: i32,
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
    use super::{
        OfflineRecognizerError, ParakeetModelPaths, SherpaOfflineApi, SherpaRecognizerConfig,
        SherpaTransducerConfig, ASR_SAMPLE_RATE,
    };

    pub(super) trait Adapter {
        fn recognize(
            &mut self,
            paths: &ParakeetModelPaths,
            samples: &[f32],
        ) -> Result<String, OfflineRecognizerError>;
    }

    pub(super) struct SherpaAdapter<C> {
        capi: C,
    }

    impl<C> SherpaAdapter<C> {
        pub(super) fn new(capi: C) -> Self {
            Self { capi }
        }
    }

    impl<C: SherpaOfflineApi> Adapter for SherpaAdapter<C> {
        fn recognize(
            &mut self,
            paths: &ParakeetModelPaths,
            samples: &[f32],
        ) -> Result<String, OfflineRecognizerError> {
            let config = SherpaRecognizerConfig {
                transducer: SherpaTransducerConfig {
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

    impl<C: SherpaOfflineApi> SherpaAdapter<C> {
        fn decode_and_copy(
            &mut self,
            recognizer: C::Recognizer,
            stream: C::Stream,
        ) -> Result<String, OfflineRecognizerError> {
            if !self.capi.decode(recognizer, stream) {
                return Err(OfflineRecognizerError::NativeDecodeFailed);
            }
            let result = self
                .capi
                .get_result(recognizer, stream)
                .ok_or(OfflineRecognizerError::NativeResultFailed)?;
            let mut bytes = Vec::new();
            let copied = self
                .capi
                .read_result_text(result, &mut |text| bytes.extend_from_slice(text));
            self.capi.destroy_result(result);
            if !copied {
                return Err(OfflineRecognizerError::NativeResultFailed);
            }
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
    use super::*;
    use crate::asr::{
        AsrLifecycleError, AsrModelSetVerificationError, AsrRevisionLifecycle,
        PARAKEET_MODEL_MANIFEST, PARAKEET_MODEL_MANIFESTS,
    };
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::fs;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    #[derive(Default)]
    struct FakeCapi {
        config: Option<SherpaRecognizerConfig>,
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

    impl SherpaOfflineApi for FakeCapi {
        type Recognizer = u8;
        type Stream = u8;
        type Result = u8;

        fn create_recognizer(&mut self, config: &SherpaRecognizerConfig) -> Option<u8> {
            self.config = Some(config.clone());
            (!self.fail_create).then_some(1)
        }
        fn destroy_recognizer(&mut self, _: u8) {
            *self.releases.entry("recognizer").or_default() += 1;
        }
        fn create_stream(&mut self, _: u8) -> Option<u8> {
            (!self.fail_stream).then_some(2)
        }
        fn destroy_stream(&mut self, _: u8) {
            *self.releases.entry("stream").or_default() += 1;
        }
        fn accept_waveform(&mut self, _: u8, rate: i32, samples: &[f32]) {
            self.sample_rate = Some(rate);
            self.sample_count = Some(samples.len());
        }
        fn decode(&mut self, _: u8, _: u8) -> bool {
            !self.fail_decode
        }
        fn get_result(&mut self, _: u8, _: u8) -> Option<u8> {
            (!self.fail_result).then_some(3)
        }
        fn read_result_text(&mut self, _: u8, copy: &mut dyn FnMut(&[u8])) -> bool {
            if self.fail_copy {
                false
            } else {
                self.events.push("copy");
                copy(&self.text);
                true
            }
        }
        fn destroy_result(&mut self, _: u8) {
            self.events.push("release-result");
            *self.releases.entry("result").or_default() += 1;
        }
    }

    impl SherpaOfflineApi for Rc<RefCell<FakeCapi>> {
        type Recognizer = u8;
        type Stream = u8;
        type Result = u8;

        fn create_recognizer(&mut self, config: &SherpaRecognizerConfig) -> Option<u8> {
            self.borrow_mut().create_recognizer(config)
        }
        fn destroy_recognizer(&mut self, handle: u8) {
            self.borrow_mut().destroy_recognizer(handle)
        }
        fn create_stream(&mut self, handle: u8) -> Option<u8> {
            self.borrow_mut().create_stream(handle)
        }
        fn destroy_stream(&mut self, handle: u8) {
            self.borrow_mut().destroy_stream(handle)
        }
        fn accept_waveform(&mut self, handle: u8, rate: i32, samples: &[f32]) {
            self.borrow_mut().accept_waveform(handle, rate, samples)
        }
        fn decode(&mut self, recognizer: u8, stream: u8) -> bool {
            self.borrow_mut().decode(recognizer, stream)
        }
        fn get_result(&mut self, recognizer: u8, stream: u8) -> Option<u8> {
            self.borrow_mut().get_result(recognizer, stream)
        }
        fn read_result_text(&mut self, result: u8, copy: &mut dyn FnMut(&[u8])) -> bool {
            self.borrow_mut().read_result_text(result, copy)
        }
        fn destroy_result(&mut self, result: u8) {
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
        (OfflineRecognizer::new(capi.clone()), capi)
    }

    fn verified(directory: &Path) -> VerifiedParakeetModelSet {
        VerifiedParakeetModelSet {
            directory: directory.to_owned(),
        }
    }

    #[test]
    fn maps_c_configuration_audio_and_copies_before_release() {
        let directory = fixture();
        let (mut recognizer, capi) = recognizer(FakeCapi {
            text: "héllo".as_bytes().to_vec(),
            ..Default::default()
        });
        assert_eq!(
            recognizer.recognize(&verified(&directory), &[0.0, -0.5, 0.5]),
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
            recognizer(FakeCapi::default())
                .0
                .recognize(&verified(&directory), &[]),
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
            r.recognize(&verified(&directory), &[0.0]),
            Err(OfflineRecognizerError::ModelArtifactMissing)
        );
        assert!(capi.borrow().config.is_none());
        fs::create_dir(directory.join("tokens.txt")).unwrap();
        assert_eq!(
            r.recognize(&verified(&directory), &[0.0]),
            Err(OfflineRecognizerError::ModelArtifactNotFile)
        );
        assert_eq!(
            r.recognize(&verified(&directory), &[f32::NAN]),
            Err(OfflineRecognizerError::InvalidAudio)
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn corrupt_named_files_cannot_be_resolved_for_recognition() {
        let root = fixture();
        let revision = root
            .join("revisions")
            .join(PARAKEET_MODEL_MANIFEST.revision);
        fs::create_dir_all(&revision).unwrap();
        for artifact in PARAKEET_ARTIFACTS {
            fs::write(revision.join(artifact.filename), []).unwrap();
        }
        fs::write(
            root.join("current"),
            format!(
                "muniment-asr-pointer-v1\n{}\n{}\n",
                PARAKEET_MODEL_MANIFEST.identity, PARAKEET_MODEL_MANIFEST.revision
            ),
        )
        .unwrap();
        let lifecycle = AsrRevisionLifecycle::new(
            root.clone(),
            &PARAKEET_MODEL_MANIFESTS,
            &PARAKEET_MODEL_MANIFEST,
        )
        .unwrap();

        assert!(matches!(
            lifecycle.resolve_current_parakeet(),
            Err(AsrLifecycleError::RevisionInvalid(
                AsrModelSetVerificationError::WrongSize { .. }
            ))
        ));
        fs::remove_dir_all(root).unwrap();
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
            assert_eq!(r.recognize(&verified(&directory), &[0.0]), Err(error));
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
            assert_eq!(r.recognize(&verified(&directory), &[0.0]), Err(error));
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
