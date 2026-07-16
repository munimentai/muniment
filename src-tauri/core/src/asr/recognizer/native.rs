//! Minimal sherpa-onnx v1.13.2 offline C adapter.

use std::ffi::{c_char, CStr, CString};

use super::{OfflineAbi, OfflineConfig};

pub(super) struct SherpaOnnxAbi;

#[repr(C)]
struct Ptr1 {
    a: *const c_char,
}
#[repr(C)]
struct Ptr2 {
    a: *const c_char,
    b: *const c_char,
}
#[repr(C)]
struct Ptr3 {
    a: *const c_char,
    b: *const c_char,
    c: *const c_char,
}
#[repr(C)]
struct Whisper {
    encoder: *const c_char,
    decoder: *const c_char,
    language: *const c_char,
    task: *const c_char,
    tail_paddings: i32,
    token_timestamps: i32,
    segment_timestamps: i32,
}
#[repr(C)]
struct Canary {
    encoder: *const c_char,
    decoder: *const c_char,
    src_lang: *const c_char,
    tgt_lang: *const c_char,
    use_pnc: i32,
}
#[repr(C)]
struct Cohere {
    encoder: *const c_char,
    decoder: *const c_char,
    language: *const c_char,
    use_punct: i32,
    use_itn: i32,
}
#[repr(C)]
struct Moonshine {
    preprocessor: *const c_char,
    encoder: *const c_char,
    uncached_decoder: *const c_char,
    cached_decoder: *const c_char,
    merged_decoder: *const c_char,
}
#[repr(C)]
struct SenseVoice {
    model: *const c_char,
    language: *const c_char,
    use_itn: i32,
}
#[repr(C)]
struct FunAsrNano {
    encoder_adaptor: *const c_char,
    llm: *const c_char,
    embedding: *const c_char,
    tokenizer: *const c_char,
    system_prompt: *const c_char,
    user_prompt: *const c_char,
    max_new_tokens: i32,
    temperature: f32,
    top_p: f32,
    seed: i32,
    language: *const c_char,
    itn: i32,
    hotwords: *const c_char,
}
#[repr(C)]
struct Qwen3 {
    conv_frontend: *const c_char,
    encoder: *const c_char,
    decoder: *const c_char,
    tokenizer: *const c_char,
    max_total_len: i32,
    max_new_tokens: i32,
    temperature: f32,
    top_p: f32,
    seed: i32,
    hotwords: *const c_char,
}

#[repr(C)]
struct FeatureConfig {
    sample_rate: i32,
    feature_dim: i32,
}

#[repr(C)]
struct ModelConfig {
    transducer: Ptr3,
    paraformer: Ptr1,
    nemo_ctc: Ptr1,
    whisper: Whisper,
    tdnn: Ptr1,
    tokens: *const c_char,
    num_threads: i32,
    debug: i32,
    provider: *const c_char,
    model_type: *const c_char,
    modeling_unit: *const c_char,
    bpe_vocab: *const c_char,
    telespeech_ctc: *const c_char,
    sense_voice: SenseVoice,
    moonshine: Moonshine,
    fire_red_asr: Ptr2,
    dolphin: Ptr1,
    zipformer_ctc: Ptr1,
    canary: Canary,
    wenet_ctc: Ptr1,
    omnilingual: Ptr1,
    medasr: Ptr1,
    funasr_nano: FunAsrNano,
    fire_red_asr_ctc: Ptr1,
    qwen3_asr: Qwen3,
    cohere_transcribe: Cohere,
}

#[repr(C)]
struct LmConfig {
    model: *const c_char,
    scale: f32,
}
#[repr(C)]
struct Homophone {
    dict_dir: *const c_char,
    lexicon: *const c_char,
    rule_fsts: *const c_char,
}
#[repr(C)]
struct RecognizerConfig {
    feat_config: FeatureConfig,
    model_config: ModelConfig,
    lm_config: LmConfig,
    decoding_method: *const c_char,
    max_active_paths: i32,
    hotwords_file: *const c_char,
    hotwords_score: f32,
    rule_fsts: *const c_char,
    rule_fars: *const c_char,
    blank_penalty: f32,
    hr: Homophone,
}

#[repr(C)]
pub(super) struct Recognizer {
    _private: [u8; 0],
}
#[repr(C)]
pub(super) struct Stream {
    _private: [u8; 0],
}
#[repr(C)]
pub(super) struct ResultSnapshot {
    text: *const c_char,
}

