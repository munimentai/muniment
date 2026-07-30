use muniment_attach::{
    handshake_as_with_credential, ClientError, Id, PendingPermission, PermissionDecision,
    RedactedRunEvent, RunStreamMessage, ThreadListPage, ThreadOpenPage,
};
use std::ffi::OsString;
use std::io::{self, BufRead, IsTerminal, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

const HELP: &str = "\
Usage: muniment [--workspace <directory>] <command>

Commands:
  threads list
  threads open <thread-id>
  run start
  workspace init

Options:
  -h, --help  Print help
  --version   Print version";

enum Command {
    List,
    Open(String),
    StartRun,
}

#[derive(Debug)]
enum CliError {
    Usage,
    InvalidThreadId,
    NonInteractive,
    PromptRequired,
    PermissionAnswerRequired,
    RunFailed,
    Client(ClientError),
    RunClient(ClientError),
    PermissionClient(ClientError),
    Workspace,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("muniment: {}", guidance(&error));
        if matches!(error, CliError::Usage) {
            eprintln!("{HELP}");
        }
        std::process::exit(1);
    }
}

fn run() -> Result<(), CliError> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let mut stdout = io::stdout();
    run_args(args, &mut stdout, run_command)
}

fn run_args(
    args: Vec<OsString>,
    output: &mut impl Write,
    run_command: impl FnOnce(Vec<OsString>) -> Result<(), CliError>,
) -> Result<(), CliError> {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        writeln!(output, "{HELP}").map_err(|_| CliError::Workspace)?;
        return Ok(());
    }
    if args == [OsString::from("--version")] {
        writeln!(output, "{}", env!("CARGO_PKG_VERSION")).map_err(|_| CliError::Workspace)?;
        return Ok(());
    }
    run_command(args)
}

fn run_command(mut args: Vec<OsString>) -> Result<(), CliError> {
    let opened = std::env::current_dir().map_err(|_| CliError::Workspace)?;
    let workspace = workspace_argument(&mut args)?.unwrap_or_else(|| opened.clone());
    if !recognized_command(&args) {
        return Err(CliError::Usage);
    }
    let client_identity = authorized_client_identity()?;
    let client_credential = authorized_client_credential()?;
    if args == [OsString::from("workspace"), OsString::from("init")] {
        let mut client = handshake_as_with_credential(
            env!("CARGO_PKG_VERSION"),
            &client_identity,
            client_credential.as_deref(),
            || {},
        )
        .map_err(CliError::Client)?;
        persist_authorized_client_credential(client.authorized_client_credential())?;
        client
            .onboard_workspace(&opened.to_string_lossy(), &workspace.to_string_lossy())
            .map_err(CliError::Client)?;
        return Ok(());
    }
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut stdout = io::stdout();
    run_with(
        &args,
        &workspace.to_string_lossy(),
        io::stdin().is_terminal(),
        io::stdout().is_terminal(),
        &mut input,
        &mut stdout,
        |pairing_pending| {
            let mut client = handshake_as_with_credential(
                env!("CARGO_PKG_VERSION"),
                &client_identity,
                client_credential.as_deref(),
                pairing_pending,
            )?;
            persist_authorized_client_credential(client.authorized_client_credential())
                .map_err(|_| ClientError::UnexpectedMessage)?;
            client.onboard_workspace(&opened.to_string_lossy(), &workspace.to_string_lossy())?;
            if args.first().is_some_and(|command| command == "threads") {
                client.ensure_home()?;
            }
            Ok(client)
        },
    )
}

fn authorized_client_identity() -> Result<String, CliError> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .filter(|path| path.is_absolute())
        .ok_or(CliError::Workspace)?;
    let directory = base.join("muniment");
    let path = directory.join("cli-client-id");
    match read_private_file(&path) {
        Ok(value) => {
            let value = value.trim();
            Id::new(value).map_err(|_| CliError::Workspace)?;
            return Ok(value.to_owned());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(CliError::Workspace),
    }
    std::fs::create_dir_all(&directory).map_err(|_| CliError::Workspace)?;
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|_| CliError::Workspace)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let value = format!("{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    match options
        .open(&path)
        .and_then(|mut file| file.write_all(value.as_bytes()))
    {
        Ok(()) => Ok(value),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let existing = read_private_file(&path).map_err(|_| CliError::Workspace)?;
            let existing = existing.trim();
            Id::new(existing).map_err(|_| CliError::Workspace)?;
            Ok(existing.to_owned())
        }
        Err(_) => Err(CliError::Workspace),
    }
}

fn authorized_client_credential() -> Result<Option<String>, CliError> {
    let Some(base) = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .filter(|path| path.is_absolute())
    else {
        return Err(CliError::Workspace);
    };
    let path = base.join("muniment").join("cli-client-credential");
    match read_private_file(&path) {
        Ok(value)
            if value.trim().len() == 64
                && value.trim().bytes().all(|byte| byte.is_ascii_hexdigit()) =>
        {
            Ok(Some(value.trim().to_owned()))
        }
        Ok(_) => Err(CliError::Workspace),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(CliError::Workspace),
    }
}

fn persist_authorized_client_credential(credential: &str) -> Result<(), CliError> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .filter(|path| path.is_absolute())
        .ok_or(CliError::Workspace)?;
    let directory = base.join("muniment");
    std::fs::create_dir_all(&directory).map_err(|_| CliError::Workspace)?;
    let path = directory.join("cli-client-credential");
    atomic_write_private_file(&path, credential.as_bytes()).map_err(|_| CliError::Workspace)
}

#[cfg(unix)]
fn read_private_file(path: &Path) -> io::Result<String> {
    let mut options = std::fs::OpenOptions::new();
    options
        .read(true)
        .custom_flags(unix_abi::O_NOFOLLOW | unix_abi::O_NONBLOCK);
    let mut file = options.open(path)?;
    let metadata = file.metadata()?;
    if !private_file_metadata_is_valid(&metadata, unix_abi::effective_uid()) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "client authorization file is not private",
        ));
    }
    let mut value = String::new();
    file.read_to_string(&mut value)?;
    Ok(value)
}

#[cfg(unix)]
fn private_file_metadata_is_valid(metadata: &std::fs::Metadata, effective_uid: u32) -> bool {
    metadata.is_file() && metadata.uid() == effective_uid && metadata.mode() & 0o077 == 0
}

