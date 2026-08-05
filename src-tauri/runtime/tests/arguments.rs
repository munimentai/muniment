use std::process::{Command, Output};

const INVALID_WAIT_TIMEOUT: (&str, &str) = ("MUNIMENT_RUNTIME_TEST_WAIT_TIMEOUT_MS", "invalid");

fn runtime(argument: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_muniment-runtime"))
        .arg(argument)
        .env(INVALID_WAIT_TIMEOUT.0, INVALID_WAIT_TIMEOUT.1)
        .output()
        .unwrap()
}

#[test]
fn prints_the_version_without_starting_the_runtime() {
    let output = runtime("--version");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn prints_help_for_each_help_argument_without_starting_the_runtime() {
    for argument in ["--help", "-h"] {
        let output = runtime(argument);

        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert_eq!(stdout.matches("Usage:").count(), 1);
        assert!(stdout.contains("muniment-runtime"));
        assert!(stdout.contains("-h, --help"));
        assert!(stdout.contains("--version"));
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn rejects_an_unknown_argument_without_starting_the_runtime() {
    let output = runtime("--unknown");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("unknown argument: --unknown"));
}
