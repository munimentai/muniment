use muniment_attach::{handshake, ClientError, ThreadListPage};
use std::io::{self, IsTerminal};

const USAGE: &str = "usage: muniment threads list";

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
    if args != ["threads", "list"] {
        return Err(CliError::Usage);
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(CliError::NonInteractive);
    }
    let mut client = handshake(env!("CARGO_PKG_VERSION"), || {
        println!("Pairing requested. Approve the named ‘muniment CLI’ connection in the Muniment desktop.");
    }).map_err(CliError::Client)?;
    let page = client.list_threads().map_err(CliError::Client)?;
    print!("{}", render_page(&page));
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
    use muniment_attach::RedactedThreadSummary;

    #[test]
    fn user_guidance_covers_expected_failures_without_details() {
        assert!(guidance(&CliError::Client(ClientError::DesktopUnavailable)).contains("open"));
        assert!(guidance(&CliError::Client(ClientError::ConnectionClosed)).contains("denied"));
        assert!(guidance(&CliError::Client(ClientError::ProtocolIncompatible)).contains("update"));
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
}
