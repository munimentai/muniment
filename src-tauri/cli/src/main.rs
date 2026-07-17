use muniment_attach::{handshake, ClientError, ThreadListPage, ThreadOpenPage};
use std::ffi::OsString;
use std::io::{self, BufRead, IsTerminal, Write};

const USAGE: &str = "usage: muniment threads list | muniment threads open <thread-id>";

enum Command {
    List,
    Open(String),
}

#[derive(Debug)]
enum CliError {
    Usage,
    InvalidThreadId,
    NonInteractive,
    Client(ClientError),
}

fn main() {
    if let Err(error) = run() {
        eprintln!("muniment: {}", guidance(&error));
        if matches!(error, CliError::Usage) {
            eprintln!("{USAGE}");
        }
        std::process::exit(1);
    }
}

fn run() -> Result<(), CliError> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut stdout = io::stdout();
    run_with(
        &args,
        io::stdin().is_terminal(),
        io::stdout().is_terminal(),
        &mut input,
        &mut stdout,
        |pairing_pending| handshake(env!("CARGO_PKG_VERSION"), pairing_pending),
    )
}

fn run_with(
    args: &[OsString],
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
        _ => return Err(CliError::Usage),
    };
    if !stdin_is_terminal || !stdout_is_terminal {
        return Err(CliError::NonInteractive);
    }
    let mut pairing_pending = || {
        writeln!(output, "Pairing requested. Approve the named ‘muniment CLI’ connection in the Muniment desktop.").ok();
    };
    let mut client = connect(&mut pairing_pending).map_err(CliError::Client)?;
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
        CliError::NonInteractive => "thread commands require interactive stdin and stdout",
        CliError::Client(ClientError::DesktopUnavailable)
        | CliError::Client(ClientError::RuntimeDirectoryMissing)
        | CliError::Client(ClientError::RuntimeDirectoryRelative) => {
            "open the Muniment desktop, then try again"
        }
        CliError::Client(ClientError::ConnectionClosed) => {
            "pairing was denied or closed; retry and approve the CLI in the desktop"
        }
        CliError::Client(ClientError::Timeout) => {
            "the desktop timed out; retry and keep the desktop open"
        }
        CliError::Client(ClientError::AuthorizationExpired) => {
            "authorization expired; retry and approve the CLI in the desktop"
        }
        CliError::Client(ClientError::DesktopFailed) => {
            "the desktop could not read threads; retry, then check the desktop"
        }
        CliError::Client(ClientError::RequestRejected) => {
            "the desktop rejected the thread request; check the thread ID, retry, then update Muniment if it continues"
        }
        CliError::Client(ClientError::ProtocolIncompatible) => {
            "the desktop and CLI are incompatible; update Muniment desktop and the CLI"
        }
        CliError::Client(ClientError::UnsupportedPlatform) => {
            "desktop attach is not supported on this platform"
        }
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
        authorized, encode_frame, handshake_stream, welcome, Id, Protocol, RedactedThreadSummary,
        Response, Success,
    };
    use std::collections::{BTreeMap, BTreeSet};
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    #[test]
    fn user_guidance_covers_expected_failures_without_details() {
        assert!(guidance(&CliError::Client(ClientError::DesktopUnavailable)).contains("open"));
        assert!(guidance(&CliError::Client(ClientError::ConnectionClosed)).contains("denied"));
        assert!(guidance(&CliError::Client(ClientError::ProtocolIncompatible)).contains("update"));
        let rejected = guidance(&CliError::Client(ClientError::RequestRejected));
        assert!(rejected.contains("thread request"));
        assert!(rejected.contains("thread ID"));
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
}
