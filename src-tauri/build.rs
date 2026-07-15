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
    match target.as_str() {
        "x86_64-unknown-linux-gnu" => {
            println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/../lib/muniment");
        }
        "x86_64-apple-darwin" | "aarch64-apple-darwin" => {
            println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Resources");
        }
        "x86_64-pc-windows-msvc" => {}
        _ => unreachable!(),
    }
    println!("cargo:rerun-if-changed=asr-runtime.toml");
    println!("cargo:rerun-if-changed=../docs/third-party/sherpa-onnx-NOTICE.md");
    println!("cargo:rerun-if-changed=../docs/third-party/sherpa-onnx-LICENSE.txt");
    println!("cargo:rerun-if-changed=../docs/third-party/onnxruntime-LICENSE.txt");
    tauri_build::build()
}
