use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::{Duration, Instant};

use muniment_core::attach::{evaluate_quiesce, RuntimeActivityRegistry};
use muniment_runtime::sign_in;

#[test]
fn native_registration_sends_a_tls_client_hello() {
    // This integration-test binary has one test. Keep the loopback request off environment proxies.
    for name in [
        "ALL_PROXY",
        "all_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ] {
        std::env::remove_var(name);
    }

    let timeout = Duration::from_secs(5);
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    muniment_core::chat_prompt::use_mock_keyring_for_tests();
    std::env::set_var(
        "MUNIMENT_API_BASE_URL",
        format!("https://{}", listener.local_addr().unwrap()),
    );
    let server = thread::spawn(move || {
        let deadline = Instant::now() + timeout;
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "The TLS client did not connect.");
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("The TLS listener failed: {error}"),
            }
        };
        stream.set_read_timeout(Some(timeout)).unwrap();
        stream.set_write_timeout(Some(timeout)).unwrap();
        let mut header = [0; 5];
        stream.read_exact(&mut header).expect(
            "The native-auth client must send a TLS ClientHello, not reject HTTPS with UnknownScheme.",
        );
        assert_eq!(
            header[0], 22,
            "The client must send a TLS handshake record."
        );
        assert_eq!(header[1], 3, "The client must use a TLS record version.");
        let mut record = vec![0; u16::from_be_bytes([header[3], header[4]]) as usize];
        stream.read_exact(&mut record).unwrap();
        assert_eq!(
            record.first(),
            Some(&1),
            "The handshake must start with ClientHello."
        );

        // Reject the handshake with a fatal handshake_failure alert. The listener needs no certificate or external server.
        stream.write_all(&[21, 3, 3, 0, 2, 2, 40]).unwrap();
    });

    let activity = RuntimeActivityRegistry::new();
    let browser = |_: &str| panic!("A failed TLS handshake must not open the browser.");
    let result = sign_in(&|| true, &browser, &Default::default(), &activity);
    std::env::remove_var("MUNIMENT_API_BASE_URL");
    server.join().unwrap();
    assert!(
        result.is_err(),
        "The listener must reject the TLS handshake."
    );
    assert!(evaluate_quiesce(activity.snapshot()).is_ok());
}
