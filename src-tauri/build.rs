use std::path::Path;

const ROOT: &str = "third-party/sherpa-onnx-v1.13.2";

fn main() {
    let os = std::env::var("CARGO_CFG_TARGET_OS").expect("target OS is set by Cargo");
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").expect("target arch is set by Cargo");
    let runtime = match (os.as_str(), arch.as_str()) {
        ("linux", "x86_64") => "linux-x86_64",
        ("windows", "x86_64") => "windows-x86_64",
        ("macos", "x86_64" | "aarch64") => "macos-universal2",
        _ => panic!(
            "unsupported desktop ASR target: {os}-{arch}; supported targets are macOS universal2, Windows x86_64, and Linux x86_64"
        ),
    };

    let required_runtime: &[&str] = match os.as_str() {
        "linux" => &["libsherpa-onnx-c-api.so", "libonnxruntime.so"],
        "macos" => &["libsherpa-onnx-c-api.dylib", "libonnxruntime.1.24.4.dylib"],
        "windows" => &[
            "sherpa-onnx-c-api.dll",
            "onnxruntime.dll",
            "onnxruntime_providers_shared.dll",
        ],
        _ => unreachable!(),
    };
    for filename in required_runtime {
        require(&format!("{ROOT}/{runtime}/{filename}"));
    }
    for notice in [
        "notices/THIRD-PARTY-NOTICES.md",
        "notices/sherpa-onnx-LICENSE.txt",
        "notices/onnxruntime-LICENSE.txt",
        "notices/onnxruntime-ThirdPartyNotices.txt",
    ] {
        require(&format!("{ROOT}/{notice}"));
    }

    // The packaged libraries live in Tauri's resource directory. Keep loader
    // lookup relative to the executable so no machine-global install is used.
    match os.as_str() {
        "linux" => println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/../lib/muniment/resources/asr-runtime"),
        "macos" => println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Resources/asr-runtime"),
        "windows" => {}
        _ => unreachable!(),
    }
    tauri_build::build()
}

fn require(path: &str) {
    println!("cargo:rerun-if-changed={path}");
    assert!(Path::new(path).is_file(), "packaged ASR runtime component or corresponding notice is missing: {path}");
}
