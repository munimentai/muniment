//! ABI declarations for sherpa-onnx v1.13.2's offline recognizer C API.

#![allow(dead_code)]

use std::ffi::{c_char, c_float, c_void};

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FeatureConfig {
    pub sample_rate: i32,
    pub feature_dim: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Transducer {
    pub encoder: *const c_char,
    pub decoder: *const c_char,
    pub joiner: *const c_char,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct OneModel {
    pub model: *const c_char,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Whisper {
    pub encoder: *const c_char,
    pub decoder: *const c_char,
    pub language: *const c_char,
    pub task: *const c_char,
    pub tail_paddings: i32,
    pub enable_token_timestamps: i32,
    pub enable_segment_timestamps: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Canary {
    pub encoder: *const c_char,
    pub decoder: *const c_char,
    pub src_lang: *const c_char,
    pub tgt_lang: *const c_char,
    pub use_pnc: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TwoModel {
    pub encoder: *const c_char,
    pub decoder: *const c_char,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Moonshine {
    pub preprocessor: *const c_char,
    pub encoder: *const c_char,
    pub uncached_decoder: *const c_char,
    pub cached_decoder: *const c_char,
    pub merged_decoder: *const c_char,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Lm {
    pub model: *const c_char,
    pub scale: c_float,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SenseVoice {
    pub model: *const c_char,
    pub language: *const c_char,
    pub use_itn: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FunAsrNano {
    pub encoder_adaptor: *const c_char,
    pub llm: *const c_char,
    pub embedding: *const c_char,
    pub tokenizer: *const c_char,
    pub system_prompt: *const c_char,
    pub user_prompt: *const c_char,
    pub max_new_tokens: i32,
    pub temperature: c_float,
    pub top_p: c_float,
    pub seed: i32,
    pub language: *const c_char,
    pub itn: i32,
    pub hotwords: *const c_char,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Qwen3 {
    pub conv_frontend: *const c_char,
    pub encoder: *const c_char,
    pub decoder: *const c_char,
    pub tokenizer: *const c_char,
    pub max_total_len: i32,
    pub max_new_tokens: i32,
    pub temperature: c_float,
    pub top_p: c_float,
    pub seed: i32,
    pub hotwords: *const c_char,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Cohere {
    pub encoder: *const c_char,
    pub decoder: *const c_char,
    pub language: *const c_char,
    pub use_punct: i32,
    pub use_itn: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ModelConfig {
    pub transducer: Transducer,
    pub paraformer: OneModel,
    pub nemo_ctc: OneModel,
    pub whisper: Whisper,
    pub tdnn: OneModel,
    pub tokens: *const c_char,
    pub num_threads: i32,
    pub debug: i32,
    pub provider: *const c_char,
    pub model_type: *const c_char,
    pub modeling_unit: *const c_char,
    pub bpe_vocab: *const c_char,
    pub telespeech_ctc: *const c_char,
    pub sense_voice: SenseVoice,
    pub moonshine: Moonshine,
    pub fire_red_asr: TwoModel,
    pub dolphin: OneModel,
    pub zipformer_ctc: OneModel,
    pub canary: Canary,
    pub wenet_ctc: OneModel,
    pub omnilingual: OneModel,
    pub medasr: OneModel,
    pub funasr_nano: FunAsrNano,
    pub fire_red_asr_ctc: OneModel,
    pub qwen3_asr: Qwen3,
    pub cohere_transcribe: Cohere,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Homophone {
    pub dict_dir: *const c_char,
    pub lexicon: *const c_char,
    pub rule_fsts: *const c_char,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RecognizerConfig {
    pub feat_config: FeatureConfig,
    pub model_config: ModelConfig,
    pub lm_config: Lm,
    pub decoding_method: *const c_char,
    pub max_active_paths: i32,
    pub hotwords_file: *const c_char,
    pub hotwords_score: c_float,
    pub rule_fsts: *const c_char,
    pub rule_fars: *const c_char,
    pub blank_penalty: c_float,
    pub hr: Homophone,
}

pub type Recognizer = c_void;
pub type Stream = c_void;
pub type GetString = unsafe extern "C" fn() -> *const c_char;
pub type CreateRecognizer = unsafe extern "C" fn(*const RecognizerConfig) -> *const Recognizer;
pub type DestroyRecognizer = unsafe extern "C" fn(*const Recognizer);
pub type CreateStream = unsafe extern "C" fn(*const Recognizer) -> *const Stream;
pub type DestroyStream = unsafe extern "C" fn(*const Stream);
pub type AcceptWaveform = unsafe extern "C" fn(*const Stream, i32, *const f32, i32);
pub type Decode = unsafe extern "C" fn(*const Recognizer, *const Stream);
pub type GetResult = unsafe extern "C" fn(*const Stream) -> *const c_char;
pub type DestroyResult = unsafe extern "C" fn(*const c_char);
