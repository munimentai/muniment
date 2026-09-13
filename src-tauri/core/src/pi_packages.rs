//! The candidate acquires packages through the verified Pi executable's embedded Bun runtime.

use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fs2::FileExt;
use serde_json::Value;

pub const PI_PACKAGES: [(&str, &str); 4] = [
    ("pi-web-access", "0.28.0"),
    ("pi-subagents", "0.65.1"),
    ("pi-background-tasks", "2.5.0"),
    ("pi-mcp-adapter", "2.32.1"),
];

fn installed(directory: &Path) -> bool {
    PI_PACKAGES.iter().all(|(name, version)| {
        fs::read(
            directory
                .join("node_modules")
                .join(name)
                .join("package.json"),
        )
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .is_some_and(|manifest| manifest["version"] == *version)
    })
}

fn install_command(executable: &Path, directory: &Path) -> Command {
    let mut command = Command::new(executable);
    command
        // Bun's documented BUN_BE_BUN switch exposes the embedded package manager.
        // Scope it to acquisition. The RPC process must run Pi's entrypoint.
        .env("BUN_BE_BUN", "1")
        .arg("install")
        .args(
            PI_PACKAGES
                .iter()
                .map(|(name, version)| format!("{name}@{version}")),
        )
        .args(["--omit=peer", "--ignore-scripts", "--exact", "--cwd"])
        .arg(directory)
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    command
}

fn capture_stderr(mut reader: impl Read, tail: &Mutex<VecDeque<String>>) {
    let mut buffer = [0; 4096];
    let mut line = Vec::new();
    let mut oversized = false;
    let mut private_key = false;
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        for byte in buffer[..count]
            .iter()
            .copied()
            .chain((count == 0).then_some(b'\n'))
        {
            if byte == b'\n' {
                let text = String::from_utf8_lossy(&line);
                if text.contains("-----BEGIN") && text.contains("PRIVATE KEY") {
                    private_key = true;
                }
                let detail = if oversized || private_key {
                    "[redacted]".into()
                } else {
                    crate::pi_launch::diagnostic_text(&text)
                };
                if text.contains("-----END") && text.contains("PRIVATE KEY") {
                    private_key = false;
                }
                if !detail.is_empty() {
                    let mut tail = tail.lock().unwrap();
                    if tail.len() == 20 {
                        tail.pop_front();
                    }
                    tail.push_back(detail);
                }
                line.clear();
                oversized = false;
            } else if line.len() < 65_536 {
                line.push(byte);
            } else {
                // Drop the whole line rather than expose a clipped credential.
                oversized = true;
            }
        }
        if count == 0 {
            break;
        }
    }
}

fn install_failure(error: io::Error, tail: &str) -> io::Error {
    io::Error::new(error.kind(), format!("{error} stderr_tail={tail}"))
}