#[cfg(unix)]
fn atomic_write_private_file(path: &Path, value: &[u8]) -> io::Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing parent directory"))?;
    let mut random = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
    let temporary = directory.join(format!(
        ".cli-client-credential.{}.tmp",
        random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ));
    let mut options = std::fs::OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(unix_abi::O_NOFOLLOW);
    let result = (|| {
        let mut file = options.open(&temporary)?;
        file.write_all(value)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(unix)]
mod unix_abi {
    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "solaris",
        target_os = "illumos"
    ))]
    pub const O_NOFOLLOW: i32 = 0x20000;
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    pub const O_NOFOLLOW: i32 = 0x100;
    pub const O_NONBLOCK: i32 = 0x4;

    unsafe extern "C" {
        fn geteuid() -> u32;
        #[cfg(test)]
        fn chown(path: *const std::ffi::c_char, owner: u32, group: u32) -> i32;
    }

    pub fn effective_uid() -> u32 {
        // SAFETY: geteuid takes no arguments and has no preconditions.
        unsafe { geteuid() }
    }

    #[cfg(test)]
    pub unsafe fn change_owner(path: *const std::ffi::c_char, owner: u32, group: u32) -> i32 {
        // SAFETY: the caller supplies the NUL-terminated path required by chown.
        unsafe { chown(path, owner, group) }
    }
}

#[cfg(not(unix))]
fn read_private_file(path: &Path) -> io::Result<String> {
    std::fs::read_to_string(path)
}

#[cfg(not(unix))]
fn atomic_write_private_file(path: &Path, value: &[u8]) -> io::Result<()> {
    std::fs::write(path, value)
}

fn recognized_command(args: &[OsString]) -> bool {
    matches!(args,
        [first, second] if (first == "threads" && second == "list")
            || (first == "run" && second == "start")
            || (first == "workspace" && second == "init")
    ) || matches!(args, [first, second, _] if first == "threads" && second == "open")
}

fn workspace_argument(args: &mut Vec<OsString>) -> Result<Option<PathBuf>, CliError> {
    let positions = args
        .iter()
        .enumerate()
        .filter_map(|(index, value)| (value == "--workspace").then_some(index))
        .collect::<Vec<_>>();
    if positions.len() > 1 {
        return Err(CliError::Usage);
    }
    let Some(index) = positions.first().copied() else {
        return Ok(None);
    };
    if index + 1 >= args.len() || args[index + 1] == "--workspace" {
        return Err(CliError::Usage);
    }
    let path = PathBuf::from(args.remove(index + 1));
    args.remove(index);
    if !path.is_absolute() {
        return Err(CliError::Usage);
    }
    Ok(Some(path))
}

fn run_with(
    args: &[OsString],
    workspace: &str,
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
    input: &mut impl BufRead,
    output: &mut impl Write,
    connect: impl FnOnce(&mut dyn FnMut()) -> Result<muniment_attach::AuthorizedClient, ClientError>,
) -> Result<(), CliError> {
    let command = match args {
        [threads, list] if threads == "threads" && list == "list" => Command::List,
        [threads, open, thread_id] if threads == "threads" && open == "open" => Command::Open(
            thread_id
                .to_str()
                .filter(|thread_id| !thread_id.is_empty() && thread_id.len() <= 36)
                .ok_or(CliError::InvalidThreadId)?
                .to_owned(),
        ),
        [run, start] if run == "run" && start == "start" => Command::StartRun,
        _ => return Err(CliError::Usage),
    };
    if !stdin_is_terminal || !stdout_is_terminal {
        return Err(CliError::NonInteractive);
    }
    let prompt = if matches!(command, Command::StartRun) {
        write!(output, "Prompt: ")
            .and_then(|_| output.flush())
            .map_err(|_| CliError::RunClient(ClientError::ConnectionClosed))?;
        let mut prompt = String::new();
        if input
            .read_line(&mut prompt)
            .map_err(|_| CliError::PromptRequired)?
            == 0
            || prompt.trim().is_empty()
        {
            return Err(CliError::PromptRequired);
        }
        Some(prompt.trim_end_matches(['\r', '\n']).to_owned())
    } else {
        None
    };
    let mut pairing_pending = || {
        writeln!(output, "Pairing requested. Approve the named ‘muniment CLI’ connection in the Muniment desktop.").ok();
    };
    let mut client = connect(&mut pairing_pending).map_err(|error| match command {
        Command::StartRun => CliError::RunClient(error),
        _ => CliError::Client(error),
    })?;
    if let Some(prompt) = prompt {
        let accepted = client
            .start_run_in_workspace(&prompt, None, Some(workspace))
            .map_err(CliError::RunClient)?;
        writeln!(output, "Run committed: {}", one_line(&accepted.run_id))
            .map_err(|_| CliError::RunClient(ClientError::ConnectionClosed))?;
        client
            .subscribe_run(&accepted.run_id, accepted.committed_seq)
            .map_err(CliError::RunClient)?;
        loop {
            let message = client
                .read_run_stream_message()
                .map_err(CliError::RunClient)?;
            let (run_seq, terminal) = match message {
                RunStreamMessage::Event(event) => {
                    writeln!(output, "{}", render_run_event(&event))
                        .map_err(|_| CliError::RunClient(ClientError::ConnectionClosed))?;
                    if let Some(receipt) = &event.receipt {
                        writeln!(output, "{}", render_receipt(receipt))
                            .map_err(|_| CliError::RunClient(ClientError::ConnectionClosed))?;
                    }
                    (event.run_seq, terminal_result(&event.event_type))
                }
                RunStreamMessage::PermissionPending(permission) => {
                    let decision = prompt_permission(input, output, &permission)?;
                    client
                        .answer_permission(&accepted.run_id, &permission.gate_id, decision)
                        .map_err(CliError::PermissionClient)?;
                    (permission.run_seq, None)
                }
                RunStreamMessage::CaughtUp { .. } => continue,
            };
            client
                .acknowledge_run_cursor(run_seq)
                .map_err(CliError::RunClient)?;
            if let Some(result) = terminal {
                return result;
            }
        }
    }
    let mut cursor = None;
    loop {
        let (rendered, next_cursor) = match &command {
            Command::List => {
                let page = client
                    .list_threads(cursor.as_deref())
                    .map_err(CliError::Client)?;
                (render_page(&page), page.next_cursor)
            }
            Command::Open(thread_id) => {
                let page = client
                    .open_thread(thread_id, cursor.as_deref())
                    .map_err(CliError::Client)?;
                (render_open_page(&page), page.next_cursor)
            }
            Command::StartRun => unreachable!("run start returns after its receipt"),
        };
        write!(output, "{rendered}")
            .map_err(|_| CliError::Client(ClientError::ConnectionClosed))?;
        let Some(next_cursor) = next_cursor else {
            return Ok(());
        };
        cursor = Some(next_cursor);
        loop {
            write!(output, "Press Enter for the next page, or q to stop: ")
                .and_then(|_| output.flush())
                .map_err(|_| CliError::Client(ClientError::ConnectionClosed))?;
            let mut response = String::new();
            if input
                .read_line(&mut response)
                .map_err(|_| CliError::Client(ClientError::ConnectionClosed))?
                == 0
                || response.trim().eq_ignore_ascii_case("q")
            {
                return Ok(());
            }
            if response.trim().is_empty() {
                break;
            }
        }
    }
}

