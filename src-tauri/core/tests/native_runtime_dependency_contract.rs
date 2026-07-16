#[test]
fn default_core_manifest_does_not_depend_on_native_sherpa_runtime_crates() {
    let manifest = include_str!("../Cargo.toml");

    assert!(
        manifest
            .lines()
            .any(|line| line.trim() == "native-sherpa-onnx = []"),
        "the native feature should expose only the direct FFI adapter"
    );

    for line in manifest.lines().map(str::trim) {
        assert!(
            !line.starts_with("sherpa-onnx =") && !line.starts_with("sherpa-onnx-sys ="),
            "the direct FFI adapter must not make the core dependency graph build a native runtime"
        );
    }
}