fn run_install(command: &mut Command, timeout: Duration) -> io::Result<String> {
    let mut child = command.spawn()?;
    let tail = Arc::new(Mutex::new(VecDeque::new()));
    let captured = tail.clone();
    let stderr = child.stderr.take().expect("package install pipes stderr");
    let reader = match std::thread::Builder::new()
        .name("pi-package-stderr".into())
        .spawn(move || {
            capture_stderr(stderr, &captured);
        }) {
        Ok(reader) => reader,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let deadline = Instant::now() + timeout;
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break Ok(()),
            Ok(Some(status)) => {
                break Err(io::Error::other(format!(
                    "Pi package install failed: {status}"
                )))
            }
            Err(error) => break Err(error),
            Ok(None) if Instant::now() >= deadline => {
                break Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Pi package acquisition timed out.",
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    // A descendant can keep stderr open after Bun exits. Bound the drain like the sidecar supervisor.
    let deadline = Instant::now() + Duration::from_millis(200);
    while !reader.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    if reader.is_finished() {
        let _ = reader.join();
    }
    let tail = tail
        .lock()
        .unwrap()
        .iter()
        .cloned()
        .collect::<Vec<_>>()
        .join(" | ");
    // Bound the tail after redaction.
    let tail: String = tail
        .chars()
        .rev()
        .take(4096)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    result
        .map(|()| tail.clone())
        .map_err(|error| install_failure(error, &tail))
}

/// Acquire packages before RPC starts. The caller supplies the verified candidate executable.
pub fn prepare_pi_packages(agent_directory: &Path, executable: &Path) -> io::Result<()> {
    let directory = agent_directory.join("npm");
    fs::create_dir_all(&directory)?;
    // An OS lock releases on process death. Never hold Pi's settings lock during network access.
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join(".muniment-install.lock"))?;
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        match lock.try_lock_exclusive() {
            Ok(()) => break,
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(error),
        }
    }
    let marker = directory.join(".muniment-packages.json");
    let identity = serde_json::to_vec(&PI_PACKAGES)?;
    if fs::read(&marker).is_ok_and(|bytes| bytes == identity) && installed(&directory) {
        return Ok(());
    }
    // A failed or interrupted install must retry, even if it wrote the top-level manifests.
    match fs::remove_file(&marker) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    match OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(directory.join("package.json"))
    {
        Ok(mut file) => file.write_all(b"{\"private\":true}")?,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    // Resolve relative overrides before --cwd changes Bun's working directory.
    let directory = fs::canonicalize(directory)?;
    let stderr_tail = run_install(
        &mut install_command(executable, &directory),
        Duration::from_secs(120),
    )?;
    if !installed(&directory) {
        return Err(install_failure(
            io::Error::other("Pi package acquisition did not install the pinned versions."),
            &stderr_tail,
        ));
    }
    let mut file = fs::File::create(marker)?;
    file.write_all(&identity)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stderr_capture_bounds_output_and_redacts_before_retaining_the_tail() {
        let tail = Mutex::new(VecDeque::new());
        let input = format!("{}\npassword=registry-secret\nhttps://user:pass@example.com/?token=hidden\n{}\nlast diagnostic", "detail\n".repeat(30), "x".repeat(70_000));
        capture_stderr(input.as_bytes(), &tail);
        let tail = tail.into_inner().unwrap();
        assert_eq!(tail.len(), 20);
        assert_eq!(tail.back().unwrap(), "last diagnostic");
        let text = tail.into_iter().collect::<Vec<_>>().join(" ");
        assert!(!text.contains("registry-secret"));
        assert!(!text.contains("hidden"));
        assert!(!text.contains(&"x".repeat(100)));
        assert!(text.contains("[redacted]"));
    }

    #[test]
    #[cfg(unix)]
    fn failed_and_timed_out_installs_keep_stderr_and_exit_details() {
        for (script, timeout, expected) in [
            (
                "printf 'registry refused\\npassword=hidden-value\\n' >&2; exit 7",
                Duration::from_secs(2),
                "exit status: 7",
            ),
            (
                "printf 'registry stalled\\n' >&2; exec sleep 5",
                Duration::from_millis(100),
                "timed out",
            ),
        ] {
            let mut command = Command::new("sh");
            command.args(["-c", script]).stderr(Stdio::piped());
            let started = Instant::now();
            let error = run_install(&mut command, timeout).unwrap_err();
            assert!(started.elapsed() < Duration::from_secs(3));
            let detail = error.to_string();
            assert!(detail.contains(expected), "{detail}");
            assert!(detail.contains("stderr_tail=registry"), "{detail}");
            assert!(!detail.contains("hidden-value"));
        }
    }

    #[test]
    #[cfg(unix)]
    fn missing_pinned_versions_keep_stderr_without_publishing_a_marker() {
        use std::os::unix::fs::PermissionsExt;
        let root =
            std::env::temp_dir().join(format!("muniment-package-version-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let executable = root.join("pi-stub");
        fs::write(
            &executable,
            "#!/bin/sh\nprintf 'registry returned incomplete packages\\n' >&2\nexit 0\n",
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let error = prepare_pi_packages(&root, &executable)
            .unwrap_err()
            .to_string();
        assert!(error.contains("did not install the pinned versions"));
        assert!(error.contains("stderr_tail=registry returned incomplete packages"));
        assert!(!root.join("npm/.muniment-packages.json").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(windows)]
    fn config_accepts_verbatim_session_root_and_agent_directory() {
        use crate::sidecar::pi_install::PI_CANDIDATE_ARTIFACT;
        let root = std::env::temp_dir().join(format!("muniment-verbatim-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("sessions")).unwrap();
        let root = root.canonicalize().unwrap();
        assert!(root.to_string_lossy().starts_with(r"\\?\"));
        let sessions = root.join("sessions");
        let agent = root.join("agent with spaces");
        let directory =
            crate::pi_settings::pi_agent_directory(&root, Some(agent.as_os_str())).unwrap();
        assert_eq!(directory, agent);
        crate::pi_settings::store_pi_settings(&agent.join("settings.json"), PI_CANDIDATE_ARTIFACT)
            .unwrap();
        let config = crate::sidecar::pi_sidecar_config("pi.exe", &sessions, None).unwrap();
        assert_eq!(config.args[3], sessions.to_string_lossy());
        let npm = agent.join("npm");
        for (name, version) in PI_PACKAGES {
            let package = npm.join("node_modules").join(name);
            fs::create_dir_all(&package).unwrap();
            fs::write(
                package.join("package.json"),
                format!("{{\"version\":\"{version}\"}}"),
            )
            .unwrap();
        }
        fs::write(
            npm.join(".muniment-packages.json"),
            serde_json::to_vec(&PI_PACKAGES).unwrap(),
        )
        .unwrap();
        prepare_pi_packages(&agent, Path::new("pi.exe")).unwrap();
        let command = install_command(Path::new("pi.exe"), &npm.canonicalize().unwrap());
        assert_eq!(command.get_current_dir(), Some(npm.as_path()));
        assert_eq!(command.get_args().last().unwrap(), npm.as_os_str());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn acquisition_uses_embedded_bun_without_system_node_or_scripts() {
        let command = install_command(Path::new("/verified/pi"), Path::new("/agent/npm"));
        assert_eq!(command.get_program(), "/verified/pi");
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_str().unwrap())
            .collect();
        assert_eq!(
            args,
            [
                "install",
                "pi-web-access@0.28.0",
                "pi-subagents@0.65.1",
                "pi-background-tasks@2.5.0",
                "pi-mcp-adapter@2.32.1",
                "--omit=peer",
                "--ignore-scripts",
                "--exact",
                "--cwd",
                "/agent/npm"
            ]
        );
        assert!(command
            .get_envs()
            .any(|(key, value)| key == "BUN_BE_BUN" && value == Some("1".as_ref())));
    }

    #[test]
    fn partial_acquisition_retries_and_releases_the_lock() {
        let root = std::env::temp_dir().join(format!("muniment-packages-{}", uuid::Uuid::new_v4()));
        let npm = root.join("npm");
        for (name, version) in PI_PACKAGES {
            let directory = npm.join("node_modules").join(name);
            fs::create_dir_all(&directory).unwrap();
            fs::write(
                directory.join("package.json"),
                format!("{{\"version\":\"{version}\"}}"),
            )
            .unwrap();
        }
        let missing = root.join("missing-executable");
        assert!(prepare_pi_packages(&root, &missing).is_err());
        assert!(prepare_pi_packages(&root, &missing).is_err());
        let marker = npm.join(".muniment-packages.json");
        assert!(!marker.exists());
        fs::write(&marker, serde_json::to_vec(&PI_PACKAGES).unwrap()).unwrap();
        prepare_pi_packages(&root, &missing).unwrap();
        fs::write(npm.join("node_modules/pi-web-access/package.json"), "{}").unwrap();
        assert!(prepare_pi_packages(&root, &missing).is_err());
        assert!(!marker.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
