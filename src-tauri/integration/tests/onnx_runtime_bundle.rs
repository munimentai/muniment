use std::fs;
use std::path::Path;

use muniment_core::onnx_runtime::bundled_library_file_name;

#[test]
fn bundled_file_name_matches_the_build_script() {
    let build_script =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../build.rs")).unwrap();
    assert!(build_script.contains(&format!("\"{}\"", bundled_library_file_name())));
}
