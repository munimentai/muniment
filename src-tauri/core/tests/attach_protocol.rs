use muniment_core::attach::{codec::*, protocol::*};
use serde_json::{json, Value};

const ID: &str = "123e4567-e89b-12d3-a456-426614174000";
const ID2: &str = "123e4567-e89b-12d3-a456-426614174001";

fn framed(value: Value) -> Vec<u8> {
    let body = serde_json::to_vec(&value).unwrap();
    let mut out = (body.len() as u32).to_be_bytes().to_vec();
    out.extend(body);
    out
}
fn request(operation: &str, body: Value) -> Value {
    let effectful = [
        "run.start",
        "run.steer",
        "run.follow_up",
        "run.cancel",
        "permission.answer",
    ];
    let mut value = json!({"protocol":PROTOCOL,"request_id":ID,"operation":operation,"capability":"capability","body":body});
    if effectful.contains(&operation) {
        value["idempotency_key"] = json!("stable-key");
    }
    value
}
fn golden_frames() -> Vec<(&'static str, Vec<u8>)> {
    include_str!("fixtures/attach/golden.frames")
        .lines()
        .map(|line| {
            let (name, hex) = line.split_once('|').unwrap();
            let bytes = hex
                .as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect();
            (name, bytes)
        })
        .collect()
}

#[test]
fn exact_golden_frames_decode_and_reencode_without_drift() {
    for (name, frame) in golden_frames() {
        assert_eq!(
            u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize,
            frame.len() - 4,
            "big-endian prefix for {name}"
        );
        let encoded = if name == "hello" {
            encode_frame(&decode_hello(&frame).unwrap()).unwrap()
        } else if name == "welcome" {
            encode_frame(&decode_welcome(&frame).unwrap()).unwrap()
        } else if name == "authorized" {
            encode_frame(&decode_authorized(&frame).unwrap()).unwrap()
        } else if name.starts_with("request.") || name == "unknown_optional" {
            encode_frame(&decode_request(&frame).unwrap()).unwrap()
        } else if name.starts_with("response.") {
            encode_frame(&decode_response(&frame).unwrap()).unwrap()
        } else if name == "error" || name == "incompatible" {
            encode_frame(&decode_error(&frame).unwrap()).unwrap()
        } else {
            encode_frame(&decode_event(&frame).unwrap()).unwrap()
        };
        // Unknown optional envelope fields are deliberately discarded by serde.
        if name != "unknown_optional" {
            assert_eq!(encoded, frame, "golden wire drift for {name}");
        }
    }
}

#[test]
fn every_closed_request_body_is_typed_and_validated() {
    let cases = [
        ("thread.list", json!({"workspace_id":ID,"limit":100})),
        ("thread.open", json!({"thread_id":ID,"limit":100})),
        ("run.open", json!({"run_id":ID})),
        (
            "run.start",
            json!({"workspace_id":ID,"text":"hello","context":["selection"]}),
        ),
        ("run.stream", json!({"run_id":ID,"after_run_seq":0})),
        (
            "run.cursor_ack",
            json!({"subscription_id":ID,"through_run_seq":1}),
        ),
        ("run.steer", json!({"run_id":ID,"text":"turn left"})),
        ("run.follow_up", json!({"run_id":ID,"text":"continue"})),
        ("run.cancel", json!({"run_id":ID})),
        (
            "permission.answer",
            json!({"gate_id":ID,"decision":"allow"}),
        ),
        ("artifact.fetch", json!({"artifact_id":ID})),
        (
            "artifact.window",
            json!({"transfer_id":ID,"ack_through_chunk":-1,"max_chunks":1}),
        ),
        ("request.cancel", json!({"kind":"request","request_id":ID})),
    ];
    for (operation, body) in cases {
        assert!(
            decode_request(&framed(request(operation, body))).is_ok(),
            "{operation}"
        );
    }
    assert!(decode_request(&framed(request("run.open", json!({})))).is_err());
    assert!(decode_request(&framed(request(
        "run.stream",
        json!({"run_id":ID,"last_run_seq":0})
    )))
    .is_err());
    assert!(decode_request(&framed(request(
        "request.cancel",
        json!({"kind":"request","request_id":ID,"subscription_id":ID2})
    )))
    .is_err());
}

