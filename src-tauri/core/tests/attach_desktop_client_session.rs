#![cfg(target_os = "linux")]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

use muniment_core::attach::linux::{
    serve_desktop_client_session, ThreadListPage, ThreadListRequest, ThreadListService,
};
use muniment_core::attach::{
    decode_frame, encode_frame, Envelope, ErrorCode, Id, Operation, Protocol, ProtocolError,
    Request,
};

struct TestService;

impl ThreadListService for TestService {
    fn list_threads(
        &mut self,
        workspace: &str,
        _request: ThreadListRequest,
    ) -> Result<ThreadListPage, ProtocolError> {
        assert_eq!(workspace, "/work/signed");
        Ok(ThreadListPage {
            threads: Vec::new(),
            next_cursor: None,
        })
    }
}

fn request(id: &str, operation: Operation, capability: &str, body: serde_json::Value) -> Request {
    Request {
        protocol: Protocol,
        request_id: Id::new(id).unwrap(),
        operation,
        capability: capability.into(),
        idempotency_key: None,
        body,
    }
}

fn exchange(stream: &mut UnixStream, request: Request) -> Envelope {
    stream.write_all(&encode_frame(&request).unwrap()).unwrap();
    let mut prefix = [0_u8; 4];
    stream.read_exact(&mut prefix).unwrap();
    let mut frame = vec![0_u8; u32::from_be_bytes(prefix) as usize + 4];
    frame[..4].copy_from_slice(&prefix);
    stream.read_exact(&mut frame[4..]).unwrap();
    decode_frame(&frame).unwrap().unwrap().0
}

#[test]
fn unauthorized_requests_do_not_end_the_session() {
    let (mut client, server) = UnixStream::pair().unwrap();
    let session = std::thread::spawn(move || {
        serve_desktop_client_session(server, "admitted", "/work/signed", &mut TestService)
    });

    for (id, operation, capability) in [
        (
            "018f0000-0000-7000-8000-000000000201",
            Operation::MigrationControl,
            "admitted",
        ),
        (
            "018f0000-0000-7000-8000-000000000202",
            Operation::ApprovalPresent,
            "admitted",
        ),
        (
            "018f0000-0000-7000-8000-000000000203",
            Operation::ThreadList,
            "other",
        ),
    ] {
        let Envelope::Error(error) = exchange(
            &mut client,
            request(id, operation, capability, serde_json::json!({})),
        ) else {
            panic!("unauthorized request did not return an error");
        };
        assert_eq!(error.error.code(), ErrorCode::Unauthorized);
    }

    let response = exchange(
        &mut client,
        request(
            "018f0000-0000-7000-8000-000000000204",
            Operation::ThreadList,
            "admitted",
            serde_json::json!({"limit": 20}),
        ),
    );
    assert!(matches!(response, Envelope::Response(_)));
    drop(client);
    assert_eq!(session.join().unwrap(), Ok(()));
}
