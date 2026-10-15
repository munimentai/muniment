//! Pairing and reaching a runtime through the system `ssh`.
//!
//! The app runs `ssh` with an argument vector, never through a shell on this
//! side. `--` ends the options before the host, so neither the host nor the
//! remote command can add an `ssh` option. Validation also refuses a host or
//! command that starts with `-`, and limits the remote command to characters
//! that a remote shell reads literally.

use std::ffi::OsStr;
use std::io::Read;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use muniment_core::attach::{CertificateFingerprint, PairingLink};

use super::registry::SshTarget;

pub(crate) const DEFAULT_COMMAND: &str = "muniment-server";
pub(crate) const DEFAULT_REMOTE_PORT: u16 = 7443;
const CONNECT_TIMEOUT_SECONDS: u32 = 10;
const PAIR_TIMEOUT: Duration = Duration::from_secs(45);
const TUNNEL_READY_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_HOST_LENGTH: usize = 255;
const MAX_COMMAND_LENGTH: usize = 256;
const MAX_COMMAND_WORDS: usize = 16;
const MAX_OUTPUT_BYTES: u64 = 64 * 1024;

/// Why an SSH step failed. No variant carries the pairing secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SshError {
    InvalidHost,
    InvalidCommand,
    InvalidPort,
    /// No `ssh` program is on the PATH.
    Unavailable,
    /// `ssh` itself failed, with its last error line.
    ConnectFailed(String),
    /// The remote command ran and failed.
    CommandFailed,
    TimedOut,
    /// The remote command answered something other than a pairing link.
    InvalidOutput,
    /// The link the remote command printed has already expired.
    Expired,
    /// The local forward did not open.
    TunnelFailed(String),
}

impl SshError {
    pub(crate) fn message(&self) -> String {
        match self {
            Self::InvalidHost => "Enter an SSH host that does not start with a dash.".into(),
            Self::InvalidCommand => {
                "Enter a remote command of plain words, letters, digits and . _ / @ : = + , -."
                    .into()
            }
            Self::InvalidPort => "Enter a port from 1 to 65535.".into(),
            Self::Unavailable => {
                "This computer has no ssh program. Install OpenSSH and try again.".into()
            }
            Self::ConnectFailed(detail) if detail.is_empty() => {
                "SSH could not sign in to the host. Check the host and your SSH keys.".into()
            }
            Self::ConnectFailed(detail) => format!("SSH could not sign in to the host: {detail}"),
            Self::CommandFailed => {
                "The remote command failed. Check the command and that the runtime is installed."
                    .into()
            }
            Self::TimedOut => "SSH did not answer in time. Check the host and try again.".into(),
            Self::InvalidOutput => "The remote command did not return a pairing link.".into(),
            Self::Expired => {
                "The pairing link expired. Check the host's clock and try again.".into()
            }
            Self::TunnelFailed(detail) if detail.is_empty() => {
                "The SSH tunnel did not open. Check the runtime port.".into()
            }
            Self::TunnelFailed(detail) => format!("The SSH tunnel did not open: {detail}"),
        }
    }
}

/// Checks an SSH host: a name, an `ssh_config` alias, `user@host` or an
/// address, with no space, no shell character and no leading dash.
pub(crate) fn validate_host(host: &str) -> Result<String, SshError> {
    if host.chars().any(char::is_control) {
        return Err(SshError::InvalidHost);
    }
    let host = host.trim();
    let valid = !host.is_empty()
        && host.len() <= MAX_HOST_LENGTH
        && !host.starts_with('-')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@' | ':' | '%'));
    if valid {
        Ok(host.to_owned())
    } else {
        Err(SshError::InvalidHost)
    }
}

