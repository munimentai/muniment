use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

const MACOS_TEST_EXIT_ENV: &str = "MUNIMENT_RUNTIME_TEST_MACOS_ACTIVATION_EXIT";

fn directory() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "muniment-runtime-macos-status-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    path
}

fn runtime(directory: &PathBuf, exit: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_muniment-runtime"))
        .env("XDG_DATA_HOME", directory)
        .env(MACOS_TEST_EXIT_ENV, exit)
        .output()
        .unwrap()
}

#[test]
fn maps_macos_activation_outcomes_to_process_statuses() {
    let directory = directory();

    assert!(runtime(&directory, "orderly").status.success());
    for _ in 0..4 {
        assert_eq!(runtime(&directory, "failed").status.code(), Some(1));
    }
    assert!(runtime(&directory, "failed").status.success());

    fs::remove_dir_all(directory).unwrap();
}
