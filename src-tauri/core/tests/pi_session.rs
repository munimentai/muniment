use std::fs;
use std::path::{Path, PathBuf};

use muniment_core::sidecar::{pi_sidecar_config, validate_pi_session};

fn root() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "muniment-pi-session-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    root
}

#[test]
fn new_and_reopen_launch_arguments_are_owned() {
    let root = root();
    let new = pi_sidecar_config("pi", &root, None).unwrap();
    assert_eq!(&new.args[..3], ["--mode", "rpc", "--session-dir"]);
    assert_eq!(Path::new(&new.args[3]), root.canonicalize().unwrap());
    assert!(!new.args.iter().any(|arg| arg == "--no-session"));

    fs::write(root.join("session.jsonl"), "{}\n").unwrap();
    let (locator, path) = validate_pi_session(&root, "session.jsonl").unwrap();
    let reopen = pi_sidecar_config("pi", &root, Some(&locator)).unwrap();
    assert_eq!(&reopen.args[4..], ["--session", path.to_str().unwrap()]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn session_validation_fails_closed_without_disclosing_paths() {
    let root = root();
    assert_eq!(
        validate_pi_session(&root, "../outside.jsonl").unwrap_err(),
        "Pi session locator is invalid"
    );
    assert_eq!(
        validate_pi_session(&root, "missing.jsonl").unwrap_err(),
        "Pi session file is unavailable"
    );
    fs::create_dir(root.join("directory.jsonl")).unwrap();
    assert_eq!(
        validate_pi_session(&root, "directory.jsonl").unwrap_err(),
        "Pi session file is unavailable"
    );
    fs::remove_dir_all(root).unwrap();
}