/// Splits the remote command into its words. The words reach the remote
/// shell, so each holds only characters that shell reads literally.
pub(crate) fn command_words(command: &str) -> Result<Vec<String>, SshError> {
    if command.chars().any(char::is_control) {
        return Err(SshError::InvalidCommand);
    }
    let words: Vec<String> = command.split_whitespace().map(str::to_owned).collect();
    let valid = !words.is_empty()
        && command.len() <= MAX_COMMAND_LENGTH
        && words.len() <= MAX_COMMAND_WORDS
        && !words[0].starts_with('-')
        && words.iter().all(|word| {
            word.chars().all(|c| {
                c.is_ascii_alphanumeric()
                    || matches!(c, '.' | '_' | '/' | '@' | ':' | '=' | '+' | ',' | '-')
            })
        });
    if valid {
        Ok(words)
    } else {
        Err(SshError::InvalidCommand)
    }
}

pub(crate) fn validate_port(port: u32) -> Result<u16, SshError> {
    u16::try_from(port)
        .ok()
        .filter(|port| *port != 0)
        .ok_or(SshError::InvalidPort)
}

/// Checks a target the user entered and answers it in its stored form.
pub(crate) fn target(host: &str, command: &str, remote_port: u32) -> Result<SshTarget, SshError> {
    Ok(SshTarget {
        host: validate_host(host)?,
        command: command_words(command)?.join(" "),
        remote_port: validate_port(remote_port)?,
    })
}

fn common_options() -> Vec<String> {
    [
        "-T".to_owned(),
        "-o".to_owned(),
        "BatchMode=yes".to_owned(),
        "-o".to_owned(),
        format!("ConnectTimeout={CONNECT_TIMEOUT_SECONDS}"),
    ]
    .into()
}

/// `ssh -T -o BatchMode=yes -o ConnectTimeout=10 -- <host> <command…> pair --json`
pub(crate) fn pair_arguments(host: &str, words: &[String]) -> Vec<String> {
    let mut arguments = common_options();
    arguments.extend(["-o".into(), "ClearAllForwardings=yes".into()]);
    arguments.push("--".into());
    arguments.push(host.to_owned());
    arguments.extend(words.iter().cloned());
    arguments.extend(["pair".into(), "--json".into()]);
    arguments
}

/// `ssh -N -T … -o ExitOnForwardFailure=yes -L 127.0.0.1:<local>:127.0.0.1:<remote> -- <host>`
pub(crate) fn tunnel_arguments(host: &str, local_port: u16, remote_port: u16) -> Vec<String> {
    let mut arguments = vec!["-N".to_owned()];
    arguments.extend(common_options());
    arguments.extend([
        "-o".into(),
        "ExitOnForwardFailure=yes".into(),
        "-o".into(),
        "ServerAliveInterval=15".into(),
        "-o".into(),
        "ServerAliveCountMax=3".into(),
        "-L".into(),
        format!("127.0.0.1:{local_port}:127.0.0.1:{remote_port}"),
        "--".into(),
        host.to_owned(),
    ]);
    arguments
}

fn program_name() -> &'static str {
    if cfg!(windows) {
        "ssh.exe"
    } else {
        "ssh"
    }
}

/// Finds `ssh` on a PATH value.
pub(crate) fn find_program(path: Option<&OsStr>) -> Option<PathBuf> {
    std::env::split_paths(path?)
        .filter(|directory| directory.is_absolute())
        .map(|directory| directory.join(program_name()))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
}

/// What `<command> pair --json` answered: the link, checked against the
/// fingerprint, runtime id and expiry it printed beside it.
#[derive(Debug)]
pub(crate) struct PairOutput {
    pub(crate) link: PairingLink,
}

