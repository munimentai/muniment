use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::process::Command;

use muniment_core::sidecar::pi_install::{PiArtifactDescriptor, PI_ARTIFACT};

pub fn stage_pi_stub(temporary_root: &Path) -> PiArtifactDescriptor {
    let pi_root = temporary_root.join("pi");
    let revision_root = pi_root.join("revisions").join(PI_ARTIFACT.version);
    let staged_stub = revision_root.join(PI_ARTIFACT.executable);
    fs::create_dir_all(staged_stub.parent().unwrap()).unwrap();

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut workspace_hasher = DefaultHasher::new();
    manifest_dir.hash(&mut workspace_hasher);
    let build_root = std::env::temp_dir().join(format!(
        "muniment-runtime-sidecar-test-stub-{:x}",
        workspace_hasher.finish()
    ));
    let status = Command::new(env!("CARGO"))
        .args([
            "build",
            "--quiet",
            "--package",
            "muniment-core",
            "--bin",
            "sidecar-test-stub",
            "--target-dir",
        ])
        .arg(&build_root)
        .current_dir(manifest_dir)
        .status()
        .unwrap();
    assert!(status.success());

    let stub_name = if cfg!(windows) {
        "sidecar-test-stub.exe"
    } else {
        "sidecar-test-stub"
    };
    let built_stub = build_root.join("debug").join(stub_name);
    fs::copy(built_stub, staged_stub).unwrap();
    let stub_archive = b"muniment-sidecar-test-stub\n";
    fs::write(revision_root.join(PI_ARTIFACT.archive), stub_archive).unwrap();
    fs::write(
        pi_root.join("current"),
        format!("muniment-pi-pointer-v1\n{}\n", PI_ARTIFACT.version),
    )
    .unwrap();
    std::env::set_var("MUNIMENT_PI_ROOT", pi_root);

    PiArtifactDescriptor {
        version: PI_ARTIFACT.version,
        archive: PI_ARTIFACT.archive,
        byte_size: stub_archive.len() as u64,
        sha256: "758b0db8f6304639edfca2b779e886f3006afeb006417e49dd6bce53ff2a65ab",
        executable: PI_ARTIFACT.executable,
    }
}