fn guidance(error: &CliError) -> &'static str {
    match error {
        CliError::Usage => "unsupported arguments",
        CliError::InvalidThreadId => "provide a valid thread ID from `muniment threads list`",
        CliError::NonInteractive => "commands require interactive stdin and stdout",
        CliError::PromptRequired => "enter one non-empty prompt, then try again",
        CliError::PermissionAnswerRequired => {
            "answer the pending permission with allow or deny, then try again"
        }
        CliError::RunFailed => "the run failed; check the Muniment desktop, then try again",
        CliError::Workspace => "the workspace memory directory could not be initialized",
        CliError::RunClient(ClientError::RequestRejected) => {
            "the desktop rejected the run request; retry, then check the desktop"
        }
        CliError::PermissionClient(ClientError::RequestRejected) => {
            "the desktop rejected the permission answer; retry the run, then check the desktop"
        }
        CliError::Client(ClientError::RequestRejected) => {
            "the desktop rejected the thread request; check the thread ID, retry, then update Muniment if it continues"
        }
        CliError::Client(ClientError::DesktopFailed) => {
            "the desktop could not read threads; retry, then check the desktop"
        }
        CliError::RunClient(ClientError::DesktopFailed) => {
            "the desktop could not start the run; retry, then check the desktop"
        }
        CliError::PermissionClient(ClientError::DesktopFailed) => {
            "the desktop could not record the permission answer; retry the run, then check the desktop"
        }
        CliError::Client(error)
        | CliError::RunClient(error)
        | CliError::PermissionClient(error) => client_guidance(error),
    }
}

fn prompt_permission(
    input: &mut impl BufRead,
    output: &mut impl Write,
    permission: &PendingPermission,
) -> Result<PermissionDecision, CliError> {
    writeln!(
        output,
        "Permission required: {}",
        one_line(&permission.title)
    )
    .map_err(|_| CliError::RunClient(ClientError::ConnectionClosed))?;
    if let Some(message) = &permission.message {
        writeln!(output, "{}", one_line(message))
            .map_err(|_| CliError::RunClient(ClientError::ConnectionClosed))?;
    }
    loop {
        write!(output, "Allow? [y/n]: ")
            .and_then(|_| output.flush())
            .map_err(|_| CliError::RunClient(ClientError::ConnectionClosed))?;
        let mut answer = String::new();
        if input
            .read_line(&mut answer)
            .map_err(|_| CliError::PermissionAnswerRequired)?
            == 0
        {
            return Err(CliError::PermissionAnswerRequired);
        }
        match answer.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" | "allow" => return Ok(PermissionDecision::Allow),
            "n" | "no" | "deny" => return Ok(PermissionDecision::Deny),
            _ => writeln!(output, "Enter y to allow or n to deny.")
                .map_err(|_| CliError::RunClient(ClientError::ConnectionClosed))?,
        }
    }
}

fn client_guidance(error: &ClientError) -> &'static str {
    match error {
        ClientError::DesktopUnavailable
        | ClientError::RuntimeDirectoryMissing
        | ClientError::RuntimeDirectoryRelative => "open the Muniment desktop, then try again",
        ClientError::ConnectionClosed => {
            "pairing was denied or closed; retry and approve the CLI in the desktop"
        }
        ClientError::Timeout => "the desktop timed out; retry and keep the desktop open",
        ClientError::AuthorizationExpired => {
            "authorization expired; retry and approve the CLI in the desktop"
        }
        ClientError::DesktopFailed => "the desktop request failed; retry, then check the desktop",
        ClientError::ProtocolIncompatible => {
            "the desktop and CLI are incompatible; update Muniment desktop and the CLI"
        }
        ClientError::UnsupportedPlatform => "desktop attach is not supported on this platform",
        _ => "the desktop pairing response was invalid; update Muniment and try again",
    }
}

fn render_page(page: &ThreadListPage) -> String {
    use std::fmt::Write as _;
    let mut output = String::new();
    if page.threads.is_empty() {
        writeln!(output, "No threads found.").unwrap();
    } else {
        writeln!(output, "TITLE\tUPDATED\tTHREAD ID").unwrap();
        for thread in &page.threads {
            writeln!(
                output,
                "{}\t{}\t{}",
                one_line(&thread.title),
                one_line(&thread.updated_at),
                one_line(&thread.thread_id)
            )
            .unwrap();
        }
    }
    output
}

fn render_open_page(page: &ThreadOpenPage) -> String {
    use std::fmt::Write as _;
    let mut output = String::new();
    let mut entries: Vec<_> = page.entries.iter().collect();
    entries.sort_by_key(|entry| entry.run_seq);
    for entry in entries {
        if let Some(text) = &entry.text {
            writeln!(
                output,
                "{}\t{}\t{}",
                entry.run_seq,
                one_line(&entry.kind),
                one_line(text)
            )
            .unwrap();
        } else {
            writeln!(output, "{}\t{}", entry.run_seq, one_line(&entry.kind)).unwrap();
        }
    }
    output
}