/// Reads the JSON object `pair --json` prints. The object names the link and
/// may also name the certificate fingerprint, the runtime id and the expiry.
pub(crate) fn parse_pair_output(stdout: &[u8]) -> Result<PairOutput, SshError> {
    let text = std::str::from_utf8(stdout).map_err(|_| SshError::InvalidOutput)?;
    let value = serde_json::from_str::<serde_json::Value>(text.trim())
        .ok()
        .or_else(|| {
            text.lines()
                .rev()
                .filter(|line| line.trim_start().starts_with('{'))
                .find_map(|line| serde_json::from_str(line.trim()).ok())
        })
        .filter(serde_json::Value::is_object)
        .ok_or(SshError::InvalidOutput)?;
    let field = |names: &[&str]| {
        names
            .iter()
            .find_map(|name| value.get(*name).and_then(serde_json::Value::as_str))
            .map(str::to_owned)
    };
    let link = field(&["link"])
        .and_then(|link| PairingLink::parse(&link).ok())
        .ok_or(SshError::InvalidOutput)?;
    if let Some(fingerprint) = field(&["fingerprint", "certificate_sha256"]) {
        if CertificateFingerprint::parse(&fingerprint) != Some(link.fingerprint) {
            return Err(SshError::InvalidOutput);
        }
    }
    if let Some(runtime_id) = field(&["runtime_id"]) {
        if !runtime_id.eq_ignore_ascii_case(link.runtime_id.as_str()) {
            return Err(SshError::InvalidOutput);
        }
    }
    if field(&["expires_at"])
        .as_deref()
        .and_then(|expiry| chrono::DateTime::parse_from_rfc3339(expiry).ok())
        .is_some_and(|expiry| expiry <= chrono::Utc::now())
    {
        return Err(SshError::Expired);
    }
    Ok(PairOutput { link })
}

/// The last line `ssh` wrote to stderr, bounded, for a connection failure.
fn error_line(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(|line| line.chars().filter(|c| !c.is_control()).take(160).collect())
        .unwrap_or_default()
}

#[derive(Clone, Debug)]
pub(crate) struct Ssh {
    program: PathBuf,
}

impl Ssh {
    pub(crate) fn new(program: PathBuf) -> Self {
        Self { program }
    }

    /// The `ssh` on this process's PATH.
    pub(crate) fn from_environment() -> Result<Self, SshError> {
        find_program(std::env::var_os("PATH").as_deref())
            .map(Self::new)
            .ok_or(SshError::Unavailable)
    }

    fn command(&self, arguments: &[String]) -> Command {
        let mut command = Command::new(&self.program);
        command.args(arguments).stdin(Stdio::null());
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        command
    }

    /// Runs `<command> pair --json` on the host and reads the link it prints.
    pub(crate) fn pair(&self, target: &SshTarget) -> Result<PairOutput, SshError> {
        let words = command_words(&target.command)?;
        let host = validate_host(&target.host)?;
        let child = self
            .command(&pair_arguments(&host, &words))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| SshError::Unavailable)?;
        let (status, stdout, stderr) = wait_with_output(child, PAIR_TIMEOUT)?;
        match status.code() {
            Some(0) => parse_pair_output(&stdout),
            Some(255) => Err(SshError::ConnectFailed(error_line(&stderr))),
            _ => Err(SshError::CommandFailed),
        }
    }

    /// Opens `ssh -N -L` from a free loopback port to the runtime port on the
    /// host, and answers once the local port accepts connections.
    pub(crate) fn open_tunnel(&self, host: &str, remote_port: u16) -> Result<SshTunnel, SshError> {
        let host = validate_host(host)?;
        let local_port = free_local_port().map_err(|_| SshError::TunnelFailed(String::new()))?;
        let mut child = self
            .command(&tunnel_arguments(&host, local_port, remote_port))
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| SshError::Unavailable)?;
        let deadline = Instant::now() + TUNNEL_READY_TIMEOUT;
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, local_port));
        loop {
            if let Ok(Some(_)) = child.try_wait() {
                let mut stderr = Vec::new();
                if let Some(pipe) = child.stderr.as_mut() {
                    let _ = pipe.take(MAX_OUTPUT_BYTES).read_to_end(&mut stderr);
                }
                return Err(SshError::TunnelFailed(error_line(&stderr)));
            }
            if TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_ok() {
                // Nothing reads ssh's stderr while the tunnel runs.
                drop(child.stderr.take());
                return Ok(SshTunnel { child, local_port });
            }
            if Instant::now() >= deadline {
                stop_child(&mut child);
                return Err(SshError::TimedOut);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// A running `ssh -N -L` process. Dropping it ends the process.
#[derive(Debug)]
pub(crate) struct SshTunnel {
    child: Child,
    local_port: u16,
}

impl SshTunnel {
    pub(crate) fn endpoint(&self) -> String {
        format!("127.0.0.1:{}", self.local_port)
    }

    pub(crate) fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for SshTunnel {
    fn drop(&mut self) {
        stop_child(&mut self.child);
    }
}

fn stop_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn free_local_port() -> std::io::Result<u16> {
    Ok(TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?
        .local_addr()?
        .port())
}

fn wait_with_output(
    mut child: Child,
    timeout: Duration,
) -> Result<(ExitStatus, Vec<u8>, Vec<u8>), SshError> {
    fn reader(pipe: Option<impl Read + Send + 'static>) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(pipe) = pipe {
                let _ = pipe.take(MAX_OUTPUT_BYTES).read_to_end(&mut bytes);
            }
            bytes
        })
    }
    let stdout = reader(child.stdout.take());
    let stderr = reader(child.stderr.take());
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                stop_child(&mut child);
                return Err(SshError::TimedOut);
            }
        }
    };
    Ok((
        status,
        stdout.join().unwrap_or_default(),
        stderr.join().unwrap_or_default(),
    ))
}

