use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

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

    let required_runtime: &[(&str, &str)] = match os.as_str() {
        "linux" => &[
            (
                "libsherpa-onnx-c-api.so",
                "33e09c24dabb94f749cae7f97d9dd3317944695dd68a6c09d316a863b81ea011",
            ),
            (
                "libonnxruntime.so",
                "88468d42d3381c18a7bd3f99a01f07293ff8c9e38f71fd5bcc5fa08c101d31bf",
            ),
        ],
        "macos" => &[
            (
                "libsherpa-onnx-c-api.dylib",
                "b7b0a34667834cb03a227d13130a08e990204fcc8dd9a5c9d532224266a18afd",
            ),
            (
                "libonnxruntime.1.24.4.dylib",
                "e9a9534fc92910d9bd6ffd155c13ce7920417c652c5c1920178520880627513e",
            ),
        ],
        "windows" => &[
            (
                "sherpa-onnx-c-api.dll",
                "ee59933bb110fe8badf886a85fe3caaee0cf0d1a28497028b67ec711375c0cca",
            ),
            (
                "sherpa-onnx-c-api.lib",
                "a42f595587731c8c83b8b48f99e94a3ebe5885399902b54e218a0cb1066e62aa",
            ),
            (
                "onnxruntime.dll",
                "8b695444d1a35ed0c8338b8c14438b3be5e0a3b222b88b1e7b4ce8753f135b50",
            ),
            (
                "onnxruntime.lib",
                "4b8482a2b5cc3b825e468a13672e65e1e2b771c669cc6f29bb90cea3a87e16b0",
            ),
            (
                "onnxruntime_providers_shared.dll",
                "ebc55b0f28e8a79cbf78e810a7f510ba70e75a2dfbcfcc6aca31ab2b8710a59a",
            ),
        ],
        _ => unreachable!(),
    };
    let link_directory = PathBuf::from(ROOT).join("link");
    let configured_link_directory = std::env::var_os("SHERPA_ONNX_LIB_DIR")
        .map(PathBuf::from)
        .expect("SHERPA_ONNX_LIB_DIR must select the checked-in ASR runtime");
    assert_eq!(
        canonical(&configured_link_directory),
        canonical(&link_directory),
        "sherpa-onnx must link the checked-in runtime that Tauri bundles"
    );
    for (filename, sha256) in required_runtime {
        let bundled = PathBuf::from(ROOT).join(runtime).join(filename);
        require_hash(&bundled, sha256);
        let linked = link_directory.join(filename);
        require_hash(&linked, sha256);
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
        "linux" => println!(
            "cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/../lib/muniment/resources/asr-runtime"
        ),
        "macos" => {
            println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Resources/asr-runtime")
        }
        "windows" => {}
        _ => unreachable!(),
    }
    tauri_build::build()
}

fn require(path: &str) {
    println!("cargo:rerun-if-changed={path}");
    assert!(
        Path::new(path).is_file(),
        "packaged ASR runtime component or corresponding notice is missing: {path}"
    );
}

fn require_hash(path: &Path, expected: &str) {
    let display = path.display();
    println!("cargo:rerun-if-changed={display}");
    let mut file = File::open(path)
        .unwrap_or_else(|_| panic!("packaged ASR runtime component is missing: {display}"));
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .unwrap_or_else(|_| panic!("packaged ASR runtime component is unreadable: {display}"));
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    assert_eq!(
        format!("{:x}", digest.finalize()),
        expected,
        "packaged ASR runtime component has the wrong pinned identity: {display}"
    );
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize()
        .unwrap_or_else(|_| panic!("ASR runtime path is missing: {}", path.display()))
}
