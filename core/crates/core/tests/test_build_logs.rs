//! A test binary never writes the owner's runtime logs, however it starts.

use muniment_core::state_root;

const CHILD: &str = "MUNIMENT_TEST_BUILD_LOGS_CHILD";

/// Checks the log rules from inside the copy, which runs outside `deps`
/// with no test variable set.
fn check_as_copy() {
    const { assert!(state_root::TEST_BUILD) };
    assert!(!state_root::is_test_run());
    assert!(state_root::is_test_build());
    assert_eq!(
        state_root::log_directory_override(),
        Some(state_root::test_log_directory())
    );
    muniment_core::runtime_eprintln!("muniment-runtime: test build log check");
    #[cfg(target_os = "macos")]
    {
        let written =
            std::fs::read_to_string(state_root::test_log_directory().join("runtime-service.log"))
                .unwrap();
        assert!(written.contains("test build log check"), "{written}");
        let owner = muniment_core::user_diagnostics::effective_user_home()
            .unwrap()
            .join("Library/Logs/Muniment");
        let refused = std::panic::catch_unwind(|| state_root::guard_log_directory(&owner));
        assert!(refused.is_err());
    }
}

#[test]
fn a_copied_test_binary_started_by_hand_logs_under_its_test_root() {
    if std::env::var_os(CHILD).is_some() {
        check_as_copy();
        return;
    }
    const { assert!(state_root::TEST_BUILD) };
    let copy = state_root::test_root().join("copied-test-binary");
    std::fs::create_dir_all(&copy).unwrap();
    let executable = copy.join("test-build-logs");
    std::fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
    let output = std::process::Command::new(&executable)
        .args([
            "--exact",
            "a_copied_test_binary_started_by_hand_logs_under_its_test_root",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .env_remove(state_root::LOG_DIRECTORY_VARIABLE)
        .env_remove(state_root::TEST_RUN_VARIABLE)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::remove_dir_all(copy).unwrap();
}
