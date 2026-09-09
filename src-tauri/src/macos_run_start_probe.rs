use std::path::Path;
use std::time::Duration;

use muniment_core::attach::connect_desktop_client_at;

pub(crate) fn run(endpoint: &Path) -> Result<String, &'static str> {
    submit_after_idle(endpoint, Duration::from_secs(60))
}

fn submit_after_idle(endpoint: &Path, idle: Duration) -> Result<String, &'static str> {
    // The installed desktop executable must connect so the runtime can verify its peer identity.
    let mut client =
        connect_desktop_client_at(endpoint, env!("CARGO_PKG_VERSION"), Duration::from_secs(5))
            .map_err(|_| "The run-start probe could not connect.")?;
    // Keep this connection open without retries or keepalive requests.
    std::thread::sleep(idle);
    let accepted = client
        .run_submit("Start the installed local run-start probe.", &[], None)
        .map_err(|_| "The run-start probe could not submit a run.")?;
    // The journal proves admission. The probe does not need a model reply.
    let _ = client.run_cancel(&accepted.run_id);
    Ok(accepted.run_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;
    use std::time::Instant;

    use muniment_core::attach::{
        admit_desktop_client_over_stream, encode_frame, Operation, Request,
    };
    use serde_json::json;

    #[test]
    fn probe_rejects_an_absent_endpoint() {
        let endpoint =
            std::env::temp_dir().join(format!("muniment-absent-{}", uuid::Uuid::now_v7()));
        assert_eq!(
            run(&endpoint),
            Err("The run-start probe could not connect.")
        );
    }

    #[test]
    fn probe_rejects_a_connection_that_closes_after_admission() {
        let endpoint = Path::new("/tmp").join(format!("muniment-probe-{}", uuid::Uuid::now_v7()));
        let listener = UnixListener::bind(&endpoint).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            admit_desktop_client_over_stream(
                &mut stream,
                "1.2.3",
                None,
                Instant::now() + Duration::from_secs(5),
            )
            .unwrap();
        });
        let result = submit_after_idle(&endpoint, Duration::from_millis(50));
        server.join().unwrap();
        std::fs::remove_file(endpoint).unwrap();
        assert_eq!(result, Err("The run-start probe could not submit a run."));
    }

    #[test]
    fn probe_submits_on_the_admitted_connection_and_cancels_the_run() {
        // macOS limits Unix socket paths to 104 bytes.
        let directory =
            Path::new("/tmp").join(format!("muniment-run-probe-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir(&directory).unwrap();
        let endpoint = directory.join("attach.sock");
        let listener = UnixListener::bind(&endpoint).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let admitted = admit_desktop_client_over_stream(
                &mut stream,
                "1.2.3",
                None,
                Instant::now() + Duration::from_secs(5),
            )
            .unwrap();
            for operation in [Operation::RunSubmit, Operation::RunCancel] {
                let mut prefix = [0; 4];
                stream.read_exact(&mut prefix).unwrap();
                let mut body = vec![0; u32::from_be_bytes(prefix) as usize];
                stream.read_exact(&mut body).unwrap();
                let request: Request = serde_json::from_slice(&body).unwrap();
                assert_eq!(request.operation, operation);
                assert_eq!(request.capability, admitted.capability);
                assert!(request.idempotency_key.is_some());
                if operation == Operation::RunSubmit {
                    assert_eq!(
                        request.body,
                        json!({
                            "text": "Start the installed local run-start probe.", "files": [], "thread_id": null,
                        })
                    );
                } else {
                    assert_eq!(
                        request.body["run_id"],
                        "018f0000-0000-7000-8000-000000000201"
                    );
                }
                let mut body = json!({
                    "run_id": "018f0000-0000-7000-8000-000000000201",
                    "accepted_at": "2026-01-01T00:00:00Z",
                });
                if operation == Operation::RunSubmit {
                    body["thread_id"] = json!("018f0000-0000-7000-8000-000000000202");
                    body["committed_seq"] = json!(1);
                    body["attachments"] = json!([]);
                }
                stream
                    .write_all(
                        &encode_frame(&json!({
                            "protocol": "muniment.attach/1", "request_id": request.request_id,
                            "ok": true, "body": body,
                        }))
                        .unwrap(),
                    )
                    .unwrap();
            }
        });
        let result = submit_after_idle(&endpoint, Duration::from_millis(20));
        server.join().unwrap();
        std::fs::remove_dir_all(directory).unwrap();
        assert_eq!(result, Ok("018f0000-0000-7000-8000-000000000201".into()));
    }
}