#[link(name = "sherpa-onnx-c-api")]
extern "C" {
    fn SherpaOnnxCreateOfflineRecognizer(config: *const RecognizerConfig) -> *const Recognizer;
    fn SherpaOnnxDestroyOfflineRecognizer(recognizer: *const Recognizer);
    fn SherpaOnnxCreateOfflineStream(recognizer: *const Recognizer) -> *const Stream;
    fn SherpaOnnxDestroyOfflineStream(stream: *const Stream);
    fn SherpaOnnxAcceptWaveformOffline(
        stream: *const Stream,
        sample_rate: i32,
        samples: *const f32,
        n: i32,
    );
    fn SherpaOnnxDecodeOfflineStream(recognizer: *const Recognizer, stream: *const Stream);
    fn SherpaOnnxGetOfflineStreamResult(stream: *const Stream) -> *const ResultSnapshot;
    fn SherpaOnnxDestroyOfflineRecognizerResult(result: *const ResultSnapshot);
}

impl OfflineAbi for SherpaOnnxAbi {
    type Recognizer = *const Recognizer;
    type Stream = *const Stream;
    type Result = *const ResultSnapshot;

    fn create_recognizer(&self, config: &OfflineConfig) -> Option<Self::Recognizer> {
        let encoder = path_cstring(&config.encoder)?;
        let decoder = path_cstring(&config.decoder)?;
        let joiner = path_cstring(&config.joiner)?;
        let tokens = path_cstring(&config.tokens)?;
        let provider = CString::new(config.provider).ok()?;
        let model_type = CString::new(config.model_type).ok()?;
        let decoding_method = CString::new(config.decoding_method).ok()?;
        // All C strings remain owned in this scope for the complete create call.
        let mut native: RecognizerConfig = unsafe { std::mem::zeroed() };
        native.feat_config = FeatureConfig {
            sample_rate: config.sample_rate,
            feature_dim: config.feature_dim,
        };
        native.model_config.transducer = Ptr3 {
            a: encoder.as_ptr(),
            b: decoder.as_ptr(),
            c: joiner.as_ptr(),
        };
        native.model_config.tokens = tokens.as_ptr();
        native.model_config.num_threads = config.num_threads;
        native.model_config.provider = provider.as_ptr();
        native.model_config.model_type = model_type.as_ptr();
        native.decoding_method = decoding_method.as_ptr();
        let handle = unsafe { SherpaOnnxCreateOfflineRecognizer(&native) };
        (!handle.is_null()).then_some(handle)
    }

    fn destroy_recognizer(&self, recognizer: Self::Recognizer) {
        unsafe { SherpaOnnxDestroyOfflineRecognizer(recognizer) }
    }
    fn create_stream(&self, recognizer: Self::Recognizer) -> Option<Self::Stream> {
        let stream = unsafe { SherpaOnnxCreateOfflineStream(recognizer) };
        (!stream.is_null()).then_some(stream)
    }
    fn destroy_stream(&self, stream: Self::Stream) {
        unsafe { SherpaOnnxDestroyOfflineStream(stream) }
    }
    fn accept_waveform(
        &self,
        stream: Self::Stream,
        sample_rate: i32,
        samples: &[f32],
        sample_count: i32,
    ) {
        unsafe {
            SherpaOnnxAcceptWaveformOffline(stream, sample_rate, samples.as_ptr(), sample_count)
        }
    }
    fn decode(&self, recognizer: Self::Recognizer, stream: Self::Stream) {
        unsafe { SherpaOnnxDecodeOfflineStream(recognizer, stream) }
    }
    fn get_result(&self, stream: Self::Stream) -> Option<Self::Result> {
        let result = unsafe { SherpaOnnxGetOfflineStreamResult(stream) };
        (!result.is_null()).then_some(result)
    }
    fn copy_result_text(&self, result: Self::Result) -> Option<Vec<u8>> {
        let text = unsafe { (*result).text };
        (!text.is_null()).then(|| unsafe { CStr::from_ptr(text).to_bytes().to_vec() })
    }
    fn destroy_result(&self, result: Self::Result) {
        unsafe { SherpaOnnxDestroyOfflineRecognizerResult(result) }
    }
}

fn path_cstring(path: &std::path::Path) -> Option<CString> {
    CString::new(path.to_str()?.as_bytes()).ok()
}
