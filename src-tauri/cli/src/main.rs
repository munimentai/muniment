use muniment_attach::{handshake, ClientError};

fn main() {
    if let Err(error) = run() {
        eprintln!("muniment: {}", guidance(error));
        std::process::exit(1);
    }
}

fn run() -> Result<(), ClientError> {
    handshake(env!("CARGO_PKG_VERSION"), || {
        println!("Pairing requested. Approve the named ‘muniment CLI’ connection in the Muniment desktop.");
    })?;
    println!("Muniment CLI authorized. No thread operation was requested.");
    Ok(())
}

fn guidance(error: ClientError) -> &'static str {
    match error {
        ClientError::DesktopUnavailable
        | ClientError::RuntimeDirectoryMissing
        | ClientError::RuntimeDirectoryRelative => "open the Muniment desktop, then try again",
        ClientError::ConnectionClosed => {
            "pairing was denied or closed; retry and approve the CLI in the desktop"
        }
        ClientError::Timeout => "pairing timed out; retry and approve the CLI in the desktop",
        ClientError::ProtocolIncompatible => {
            "the desktop and CLI are incompatible; update Muniment desktop and the CLI"
        }
        ClientError::UnsupportedPlatform => "desktop attach is not supported on this platform",
        _ => "the desktop pairing response was invalid; update Muniment and try again",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_guidance_covers_expected_failures_without_details() {
        assert!(guidance(ClientError::DesktopUnavailable).contains("open"));
        assert!(guidance(ClientError::ConnectionClosed).contains("denied"));
        assert!(guidance(ClientError::ProtocolIncompatible).contains("update"));
    }
}