/// The PATH value with `directory` first, for a test's fake `ssh`.
#[cfg(test)]
pub(crate) fn path_with(directory: &Path) -> std::ffi::OsString {
    let mut paths = vec![directory.to_owned()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    std::env::join_paths(paths).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use muniment_core::attach::Id;

    fn link() -> PairingLink {
        PairingLink {
            runtime_id: Id::new("018f0000-0000-7000-8000-000000000042").unwrap(),
            runtime_name: "Build box".into(),
            endpoints: vec!["192.0.2.20:7443".into()],
            fingerprint: CertificateFingerprint::of_certificate(b"certificate"),
            secret: "ab".repeat(16),
        }
    }

    #[test]
    fn hosts_take_names_aliases_users_and_addresses_but_no_option() {
        for host in [
            "box",
            "build.example.net",
            "me@box",
            "192.0.2.20",
            "::1",
            "fe80::1%en0",
            "my_alias",
        ] {
            assert_eq!(validate_host(host).as_deref(), Ok(host), "{host}");
        }
        assert_eq!(validate_host("  box  ").as_deref(), Ok("box"));
        for host in [
            "",
            "-oProxyCommand=touch /tmp/x",
            "-p22",
            "box name",
            "box;rm",
            "box`id`",
            "$(id)",
            "box\n",
            "box'",
            "box\"",
            "box|cat",
            "box&",
            &"a".repeat(256),
        ] {
            assert_eq!(validate_host(host), Err(SshError::InvalidHost), "{host:?}");
        }
    }

    #[test]
    fn commands_are_plain_words_that_start_with_no_dash() {
        assert_eq!(
            command_words("muniment-server").unwrap(),
            ["muniment-server"]
        );
        assert_eq!(
            command_words("  docker exec muniment-runtime   muniment-server ").unwrap(),
            ["docker", "exec", "muniment-runtime", "muniment-server"]
        );
        assert_eq!(
            command_words("sudo -n /opt/muniment/bin/muniment-server").unwrap(),
            ["sudo", "-n", "/opt/muniment/bin/muniment-server"]
        );
        for command in [
            "",
            "   ",
            "-oProxyCommand=x",
            "muniment-server; rm -rf ~",
            "muniment-server && id",
            "muniment-server | cat",
            "muniment-server $(id)",
            "muniment-server `id`",
            "muniment-server 'quoted'",
            "muniment-server \"quoted\"",
            "muniment-server > out",
            "muniment-server\nid",
            "muniment-server ~",
            "muniment-server *",
        ] {
            assert_eq!(
                command_words(command),
                Err(SshError::InvalidCommand),
                "{command:?}"
            );
        }
        assert_eq!(
            command_words(&vec!["a"; 17].join(" ")),
            Err(SshError::InvalidCommand)
        );
        assert_eq!(validate_port(7443), Ok(7443));
        assert_eq!(validate_port(0), Err(SshError::InvalidPort));
        assert_eq!(validate_port(65536), Err(SshError::InvalidPort));
    }

    #[test]
    fn arguments_end_the_options_before_the_host() {
        let words = command_words("docker exec muniment-runtime muniment-server").unwrap();
        let pair = pair_arguments("box", &words);
        assert_eq!(
            pair,
            [
                "-T",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=10",
                "-o",
                "ClearAllForwardings=yes",
                "--",
                "box",
                "docker",
                "exec",
                "muniment-runtime",
                "muniment-server",
                "pair",
                "--json",
            ]
        );
        let tunnel = tunnel_arguments("me@box", 51000, 7443);
        assert_eq!(tunnel[0], "-N");
        assert!(tunnel.contains(&"ExitOnForwardFailure=yes".to_owned()));
        assert!(tunnel.contains(&"BatchMode=yes".to_owned()));
        let forward = tunnel.iter().position(|argument| argument == "-L").unwrap();
        assert_eq!(tunnel[forward + 1], "127.0.0.1:51000:127.0.0.1:7443");
        assert_eq!(&tunnel[tunnel.len() - 2..], ["--", "me@box"]);
    }

    #[test]
    fn pair_output_is_checked_against_its_link() {
        let link = link();
        let output = serde_json::json!({
            "link": link.to_uri(),
            "fingerprint": link.fingerprint.to_hex(),
            "runtime_id": link.runtime_id.as_str(),
            "expires_at": "2030-01-01T00:00:00Z",
        });
        let parsed = parse_pair_output(output.to_string().as_bytes()).unwrap();
        assert_eq!(parsed.link, link);
        assert!(!format!("{parsed:?}").contains(&"ab".repeat(16)));

        let noisy = format!("Welcome to the box\n{output}\n");
        assert_eq!(parse_pair_output(noisy.as_bytes()).unwrap().link, link);

        let mut wrong = output.clone();
        wrong["fingerprint"] = serde_json::json!("00".repeat(32));
        assert_eq!(
            parse_pair_output(wrong.to_string().as_bytes()).unwrap_err(),
            SshError::InvalidOutput
        );
        let mut wrong = output.clone();
        wrong["runtime_id"] = serde_json::json!("018f0000-0000-7000-8000-000000000099");
        assert_eq!(
            parse_pair_output(wrong.to_string().as_bytes()).unwrap_err(),
            SshError::InvalidOutput
        );
        let mut expired = output.clone();
        expired["expires_at"] = serde_json::json!("2001-01-01T00:00:00Z");
        assert_eq!(
            parse_pair_output(expired.to_string().as_bytes()).unwrap_err(),
            SshError::Expired
        );
        for text in ["", "not json", "{\"link\": \"https://example.com\"}", "[]"] {
            assert_eq!(
                parse_pair_output(text.as_bytes()).unwrap_err(),
                SshError::InvalidOutput
            );
        }
    }

    #[test]
    fn errors_carry_no_secret_and_name_the_next_step() {
        assert!(SshError::Unavailable.message().contains("OpenSSH"));
        assert_eq!(
            error_line(b"debug\nPermission denied (publickey).\n\n"),
            "Permission denied (publickey)."
        );
        assert_eq!(error_line(&[b'x'; 400]).len(), 160);
    }

    #[cfg(unix)]
    mod fake_ssh {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        /// A directory holding an `ssh` script that records its arguments,
        /// one per line, and then runs `body`.
        fn fake(body: &str) -> (PathBuf, PathBuf) {
            let directory =
                std::env::temp_dir().join(format!("muniment-fake-ssh-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir_all(&directory).unwrap();
            let log = directory.join("arguments");
            let script = format!(
                "#!/bin/sh\nfor argument in \"$@\"; do printf '%s\\n' \"$argument\" >> '{}'; done\n{body}\n",
                log.display()
            );
            let program = directory.join("ssh");
            std::fs::write(&program, script).unwrap();
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
            (directory, log)
        }

        fn found(directory: &Path) -> Ssh {
            let program = find_program(Some(&path_with(directory))).unwrap();
            assert_eq!(program, directory.join("ssh"));
            Ssh::new(program)
        }

        fn logged(log: &Path) -> Vec<String> {
            std::fs::read_to_string(log)
                .unwrap()
                .lines()
                .map(str::to_owned)
                .collect()
        }

        #[test]
        fn pairing_runs_the_command_and_reads_the_link() {
            let link = link();
            let output = serde_json::json!({
                "link": link.to_uri(),
                "fingerprint": link.fingerprint.to_hex(),
                "runtime_id": link.runtime_id.as_str(),
                "expires_at": "2030-01-01T00:00:00Z",
            });
            let (directory, log) = fake(&format!("printf '%s\\n' '{output}'"));
            let target = target(
                "me@box",
                "docker exec muniment-runtime muniment-server",
                7443,
            )
            .unwrap();
            let paired = found(&directory).pair(&target).unwrap();
            assert_eq!(paired.link, link);
            assert_eq!(
                logged(&log),
                pair_arguments(
                    "me@box",
                    &command_words("docker exec muniment-runtime muniment-server").unwrap()
                )
            );
            std::fs::remove_dir_all(directory).unwrap();
        }

        #[test]
        fn a_connection_failure_names_the_ssh_error_and_a_command_failure_does_not() {
            let (directory, _) = fake("echo 'Permission denied (publickey).' >&2\nexit 255");
            let target = target("box", DEFAULT_COMMAND, 7443).unwrap();
            assert_eq!(
                found(&directory).pair(&target).unwrap_err(),
                SshError::ConnectFailed("Permission denied (publickey).".into())
            );
            std::fs::remove_dir_all(directory).unwrap();

            let (directory, _) = fake("echo 'secret detail' >&2\nexit 2");
            assert_eq!(
                found(&directory).pair(&target).unwrap_err(),
                SshError::CommandFailed
            );
            std::fs::remove_dir_all(directory).unwrap();

            let (directory, _) = fake("echo 'not a link'");
            assert_eq!(
                found(&directory).pair(&target).unwrap_err(),
                SshError::InvalidOutput
            );
            std::fs::remove_dir_all(directory).unwrap();
        }

        #[test]
        fn a_tunnel_that_exits_reports_its_error_and_passes_the_forward() {
            let (directory, log) =
                fake("echo 'bind [127.0.0.1]: Address already in use' >&2\nexit 255");
            let error = found(&directory).open_tunnel("box", 7443).unwrap_err();
            assert_eq!(
                error,
                SshError::TunnelFailed("bind [127.0.0.1]: Address already in use".into())
            );
            let arguments = logged(&log);
            let forward = arguments
                .iter()
                .position(|argument| argument == "-L")
                .unwrap();
            assert!(arguments[forward + 1].starts_with("127.0.0.1:"));
            assert!(arguments[forward + 1].ends_with(":127.0.0.1:7443"));
            assert_eq!(&arguments[arguments.len() - 2..], ["--", "box"]);
            std::fs::remove_dir_all(directory).unwrap();
        }

        #[test]
        fn a_host_that_fails_validation_never_runs_ssh() {
            let (directory, log) = fake("exit 0");
            let ssh = found(&directory);
            let target = SshTarget {
                host: "-oProxyCommand=touch".into(),
                command: DEFAULT_COMMAND.into(),
                remote_port: 7443,
            };
            assert_eq!(ssh.pair(&target).unwrap_err(), SshError::InvalidHost);
            assert_eq!(
                ssh.open_tunnel("-oProxyCommand=touch", 7443).unwrap_err(),
                SshError::InvalidHost
            );
            assert!(!log.exists());
            std::fs::remove_dir_all(directory).unwrap();
        }

        #[test]
        fn a_path_without_ssh_finds_none() {
            let directory =
                std::env::temp_dir().join(format!("muniment-no-ssh-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir_all(&directory).unwrap();
            assert_eq!(find_program(Some(directory.as_os_str())), None);
            assert_eq!(find_program(None), None);
            std::fs::remove_dir_all(directory).unwrap();
        }
    }
}