#[test]
fn every_event_schema_and_unknown_event_compatibility() {
    let data = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b"abc");
    let cases = [
        (
            "run.event",
            json!({"journal_event":"message.committed","payload":{}}),
            Some((ID, 1)),
        ),
        ("subscription.caught_up", json!({"through_run_seq":1}), None),
        (
            "permission.pending",
            json!({"gate_id":ID,"description":"May run tool"}),
            None,
        ),
        (
            "artifact.chunk",
            json!({"artifact_id":ID,"chunk_index":0,"offset":0,"byte_length":3,"chunk_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","data":data}),
            None,
        ),
        (
            "artifact.complete",
            json!({"transfer_id":ID,"total_bytes":3,"sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}),
            None,
        ),
        ("request.cancelled", json!({"kind":"request","id":ID}), None),
        ("capability.revoked", json!({"reason":"locked"}), None),
        (
            "stream.closed",
            json!({"code":"cancelled","resumable":true}),
            None,
        ),
        ("future.event", json!({"future":"bounded"}), None),
    ];
    for (event, body, run) in cases {
        let mut v = json!({"protocol":PROTOCOL,"subscription_id":ID,"event":event,"body":body,"future_optional":true});
        if let Some((id, seq)) = run {
            v["run_id"] = json!(id);
            v["run_seq"] = json!(seq);
        }
        assert!(decode_event(&framed(v)).is_ok(), "{event}");
    }
    let missing = json!({"protocol":PROTOCOL,"subscription_id":ID,"event":"artifact.chunk","body":{"data":"YWJj"}});
    assert!(decode_event(&framed(missing)).is_err());
}

#[test]
fn semantic_errors_survive_codec_boundary_and_are_redacted() {
    let missing = request("run.start", json!({"workspace_id":ID,"text":"hello"}));
    let mut missing = missing;
    missing.as_object_mut().unwrap().remove("idempotency_key");
    let CodecError::Protocol(error) = decode_request(&framed(missing)).unwrap_err() else {
        panic!()
    };
    assert_eq!(error.code, ErrorCode::IdempotencyKeyRequired);
    let envelope = ErrorEnvelope {
        protocol: Protocol,
        request_id: None,
        ok: False,
        error,
    };
    assert_eq!(
        serde_json::to_value(envelope).unwrap(),
        json!({"protocol":PROTOCOL,"ok":false,"error":{"code":"idempotency_key_required","message":"idempotency key required","retryable":false}})
    );
    let mut invalid = request("thread.list", json!({"workspace_id":ID}));
    invalid["idempotency_key"] = json!("meaningless");
    assert!(matches!(
        decode_request(&framed(invalid)),
        Err(CodecError::Protocol(ProtocolError {
            code: ErrorCode::InvalidRequest,
            ..
        }))
    ));
}

#[test]
fn malformed_and_resource_limit_matrix() {
    assert!(matches!(
        decode_frame::<Value>(&[0, 0, 0, 0]),
        Err(CodecError::ZeroLength)
    ));
    assert!(matches!(
        decode_frame::<Value>(&[0, 0, 0, 4, b'{']),
        Err(CodecError::Truncated)
    ));
    assert!(matches!(
        decode_frame::<Value>(&[0, 16, 0, 1]),
        Err(CodecError::FrameTooLarge)
    ));
    assert!(matches!(
        decode_frame::<Value>(&[0, 0, 0, 1, 0xff]),
        Err(CodecError::InvalidUtf8)
    ));
    let mut trailing = framed(json!({}));
    trailing.push(b'x');
    assert!(matches!(
        decode_frame::<Value>(&trailing),
        Err(CodecError::TrailingData)
    ));
    let malformed = [0, 0, 0, 1, b'{'];
    assert!(matches!(
        decode_frame::<Value>(&malformed),
        Err(CodecError::InvalidJson)
    ));
    let mut nested = json!(null);
    for _ in 0..MAX_JSON_DEPTH {
        nested = json!([nested]);
    }
    assert!(matches!(
        decode_frame::<Value>(&framed(nested)),
        Err(CodecError::LimitExceeded)
    ));
    assert!(matches!(
        decode_frame::<Value>(&framed(json!({"x":"x".repeat(MAX_STRING_BYTES+1)}))),
        Err(CodecError::LimitExceeded)
    ));
    assert!(matches!(
        decode_frame::<Value>(&framed(json!(
            (0..=MAX_COLLECTION_ITEMS).collect::<Vec<_>>()
        ))),
        Err(CodecError::LimitExceeded)
    ));
    assert!(decode_request(&framed(request("run.open", json!({"run_id":"not-a-uuid"})))).is_err());
}

#[test]
fn artifact_data_exception_is_exact_and_length_checked() {
    let oversized = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        vec![0; MAX_ARTIFACT_CHUNK_BYTES + 1],
    );
    let event = json!({"protocol":PROTOCOL,"subscription_id":ID,"event":"artifact.chunk","body":{"artifact_id":ID,"chunk_index":0,"offset":0,"byte_length":MAX_ARTIFACT_CHUNK_BYTES+1,"chunk_sha256":"a".repeat(64),"data":oversized}});
    assert!(matches!(
        decode_event(&framed(event)),
        Err(CodecError::ArtifactChunkTooLarge)
    ));
    let wrong = json!({"protocol":PROTOCOL,"subscription_id":ID,"event":"artifact.chunk","body":{"artifact_id":ID,"chunk_index":0,"offset":0,"byte_length":4,"chunk_sha256":"a".repeat(64),"data":"YWJj"}});
    assert!(matches!(
        decode_event(&framed(wrong)),
        Err(CodecError::InvalidJson)
    ));
    let nested_data = json!({"data":{"data":"x".repeat(MAX_STRING_BYTES+1)}});
    assert!(matches!(
        decode_frame::<Value>(&framed(nested_data)),
        Err(CodecError::LimitExceeded)
    ));
}

#[test]
fn negotiation_is_minimal_and_actionable() {
    assert_eq!(
        negotiate_version(VersionRange { min: 1, max: 2 }).unwrap(),
        1
    );
    assert_eq!(
        negotiate_version(VersionRange { min: 0, max: 0 })
            .unwrap_err()
            .action,
        Some(ErrorAction::UpgradeCompanion)
    );
    let new = negotiate_version(VersionRange { min: 2, max: 3 }).unwrap_err();
    assert_eq!(new.action, Some(ErrorAction::UpgradeDesktop));
    let encoded = serde_json::to_string(&new).unwrap();
    let (_, incompatible) = golden_frames()
        .into_iter()
        .find(|(name, _)| *name == "incompatible")
        .unwrap();
    let golden: ErrorEnvelope = decode_error(&incompatible).unwrap();
    assert_eq!(
        golden.error,
        negotiate_version(VersionRange { min: 0, max: 0 }).unwrap_err()
    );
    for secret in ["profile", "session", "workspace", "entitlement", "runtime"] {
        assert!(!encoded.contains(secret));
    }
}
