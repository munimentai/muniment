fn main() {
    let target = std::env::var("TARGET").expect("Cargo did not provide TARGET");
    const SUPPORTED: &[&str] = &[
        "x86_64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
    ];
    if !SUPPORTED.contains(&target.as_str()) {
        panic!("unsupported desktop ASR target {target}; no pinned sherpa-onnx CPU artifact exists");
    }
    println!("cargo:rerun-if-changed=asr-runtime.toml");
    println!("cargo:rerun-if-changed=../docs/third-party/sherpa-onnx-NOTICE.md");
    tauri_build::build()
}
