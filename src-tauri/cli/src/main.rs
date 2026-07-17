use muniment_attach::{handshake, ClientError, ThreadListPage};
use std::ffi::OsString;
use std::io::{self, IsTerminal, Write};

const USAGE: &str = "usage: muniment threads list";

#[derive(Debug)]
enum CliError {
    Usage,
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
    let mut stdout = io::stdout();
    run_with(
        &args,
        io::stdin().is_terminal(),
        io::stdout().is_terminal(),
        &mut stdout,
        |pairing_pending| handshake(env!("CARGO_PKG_VERSION"), pairing_pending),
    )
}

fn run_with(
    args: &[OsString],
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
    output: &mut impl Write,
    connect: impl FnOnce(&mut dyn FnMut()) -> Result<muniment_attach::AuthorizedClient, ClientError>,
) -> Result<(), CliError> {
    if args != ["threads", "list"] {
        return Err(CliError::Usage);
    }
    if !stdin_is_terminal || !stdout_is_terminal {
        return Err(CliError::NonInteractive);
    }
    let mut pairing_pending = || {
        writeln!(output, "Pairing requested. Approve the named ‘muniment CLI’ connection in the Muniment desktop.").ok();
    };
    let mut client = connect(&mut pairing_pending).map_err(CliError::Client)?;
    let page = client.list_threads().map_err(CliError::Client)?;
    write!(output, "{}", render_page(&page))
        .map_err(|_| CliError::Client(ClientError::ConnectionClosed))?;
    Ok(())
}

fn guidance(error: &CliError) -> &'static str {
    match error {
        CliError::Usage => "unsupported arguments",
        CliError::NonInteractive => "threads list requires interactive stdin and stdout",
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
            "the desktop could not list threads; retry, then check the desktop"
        }
        CliError::Client(ClientError::RequestRejected) => {
            "the desktop rejected the thread-list request; retry, then update Muniment if it continues"
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
    if page.next_cursor.is_some() {
        writeln!(
            output,
            "More threads are available; continuation support is coming soon."
        )
        .unwrap();
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
        assert!(rejected.contains("thread-list request"));
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
        assert!(output.contains("More threads are available"));
        assert!(!output.contains("secret-cursor"));
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
        run_with(
            &["threads".into(), "list".into()],
            true,
            true,
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

        let page = paired_output(serde_json::json!({
            "threads": [{"thread_id": "opaque-id", "title": "Visible title", "updated_at": "2026-07-17T00:00:00Z"}],
            "next_cursor": "private-cursor"
        }));
        assert!(page.contains("Visible title\t2026-07-17T00:00:00Z\topaque-id"));
        assert!(page.contains("More threads are available"));
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
        ] {
            let mut output = Vec::new();
            let result = run_with(&args, stdin_terminal, stdout_terminal, &mut output, |_| {
                panic!("connection attempted before input validation")
            });
            assert!(guidance(&result.unwrap_err()).contains(expected));
            assert!(output.is_empty());
        }
    }
}
