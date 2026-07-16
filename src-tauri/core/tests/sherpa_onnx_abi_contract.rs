#[test]
fn get_offline_stream_result_matches_pinned_header() {
    let header = include_str!("fixtures/sherpa-onnx-v1.13.2-get-result.h");
    let declaration = header.split_whitespace().collect::<String>();
    assert_eq!(
        declaration,
        "/*Extractedverbatimfromsherpa-onnxv1.13.2c-api/c-api.h.*/\
SHERPA_ONNX_APIconstSherpaOnnxOfflineRecognizerResult*\
SherpaOnnxGetOfflineStreamResult(constSherpaOnnxOfflineStream*stream);"
    );

    let adapter = include_str!("../src/asr/recognizer/native.rs");
    assert!(adapter.contains(
        "fn SherpaOnnxGetOfflineStreamResult(stream: *const Stream) -> *const ResultSnapshot;"
    ));
    assert!(!adapter.contains("SherpaOnnxGetOfflineStreamResult(recognizer"));
}
