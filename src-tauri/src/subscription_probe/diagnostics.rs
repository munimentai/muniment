use std::io::Write;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Code {
    Started,
    ProfileMissing,
    JobReadFailed,
    JobNameInvalid,
    JobOpenFailed,
    JobAssignFailed,
    ObserverTimedOut,
    DiagnosticWriteFailed,
    ProfileRestored,
    RuntimeWaiting,
    RuntimeConnected,
    RuntimeFailed,
}

impl Code {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::ProfileMissing => "profile-missing",
            Self::JobReadFailed => "job-read-failed",
            Self::JobNameInvalid => "job-name-invalid",
            Self::JobOpenFailed => "job-open-failed",
            Self::JobAssignFailed => "job-assign-failed",
            Self::ObserverTimedOut => "observer-timed-out",
            Self::DiagnosticWriteFailed => "diagnostic-write-failed",
            Self::ProfileRestored => "profile-restored",
            Self::RuntimeWaiting => "runtime-waiting",
            Self::RuntimeConnected => "runtime-connected",
            Self::RuntimeFailed => "runtime-failed",
        }
    }
}

pub(super) fn record(root: &Path, code: Code) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("subscription-probe-startup.log"))?;
    file.write_all(format!("pid={} code={}\n", std::process::id(), code.as_str()).as_bytes())
}

pub(super) fn restore(root: &Path, admit: impl FnOnce() -> Result<(), Code>) -> Result<(), Code> {
    if !root.is_absolute() || !root.join("subscription-probe.json").is_file() {
        return Err(Code::ProfileMissing);
    }
    admit()
}

// The updater joins installer arguments without quoting them.
pub(super) fn installer_arguments(root: &Path) -> Result<Vec<std::ffi::OsString>, &'static str> {
    let log = root.join("subscription-probe-msi.log");
    let log = log.to_str().ok_or("update-log-path")?;
    if !root.is_absolute() || log.contains(['"', '\r', '\n', '\0']) {
        return Err("update-log-path");
    }
    Ok([
        "REINSTALL=ALL",
        "REINSTALLMODE=vomus",
        "/L*V!",
        &format!("\"{log}\""),
    ]
    .into_iter()
    .map(Into::into)
    .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_profiles_never_admit_the_process() {
        assert_eq!(
            restore(Path::new("relative"), || panic!(
                "The probe admitted an invalid profile."
            )),
            Err(Code::ProfileMissing)
        );
        let root = std::env::temp_dir().join(format!("probe-missing-{}", std::process::id()));
        assert_eq!(
            restore(&root, || panic!("The probe admitted an invalid profile.")),
            Err(Code::ProfileMissing)
        );
    }

    #[test]
    fn restore_preserves_job_failure_codes_and_writes_only_fixed_fields() {
        let root = std::env::temp_dir().join(format!("probe-startup-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("subscription-probe.json"), "{}").unwrap();
        for code in [
            Code::Started,
            Code::ProfileMissing,
            Code::JobReadFailed,
            Code::JobNameInvalid,
            Code::JobOpenFailed,
            Code::JobAssignFailed,
            Code::ObserverTimedOut,
            Code::DiagnosticWriteFailed,
            Code::ProfileRestored,
            Code::RuntimeWaiting,
            Code::RuntimeConnected,
            Code::RuntimeFailed,
        ] {
            assert_eq!(restore(&root, || Err(code)), Err(code));
            record(&root, code).unwrap();
        }
        assert_eq!(restore(&root, || Ok(())), Ok(()));
        let text = std::fs::read_to_string(root.join("subscription-probe-startup.log")).unwrap();
        assert_eq!(text.lines().count(), 12);
        assert!(text
            .lines()
            .all(|line| line.starts_with(&format!("pid={} code=", std::process::id()))));
        assert!(!text.contains(&root.to_string_lossy().to_string()));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn msi_logging_quotes_spaces_and_rejects_argument_injection() {
        let root = std::env::temp_dir().join("probe profile");
        let args = installer_arguments(&root).unwrap();
        assert_eq!(args[..3], ["REINSTALL=ALL", "REINSTALLMODE=vomus", "/L*V!"]);
        assert_eq!(
            args[3],
            format!("\"{}\"", root.join("subscription-probe-msi.log").display()).as_str()
        );
        assert!(installer_arguments(Path::new("relative")).is_err());
        assert!(installer_arguments(&root.join("bad\" /quiet")).is_err());
        assert!(installer_arguments(&root.join("bad\npath")).is_err());
    }
}
