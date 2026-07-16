#![cfg(target_os = "linux")]

use muniment_core::attach::linux::{
    run_authenticated_session_with, AttachSessionError, PeerCredentials,
};
use muniment_core::attach::{
    decode_frame, encode_frame, ErrorAction, ErrorEnvelope, Hello, Protocol, VersionRange, Welcome,
};
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::thread;
use std::time::{Duration, Instant};

fn credentials() -> PeerCredentials {
    PeerCredentials {
        pid: std::process::id() as libc::pid_t,
        uid: unsafe { libc::geteuid() },
        gid: unsafe { libc::getegid() },
    }
}

fn hello(min: u32, max: u32) -> Vec<u8> {
    encode_frame(&Hello {
        protocol: Protocol,
        client: muniment_core::attach::Client {
            kind: "cli".into(),
            version: "1.0.0".into(),
        },
        supported: VersionRange { min, max },
        client_nonce: "client-nonce".into(),
    })
    .unwrap()
}

fn read_frame<T: serde::de::DeserializeOwned>(stream: &mut UnixStream) -> T {
    let mut prefix = [0; 4];
    stream.read_exact(&mut prefix).unwrap();
    let mut bytes = vec![0; 4 + u32::from_be_bytes(prefix) as usize];
    bytes[..4].copy_from_slice(&prefix);
    stream.read_exact(&mut bytes[4..]).unwrap();
    decode_frame(&bytes).unwrap().unwrap().0
}

#[test]
fn fragmented_hello_receives_deterministic_welcome_then_closes() {
    let (mut client, server) = UnixStream::pair().unwrap();
    let task = thread::spawn(move || {
        run_authenticated_session_with(
            server,
            credentials(),
            "0.1.0",
            Duration::from_secs(1),
            |bytes| {
                for (index, byte) in bytes.iter_mut().enumerate() {
                    *byte = index as u8;
                }
                Ok(())
            },
        )
    });
    let frame = hello(1, 1);
    for part in frame.chunks(3) {
        client.write_all(part).unwrap();
    }
    let welcome: Welcome = read_frame(&mut client);
    assert_eq!(welcome.selected, 1);
    assert_eq!(welcome.desktop_version, "0.1.0");
    assert_eq!(welcome.server_nonce, "000102030405060708090a0b0c0d0e0f");
    assert_eq!(
        welcome.approval_challenge,
        "101112131415161718191a1b1c1d1e1f"
    );
    assert_eq!(client.read(&mut [0]).unwrap(), 0);
    assert_eq!(task.join().unwrap(), Ok(()));
}

#[test]
fn hello_deadline_is_short_and_closes_without_a_response() {
    let (mut client, server) = UnixStream::pair().unwrap();
    let start = Instant::now();
    let result = run_authenticated_session_with(
        server,
        credentials(),
        "0.1.0",
        Duration::from_millis(20),
        |_| Ok(()),
    );
    assert_eq!(result, Err(AttachSessionError::Timeout));
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!(client.read(&mut [0]).unwrap(), 0);
}

#[test]
fn incompatibility_actions_are_framed_and_terminal() {
    for (range, action) in [
        ((0, 0), ErrorAction::UpgradeCompanion),
        ((2, 2), ErrorAction::UpgradeDesktop),
    ] {
        let (mut client, server) = UnixStream::pair().unwrap();
        client.write_all(&hello(range.0, range.1)).unwrap();
        client.shutdown(Shutdown::Write).unwrap();
        assert_eq!(
            run_authenticated_session_with(
                server,
                credentials(),
                "0.1.0",
                Duration::from_secs(1),
                |_| Ok(())
            ),
            Err(AttachSessionError::ProtocolIncompatible)
        );
        let error: ErrorEnvelope = read_frame(&mut client);
        assert_eq!(error.error.action(), Some(action));
        assert_eq!(client.read(&mut [0]).unwrap(), 0);
    }
}