fn render_run_event(event: &RedactedRunEvent) -> String {
    let detail = match event.event_type.as_str() {
        "tool.requested" => "Tool queued",
        "tool.effect.started" => "Tool running",
        "tool.effect.completed" => "Tool completed",
        "tool.effect.failed" => "Tool failed",
        _ => {
            return format!(
                "{}\t{}\t{}",
                event.run_seq,
                one_line(&event.event_type),
                one_line(&event.recorded_at)
            );
        }
    };
    format!(
        "{}\t{}\t{}\t{}",
        event.run_seq,
        one_line(&event.event_type),
        one_line(&event.recorded_at),
        detail
    )
}

fn render_receipt(receipt: &muniment_attach::RunReceipt) -> String {
    let mut fields = Vec::new();
    for (label, value) in [
        ("route", &receipt.route),
        ("model", &receipt.model),
        ("cost", &receipt.cost),
        ("time", &receipt.time),
    ] {
        if let Some(value) = value {
            fields.push(format!("{label}={}", one_line(value)));
        }
    }
    if !receipt.capabilities.is_empty() {
        fields.push(format!(
            "capabilities={}",
            receipt
                .capabilities
                .iter()
                .map(|capability| format!(
                    "{}@{}",
                    one_line(&capability.name),
                    one_line(&capability.version)
                ))
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    if fields.is_empty() {
        "Receipt".to_owned()
    } else {
        format!("Receipt\t{}", fields.join("\t"))
    }
}

fn terminal_result(event_type: &str) -> Option<Result<(), CliError>> {
    match event_type {
        "run.completed" | "run.cancelled" => Some(Ok(())),
        "run.failed" => Some(Err(CliError::RunFailed)),
        _ => None,
    }
}

fn one_line(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use muniment_attach::{
        authorized, encode_frame, handshake_stream, welcome, ErrorEnvelope, Event, EventName,
        Failure, Id, Protocol, ProtocolError, RedactedThreadSummary, Response, Success,
    };
    use std::collections::{BTreeMap, BTreeSet};
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    #[cfg(unix)]
    fn private_test_directory(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "muniment-cli-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        directory
    }

    #[cfg(unix)]
    #[test]
    fn private_client_files_reject_symlinks_non_regular_and_unsafe_metadata() {
        use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};

        let directory = private_test_directory("unsafe-files");
        let target = directory.join("target");
        std::fs::write(&target, "secret").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        let link = directory.join("link");
        symlink(&target, &link).unwrap();
        assert!(read_private_file(&link).is_err());
        assert!(read_private_file(Path::new("/dev/null")).is_err());

        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
        assert!(read_private_file(&target).is_err());
        let metadata = std::fs::metadata(&target).unwrap();
        assert!(!private_file_metadata_is_valid(
            &metadata,
            metadata.uid().wrapping_add(1)
        ));
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        let wrong_uid = metadata.uid().wrapping_add(1);
        let target_bytes = std::ffi::CString::new(target.as_os_str().as_encoded_bytes()).unwrap();
        if unsafe { unix_abi::change_owner(target_bytes.as_ptr(), wrong_uid, metadata.gid()) } == 0
        {
            assert!(read_private_file(&target).is_err());
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn private_client_file_persistence_is_owner_only_and_does_not_follow_destination() {
        use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};

        let directory = private_test_directory("atomic-persistence");
        let target = directory.join("unrelated");
        std::fs::write(&target, "preserve me").unwrap();
        let credential = directory.join("cli-client-credential");
        symlink(&target, &credential).unwrap();

        atomic_write_private_file(&credential, b"new credential").unwrap();

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "preserve me");
        assert_eq!(read_private_file(&credential).unwrap(), "new credential");
        let metadata = std::fs::metadata(&credential).unwrap();
        assert!(metadata.is_file());
        assert_eq!(metadata.uid(), unix_abi::effective_uid());
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn user_guidance_covers_expected_failures_without_details() {
        assert!(guidance(&CliError::Client(ClientError::DesktopUnavailable)).contains("open"));
        assert!(guidance(&CliError::Client(ClientError::ConnectionClosed)).contains("denied"));
        assert!(guidance(&CliError::Client(ClientError::ProtocolIncompatible)).contains("update"));
        let rejected = guidance(&CliError::Client(ClientError::RequestRejected));
        assert!(rejected.contains("thread request"));
        assert!(rejected.contains("thread ID"));
        assert!(rejected.contains("retry"));
        let rejected = guidance(&CliError::PermissionClient(ClientError::RequestRejected));
        assert!(rejected.contains("permission answer"));
        assert!(rejected.contains("retry"));
    }

    #[test]
    fn rendering_covers_empty_and_page_present_without_cursor_details() {
        assert_eq!(
            render_page(&ThreadListPage {
                threads: vec![],
                next_cursor: None
            }),
            "No threads found.\n"
        );
        let output = render_page(&ThreadListPage {
            threads: vec![RedactedThreadSummary {
                thread_id: "opaque-id".into(),
                title: "A\nTitle".into(),
                updated_at: "2026-07-17T00:00:00Z".into(),
            }],
            next_cursor: Some("secret-cursor".into()),
        });
        assert!(output.contains("A Title\t2026-07-17T00:00:00Z\topaque-id"));
        assert!(!output.contains("secret-cursor"));
    }

    #[test]
    fn open_rendering_is_sequence_ordered_terminal_safe_and_redacted() {
        let output = render_open_page(&ThreadOpenPage {
            thread_id: "thread-1".into(),
            entries: vec![
                muniment_attach::RedactedThreadEntry {
                    run_seq: 2,
                    kind: "tool\u{1b}[31m".into(),
                    text: None,
                },
                muniment_attach::RedactedThreadEntry {
                    run_seq: 1,
                    kind: "message".into(),
                    text: Some("hello\nworld\t!".into()),
                },
            ],
            next_cursor: Some("private-cursor".into()),
        });
        assert_eq!(output, "1\tmessage\thello world !\n2\ttool [31m\n");
        assert!(!output.contains("thread-1"));
        assert!(!output.contains("private-cursor"));
        assert!(!output.contains('\u{1b}'));
    }

    #[test]
    fn run_terminal_outcomes_have_the_expected_exit_results() {
        assert!(terminal_result("run.completed").unwrap().is_ok());
        assert!(terminal_result("run.cancelled").unwrap().is_ok());
        let failed = terminal_result("run.failed").unwrap().unwrap_err();
        assert!(matches!(failed, CliError::RunFailed));
        assert!(guidance(&failed).contains("run failed"));
        assert!(terminal_result("assistant.message").is_none());
    }

    #[test]
    fn run_rendering_adds_bounded_inline_tool_lifecycle_details() {
        let event = |run_seq, event_type: &str| RedactedRunEvent {
            run_seq,
            event_type: event_type.into(),
            event_version: 1,
            recorded_at: "2026-07-18T15:50:00Z".into(),
            receipt: None,
        };
        for (event_type, detail) in [
            ("tool.requested", "Tool queued"),
            ("tool.effect.started", "Tool running"),
            ("tool.effect.completed", "Tool completed"),
            ("tool.effect.failed", "Tool failed"),
        ] {
            assert_eq!(
                render_run_event(&event(4, event_type)),
                format!("4\t{event_type}\t2026-07-18T15:50:00Z\t{detail}")
            );
        }
        assert_eq!(
            render_run_event(&event(5, "tool.effect.future\n\u{1b}[31m")),
            "5\ttool.effect.future  [31m\t2026-07-18T15:50:00Z"
        );
    }

    #[test]
    fn receipt_rendering_prints_only_authoritative_present_fields_and_is_terminal_safe() {
        let receipt = muniment_attach::RunReceipt {
            route: Some("cloud\nroute".into()),
            model: None,
            cost: Some("$0.01".into()),
            time: None,
            capabilities: vec![muniment_attach::ReceiptCapability {
                name: "web\u{1b}[31m".into(),
                version: "1\tstable".into(),
            }],
        };
        assert_eq!(
            render_receipt(&receipt),
            "Receipt\troute=cloud route\tcost=$0.01\tcapabilities=web [31m@1 stable"
        );
        assert!(!render_receipt(&receipt).contains("model="));
        assert!(!render_receipt(&receipt).contains("time="));
        assert!(!render_receipt(&receipt).contains('\u{1b}'));
    }

    #[test]
    fn permission_prompt_is_terminal_safe_requires_a_choice_and_supports_both_decisions() {
        let permission = PendingPermission {
            run_seq: 8,
            gate_id: "private-gate".into(),
            kind: muniment_attach::PermissionKind::Confirm,
            title: "Use camera\nnow\u{1b}[31m".into(),
            message: Some("Needed\tfor capture".into()),
        };
        for (input_bytes, expected) in [
            (b"maybe\nyes\n".as_slice(), PermissionDecision::Allow),
            (b"deny\n".as_slice(), PermissionDecision::Deny),
        ] {
            let mut input = io::Cursor::new(input_bytes);
            let mut output = Vec::new();
            assert_eq!(
                prompt_permission(&mut input, &mut output, &permission).unwrap(),
                expected
            );
            let output = String::from_utf8(output).unwrap();
            assert!(output.contains("Permission required: Use camera now [31m"));
            assert!(output.contains("Needed for capture"));
            assert!(!output.contains("private-gate"));
            assert!(!output.contains('\u{1b}'));
        }
        let error = prompt_permission(
            &mut io::Cursor::new(Vec::<u8>::new()),
            &mut Vec::new(),
            &permission,
        )
        .unwrap_err();
        assert!(matches!(error, CliError::PermissionAnswerRequired));
    }

    fn read_frame(stream: &mut UnixStream) -> serde_json::Value {
        let mut prefix = [0; 4];
        stream.read_exact(&mut prefix).unwrap();
        let mut payload = vec![0; u32::from_be_bytes(prefix) as usize];
        stream.read_exact(&mut payload).unwrap();
        serde_json::from_slice(&payload).unwrap()
    }

    fn paired_output(body: serde_json::Value) -> String {
        let (client, mut server) = UnixStream::pair().unwrap();
        let worker = std::thread::spawn(move || {
            read_frame(&mut server);
            server
                .write_all(
                    &encode_frame(&welcome(
                        1,
                        "server-detail",
                        "11".repeat(16),
                        "22".repeat(16),
                    ))
                    .unwrap(),
                )
                .unwrap();
            server
                .write_all(
                    &encode_frame(&authorized(
                        "deadcafe".repeat(8),
                        3600,
                        900,
                        BTreeMap::from([(
                            "private-path".into(),
                            BTreeSet::from(["thread.read".into()]),
                        )]),
                    ))
                    .unwrap(),
                )
                .unwrap();
            let request = read_frame(&mut server);
            let response = Response {
                protocol: Protocol,
                request_id: Id::new(request["request_id"].as_str().unwrap()).unwrap(),
                ok: Success,
                body,
            };
            for byte in encode_frame(&response).unwrap() {
                server.write_all(&[byte]).unwrap();
            }
        });
        let mut output = Vec::new();
        let mut input = io::Cursor::new(Vec::<u8>::new());
        run_with(
            &["threads".into(), "list".into()],
            "/workspace",
            true,
            true,
            &mut input,
            &mut output,
            |pending| {
                handshake_stream(
                    client,
                    "0.0.1",
                    Duration::from_secs(1),
                    Duration::from_secs(1),
                    pending,
                )
            },
        )
        .unwrap();
        worker.join().unwrap();
        String::from_utf8(output).unwrap()
    }

    #[test]
    fn paired_fragmented_pages_reach_human_output_without_private_values() {
        let empty = paired_output(serde_json::json!({"threads": []}));
        assert!(empty.contains("No threads found."));
        assert!(!empty.contains("Press Enter"));

        let page = paired_output(serde_json::json!({
            "threads": [{"thread_id": "opaque-id", "title": "Visible title", "updated_at": "2026-07-17T00:00:00Z"}],
            "next_cursor": "private-cursor"
        }));
        assert!(page.contains("Visible title\t2026-07-17T00:00:00Z\topaque-id"));
        assert!(page.contains("Press Enter for the next page, or q to stop"));
        for private in [
            "deadcafe",
            "private-path",
            "private-cursor",
            "server-detail",
        ] {
            assert!(!empty.contains(private));
            assert!(!page.contains(private));
        }
    }

    #[test]
    fn run_start_follows_catch_up_and_live_events_on_one_redacted_pairing() {
        let (client, mut server) = UnixStream::pair().unwrap();
        let run_id = "123e4567-e89b-12d3-a456-426614174000";
        let subscription_id = "123e4567-e89b-12d3-a456-426614174001";
        let worker = std::thread::spawn(move || {
            read_frame(&mut server);
            server
                .write_all(
                    &encode_frame(&welcome(
                        1,
                        "private-server",
                        "11".repeat(16),
                        "22".repeat(16),
                    ))
                    .unwrap(),
                )
                .unwrap();
            server
                .write_all(
                    &encode_frame(&authorized(
                        "deadcafe".repeat(8),
                        3600,
                        900,
                        BTreeMap::new(),
                    ))
                    .unwrap(),
                )
                .unwrap();
            let request = read_frame(&mut server);
            assert_eq!(request["operation"], "run.start");
            assert_eq!(
                request["body"],
                serde_json::json!({"text": "ship it", "workspace": "/workspace"})
            );
            assert!(request["idempotency_key"].is_string());
            let response = Response {
                protocol: Protocol,
                request_id: Id::new(request["request_id"].as_str().unwrap()).unwrap(),
                ok: Success,
                body: serde_json::json!({
                    "run_id": run_id,
                    "committed_seq": 7,
                    "accepted_at": "2026-07-17T12:00:00Z"
                }),
            };
            server.write_all(&encode_frame(&response).unwrap()).unwrap();
            let subscribe = read_frame(&mut server);
            assert_eq!(subscribe["operation"], "run.stream");
            assert_eq!(
                subscribe["body"],
                serde_json::json!({"run_id": run_id, "after_run_seq": 7})
            );
            server
                .write_all(
                    &encode_frame(&Response {
                        protocol: Protocol,
                        request_id: Id::new(subscribe["request_id"].as_str().unwrap()).unwrap(),
                        ok: Success,
                        body: serde_json::json!({
                            "subscription_id": subscription_id,
                            "run_id": run_id,
                            "first_available_run_seq": 1,
                            "current_run_seq": 8,
                            "window": {"max_events": 2, "max_bytes": 4096}
                        }),
                    })
                    .unwrap(),
                )
                .unwrap();
            let pending = Event {
                protocol: Protocol,
                subscription_id: Id::new(subscription_id).unwrap(),
                event: EventName::PermissionPending,
                run_id: Some(Id::new(run_id).unwrap()),
                run_seq: Some(8),
                body: serde_json::json!({
                    "gate_id": "private-gate",
                    "kind": "confirm",
                    "title": "Use camera\nnow",
                    "message": "Needed for capture"
                }),
            };
            server.write_all(&encode_frame(&pending).unwrap()).unwrap();
            let answer = read_frame(&mut server);
            assert_eq!(answer["operation"], "permission.answer");
            assert_eq!(answer["body"]["run_id"], run_id);
            assert_eq!(answer["body"]["gate_id"], "private-gate");
            assert_eq!(answer["body"]["decision"], "allow");
            server
                .write_all(
                    &encode_frame(&Response {
                        protocol: Protocol,
                        request_id: Id::new(answer["request_id"].as_str().unwrap()).unwrap(),
                        ok: Success,
                        body: serde_json::json!({
                            "run_id": run_id,
                            "gate_id": "private-gate",
                            "decision": "allow",
                            "committed_seq": 9,
                            "accepted_at": "2026-07-17T12:00:01Z"
                        }),
                    })
                    .unwrap(),
                )
                .unwrap();
            let ack = read_frame(&mut server);
            assert_eq!(ack["operation"], "run.cursor_ack");
            assert_eq!(ack["body"]["through_run_seq"], 8);
            server
                .write_all(
                    &encode_frame(&Response {
                        protocol: Protocol,
                        request_id: Id::new(ack["request_id"].as_str().unwrap()).unwrap(),
                        ok: Success,
                        body: serde_json::json!({
                            "subscription_id": subscription_id,
                            "through_run_seq": 8
                        }),
                    })
                    .unwrap(),
                )
                .unwrap();
            let events = [
                Event {
                    protocol: Protocol,
                    subscription_id: Id::new(subscription_id).unwrap(),
                    event: EventName::SubscriptionCaughtUp,
                    run_id: Some(Id::new(run_id).unwrap()),
                    run_seq: Some(8),
                    body: serde_json::json!({}),
                },
                Event {
                    protocol: Protocol,
                    subscription_id: Id::new(subscription_id).unwrap(),
                    event: EventName::RunEvent,
                    run_id: Some(Id::new(run_id).unwrap()),
                    run_seq: Some(9),
                    body: serde_json::json!({
                        "event_type": "run.completed",
                        "event_version": 999,
                        "recorded_at": "2026-07-17T12:00:02Z",
                        "payload": {"withheld": true, "receipt": {
                            "route": "cloud", "model": null, "cost": "$0.01", "time": "2s",
                            "capabilities": [{"name": "search", "version": "1"}]
                        }}
                    }),
                },
            ];
            for event in &events {
                server.write_all(&encode_frame(&event).unwrap()).unwrap();
            }
            let ack = read_frame(&mut server);
            assert_eq!(ack["operation"], "run.cursor_ack");
            assert_eq!(ack["body"]["through_run_seq"], 9);
            server
                .write_all(
                    &encode_frame(&Response {
                        protocol: Protocol,
                        request_id: Id::new(ack["request_id"].as_str().unwrap()).unwrap(),
                        ok: Success,
                        body: serde_json::json!({
                            "subscription_id": subscription_id,
                            "through_run_seq": 9
                        }),
                    })
                    .unwrap(),
                )
                .unwrap();
        });
        let mut input = io::Cursor::new(b"ship it\nmaybe\nyes\n");
        let mut output = Vec::new();
        run_with(
            &["run".into(), "start".into()],
            "/workspace",
            true,
            true,
            &mut input,
            &mut output,
            |pending| {
                handshake_stream(
                    client,
                    "0.0.1",
                    Duration::from_secs(1),
                    Duration::from_secs(1),
                    pending,
                )
            },
        )
        .unwrap();
        worker.join().unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("Run committed: 123e4567-e89b-12d3-a456-426614174000"));
        let permission = output
            .find("Permission required: Use camera now\n")
            .unwrap();
        let live_event = output
            .find("9\trun.completed\t2026-07-17T12:00:02Z\n")
            .unwrap();
        assert!(permission < live_event);
        assert!(output.contains("Enter y to allow or n to deny."));
        assert!(!output.contains("ship it"));
        assert_eq!(output.matches("Pairing requested").count(), 1);
        assert_eq!(output.matches("run.completed").count(), 1);
        assert!(output.contains("Receipt\troute=cloud\tcost=$0.01\ttime=2s\tcapabilities=search@1"));
        assert!(!output.contains('\u{1b}'));
        for private in [
            "deadcafe",
            "private-server",
            subscription_id,
            "private-gate",
            "withheld",
            "777",
            "999",
        ] {
            assert!(!output.contains(private));
        }
    }

    #[test]
    fn empty_and_eof_run_prompts_fail_before_connecting_without_echoing_input() {
        for bytes in [b"".as_slice(), b"  \t\n".as_slice()] {
            let mut input = io::Cursor::new(bytes);
            let mut output = Vec::new();
            let error = run_with(
                &["run".into(), "start".into()],
                "/workspace",
                true,
                true,
                &mut input,
                &mut output,
                |_| panic!("empty prompt connected"),
            )
            .unwrap_err();
            assert!(guidance(&error).contains("non-empty prompt"));
            assert_eq!(String::from_utf8(output).unwrap(), "Prompt: ");
        }
    }

    #[test]
    fn rejected_run_start_has_redacted_run_specific_guidance() {
        let (client, mut server) = UnixStream::pair().unwrap();
        let worker = std::thread::spawn(move || {
            read_frame(&mut server);
            server
                .write_all(
                    &encode_frame(&welcome(
                        1,
                        "private-server",
                        "11".repeat(16),
                        "22".repeat(16),
                    ))
                    .unwrap(),
                )
                .unwrap();
            server
                .write_all(
                    &encode_frame(&authorized(
                        "deadcafe".repeat(8),
                        3600,
                        900,
                        BTreeMap::new(),
                    ))
                    .unwrap(),
                )
                .unwrap();
            let request = read_frame(&mut server);
            let rejection = ErrorEnvelope {
                protocol: Protocol,
                request_id: Some(Id::new(request["request_id"].as_str().unwrap()).unwrap()),
                ok: Failure,
                error: ProtocolError::invalid_request(),
            };
            server
                .write_all(&encode_frame(&rejection).unwrap())
                .unwrap();
        });
        let mut input = io::Cursor::new(b"private prompt\n");
        let mut output = Vec::new();
        let error = run_with(
            &["run".into(), "start".into()],
            "/workspace",
            true,
            true,
            &mut input,
            &mut output,
            |pending| {
                handshake_stream(
                    client,
                    "0.0.1",
                    Duration::from_secs(1),
                    Duration::from_secs(1),
                    pending,
                )
            },
        )
        .unwrap_err();
        worker.join().unwrap();
        let message = guidance(&error);
        assert!(message.contains("rejected the run request"));
        assert!(!message.contains("private prompt"));
        assert!(!String::from_utf8(output)
            .unwrap()
            .contains("private prompt"));
    }

    #[test]
    fn end_of_input_at_the_prompt_stops_cleanly() {
        let output = paginated_output(b"", false);
        assert_eq!(output.matches("Press Enter").count(), 1);
        assert!(!output.contains("private-cursor"));
    }

    fn paginated_output(input: &[u8], continue_expected: bool) -> String {
        let (client, mut server) = UnixStream::pair().unwrap();
        let worker = std::thread::spawn(move || {
            read_frame(&mut server);
            server
                .write_all(
                    &encode_frame(&welcome(
                        1,
                        "server-detail",
                        "11".repeat(16),
                        "22".repeat(16),
                    ))
                    .unwrap(),
                )
                .unwrap();
            server
                .write_all(
                    &encode_frame(&authorized(
                        "deadcafe".repeat(8),
                        3600,
                        900,
                        BTreeMap::new(),
                    ))
                    .unwrap(),
                )
                .unwrap();
            let first = read_frame(&mut server);
            let response = Response {
                protocol: Protocol,
                request_id: Id::new(first["request_id"].as_str().unwrap()).unwrap(),
                ok: Success,
                body: serde_json::json!({
                    "threads": [{"thread_id": "opaque-1", "title": "First", "updated_at": "2026-07-17T00:00:00Z"}],
                    "next_cursor": "private-cursor"
                }),
            };
            for byte in encode_frame(&response).unwrap() {
                server.write_all(&[byte]).unwrap();
            }
            if continue_expected {
                let second = read_frame(&mut server);
                assert_eq!(
                    second["body"],
                    serde_json::json!({
                        "limit": 100,
                        "cursor": "private-cursor"
                    })
                );
                assert_ne!(first["request_id"], second["request_id"]);
                let response = Response {
                    protocol: Protocol,
                    request_id: Id::new(second["request_id"].as_str().unwrap()).unwrap(),
                    ok: Success,
                    body: serde_json::json!({
                        "threads": [{"thread_id": "opaque-2", "title": "Second", "updated_at": "2026-07-16T00:00:00Z"}]
                    }),
                };
                for chunk in encode_frame(&response).unwrap().chunks(2) {
                    server.write_all(chunk).unwrap();
                }
            } else {
                let mut byte = [0];
                assert_eq!(server.read(&mut byte).unwrap(), 0);
            }
        });
        let mut input = io::Cursor::new(input);
        let mut output = Vec::new();
        run_with(
            &["threads".into(), "list".into()],
            "/workspace",
            true,
            true,
            &mut input,
            &mut output,
            |pending| {
                handshake_stream(
                    client,
                    "0.0.1",
                    Duration::from_secs(1),
                    Duration::from_secs(1),
                    pending,
                )
            },
        )
        .unwrap();
        worker.join().unwrap();
        String::from_utf8(output).unwrap()
    }

    #[test]
    fn enter_fetches_and_renders_the_terminal_page_on_one_pairing() {
        let output = paginated_output(b"\n", true);
        assert!(output.contains("First\t2026-07-17T00:00:00Z\topaque-1"));
        assert!(output.contains("Second\t2026-07-16T00:00:00Z\topaque-2"));
        assert_eq!(output.matches("Press Enter").count(), 1);
        assert!(!output.contains("private-cursor"));
        assert_eq!(output.matches("Pairing requested").count(), 1);
    }

    #[test]
    fn open_pages_reuse_pairing_and_render_entries_with_optional_text() {
        let (client, mut server) = UnixStream::pair().unwrap();
        let worker = std::thread::spawn(move || {
            read_frame(&mut server);
            server
                .write_all(
                    &encode_frame(&welcome(
                        1,
                        "server-detail",
                        "11".repeat(16),
                        "22".repeat(16),
                    ))
                    .unwrap(),
                )
                .unwrap();
            server
                .write_all(
                    &encode_frame(&authorized(
                        "deadcafe".repeat(8),
                        3600,
                        900,
                        BTreeMap::new(),
                    ))
                    .unwrap(),
                )
                .unwrap();
            let first = read_frame(&mut server);
            assert_eq!(first["operation"], "thread.open");
            assert_eq!(
                first["body"],
                serde_json::json!({"thread_id": "thread-1", "limit": 100})
            );
            let response = Response {
                protocol: Protocol,
                request_id: Id::new(first["request_id"].as_str().unwrap()).unwrap(),
                ok: Success,
                body: serde_json::json!({
                    "thread_id": "thread-1",
                    "entries": [{"run_seq": 2, "kind": "tool"}],
                    "next_cursor": "private-cursor"
                }),
            };
            for byte in encode_frame(&response).unwrap() {
                server.write_all(&[byte]).unwrap();
            }
            let second = read_frame(&mut server);
            assert_eq!(second["body"]["cursor"], "private-cursor");
            let response = Response {
                protocol: Protocol,
                request_id: Id::new(second["request_id"].as_str().unwrap()).unwrap(),
                ok: Success,
                body: serde_json::json!({
                    "thread_id": "thread-1",
                    "entries": [{"run_seq": 3, "kind": "message", "text": "done\nnow"}]
                }),
            };
            for chunk in encode_frame(&response).unwrap().chunks(2) {
                server.write_all(chunk).unwrap();
            }
        });
        let mut input = io::Cursor::new(b"\n");
        let mut output = Vec::new();
        run_with(
            &["threads".into(), "open".into(), "thread-1".into()],
            "/workspace",
            true,
            true,
            &mut input,
            &mut output,
            |pending| {
                handshake_stream(
                    client,
                    "0.0.1",
                    Duration::from_secs(1),
                    Duration::from_secs(1),
                    pending,
                )
            },
        )
        .unwrap();
        worker.join().unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("2\ttool\n"));
        assert!(output.contains("3\tmessage\tdone now\n"));
        assert_eq!(output.matches("Pairing requested").count(), 1);
        for private in ["private-cursor", "deadcafe", "server-detail"] {
            assert!(!output.contains(private));
        }
    }

    #[test]
    fn q_declines_without_requesting_another_page() {
        let output = paginated_output(b"q\n", false);
        assert!(output.contains("First\t2026-07-17T00:00:00Z\topaque-1"));
        assert!(!output.contains("Second"));
        assert!(!output.contains("private-cursor"));
    }

    #[test]
    fn unsupported_and_noninteractive_inputs_do_not_connect() {
        for (args, stdin_terminal, stdout_terminal, expected) in [
            (
                vec![OsString::from("threads")],
                true,
                true,
                "unsupported arguments",
            ),
            (
                vec!["threads".into(), "list".into()],
                false,
                true,
                "interactive",
            ),
            (
                vec!["run".into(), "start".into()],
                false,
                true,
                "interactive",
            ),
            (
                vec!["threads".into(), "list".into()],
                true,
                false,
                "interactive",
            ),
            (
                vec!["threads".into(), "open".into(), "".into()],
                true,
                true,
                "valid thread ID",
            ),
            (
                vec!["threads".into(), "open".into(), "x".repeat(37).into()],
                true,
                true,
                "valid thread ID",
            ),
        ] {
            let mut output = Vec::new();
            let mut input = io::Cursor::new(Vec::<u8>::new());
            let result = run_with(
                &args,
                "/workspace",
                stdin_terminal,
                stdout_terminal,
                &mut input,
                &mut output,
                |_| panic!("connection attempted before input validation"),
            );
            assert!(guidance(&result.unwrap_err()).contains(expected));
            assert!(output.is_empty());
        }
    }

    #[test]
    fn help_and_version_exit_before_credentials_or_connection() {
        assert_eq!(HELP.matches("muniment").count(), 1);
        assert_eq!(HELP.matches("--workspace").count(), 1);
        for command in [
            "  threads list\n",
            "  threads open <thread-id>\n",
            "  run start\n",
            "  workspace init\n",
        ] {
            assert!(HELP.contains(command));
        }
        for (args, expected) in [
            (vec![OsString::from("--help")], HELP),
            (vec![OsString::from("-h")], HELP),
            (vec![OsString::from("--version")], env!("CARGO_PKG_VERSION")),
        ] {
            let mut output = Vec::new();
            run_args(args, &mut output, |_| {
                panic!("credential read or connection attempted")
            })
            .unwrap();
            assert_eq!(String::from_utf8(output).unwrap(), format!("{expected}\n"));
        }
    }

    #[test]
    fn workspace_override_is_absolute_unique_and_position_independent() {
        let absolute = std::env::temp_dir().join("muniment-override");
        let mut args = vec![
            "threads".into(),
            "list".into(),
            "--workspace".into(),
            absolute.clone().into_os_string(),
        ];
        assert_eq!(workspace_argument(&mut args).unwrap(), Some(absolute));
        assert_eq!(args, [OsString::from("threads"), OsString::from("list")]);

        for mut invalid in [
            vec![OsString::from("--workspace")],
            vec![OsString::from("--workspace"), OsString::from("relative")],
            vec![
                OsString::from("--workspace"),
                std::env::temp_dir().into_os_string(),
                OsString::from("--workspace"),
                std::env::temp_dir().into_os_string(),
            ],
        ] {
            assert!(workspace_argument(&mut invalid).is_err());
        }
    }
}
